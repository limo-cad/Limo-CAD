use crate::{error, failed, ProjectFileError, MAX_EXPANDED_BYTES, MAX_FILE_BYTES};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

pub const PROJECT_EXTENSION: &str = ".limo";
pub const PROJECT_FORMAT: &str = "limo-cad-project";
pub const LEGACY_FORMAT: &str = "tfcad-project";
pub const PREVIOUS_FORMAT: &str = "nbcad-project";
pub const CONTAINER_VERSION: u64 = 1;

/// Clock and build identity belong to the host. Explicit metadata also makes
/// reference archives reproducible without installing another clock library.
pub struct SaveMetadata<'a> {
    pub application_version: &'a str,
    pub saved_at: &'a str,
}

/// The JSON model remains authoritative and opaque to this container layer.
/// Keep the opened container to preserve ancillary images, recipes and future
/// extension entries on native re-save without inflating/recompressing them.
pub struct ProjectArchive {
    manifest: Value,
    model_json: String,
    original: Option<Vec<u8>>,
    attachments: BTreeMap<String, Option<Vec<u8>>>,
}

#[derive(Deserialize)]
struct ModelHeader {
    format: Option<String>,
    schema_version: Option<Value>,
}

impl ProjectArchive {
    pub fn new(model_json: String, metadata: SaveMetadata<'_>) -> Result<Self, ProjectFileError> {
        let mut archive = Self {
            manifest: json!({}),
            model_json: String::new(),
            original: None,
            attachments: BTreeMap::new(),
        };
        archive.update_model(model_json, metadata)?;
        Ok(archive)
    }

    pub fn manifest(&self) -> &Value {
        &self.manifest
    }
    pub fn model_json(&self) -> &str {
        &self.model_json
    }

    /// Adopt only a successful engine export. Unknown manifest properties and
    /// all ancillary entry payloads remain attached to this opened document.
    pub fn update_model(
        &mut self,
        model_json: String,
        metadata: SaveMetadata<'_>,
    ) -> Result<(), ProjectFileError> {
        let header: ModelHeader = serde_json::from_str(&model_json)
            .map_err(|source| failed("the engine produced an invalid project model", source))?;
        if header.format.as_deref() != Some(PROJECT_FORMAT)
            || !header.schema_version.as_ref().is_some_and(integer)
        {
            return Err(error("the engine produced an invalid project model"));
        }
        let mut manifest = self.manifest.clone();
        manifest["format"] = json!(PROJECT_FORMAT);
        manifest["container_version"] = json!(CONTAINER_VERSION);
        manifest["model"] = json!("model.json");
        manifest["model_schema_version"] = header.schema_version.unwrap();
        manifest["application"] = json!("Limo CAD");
        manifest["application_version"] = json!(metadata.application_version);
        manifest["saved_at"] = json!(metadata.saved_at);
        self.manifest = manifest;
        self.model_json = if model_json.ends_with('\n') {
            model_json
        } else {
            model_json + "\n"
        };
        Ok(())
    }

    pub fn decode(bytes: Vec<u8>) -> Result<Self, ProjectFileError> {
        Self::decode_bounded(bytes, MAX_FILE_BYTES, MAX_EXPANDED_BYTES)
    }

    fn decode_bounded(
        bytes: Vec<u8>,
        archive_limit: u64,
        expanded_limit: u64,
    ) -> Result<Self, ProjectFileError> {
        if bytes.len() < 4 || bytes.len() as u64 > archive_limit {
            return Err(error(
                "the project archive is empty or exceeds the 256 MB safety limit",
            ));
        }
        if !bytes.starts_with(b"PK") {
            return Err(error("this is not a ZIP-based .limo project"));
        }
        let mut archive = ZipArchive::new(Cursor::new(&bytes))
            .map_err(|source| failed("the .limo ZIP is damaged", source))?;
        let mut expanded = 0_u64;
        for index in 0..archive.len() {
            let entry = archive
                .by_index_raw(index)
                .map_err(|source| failed("could not read project entry", source))?;
            expanded = expanded
                .checked_add(entry.size())
                .ok_or_else(|| error("expanded project size overflow"))?;
            if expanded > expanded_limit {
                return Err(error("expanded project exceeds the 512 MB safety limit"));
            }
        }
        let manifest_json = read_entry(&mut archive, "manifest.json", expanded_limit)?;
        let manifest: Value = serde_json::from_str(&manifest_json)
            .map_err(|source| failed("manifest.json is not valid JSON", source))?;
        if manifest["container_version"].as_f64() != Some(CONTAINER_VERSION as f64)
            || manifest["model"] != "model.json"
        {
            return Err(error("this .limo container version is not supported"));
        }
        if !matches!(
            manifest["format"].as_str(),
            Some(PROJECT_FORMAT | LEGACY_FORMAT | PREVIOUS_FORMAT)
        ) {
            return Err(error("unsupported project format"));
        }
        let model_json = read_entry(&mut archive, "model.json", expanded_limit)?;
        if let Some(schema) = manifest.get("model_schema_version") {
            let header: ModelHeader = serde_json::from_str(&model_json)
                .map_err(|source| failed("model.json is not valid JSON", source))?;
            if !integer(schema)
                || schema.as_f64() != header.schema_version.as_ref().and_then(Value::as_f64)
            {
                return Err(error(
                    "the project manifest and model schema versions do not match",
                ));
            }
        }
        drop(archive);
        Ok(Self {
            manifest,
            model_json,
            original: Some(bytes),
            attachments: BTreeMap::new(),
        })
    }

