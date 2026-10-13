//! Stable, host-neutral callback event projection for post adapters.
//!
//! The planner emits controller-neutral commands. This module maps those
//! commands to a versioned event stream without executing a post script or
//! coupling path planning to any third-party runtime.

use serde::{Deserialize, Serialize};

use crate::model::{CamDocumentDto, CamToolDto, CoolantMode, Point3Dto, SpindleDirection};
use crate::planner::{CamCommandDto, CamProgramDto};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PostEventStreamDto {
    pub format: String,
    pub version: u32,
    pub units: String,
    pub program_name: String,
    pub tools: Vec<CamToolDto>,
    pub events: Vec<PostEventDto>,
    /// Planning and pre-post verification findings travel with the exported
    /// motion, including any limits on what was actually checked.
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "callback", rename_all = "camelCase")]
pub enum PostEventDto {
    OnOpen,
    OnWorkOffset {
        offset: crate::model::WorkOffset,
    },
    OnSection {
        operation_id: u64,
        name: String,
        tool_id: u64,
    },
    OnToolChange {
        tool_id: u64,
        tool_number: Option<u32>,
        tool_name: String,
    },
    OnSpindleSpeed {
        direction: SpindleDirection,
        rpm: u32,
    },
    OnCoolant {
        mode: CoolantMode,
    },
    OnRapid {
        x: f64,
        y: f64,
        z: f64,
    },
    OnLinear {
        x: f64,
        y: f64,
        z: f64,
        feedrate: f64,
    },
    OnCircular {
        clockwise: bool,
        center: Point3Dto,
        end: Point3Dto,
        feedrate: f64,
    },
    OnDwell {
        seconds: f64,
    },
    /// Machine-side cutter radius compensation activates on the linear move
    /// that follows (`left` true = tool left of travel, G41; false = G42).
    /// Only present for in-control contour sections; adapters that cannot
    /// drive machine compensation must reject the stream, not drop the word.
    OnCutterCompensationOn {
        left: bool,
    },
    /// Machine-side cutter radius compensation cancels (G40) on the linear
    /// move that follows.
    OnCutterCompensationOff,
    OnSectionEnd,
    OnClose,
}

pub fn post_event_stream(document: &CamDocumentDto, program: &CamProgramDto) -> PostEventStreamDto {
    let mut events = Vec::with_capacity(program.commands.len());
    for command in &program.commands {
        let event = match command {
            CamCommandDto::ProgramStart { .. } => PostEventDto::OnOpen,
            CamCommandDto::WorkOffset { offset } => PostEventDto::OnWorkOffset { offset: *offset },
            CamCommandDto::SectionStart {
                operation_id,
                name,
                tool_id,
            } => PostEventDto::OnSection {
                operation_id: *operation_id,
                name: name.clone(),
                tool_id: *tool_id,
            },
            CamCommandDto::ToolChange {
                tool_id,
                tool_number,
                tool_name,
            } => PostEventDto::OnToolChange {
                tool_id: *tool_id,
                tool_number: *tool_number,
                tool_name: tool_name.clone(),
            },
            CamCommandDto::Spindle { direction, rpm } => PostEventDto::OnSpindleSpeed {
                direction: *direction,
                rpm: *rpm,
            },
            CamCommandDto::Coolant { mode } => PostEventDto::OnCoolant { mode: *mode },

            CamCommandDto::SetPosition { .. } => continue,
            CamCommandDto::Rapid { to } => PostEventDto::OnRapid {
                x: to.x,
                y: to.y,
                z: to.z,
            },
            CamCommandDto::Linear { to, feed } => PostEventDto::OnLinear {
                x: to.x,
                y: to.y,
                z: to.z,
                feedrate: *feed,
            },
            CamCommandDto::Circular {
                clockwise,
                center,
                to,
                feed,
                ..
            } => PostEventDto::OnCircular {
                clockwise: *clockwise,
                center: *center,
                end: *to,
                feedrate: *feed,
            },
            CamCommandDto::Dwell { seconds } => PostEventDto::OnDwell { seconds: *seconds },
            CamCommandDto::CutterCompensationOn { left } => {
                PostEventDto::OnCutterCompensationOn { left: *left }
            }
            CamCommandDto::CutterCompensationOff => PostEventDto::OnCutterCompensationOff,
            CamCommandDto::SectionEnd => PostEventDto::OnSectionEnd,
            CamCommandDto::ProgramEnd => PostEventDto::OnClose,
        };
        events.push(event);
    }
    PostEventStreamDto {
        format: "limo-cad-post-events".to_string(),
        version: 1,
        units: "millimeters".to_string(),
        program_name: program.name.clone(),
        tools: document.tools.clone(),
        events,
        warnings: program.warnings.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Point3Dto, WorkOffset};

    #[test]
    fn maps_three_axis_motion_to_callback_events() {
        let program = CamProgramDto {
            setup_id: 7,
            name: "Sample".into(),
            commands: vec![
                CamCommandDto::ProgramStart {
                    name: "Sample".into(),
                    work_offset: WorkOffset::G54,
                },
                CamCommandDto::Rapid {
                    to: Point3Dto::new(1.0, 2.0, 3.0),
                },
                CamCommandDto::Linear {
                    to: Point3Dto::new(4.0, 5.0, -1.0),
                    feed: 500.0,
                },
                CamCommandDto::ProgramEnd,
            ],
            stats: Default::default(),
            per_operation: vec![],
            work_offsets: vec![WorkOffset::G54],
            warnings: vec!["Part-gouge clearance is UNVERIFIED".into()],
        };
        let events = post_event_stream(&CamDocumentDto::default(), &program);
        assert_eq!(events.format, "limo-cad-post-events");
        assert!(matches!(events.events[0], PostEventDto::OnOpen));
        assert!(matches!(
            events.events[1],
            PostEventDto::OnRapid { x: 1.0, .. }
        ));
        assert!(matches!(
            events.events[2],
            PostEventDto::OnLinear {
                feedrate: 500.0,
                ..
            }
        ));
        assert!(matches!(events.events[3], PostEventDto::OnClose));
        let serialized = serde_json::to_value(&events).unwrap();
        assert_eq!(serialized["events"][0]["callback"], "onOpen");
        assert_eq!(serialized["events"][1]["callback"], "onRapid");
        assert_eq!(events.warnings, program.warnings);
        assert_eq!(serialized["warnings"][0], program.warnings[0]);

        let mut legacy = serialized;
        legacy.as_object_mut().unwrap().remove("warnings");
        let legacy: PostEventStreamDto = serde_json::from_value(legacy).unwrap();
        assert!(legacy.warnings.is_empty());
        assert_eq!(legacy.events.len(), events.events.len());
    }
}
