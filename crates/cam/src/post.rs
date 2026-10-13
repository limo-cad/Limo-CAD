use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::model::{
    CamDocumentDto, CamPostConfigDto, CamUnits, CoolantMode, Point3Dto, PostDialect,
    Siemens828dPostConfigDto, Siemens828dToolChangePositioning, SpindleDirection, WorkOffset,
};
use crate::planner::{plan_setup, CamCommandDto, CamPlanError, CamProgramDto};

#[cfg(test)]
#[path = "brand_post_tests.rs"]
mod brand_post_tests;
#[path = "brand_posts.rs"]
mod brand_posts;

/// Converts canonical millimetre motion into the document's output units and
/// formats controller words. Seconds and RPM pass through unchanged.
#[derive(Debug, Clone, Copy)]
struct PostUnits {
    units: CamUnits,
    contains_arcs: bool,
}

impl PostUnits {
    fn len(self, value_mm: f64) -> String {
        if self.contains_arcs {
            decimal(self.units.from_mm(value_mm), self.arc_precision())
        } else {
            coordinate(self.units.from_mm(value_mm))
        }
    }

    fn feed(self, value_mm_per_min: f64) -> String {
        feedrate(self.units.from_mm(value_mm_per_min))
    }

    fn siemens_len(self, value_mm: f64) -> String {
        if self.contains_arcs {
            decimal(self.units.from_mm(value_mm), self.arc_precision())
        } else {
            siemens_coordinate(self.units.from_mm(value_mm))
        }
    }

    /// Rounded endpoints and independently rounded incremental centers can
    /// disagree on radius. Retain sub-micron numeric precision throughout an
    /// arc-bearing program, including the preceding linear/rapid start. This
    /// is output precision, not a claim about machine positioning accuracy.
    fn arc_precision(self) -> usize {
        match self.units {
            CamUnits::Millimeters => 6,
            CamUnits::Inches => 8,
        }
    }

    /// Modal unit word for the ISO-style posts (`G21` metric / `G20` inch).
    fn iso_mode_word(self) -> &'static str {
        match self.units {
            CamUnits::Millimeters => "G21",
            CamUnits::Inches => "G20",
        }
    }

    /// SINUMERIK geometry AND feed units (`G710` metric / `G700` inch).
    /// G70 only changes geometry; its feed uses the machine's base units.
    fn siemens_mode_word(self) -> &'static str {
        match self.units {
            CamUnits::Millimeters => "G710",
            CamUnits::Inches => "G700",
        }
    }
}