    /// Read an owned domain intent without extracting files. The caller supplies
    /// its domain byte limit; CRC/decompression errors are not treated as absence.
    pub fn read_intent(
        &self,
        name: &str,
        limit: usize,
    ) -> Result<Option<Vec<u8>>, ProjectFileError> {
        validate_intent_name(name)?;
        let limit = (limit as u64).min(MAX_EXPANDED_BYTES);
        if let Some(staged) = self.attachments.get(name) {
            if staged.as_ref().is_some_and(|b| b.len() as u64 > limit) {
                return Err(error("project intent exceeds caller limit"));
            }
            return Ok(staged.clone());
        }
        let Some(original) = &self.original else {
            return Ok(None);
        };
        let mut zip = ZipArchive::new(Cursor::new(original))
            .map_err(|e| failed("could not read project intents", e))?;
        let entry = match zip.by_name(name) {
            Ok(entry) => entry,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(e) => return Err(failed("could not read project intent", e)),
        };
        if entry.is_dir() || entry.is_symlink() || entry.encrypted() || entry.size() > limit {
            return Err(error("invalid or excessive project intent"));
        }
        let mut bytes = Vec::new();
        entry
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| failed("could not decode project intent", e))?;
        if bytes.len() as u64 > limit {
            return Err(error("project intent exceeds caller limit"));
        }
        Ok(Some(bytes))
    }

    /// Stage an explicitly owned intent replacement/removal. Other entries are
    /// retained by encode. Decode domain content before calling this method.
    /// This changes only the in-memory candidate; disk writes remain atomic at
    /// the existing file boundary. Core manifest/model names are never accepted.
    pub fn set_intent(
        &mut self,
        name: &str,
        bytes: Option<Vec<u8>>,
    ) -> Result<(), ProjectFileError> {
        validate_intent_name(name)?;
        if !self.attachments.contains_key(name) && self.attachments.len() >= 64 {
            return Err(error("too many staged project intents"));
        }
        let total = self
            .attachments
            .iter()
            .filter(|(key, _)| key.as_str() != name)
            .map(|(_, value)| value.as_ref().map_or(0, |b| b.len() as u64))
            .sum::<u64>()
            + bytes.as_ref().map_or(0, |b| b.len() as u64);
        if total > MAX_EXPANDED_BYTES {
            return Err(error("staged project intents exceed size limit"));
        }
        self.attachments.insert(name.to_owned(), bytes);
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, ProjectFileError> {
        self.encode_bounded(MAX_FILE_BYTES, MAX_EXPANDED_BYTES)
    }

    fn encode_bounded(
        &self,
        archive_limit: u64,
        expanded_limit: u64,
    ) -> Result<Vec<u8>, ProjectFileError> {
        let mut manifest = serde_json::to_vec_pretty(&self.manifest)
            .map_err(|source| failed("could not encode project manifest", source))?;
        manifest.push(b'\n');
        let mut expanded = manifest.len() as u64 + self.model_json.len() as u64;
        if expanded > expanded_limit {
            return Err(error("expanded project exceeds the 512 MB safety limit"));
        }
        let mut writer = ZipWriter::new(BoundedOutput {
            cursor: Cursor::new(Vec::new()),
            limit: archive_limit,
        });
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(6));
        writer
            .start_file("manifest.json", options)
            .map_err(|source| failed("could not create project manifest", source))?;
        writer
            .write_all(&manifest)
            .map_err(|source| failed("could not write project manifest", source))?;
        writer
            .start_file("model.json", options)
            .map_err(|source| failed("could not create project model", source))?;
        writer
            .write_all(self.model_json.as_bytes())
            .map_err(|source| failed("could not write project model", source))?;
        if let Some(original) = &self.original {
            let mut source = ZipArchive::new(Cursor::new(original))
                .map_err(|source| failed("could not reopen original project entries", source))?;
            writer
                .set_raw_comment(source.comment().into())
                .map_err(|source| failed("could not preserve project ZIP comment", source))?;
            if let Some(data) = source.raw_zip64_extensible_data_sector() {
                writer.set_raw_zip64_extensible_data_sector(data.into());
            }
            for index in 0..source.len() {
                let entry = source
                    .by_index_raw(index)
                    .map_err(|source| failed("could not read original project entry", source))?;
                let name = entry
                    .name()
                    .map_err(|source| {
                        failed(
                            "cannot preserve project entry name; keep the original project archive",
                            source,
                        )
                    })?
                    .into_owned();
                if entry.name_raw() != name.as_bytes() {
                    return Err(error("cannot preserve project entry name without changing its encoding; keep the original project archive"));
                }
                if !entry.comment().is_empty()
                    || entry.extra_data_fields().next().is_some()
                    // ZIP 9 only parses central-directory extra fields. Do not
                    // discard an opaque extension present only in the local header.
                    || entry.data_start().and_then(|start| start.checked_sub(entry.header_start()))
                        != Some(30 + entry.name_raw().len() as u64)
                    || entry.encrypted()
                    || entry.is_symlink()
                {
                    return Err(error(format!("cannot preserve opaque ZIP metadata for entry '{name}'; keep the original project archive")));
                }
                if matches!(name.as_str(), "manifest.json" | "model.json")
                    || self.attachments.contains_key(&name)
                {
                    continue;
                }
                expanded = expanded
                    .checked_add(entry.size())
                    .ok_or_else(|| error("expanded project size overflow"))?;
                if expanded > expanded_limit {
                    return Err(error("expanded project exceeds the 512 MB safety limit"));
                }
                if entry.is_dir() {
                    if entry.size() != 0 {
                        return Err(error(
                            "cannot preserve a project directory entry carrying file content",
                        ));
                    }
                    writer
                        .add_directory(name, entry.options())
                        .map_err(|source| {
                            failed("could not retain original project directory", source)
                        })?;
                } else {
                    writer.raw_copy_file(entry).map_err(|source| {
                        failed("could not retain original project entry", source)
                    })?;
                }
            }
        }
        for (name, bytes) in &self.attachments {
            let Some(bytes) = bytes else { continue };
            expanded = expanded
                .checked_add(bytes.len() as u64)
                .ok_or_else(|| error("expanded project size overflow"))?;
            if expanded > expanded_limit {
                return Err(error("expanded project exceeds the 512 MB safety limit"));
            }
            writer
                .start_file(name, options)
                .map_err(|source| failed("could not create project intent", source))?;
            writer
                .write_all(bytes)
                .map_err(|source| failed("could not write project intent", source))?;
        }
        let output = writer
            .finish()
            .map_err(|source| failed("could not finish project archive", source))?;
        Ok(output.cursor.into_inner())
    }
}

