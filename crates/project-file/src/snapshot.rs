//! Immutable composite model/domain intent for history and publication adapters.
use crate::{error, ProjectFileError, PROJECT_FORMAT};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    io::{Cursor, Write},
};

pub const MAX_SNAPSHOT_BYTES: usize = 256 * 1024 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotIntent {
    pub name: String,
    #[serde(with = "bytes")]
    pub payload: Vec<u8>,
}
/// Validated envelope shape, not validated CAD or domain semantics. Consumers
/// must prepare every known domain before committing any live replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSnapshot {
    model_json: String,
    intents: Vec<SnapshotIntent>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    format: String,
    version: u32,
    model_json: String,
    intents: Vec<SnapshotIntent>,
}
#[derive(Serialize)]
struct WireRef<'a> {
    format: &'static str,
    version: u32,
    model_json: &'a str,
    intents: &'a [SnapshotIntent],
}
impl ProjectSnapshot {
    pub fn new(
        model_json: String,
        mut intents: Vec<SnapshotIntent>,
    ) -> Result<Self, ProjectFileError> {
        if intents.len() > 64 || model_json.len() > MAX_SNAPSHOT_BYTES {
            return Err(error("project snapshot exceeds collection or model limit"));
        }
        let header: serde_json::Value = serde_json::from_str(&model_json)
            .map_err(|e| error(format!("invalid snapshot model: {e}")))?;
        if header["format"].as_str() != Some(PROJECT_FORMAT)
            || header["schema_version"].as_u64().is_none_or(|n| n == 0)
        {
            return Err(error("invalid snapshot model header"));
        }
        let mut names = BTreeSet::new();
        let mut total = model_json.len();
        for intent in &intents {
            crate::archive::validate_intent_name(&intent.name)?;
            if !names.insert(&intent.name) {
                return Err(error("duplicate snapshot intent"));
            }
            total = total
                .checked_add(intent.payload.len())
                .and_then(|n| n.checked_add(intent.name.len()))
                .ok_or_else(|| error("project snapshot size overflow"))?;
            if total > MAX_SNAPSHOT_BYTES {
                return Err(error("project snapshot exceeds size limit"));
            }
        }
        intents.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Self {
            model_json,
            intents,
        })
    }
    pub fn model_json(&self) -> &str {
        &self.model_json
    }
    pub fn intents(&self) -> &[SnapshotIntent] {
        &self.intents
    }
    /// JSON command transports carry the canonical CBOR snapshot as standard
    /// padded base64. This does not enlarge script/inbox admission limits.
    pub fn encode_base64(&self) -> Result<String, ProjectFileError> {
        use base64::Engine as _;
        Ok(base64::engine::general_purpose::STANDARD.encode(self.encode_cbor()?))
    }
    pub fn decode_base64(encoded: &str) -> Result<Self, ProjectFileError> {
        use base64::Engine as _;
        if encoded.len() > MAX_SNAPSHOT_BYTES.div_ceil(3) * 4 {
            return Err(error("encoded project snapshot exceeds size limit"));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|e| error(format!("invalid project snapshot base64: {e}")))?;
        Self::decode_cbor(&bytes)
    }
    pub fn encode_cbor(&self) -> Result<Vec<u8>, ProjectFileError> {
        let mut output = LimitedOutput(Vec::new());
        ciborium::into_writer(
            &WireRef {
                format: "limo-cad-project-snapshot",
                version: 1,
                model_json: &self.model_json,
                intents: &self.intents,
            },
            &mut output,
        )
        .map_err(|e| error(format!("could not encode project snapshot: {e}")))?;
        Ok(output.0)
    }
    pub fn decode_cbor(input: &[u8]) -> Result<Self, ProjectFileError> {
        if input.is_empty() || input.len() > MAX_SNAPSHOT_BYTES || input[0] >> 5 != 5 {
            return Err(error("invalid or excessive project snapshot envelope"));
        }
        let mut reader = Cursor::new(input);
        let wire: Wire = ciborium::de::from_reader_with_recursion_limit(&mut reader, 16)
            .map_err(|e| error(format!("invalid project snapshot CBOR: {e}")))?;
        if reader.position() != input.len() as u64
            || wire.format != "limo-cad-project-snapshot"
            || wire.version != 1
        {
            return Err(error("unsupported project snapshot or trailing bytes"));
        }
        Self::new(wire.model_json, wire.intents)
    }
}
struct LimitedOutput(Vec<u8>);
impl Write for LimitedOutput {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if b.len() > MAX_SNAPSHOT_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("snapshot exceeds size limit"));
        }
        self.0.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub(crate) mod bytes {
    use serde::{
        de::{Error, Visitor},
        Deserializer, Serializer,
    };
    pub fn serialize<S: Serializer>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(value)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        struct Bytes;
        impl<'de> Visitor<'de> for Bytes {
            type Value = Vec<u8>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a CBOR byte string")
            }
            fn visit_bytes<E: Error>(self, v: &[u8]) -> Result<Vec<u8>, E> {
                Ok(v.to_vec())
            }
            fn visit_byte_buf<E: Error>(self, v: Vec<u8>) -> Result<Vec<u8>, E> {
                Ok(v)
            }
        }
        deserializer.deserialize_byte_buf(Bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProjectArchive, SaveMetadata};
    fn model(name: &str) -> String {
        format!(
            r#"{{"format":"limo-cad-project","schema_version":15,"document":{{"name":"{name}"}}}}"#
        )
    }
    fn metadata() -> SaveMetadata<'static> {
        SaveMetadata {
            application_version: "test",
            saved_at: "test",
        }
    }
    fn item(name: &str, payload: &[u8]) -> SnapshotIntent {
        SnapshotIntent {
            name: name.into(),
            payload: payload.to_vec(),
        }
    }
    #[test]
    fn paired_snapshot_roundtrip_preserves_exact_model_and_unknown_domain_bytes() {
        let text = model("Original") + "\n";
        let value = ProjectSnapshot::new(
            text.clone(),
            vec![item("intent/z", &[255, 0, 3]), item("intent/a", b"opaque")],
        )
        .unwrap();
        let bytes = value.encode_cbor().unwrap();
        let reopened = ProjectSnapshot::decode_cbor(&bytes).unwrap();
        assert_eq!(reopened, value);
        assert_eq!(reopened.model_json(), text);
        assert_eq!(reopened.intents()[0].name, "intent/a");
        let wire: ciborium::Value = ciborium::from_reader(bytes.as_slice()).unwrap();
        let map = wire.as_map().unwrap();
        let intents = map
            .iter()
            .find(|(k, _)| k.as_text() == Some("intents"))
            .unwrap()
            .1
            .as_array()
            .unwrap();
        assert!(intents[0]
            .as_map()
            .unwrap()
            .iter()
            .find(|(k, _)| k.as_text() == Some("payload"))
            .unwrap()
            .1
            .as_bytes()
            .is_some());
    }
    #[test]
    fn archive_snapshot_restores_model_and_intents_together() {
        let mut archive = ProjectArchive::new(model("A"), metadata()).unwrap();
        archive.set_intent("intent/study", Some(vec![1])).unwrap();
        let a = archive.snapshot().unwrap();
        archive.update_model(model("B"), metadata()).unwrap();
        archive.set_intent("intent/study", Some(vec![2])).unwrap();
        archive.set_intent("intent/later", Some(vec![3])).unwrap();
        let exact = ProjectSnapshot::new(model("No newline"), vec![]).unwrap();
        assert_eq!(
            archive
                .with_snapshot(&exact, metadata())
                .unwrap()
                .model_json(),
            exact.model_json()
        );
        let b = archive.snapshot().unwrap();
        let restored = archive.with_snapshot(&a, metadata()).unwrap();
        assert_eq!(restored.snapshot().unwrap(), a);
        assert_eq!(archive.snapshot().unwrap(), b);
        assert_eq!(restored.read_intent("intent/later", 10).unwrap(), None);
        let reopened = ProjectArchive::decode(restored.encode().unwrap()).unwrap();
        assert_eq!(reopened.snapshot().unwrap(), a);
    }
    #[test]
    fn snapshot_rejects_bad_headers_names_duplicates_and_versions() {
        assert!(ProjectSnapshot::new("{}".into(), vec![]).is_err());
        for name in ["model.json", "intent/../bad", "intent//bad"] {
            assert!(ProjectSnapshot::new(model("A"), vec![item(name, b"x")]).is_err());
        }
        assert!(ProjectSnapshot::new(
            model("A"),
            vec![item("intent/a", b"x"), item("intent/a", b"y")]
        )
        .is_err());
        let wire = Wire {
            format: "limo-cad-project-snapshot".into(),
            version: 2,
            model_json: model("A"),
            intents: vec![],
        };
        let mut bytes = Vec::new();
        ciborium::into_writer(&wire, &mut bytes).unwrap();
        assert!(ProjectSnapshot::decode_cbor(&bytes).is_err());
    }
    #[test]
    fn snapshot_rejects_truncation_and_trailing_bytes() {
        let valid = ProjectSnapshot::new(model("A"), vec![item("intent/a", b"payload")])
            .unwrap()
            .encode_cbor()
            .unwrap();
        for n in 0..valid.len() {
            assert!(ProjectSnapshot::decode_cbor(&valid[..n]).is_err());
        }
        let mut trailing = valid;
        trailing.push(0);
        assert!(ProjectSnapshot::decode_cbor(&trailing).is_err());
    }
    #[test]
    fn command_transport_preserves_pair_and_rejects_invalid_encoding() {
        let snapshot = ProjectSnapshot::new(
            model("Command"),
            vec![SnapshotIntent {
                name: "intent/future.cbor".into(),
                payload: vec![0, 255, 128, 7],
            }],
        )
        .unwrap();
        let encoded = snapshot.encode_base64().unwrap();
        assert_eq!(ProjectSnapshot::decode_base64(&encoded).unwrap(), snapshot);
        for invalid in ["", "***", "gA==", "e30=", "AA==extra"] {
            assert!(
                ProjectSnapshot::decode_base64(invalid).is_err(),
                "{invalid}"
            );
        }
    }
}