/// The compensation gate must see the same quantized coordinates that the
/// controller receives, especially with inch output and short linear leads.
pub(crate) fn output_point_mm(
    point: Point3Dto,
    units: CamUnits,
    dialect: PostDialect,
    contains_arcs: bool,
) -> Point3Dto {
    let formatter = PostUnits {
        units,
        contains_arcs,
    };
    let round = |value| {
        let word = if dialect == PostDialect::Siemens828d {
            formatter.siemens_len(value)
        } else {
            formatter.len(value)
        };
        units.to_mm(word.parse::<f64>().unwrap_or(f64::NAN))
    };
    Point3Dto::new(round(point.x), round(point.y), round(point.z))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamPostRequestDto {
    pub setup_id: u64,
    /// Program-specific output override. Must match the setup's controller
    /// and machine-specific settings; omission uses its profile snapshot.
    #[serde(default)]
    pub post: Option<CamPostConfigDto>,
    #[serde(default)]
    pub program_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamPostResultDto {
    pub program: CamProgramDto,
    pub dialect: PostDialect,
    pub extension: String,
    pub nc: String,
    pub warnings: Vec<String>,
}

/// Production NC boundary: a setup target and matching post are mandatory.
pub fn post_setup(
    document: &CamDocumentDto,
    request: &CamPostRequestDto,
) -> Result<CamPostResultDto, CamPlanError> {
    let post =
        crate::machine::machine_post_config(document, request.setup_id, request.post.as_ref())?;
    let mut resolved = request.clone();
    resolved.post = Some(post);
    let mut result = post_setup_unchecked(document, &resolved)?;
    crate::machine::check_compensation_contract(document, &result.program, result.dialect)?;
    if result.dialect == PostDialect::Siemens828d {
        crate::compensation::validate_posted_intersections(document, &result.program)?;
    }
    let machine = &document
        .setup(request.setup_id)
        .unwrap()
        .machine
        .as_ref()
        .unwrap()
        .profile;
    result.warnings.insert(0, format!(
        "Target: {} (profile {} revision {}). Controller/post checks do not certify this machine. CAM/NC simulation checks tools, stock and model only; travel, fixtures, holders, PLC and channel synchronization are not verified.",
        machine.name, machine.id, machine.revision,
    ));
    Ok(result)
}

/// Formatting seam for post unit tests. External callers must use post_setup;
/// this renderer alone does not certify a machine/controller combination.
pub(crate) fn post_setup_unchecked(
    document: &CamDocumentDto,
    request: &CamPostRequestDto,
) -> Result<CamPostResultDto, CamPlanError> {
    let program = plan_setup(document, request.setup_id)?;
    if document.setup(request.setup_id).is_none() {
        return Err(CamPlanError(format!(
            "CAM setup {} does not exist",
            request.setup_id
        )));
    }
    let post = request
        .post
        .clone()
        .unwrap_or_else(|| document.post_defaults.clone());
    if let Some(profile) = &post.siemens_828d {
        profile.validate().map_err(CamPlanError)?;
    }
    let dialect = post.dialect;
    let units = PostUnits {
        units: document.units,
        contains_arcs: program
            .commands
            .iter()
            .any(|command| matches!(command, CamCommandDto::Circular { .. })),
    };
    let name = request
        .program_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(&program.name);
    let tool_calls = if dialect == PostDialect::Siemens828d {
        crate::tool_calls::siemens_program_tool_calls(
            document,
            document.setup(request.setup_id).unwrap(),
            &program,
            post.tool_call_mode,
        )?
    } else {
        BTreeMap::new()
    };
    let nc = if dialect.requires_machine_retract() {
        brand_posts::render(document, &program, &post, name, units)?
    } else {
        render_program(
            &program,
            dialect,
            name,
            post.sequence_numbers,
            post.siemens_828d.as_ref(),
            &tool_calls,
            units,
        )?
    };
    let mut warnings = program.warnings.clone();
    if document.units == CamUnits::Inches {
        warnings.push(
            "Posted in inches (G20 for ISO-style output / G700 for Siemens native); confirm the control's dimensional mode and every offset before running."
                .to_string(),
        );
    }
    if program.work_offsets.len() > 1 {
        warnings.push(format!(
            "Program repeats the toolpath under {} ({} parts); verify every fixture offset on the control.",
            program
                .work_offsets
                .iter()
                .map(|offset| offset.code())
                .collect::<Vec<_>>()
                .join(", "),
            program.work_offsets.len(),
        ));
    }
    warnings.push(match dialect {
        PostDialect::Grbl => {
            "GRBL output pauses for manual tool changes; confirm the tool and re-zero policy before resuming."
                .to_string()
        }
        PostDialect::LinuxCnc => {
            "LinuxCNC output assumes standard G54, M6, spindle, and coolant mappings."
                .to_string()
        }
        PostDialect::Fanuc => {
            "Generic Fanuc output is a starting point only; verify controller options, tool-change position, and M-codes."
                .to_string()
        }
        PostDialect::Haas | PostDialect::Mitsubishi | PostDialect::Mazak | PostDialect::Syntec | PostDialect::Okuma | PostDialect::Heidenhain | PostDialect::HermleHeidenhain => {
            "Fixed 3-axis starter post, not a commissioned machine kit. Uses project-library tool numbers/names and the configured machine retract Z. Verify work offsets, tool-length registers, coolant and tool-change behavior; no builder macros or rotary motion are emitted.".into()
        }
        PostDialect::Siemens828d => {
            if let Some(profile) = post.siemens_828d.as_ref() {
                format!(
                    "Siemens 828D native output uses G0 SUPA Z{} D0 for machine-coordinate retracts; verify that position, D{}, tool calls, and M-codes on the control before running.",
                    units.len(profile.supa_retract_z),
                    profile.tool_length_offset,
                )
            } else {


                "Siemens 828D native output requires a confirmed machine profile."
                    .to_string()
            }
        }
    });
    if dialect.requires_machine_retract() {
        warnings.push("Approaches use XY while retracted, then Z with tool-length compensation. Work-offset changes retract first. Confirm the machine-coordinate retract and M6/TOOL CALL behavior. Drilling cycles are expanded into explicit motion. Optional tool-change stops and preloading are off.".into());
    }
    if matches!(
        dialect,
        PostDialect::Heidenhain | PostDialect::HermleHeidenhain
    ) {
        warnings.push("Setup offsets 1–6 select preset-table rows 1–6 using cycle 247. Helices are chorded to a 0.002 mm tolerance. The NC simulator does not yet interpret conversational programs; CAM stock evidence is not NC replay.".into());
    }
    if dialect == PostDialect::Okuma {
        warnings.push("Setup offsets 1–6 use G15 H1–H6; G56 H uses the project tool number. G16 H0 retracts in the machine frame. The NC simulator does not yet interpret OSP; do not replay this as ISO G-code.".into());
    }
    if dialect == PostDialect::Siemens828d {
        warnings.push("Start this fixed-axis program from a reviewed reset state in Siemens native mode, without active modal cycles, programmable frames, TCP or cutter compensation. G700/G710 require the extended units option; arbitrary block restart is not verified.".into());
        warnings.push("After M6, position XY while retracted, then approach workpiece Z. Confirm that M6/PLC leaves the tool at a safe traversing height; machine and fixture travel are not simulated. Work-offset changes also retract to the configured SUPA Z before traversing.".into());
        warnings.push("Radius compensation uses explicit G451 intersection corners and NORM approach/retract. The supported outside turn is at most 90 degrees; verify the control's machine-data corner-switch limit and full-radius D geometry. G450 transition circles are not simulated.".into());
        warnings.push(if let Some(name) = post.siemens_828d.as_ref().and_then(|s| s.spindle_stop_subprogram.as_deref()) {
            format!("Private spindle-stop subprogram {name} is called immediately before each explicit M5. Its controller-resident body is not included, executed or verified by Limo CAD. NC replay is unavailable for these calls; CAM simulation excludes their motion, time and side effects. Commission the subprogram and its machine state before use.")
        } else {
            "The standard Siemens profile emits no shop-specific spindle slowdown or other custom machine macro. Add such behavior only through an explicit machine profile after validation.".into()
        });
        warnings.push(
            "ATC style is descriptive only; generated positioning and next-tool preloading follow separate, explicit machine-profile settings."
                .to_string(),
        );
        if let Some(profile) = post.siemens_828d.as_ref() {
            warnings.push(match profile.tool_change_positioning {
                Siemens828dToolChangePositioning::SupaZ => format!(
                    "Before later tool changes, the post commands G0 SUPA Z{} D0 and then M6.",
                    units.len(profile.supa_retract_z)
                ),
                Siemens828dToolChangePositioning::ControllerManaged => {
                    "Before later tool changes, the post emits no machine-axis station move and assumes the machine builder's M6/PLC cycle owns positioning."
                        .to_string()
                }
                Siemens828dToolChangePositioning::SupaZThenXy => format!(
                    "Before later tool changes, the post commands SUPA Z first and then the verified machine station X{} Y{} before M6.",
                    units.len(profile.station_x.unwrap_or_default()),
                    units.len(profile.station_y.unwrap_or_default())
                ),
            });
            warnings.push(if profile.preload_next_tool {
                "Next-tool T preloading is enabled. The post emits the next T call immediately after M6/D and wraps the final preload to the first program tool when different; verify that early T calls cannot move this magazine unsafely."
                    .to_string()
            } else {
                "Next-tool T preloading is disabled. Every executable T call is emitted only for the M6 immediately following it."
                    .to_string()
            });
        }
    }
    Ok(CamPostResultDto {
        program,
        dialect,
        extension: dialect.extension().to_string(),
        nc,
        warnings,
    })
}

fn render_program(
    program: &CamProgramDto,
    dialect: PostDialect,
    program_name: &str,
    sequence_numbers: bool,
    siemens_profile: Option<&Siemens828dPostConfigDto>,
    tool_calls: &BTreeMap<u64, String>,
    units: PostUnits,
) -> Result<String, CamPlanError> {
    if dialect == PostDialect::Siemens828d {
        let profile = siemens_profile.ok_or_else(|| {
            CamPlanError(
                "Siemens 828D posting requires an explicitly confirmed machine profile with a safe SUPA retract Z"
                    .to_string(),
            )
        })?;
        return render_siemens828d_program(
            program,
            program_name,
            sequence_numbers,
            profile,
            tool_calls,
            units,
        );
    }
    if !matches!(dialect, PostDialect::Grbl | PostDialect::LinuxCnc) {
        return Err(CamPlanError(
            "This controller requires its dedicated brand renderer".into(),
        ));
    }
    let mut writer = NcWriter::new(sequence_numbers, 10);
    let mut position: Option<Point3Dto> = None;

    let mut comp_words: Option<String> = None;
    let mut active_tool_number: Option<u32> = None;
    for command in &program.commands {
        match command {
            CamCommandDto::ProgramStart { .. } => {
                writer.comment(program_name);
                writer.comment("GENERATED BY LIMO CAD - VERIFY AND DRY RUN");
                writer.block(&format!("G90 G17 {} G40 G49 G80", units.iso_mode_word()));
                writer.block("M5");
                writer.block("M9");
            }
            CamCommandDto::WorkOffset { offset } => {
                writer.block(offset.code());
            }
            CamCommandDto::SectionStart { name, .. } => writer.comment(name),
            CamCommandDto::ToolChange {
                tool_number,
                tool_name,
                ..
            } => {
                let Some(tool_number) = tool_number else {
                    return Err(CamPlanError(format!(
                        "tool '{tool_name}' has no tool number, but this post calls tools numerically; assign a number in the tool library or post with a name-capable control"
                    )));
                };
                active_tool_number = Some(*tool_number);
                match dialect {
                    PostDialect::Grbl => {
                        writer.block("M5");
                        writer.comment(&format!("MANUAL TOOL CHANGE: T{tool_number} {tool_name}"));
                        writer.block("M0");
                    }
                    _ => {
                        writer.comment(tool_name);
                        writer.block(&format!("T{tool_number} M6"));
                    }
                }
            }
            CamCommandDto::Spindle { direction, rpm } => match direction {
                SpindleDirection::Off => writer.block("M5"),
                SpindleDirection::Clockwise => writer.block(&format!("S{rpm} M3")),
                SpindleDirection::Counterclockwise => writer.block(&format!("S{rpm} M4")),
            },
            CamCommandDto::Coolant { mode } => match mode {
                CoolantMode::Off => writer.block("M9"),
                CoolantMode::Mist => writer.block("M7"),
                CoolantMode::Flood => writer.block("M8"),
            },
            CamCommandDto::SetPosition { .. } => {
                return Err(CamPlanError(
                    "workpiece position resets are simulator-only and cannot be posted".to_string(),
                ));
            }
            CamCommandDto::Rapid { to } => {
                if position.is_none() {
                    writer.block(&format!("G0 Z{}", units.len(to.z)));
                    writer.block(&format!("G0 X{} Y{}", units.len(to.x), units.len(to.y)));
                } else {
                    writer.block(&format!(
                        "G0 X{} Y{} Z{}",
                        units.len(to.x),
                        units.len(to.y),
                        units.len(to.z)
                    ));
                }
                position = Some(*to);
            }
            CamCommandDto::Linear { to, feed } => {
                let words = comp_words
                    .take()
                    .map(|pending| format!("{pending} "))
                    .unwrap_or_default();
                writer.block(&format!(
                    "G1 {words}X{} Y{} Z{} F{}",
                    units.len(to.x),
                    units.len(to.y),
                    units.len(to.z),
                    units.feed(*feed)
                ));
                position = Some(*to);
            }
            CamCommandDto::Circular {
                clockwise,
                center,
                to,
                feed,
                ..
            } => {
                let from = position.ok_or_else(|| {
                    CamPlanError("a circular post record needs a known start position".to_string())
                })?;

                writer.block(&format!(
                    "{} X{} Y{} Z{} I{} J{} F{}",
                    if *clockwise { "G2" } else { "G3" },
                    units.len(to.x),
                    units.len(to.y),
                    units.len(to.z),
                    units.len(center.x - from.x),
                    units.len(center.y - from.y),
                    units.feed(*feed)
                ));
                position = Some(*to);
            }
            CamCommandDto::Dwell { seconds } => {
                writer.block(&format!("G4 P{}", coordinate(*seconds)));
            }
            CamCommandDto::CutterCompensationOn { left } => {
                if dialect == PostDialect::Grbl {
                    return Err(CamPlanError(
                        "GRBL has no cutter radius compensation (G41/G42); switch the contour operation's compensation mode to in software, or post for a control with radius compensation"
                            .to_string(),
                    ));
                }
                let Some(tool_number) = active_tool_number else {
                    return Err(CamPlanError(
                        "cutter compensation needs an active numbered tool for the diameter register"
                            .to_string(),
                    ));
                };
                comp_words = Some(format!(
                    "{} D{}",
                    if *left { "G41" } else { "G42" },
                    tool_number
                ));
            }
            CamCommandDto::CutterCompensationOff => {
                comp_words = Some("G40".to_string());
            }
            CamCommandDto::SectionEnd => {}
            CamCommandDto::ProgramEnd => {
                writer.block("M30");
            }
        }
    }
    Ok(format!("{}\n", writer.lines.join("\n")))
}

/// Native fixed-axis SINUMERIK program calibrated against an
/// operator-provided, known-good 828D MPF and public Siemens programming
/// syntax. Standard profiles contain no builder macros. Private versioned
/// profiles may request a disclosed, controller-resident spindle-stop call.
fn render_siemens828d_program(
    program: &CamProgramDto,
    program_name: &str,
    sequence_numbers: bool,
    profile: &Siemens828dPostConfigDto,
    tool_calls: &BTreeMap<u64, String>,
    units: PostUnits,
) -> Result<String, CamPlanError> {
    if profile.tool_change_positioning == Siemens828dToolChangePositioning::SupaZThenXy
        && (profile.station_x.is_none() || profile.station_y.is_none())
    {
        return Err(CamPlanError(
            "Siemens 828D fixed-station positioning requires machine X and Y coordinates"
                .to_string(),
        ));
    }
    let mut writer = NcWriter::new(sequence_numbers, 1);
    let mut position: Option<Point3Dto> = None;
    let mut work_offset = WorkOffset::G54;
    let mut pending_section: Option<String> = None;
    let mut tool_change_count = 0_u32;

    let mut comp_words: Option<&'static str> = None;

    for (index, command) in program.commands.iter().enumerate() {
        match command {
            CamCommandDto::ProgramStart { .. } => {
                writer.raw(&format!("; %_N_{}_MPF", siemens_program_name(program_name)));
                writer.raw("; GENERATED BY LIMO CAD - VERIFY, SIMULATE, AND DRY RUN");
                writer.block(&format!("G17 {} G90 G94", units.siemens_mode_word()));
                writer.block("G64");
                writer.block(&siemens_supa_retract(profile, units));
            }
            CamCommandDto::WorkOffset { offset } => {
                if *offset != work_offset && position.is_some() {
                    writer.block(&siemens_supa_retract(profile, units));
                    writer.block(&format!("D{}", profile.tool_length_offset));
                    position = None;
                }
                work_offset = *offset;
                writer.block(work_offset.code());
            }
            CamCommandDto::SectionStart { name, .. } => {
                let changes_tool = program.commands[index + 1..]
                    .iter()
                    .take_while(|next| !matches!(next, CamCommandDto::SectionEnd))
                    .any(|next| matches!(next, CamCommandDto::ToolChange { .. }));
                if changes_tool {
                    pending_section = Some(name.clone());
                } else {
                    write_siemens_section(&mut writer, name, work_offset);
                }
            }
            CamCommandDto::ToolChange {
                tool_id, tool_name, ..
            } => {
                if tool_change_count > 0
                    && write_siemens_tool_change_positioning(&mut writer, profile, units)?
                {
                    writer.block(&format!("D{}", profile.tool_length_offset));
                }
                if let Some(name) = pending_section.take() {
                    writer.raw("");
                    write_siemens_message(&mut writer, &name);
                }
                if tool_change_count > 0 && profile.optional_stop_on_tool_change {
                    writer.block("M1");
                }
                let tool_call = tool_calls.get(tool_id).ok_or_else(|| {
                    CamPlanError(format!(
                        "No confirmed controller tool call for '{tool_name}'"
                    ))
                })?;
                writer.raw(&format!(
                    "; {} {}",
                    tool_call,
                    siemens_plain_text(tool_name)
                ));
                writer.block(tool_call);
                writer.block("M6");
                writer.block(&format!("D{}", profile.tool_length_offset));
                if profile.preload_next_tool {
                    if let Some(next_call) =
                        siemens_next_preload_call(program, index, tool_call, tool_calls)
                    {
                        writer.block(&next_call);
                    }
                }
                writer.block("G17 G90 G94");
                writer.block(work_offset.code());
                position = None;
                tool_change_count = tool_change_count.saturating_add(1);
            }
            CamCommandDto::Spindle { direction, rpm } => match direction {
                SpindleDirection::Off => {
                    if let Some(name) = &profile.spindle_stop_subprogram {
                        writer.block(name);
                    }
                    writer.block("M5");
                }
                SpindleDirection::Clockwise => writer.block(&format!("S{rpm} M3")),
                SpindleDirection::Counterclockwise => writer.block(&format!("S{rpm} M4")),
            },
            CamCommandDto::Coolant { mode } => match mode {
                CoolantMode::Off => writer.block("M9"),
                CoolantMode::Mist => writer.block("M7"),
                CoolantMode::Flood => writer.block("M8"),
            },
            CamCommandDto::SetPosition { .. } => {
                return Err(CamPlanError(
                    "workpiece position resets are simulator-only and cannot be posted".to_string(),
                ));
            }
            CamCommandDto::Rapid { to } => {
                if position.is_none() {
                    write_siemens_initial_position(&mut writer, *to, None, units);
                } else {
                    writer.block(&format!(
                        "G0 X{} Y{} Z{}",
                        units.siemens_len(to.x),
                        units.siemens_len(to.y),
                        units.siemens_len(to.z)
                    ));
                }
                position = Some(*to);
            }
            CamCommandDto::Linear { to, feed } => {
                if position.is_none() {
                    if comp_words.is_some() {
                        return Err(CamPlanError("Cutter compensation requires an established workpiece approach position".into()));
                    }

                    write_siemens_initial_position(&mut writer, *to, Some(*feed), units);
                    position = Some(*to);
                    continue;
                }
                let words = comp_words
                    .take()
                    .map(|pending| format!("{pending} "))
                    .unwrap_or_default();
                writer.block(&format!(
                    "G1 {words}X{} Y{} Z{} F{}",
                    units.siemens_len(to.x),
                    units.siemens_len(to.y),
                    units.siemens_len(to.z),
                    units.feed(*feed)
                ));
                position = Some(*to);
            }
            CamCommandDto::Circular {
                clockwise,
                center,
                to,
                feed,
                ..
            } => {
                let from = position.ok_or_else(|| {
                    CamPlanError("a circular post record needs a known start position".to_string())
                })?;

                writer.block(&format!(
                    "{} X{} Y{} Z{} I{} J{} F{}",
                    if *clockwise { "G2" } else { "G3" },
                    units.siemens_len(to.x),
                    units.siemens_len(to.y),
                    units.siemens_len(to.z),
                    units.siemens_len(center.x - from.x),
                    units.siemens_len(center.y - from.y),
                    units.feed(*feed)
                ));
                position = Some(*to);
            }
            CamCommandDto::Dwell { seconds } => {
                writer.block(&format!("G4 F{}", siemens_coordinate(*seconds)));
            }
            CamCommandDto::CutterCompensationOn { left } => {
                comp_words = Some(if *left {
                    "G41 NORM G451"
                } else {
                    "G42 NORM G451"
                });
            }
            CamCommandDto::CutterCompensationOff => {
                comp_words = Some("G40");
            }
            CamCommandDto::SectionEnd => {
                pending_section = None;
            }
            CamCommandDto::ProgramEnd => {
                writer.block(&siemens_supa_retract(profile, units));
                writer.block(&format!("D{}", profile.tool_length_offset));
                writer.block("M30");
            }
        }
    }

    Ok(format!("{}\n", writer.lines.join("\n")))
}

fn write_siemens_section(writer: &mut NcWriter, name: &str, work_offset: WorkOffset) {
    writer.raw("");
    write_siemens_message(writer, name);
    writer.block("G17 G90 G94");
    writer.block(work_offset.code());
}

fn write_siemens_message(writer: &mut NcWriter, name: &str) {
    writer.raw(&format!("MSG (\"{}\")", siemens_plain_text(name)));
}

fn siemens_supa_retract(profile: &Siemens828dPostConfigDto, units: PostUnits) -> String {
    format!("G0 SUPA Z{} D0", units.siemens_len(profile.supa_retract_z))
}

fn write_siemens_initial_position(
    writer: &mut NcWriter,
    to: Point3Dto,
    feed: Option<f64>,
    units: PostUnits,
) {
    let motion = if feed.is_some() { "G1" } else { "G0" };
    let f = feed
        .map(|value| format!(" F{}", units.feed(value)))
        .unwrap_or_default();
    writer.block(&format!(
        "{motion} X{} Y{}{f}",
        units.siemens_len(to.x),
        units.siemens_len(to.y)
    ));
    writer.block(&format!("{motion} Z{}{f}", units.siemens_len(to.z)));
}

/// Emit only the positioning behavior explicitly confirmed in the machine
/// profile. Physical magazine style is intentionally not consulted here.
fn write_siemens_tool_change_positioning(
    writer: &mut NcWriter,
    profile: &Siemens828dPostConfigDto,
    units: PostUnits,
) -> Result<bool, CamPlanError> {
    match profile.tool_change_positioning {
        Siemens828dToolChangePositioning::SupaZ => {
            writer.block(&siemens_supa_retract(profile, units));
            Ok(true)
        }
        Siemens828dToolChangePositioning::ControllerManaged => {
            writer.raw("; M6/PLC CONTROLS TOOL-CHANGE POSITIONING");
            Ok(false)
        }
        Siemens828dToolChangePositioning::SupaZThenXy => {
            let (Some(x), Some(y)) = (profile.station_x, profile.station_y) else {
                return Err(CamPlanError(
                    "Siemens 828D fixed-station positioning requires machine X and Y coordinates"
                        .to_string(),
                ));
            };
            writer.block(&siemens_supa_retract(profile, units));
            writer.block(&format!(
                "G0 SUPA X{} Y{}",
                units.siemens_len(x),
                units.siemens_len(y)
            ));
            Ok(true)
        }
    }
}

/// Return the next actual tool change call, wrapping to the program's first
/// tool so repeated program runs can stage it too. A one-tool program never
/// emits a redundant/self preload.
fn siemens_next_preload_call(
    program: &CamProgramDto,
    current_index: usize,
    current_call: &str,
    tool_calls: &BTreeMap<u64, String>,
) -> Option<String> {
    let find_call = |commands: &[CamCommandDto]| {
        commands.iter().find_map(|command| match command {
            CamCommandDto::ToolChange { tool_id, .. } => tool_calls.get(tool_id).cloned(),
            _ => None,
        })
    };
    find_call(&program.commands[current_index + 1..])
        .or_else(|| find_call(&program.commands[..current_index]))
        .filter(|call| call != current_call)
}

fn siemens_program_name(value: &str) -> String {
    let mut result = String::new();
    let mut previous_separator = false;
    for character in value.trim().chars() {
        let normalized = if character.is_ascii_alphanumeric() {
            character.to_ascii_uppercase()
        } else {
            '_'
        };
        if normalized == '_' {
            if previous_separator || result.is_empty() {
                continue;
            }
            previous_separator = true;
        } else {
            previous_separator = false;
        }
        result.push(normalized);
        if result.len() >= 24 {
            break;
        }
    }
    while result.ends_with('_') {
        result.pop();
    }
    if let Some(without_suffix) = result.strip_suffix("_MPF") {
        result = without_suffix.trim_end_matches('_').to_string();
    }
    if result.is_empty() {
        "PROGRAM".to_string()
    } else {
        result
    }
}

fn siemens_plain_text(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '"' => '\'',
            '\r' | '\n' | ';' => ' ',
            character if character.is_ascii_graphic() || character == ' ' => character,
            _ => '?',
        })
        .take(80)
        .collect::<String>()
        .trim()
        .to_string()
}

