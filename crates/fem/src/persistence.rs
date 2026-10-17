//! CBOR v1 study intent. No worker identity or computed result is persisted.
use crate::{ThermalStudy, ThermalStudyInput};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    io::{Cursor, Write},
};

pub const MAX_THERMAL_STUDY_BYTES: usize = 16 * 1024 * 1024;
const FORMAT: &str = "limo-cad-thermal-studies";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedThermalStudy {
    pub id: u64,
    pub name: String,
    pub input: ThermalStudyInput,
}
impl SavedThermalStudy {
    /// Reopened intent starts idle with no result and no pending worker.
    pub fn open(&self) -> ThermalStudy {
        ThermalStudy::new(self.input.clone())
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThermalStudyDocument {
    pub studies: Vec<SavedThermalStudy>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format: String,
    version: u32,
    studies: Vec<SavedThermalStudy>,
}
#[derive(Serialize)]
struct EnvelopeRef<'a> {
    format: &'static str,
    version: u32,
    studies: &'a [SavedThermalStudy],
}

struct BoundedWriter(Vec<u8>);
impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_THERMAL_STUDY_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other(
                "Thermal study payload exceeds 16 MiB",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl ThermalStudyDocument {
    pub fn encode_cbor(&self) -> Result<Vec<u8>, String> {
        self.validate_storage()?;
        let mut writer = BoundedWriter(Vec::new());
        ciborium::into_writer(
            &EnvelopeRef {
                format: FORMAT,
                version: 1,
                studies: &self.studies,
            },
            &mut writer,
        )
        .map_err(|e| format!("Could not encode thermal studies: {e}"))?;
        Ok(writer.0)
    }
    /// Decode a candidate before replacing live state. Unknown fields/versions,
    /// trailing bytes and malformed or excessive data fail without partial data.
    pub fn decode_cbor(bytes: &[u8]) -> Result<Self, String> {
        if bytes.is_empty() || bytes.len() > MAX_THERMAL_STUDY_BYTES {
            return Err("Thermal study payload must contain 1 byte to 16 MiB".into());
        }
        if bytes[0] >> 5 != 5 {
            return Err("Thermal study envelope must be a CBOR map".into());
        }
        let mut reader = Cursor::new(bytes);
        let envelope: Envelope = ciborium::de::from_reader_with_recursion_limit(&mut reader, 32)
            .map_err(|e| format!("Invalid thermal study CBOR: {e}"))?;
        if reader.position() != bytes.len() as u64 {
            return Err("Trailing data after thermal studies".into());
        }
        if envelope.format != FORMAT || envelope.version != 1 {
            return Err("Unsupported thermal study format or version".into());
        }
        let document = Self {
            studies: envelope.studies,
        };
        document.validate_storage()?;
        Ok(document)
    }
    fn validate_storage(&self) -> Result<(), String> {
        if self.studies.len() > 64 {
            return Err("At most 64 thermal studies can be stored".into());
        }
        let mut ids = BTreeSet::new();
        for study in &self.studies {
            if study.id == 0
                || !ids.insert(study.id)
                || study.name.trim().is_empty()
                || study.name.len() > 256
            {
                return Err("Invalid or duplicate thermal study identity".into());
            }
            validate_input(&study.input)?;
        }
        Ok(())
    }
}
fn validate_input(i: &ThermalStudyInput) -> Result<(), String> {
    if i.mesh.points_m.len() > 65_536
        || i.mesh.elements.len() > 262_144
        || i.materials.len() > 256
        || i.prescribed_k.len() > 65_536
        || i.nodal_load_w.len() > 65_536
        || i.surfaces.len() > 262_144
    {
        return Err("Thermal study collections exceed storage limits".into());
    }
    // Incomplete drafts are retained, including unresolved indices and missing
    // constraints. Finite values and portable integer ranges are required.
    let finite = i
        .mesh
        .points_m
        .iter()
        .flatten()
        .copied()
        .chain(i.mesh.elements.iter().map(|e| e.source_w_m3))
        .chain(i.prescribed_k.iter().map(|(_, k)| *k))
        .chain(i.nodal_load_w.iter().copied())
        .chain(i.surfaces.iter().flat_map(|s| {
            [
                Some(s.heat_flux_in_w_m2),
                s.convection.map(|c| c.coefficient_w_m2_k),
                s.convection.map(|c| c.ambient_k),
            ]
            .into_iter()
            .flatten()
        }))
        .chain([i.options.relative_tolerance, i.options.absolute_tolerance_w])
        .all(f64::is_finite);
    if !finite {
        return Err("Stored thermal values must be finite".into());
    }
    let portable = [
        i.options.max_nodes,
        i.options.max_elements,
        i.options.max_iterations,
    ]
    .into_iter()
    .chain(
        i.mesh
            .elements
            .iter()
            .flat_map(|e| e.nodes.into_iter().chain([e.material])),
    )
    .chain(i.prescribed_k.iter().map(|(node, _)| *node))
    .chain(i.surfaces.iter().flat_map(|s| s.nodes))
    .all(|v| u32::try_from(v).is_ok());
    if !portable {
        return Err("Stored thermal indices and budgets must fit unsigned 32 bits".into());
    }
    for m in &i.materials {
        m.details.validate()?;
        if m.context.as_ref().is_some_and(|s| s.len() > 512)
            || m.source_id.as_ref().is_some_and(|s| s.len() > 4096)
        {
            return Err("Thermal material selection exceeds storage limits".into());
        }
    }
    Ok(())
}

/// Reserved project-container entry for the versioned thermal document.
pub const THERMAL_STUDY_ENTRY: &str = "intent/thermal-studies.cbor";
impl ThermalStudyDocument {
    /// Legacy containers without the entry have no thermal studies. Invalid or
    /// newer data is an error, never silently converted to an empty document.
    pub fn from_project(archive: &limo_cad_project_file::ProjectArchive) -> Result<Self, String> {
        match archive
            .read_intent(THERMAL_STUDY_ENTRY, MAX_THERMAL_STUDY_BYTES)
            .map_err(|e| e.to_string())?
        {
            Some(bytes) => Self::decode_cbor(&bytes),
            None => Ok(Self::default()),
        }
    }
    /// Validate both the existing payload and the replacement before staging.
    /// This prevents an older reader from overwriting unknown future intent.
    pub fn store_in_project(
        &self,
        archive: &mut limo_cad_project_file::ProjectArchive,
    ) -> Result<(), String> {
        Self::from_project(archive)?;
        let bytes = self.encode_cbor()?;
        archive
            .set_intent(THERMAL_STUDY_ENTRY, Some(bytes))
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn writer_never_exceeds_payload_limit() {
        let mut writer = BoundedWriter(Vec::new());
        let chunk = [0_u8; 4096];
        for _ in 0..MAX_THERMAL_STUDY_BYTES / chunk.len() {
            writer.write_all(&chunk).unwrap();
        }
        assert_eq!(writer.0.len(), MAX_THERMAL_STUDY_BYTES);
        assert!(writer.write_all(&[0]).is_err());
        assert_eq!(writer.0.len(), MAX_THERMAL_STUDY_BYTES);
    }
}
