//! Read-only Windows PE import closure. No DLL execution or host PATH lookup.
//! Layout follows https://learn.microsoft.com/windows/win32/debug/pe-format.
use anyhow::{bail, ensure, Context, Result};
use std::{collections::BTreeMap, fs, io::Read, path::Path};

pub(crate) struct ImageImports {
    pub machine: u16,
    pub imports: Vec<String>,
}

pub(crate) fn inspect_imports(path: &Path) -> Result<ImageImports> {
    crate::package::ordinary_file(path)?;
    ensure!(
        fs::metadata(path)?.len() <= 512 * 1024 * 1024,
        "PE image exceeds inspection limit: {}",
        path.display()
    );
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(512 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    inspect_bytes(&bytes).with_context(|| format!("inspect PE imports {}", path.display()))
}

fn span(bytes: &[u8], offset: usize, length: usize) -> Result<&[u8]> {
    bytes
        .get(offset..offset.checked_add(length).context("PE range overflow")?)
        .context("truncated PE range")
}
fn u16_at(bytes: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(span(bytes, offset, 2)?.try_into()?))
}
fn u32_at(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(span(bytes, offset, 4)?.try_into()?))
}

struct Section {
    address: u32,
    raw: u32,
    size: u32,
}
struct Image<'a> {
    bytes: &'a [u8],
    headers: u32,
    sections: Vec<Section>,
    pointer_width: usize,
}
impl Image<'_> {
    fn offset(&self, address: u32, length: usize) -> Result<usize> {
        let length = u32::try_from(length)?;
        let end = address.checked_add(length).context("PE RVA overflow")?;
        if end <= self.headers {
            span(self.bytes, address as usize, length as usize)?;
            return Ok(address as usize);
        }
        let mut found = None;
        for section in &self.sections {
            if let Some(delta) = address.checked_sub(section.address) {
                if delta
                    .checked_add(length)
                    .is_some_and(|end| end <= section.size)
                {
                    ensure!(found.is_none(), "ambiguous overlapping PE sections");
                    let offset = section
                        .raw
                        .checked_add(delta)
                        .context("PE file offset overflow")?
                        as usize;
                    span(self.bytes, offset, length as usize)?;
                    found = Some(offset);
                }
            }
        }
        found.context("PE RVA has no backed file range")
    }
    fn name(&self, address: u32) -> Result<String> {
        let mut name = Vec::new();
        for index in 0..=255u32 {
            let byte = self.bytes
                [self.offset(address.checked_add(index).context("PE name overflow")?, 1)?];
            if byte == 0 {
                ensure!(!name.is_empty(), "empty PE import name");
                let name = String::from_utf8(name)?.to_ascii_lowercase();
                ensure!(
                    name.ends_with(".dll") || name == "winspool.drv",
                    "unsupported PE import basename {name}"
                );
                return Ok(name);
            }
            ensure!(
                byte.is_ascii_alphanumeric() || b"_.-".contains(&byte),
                "unsafe PE import basename"
            );
            name.push(byte);
        }
        bail!("unterminated or overlong PE import name")
    }
    fn directory(
        &self,
        address: u32,
        size: u32,
        delay: bool,
        imports: &mut Vec<String>,
    ) -> Result<()> {
        if address == 0 && size == 0 {
            return Ok(());
        }
        let width = if delay { 32 } else { 20 };
        ensure!(
            address != 0 && size >= width && size <= 128 * 1024,
            "invalid PE import directory range"
        );
        let start = self.offset(address, size as usize)?;
        for descriptor in span(self.bytes, start, size as usize)?.chunks_exact(width as usize) {
            if descriptor.iter().all(|byte| *byte == 0) {
                return Ok(());
            }
            let (name, address_table) = if delay {
                ensure!(
                    u32_at(descriptor, 0)? == 1,
                    "unsupported PE delay import address mode"
                );
                (u32_at(descriptor, 4)?, u32_at(descriptor, 12)?)
            } else {
                (u32_at(descriptor, 12)?, u32_at(descriptor, 16)?)
            };
            ensure!(address_table != 0, "PE import lacks address table");
            self.offset(address_table, self.pointer_width)?;
            ensure!(name != 0, "PE import lacks DLL name");
            imports.push(self.name(name)?);
        }
        bail!("unterminated PE import descriptor table")
    }
}