struct NcWriter {
    lines: Vec<String>,
    sequence_numbers: bool,
    next_sequence: u32,
    sequence_increment: u32,
}

impl NcWriter {
    fn new(sequence_numbers: bool, sequence_increment: u32) -> Self {
        Self {
            lines: Vec::new(),
            sequence_numbers,
            next_sequence: 10,
            sequence_increment: sequence_increment.max(1),
        }
    }

    fn raw(&mut self, line: &str) {
        self.lines.push(line.to_string());
    }

    fn comment(&mut self, text: &str) {
        let clean = text
            .chars()
            .map(|character| match character {
                '(' | ')' | '\r' | '\n' => ' ',
                other => other,
            })
            .collect::<String>();
        self.raw(&format!("({})", clean.trim()));
    }

    fn block(&mut self, words: &str) {
        if self.sequence_numbers {
            self.lines.push(format!("N{} {words}", self.next_sequence));
            self.next_sequence = self.next_sequence.saturating_add(self.sequence_increment);
        } else {
            self.lines.push(words.to_string());
        }
    }
}

fn coordinate(value: f64) -> String {
    decimal(value, 3)
}

fn siemens_coordinate(value: f64) -> String {
    decimal(value, 5)
}

fn decimal(value: f64, precision: usize) -> String {
    let zero_threshold = 0.5 * 10_f64.powi(-(precision as i32));
    let normalized = if value.abs() < zero_threshold {
        0.0
    } else {
        value
    };
    let mut formatted = format!("{normalized:.precision$}");
    while formatted.contains('.') && formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    formatted
}

