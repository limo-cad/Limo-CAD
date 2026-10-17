//! Portable replay envelope. This codec validates framing and byte limits only.
//! Hosts must parse script v2 and validate archive/domain semantics before mutation.
use crate::{error, ProjectArchive, ProjectFileError, MAX_FILE_BYTES};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_REPLAY_SCRIPT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_REPLAY_PACKAGE_BYTES: usize =
    MAX_FILE_BYTES as usize + MAX_REPLAY_SCRIPT_BYTES + 65536;
const FORMAT: &str = "limo-cad-replay";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayPackage {
    script: String,
    project: Vec<u8>,
}

/// Identity of the original wire bytes, not a re-encoded package. Integrity
/// only: callers must still enforce admission, ownership and execution policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayPackageIdentity {
    pub byte_length: u64,
    pub sha256: [u8; 32],
}
impl ReplayPackageIdentity {
    fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            byte_length: bytes.len() as u64,
            sha256: Sha256::digest(bytes).into(),
        }
    }
    fn validate_length(&self) -> Result<(), ProjectFileError> {
        if self.byte_length == 0 || self.byte_length > MAX_REPLAY_PACKAGE_BYTES as u64 {
            return Err(error("invalid replay package identity length"));
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct Wire<'a> {
    format: &'a str,
    version: u32,
    script: &'a str,
    #[serde(serialize_with = "serialize_bytes")]
    project: &'a [u8],
}
fn serialize_bytes<S: serde::Serializer>(value: &&[u8], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_bytes(value)
}

impl ReplayPackage {
    /// Read within the package-specific bound, then validate the complete CBOR
    /// envelope. Reading does not execute scripts or change a live document.
    pub fn read_file(path: &std::path::Path) -> Result<Self, ProjectFileError> {
        let bytes = crate::files::read_bounded(path, MAX_REPLAY_PACKAGE_BYTES as u64)?;
        Self::decode_cbor(&bytes)
    }
    /// Select a package and retain the identity of the exact bounded bytes decoded.
    pub fn read_file_with_identity(
        path: &std::path::Path,
    ) -> Result<(Self, ReplayPackageIdentity), ProjectFileError> {
        let bytes = crate::files::read_bounded(path, MAX_REPLAY_PACKAGE_BYTES as u64)?;
        let package = Self::decode_cbor(&bytes)?;
        Ok((package, ReplayPackageIdentity::from_bytes(&bytes)))
    }
    /// Reopen a selected package, verify its identity and decode the same buffer.
    /// No second read or re-encoding may occur between verification and decoding.
    pub fn read_file_verified(
        path: &std::path::Path,
        expected: &ReplayPackageIdentity,
    ) -> Result<Self, ProjectFileError> {
        expected.validate_length()?;
        let bytes = crate::files::read_bounded(path, expected.byte_length)?;
        if bytes.len() as u64 != expected.byte_length {
            return Err(error("replay package length changed after selection"));
        }
        if ReplayPackageIdentity::from_bytes(&bytes) != *expected {
            return Err(error("replay package contents changed after selection"));
        }
        Self::decode_cbor(&bytes)
    }
    /// Encode fully before touching the destination, then publish atomically.
    /// Callers own destination selection and replacement authorization.
    pub fn write_file_atomic(&self, path: &std::path::Path) -> Result<(), ProjectFileError> {
        crate::files::write_bounded_mode(
            path,
            &self.encode_cbor()?,
            MAX_REPLAY_PACKAGE_BYTES as u64,
            true,
        )
    }
    /// Create a completed package without overwriting any existing destination.
    pub fn write_file_new(&self, path: &std::path::Path) -> Result<(), ProjectFileError> {
        crate::files::write_bounded_mode(
            path,
            &self.encode_cbor()?,
            MAX_REPLAY_PACKAGE_BYTES as u64,
            false,
        )
    }
    pub fn new(script: String, project: Vec<u8>) -> Result<Self, ProjectFileError> {
        check_lengths(script.len(), project.len())?;
        Ok(Self { script, project })
    }
    pub fn script(&self) -> &str {
        &self.script
    }
    /// Exact original archive bytes, including ancillary entries and metadata.
    pub fn project_bytes(&self) -> &[u8] {
        &self.project
    }
    /// Decode the retained archive without extraction or re-encoding. This is
    /// still not script validation, domain validation or permission to execute.
    pub fn into_project(self) -> Result<(String, ProjectArchive), ProjectFileError> {
        Ok((self.script, ProjectArchive::decode(self.project)?))
    }
    pub fn encode_cbor(&self) -> Result<Vec<u8>, ProjectFileError> {
        let mut output = Vec::new();
        ciborium::into_writer(
            &Wire {
                format: FORMAT,
                version: 1,
                script: &self.script,
                project: &self.project,
            },
            &mut output,
        )
        .map_err(|e| error(format!("could not encode replay package: {e}")))?;
        // All fields are immutable and bounded; fixed framing fits in 64 KiB.
        if output.len() > MAX_REPLAY_PACKAGE_BYTES {
            return Err(error("replay package exceeds size limit"));
        }
        Ok(output)
    }
    /// Accept exactly a definite-length four-field map. Inspect declared byte
    /// lengths against each field's bound before allocating owned payloads.
    pub fn decode_cbor(input: &[u8]) -> Result<Self, ProjectFileError> {
        if input.len() > MAX_REPLAY_PACKAGE_BYTES {
            return Err(error("replay package exceeds size limit"));
        }
        let mut reader = Reader(input);
        if reader.head(5)? != 4 {
            return Err(error("replay package requires four fields"));
        }
        let (mut format, mut version, mut script, mut project) = (None, None, None, None);
        for _ in 0..4 {
            match reader.text(16)? {
                "format" if format.is_none() => format = Some(reader.text(32)?),
                "version" if version.is_none() => version = Some(reader.head(0)?),
                "script" if script.is_none() => {
                    script = Some(reader.text(MAX_REPLAY_SCRIPT_BYTES)?)
                }
                "project" if project.is_none() => {
                    project = Some(reader.bytes(2, MAX_FILE_BYTES as usize)?)
                }
                _ => return Err(error("unknown or duplicate replay package field")),
            }
        }
        if !reader.0.is_empty() || format != Some(FORMAT) || version != Some(1) {
            return Err(error("unsupported replay package or trailing bytes"));
        }
        let script = script.ok_or_else(|| error("missing replay script"))?;
        let project = project.ok_or_else(|| error("missing replay project"))?;
        check_lengths(script.len(), project.len())?;
        Self::new(script.to_owned(), project.to_vec())
    }
}
fn check_lengths(script: usize, project: usize) -> Result<(), ProjectFileError> {
    if script == 0
        || script > MAX_REPLAY_SCRIPT_BYTES
        || project == 0
        || project > MAX_FILE_BYTES as usize
    {
        return Err(error("empty or excessive replay script/project"));
    }
    Ok(())
}

struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], ProjectFileError> {
        let value = self
            .0
            .get(..count)
            .ok_or_else(|| error("truncated replay CBOR"))?;
        self.0 = &self.0[count..];
        Ok(value)
    }
    fn head(&mut self, major: u8) -> Result<u64, ProjectFileError> {
        let byte = self.take(1)?[0];
        if byte >> 5 != major {
            return Err(error("wrong replay CBOR type"));
        }
        let extra = byte & 31;
        let count = match extra {
            0..=23 => return Ok(u64::from(extra)),
            24 => 1,
            25 => 2,
            26 => 4,
            27 => 8,
            _ => return Err(error("indefinite or reserved replay CBOR length")),
        };
        Ok(self
            .take(count)?
            .iter()
            .fold(0, |value, byte| (value << 8) | u64::from(*byte)))
    }
    fn bytes(&mut self, major: u8, limit: usize) -> Result<&'a [u8], ProjectFileError> {
        let count = self.head(major)?;
        if count > limit as u64 {
            return Err(error("replay CBOR field exceeds size limit"));
        }
        self.take(count as usize)
    }
    fn text(&mut self, limit: usize) -> Result<&'a str, ProjectFileError> {
        std::str::from_utf8(self.bytes(3, limit)?).map_err(|_| error("invalid replay UTF-8"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SCRIPT: &str = r#"{"version":2,"name":"Replay","starting_state":"project","steps":[]}"#;
    fn package() -> ReplayPackage {
        ReplayPackage::new(SCRIPT.into(), vec![0, 255, 7]).unwrap()
    }
    fn encode(value: &ciborium::Value) -> Vec<u8> {
        let mut bytes = Vec::new();
        ciborium::into_writer(value, &mut bytes).unwrap();
        bytes
    }
    #[test]
    fn preserves_exact_script_and_binary_archive_bytes() {
        let p = package();
        let bytes = p.encode_cbor().unwrap();
        assert_eq!(ReplayPackage::decode_cbor(&bytes).unwrap(), p);
        // Framing success is not a claim of archive validity.
        assert!(p.into_project().is_err());
    }
    #[test]
    fn rejects_wrong_version_duplicate_unknown_types_and_missing_fields() {
        let bytes = package().encode_cbor().unwrap();
        for case in 0..7 {
            let mut value: ciborium::Value = ciborium::from_reader(bytes.as_slice()).unwrap();
            let fields = value.as_map_mut().unwrap();
            match case {
                0 => fields[1].1 = 2.into(),
                1 => fields[3] = fields[0].clone(),
                2 => fields[3].0 = "unknown".into(),
                3 => fields[3].1 = ciborium::Value::Array(vec![]),
                4 => fields[2].1 = ciborium::Value::Bytes(vec![1]),
                5 => {
                    fields.pop();
                }
                _ => fields[0].1 = "other".into(),
            }
            assert!(
                ReplayPackage::decode_cbor(&encode(&value)).is_err(),
                "{case}"
            );
        }
    }
    #[test]
    fn rejects_truncation_trailing_invalid_utf8_and_indefinite_map() {
        let bytes = package().encode_cbor().unwrap();
        for end in 0..bytes.len() {
            assert!(ReplayPackage::decode_cbor(&bytes[..end]).is_err());
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(ReplayPackage::decode_cbor(&trailing).is_err());
        let mut indefinite = bytes.clone();
        indefinite[0] = 0xbf;
        assert!(ReplayPackage::decode_cbor(&indefinite).is_err());
        let mut invalid = bytes;
        let index = invalid
            .windows(FORMAT.len())
            .position(|x| x == FORMAT.as_bytes())
            .unwrap();
        invalid[index] = 255;
        assert!(ReplayPackage::decode_cbor(&invalid).is_err());
    }
    #[test]
    fn archive_decode_and_resave_retain_ancillary_and_opaque_intent() {
        use std::io::{Cursor, Read, Write};
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in [
            (
                "manifest.json",
                br#"{"format":"limo-cad-project","container_version":1,"model":"model.json"}"#
                    .as_slice(),
            ),
            (
                "model.json",
                br#"{"format":"limo-cad-project","schema_version":15}"#.as_slice(),
            ),
            ("images/retained.bin", &[0, 255, 128, 7][..]),
            ("intent/future.cbor", &[255, 0, 42][..]),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        let original = zip.finish().unwrap().into_inner();
        let package = ReplayPackage::new(SCRIPT.into(), original.clone()).unwrap();
        let decoded = ReplayPackage::decode_cbor(&package.encode_cbor().unwrap()).unwrap();
        assert_eq!(decoded.project_bytes(), original);
        let (source, project) = decoded.into_project().unwrap();
        assert_eq!(source, SCRIPT);
        assert_eq!(
            project.snapshot().unwrap().intents()[0].payload,
            [255, 0, 42]
        );
        let resaved = project.encode().unwrap();
        let mut reopened = zip::ZipArchive::new(Cursor::new(resaved)).unwrap();
        let mut bytes = Vec::new();
        reopened
            .by_name("images/retained.bin")
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, [0, 255, 128, 7]);
    }

    #[test]
    fn field_limits_checked_without_allocating_maximum_payloads() {
        assert!(check_lengths(MAX_REPLAY_SCRIPT_BYTES, MAX_FILE_BYTES as usize).is_ok());
        for (script, project) in [
            (0, 1),
            (1, 0),
            (MAX_REPLAY_SCRIPT_BYTES + 1, 1),
            (1, MAX_FILE_BYTES as usize + 1),
        ] {
            assert!(check_lengths(script, project).is_err());
        }
        // A declared u64::MAX length is rejected before seeking or allocation.
        let input = [0x5b, 255, 255, 255, 255, 255, 255, 255, 255];
        assert!(Reader(&input).bytes(2, MAX_FILE_BYTES as usize).is_err());
        assert_eq!(Reader(&[0x43, 1, 2, 3]).bytes(2, 3).unwrap(), &[1, 2, 3]);
        assert!(Reader(&[0x44, 1, 2, 3, 4]).bytes(2, 3).is_err());
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    #[test]
    fn sha256_known_vector_and_descriptor_bounds() {
        assert_eq!(
            ReplayPackageIdentity::from_bytes(b"abc").sha256,
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad
            ]
        );
        for length in [0, MAX_REPLAY_PACKAGE_BYTES as u64 + 1, u64::MAX] {
            let identity = ReplayPackageIdentity {
                byte_length: length,
                sha256: [0; 32],
            };
            assert!(identity.validate_length().is_err());
        }
        let identity = ReplayPackageIdentity::from_bytes(b"abc");
        let mut value = serde_json::to_value(&identity).unwrap();
        assert_eq!(
            serde_json::from_value::<ReplayPackageIdentity>(value.clone()).unwrap(),
            identity
        );
        value
            .as_object_mut()
            .unwrap()
            .insert("extra".into(), true.into());
        assert!(serde_json::from_value::<ReplayPackageIdentity>(value).is_err());
    }
    #[test]
    fn verifies_original_encoding_and_rejects_changed_or_malformed_file() {
        let path = std::env::temp_dir().join(format!(
            "limo-replay-identity-{}-{}.cbor",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let package = ReplayPackage::new("version: 2".into(), vec![1, 2, 3]).unwrap();
        let canonical = package.encode_cbor().unwrap();
        // A nonminimal definite map length is accepted, but has a distinct identity.
        let mut original = vec![0xb8, 4];
        original.extend_from_slice(&canonical[1..]);
        std::fs::write(&path, &original).unwrap();
        let (loaded, identity) = ReplayPackage::read_file_with_identity(&path).unwrap();
        assert_eq!(loaded, package);
        assert_eq!(identity, ReplayPackageIdentity::from_bytes(&original));
        assert_ne!(identity, ReplayPackageIdentity::from_bytes(&canonical));
        assert_eq!(
            ReplayPackage::read_file_verified(&path, &identity).unwrap(),
            package
        );
        let mut changed = original.clone();
        *changed.last_mut().unwrap() ^= 1;
        for bytes in [
            changed,
            original[..original.len() - 1].to_vec(),
            [original.clone(), vec![0]].concat(),
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert!(ReplayPackage::read_file_verified(&path, &identity).is_err());
        }
        // Correct integrity never substitutes for valid framing.
        let malformed = [0xff];
        std::fs::write(&path, malformed).unwrap();
        assert!(ReplayPackage::read_file_verified(
            &path,
            &ReplayPackageIdentity::from_bytes(&malformed)
        )
        .is_err());
        assert!(ReplayPackage::read_file_with_identity(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
