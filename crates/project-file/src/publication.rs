//! Versioned publication data, independent of filesystem commit protocols.
//! Matching a receipt proves envelope consistency, not publisher authenticity
//! or CAD/domain validity. Callers must verify their transport and live owner.
use crate::{error, ProjectFileError, ProjectSnapshot, MAX_SNAPSHOT_BYTES};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

pub const MAX_PUBLICATION_BYTES: usize = MAX_SNAPSHOT_BYTES + 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationReceipt {
    pub process_instance_id: String,
    pub session_id: String,
    pub window_id: String,
    pub document_id: String,
    pub epoch: u64,
    pub engine_revision: u64,
    pub export_sequence: u64,
}
impl PublicationReceipt {
    fn validate(&self) -> Result<(), ProjectFileError> {
        for value in [
            &self.process_instance_id,
            &self.session_id,
            &self.window_id,
            &self.document_id,
        ] {
            if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
                return Err(error("invalid publication owner identity"));
            }
        }
        if self.epoch == 0 || self.engine_revision == 0 || self.export_sequence == 0 {
            return Err(error("publication counters must be positive"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectPublication {
    receipt: PublicationReceipt,
    snapshot: ProjectSnapshot,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    format: String,
    version: u32,
    receipt: PublicationReceipt,
    #[serde(with = "crate::snapshot::bytes")]
    snapshot: Vec<u8>,
}
impl ProjectPublication {
    pub fn new(
        receipt: PublicationReceipt,
        snapshot: ProjectSnapshot,
    ) -> Result<Self, ProjectFileError> {
        receipt.validate()?;
        Ok(Self { receipt, snapshot })
    }
    pub fn receipt(&self) -> &PublicationReceipt {
        &self.receipt
    }
    pub fn snapshot(&self) -> &ProjectSnapshot {
        &self.snapshot
    }

    pub fn encode_cbor(&self) -> Result<Vec<u8>, ProjectFileError> {
        // The snapshot encoder is bounded; four bounded identity strings and
        // fixed map/counter overhead fit within the additional 4096 bytes.
        let wire = Wire {
            format: "limo-cad-project-publication".into(),
            version: 1,
            receipt: self.receipt.clone(),
            snapshot: self.snapshot.encode_cbor()?,
        };
        let mut output = Vec::new();
        ciborium::into_writer(&wire, &mut output)
            .map_err(|e| error(format!("could not encode publication: {e}")))?;
        if output.len() > MAX_PUBLICATION_BYTES {
            return Err(error("publication exceeds size limit"));
        }
        Ok(output)
    }

    /// Decode only for an independently obtained commit receipt. All identity
    /// and revision fields must match; older/newer is not an acceptable match.
    /// No model/domain data is returned if the receipt differs.
    pub fn decode_for(
        input: &[u8],
        expected: &PublicationReceipt,
    ) -> Result<Self, ProjectFileError> {
        expected.validate()?;
        if input.is_empty() || input.len() > MAX_PUBLICATION_BYTES || input[0] >> 5 != 5 {
            return Err(error("invalid or excessive publication envelope"));
        }
        let mut reader = Cursor::new(input);
        let wire: Wire = ciborium::de::from_reader_with_recursion_limit(&mut reader, 16)
            .map_err(|e| error(format!("invalid publication CBOR: {e}")))?;
        if reader.position() != input.len() as u64
            || wire.format != "limo-cad-project-publication"
            || wire.version != 1
        {
            return Err(error("unsupported publication or trailing bytes"));
        }
        if &wire.receipt != expected {
            return Err(error("publication does not match its commit receipt"));
        }
        Self::new(wire.receipt, ProjectSnapshot::decode_cbor(&wire.snapshot)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SnapshotIntent;
    fn receipt() -> PublicationReceipt {
        PublicationReceipt {
            process_instance_id: "process-a".into(),
            session_id: "session-a".into(),
            window_id: "main".into(),
            document_id: "document-a".into(),
            epoch: 7,
            engine_revision: 11,
            export_sequence: 13,
        }
    }
    fn publication() -> ProjectPublication {
        ProjectPublication::new(
            receipt(),
            ProjectSnapshot::new(
                r#"{"format":"limo-cad-project","schema_version":15}"#.into(),
                vec![SnapshotIntent {
                    name: "intent/future.cbor".into(),
                    payload: vec![0, 255, 128, 7],
                }],
            )
            .unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn round_trip_retains_complete_snapshot_and_exact_receipt() {
        let original = publication();
        let restored =
            ProjectPublication::decode_for(&original.encode_cbor().unwrap(), &receipt()).unwrap();
        assert_eq!(restored, original);
        assert_eq!(restored.snapshot().intents()[0].payload, [0, 255, 128, 7]);
    }
    #[test]
    fn every_owner_and_counter_mismatch_rejects() {
        let bytes = publication().encode_cbor().unwrap();
        for field in 0..7 {
            let mut other = receipt();
            match field {
                0 => other.process_instance_id.push('x'),
                1 => other.session_id.push('x'),
                2 => other.window_id.push('x'),
                3 => other.document_id.push('x'),
                4 => other.epoch += 1,
                5 => other.engine_revision += 1,
                _ => other.export_sequence += 1,
            }
            assert!(
                ProjectPublication::decode_for(&bytes, &other).is_err(),
                "{field}"
            );
        }
    }
    #[test]
    fn invalid_identities_and_zero_counters_reject_before_publication() {
        for field in 0..7 {
            let mut bad = receipt();
            match field {
                0 => bad.process_instance_id.clear(),
                1 => bad.session_id = "x".repeat(257),
                2 => bad.window_id = "bad\nwindow".into(),
                3 => bad.document_id.clear(),
                4 => bad.epoch = 0,
                5 => bad.engine_revision = 0,
                _ => bad.export_sequence = 0,
            }
            assert!(
                ProjectPublication::new(bad, publication().snapshot().clone()).is_err(),
                "{field}"
            );
        }
    }
    #[test]
    fn malformed_version_trailing_and_nested_snapshot_reject() {
        let good = publication().encode_cbor().unwrap();
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(ProjectPublication::decode_for(&trailing, &receipt()).is_err());
        for bytes in [&[][..], &[0x80][..], &good[..good.len() - 1]] {
            assert!(ProjectPublication::decode_for(bytes, &receipt()).is_err());
        }
        for case in 0..3 {
            let mut wire: Wire = ciborium::from_reader(good.as_slice()).unwrap();
            match case {
                0 => wire.version = 2,
                1 => wire.format = "other".into(),
                _ => wire.snapshot = vec![0x80],
            }
            let mut encoded = Vec::new();
            ciborium::into_writer(&wire, &mut encoded).unwrap();
            assert!(ProjectPublication::decode_for(&encoded, &receipt()).is_err());
        }
    }
    #[test]
    fn unknown_duplicate_fields_and_array_encoded_snapshot_reject() {
        let good = publication().encode_cbor().unwrap();
        for case in 0..3 {
            let mut value: ciborium::Value = ciborium::from_reader(good.as_slice()).unwrap();
            let ciborium::Value::Map(ref mut fields) = value else {
                panic!("map required")
            };
            match case {
                0 => fields.push(("unknown".into(), 1.into())),
                1 => fields.push(fields[0].clone()),
                _ => {
                    fields
                        .iter_mut()
                        .find(|(key, _)| key.as_text() == Some("snapshot"))
                        .unwrap()
                        .1 = ciborium::Value::Array(vec![])
                }
            }
            let mut encoded = Vec::new();
            ciborium::into_writer(&value, &mut encoded).unwrap();
            assert!(
                ProjectPublication::decode_for(&encoded, &receipt()).is_err(),
                "{case}"
            );
        }
    }
}