pub(crate) fn inspect_bytes(bytes: &[u8]) -> Result<ImageImports> {
    ensure!(
        bytes.len() <= 512 * 1024 * 1024,
        "PE image exceeds inspection limit"
    );
    ensure!(span(bytes, 0, 2)? == b"MZ", "missing DOS signature");
    let pe = u32_at(bytes, 0x3c)? as usize;
    ensure!(span(bytes, pe, 4)? == b"PE\0\0", "missing PE signature");
    let coff = pe.checked_add(4).context("PE header overflow")?;
    let machine = u16_at(bytes, coff)?;
    ensure!(
        matches!(machine, 0x14c | 0x8664 | 0xaa64),
        "unsupported PE machine {machine:#x}"
    );
    let count = u16_at(bytes, coff + 2)? as usize;
    ensure!((1..=96).contains(&count), "invalid PE section count");
    let optional = coff + 20;
    let optional_size = u16_at(bytes, coff + 16)? as usize;
    let header = span(bytes, optional, optional_size)?;
    let (directories, number, pointer_width) = match u16_at(header, 0)? {
        0x10b if machine == 0x14c => (96, 92, 4),
        0x20b if machine != 0x14c => (112, 108, 8),
        _ => bail!("unsupported PE optional header"),
    };
    let directory_count = u32_at(header, number)? as usize;
    ensure!(directory_count <= 16, "unsupported PE data directory count");
    span(header, directories, directory_count * 8)?;
    let headers = u32_at(header, 60)?;
    ensure!(headers as usize <= bytes.len(), "truncated PE headers");
    let table = optional + optional_size;
    ensure!(
        table + count * 40 <= headers as usize,
        "PE section table exceeds declared headers"
    );
    let mut sections = Vec::with_capacity(count);
    for entry in span(bytes, table, count * 40)?.as_chunks::<40>().0 {
        let section = Section {
            address: u32_at(entry, 12)?,
            raw: u32_at(entry, 20)?,
            size: u32_at(entry, 16)?,
        };
        span(bytes, section.raw as usize, section.size as usize)?;
        sections.push(section);
    }
    let image = Image {
        bytes,
        headers,
        sections,
        pointer_width,
    };
    let mut imports = Vec::new();
    for (index, delay) in [(1, false), (13, true)] {
        if index < directory_count {
            let entry = directories + index * 8;
            image.directory(
                u32_at(header, entry)?,
                u32_at(header, entry + 4)?,
                delay,
                &mut imports,
            )?;
        }
    }
    imports.sort();
    imports.dedup();
    Ok(ImageImports { machine, imports })
}

/// Deliberate Windows/API-set and centrally serviced VC v14 prerequisites.
/// Arbitrary DLLs found in System32, an SDK or PATH are never exemptions.
pub(crate) fn is_system_import(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ((name.starts_with("api-ms-win-") || name.starts_with("ext-ms-win-")) && name.ends_with(".dll"))
        || matches!(
            name.as_str(),
            "kernel32.dll"
                | "kernelbase.dll"
                | "ntdll.dll"
                | "advapi32.dll"
                | "user32.dll"
                | "gdi32.dll"
                | "gdi32full.dll"
                | "comdlg32.dll"
                | "comctl32.dll"
                | "coremessaging.dll"
                | "combase.dll"
                | "uiautomationcore.dll"
                | "shell32.dll"
                | "shlwapi.dll"
                | "shcore.dll"
                | "ole32.dll"
                | "oleaut32.dll"
                | "uuid.dll"
                | "rpcrt4.dll"
                | "bcrypt.dll"
                | "bcryptprimitives.dll"
                | "crypt32.dll"
                | "secur32.dll"
                | "security.dll"
                | "ws2_32.dll"
                | "iphlpapi.dll"
                | "dnsapi.dll"
                | "winhttp.dll"
                | "wininet.dll"
                | "version.dll"
                | "winmm.dll"
                | "winspool.drv"
                | "imm32.dll"
                | "uxtheme.dll"
                | "dwmapi.dll"
                | "dcomp.dll"
                | "dwrite.dll"
                | "d2d1.dll"
                | "dxgi.dll"
                | "d3d11.dll"
                | "d3d12.dll"
                | "d3dcompiler_47.dll"
                | "opengl32.dll"
                | "glu32.dll"
                | "setupapi.dll"
                | "cfgmgr32.dll"
                | "hid.dll"
                | "winusb.dll"
                | "powrprof.dll"
                | "userenv.dll"
                | "wtsapi32.dll"
                | "netapi32.dll"
                | "psapi.dll"
                | "dbghelp.dll"
                | "imagehlp.dll"
                | "pdh.dll"
                | "msimg32.dll"
                | "propsys.dll"
                | "wintrust.dll"
                | "msvcrt.dll"
                | "ucrtbase.dll"
                | "vcruntime140.dll"
                | "vcruntime140_1.dll"
                | "msvcp140.dll"
                | "msvcp140_1.dll"
                | "msvcp140_2.dll"
                | "msvcp140_atomic_wait.dll"
                | "msvcp140_codecvt_ids.dll"
                | "concrt140.dll"
                | "vcomp140.dll"
        )
}

