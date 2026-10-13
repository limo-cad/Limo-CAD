//! Transient NC input for the existing shared workpiece simulator.
//! Source text belongs to the simulation view, never the CAM document.
use limo_cad_cam::{CamGcodeDialectDto, CamGcodeSimulationRequestDto, CamSimulationRequestDto};
use std::{io::Read, path::Path};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Input {
    pub source: String,
    pub file_name: Option<String>,
    pub dialect: CamGcodeDialectDto,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            source: String::new(),
            file_name: Some("program.mpf".into()),
            dialect: CamGcodeDialectDto::Auto,
        }
    }
}

impl Input {
    pub fn request(&self, base: CamSimulationRequestDto) -> CamGcodeSimulationRequestDto {
        CamGcodeSimulationRequestDto {
            setup_id: base.setup_id,
            source: self.source.clone(),
            file_name: Some(
                self.file_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .unwrap_or("program.nc")
                    .to_owned(),
            ),
            dialect: self.dialect,
            voxel_size: base.voxel_size,
            max_voxels: base.max_voxels,
            stock_mesh: base.stock_mesh,
            target: base.target,
            completed_steps: None,
        }
    }

    pub fn edit(&mut self, source: String) -> Result<(), String> {
        if source.len() > limo_cad_cam::MAX_GCODE_BYTES {
            return Err(format!(
                "NC source exceeds {} bytes",
                limo_cad_cam::MAX_GCODE_BYTES
            ));
        }
        self.source = source;
        Ok(())
    }
}

pub(super) fn read(path: &Path) -> Result<Input, String> {
    if !path.is_absolute() {
        return Err("Choose an absolute NC file path".into());
    }
    if !std::fs::metadata(path)
        .map_err(|error| format!("Cannot inspect NC source: {error}"))?
        .is_file()
    {
        return Err("Choose a regular NC source file".into());
    }
    let file =
        std::fs::File::open(path).map_err(|error| format!("Cannot open NC source: {error}"))?;
    let mut bytes = Vec::new();
    file.take(limo_cad_cam::MAX_GCODE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Cannot read NC source: {error}"))?;
    if bytes.len() > limo_cad_cam::MAX_GCODE_BYTES {
        return Err(format!(
            "NC source exceeds {} bytes",
            limo_cad_cam::MAX_GCODE_BYTES
        ));
    }
    let text = String::from_utf8(bytes).map_err(|_| "NC source must contain valid UTF-8 text")?;
    let source = text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned();
    Ok(Input {
        source,
        file_name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned()),
        dialect: CamGcodeDialectDto::Auto,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasted_program_defaults_match_existing_controller_detection_and_submission() {
        let base: CamSimulationRequestDto =
            serde_json::from_value(serde_json::json!({"setup_id":1})).unwrap();
        let mut input = Input::default();
        assert_eq!(
            input.request(base.clone()).file_name.as_deref(),
            Some("program.mpf")
        );
        input.file_name = Some("   ".into());
        assert_eq!(
            input.request(base.clone()).file_name.as_deref(),
            Some("program.nc")
        );
        input.file_name = Some("  actual.spf  ".into());
        assert_eq!(input.request(base).file_name.as_deref(), Some("actual.spf"));
    }

    #[test]
    fn nc_file_read_preserves_program_and_rejects_invalid_or_oversized_input() {
        let directory =
            std::env::temp_dir().join(format!("limo-cad-nc-input-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("program.mpf");
        std::fs::write(&path, "\u{feff}N10 G0 X0 Y0 Z10\r\n; Café 零件\r\n").unwrap();
        let input = read(&path).unwrap();
        assert_eq!(input.source, "N10 G0 X0 Y0 Z10\r\n; Café 零件\r\n");
        assert_eq!(input.file_name.as_deref(), Some("program.mpf"));
        assert_eq!(input.dialect, CamGcodeDialectDto::Auto);
        std::fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(read(&path).unwrap_err().contains("UTF-8"));
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(limo_cad_cam::MAX_GCODE_BYTES as u64 + 1)
            .unwrap();
        drop(file);
        assert!(read(&path).unwrap_err().contains("exceeds"));
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn nc_request_keeps_shared_settings_and_never_inherits_cam_operation_scope() {
        let base: CamSimulationRequestDto = serde_json::from_value(serde_json::json!({
            "setup_id":9,"voxel_size":0.25,"max_voxels":1000000,
            "through_operation_id":37,"completed_steps":5,"playback_time_seconds":12.5
        }))
        .unwrap();
        let input = Input {
            source: "G21\nG90\n".into(),
            file_name: Some("part.nc".into()),
            dialect: CamGcodeDialectDto::Iso,
        };
        let request = input.request(base);
        assert_eq!(request.setup_id, 9);
        assert_eq!(request.voxel_size, Some(0.25));
        assert_eq!(request.max_voxels, Some(1000000));
        assert_eq!(request.source, input.source);
        assert_eq!(request.file_name, input.file_name);
        assert_eq!(request.dialect, CamGcodeDialectDto::Iso);
        assert_eq!(request.completed_steps, None);
    }
}