fn validate_intent_name(name: &str) -> Result<(), ProjectFileError> {
    if !name.starts_with("intent/")
        || name.len() > 256
        || name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_-.".contains(&b))
    {
        return Err(error(
            "project intent requires a safe relative name under intent/",
        ));
    }
    Ok(())
}

fn integer(value: &Value) -> bool {
    value
        .as_f64()
        .is_some_and(|number| number.is_finite() && number.fract() == 0.)
}

fn read_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
    limit: u64,
) -> Result<String, ProjectFileError> {
    let entry = archive.by_name(name).map_err(|source| {
        failed(
            "the .limo archive must contain manifest.json and model.json",
            source,
        )
    })?;
    if entry.size() > limit {
        return Err(error("expanded project exceeds the 512 MB safety limit"));
    }
    let mut bytes = Vec::new();
    entry
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| failed("could not read project JSON", source))?;
    if bytes.len() as u64 > limit {
        return Err(error("expanded project exceeds the 512 MB safety limit"));
    }
    String::from_utf8(bytes).map_err(|source| failed("project JSON is not valid UTF-8", source))
}

struct BoundedOutput {
    cursor: Cursor<Vec<u8>>,
    limit: u64,
}
impl Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self
            .cursor
            .position()
            .checked_add(bytes.len() as u64)
            .is_none_or(|end| end > self.limit)
        {
            return Err(io::Error::other(
                "project archive exceeds the 256 MB safety limit",
            ));
        }
        self.cursor.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.cursor.flush()
    }
}
impl Seek for BoundedOutput {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let position = self.cursor.seek(from)?;
        if position > self.limit {
            return Err(io::Error::other(
                "project archive position exceeds safety limit",
            ));
        }
        Ok(position)
    }
}

#[cfg(test)]
mod tests;