pub(crate) fn dependency_port(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().as_str() {
        "freetype.dll" => Some("freetype"),
        "z.dll" => Some("zlib"),
        "bz2.dll" => Some("bzip2"),
        "libpng16.dll" => Some("libpng"),
        "brotlidec.dll" | "brotlicommon.dll" => Some("brotli"),
        _ => None,
    }
}

pub(crate) fn verify_directory(directory: &Path) -> Result<()> {
    crate::package::ordinary_directory(directory)?;
    let mut images = BTreeMap::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("dll") || extension.eq_ignore_ascii_case("exe")
        }) {
            let name = path
                .file_name()
                .context("PE image basename")?
                .to_str()
                .context("non-UTF8 PE image basename")?
                .to_ascii_lowercase();
            ensure!(
                !images.contains_key(&name),
                "conflicting case-insensitive runtime name {name}"
            );
            images.insert(name, inspect_imports(&path)?);
        }
    }
    ensure!(
        !images.is_empty(),
        "runtime directory contains no PE images"
    );
    let machine = images.values().next().unwrap().machine;
    for (name, image) in &images {
        ensure!(
            image.machine == machine,
            "mixed PE machine in runtime: {name}"
        );
        for import in &image.imports {
            ensure!(
                is_system_import(import) || images.contains_key(import),
                "runtime {name} requires missing packaged dependency {import}"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn fixture_image(machine: u16, imports: &[&str]) -> Vec<u8> {
    assert!(imports.len() <= 16);
    let mut bytes = vec![0; 0x1200];
    fn word(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn dword(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[..2].copy_from_slice(b"MZ");
    dword(&mut bytes, 0x3c, 0x80);
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    word(&mut bytes, 0x84, machine);
    word(&mut bytes, 0x86, 1);
    let pe32 = machine == 0x14c;
    let directories = if pe32 { 96 } else { 112 };
    let section = if pe32 { 0x178 } else { 0x188 };
    word(&mut bytes, 0x94, if pe32 { 0xe0 } else { 0xf0 });
    word(&mut bytes, 0x96, 0x22);
    word(&mut bytes, 0x98, if pe32 { 0x10b } else { 0x20b });
    dword(&mut bytes, 0x98 + 32, 0x1000);
    dword(&mut bytes, 0x98 + 36, 0x200);
    dword(&mut bytes, 0x98 + 56, 0x2000);
    dword(&mut bytes, 0x98 + 60, 0x200);
    dword(&mut bytes, 0x98 + directories - 4, 16);
    if !imports.is_empty() {
        dword(&mut bytes, 0x98 + directories + 8, 0x1000);
        dword(
            &mut bytes,
            0x98 + directories + 12,
            ((imports.len() + 1) * 20) as u32,
        );
    }
    bytes[section..section + 8].copy_from_slice(b".idata\0\0");
    dword(&mut bytes, section + 8, 0x1000);
    dword(&mut bytes, section + 12, 0x1000);
    dword(&mut bytes, section + 16, 0x1000);
    dword(&mut bytes, section + 20, 0x200);
    let mut name_offset = 0x800;
    for (index, name) in imports.iter().enumerate() {
        let descriptor = 0x200 + index * 20;
        dword(&mut bytes, descriptor, 0x1300 + (index * 32) as u32);
        dword(
            &mut bytes,
            descriptor + 12,
            0x1000 + (name_offset - 0x200) as u32,
        );
        dword(&mut bytes, descriptor + 16, 0x1300 + (index * 32) as u32);
        if pe32 {
            dword(&mut bytes, 0x500 + index * 32, 0x80000001);
        } else {
            bytes[0x500 + index * 32..0x508 + index * 32]
                .copy_from_slice(&0x8000000000000001u64.to_le_bytes());
        }
        bytes[name_offset..name_offset + name.len()].copy_from_slice(name.as_bytes());
        name_offset += name.len() + 1;
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packaged_pe_closure_rejects_missing_transitive_and_mixed_architecture() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path();
        fs::write(
            directory.join("Limo-CAD.exe"),
            fixture_image(
                0x8664,
                &[
                    "TKMath.dll",
                    "coremessaging.dll",
                    "combase.dll",
                    "uiautomationcore.dll",
                ],
            ),
        )
        .unwrap();
        fs::write(
            directory.join("TKMath.dll"),
            fixture_image(
                0x8664,
                &["freetype.dll", "KERNEL32.dll", "VCRUNTIME140.dll"],
            ),
        )
        .unwrap();
        assert!(verify_directory(directory)
            .unwrap_err()
            .to_string()
            .contains("freetype.dll"));
        fs::write(directory.join("FreeType.DLL"), fixture_image(0xaa64, &[])).unwrap();
        assert!(verify_directory(directory)
            .unwrap_err()
            .to_string()
            .contains("mixed PE machine"));
        fs::write(
            directory.join("FreeType.DLL"),
            fixture_image(0x8664, &["api-ms-win-crt-runtime-l1-1-0.dll"]),
        )
        .unwrap();
        verify_directory(directory).unwrap();
        assert!(!is_system_import("freetype.dll"));
        assert!(!is_system_import("vcruntime140d.dll"));
    }
    #[test]
    fn pe_closure_accepts_cycles_and_rejects_conflicting_case_names() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("A.dll"), fixture_image(0x8664, &["b.DLL"])).unwrap();
        fs::write(temp.path().join("b.dll"), fixture_image(0x8664, &["a.DLL"])).unwrap();
        verify_directory(temp.path()).unwrap();
        // On case-insensitive Windows the alias is the same file; Unix can
        // actually represent the conflicting directory that must be rejected.
        #[cfg(not(windows))]
        {
            fs::write(temp.path().join("a.DLL"), fixture_image(0x8664, &[])).unwrap();
            assert!(verify_directory(temp.path())
                .unwrap_err()
                .to_string()
                .contains("conflicting case-insensitive"));
        }
    }
    #[test]
    fn pe_import_ranges_names_and_delay_descriptors_fail_closed() {
        let valid = fixture_image(0x8664, &["freetype.dll"]);
        assert_eq!(inspect_bytes(&valid).unwrap().imports, ["freetype.dll"]);
        assert_eq!(
            inspect_bytes(&fixture_image(0x14c, &["freetype.dll"]))
                .unwrap()
                .imports,
            ["freetype.dll"]
        );
        let mut wrong_magic = valid.clone();
        wrong_magic[0x98..0x9a].copy_from_slice(&0x10bu16.to_le_bytes());
        assert!(inspect_bytes(&wrong_magic).is_err());
        for length in [0, 63, 0x84, 0x18f, 0x220] {
            assert!(inspect_bytes(&valid[..length]).is_err());
        }
        let mut malformed = valid.clone();
        malformed[0x20c..0x210].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(inspect_bytes(&malformed).is_err());
        let mut short_headers = valid.clone();
        short_headers[0x98 + 60..0x98 + 64].copy_from_slice(&0x100u32.to_le_bytes());
        assert!(inspect_bytes(&short_headers).is_err());
        let mut unsafe_name = valid.clone();
        unsafe_name[0x800] = b'/';
        assert!(inspect_bytes(&unsafe_name).is_err());
        let mut unterminated = valid.clone();
        unterminated[0x98 + 112 + 12..0x98 + 112 + 16].copy_from_slice(&20u32.to_le_bytes());
        assert!(inspect_bytes(&unterminated).is_err());
        let mut delay = valid;
        delay[0x98 + 112 + 13 * 8..0x98 + 112 + 13 * 8 + 4]
            .copy_from_slice(&0x1100u32.to_le_bytes());
        delay[0x98 + 112 + 13 * 8 + 4..0x98 + 112 + 13 * 8 + 8]
            .copy_from_slice(&64u32.to_le_bytes());
        delay[0x300..0x304].copy_from_slice(&1u32.to_le_bytes());
        delay[0x304..0x308].copy_from_slice(&0x1700u32.to_le_bytes());
        delay[0x30c..0x310].copy_from_slice(&0x1300u32.to_le_bytes());
        delay[0x900..0x90c].copy_from_slice(b"missing.dll\0");
        assert_eq!(
            inspect_bytes(&delay).unwrap().imports,
            ["freetype.dll", "missing.dll"]
        );
        let mut missing_delay_table = delay.clone();
        missing_delay_table[0x30c..0x310].fill(0);
        assert!(inspect_bytes(&missing_delay_table).is_err());
        delay[0x300..0x304].copy_from_slice(&0u32.to_le_bytes());
        assert!(inspect_bytes(&delay).is_err());
    }
}