fn feedrate(value: f64) -> String {
    coordinate(value)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use super::post_setup_unchecked as post_setup;
    use crate::model::{
        CamOperationDto, CamPostConfigDto, CamSetupDto, CamToolDto, CamToolKind, CamUnits,
        CompensationMode, ContourCompensation, CuttingParametersDto, DrillCycle, MillingDirection,
        Point2Dto, Rect2Dto, Siemens828dAtcStyle, StockBoxDto, ThreadHand, WcsOriginSpecDto,
        WorkCoordinateSystemDto, WorkOffset,
    };

    pub(crate) fn document(dialect: PostDialect) -> CamDocumentDto {
        let mut document = CamDocumentDto {
            load_warnings: Vec::new(),
            toolpath_generations: Vec::new(),
            height_expressions: Vec::new(),
            linking: Vec::new(),
            setups: vec![CamSetupDto {
                id: 1,
                name: "Fixture A".into(),
                wcs: WorkCoordinateSystemDto::default(),
                wcs_origin: WcsOriginSpecDto::Explicit,
                work_offset: WorkOffset::G54,
                work_offset_count: 1,
                stock_spec: crate::model::CamStockSpecDto::LegacyBox,
                resolved_stock: crate::model::CamResolvedStockDto::Box,
                stock: StockBoxDto {
                    min: Point3Dto::new(0.0, 0.0, -10.0),
                    max: Point3Dto::new(20.0, 20.0, 0.0),
                },
                stock_model_box: None,
                body_ids: vec![],
                machine: None,
                legacy_clearance_z: None,
                legacy_retract_z: None,
                operations: vec![CamOperationDto::Face {
                    id: 1,
                    name: "Face top".into(),
                    enabled: true,
                    tool_id: 1,
                    bounds: Rect2Dto {
                        min: Point2Dto::new(0.0, 0.0),
                        max: Point2Dto::new(20.0, 20.0),
                    },
                    top_z: 0.0,
                    target_z: -1.0,
                    step_over: 3.0,
                    step_down: 1.0,
                    safe_distance: 5.0,
                    direction: crate::model::FaceDirection::BothWays,
                    clearance_z: 8.0,
                    retract_z: 2.0,
                    feed_height_z: 1.0,
                    cutting: CuttingParametersDto {
                        spindle_rpm: 10_000,
                        feed_xy: 600.0,
                        feed_z: 150.0,
                        coolant: CoolantMode::Flood,
                    },
                }],
            }],
            active_setup_id: Some(1),
            tools: vec![CamToolDto {
                id: 1,
                number: Some(1),
                name: "6 mm flat".into(),
                kind: CamToolKind::FlatEndMill,
                diameter: 6.0,
                flute_length: 15.0,
                overall_length: 50.0,
                center_cutting: true,
                flute_count: 4,
                point_angle_degrees: None,
                corner_radius: None,
                corner_chamfer: None,
                cutting: CuttingParametersDto::default(),
                cutting_presets: vec![],
                maximum_axial_depth: None,
                default_step_down: None,
                default_step_over: None,
            }],
            units: CamUnits::Millimeters,
            post_defaults: CamPostConfigDto {
                machine_retract_z: Some(0.0),
                tool_call_mode: crate::CamToolCallMode::Automatic,
                dialect,
                program_number: Some(42),
                sequence_numbers: false,
                siemens_828d: (dialect == PostDialect::Siemens828d)
                    .then(Siemens828dPostConfigDto::default),
            },
            next_setup_id: 2,
            next_operation_id: 2,
            next_tool_id: 2,
        };
        if dialect == PostDialect::Siemens828d {
            bind_test_names(&mut document, &[(1, "6_MM_FLAT")]);
        }
        document
    }

    /// Give fixtures exact library names; no separate mapping is required.
    pub(crate) fn bind_test_names(doc: &mut CamDocumentDto, names: &[(u64, &str)]) {
        let machine = doc.setups[0].machine.get_or_insert_with(|| {
            crate::CamMachineAssignmentDto::three_axis(doc.post_defaults.clone())
        });
        machine.tool_calls = names
            .iter()
            .map(|(tool_id, name)| crate::CamMachineToolBindingDto {
                tool_id: *tool_id,
                call: crate::CamMachineToolCallDto::Name {
                    name: (*name).into(),
                },
            })
            .collect();
        for (id, name) in names {
            if let Some(tool) = doc.tools.iter_mut().find(|t| t.id == *id) {
                tool.name = (*name).into();
                tool.number = None;
            }
        }
    }

    fn two_tool_siemens_document() -> CamDocumentDto {
        let mut source = document(PostDialect::Siemens828d);
        source.tools.push(CamToolDto {
            id: 2,
            number: Some(19),
            name: "5 mm drill".into(),
            kind: CamToolKind::Drill,
            diameter: 5.0,
            flute_length: 25.0,
            overall_length: 55.0,
            center_cutting: true,
            flute_count: 2,
            point_angle_degrees: None,
            corner_radius: None,
            corner_chamfer: None,
            cutting: CuttingParametersDto::default(),
            cutting_presets: vec![],
            maximum_axial_depth: None,
            default_step_down: None,
            default_step_over: None,
        });
        source.setups[0].operations.push(CamOperationDto::Drill {
            id: 2,
            name: "Drill holes".into(),
            enabled: true,
            tool_id: 2,
            points: vec![Point2Dto::new(10.0, 10.0)],
            holes: Vec::new(),
            drill_tip_through: false,
            breakthrough_depth: 0.0,
            top_z: 0.0,
            bottom_z: -3.0,
            retract_z: 2.0,
            clearance_z: 8.0,
            feed_height_z: 1.0,
            peck_depth: None,
            dwell_seconds: 0.25,
            cycle: DrillCycle::Drill,
            peck_retract: None,
            thread_pitch: None,
            floating_tap_holder: false,
            feed_out: None,
            cutting: CuttingParametersDto {
                spindle_rpm: 2_500,
                feed_xy: 300.0,
                feed_z: 100.0,
                coolant: CoolantMode::Flood,
            },
        });
        source.next_tool_id = 3;
        source.next_operation_id = 3;
        bind_test_names(&mut source, &[(1, "6_MM_FLAT"), (2, "5_MM_DRILL")]);
        source
    }

    fn thread_document(dialect: PostDialect) -> CamDocumentDto {
        let mut source = document(dialect);
        source.tools.push(CamToolDto {
            id: 7,
            number: Some(61),
            name: "4.8 mm thread mill".into(),
            kind: CamToolKind::ThreadMill,
            diameter: 4.8,
            flute_length: 12.0,
            overall_length: 58.0,
            center_cutting: false,
            flute_count: 4,
            point_angle_degrees: None,
            corner_radius: None,
            corner_chamfer: None,
            cutting: CuttingParametersDto::default(),
            cutting_presets: vec![],
            maximum_axial_depth: None,
            default_step_down: None,
            default_step_over: None,
        });
        source.setups[0].operations = vec![CamOperationDto::Thread {
            id: 3,
            name: "Thread M6".into(),
            enabled: true,
            tool_id: 7,
            points: vec![Point2Dto::new(10.0, 10.0)],
            holes: Vec::new(),
            top_z: 0.0,
            bottom_z: -6.0,
            pitch: 1.0,
            major_diameter: 6.0,
            minor_diameter: 5.035,
            hand: ThreadHand::Right,
            direction: MillingDirection::Climb,
            radial_passes: 1,
            step_over: None,
            clearance_z: 8.0,
            retract_z: 2.0,
            feed_height_z: 1.0,
            cutting: CuttingParametersDto {
                spindle_rpm: 8_000,
                feed_xy: 500.0,
                feed_z: 150.0,
                coolant: CoolantMode::Flood,
            },
        }];
        source.next_tool_id = 8;
        if dialect == PostDialect::Siemens828d {
            bind_test_names(&mut source, &[(7, "ThreadMill48")]);
        }
        source.next_operation_id = 4;
        source
    }

    #[test]
    fn helical_thread_arcs_post_with_a_z_word() {
        for dialect in [PostDialect::Fanuc, PostDialect::Siemens828d] {
            let posted = post_setup(
                &thread_document(dialect),
                &CamPostRequestDto {
                    setup_id: 1,
                    post: None,
                    program_name: None,
                },
            )
            .unwrap();
            let arc_blocks: Vec<&str> = posted
                .nc
                .lines()
                .filter(|line| line.starts_with("G3 "))
                .collect();
            assert!(
                !arc_blocks.is_empty(),
                "{dialect:?} right-hand climb thread program needs G3 arc blocks"
            );

            assert!(
                arc_blocks
                    .iter()
                    .all(|line| line.contains(" Z") && line.contains(" I") && line.contains(" J")),
                "{dialect:?} helical arcs keep their Z word: {arc_blocks:?}"
            );
        }
    }

    #[test]
    fn grbl_post_is_metric_absolute_and_pauses_for_manual_tool_change() {
        let posted = post_setup(
            &document(PostDialect::Grbl),
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: Some("FACE SAMPLE".into()),
            },
        )
        .unwrap();
        assert!(posted.nc.contains("G90 G17 G21 G40 G49 G80"));
        assert!(posted.nc.contains("M5\nM9\nG54"));
        assert!(posted.nc.contains("(MANUAL TOOL CHANGE: T1 6 mm flat)"));
        assert!(posted.nc.contains("M0"));
        assert!(posted.nc.contains("G1 X"));
        let z_clearance = posted.nc.find("G0 Z8").unwrap();

        let xy_position = posted.nc.find("G0 X-8 Y2.5").unwrap();
        assert!(z_clearance < xy_position);
        assert!(posted.nc.ends_with("M30\n"));
    }

    #[test]
    fn fanuc_post_wraps_program_and_uses_program_number() {
        let posted = post_setup(
            &document(PostDialect::Fanuc),
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .unwrap();
        assert!(posted.nc.starts_with("%\nO0042\n"));
        assert!(posted.nc.contains("T1 M6"));
        assert!(posted.nc.ends_with("M30\n%\n"));
    }

    pub(crate) fn contour_document(dialect: PostDialect) -> CamDocumentDto {
        let mut source = document(dialect);
        source.setups[0].operations = vec![CamOperationDto::Contour2d {
            id: 1,
            name: "Boss wall".into(),
            enabled: true,
            tool_id: 1,

            path: vec![
                Point2Dto::new(5.0, 5.0),
                Point2Dto::new(15.0, 5.0),
                Point2Dto::new(15.0, 15.0),
                Point2Dto::new(5.0, 15.0),
            ],
            closed: true,
            top_z: 0.0,
            bottom_z: -2.0,
            step_down: 2.0,
            compensation: ContourCompensation::Outside,
            compensation_mode: CompensationMode::InControl,
            lead_in: 5.0,
            lead_out: 5.0,
            lead_arc_radius: None,
            direction: MillingDirection::Climb,
            roughing_passes: 1,
            roughing_step_over: None,
            finishing_pass: false,
            finish_allowance: 0.0,
            finish_feed: None,
            spring_pass: false,
            chain_ref: None,
            clearance_z: 8.0,
            retract_z: 2.0,
            feed_height_z: 1.0,
            cutting: CuttingParametersDto {
                spindle_rpm: 10_000,
                feed_xy: 600.0,
                feed_z: 150.0,
                coolant: CoolantMode::Flood,
            },
        }];
        source
    }

    #[test]
    fn in_control_contour_posts_g41_with_the_tool_register_and_cancels_with_g40() {
        let posted = post_setup(
            &contour_document(PostDialect::Fanuc),
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .unwrap();

        assert!(
            posted.nc.contains("G1 G41 D1 X5 Y5 Z-2 F600"),
            "{}",
            posted.nc
        );
        assert!(posted.nc.contains("G1 G40 X0 Y2 Z-2 F600"), "{}", posted.nc);
    }

    #[test]
    fn siemens_post_applies_radius_compensation_without_a_register_word() {
        let posted = post_setup(
            &contour_document(PostDialect::Siemens828d),
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: Some("comp".into()),
            },
        )
        .unwrap();
        assert!(
            posted.nc.contains("G1 G41 NORM G451 X5 Y5 Z-2 F600"),
            "{}",
            posted.nc
        );
        assert!(posted.nc.contains("G1 G40 X0 Y2 Z-2 F600"), "{}", posted.nc);
    }

    #[test]
    fn grbl_post_fails_closed_on_machine_cutter_compensation() {
        let error = post_setup(
            &contour_document(PostDialect::Grbl),
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .unwrap_err();
        assert!(error.0.contains("no cutter radius compensation"));
    }

    #[test]
    fn siemens_native_post_matches_the_validated_828d_envelope_without_shop_macros() {
        let mut source = document(PostDialect::Siemens828d);
        source.setups[0].work_offset = WorkOffset::G58;
        source.post_defaults.sequence_numbers = true;
        let posted = post_setup(
            &source,
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: Some("61047097 op1 v4".to_string()),
            },
        )
        .unwrap();

        assert!(posted.nc.starts_with("; %_N_61047097_OP1_V4_MPF\n"));
        assert!(posted.nc.contains("N10 G17 G710 G90 G94\nN11 G64"));
        assert!(posted.nc.contains("N13 G58\n"));
        assert!(posted.nc.contains("G0 SUPA Z0 D0"));
        assert!(posted.nc.contains("MSG (\"Face top\")"));

        assert!(posted.nc.contains("T=\"6_MM_FLAT\"\n"));
        assert!(posted.nc.contains("M6\n"));
        assert!(posted.nc.contains("D1\n"));
        assert!(posted.nc.contains("S10000 M3"));
        assert!(posted.nc.contains("M8"));
        assert!(posted.nc.ends_with("M30\n"));
        assert!(posted
            .nc
            .lines()
            .any(|line| line.ends_with(" M5") || line == "M5"));
        assert_eq!(posted.extension, "mpf");
    }

    #[test]
    fn siemens_native_post_fails_closed_without_a_confirmed_machine_profile() {
        let mut source = document(PostDialect::Siemens828d);
        source.post_defaults.siemens_828d = None;
        let error = post_setup(
            &source,
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .expect_err("a Siemens post without a machine-safe SUPA value must fail closed");

        assert!(error
            .to_string()
            .contains("explicitly confirmed machine profile"));
        assert!(error.to_string().contains("SUPA retract Z"));
    }

    #[test]
    fn siemens_later_tool_change_uses_standard_shutdown_and_native_dwell() {
        let mut source = two_tool_siemens_document();
        source
            .post_defaults
            .siemens_828d
            .as_mut()
            .unwrap()
            .optional_stop_on_tool_change = true;

        let posted = post_setup(
            &source,
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .unwrap();

        let shutdown = posted.nc.find("M9\nM5\nG0 SUPA Z0 D0").unwrap();
        let message = posted.nc.find("MSG (\"Drill holes\")").unwrap();
        let optional_stop = posted.nc[message..].find("M1").unwrap() + message;
        let tool_change =
            posted.nc[optional_stop..].find("T=\"5_MM_DRILL\"").unwrap() + optional_stop;
        assert!(shutdown < message && message < optional_stop && optional_stop < tool_change);
        assert!(posted.nc.contains("G4 F0.25"));
        assert_eq!(posted.nc.matches("G0 SUPA Z0 D0").count(), 3);
        assert!(!posted.nc.contains("M6\nD1\nT=\"5_MM_DRILL\"\n"));
        assert!(posted.nc.lines().any(|line| line == "M5"));
    }

    #[test]
    fn private_spindle_stop_is_opt_in_paired_with_m5_and_preserves_speed_changes() {
        for units in [CamUnits::Millimeters, CamUnits::Inches] {
            for numbered in [false, true] {
                let mut source = two_tool_siemens_document();
                source.units = units;
                let mut same_tool = source.setups[0].operations[0].clone();
                if let CamOperationDto::Face {
                    id, name, cutting, ..
                } = &mut same_tool
                {
                    *id = 90;
                    *name = "Same tool, changed speed".into();
                    cutting.spindle_rpm = 9_000;
                }
                source.setups[0].operations.insert(1, same_tool);
                source.next_operation_id = 91;
                source.post_defaults.sequence_numbers = numbered;
                source
                    .post_defaults
                    .siemens_828d
                    .as_mut()
                    .unwrap()
                    .spindle_stop_subprogram = Some("SHOP_STOP".into());
                source.setups[0].machine = Some(crate::CamMachineAssignmentDto::three_axis(
                    source.post_defaults.clone(),
                ));
                let request = CamPostRequestDto {
                    setup_id: 1,
                    post: None,
                    program_name: None,
                };
                let result = super::post_setup(&source, &request).unwrap();
                let blocks: Vec<_> = result
                    .nc
                    .lines()
                    .map(|l| {
                        if l.starts_with('N') {
                            l.split_once(' ').map_or(l, |(_, block)| block)
                        } else {
                            l
                        }
                    })
                    .collect();
                let stops: Vec<_> = blocks
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| **b == "M5")
                    .map(|(i, _)| i)
                    .collect();
                assert_eq!(stops.len(), 2, "same-tool RPM changes must not add stops");
                assert_eq!(
                    blocks.iter().filter(|b| **b == "SHOP_STOP").count(),
                    stops.len()
                );
                for i in &stops {
                    assert_eq!(blocks[i - 1], "SHOP_STOP");
                }
                assert!(blocks.iter().position(|b| *b == "M6").unwrap() < stops[0]);
                assert!(blocks.contains(&"S9000 M3"));
                assert_eq!(*blocks.last().unwrap(), "M30");
                assert!(result
                    .warnings
                    .iter()
                    .any(|w| w.contains("SHOP_STOP") && w.contains("NC replay is unavailable")));
                let mut roundtrip: CamDocumentDto =
                    serde_json::from_str(&serde_json::to_string(&source).unwrap()).unwrap();
                roundtrip.soften_for_load();
                assert_eq!(
                    super::post_setup(&roundtrip, &request).unwrap().nc,
                    result.nc
                );

                assert_eq!(
                    roundtrip.setups[0]
                        .machine
                        .as_ref()
                        .unwrap()
                        .profile
                        .schema_version,
                    2
                );
                let m = source.setups[0].machine.as_mut().unwrap();
                m.profile
                    .post
                    .siemens_828d
                    .as_mut()
                    .unwrap()
                    .spindle_stop_subprogram = None;
                let standard = super::post_setup(&source, &request).unwrap();
                assert!(!standard.nc.contains("SHOP_STOP"));
                let standard_blocks: Vec<_> = standard
                    .nc
                    .lines()
                    .map(|l| {
                        if l.starts_with('N') {
                            l.split_once(' ').map_or(l, |(_, block)| block)
                        } else {
                            l
                        }
                    })
                    .collect();
                assert_eq!(
                    standard_blocks,
                    blocks
                        .into_iter()
                        .filter(|b| *b != "SHOP_STOP")
                        .collect::<Vec<_>>(),
                    "Only the stop calls change executable blocks"
                );
            }
        }
    }

    #[test]
    fn private_spindle_stop_rejects_code_injection_and_old_reader_versions() {
        let mut source = document(PostDialect::Siemens828d);
        for invalid in [
            "",
            "M5",
            "RET",
            "CALL",
            "SHOP_STOP\nM30",
            "SHOP_STOP(1)",
            "../SHOP_STOP",
            "SHOP_STOP;M5",
            "0_SHOP",
            "Å_STOP",
            "ABCDEFGHIJKLMNOPQRSTUVWXYZ_STOP_1",
        ] {
            source
                .post_defaults
                .siemens_828d
                .as_mut()
                .unwrap()
                .spindle_stop_subprogram = Some(invalid.into());
            let machine = crate::CamMachineAssignmentDto::three_axis(source.post_defaults.clone());
            assert!(machine.validate().is_err(), "{invalid:?}");
        }
        source
            .post_defaults
            .siemens_828d
            .as_mut()
            .unwrap()
            .spindle_stop_subprogram = Some("SHOP_STOP".into());
        let mut machine = crate::CamMachineAssignmentDto::three_axis(source.post_defaults.clone());
        assert!(machine.validate().is_ok());
        machine.profile.schema_version = 1;
        assert!(machine.validate().unwrap_err().contains("version 2"));
        machine.profile.schema_version = 2;
        machine.profile.post.dialect = PostDialect::Fanuc;
        assert!(machine.validate().is_err());
    }

    #[test]
    #[ignore = "operator-supplied private profile; set LIMO_CAD_PRIVATE_POST_PATH"]
    fn inspect_private_profile_and_emit_sample_without_private_repo_data() {
        let file = std::env::var("LIMO_CAD_PRIVATE_POST_PATH").expect("private profile path");
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
        assert_eq!(raw["format"], "nbpost");
        assert_eq!(raw["schema_version"], 2);
        let machine: crate::CamMachineAssignmentDto =
            serde_json::from_value(raw["machine"].clone()).unwrap();
        machine.ensure_post_matches(&machine.profile.post).unwrap();
        let mut source = two_tool_siemens_document();

        for (i, tool) in source.tools.iter_mut().enumerate() {
            tool.number = Some(i as u32 + 1);
        }
        source.setups[0].machine = Some(machine);
        let result = super::post_setup(
            &source,
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: Some("PRIVATE_POST_TEST".into()),
            },
        )
        .unwrap();
        println!("{}", result.nc);
        for warning in result.warnings {
            println!("CHECK: {warning}");
        }
    }

    #[test]
    fn native_production_post_positions_xy_before_z_after_every_m6() {
        let source = two_tool_siemens_document();
        let posted = super::post_setup(
            &source,
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .unwrap();
        let mut changes = 0;
        for section in posted.nc.split("\nM6\n").skip(1) {
            let moves: Vec<_> = section
                .lines()
                .filter(|line| line.starts_with("G0 "))
                .take(2)
                .collect();
            assert_eq!(moves.len(), 2);
            assert!(
                moves[0].starts_with("G0 X") && !moves[0].contains('Z'),
                "{moves:?}"
            );
            assert!(
                moves[1].starts_with("G0 Z") && !moves[1].contains('X'),
                "{moves:?}"
            );
            changes += 1;
        }
        assert_eq!(changes, 2);
    }

    #[test]
    fn native_production_post_retracts_before_a_same_tool_fixture_change() {
        let mut source = document(PostDialect::Siemens828d);
        source.setups[0].work_offset_count = 2;
        let posted = super::post_setup(
            &source,
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .unwrap();
        assert_eq!(posted.nc.matches("\nM6\n").count(), 1);
        let next = posted.nc.find("\nG55\n").unwrap();
        assert!(posted.nc[..next].ends_with("G0 SUPA Z0 D0\nD1"));
        let moves: Vec<_> = posted.nc[next..]
            .lines()
            .filter(|line| line.starts_with("G0 "))
            .take(2)
            .collect();
        assert!(
            moves[0].starts_with("G0 X") && !moves[0].contains('Z'),
            "{moves:?}"
        );
        assert!(moves[1].starts_with("G0 Z"), "{moves:?}");
    }

    #[test]
    fn native_high_feed_approach_keeps_xy_before_z_without_inventing_a_rapid() {
        let mut source = document(PostDialect::Siemens828d);
        source.linking.push(crate::CamLinkingDto {
            operation_id: 1,
            high_feed_mode: crate::CamHighFeedMode::Always,
            high_feed: 1200.0,
            ..Default::default()
        });
        let posted = super::post_setup(
            &source,
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .unwrap();
        let moves: Vec<_> = posted
            .nc
            .split("\nM6\n")
            .nth(1)
            .unwrap()
            .lines()
            .filter(|line| line.starts_with("G0 ") || line.starts_with("G1 "))
            .take(2)
            .collect();
        assert!(
            moves[0].starts_with("G1 X") && !moves[0].contains('Z'),
            "{moves:?}"
        );
        assert!(moves[1].starts_with("G1 Z"), "{moves:?}");
        assert!(moves.iter().all(|line| line.ends_with("F1200")));
    }

    #[test]
    fn siemens_next_tool_preload_is_explicit_and_wraps_to_the_first_tool() {
        let mut source = two_tool_siemens_document();
        source
            .post_defaults
            .siemens_828d
            .as_mut()
            .unwrap()
            .preload_next_tool = true;

        let posted = post_setup(
            &source,
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .unwrap();

        assert!(posted
            .nc
            .contains("T=\"6_MM_FLAT\"\nM6\nD1\nT=\"5_MM_DRILL\"\nG17 G90 G94"));
        assert!(posted
            .nc
            .contains("T=\"5_MM_DRILL\"\nM6\nD1\nT=\"6_MM_FLAT\"\nG17 G90 G94"));
    }

    #[test]
    fn siemens_next_tool_preload_never_repeats_the_only_tool() {
        let mut source = document(PostDialect::Siemens828d);
        source
            .post_defaults
            .siemens_828d
            .as_mut()
            .unwrap()
            .preload_next_tool = true;

        let posted = post_setup(
            &source,
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .unwrap();

        assert_eq!(posted.nc.matches("\nT=\"6_MM_FLAT\"\n").count(), 1);
    }

    #[test]
    fn number_based_posts_fail_closed_when_a_tool_has_no_number() {
        let mut source = document(PostDialect::Fanuc);
        source.tools[0].number = None;
        let error = post_setup(
            &source,
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .expect_err("a numberless tool must not reach a numeric tool call");
        assert!(error.to_string().contains("needs a number"));
    }

    #[test]
    fn siemens_numeric_call_comes_from_the_library_without_a_binding() {
        let mut source = document(PostDialect::Siemens828d);
        source.tools[0].name = "!!!".into();
        source.tools[0].number = Some(27);
        source.setups[0]
            .machine
            .as_mut()
            .unwrap()
            .tool_calls
            .clear();
        let posted = post_setup(
            &source,
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .unwrap();
        assert!(posted.nc.contains("T27\n"));
        assert!(!posted.nc.contains("T=\""));
    }

    #[test]
    fn siemens_named_tool_call_survives_a_missing_number() {
        let mut source = document(PostDialect::Siemens828d);
        source.tools[0].number = None;
        let posted = post_setup(
            &source,
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .unwrap();
        assert!(posted.nc.contains("T=\"6_MM_FLAT\"\n"));
    }

    #[test]
    fn siemens_controller_managed_strategy_leaves_station_motion_to_m6() {
        let mut source = two_tool_siemens_document();
        source
            .post_defaults
            .siemens_828d
            .as_mut()
            .unwrap()
            .tool_change_positioning = Siemens828dToolChangePositioning::ControllerManaged;

        let posted = post_setup(
            &source,
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .unwrap();

        assert!(posted
            .nc
            .contains("M9\nM5\n; M6/PLC CONTROLS TOOL-CHANGE POSITIONING"));
        assert_eq!(posted.nc.matches("G0 SUPA Z0 D0").count(), 2);
    }

    #[test]
    fn siemens_fixed_station_strategy_moves_z_before_machine_xy() {
        let mut source = two_tool_siemens_document();
        let profile = source.post_defaults.siemens_828d.as_mut().unwrap();
        profile.tool_change_positioning = Siemens828dToolChangePositioning::SupaZThenXy;
        profile.station_x = Some(123.4);
        profile.station_y = Some(-56.7);

        let posted = post_setup(
            &source,
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .unwrap();

        assert!(posted
            .nc
            .contains("M9\nM5\nG0 SUPA Z0 D0\nG0 SUPA X123.4 Y-56.7\nD1"));
    }

    #[test]
    fn siemens_fixed_station_strategy_fails_closed_until_both_axes_are_entered() {
        let mut source = two_tool_siemens_document();
        source
            .post_defaults
            .siemens_828d
            .as_mut()
            .unwrap()
            .tool_change_positioning = Siemens828dToolChangePositioning::SupaZThenXy;

        let error = post_setup(
            &source,
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("requires machine X and Y"));
    }

    #[test]
    fn siemens_atc_style_is_informational_and_cannot_silently_change_motion() {
        let source = two_tool_siemens_document();
        let baseline = post_setup(
            &source,
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .unwrap();
        let mut umbrella = source;
        umbrella
            .post_defaults
            .siemens_828d
            .as_mut()
            .unwrap()
            .atc_style = Siemens828dAtcStyle::Umbrella;
        let changed = post_setup(
            &umbrella,
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .unwrap();
        assert_eq!(baseline.nc, changed.nc);
    }

    #[test]
    fn older_siemens_profiles_keep_the_original_supa_z_behavior() {
        let profile: Siemens828dPostConfigDto = serde_json::from_str(
            r#"{"supa_retract_z":0.0,"tool_length_offset":1,"optional_stop_on_tool_change":true}"#,
        )
        .unwrap();
        assert_eq!(profile.atc_style, Siemens828dAtcStyle::DoubleArm);
        assert_eq!(
            profile.tool_change_positioning,
            Siemens828dToolChangePositioning::SupaZ
        );
        assert_eq!(profile.station_x, None);
        assert_eq!(profile.station_y, None);
        assert!(!profile.preload_next_tool);
        assert!(
            profile.optional_stop_on_tool_change,
            "an explicit saved opt-in is retained"
        );
    }

    #[test]
    fn optional_tool_stops_default_off_in_new_and_unspecified_profiles() {
        let profile: Siemens828dPostConfigDto =
            serde_json::from_str(r#"{"supa_retract_z":0.0}"#).unwrap();
        assert!(!profile.optional_stop_on_tool_change);
        assert!(!Siemens828dPostConfigDto::default().optional_stop_on_tool_change);
        let posted = post_setup(
            &two_tool_siemens_document(),
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .unwrap();
        assert!(!posted
            .nc
            .lines()
            .any(|line| line.split_whitespace().any(|word| word == "M1")));
    }

    #[test]
    fn arc_output_keeps_rounded_endpoints_and_incremental_centers_consistent() {
        for unit in [CamUnits::Millimeters, CamUnits::Inches] {
            let units = PostUnits {
                units: unit,
                contains_arcs: true,
            };
            for angle in [0.23_f64, 0.77, 1.29, 2.71, 4.93] {
                let c = [7.381_293, -3.117_943];
                let start = [c[0] + 0.8 * angle.cos(), c[1] + 0.8 * angle.sin()];
                let end = [
                    c[0] + 0.8 * (angle + 1.137).cos(),
                    c[1] + 0.8 * (angle + 1.137).sin(),
                ];
                for native in [false, true] {
                    let roundtrip = |v: f64| {
                        let text = if native {
                            units.siemens_len(v)
                        } else {
                            units.len(v)
                        };
                        unit.to_mm(text.parse::<f64>().unwrap())
                    };
                    let a = start.map(roundtrip);
                    let b = end.map(roundtrip);
                    let center = [
                        a[0] + roundtrip(c[0] - start[0]),
                        a[1] + roundtrip(c[1] - start[1]),
                    ];
                    let r0 = (a[0] - center[0]).hypot(a[1] - center[1]);
                    let r1 = (b[0] - center[0]).hypot(b[1] - center[1]);
                    assert!(
                        (r0 - r1).abs() < 1e-5,
                        "{unit:?}, native={native}, angle={angle}"
                    );
                }
            }
        }
    }

    #[test]
    fn coordinate_format_suppresses_negative_zero_and_noise() {
        assert_eq!(coordinate(-0.000_1), "0");
        assert_eq!(coordinate(12.340), "12.34");
        assert_eq!(coordinate(-2.5), "-2.5");
        assert_eq!(siemens_coordinate(1.234_567), "1.23457");
        assert_eq!(siemens_program_name(" Part 12.mpf "), "PART_12");
    }

    #[test]
    fn inch_documents_post_g20_with_converted_lengths_and_feeds() {
        let mut source = document(PostDialect::Grbl);
        source.units = CamUnits::Inches;
        let posted = post_setup(
            &source,
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .unwrap();
        assert!(posted.nc.contains("G90 G17 G20 G40 G49 G80"));

        assert!(posted.nc.contains("G0 Z0.315"));
        assert!(posted.nc.contains("F23.622"));
        assert!(!posted.nc.contains("G21"));
        assert!(posted
            .warnings
            .iter()
            .any(|warning| warning.contains("inches")));
    }

    #[test]
    fn inch_documents_post_g700_for_geometry_and_feed_on_the_siemens_profile() {
        let mut source = document(PostDialect::Siemens828d);
        source.units = CamUnits::Inches;
        let posted = post_setup(
            &source,
            &CamPostRequestDto {
                setup_id: 1,
                post: None,
                program_name: None,
            },
        )
        .unwrap();
        assert!(posted.nc.contains("G17 G700 G90 G94"));
        assert!(posted.nc.contains("F23.622"));
        assert!(!posted.nc.split_whitespace().any(|word| word == "G70"));
        assert!(!posted.nc.contains("G710"));
    }
}
