use serde::{Deserialize, Serialize};

#[path = "adaptive.rs"]
mod adaptive;
#[path = "flat.rs"]
mod flat;
#[path = "linking_planner.rs"]
mod linking_planner;
use crate::linking::{CamHighFeedMode, CamLinkingDto};

use crate::model::{
    signed_area, CamDocumentDto, CamOperationDto, CamToolDto, CompensationMode,
    ContourCompensation, CoolantMode, DrillCycle, FaceDirection, MillingDirection, Point2Dto,
    Point3Dto, SpindleDirection, ThreadHand, WorkOffset,
};

const EPSILON: f64 = 1.0e-9;

/// Physical M3 milling convention shared by every closed 2D planner.
/// `material_inside` says the protected/remaining material is toward the
/// loop interior. Climb requires that material on the right of travel.
fn m3_closed_cut_is_ccw(direction: MillingDirection, material_inside: bool) -> bool {
    matches!(
        (direction, material_inside),
        (MillingDirection::Climb, false) | (MillingDirection::Conventional, true)
    )
}
const MAX_GENERATED_STEPS: usize = 250_000;
const MAX_PROGRAM_COMMANDS: usize = 300_000;
/// G0 rapid is a full-speed machine move; standard G-code has no programmable
/// rapid feed. This estimate only feeds the cycle-time statistic.
pub(crate) const RAPID_FEED_ESTIMATE_MM_PER_MIN: f64 = 8_000.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CamPlanError(pub String);

impl std::fmt::Display for CamPlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CamPlanError {}

impl From<String> for CamPlanError {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionKind {
    Rapid,
    Cutting,
}

/// Principal interpolation plane for a circular move. CAM planning currently
/// emits XY arcs, while the shared simulation timeline also accepts XZ/YZ
/// arcs parsed from final controller programs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CamArcPlane {
    #[default]
    Xy,
    Xz,
    Yz,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CamCommandDto {
    ProgramStart {
        name: String,
        work_offset: WorkOffset,
    },
    /// Selects the fixture offset for the motion that follows. Emitted once
    /// per repeated work-offset copy of the program.
    WorkOffset {
        offset: WorkOffset,
    },
    SectionStart {
        operation_id: u64,
        name: String,
        tool_id: u64,
    },
    ToolChange {
        tool_id: u64,
        /// Machine-facing tool number when the library assigns one. Posts
        /// that call tools numerically fail closed on `None`; name-capable
        /// posts may use the exact project-library tool name.
        tool_number: Option<u32>,
        tool_name: String,
    },
    Spindle {
        direction: SpindleDirection,
        rpm: u32,
    },
    Coolant {
        mode: CoolantMode,
    },
    /// Establishes the tool-tip pose in workpiece coordinates without
    /// claiming that the tool travelled there. The strict NC interpreter
    /// emits this after excluded machine-coordinate/tool-change motion; CAM
    /// planning never emits it.
    SetPosition {
        to: Point3Dto,
    },
    Rapid {
        to: Point3Dto,
    },
    Linear {
        to: Point3Dto,
        feed: f64,
    },
    Circular {
        clockwise: bool,
        #[serde(default)]
        plane: CamArcPlane,
        center: Point3Dto,
        to: Point3Dto,
        feed: f64,
    },
    Dwell {
        seconds: f64,
    },
    /// Activates machine-side cutter radius compensation: `left` true means
    /// the tool shifts left of the programmed travel direction (G41), false
    /// means right (G42). Takes effect on the LINEAR move that follows — the
    /// lead-in — and stays active until `CutterCompensationOff`. Only
    /// emitted for contour operations whose compensation mode is in control;
    /// the programmed path then stays the part contour and the diameter
    /// register is a machine-side value the post resolves.
    CutterCompensationOn {
        left: bool,
    },
    /// Cancels machine-side cutter radius compensation on the LINEAR move
    /// that follows — the lead-out (G40).
    CutterCompensationOff,
    SectionEnd,
    ProgramEnd,
}

impl CamCommandDto {
    pub fn endpoint(&self) -> Option<Point3Dto> {
        match self {
            Self::SetPosition { to }
            | Self::Rapid { to }
            | Self::Linear { to, .. }
            | Self::Circular { to, .. } => Some(*to),
            _ => None,
        }
    }

    pub fn motion_kind(&self) -> Option<MotionKind> {
        match self {
            Self::Rapid { .. } => Some(MotionKind::Rapid),
            Self::Linear { .. } | Self::Circular { .. } => Some(MotionKind::Cutting),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct CamProgramStatsDto {
    pub rapid_distance: f64,
    pub cutting_distance: f64,
    pub estimated_seconds: f64,
    pub operation_count: usize,
}

/// Motion totals of one operation within a single work-offset copy of the
/// program. Programs repeated across consecutive work offsets report the
/// first copy, which every duplicate shares.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct CamOperationStatsDto {
    pub operation_id: u64,
    pub rapid_distance: f64,
    pub cutting_distance: f64,
    pub estimated_seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamProgramDto {
    pub setup_id: u64,
    pub name: String,
    pub commands: Vec<CamCommandDto>,
    pub stats: CamProgramStatsDto,
    /// Per-operation motion totals for the manufacturing status readout.
    #[serde(default)]
    pub per_operation: Vec<CamOperationStatsDto>,
    /// Work offsets the program repeats with, in posted order.
    #[serde(default)]
    pub work_offsets: Vec<WorkOffset>,
    pub warnings: Vec<String>,
}

/// Preview/simulate only the requested prefix, including the preceding
/// operations that establish incoming stock. A later invalid operation must
/// not prevent viewing an earlier valid one. NC posting uses `plan_setup`.
pub fn plan_setup_through(
    document: &CamDocumentDto,
    setup_id: u64,
    through_operation_id: u64,
) -> Result<CamProgramDto, CamPlanError> {
    let setup = document
        .setup(setup_id)
        .ok_or_else(|| CamPlanError(format!("CAM setup {setup_id} does not exist")))?;
    let through = setup
        .operations
        .iter()
        .position(|operation| operation.id() == through_operation_id)
        .ok_or_else(|| {
            CamPlanError(format!(
                "CAM operation {through_operation_id} does not exist in setup '{}'",
                setup.name
            ))
        })?
        + 1;
    let mut scoped = document.clone();
    scoped
        .setups
        .iter_mut()
        .find(|setup| setup.id == setup_id)
        .expect("validated setup")
        .operations
        .truncate(through);
    let kept = scoped
        .setups
        .iter()
        .flat_map(|setup| setup.operations.iter().map(CamOperationDto::id))
        .collect::<std::collections::HashSet<_>>();
    scoped
        .height_expressions
        .retain(|item| kept.contains(&item.operation_id));
    scoped
        .linking
        .retain(|item| kept.contains(&item.operation_id));
    let setup = scoped.setup(setup_id).expect("validated setup");
    if !setup.operations.iter().any(CamOperationDto::enabled) {
        scoped.validate().map_err(CamPlanError)?;
        crate::machine::ensure_setup_machines_supported(&scoped, setup_id)?;
        return Ok(CamProgramDto {
            setup_id,
            name: setup.name.clone(),
            commands: Vec::new(),
            stats: Default::default(),
            per_operation: Vec::new(),
            work_offsets: setup.work_offsets(),
            warnings: Vec::new(),
        });
    }
    plan_setup(&scoped, setup_id)
}

/// Expand one fixed-axis setup into deterministic, controller-neutral motion.
/// All coordinates are millimetres in setup/WCS coordinates; Z+ points away
/// from the stock and the spindle axis remains parallel to setup Z.
pub fn plan_setup(document: &CamDocumentDto, setup_id: u64) -> Result<CamProgramDto, CamPlanError> {
    document.validate().map_err(CamPlanError)?;
    crate::machine::ensure_setup_machines_supported(document, setup_id)?;

    let cache_key = document
        .setup(setup_id)
        .filter(|s| {
            s.operations.iter().any(|o| {
                o.enabled()
                    && matches!(
                        o,
                        CamOperationDto::Adaptive3d { .. } | CamOperationDto::Flat3d { .. }
                    )
            })
        })
        .and_then(|s| {
            let mut chain = Vec::new();
            let mut cursor = s;
            loop {
                let mut intent = cursor.clone();
                intent.machine = None;
                chain.push(intent);
                let crate::CamResolvedStockDto::Rest { source_setup_id } = cursor.resolved_stock
                else {
                    break;
                };
                cursor = document.setup(source_setup_id)?;
            }
            serde_json::to_vec(&(chain, &document.tools, &document.linking)).ok()
        })
        .filter(|key| key.len() <= ADAPTIVE_PLAN_CACHE_BYTES / 2);
    if let Some(key) = &cache_key {
        if let Ok(mut cache) = adaptive_plan_cache().lock() {
            if let Some(index) = cache.iter().position(|entry| &entry.key == key) {
                let entry = cache.remove(index).unwrap();
                let program = entry.program.clone();
                cache.push_back(entry);
                return Ok(program);
            }
        }
    }
    let program = plan_setup_uncached(document, setup_id)?;
    if let Some(key) = cache_key {
        let bytes = key.len()
            + program.commands.len() * std::mem::size_of::<CamCommandDto>()
            + program.warnings.iter().map(String::len).sum::<usize>()
            + program
                .commands
                .iter()
                .map(|c| match c {
                    CamCommandDto::ProgramStart { name, .. }
                    | CamCommandDto::SectionStart { name, .. } => name.len(),
                    CamCommandDto::ToolChange { tool_name, .. } => tool_name.len(),
                    _ => 0,
                })
                .sum::<usize>();
        if bytes <= ADAPTIVE_PLAN_CACHE_BYTES {
            if let Ok(mut cache) = adaptive_plan_cache().lock() {
                cache.retain(|entry| entry.key != key);
                while cache.len() >= 4
                    || cache.iter().map(|entry| entry.bytes).sum::<usize>() + bytes
                        > ADAPTIVE_PLAN_CACHE_BYTES
                {
                    if cache.pop_front().is_none() {
                        break;
                    }
                }
                cache.push_back(AdaptivePlanCacheEntry {
                    key,
                    program: program.clone(),
                    bytes,
                });
            }
        }
    }
    Ok(program)
}

const ADAPTIVE_PLAN_CACHE_BYTES: usize = 16 * 1024 * 1024;
struct AdaptivePlanCacheEntry {
    key: Vec<u8>,
    program: CamProgramDto,
    bytes: usize,
}
fn adaptive_plan_cache(
) -> &'static std::sync::Mutex<std::collections::VecDeque<AdaptivePlanCacheEntry>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::VecDeque<AdaptivePlanCacheEntry>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn plan_setup_uncached(
    document: &CamDocumentDto,
    setup_id: u64,
) -> Result<CamProgramDto, CamPlanError> {
    let setup = document
        .setup(setup_id)
        .ok_or_else(|| CamPlanError(format!("CAM setup {setup_id} does not exist")))?;
    let operations = setup
        .operations
        .iter()
        .filter(|operation| operation.enabled())
        .collect::<Vec<_>>();
    if operations.is_empty() {
        return Err(CamPlanError(format!(
            "CAM setup '{}' has no enabled operations",
            setup.name
        )));
    }

    let mut builder = ProgramBuilder::new();
    if matches!(
        setup.resolved_stock,
        crate::CamResolvedStockDto::Rest { .. }
    ) && operations
        .iter()
        .any(|o| matches!(o, CamOperationDto::Adaptive3d { .. }))
    {
        builder.rest_stock = Some(crate::simulation::planning_stock(document, setup)?);
        builder.warnings.push("Roughing uses simulated remaining stock in this setup's WCS. Its conservative upper envelope retains material below overhangs.".into());
    }
    let work_offsets = setup.work_offsets();
    builder.commands.push(CamCommandDto::ProgramStart {
        name: setup.name.clone(),
        work_offset: setup.work_offset,
    });

    let mut active_tool: Option<u64> = None;
    let mut active_spindle: Option<(SpindleDirection, u32)> = None;
    let mut active_coolant = CoolantMode::Off;
    let mut per_operation: Vec<CamOperationStatsDto> = Vec::new();

    for offset in work_offsets.iter().copied() {
        builder.incoming_top = builder
            .rest_stock
            .as_ref()
            .map_or(setup.stock.max.z, |s| s.top().max(setup.stock.min.z));
        builder.incoming_bounds = Some(setup.stock);
        builder.commands.push(CamCommandDto::WorkOffset { offset });
        let copy_start = builder.commands.len() - 1;
        let mut remaining_stock_warned = false;
        let mut planning_stock: Option<crate::simulation::PlanningStock> = None;
        for operation in &operations {
            let earlier_cuts = builder.commands[copy_start..]
                .iter()
                .any(|c| matches!(c, CamCommandDto::SectionStart { .. }));
            let mut incoming = None;
            if earlier_cuts && crate::dependencies::consumes_remaining_stock(setup, operation) {
                if planning_stock.is_none() {
                    planning_stock = Some(crate::simulation::PlanningStock::new(document, setup)?);
                }
                let remaining = planning_stock.as_mut().expect("created above").after(
                    document,
                    setup,
                    &builder.commands[copy_start..],
                )?;
                incoming = Some((
                    builder.rest_stock.take(),
                    builder.remaining_stock.take(),
                    builder.incoming_top,
                ));
                if matches!(operation, CamOperationDto::Adaptive3d { .. }) {
                    builder.incoming_top = builder
                        .incoming_top
                        .min(remaining.top().max(setup.stock.min.z));
                    builder.rest_stock = Some(remaining);
                    if !remaining_stock_warned {
                        remaining_stock_warned = true;
                        builder.warnings.push("Roughing after earlier operations in this setup uses their simulated remaining stock (a conservative upper envelope) as incoming material.".into());
                    }
                } else {
                    builder.remaining_stock = Some(remaining);
                }
            }
            let tool = document
                .tool(operation.tool_id())
                .ok_or_else(|| CamPlanError("validated operation tool disappeared".to_string()))?;
            builder.set_safe_heights(operation);
            builder.linking = document
                .linking
                .iter()
                .find(|item| item.operation_id == operation.id())
                .cloned();
            if let Some(link) = &mut builder.linking {
                link.resolve_feeds(operation.cutting().feed_xy);
            }
            builder.tool_radius = tool.diameter / 2.0;
            builder.link_obstacles = None;

            let dependencies = crate::dependencies::planning_dependency_policy(
                setup,
                operation,
                builder.linking.as_ref(),
            );
            builder.predrilled = if dependencies.predrilled_entry {
                linking_planner::predrilled_holes(document, setup, operation.id())
            } else {
                Vec::new()
            };
            if matches!(
                operation,
                CamOperationDto::Drill { .. }
                    | CamOperationDto::Pocket2d { .. }
                    | CamOperationDto::Thread { .. }
            ) && operation.feed_height_z()
                < builder
                    .remaining_stock
                    .as_ref()
                    .map_or(builder.incoming_top, |stock| {
                        stock.top().min(builder.incoming_top)
                    })
                    - EPSILON
            {
                return Err(CamPlanError(format!(
                    "operation '{}' feed height {:.3} mm is below the conservatively known incoming stock top {:.3} mm; raise the feed/retract planes or regenerate an enabled whole-stock facing operation first. Selected model faces alone do not prove previous stock removal",
                    operation.name(), operation.feed_height_z(), builder.incoming_top
                )));
            }
            builder.commands.push(CamCommandDto::SectionStart {
                operation_id: operation.id(),
                name: operation.name().to_string(),
                tool_id: tool.id,
            });
            let section_stats = builder.stats;

            if active_tool != Some(tool.id) {
                builder.retract_to_clearance();
                if active_coolant != CoolantMode::Off {
                    builder.commands.push(CamCommandDto::Coolant {
                        mode: CoolantMode::Off,
                    });
                    active_coolant = CoolantMode::Off;
                }
                if active_spindle.is_some() {
                    builder.commands.push(CamCommandDto::Spindle {
                        direction: SpindleDirection::Off,
                        rpm: 0,
                    });
                    active_spindle = None;
                    builder.spindle = None;
                }
                builder.commands.push(CamCommandDto::ToolChange {
                    tool_id: tool.id,
                    tool_number: tool.number,
                    tool_name: tool.name.clone(),
                });
                active_tool = Some(tool.id);
            }

            let cutting = operation.cutting();
            let desired_spindle = (SpindleDirection::Clockwise, cutting.spindle_rpm);
            if active_spindle != Some(desired_spindle) {
                builder.commands.push(CamCommandDto::Spindle {
                    direction: desired_spindle.0,
                    rpm: desired_spindle.1,
                });
                active_spindle = Some(desired_spindle);
                builder.spindle = Some(desired_spindle);
            }
            if active_coolant != cutting.coolant {
                builder.commands.push(CamCommandDto::Coolant {
                    mode: cutting.coolant,
                });
                active_coolant = cutting.coolant;
            }

            match operation {
                CamOperationDto::Adaptive3d { .. } => {
                    adaptive::plan(&mut builder, setup, operation, tool)?
                }
                CamOperationDto::Flat3d { .. } => flat::plan(&mut builder, setup, operation, tool)?,
                CamOperationDto::Face { .. } => plan_face(&mut builder, setup, operation, tool)?,
                CamOperationDto::Contour2d { .. } => plan_contour(&mut builder, operation, tool)?,
                CamOperationDto::Drill { .. } => plan_drill(&mut builder, operation, tool)?,
                CamOperationDto::Pocket2d { .. } => plan_pocket(&mut builder, operation, tool)?,
                CamOperationDto::Chamfer2d { .. } => plan_chamfer(&mut builder, operation, tool)?,
                CamOperationDto::Thread { .. } => plan_thread(&mut builder, operation, tool)?,
            }
            builder.commands.push(CamCommandDto::SectionEnd);
            if let Some(incoming) = incoming {
                (
                    builder.rest_stock,
                    builder.remaining_stock,
                    builder.incoming_top,
                ) = incoming;
            }
            builder.stats.operation_count += 1;

            if !per_operation
                .iter()
                .any(|entry| entry.operation_id == operation.id())
            {
                per_operation.push(CamOperationStatsDto {
                    operation_id: operation.id(),
                    rapid_distance: builder.stats.rapid_distance - section_stats.rapid_distance,
                    cutting_distance: builder.stats.cutting_distance
                        - section_stats.cutting_distance,
                    estimated_seconds: builder.stats.estimated_seconds
                        - section_stats.estimated_seconds,
                });
            }
        }
    }

    builder.retract_to_clearance();
    if active_coolant != CoolantMode::Off {
        builder.commands.push(CamCommandDto::Coolant {
            mode: CoolantMode::Off,
        });
    }
    if active_spindle.is_some() {
        builder.commands.push(CamCommandDto::Spindle {
            direction: SpindleDirection::Off,
            rpm: 0,
        });
    }
    builder.commands.push(CamCommandDto::ProgramEnd);

    Ok(CamProgramDto {
        setup_id,
        name: setup.name.clone(),
        commands: builder.commands,
        stats: builder.stats,
        per_operation,
        work_offsets,
        warnings: builder.warnings.into_iter().chain(vec![
            "Toolpaths are stock-aware but are not yet collision-checked against fixtures or holders."
                .to_string(),
            "Posted programs retract Z before the first XY move, but still require a verified WCS and machine-safe start position."
                .to_string(),
            "Simulate, inspect, and dry-run every posted program before machining.".to_string(),
        ]).collect(),
    })
}

struct ProgramBuilder {
    commands: Vec<CamCommandDto>,
    stats: CamProgramStatsDto,
    position: Option<Point3Dto>,
    /// Safe travel planes of the operation currently being planned.
    clearance_z: f64,
    retract_z: f64,
    /// Feed-engagement plane of the current operation: rapids never go below
    /// it (clamped to the cut depth), everything underneath is feed rate.
    feed_height_z: f64,
    incoming_top: f64,
    rest_stock: Option<crate::simulation::RestHeightMap>,
    /// What earlier operations in this setup leave, for proving a low rapid
    /// approach clear. Roughing instead reads it as `rest_stock`.
    remaining_stock: Option<crate::simulation::RestHeightMap>,
    incoming_bounds: Option<crate::model::StockBoxDto>,
    /// Last spindle word emitted, so mid-operation reversals (tapping) only
    /// emit blocks when the state actually changes.
    spindle: Option<(SpindleDirection, u32)>,
    warnings: Vec<String>,
    linking: Option<CamLinkingDto>,
    predrilled: Vec<linking_planner::PredrilledHole>,
    tool_radius: f64,
    link_obstacles: Option<crate::model::StockBoxDto>,
}

impl ProgramBuilder {
    fn new() -> Self {
        Self {
            commands: Vec::new(),
            stats: CamProgramStatsDto::default(),
            position: None,
            clearance_z: 0.0,
            retract_z: 0.0,
            feed_height_z: 0.0,
            incoming_top: f64::NEG_INFINITY,
            incoming_bounds: None,
            rest_stock: None,
            remaining_stock: None,
            spindle: None,
            warnings: Vec::new(),
            linking: None,
            predrilled: Vec::new(),
            tool_radius: 0.0,
            link_obstacles: None,
        }
    }

    fn set_safe_heights(&mut self, operation: &CamOperationDto) {
        self.clearance_z = operation.clearance_z();
        self.retract_z = operation.retract_z();
        self.feed_height_z = operation.feed_height_z();
    }

    /// The lowest plane a rapid may reach when the cut goes to `depth`: the
    /// feed-engagement plane, but never below the target depth itself.
    fn feed_plane(&self, depth: f64) -> f64 {
        self.feed_height_z.max(depth)
    }

    fn require_clear_approach(
        &self,
        point: Point2Dto,
        radius: f64,
        name: &str,
    ) -> Result<(), CamPlanError> {
        if self.feed_height_z >= self.incoming_top - EPSILON {
            return Ok(());
        }
        if let Some(stock) = &self.incoming_bounds {
            let dx = (stock.min.x - point.x).max(point.x - stock.max.x).max(0.0);
            let dy = (stock.min.y - point.y).max(point.y - stock.max.y).max(0.0);
            if dx.hypot(dy) >= radius + 1e-6 {
                return Ok(());
            }
        }
        if let Some(stock) = &self.remaining_stock {
            let top = stock.upper_over(
                [point.x - radius, point.y - radius],
                [point.x + radius, point.y + radius],
            );
            if top < self.feed_height_z - EPSILON {
                return Ok(());
            }
        }
        Err(CamPlanError(format!("operation '{name}' cannot prove its rapid approach clear of incoming stock at {:.3} mm; raise feed/retract heights above {:.3} mm, use an outside-stock entry, or regenerate preceding whole-stock facing", self.feed_height_z, self.incoming_top)))
    }

    fn rapid(&mut self, to: Point3Dto) {
        if self.position == Some(to) {
            return;
        }
        if let Some(link) = &self.linking {
            if self.position.is_none() && link.high_feed_mode == CamHighFeedMode::Always {
                self.linear(to, link.high_feed);
                return;
            }
            if let Some(from) = self.position {
                let x = (from.x - to.x).abs() > EPSILON;
                let y = (from.y - to.y).abs() > EPSILON;
                let z = (from.z - to.z).abs() > EPSILON;
                if !link.allow_rapid_retract && !x && !y && to.z > from.z {
                    self.linear(to, link.lead_out_feed);
                    return;
                }
                let preserve = match link.high_feed_mode {
                    CamHighFeedMode::Preserve => true,
                    CamHighFeedMode::AxialRadial => !z || (!x && !y),
                    CamHighFeedMode::Axial => !x && !y,
                    CamHighFeedMode::Radial => !z,
                    CamHighFeedMode::SingleAxis => x as u8 + y as u8 + z as u8 <= 1,
                    CamHighFeedMode::Always => false,
                };
                if !preserve {
                    self.linear(to, link.high_feed);
                    return;
                }
            }
        }
        if let Some(from) = self.position {
            let distance = distance(from, to);
            self.stats.rapid_distance += distance;

            self.stats.estimated_seconds += distance / RAPID_FEED_ESTIMATE_MM_PER_MIN * 60.0;
        }
        self.commands.push(CamCommandDto::Rapid { to });
        self.position = Some(to);
    }

    fn linear(&mut self, to: Point3Dto, feed: f64) {
        if self.position == Some(to) {
            return;
        }
        if let Some(from) = self.position {
            let distance = distance(from, to);
            self.stats.cutting_distance += distance;
            self.stats.estimated_seconds += distance / feed * 60.0;
        }
        self.commands.push(CamCommandDto::Linear { to, feed });
        self.position = Some(to);
    }

    fn dwell(&mut self, seconds: f64) {
        if seconds <= EPSILON {
            return;
        }
        self.commands.push(CamCommandDto::Dwell { seconds });
        self.stats.estimated_seconds += seconds;
    }

    /// Circular interpolation in the setup XY plane. `to` may carry a
    /// different Z than the current position, producing a helical move
    /// (thread milling). The arc length, including the Z travel, feeds the
    /// distance and time estimates.
    fn circular(&mut self, to: Point3Dto, center: Point2Dto, clockwise: bool, feed: f64) {
        let Some(from) = self.position else {
            self.linear(to, feed);
            return;
        };
        if from == to {
            return;
        }
        let radius = distance_2d(
            Point2Dto {
                x: from.x,
                y: from.y,
            },
            center,
        );
        let start_angle = (from.y - center.y).atan2(from.x - center.x);
        let end_angle = (to.y - center.y).atan2(to.x - center.x);
        let mut sweep = end_angle - start_angle;
        if clockwise {
            while sweep >= 0.0 {
                sweep -= std::f64::consts::TAU;
            }
        } else {
            while sweep <= 0.0 {
                sweep += std::f64::consts::TAU;
            }
        }
        let arc = radius * sweep.abs();
        let length = arc.hypot(to.z - from.z);
        self.stats.cutting_distance += length;
        self.stats.estimated_seconds += length / feed * 60.0;
        self.commands.push(CamCommandDto::Circular {
            clockwise,
            plane: CamArcPlane::Xy,
            center: Point3Dto::new(center.x, center.y, from.z),
            to,
            feed,
        });
        self.position = Some(to);
    }

    /// Emit a spindle word only when the state changes. Tapping cycles
    /// reverse the spindle mid-operation; tracking keeps redundant M3/M4
    /// blocks out of the stream.
    fn spindle(&mut self, direction: SpindleDirection, rpm: u32) {
        if self.spindle == Some((direction, rpm)) {
            return;
        }
        self.commands
            .push(CamCommandDto::Spindle { direction, rpm });
        self.spindle = Some((direction, rpm));
    }

    fn retract_to_clearance(&mut self) {
        let Some(position) = self.position else {
            return;
        };

        if position.z < self.retract_z - EPSILON {
            self.rapid(Point3Dto::new(position.x, position.y, self.retract_z));
        }
        let position = self.position.expect("position set by the rapid above");
        if (position.z - self.clearance_z).abs() > EPSILON {
            self.rapid(Point3Dto::new(position.x, position.y, self.clearance_z));
        }
    }

    fn approach(&mut self, point: Point2Dto, depth: f64, plunge_feed: f64) {
        if linking_planner::approach_policy(self, point, depth, plunge_feed) {
            return;
        }
        self.retract_to_clearance();
        self.rapid(Point3Dto::new(point.x, point.y, self.clearance_z));
        self.rapid(Point3Dto::new(point.x, point.y, self.retract_z));

        let feed_plane = self.feed_plane(depth);
        self.rapid(Point3Dto::new(point.x, point.y, feed_plane));
        self.linear(Point3Dto::new(point.x, point.y, depth), plunge_feed);
    }
}

/// Sufficient exact coverage proof for parallel horizontal facing strokes.
/// At the floor, only the flat land cuts: R-c for a rounded corner, R-w for
/// a bevel. The full-diameter bands can overlap while leaving floor ridges.
fn facing_clears_stock_floor(
    tool: &CamToolDto,
    stock: &crate::model::StockBoxDto,
    rows: &[f64],
    cut_min_x: f64,
    cut_max_x: f64,
) -> bool {
    let Some(flat_radius) = crate::CutterProfile::new(tool.into())
        .ok()
        .and_then(|profile| profile.radius_at_height(0.0))
    else {
        return false;
    };
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        return false;
    };
    flat_radius > EPSILON
        && cut_min_x <= stock.min.x + EPSILON
        && cut_max_x >= stock.max.x - EPSILON
        && first - flat_radius <= stock.min.y + EPSILON
        && last + flat_radius >= stock.max.y - EPSILON
        && rows
            .windows(2)
            .all(|pair| pair[1] >= pair[0] && pair[1] - pair[0] <= 2.0 * flat_radius + EPSILON)
}

fn plan_face(
    builder: &mut ProgramBuilder,
    setup: &crate::model::CamSetupDto,
    operation: &CamOperationDto,
    tool: &CamToolDto,
) -> Result<(), CamPlanError> {
    if builder.linking.is_some() {
        return linking_planner::plan_face(builder, setup, operation, tool);
    }
    let CamOperationDto::Face {
        bounds,
        top_z,
        target_z,
        step_over,
        step_down,
        safe_distance,
        direction,
        cutting,
        name,
        ..
    } = operation
    else {
        unreachable!();
    };
    let material_top = top_z.max(builder.incoming_top);
    require_flute_length(tool, material_top - target_z, name)?;
    let radius = tool.diameter * 0.5;

    let width = bounds.max.y - bounds.min.y;
    let span_needed = (width - tool.diameter).max(0.0);
    let mut row_count = 1usize;
    while span_needed > EPSILON && (row_count - 1) as f64 * step_over < span_needed - EPSILON {
        row_count += 1;
        if row_count >= MAX_GENERATED_STEPS {
            return Err(CamPlanError(format!(
                "toolpath needs more than {MAX_GENERATED_STEPS} stepover rows; increase the stepover"
            )));
        }
    }
    let center_y = (bounds.min.y + bounds.max.y) * 0.5;
    let rows: Vec<f64> = (0..row_count)
        .map(|index| center_y + (index as f64 - (row_count - 1) as f64 * 0.5) * step_over)
        .collect();
    let depths = depth_levels(material_top, *target_z, *step_down)?;
    ensure_program_budget(
        builder.commands.len(),
        depths
            .len()
            .saturating_mul(rows.len().saturating_mul(8).saturating_add(4)),
        name,
    )?;

    let left_clear_x = setup.stock.min.x - radius - safe_distance;
    let right_clear_x = setup.stock.max.x + radius + safe_distance;
    let left_cut_x = bounds.min.x - radius;
    let right_cut_x = bounds.max.x + radius;
    let one_way = !matches!(direction, FaceDirection::BothWays);
    for depth in depths {
        if one_way {
            let climb = matches!(direction, FaceDirection::Climb);
            let (enter_x, cut_start_x, cut_end_x) = if climb {
                (right_clear_x, right_cut_x, left_cut_x)
            } else {
                (left_clear_x, left_cut_x, right_cut_x)
            };
            builder.approach(Point2Dto::new(enter_x, rows[0]), depth, cutting.feed_z);
            for (index, y) in rows.iter().copied().enumerate() {
                builder.linear(Point3Dto::new(cut_start_x, y, depth), cutting.feed_xy);
                builder.linear(Point3Dto::new(cut_end_x, y, depth), cutting.feed_xy);
                if let Some(next_y) = rows.get(index + 1) {
                    builder.approach(Point2Dto::new(enter_x, *next_y), depth, cutting.feed_z);
                }
            }
        } else {
            let first = Point2Dto::new(left_clear_x, rows[0]);
            builder.approach(first, depth, cutting.feed_z);
            for (index, y) in rows.iter().copied().enumerate() {
                let x = if index % 2 == 0 {
                    right_cut_x
                } else {
                    left_cut_x
                };
                builder.linear(Point3Dto::new(x, y, depth), cutting.feed_xy);
                if let Some(next_y) = rows.get(index + 1) {
                    builder.linear(Point3Dto::new(x, *next_y, depth), cutting.feed_xy);
                }
            }
        }
        builder.retract_to_clearance();
    }
    if facing_clears_stock_floor(
        tool,
        &setup.stock,
        &rows,
        left_cut_x.max(left_clear_x),
        right_cut_x.min(right_clear_x),
    ) {
        builder.incoming_top = builder.incoming_top.min(*target_z);
    }
    Ok(())
}

fn plan_contour(
    builder: &mut ProgramBuilder,
    operation: &CamOperationDto,
    tool: &CamToolDto,
) -> Result<(), CamPlanError> {
    let CamOperationDto::Contour2d {
        path,
        closed,
        top_z,
        bottom_z,
        step_down,
        compensation,
        compensation_mode,
        lead_in,
        lead_out,
        lead_arc_radius,
        direction,
        roughing_passes,
        roughing_step_over,
        finishing_pass,
        finish_allowance,
        finish_feed,
        spring_pass,
        cutting,
        name,
        ..
    } = operation
    else {
        unreachable!();
    };
    let material_top = top_z.max(builder.incoming_top);
    require_flute_length(tool, material_top - bottom_z, name)?;
    let linking = builder.linking.clone();
    let lead_in = linking
        .as_ref()
        .map(|l| {
            if l.lead_in.enabled {
                l.lead_in.linear_distance
            } else {
                0.0
            }
        })
        .unwrap_or(*lead_in);
    let lead_out = linking
        .as_ref()
        .map(|l| {
            if l.exit().enabled {
                l.exit().linear_distance
            } else {
                0.0
            }
        })
        .unwrap_or(*lead_out);
    let radius = tool.diameter * 0.5;
    let source = without_duplicate_closure(path);

    let mut oriented = source.clone();
    let mut chain_reversed = false;
    if *closed && !matches!(compensation, ContourCompensation::On) {
        let material_inside = matches!(compensation, ContourCompensation::Outside);
        let want_ccw = m3_closed_cut_is_ccw(*direction, material_inside);
        if (signed_area(&source) > 0.0) != want_ccw {
            oriented = std::iter::once(source[0])
                .chain(source[1..].iter().rev().copied())
                .collect();
            chain_reversed = true;
        }
    } else if !*closed
        && matches!(
            compensation,
            ContourCompensation::Left | ContourCompensation::Right
        )
    {
        let physical_left = matches!(compensation, ContourCompensation::Left);
        let want_left = matches!(direction, MillingDirection::Climb);
        if physical_left != want_left {
            oriented.reverse();
            chain_reversed = true;
        }
    }

    let effective_left = match compensation {
        ContourCompensation::Left => Some(!chain_reversed),
        ContourCompensation::Right => Some(chain_reversed),
        _ => None,
    };
    let oriented_area = signed_area(&oriented);
    if let Some(hint) = linking
        .as_ref()
        .and_then(|l| l.entry_positions.first().or(l.predrill_positions.first()))
        .filter(|_| *closed)
    {
        oriented = linking_planner::split_at_hint(&oriented, *hint)?;
    }

    let comp_left = match (compensation_mode, compensation) {
        (CompensationMode::InControl, ContourCompensation::Left | ContourCompensation::Right) => {
            effective_left
        }
        (CompensationMode::InControl, ContourCompensation::Inside) => Some(oriented_area > 0.0),
        (CompensationMode::InControl, ContourCompensation::Outside) => Some(oriented_area <= 0.0),
        _ => None,
    };

    let step = if *roughing_passes > 1 {
        roughing_step_over.unwrap_or(0.0)
    } else {
        0.0
    };
    let mut extras: Vec<f64> = (0..*roughing_passes)
        .rev()
        .map(|index| {
            let mut extra = f64::from(index) * step;
            if *finishing_pass {
                extra += finish_allowance;
            }
            extra
        })
        .collect();
    if *finishing_pass {
        extras.push(0.0);
    }

    let inside_closed = *closed && matches!(compensation, ContourCompensation::Inside);

    let control_profile_path = if comp_left.is_some()
        && inside_closed
        && linking
            .as_ref()
            .is_none_or(|l| l.entry_positions.is_empty() && l.predrill_positions.is_empty())
    {
        Some(inside_control_profile_path(
            &oriented, radius, lead_in, lead_out, name,
        )?)
    } else {
        None
    };
    let profile_point_count = control_profile_path
        .as_ref()
        .map_or(source.len(), |path| path.len())
        .max(source.len());

    let bend_left = if inside_closed {
        oriented_area > 0.0
    } else if *closed {
        oriented_area < 0.0
    } else {
        effective_left.unwrap_or(false)
    };

    let ramping = linking.as_ref().is_some_and(|l| l.ramp_enabled);
    let depths = if ramping {
        vec![*bottom_z]
    } else {
        depth_levels(material_top, *bottom_z, *step_down)?
    };
    ensure_program_budget(
        builder.commands.len(),
        depths.len().saturating_mul(
            extras
                .len()
                .saturating_mul(profile_point_count.saturating_add(14)),
        ),
        name,
    )?;

    let mut passes = Vec::with_capacity(extras.len());
    for extra in extras.iter().copied() {
        let profile_pass = extra <= EPSILON;
        let use_comp = comp_left.is_some() && profile_pass;

        let mut center_path = if use_comp {
            control_profile_path.as_ref().unwrap_or(&oriented).clone()
        } else if matches!(compensation, ContourCompensation::On) {
            oriented.clone()
        } else {
            match compensation {
                ContourCompensation::On => unreachable!(),
                ContourCompensation::Inside => offset_polygon(&oriented, radius + extra, true)?,
                ContourCompensation::Outside => offset_polygon(&oriented, radius + extra, false)?,
                ContourCompensation::Left | ContourCompensation::Right => {
                    offset_polyline_open(&oriented, radius + extra, effective_left.unwrap())?
                }
            }
        };
        if inside_closed {
            let physical_profile = if use_comp {
                offset_polygon(&center_path, radius, true)?
            } else {
                center_path.clone()
            };
            if signed_area(&physical_profile) * oriented_area <= EPSILON
                || !inward_offset_is_clear(&physical_profile, &oriented, radius)
            {
                return Err(CamPlanError(format!(
                        "contour operation '{name}' cannot fit the physical {:.3} mm cutter inside the selected profile; check opposite walls or use a smaller tool",
                        tool.diameter
                    )));
            }
            if !use_comp
                && linking
                    .as_ref()
                    .is_none_or(|l| l.entry_positions.is_empty() && l.predrill_positions.is_empty())
            {
                center_path =
                    inside_control_profile_path(&center_path, 0.0, lead_in, lead_out, name)?;
            }
        }
        let feed = if profile_pass && *finishing_pass {
            finish_feed.unwrap_or(cutting.feed_xy)
        } else {
            cutting.feed_xy
        };
        let pass_comp = comp_left.filter(|_| use_comp);
        let mut pass_closed = *closed;
        if let Some(hint) = linking
            .as_ref()
            .and_then(|l| l.exit_positions.first())
            .filter(|_| *closed)
        {
            if *spring_pass || ramping {
                return Err(CamPlanError("Separate contour exit positions cannot be combined with spring passes or profile ramps; use the entry station as the exit.".into()));
            }
            center_path = linking_planner::extend_to_exit(&center_path, *hint);
            pass_closed = false;
        }
        let options = ContourLeadOptions {
            closed: pass_closed,
            inside_closed,
            lead_in,
            lead_out,
            arc_radius: *lead_arc_radius,
            bend_left,
            control_compensation: pass_comp.map(|left| (left, radius)),
        };
        let leads = if let Some(link) = &linking {
            linking_planner::contour_leads(&center_path, options, link)?
        } else {
            contour_leads(&center_path, options)?
        };

        let physical_leads =
            physical_contour_leads(&leads, &center_path, pass_comp.map(|left| (left, radius)))?;
        let mut checked_leads = physical_leads.clone();
        let start_tangent = if distance_2d(physical_leads.start, physical_leads.line_end) > EPSILON
        {
            unit_direction(physical_leads.start, physical_leads.line_end)?
        } else if let Some(arc) = &physical_leads.start_arc {
            linking_planner::arc_tangent(physical_leads.line_end, arc)
        } else {
            unit_direction(center_path[0], center_path[1])?
        };
        let end_anchor = physical_leads.end_arc.as_ref().map_or(
            if pass_closed {
                physical_leads
                    .start_arc
                    .as_ref()
                    .map_or(physical_leads.line_end, |a| a.arc_end)
            } else {
                *center_path.last().unwrap()
            },
            |a| a.arc_end,
        );
        let end_tangent = if distance_2d(end_anchor, physical_leads.end) > EPSILON {
            unit_direction(end_anchor, physical_leads.end)?
        } else if let Some(arc) = &physical_leads.end_arc {
            linking_planner::arc_tangent(arc.arc_end, arc)
        } else {
            unit_direction(
                center_path[center_path.len() - 2],
                *center_path.last().unwrap(),
            )?
        };
        let rin = linking.as_ref().map_or(0.0, |l| {
            if l.lead_in.enabled {
                l.lead_in.vertical_radius
            } else {
                0.0
            }
        });
        let rout = linking.as_ref().map_or(0.0, |l| {
            if l.exit().enabled {
                l.exit().vertical_radius
            } else {
                0.0
            }
        });
        checked_leads.start = Point2Dto::new(
            leads.start.x - start_tangent.x * rin,
            leads.start.y - start_tangent.y * rin,
        );
        checked_leads.end = Point2Dto::new(
            leads.end.x + end_tangent.x * rout,
            leads.end.y + end_tangent.y * rout,
        );
        let physical_profile_end = {
            let (p, t) = if pass_closed {
                (
                    center_path[0],
                    unit_direction(*center_path.last().unwrap(), center_path[0])?,
                )
            } else {
                (
                    *center_path.last().unwrap(),
                    unit_direction(
                        center_path[center_path.len() - 2],
                        *center_path.last().unwrap(),
                    )?,
                )
            };
            if let Some(left) = pass_comp {
                let sign = if left { 1.0 } else { -1.0 };
                Point2Dto::new(p.x - sign * radius * t.y, p.y + sign * radius * t.x)
            } else {
                p
            }
        };
        if *closed
            && matches!(
                compensation,
                ContourCompensation::Inside | ContourCompensation::Outside
            )
            && !linking_planner::leads_clear(
                &checked_leads,
                physical_profile_end,
                &oriented,
                matches!(compensation, ContourCompensation::Outside),
                radius,
            )
        {
            return Err(CamPlanError(format!(
                    "contour operation '{name}' cannot fit its lead without crossing the selected protected profile; shorten the lead/arc, use a smaller tool, or choose a clearer start edge"
                )));
        }
        if !*closed && !matches!(compensation, ContourCompensation::On) {
            if !open_leads_clear_profile(
                &checked_leads,
                &center_path,
                &oriented,
                radius,
                pass_comp,
            )? {
                return Err(CamPlanError(format!("contour operation '{name}' cannot fit its physical leads clear of the selected open chain; shorten the leads or choose a clearer station")));
            }
            builder.warnings.push(format!("Contour '{name}' checks clearance from the selected open chain only; neighboring geometry and the material beyond its endpoints require target verification."));
        }

        builder.require_clear_approach(checked_leads.start, radius, name)?;
        if let Some(link) = &linking {
            if !link.predrill_positions.is_empty()
                && !builder.predrilled.iter().any(|h| {
                    h.bottom <= *bottom_z + EPSILON
                        && distance_2d(h.center, checked_leads.start) + radius <= h.radius - 1e-5
                        && link
                            .predrill_positions
                            .iter()
                            .any(|&p| distance_2d(p, h.center) < 1e-5)
                })
            {
                return Err(CamPlanError("The selected predrill does not clear the actual contour entry to full cutter diameter and depth. Adjust its entry station/leads or move a suitable drilling operation earlier.".into()));
            }
        }
        passes.push((
            center_path,
            leads,
            pass_comp,
            feed,
            profile_pass,
            pass_closed,
            start_tangent,
            end_tangent,
            rin,
            rout,
        ));
    }
    for depth in depths {
        for (
            center_path,
            leads,
            pass_comp,
            feed,
            profile_pass,
            pass_closed,
            start_tangent,
            end_tangent,
            rin,
            rout,
        ) in &passes
        {
            let feed = *feed;
            let entry_depth = if ramping {
                material_top + linking.as_ref().unwrap().ramp_clearance
            } else {
                depth
            };
            let entry_feed = linking.as_ref().map_or(feed, |l| l.lead_in_feed);
            let exit_feed = linking.as_ref().map_or(feed, |l| l.lead_out_feed);
            if linking.is_some() {
                linking_planner::entry(
                    builder,
                    leads.start,
                    *start_tangent,
                    entry_depth,
                    *rin,
                    cutting.feed_z,
                    entry_feed,
                )?;
            } else {
                builder.approach(leads.start, depth, cutting.feed_z);
            }
            if let Some(left) = pass_comp {
                builder
                    .commands
                    .push(CamCommandDto::CutterCompensationOn { left: *left });
            }
            builder.linear(
                Point3Dto::new(leads.line_end.x, leads.line_end.y, entry_depth),
                entry_feed,
            );
            if let Some(arc) = &leads.start_arc {
                builder.circular(
                    Point3Dto::new(center_path[0].x, center_path[0].y, entry_depth),
                    arc.center,
                    arc.clockwise,
                    entry_feed,
                );
            }
            if ramping {
                linking_planner::ramp_profile(
                    builder,
                    center_path,
                    *pass_closed,
                    entry_depth,
                    depth,
                    linking.as_ref().unwrap(),
                )?;
            }
            emit_profile_lap(builder, center_path, *pass_closed, depth, feed);

            if *spring_pass && *profile_pass {
                emit_profile_lap(builder, center_path, *pass_closed, depth, feed);
            }
            if let Some(arc) = &leads.end_arc {
                builder.circular(
                    Point3Dto::new(arc.arc_end.x, arc.arc_end.y, depth),
                    arc.center,
                    arc.clockwise,
                    exit_feed,
                );
            }
            if pass_comp.is_some() {
                builder.commands.push(CamCommandDto::CutterCompensationOff);
            }
            builder.linear(Point3Dto::new(leads.end.x, leads.end.y, depth), exit_feed);
            if linking.is_some() {
                linking_planner::exit(builder, leads.end, *end_tangent, depth, *rout, exit_feed)?;
            }
            if linking.as_ref().is_none_or(|l| !l.keep_tool_down) {
                builder.retract_to_clearance();
            }
        }
    }
    builder.retract_to_clearance();
    Ok(())
}

/// One trip around (or along) the profile at a constant depth.
fn emit_profile_lap(
    builder: &mut ProgramBuilder,
    center_path: &[Point2Dto],
    closed: bool,
    depth: f64,
    feed: f64,
) {
    for point in center_path.iter().copied().skip(1) {
        builder.linear(Point3Dto::new(point.x, point.y, depth), feed);
    }

    if closed {
        builder.linear(
            Point3Dto::new(center_path[0].x, center_path[0].y, depth),
            feed,
        );
    }
}

/// Rotate a closed inside profile onto a straight edge that has room for the
/// cutter and both physical leads. Activating compensation at a
/// polygon vertex is ambiguous under normal controller approach behavior:
/// the offset is normal to the outgoing edge and the cutter can overlap the
/// incoming wall before the first contour block runs.
fn inside_control_profile_path(
    points: &[Point2Dto],
    tool_radius: f64,
    lead_in: f64,
    lead_out: f64,
    operation_name: &str,
) -> Result<Vec<Point2Dto>, CamPlanError> {
    let Some((edge_index, edge_length)) = (0..points.len())
        .map(|index| {
            let next = (index + 1) % points.len();
            (index, distance_2d(points[index], points[next]))
        })
        .max_by(|left, right| left.1.total_cmp(&right.1))
    else {
        return Err(CamPlanError(format!(
            "contour operation '{operation_name}' has no edge for cutter compensation activation"
        )));
    };

    let required_length = tool_radius * 2.0 + lead_in + lead_out;
    if edge_length < required_length - EPSILON {
        return Err(CamPlanError(format!(
            "contour operation '{operation_name}' cannot fit its inside leads: a straight edge at least {required_length:.3} mm long is required; shorten the leads or use a smaller tool"
        )));
    }
    let next_index = (edge_index + 1) % points.len();
    let edge_start = points[edge_index];
    let edge_end = points[next_index];
    let tangent = Point2Dto::new(
        (edge_end.x - edge_start.x) / edge_length,
        (edge_end.y - edge_start.y) / edge_length,
    );
    let spare = edge_length - required_length;
    let start_distance = tool_radius + lead_in + spare * 0.5;
    let start = Point2Dto::new(
        edge_start.x + tangent.x * start_distance,
        edge_start.y + tangent.y * start_distance,
    );
    let mut result = Vec::with_capacity(points.len() + 1);
    result.push(start);
    let mut index = next_index;
    while index != edge_index {
        result.push(points[index]);
        index = (index + 1) % points.len();
    }
    result.push(points[edge_index]);
    Ok(result)
}

/// A 90 degree horizontal arc closing a lead onto (or off) the profile.
#[derive(Clone)]
struct LeadArc {
    center: Point2Dto,
    clockwise: bool,
    /// Where the lead-OUT arc ends (the straight lead-out continues from
    /// here); unused on the lead-in, whose arc ends at the profile start.
    arc_end: Point2Dto,
}

/// Entry/exit geometry of one contour pass: the straight lead segment
/// (carrying the compensation activation/cancellation), optionally rounded
/// into a 90 degree tangential arc, and the endpoints.
#[derive(Clone)]
struct ContourLeads {
    /// Plunge point: the uncompensated position from which the controller's
    /// activation move reaches the compensated lead/profile start.
    start: Point2Dto,
    /// Where the straight lead-in ends: the arc start, or the profile start
    /// when no arc is used.
    line_end: Point2Dto,
    start_arc: Option<LeadArc>,
    end_arc: Option<LeadArc>,
    /// Final uncompensated point reached while compensation is cancelled.
    end: Point2Dto,
}

#[derive(Clone, Copy)]
struct ContourLeadOptions {
    closed: bool,
    inside_closed: bool,
    lead_in: f64,
    lead_out: f64,
    arc_radius: Option<f64>,
    bend_left: bool,
    control_compensation: Option<(bool, f64)>,
}

/// Build the lead geometry for one contour pass. Tangent leads extend the
/// end segments straight; inside profiles must first be split on a straight
/// edge. These are candidates, not a clearance guarantee: callers verify the
/// entire physical lead against the selected protected profile.
/// Everywhere else an optional 90 degree arc rounds the straight lead into
/// a tangential meet with the profile, so the tool arrives (and leaves) at
/// full offset without sliding along the wall line. For machine-side
/// compensation, lead lengths and arc radius describe the physical cutter-
/// center path. The programmed arc is enlarged by the tool radius and the
/// uncompensated entry/exit points are shifted onto that center path; the
/// controller's normal approach/retract behavior then reconstructs exactly
/// the requested safe motion without gouging the profile corner.
fn contour_leads(
    center_path: &[Point2Dto],
    options: ContourLeadOptions,
) -> Result<ContourLeads, CamPlanError> {
    let ContourLeadOptions {
        closed,
        inside_closed,
        lead_in,
        lead_out,
        arc_radius,
        bend_left,
        control_compensation,
    } = options;
    let first = center_path[0];
    let start_tangent = unit_direction(center_path[0], center_path[1])?;
    let last_index = center_path.len() - 1;
    let (end_anchor, end_tangent) = if closed {
        (first, unit_direction(center_path[last_index], first)?)
    } else {
        (
            center_path[last_index],
            unit_direction(center_path[last_index - 1], center_path[last_index])?,
        )
    };
    let compensated_point = |point: Point2Dto, tangent: Point2Dto| {
        let Some((left, radius)) = control_compensation else {
            return point;
        };
        let normal = if left {
            Point2Dto::new(-tangent.y, tangent.x)
        } else {
            Point2Dto::new(tangent.y, -tangent.x)
        };
        Point2Dto::new(point.x + normal.x * radius, point.y + normal.y * radius)
    };
    if inside_closed {
        let straight_start = (start_tangent.x - end_tangent.x).abs() <= EPSILON
            && (start_tangent.y - end_tangent.y).abs() <= EPSILON;
        if !straight_start {
            return Err(CamPlanError(
                "Inside contour leads require a straight entry station, not a corner".into(),
            ));
        }
    }
    let arc = arc_radius
        .filter(|radius| radius.is_finite() && *radius > EPSILON)
        .map(|radius| {
            if let Some((left, tool_radius)) = control_compensation {
                debug_assert_eq!(left, bend_left);
                radius + tool_radius
            } else {
                radius
            }
        });
    let normal = |tangent: Point2Dto| {
        if bend_left {
            Point2Dto::new(-tangent.y, tangent.x)
        } else {
            Point2Dto::new(tangent.y, -tangent.x)
        }
    };
    let (start, line_end, start_arc) = match arc {
        Some(radius) => {
            let n = normal(start_tangent);
            let center = Point2Dto::new(first.x + n.x * radius, first.y + n.y * radius);
            let v0 = Point2Dto::new(first.x - center.x, first.y - center.y);

            let vs = if bend_left {
                Point2Dto::new(v0.y, -v0.x)
            } else {
                Point2Dto::new(-v0.y, v0.x)
            };
            let arc_start = Point2Dto::new(center.x + vs.x, center.y + vs.y);
            let tangent = Point2Dto::new(-n.x, -n.y);
            let compensated_start = compensated_point(arc_start, tangent);
            (
                Point2Dto::new(
                    compensated_start.x + n.x * lead_in,
                    compensated_start.y + n.y * lead_in,
                ),
                arc_start,
                Some(LeadArc {
                    center,
                    clockwise: !bend_left,
                    arc_end: first,
                }),
            )
        }
        None => {
            let compensated_start = compensated_point(first, start_tangent);
            (
                Point2Dto::new(
                    compensated_start.x - start_tangent.x * lead_in,
                    compensated_start.y - start_tangent.y * lead_in,
                ),
                first,
                None,
            )
        }
    };
    let (end, end_arc) = match arc {
        Some(radius) => {
            let n = normal(end_tangent);
            let center = Point2Dto::new(end_anchor.x + n.x * radius, end_anchor.y + n.y * radius);
            let w0 = Point2Dto::new(end_anchor.x - center.x, end_anchor.y - center.y);

            let w1 = if bend_left {
                Point2Dto::new(-w0.y, w0.x)
            } else {
                Point2Dto::new(w0.y, -w0.x)
            };
            let arc_end = Point2Dto::new(center.x + w1.x, center.y + w1.y);
            let compensated_end = compensated_point(arc_end, n);
            (
                Point2Dto::new(
                    compensated_end.x + n.x * lead_out,
                    compensated_end.y + n.y * lead_out,
                ),
                Some(LeadArc {
                    center,
                    clockwise: !bend_left,
                    arc_end,
                }),
            )
        }
        None => {
            let compensated_end = compensated_point(end_anchor, end_tangent);
            (
                Point2Dto::new(
                    compensated_end.x + end_tangent.x * lead_out,
                    compensated_end.y + end_tangent.y * lead_out,
                ),
                None,
            )
        }
    };
    Ok(ContourLeads {
        start,
        line_end,
        start_arc,
        end_arc,
        end,
    })
}

fn plan_drill(
    builder: &mut ProgramBuilder,
    operation: &CamOperationDto,
    tool: &CamToolDto,
) -> Result<(), CamPlanError> {
    let CamOperationDto::Drill {
        points,
        holes,
        top_z,
        bottom_z,
        retract_z,
        cycle,
        peck_depth,
        peck_retract,
        thread_pitch,
        floating_tap_holder,
        feed_out,
        dwell_seconds,
        drill_tip_through,
        breakthrough_depth,
        cutting,
        name,
        ..
    } = operation
    else {
        unreachable!();
    };

    let mut targets: Vec<(Point2Dto, f64, f64)> = holes
        .iter()
        .map(|hole| (hole.point, hole.top_z, hole.bottom_z))
        .collect();
    targets.extend(points.iter().map(|point| (*point, *top_z, *bottom_z)));

    let tip_length = if *drill_tip_through {
        let half_angle = tool.point_angle_degrees.unwrap_or(118.0).to_radians() * 0.5;
        (tool.diameter * 0.5) / half_angle.tan().max(1.0e-6) + *breakthrough_depth
    } else {
        0.0
    };
    let deepest_travel = targets
        .iter()
        .map(|(_, top, bottom)| top.max(builder.incoming_top) - bottom + tip_length)
        .fold(0.0_f64, f64::max);
    require_flute_length(tool, deepest_travel, name)?;
    let pecking = matches!(cycle, DrillCycle::ChipBreaking | DrillCycle::DeepHole);

    let peck = if pecking {
        Some(peck_depth.ok_or_else(|| {
            CamPlanError(format!(
                "drill operation '{name}' pecking cycles require a peck depth"
            ))
        })?)
    } else {
        None
    };
    let partial_retract = match cycle {
        DrillCycle::ChipBreaking => {
            Some(peck_retract.unwrap_or(0.5_f64.min(peck.expect("pecking cycle") * 0.5)))
        }
        _ => None,
    };
    let tap_feed = match cycle {
        DrillCycle::TappingRight | DrillCycle::TappingLeft => {
            let pitch = thread_pitch.ok_or_else(|| {
                CamPlanError(format!(
                    "tapping operation '{name}' requires a thread pitch"
                ))
            })?;

            Some(pitch * f64::from(cutting.spindle_rpm))
        }
        _ => None,
    };
    if tap_feed.is_some() {
        if !floating_tap_holder {
            return Err(CamPlanError(format!(
                "tapping operation '{name}' has no confirmed floating-holder contract; ordinary feed motion cannot guarantee spindle synchronization"
            )));
        }
        builder.warnings.push(format!(
            "Tapping operation '{name}' uses explicit longhand feed/reverse motion under the confirmed floating-holder contract. It is not rigid tapping; verify holder travel, spindle reversal, and feed on the actual control."
        ));
    }
    for (point, hole_top, hole_bottom) in targets {
        let cut_bottom = hole_bottom - tip_length;
        let depths = match peck {
            Some(peck) => depth_levels(hole_top.max(builder.incoming_top), cut_bottom, peck)?,
            None => vec![cut_bottom],
        };
        ensure_program_budget(
            builder.commands.len(),
            depths.len().saturating_mul(3).saturating_add(5),
            name,
        )?;
        builder.retract_to_clearance();
        builder.rapid(Point3Dto::new(point.x, point.y, builder.clearance_z));
        builder.rapid(Point3Dto::new(point.x, point.y, *retract_z));

        let first_depth = depths.first().copied().unwrap_or(cut_bottom);
        builder.rapid(Point3Dto::new(
            point.x,
            point.y,
            builder.feed_plane(first_depth),
        ));
        match cycle {
            DrillCycle::Drill => {
                builder.linear(Point3Dto::new(point.x, point.y, cut_bottom), cutting.feed_z);
                builder.dwell(*dwell_seconds);
                builder.rapid(Point3Dto::new(point.x, point.y, *retract_z));
            }
            DrillCycle::ChipBreaking | DrillCycle::DeepHole => {
                for (index, depth) in depths.iter().copied().enumerate() {
                    builder.linear(Point3Dto::new(point.x, point.y, depth), cutting.feed_z);
                    builder.dwell(*dwell_seconds);
                    if index + 1 < depths.len() {
                        let back = match partial_retract {
                            Some(retract) => depth + retract,

                            None => *retract_z,
                        };
                        builder.rapid(Point3Dto::new(point.x, point.y, back));

                        if partial_retract.is_none() {
                            let re_entry = (depth + 0.5).min(*retract_z);
                            builder.rapid(Point3Dto::new(point.x, point.y, re_entry));
                        }
                    }
                }
                builder.rapid(Point3Dto::new(point.x, point.y, *retract_z));
            }
            DrillCycle::TappingRight | DrillCycle::TappingLeft => {
                let feed = tap_feed.expect("tap feed computed above");
                let (in_direction, out_direction) = match cycle {
                    DrillCycle::TappingRight => (
                        SpindleDirection::Clockwise,
                        SpindleDirection::Counterclockwise,
                    ),
                    _ => (
                        SpindleDirection::Counterclockwise,
                        SpindleDirection::Clockwise,
                    ),
                };
                builder.spindle(in_direction, cutting.spindle_rpm);
                builder.linear(Point3Dto::new(point.x, point.y, cut_bottom), feed);
                builder.spindle(out_direction, cutting.spindle_rpm);
                builder.linear(Point3Dto::new(point.x, point.y, *retract_z), feed);

                builder.spindle(SpindleDirection::Clockwise, cutting.spindle_rpm);
            }
            DrillCycle::Reaming | DrillCycle::Boring => {
                builder.linear(Point3Dto::new(point.x, point.y, cut_bottom), cutting.feed_z);
                builder.dwell(*dwell_seconds);
                builder.linear(
                    Point3Dto::new(point.x, point.y, *retract_z),
                    feed_out.unwrap_or(cutting.feed_z),
                );
            }
        }
    }
    builder.retract_to_clearance();
    Ok(())
}

/// Clear a closed pocket with zigzag scanlines and finish the wall with a
/// boundary pass at every depth. The operator-selected outline is offset
/// inward by the tool radius, so every generated point keeps the tool fully
/// inside the pocket; entry is a plunge, which validation restricts to
/// center-cutting tools until ramp or helical entries exist.
fn plan_pocket(
    builder: &mut ProgramBuilder,
    operation: &CamOperationDto,
    tool: &CamToolDto,
) -> Result<(), CamPlanError> {
    let CamOperationDto::Pocket2d {
        outline,
        top_z,
        bottom_z,
        step_down,
        step_over,
        direction,
        cutting,
        name,
        ..
    } = operation
    else {
        unreachable!();
    };
    let material_top = top_z.max(builder.incoming_top);
    require_flute_length(tool, material_top - bottom_z, name)?;
    let boundary = without_duplicate_closure(outline);
    let mut clearing = offset_polygon(&boundary, tool.diameter * 0.5, true)?;

    if signed_area(&clearing) * signed_area(&boundary) <= EPSILON
        || !inward_offset_is_clear(&clearing, &boundary, tool.diameter * 0.5)
    {
        return Err(CamPlanError(format!(
            "pocket operation '{name}' is too small for tool {}'s {:.3} mm diameter",
            tool.label(),
            tool.diameter
        )));
    }

    let want_ccw = m3_closed_cut_is_ccw(*direction, false);
    if (signed_area(&clearing) > 0.0) != want_ccw {
        clearing = std::iter::once(clearing[0])
            .chain(clearing[1..].iter().rev().copied())
            .collect();
    }
    let bounds = polygon_bounds(&clearing);
    let rows = inclusive_steps(bounds.min.y, bounds.max.y, *step_over)?;
    let spans_per_row = rows
        .iter()
        .map(|y| scanline_spans(&clearing, *y))
        .collect::<Vec<_>>();
    let depths = depth_levels(material_top, *bottom_z, *step_down)?;
    ensure_program_budget(
        builder.commands.len(),
        depths.len().saturating_mul(
            spans_per_row
                .iter()
                .map(|spans| spans.len().saturating_mul(8))
                .sum::<usize>()
                .saturating_add(clearing.len())
                .saturating_add(6),
        ),
        name,
    )?;
    for depth in depths {
        let mut span_index = 0usize;
        let mut entered = false;
        for (row_index, y) in rows.iter().copied().enumerate() {
            for (x0, x1) in spans_per_row[row_index].iter().copied() {
                let (start_x, end_x) = if span_index.is_multiple_of(2) {
                    (x0, x1)
                } else {
                    (x1, x0)
                };

                builder.approach(Point2Dto::new(start_x, y), depth, cutting.feed_z);
                entered = true;
                builder.linear(Point3Dto::new(end_x, y, depth), cutting.feed_xy);
                span_index += 1;
            }
        }
        if !entered {
            return Err(CamPlanError(format!(
                "pocket operation '{name}' has no machinable area at the selected stepover"
            )));
        }

        let (finish, leads) = pocket_finish_leads(&clearing, &boundary, tool.diameter * 0.5, name)?;
        builder.approach(leads.start, depth, cutting.feed_z);
        builder.linear(
            Point3Dto::new(leads.line_end.x, leads.line_end.y, depth),
            cutting.feed_xy,
        );
        let entry = leads.start_arc.as_ref().expect("tangent finish arc");
        builder.circular(
            Point3Dto::new(finish[0].x, finish[0].y, depth),
            entry.center,
            entry.clockwise,
            cutting.feed_xy,
        );
        emit_profile_lap(builder, &finish, true, depth, cutting.feed_xy);
        let exit = leads.end_arc.as_ref().expect("tangent finish arc");
        builder.circular(
            Point3Dto::new(exit.arc_end.x, exit.arc_end.y, depth),
            exit.center,
            exit.clockwise,
            cutting.feed_xy,
        );
        builder.linear(
            Point3Dto::new(leads.end.x, leads.end.y, depth),
            cutting.feed_xy,
        );
        builder.retract_to_clearance();
    }
    Ok(())
}

fn pocket_finish_leads(
    clearing: &[Point2Dto],
    boundary: &[Point2Dto],
    radius: f64,
    name: &str,
) -> Result<(Vec<Point2Dto>, ContourLeads), CamPlanError> {
    let mut edges: Vec<_> = (0..clearing.len()).collect();
    edges.sort_by(|&a, &b| {
        distance_2d(clearing[b], clearing[(b + 1) % clearing.len()]).total_cmp(&distance_2d(
            clearing[a],
            clearing[(a + 1) % clearing.len()],
        ))
    });
    for edge in edges.into_iter().take(8) {
        let mut path = vec![Point2Dto::new(
            (clearing[edge].x + clearing[(edge + 1) % clearing.len()].x) * 0.5,
            (clearing[edge].y + clearing[(edge + 1) % clearing.len()].y) * 0.5,
        )];
        path.extend((1..=clearing.len()).map(|offset| clearing[(edge + offset) % clearing.len()]));
        for factor in [0.5, 0.25, 0.125, 0.0625] {
            let rho = radius * factor;
            let leads = contour_leads(
                &path,
                ContourLeadOptions {
                    closed: true,
                    inside_closed: true,
                    lead_in: rho,
                    lead_out: rho,
                    arc_radius: Some(rho),
                    bend_left: signed_area(clearing) > 0.0,
                    control_compensation: None,
                },
            )?;
            if chamfer_leads_clear_profile(&leads, boundary, false, radius) {
                return Ok((path, leads));
            }
        }
    }
    Err(CamPlanError(format!("pocket operation '{name}' has no certified tangent wall-finishing lead at the tested stations; use a smaller tool or a wider pocket")))
}

/// Single-pass 45 degree chamfer with a 90 degree chamfer mill. With the tip
/// `tip_offset` past the chamfer root, the tip runs `chamfer_width +
/// tip_offset` below the top. A sharp profile offsets by `tip_offset`;
/// a modeled upper rim offsets by its measured width plus `tip_offset`.
fn plan_chamfer(
    builder: &mut ProgramBuilder,
    operation: &CamOperationDto,
    tool: &CamToolDto,
) -> Result<(), CamPlanError> {
    if matches!(operation, CamOperationDto::Chamfer2d { additional_chains, .. } if !additional_chains.is_empty())
    {
        for (i, chain) in operation.chamfer_chains().into_iter().enumerate() {
            plan_chamfer(builder, &operation.with_chamfer_chain(chain), tool)
                .map_err(|error| CamPlanError(format!("Chain {}: {}", i + 1, error.0)))?;
        }
        return Ok(());
    }
    let CamOperationDto::Chamfer2d {
        path,
        closed,
        top_z,
        chamfer_width,
        tip_offset,
        modeled_chamfer,
        wall_side,
        direction,
        cutting,
        name,
        ..
    } = operation
    else {
        unreachable!();
    };
    let source = if *closed {
        without_duplicate_closure(path)
    } else {
        path.clone()
    };
    let material_inside = matches!(wall_side, ContourCompensation::Inside);

    let profile_offset = tip_offset
        + modeled_chamfer
            .as_ref()
            .map_or(0., |m| chamfer_width - m.additional_width);
    let mut tool_left = matches!(wall_side, ContourCompensation::Right);
    let mut center_path = if *closed {
        offset_polygon(&source, profile_offset, !material_inside)?
    } else {
        offset_polyline_open(&source, profile_offset, tool_left)?
    };
    if !chamfer_profile_is_clear(
        &center_path,
        &source,
        *closed,
        material_inside,
        profile_offset,
    ) {
        return Err(CamPlanError(format!(
            "chamfer operation '{name}' produces a folded or wall-crossing offset; reduce the tip offset or split the selection into clearer chains"
        )));
    }

    let want_ccw = m3_closed_cut_is_ccw(*direction, material_inside);
    if *closed && (signed_area(&center_path) > 0.0) != want_ccw {
        center_path = std::iter::once(center_path[0])
            .chain(center_path[1..].iter().rev().copied())
            .collect();
    }

    if *closed {
        center_path = split_closed_path_on_longest_edge(&center_path)?;
    } else if tool_left != matches!(direction, crate::model::MillingDirection::Climb) {
        center_path.reverse();
        tool_left = !tool_left;
    }
    let depth = top_z - (chamfer_width + tip_offset);
    let bend_left = if *closed {
        (signed_area(&center_path) > 0.0) != material_inside
    } else {
        tool_left
    };
    if builder.linking.is_some() {
        return linking_planner::plan_chamfer(
            builder,
            operation,
            tool,
            &center_path,
            &source,
            profile_offset,
            bend_left,
        );
    }

    let lead_radius = (tool.diameter * 0.25)
        .max(*tip_offset)
        .min(tool.diameter * 0.5);

    let mut fitted = None;
    for scale in [1., 0.75, 0.5, 0.25, 0.125, 0.0625] {
        let radius = lead_radius * scale;
        let leads = contour_leads(
            &center_path,
            ContourLeadOptions {
                closed: *closed,
                inside_closed: false,
                lead_in: radius,
                lead_out: radius,
                arc_radius: Some(radius),
                bend_left,
                control_compensation: None,
            },
        )?;
        let clear = if *closed {
            chamfer_leads_clear_profile(&leads, &source, material_inside, profile_offset)
        } else {
            open_leads_clear_profile(&leads, &center_path, &source, profile_offset, None)?
        };
        if clear {
            fitted = Some((leads, radius));
            break;
        }
    }
    let (leads, fitted_radius) = fitted.ok_or_else(|| CamPlanError(format!(
        "chamfer operation '{name}' cannot fit a clearance-checked tangent entry/exit at the tested sizes; reduce tip offset, use a smaller tool, or select a clearer edge"
    )))?;
    if fitted_radius < lead_radius - EPSILON {
        builder.warnings.push(format!("Chamfer '{name}': entry/exit radius and straight extension reduced from {lead_radius:.3} to {fitted_radius:.3} mm to fit the selected boundary; cut width and tip offset are unchanged."));
    }
    builder.require_clear_approach(leads.start, tool.diameter * 0.5, name)?;
    builder.approach(leads.start, depth, cutting.feed_z);
    builder.linear(
        Point3Dto::new(leads.line_end.x, leads.line_end.y, depth),
        cutting.feed_xy,
    );
    if let Some(arc) = &leads.start_arc {
        builder.circular(
            Point3Dto::new(center_path[0].x, center_path[0].y, depth),
            arc.center,
            arc.clockwise,
            cutting.feed_xy,
        );
    }
    emit_profile_lap(builder, &center_path, *closed, depth, cutting.feed_xy);
    if let Some(arc) = &leads.end_arc {
        builder.circular(
            Point3Dto::new(arc.arc_end.x, arc.arc_end.y, depth),
            arc.center,
            arc.clockwise,
            cutting.feed_xy,
        );
    }
    builder.linear(
        Point3Dto::new(leads.end.x, leads.end.y, depth),
        cutting.feed_xy,
    );
    builder.retract_to_clearance();
    Ok(())
}

/// Certify complete center segments, not only offset vertices. A narrow
/// concavity can fold a miter back across the selected wall between vertices.
fn chamfer_profile_is_clear(
    path: &[Point2Dto],
    boundary: &[Point2Dto],
    closed: bool,
    material_inside: bool,
    minimum_offset: f64,
) -> bool {
    let simple = |points: &[Point2Dto]| {
        let count = points.len() - usize::from(!closed);
        (0..count).all(|i| {
            (i + 1..count).all(|j| {
                let next_i = (i + 1) % points.len();
                let next_j = (j + 1) % points.len();
                next_i == j
                    || next_j == i
                    || !segments_intersect(points[i], points[next_i], points[j], points[next_j])
            })
        })
    };
    if !simple(path) || !simple(boundary) {
        return false;
    }
    let count = path.len() - usize::from(!closed);
    let boundary_count = boundary.len() - usize::from(!closed);
    (0..count).all(|i| {
        let from = path[i];
        let to = path[(i + 1) % path.len()];
        (!closed || point_in_polygon(from, boundary) != material_inside)
            && (0..boundary_count).all(|j| {
                segment_segment_distance(from, to, boundary[j], boundary[(j + 1) % boundary.len()])
                    >= minimum_offset - 1e-6
            })
    })
}

fn split_closed_path_on_longest_edge(points: &[Point2Dto]) -> Result<Vec<Point2Dto>, CamPlanError> {
    let (edge_index, edge_length) = (0..points.len())
        .map(|index| {
            let next = (index + 1) % points.len();
            (index, distance_2d(points[index], points[next]))
        })
        .max_by(|left, right| left.1.total_cmp(&right.1))
        .ok_or_else(|| CamPlanError("closed path has no usable lead station".into()))?;
    if edge_length <= EPSILON {
        return Err(CamPlanError(
            "closed path has no usable lead station".into(),
        ));
    }
    let next_index = (edge_index + 1) % points.len();
    let start = Point2Dto::new(
        (points[edge_index].x + points[next_index].x) * 0.5,
        (points[edge_index].y + points[next_index].y) * 0.5,
    );
    let mut result = Vec::with_capacity(points.len() + 1);
    result.push(start);
    let mut index = next_index;
    while index != edge_index {
        result.push(points[index]);
        index = (index + 1) % points.len();
    }
    result.push(points[edge_index]);
    Ok(result)
}

fn chamfer_leads_clear_profile(
    leads: &ContourLeads,
    profile: &[Point2Dto],
    material_inside: bool,
    minimum_offset: f64,
) -> bool {
    let safe = |point: Point2Dto| {
        let inside = point_in_polygon(point, profile);
        let side_ok = if material_inside { !inside } else { inside };
        side_ok
            && (0..profile.len()).all(|index| {
                segment_distance(point, profile[index], profile[(index + 1) % profile.len()])
                    >= minimum_offset - 1.0e-6
            })
    };
    let line_safe = |from: Point2Dto, to: Point2Dto| {
        safe(from)
            && safe(to)
            && (0..profile.len()).all(|i| {
                segment_segment_distance(from, to, profile[i], profile[(i + 1) % profile.len()])
                    >= minimum_offset - 1e-6
            })
    };
    let arc_safe = |from: Point2Dto, arc: &LeadArc| {
        safe(from)
            && safe(arc.arc_end)
            && (0..profile.len()).all(|i| {
                arc_segment_distance(from, arc, profile[i], profile[(i + 1) % profile.len()])
                    >= minimum_offset - 1e-6
            })
    };
    if !line_safe(leads.start, leads.line_end) {
        return false;
    }
    if let Some(arc) = &leads.start_arc {
        if !arc_safe(leads.line_end, arc) {
            return false;
        }
    }
    let profile_start = leads
        .start_arc
        .as_ref()
        .map_or(leads.line_end, |arc| arc.arc_end);
    let exit_start = profile_start;
    if let Some(arc) = &leads.end_arc {
        if !arc_safe(exit_start, arc) || !line_safe(arc.arc_end, leads.end) {
            return false;
        }
    } else if !line_safe(exit_start, leads.end) {
        return false;
    }
    true
}

/// Exact minimum distance of a finite XY circular arc and a line segment.
/// Candidates are endpoints, circle/line intersections, and interior extrema
/// along the segment normal. Work is constant per boundary edge; no point
/// sampling can step over a thin wall or depend on visualization tolerance.
fn arc_segment_distance(from: Point2Dto, arc: &LeadArc, a: Point2Dto, b: Point2Dto) -> f64 {
    let center = arc.center;
    let radius = distance_2d(from, center);
    let angle = |p: Point2Dto| (p.y - center.y).atan2(p.x - center.x);
    let start = angle(from);
    let signed = if arc.clockwise { -1.0 } else { 1.0 };
    let sweep = (signed * (angle(arc.arc_end) - start)).rem_euclid(std::f64::consts::TAU);
    let on_arc = |p: Point2Dto| {
        (signed * (angle(p) - start)).rem_euclid(std::f64::consts::TAU) <= sweep + 1e-10
            || distance_2d(p, from) <= 1e-9
    };
    let point_distance = |p: Point2Dto| {
        if on_arc(p) {
            (distance_2d(p, center) - radius).abs()
        } else {
            distance_2d(p, from).min(distance_2d(p, arc.arc_end))
        }
    };
    let mut best = segment_distance(from, a, b)
        .min(segment_distance(arc.arc_end, a, b))
        .min(point_distance(a))
        .min(point_distance(b));
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let length2 = dx * dx + dy * dy;
    if length2 <= EPSILON * EPSILON {
        return best;
    }
    let cx = a.x - center.x;
    let cy = a.y - center.y;
    let dot = cx * dx + cy * dy;
    let discriminant = dot * dot - length2 * (cx * cx + cy * cy - radius * radius);
    if discriminant >= 0.0 {
        for t in [
            (-dot - discriminant.sqrt()) / length2,
            (-dot + discriminant.sqrt()) / length2,
        ] {
            if (0.0..=1.0).contains(&t) && on_arc(Point2Dto::new(a.x + t * dx, a.y + t * dy)) {
                return 0.0;
            }
        }
    }
    let length = length2.sqrt();
    for sign in [-1.0, 1.0] {
        let p = Point2Dto::new(
            center.x - sign * radius * dy / length,
            center.y + sign * radius * dx / length,
        );
        if on_arc(p) {
            best = best.min(segment_distance(p, a, b));
        }
    }
    best
}

/// Convert only the active compensated lead pieces; the approach and final
/// cancellation endpoints are already physical, uncompensated coordinates.
fn physical_contour_leads(
    leads: &ContourLeads,
    path: &[Point2Dto],
    compensation: Option<(bool, f64)>,
) -> Result<ContourLeads, CamPlanError> {
    let Some((left, radius)) = compensation else {
        return Ok(leads.clone());
    };
    let normal_offset = |p: Point2Dto, t: Point2Dto| {
        let sign = if left { 1.0 } else { -1.0 };
        Point2Dto::new(p.x - sign * radius * t.y, p.y + sign * radius * t.x)
    };
    let shrink = |p: Point2Dto, arc: &LeadArc| -> Result<Point2Dto, CamPlanError> {
        let nominal = distance_2d(p, arc.center);
        let physical = nominal
            + if left != arc.clockwise {
                -radius
            } else {
                radius
            };
        if physical <= EPSILON {
            return Err(CamPlanError(
                "Compensated lead arc collapses at the selected cutter radius".into(),
            ));
        }
        Ok(Point2Dto::new(
            arc.center.x + (p.x - arc.center.x) * physical / nominal,
            arc.center.y + (p.y - arc.center.y) * physical / nominal,
        ))
    };
    let mut result = leads.clone();
    result.line_end = if let Some(arc) = &leads.start_arc {
        shrink(leads.line_end, arc)?
    } else {
        normal_offset(leads.line_end, unit_direction(path[0], path[1])?)
    };
    if let Some(arc) = &mut result.start_arc {
        arc.arc_end = shrink(arc.arc_end, arc)?;
    }
    if let Some(arc) = &mut result.end_arc {
        arc.arc_end = shrink(arc.arc_end, arc)?;
    }
    Ok(result)
}

fn open_leads_clear_profile(
    leads: &ContourLeads,
    path: &[Point2Dto],
    boundary: &[Point2Dto],
    radius: f64,
    compensation: Option<bool>,
) -> Result<bool, CamPlanError> {
    let mut end = *path.last().expect("validated open path");
    if let Some(left) = compensation {
        let t = unit_direction(path[path.len() - 2], end)?;
        let sign = if left { 1.0 } else { -1.0 };
        end = Point2Dto::new(end.x - sign * radius * t.y, end.y + sign * radius * t.x);
    }
    let line = |a, b| {
        boundary
            .windows(2)
            .all(|edge| segment_segment_distance(a, b, edge[0], edge[1]) >= radius - 1e-6)
    };
    let arc = |from, arc: &LeadArc| {
        boundary
            .windows(2)
            .all(|edge| arc_segment_distance(from, arc, edge[0], edge[1]) >= radius - 1e-6)
    };
    Ok(line(leads.start, leads.line_end)
        && leads
            .start_arc
            .as_ref()
            .is_none_or(|a| arc(leads.line_end, a))
        && if let Some(a) = &leads.end_arc {
            arc(end, a) && line(a.arc_end, leads.end)
        } else {
            line(end, leads.end)
        })
}

fn polygon_bounds(points: &[Point2Dto]) -> crate::model::Rect2Dto {
    let mut min = Point2Dto::new(f64::INFINITY, f64::INFINITY);
    let mut max = Point2Dto::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for point in points {
        min.x = min.x.min(point.x);
        min.y = min.y.min(point.y);
        max.x = max.x.max(point.x);
        max.y = max.y.max(point.y);
    }
    crate::model::Rect2Dto { min, max }
}

/// Verify that an inward miter offset is a simple loop and that every complete
/// offset edge (not only its endpoints) stays inside the cutter-center free
/// region. This rejects folded offsets and concave chords that can look valid
/// at their vertices while crossing a protected wall between them.
fn inward_offset_is_clear(offset: &[Point2Dto], boundary: &[Point2Dto], radius: f64) -> bool {
    let tolerance = radius - 1.0e-6;
    for left in 0..offset.len() {
        let left_next = (left + 1) % offset.len();
        for right in left + 1..offset.len() {
            let right_next = (right + 1) % offset.len();
            if left == right_next || left_next == right {
                continue;
            }
            if segments_intersect(
                offset[left],
                offset[left_next],
                offset[right],
                offset[right_next],
            ) {
                return false;
            }
        }
    }
    (0..offset.len()).all(|index| {
        cutter_center_segment_is_clear(
            offset[index],
            offset[(index + 1) % offset.len()],
            boundary,
            tolerance,
        )
    })
}

fn cutter_center_segment_is_clear(
    from: Point2Dto,
    to: Point2Dto,
    boundary: &[Point2Dto],
    minimum_clearance: f64,
) -> bool {
    let midpoint = Point2Dto::new((from.x + to.x) * 0.5, (from.y + to.y) * 0.5);
    point_in_polygon(from, boundary)
        && point_in_polygon(midpoint, boundary)
        && point_in_polygon(to, boundary)
        && (0..boundary.len()).all(|index| {
            segment_segment_distance(
                from,
                to,
                boundary[index],
                boundary[(index + 1) % boundary.len()],
            ) >= minimum_clearance
        })
}

fn segment_segment_distance(a: Point2Dto, b: Point2Dto, c: Point2Dto, d: Point2Dto) -> f64 {
    if segments_intersect(a, b, c, d) {
        0.0
    } else {
        segment_distance(a, c, d)
            .min(segment_distance(b, c, d))
            .min(segment_distance(c, a, b))
            .min(segment_distance(d, a, b))
    }
}

fn segments_intersect(a: Point2Dto, b: Point2Dto, c: Point2Dto, d: Point2Dto) -> bool {
    let orient = |p: Point2Dto, q: Point2Dto, r: Point2Dto| {
        (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x)
    };
    let on_segment = |p: Point2Dto, q: Point2Dto, r: Point2Dto| {
        q.x >= p.x.min(r.x) - EPSILON
            && q.x <= p.x.max(r.x) + EPSILON
            && q.y >= p.y.min(r.y) - EPSILON
            && q.y <= p.y.max(r.y) + EPSILON
    };
    let (o1, o2, o3, o4) = (
        orient(a, b, c),
        orient(a, b, d),
        orient(c, d, a),
        orient(c, d, b),
    );
    if ((o1 > EPSILON && o2 < -EPSILON) || (o1 < -EPSILON && o2 > EPSILON))
        && ((o3 > EPSILON && o4 < -EPSILON) || (o3 < -EPSILON && o4 > EPSILON))
    {
        return true;
    }
    (o1.abs() <= EPSILON && on_segment(a, c, b))
        || (o2.abs() <= EPSILON && on_segment(a, d, b))
        || (o3.abs() <= EPSILON && on_segment(c, a, d))
        || (o4.abs() <= EPSILON && on_segment(c, b, d))
}

fn segment_distance(point: Point2Dto, a: Point2Dto, b: Point2Dto) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let length_sq = dx * dx + dy * dy;
    if length_sq <= EPSILON {
        return distance_2d(point, a);
    }
    let t = (((point.x - a.x) * dx + (point.y - a.y) * dy) / length_sq).clamp(0.0, 1.0);
    distance_2d(point, Point2Dto::new(a.x + dx * t, a.y + dy * t))
}

/// Ray-cast containment for a simple closed polygon, boundary inclusive.
fn point_in_polygon(point: Point2Dto, polygon: &[Point2Dto]) -> bool {
    let mut inside = false;
    for index in 0..polygon.len() {
        let a = polygon[index];
        let b = polygon[(index + 1) % polygon.len()];
        let on_segment = (point.x - a.x) * (b.y - a.y) - (point.y - a.y) * (b.x - a.x);
        let within_x = (point.x >= a.x.min(b.x) - EPSILON) && (point.x <= a.x.max(b.x) + EPSILON);
        let within_y = (point.y >= a.y.min(b.y) - EPSILON) && (point.y <= a.y.max(b.y) + EPSILON);
        if on_segment.abs() <= EPSILON && within_x && within_y {
            return true;
        }
        if (a.y > point.y) != (b.y > point.y) {
            let crossing_x = a.x + (point.y - a.y) * (b.x - a.x) / (b.y - a.y);
            if point.x < crossing_x {
                inside = !inside;
            }
        }
    }
    inside
}

/// Intersect a horizontal scanline with a simple closed polygon and return
/// the interior X spans. Uses the half-open edge rule so rows through a
/// vertex are not double counted.
fn scanline_spans(polygon: &[Point2Dto], y: f64) -> Vec<(f64, f64)> {
    let mut crossings = Vec::new();
    for index in 0..polygon.len() {
        let a = polygon[index];
        let b = polygon[(index + 1) % polygon.len()];
        let (low, high) = if a.y <= b.y { (a, b) } else { (b, a) };
        if y < low.y || y >= high.y {
            continue;
        }
        let t = (y - low.y) / (high.y - low.y);
        crossings.push(low.x + t * (high.x - low.x));
    }
    crossings.sort_by(|left, right| left.total_cmp(right));
    let mut spans = Vec::with_capacity(crossings.len() / 2);
    for pair in crossings.as_chunks::<2>().0 {
        let (x0, x1) = (pair[0], pair[1]);
        if x1 - x0 > EPSILON {
            spans.push((x0, x1));
        }
    }
    spans
}

/// Mill an internal thread with a helical orbit: one pitch of Z travel per
/// revolution, split into semicircular arc records. Radial stock is removed
/// in orbital passes from the smallest radius out, so the finishing pass is
/// last. With a clockwise spindle, climb milling in an internal bore orbits
/// counter-clockwise (remaining wall material on the right). Thread hand then
/// fixes the Z sense: a right-hand groove advances upward with CCW angle, so
/// right-hand climb starts at the bottom and exits at the top. The spiral
/// uses the selected Z limits exactly. No implicit end overtravel is safe
/// without bore-depth evidence, especially at a blind drilled floor.
fn plan_thread(
    builder: &mut ProgramBuilder,
    operation: &CamOperationDto,
    tool: &CamToolDto,
) -> Result<(), CamPlanError> {
    let CamOperationDto::Thread {
        points,
        holes,
        top_z,
        bottom_z,
        pitch,
        major_diameter,
        minor_diameter,
        radial_passes,
        step_over,
        hand,
        direction,
        cutting,
        name,
        ..
    } = operation
    else {
        unreachable!();
    };

    let mut targets: Vec<(Point2Dto, f64, f64)> = holes
        .iter()
        .map(|hole| (hole.point, hole.top_z, hole.bottom_z))
        .collect();
    targets.extend(points.iter().map(|point| (*point, *top_z, *bottom_z)));
    let deepest_travel = targets
        .iter()
        .map(|(_, top, bottom)| top - bottom)
        .fold(0.0_f64, f64::max);
    require_flute_length(tool, deepest_travel, name)?;
    let orbit = (major_diameter - tool.diameter) * 0.5;
    if orbit <= EPSILON {
        return Err(CamPlanError(format!(
            "thread operation '{name}' has no orbit radius; the tool must be smaller than the major diameter"
        )));
    }
    let step = if *radial_passes > 1 {
        Some(step_over.ok_or_else(|| {
            CamPlanError(format!(
                "thread operation '{name}' with multiple radial passes needs a stepover"
            ))
        })?)
    } else {
        None
    };

    let radii = (0..*radial_passes)
        .map(|index| orbit - f64::from(*radial_passes - 1 - index) * step.unwrap_or(0.0))
        .collect::<Vec<_>>();
    let clockwise = !m3_closed_cut_is_ccw(*direction, false);
    let ascending = matches!(
        (direction, hand),
        (MillingDirection::Climb, ThreadHand::Right)
            | (MillingDirection::Conventional, ThreadHand::Left)
    );
    ensure_program_budget(
        builder.commands.len(),
        targets.len().saturating_mul(radii.len()).saturating_mul(4),
        name,
    )?;
    for (point, hole_top, hole_bottom) in targets {
        let (z_start, z_end) = if ascending {
            (hole_bottom, hole_top)
        } else {
            (hole_top, hole_bottom)
        };
        let revolutions = (z_end - z_start).abs() / pitch;
        let arcs_per_pass = (revolutions * 2.0).ceil() as usize + 2;
        ensure_program_budget(
            builder.commands.len(),
            radii.len().saturating_mul(arcs_per_pass.saturating_add(3)),
            name,
        )?;
        let center = Point2Dto {
            x: point.x,
            y: point.y,
        };
        builder.retract_to_clearance();
        builder.rapid(Point3Dto::new(point.x, point.y, builder.clearance_z));
        builder.rapid(Point3Dto::new(point.x, point.y, builder.retract_z));
        for radius in radii.iter().copied() {
            let entry_z = builder.feed_plane(z_start);
            builder.rapid(Point3Dto::new(point.x, point.y, entry_z));
            builder.linear(Point3Dto::new(point.x, point.y, z_start), cutting.feed_z);

            let entry_arc_center = Point2Dto::new(point.x + radius * 0.5, point.y);
            builder.circular(
                Point3Dto::new(point.x + radius, point.y, z_start),
                entry_arc_center,
                clockwise,
                cutting.feed_xy,
            );
            let total_angle = revolutions * std::f64::consts::TAU;
            let mut covered = 0.0;
            let mut angle = 0.0;
            while covered < total_angle - EPSILON {
                let step_angle = (total_angle - covered).min(std::f64::consts::PI);
                let next_angle = if clockwise {
                    angle - step_angle
                } else {
                    angle + step_angle
                };
                let next_z = z_start + (z_end - z_start) * ((covered + step_angle) / total_angle);
                builder.circular(
                    Point3Dto::new(
                        point.x + radius * next_angle.cos(),
                        point.y + radius * next_angle.sin(),
                        next_z,
                    ),
                    center,
                    clockwise,
                    cutting.feed_xy,
                );
                angle = next_angle;
                covered += step_angle;
            }

            let orbit_end = Point2Dto::new(
                point.x + radius * angle.cos(),
                point.y + radius * angle.sin(),
            );
            let exit_arc_center =
                Point2Dto::new((orbit_end.x + point.x) * 0.5, (orbit_end.y + point.y) * 0.5);
            builder.circular(
                Point3Dto::new(point.x, point.y, z_end),
                exit_arc_center,
                clockwise,
                cutting.feed_xy,
            );
            builder.rapid(Point3Dto::new(point.x, point.y, builder.retract_z));
        }
    }
    builder.warnings.push(format!(
        "Thread operation '{name}' requires an existing {:.3} mm minor bore. Z motion stays within the selected ends, with no implicit pitch overtravel. Desktop export checks that axial thread-tool entry removes no stock at the disclosed simulation resolution; cutter teeth, neck and sub-cell bore clearance are not certified.",
        minor_diameter
    ));
    builder.retract_to_clearance();
    Ok(())
}

fn require_flute_length(
    tool: &CamToolDto,
    depth: f64,
    operation: &str,
) -> Result<(), CamPlanError> {
    if depth > tool.flute_length + EPSILON {
        return Err(CamPlanError(format!(
            "operation '{operation}' cuts {:.3} mm deep, beyond tool {}'s {:.3} mm flute length",
            depth,
            tool.label(),
            tool.flute_length
        )));
    }
    Ok(())
}

fn depth_levels(top: f64, bottom: f64, step_down: f64) -> Result<Vec<f64>, CamPlanError> {
    let mut levels = Vec::new();
    let mut depth = top;
    loop {
        if levels.len() >= MAX_GENERATED_STEPS {
            return Err(CamPlanError(format!(
                "toolpath needs more than {MAX_GENERATED_STEPS} depth steps; increase the stepdown or peck depth"
            )));
        }
        let next = (depth - step_down).max(bottom);
        levels.push(next);
        if next <= bottom + EPSILON {
            break;
        }
        depth = next;
    }
    Ok(levels)
}

fn inclusive_steps(min: f64, max: f64, step: f64) -> Result<Vec<f64>, CamPlanError> {
    let mut values = vec![min];
    let mut value = min;
    while value + step < max - EPSILON {
        if values.len() >= MAX_GENERATED_STEPS {
            return Err(CamPlanError(format!(
                "toolpath needs more than {MAX_GENERATED_STEPS} stepover rows; increase the stepover"
            )));
        }
        value += step;
        values.push(value);
    }
    if max - values[values.len() - 1] > EPSILON {
        values.push(max);
    }
    Ok(values)
}

fn ensure_program_budget(
    current_commands: usize,
    estimated_commands: usize,
    operation: &str,
) -> Result<(), CamPlanError> {
    if current_commands.saturating_add(estimated_commands) > MAX_PROGRAM_COMMANDS {
        return Err(CamPlanError(format!(
            "operation '{operation}' would exceed the {MAX_PROGRAM_COMMANDS}-command planning limit; simplify the path or use larger cutting steps"
        )));
    }
    Ok(())
}

fn without_duplicate_closure(points: &[Point2Dto]) -> Vec<Point2Dto> {
    let mut result = points.to_vec();
    if result.len() > 3 && distance_2d(result[0], result[result.len() - 1]) <= EPSILON {
        result.pop();
    }
    result
}

/// Mitered polyline offset for simple closed contours. The stored polygon
/// orientation is preserved; `inside` selects the material-facing side
/// independent of clockwise/counter-clockwise point order.
fn offset_polygon(
    points: &[Point2Dto],
    radius: f64,
    inside: bool,
) -> Result<Vec<Point2Dto>, CamPlanError> {
    let area = signed_area(points);
    let left_is_inside = area > 0.0;
    let side = if inside == left_is_inside { 1.0 } else { -1.0 };
    let offset = radius * side;
    let mut result = Vec::with_capacity(points.len());
    for index in 0..points.len() {
        let previous = points[(index + points.len() - 1) % points.len()];
        let current = points[index];
        let next = points[(index + 1) % points.len()];
        let first_direction = unit_direction(previous, current)?;
        let second_direction = unit_direction(current, next)?;
        let first_normal = Point2Dto::new(-first_direction.y, first_direction.x);
        let second_normal = Point2Dto::new(-second_direction.y, second_direction.x);
        let first_line = Point2Dto::new(
            current.x + first_normal.x * offset,
            current.y + first_normal.y * offset,
        );
        let second_line = Point2Dto::new(
            current.x + second_normal.x * offset,
            current.y + second_normal.y * offset,
        );
        let denominator = cross(first_direction, second_direction);
        let candidate = if denominator.abs() <= EPSILON {
            Point2Dto::new(
                (first_line.x + second_line.x) * 0.5,
                (first_line.y + second_line.y) * 0.5,
            )
        } else {
            let between =
                Point2Dto::new(second_line.x - first_line.x, second_line.y - first_line.y);
            let t = cross(between, second_direction) / denominator;
            Point2Dto::new(
                first_line.x + first_direction.x * t,
                first_line.y + first_direction.y * t,
            )
        };
        if !candidate.is_finite() || distance_2d(candidate, current) > radius * 25.0 {
            return Err(CamPlanError(
                "contour offset produced an excessive miter; simplify the path or use an on-path contour"
                    .to_string(),
            ));
        }
        result.push(candidate);
    }
    if signed_area(&result).abs() <= EPSILON {
        return Err(CamPlanError(
            "tool is too large for the selected contour offset".to_string(),
        ));
    }
    Ok(result)
}

/// Mitered offset of an OPEN polyline to one side of its travel direction.
/// `left` picks the left-hand normal (travel-direction +90 degrees); the
/// endpoints shift along their single segment's normal, interior vertices
/// miter exactly like the closed-polygon case. Unlike `offset_polygon` there
/// is no interior to collapse, so the only failure is a degenerate miter.
/// Also used by the simulator to reproduce the machine's compensated path
/// for in-control contour sections.
pub(crate) fn offset_polyline_open(
    points: &[Point2Dto],
    radius: f64,
    left: bool,
) -> Result<Vec<Point2Dto>, CamPlanError> {
    if points.len() < 2 {
        return Err(CamPlanError(
            "open contour chains need at least two points".to_string(),
        ));
    }
    let side = if left { 1.0 } else { -1.0 };
    let offset = radius * side;
    let segment_direction = |index: usize| unit_direction(points[index], points[index + 1]);
    let shifted = |point: Point2Dto, direction: Point2Dto| {
        Point2Dto::new(
            point.x - direction.y * offset,
            point.y + direction.x * offset,
        )
    };
    let mut result = Vec::with_capacity(points.len());

    result.push(shifted(points[0], segment_direction(0)?));
    for window in points.windows(3) {
        let current = window[1];
        let first_direction = unit_direction(window[0], window[1])?;
        let second_direction = unit_direction(window[1], window[2])?;
        let first_line = shifted(current, first_direction);
        let second_line = shifted(current, second_direction);
        let denominator = cross(first_direction, second_direction);
        let candidate = if denominator.abs() <= EPSILON {
            Point2Dto::new(
                (first_line.x + second_line.x) * 0.5,
                (first_line.y + second_line.y) * 0.5,
            )
        } else {
            let between =
                Point2Dto::new(second_line.x - first_line.x, second_line.y - first_line.y);
            let t = cross(between, second_direction) / denominator;
            Point2Dto::new(
                first_line.x + first_direction.x * t,
                first_line.y + first_direction.y * t,
            )
        };
        if !candidate.is_finite() || distance_2d(candidate, current) > radius * 25.0 {
            return Err(CamPlanError(
                "contour offset produced an excessive miter; simplify the path or use an on-path contour"
                    .to_string(),
            ));
        }
        result.push(candidate);
    }

    result.push(shifted(
        points[points.len() - 1],
        segment_direction(points.len() - 2)?,
    ));
    Ok(result)
}

fn unit_direction(from: Point2Dto, to: Point2Dto) -> Result<Point2Dto, CamPlanError> {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let length = (dx * dx + dy * dy).sqrt();
    if length <= EPSILON {
        return Err(CamPlanError(
            "contour contains consecutive duplicate points".to_string(),
        ));
    }
    Ok(Point2Dto::new(dx / length, dy / length))
}

fn cross(a: Point2Dto, b: Point2Dto) -> f64 {
    a.x * b.y - a.y * b.x
}

fn distance_2d(a: Point2Dto, b: Point2Dto) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

fn distance(a: Point3Dto, b: Point3Dto) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

#[cfg(test)]
#[path = "lead_geometry_tests.rs"]
mod lead_geometry_tests;

#[cfg(test)]
#[path = "flat_tests.rs"]
mod flat_tests;

#[cfg(test)]
mod tests {
    #[test]
    fn low_rapid_approach_is_proved_clear_by_remaining_stock() {
        let mut builder = ProgramBuilder::new();
        builder.feed_height_z = 20.0;
        builder.incoming_top = 22.0;
        builder.incoming_bounds = Some(crate::model::StockBoxDto {
            min: Point3Dto::new(-25.0, -25.0, 0.0),
            max: Point3Dto::new(25.0, 25.0, 22.0),
        });
        let point = Point2Dto::new(20.0, 0.0);
        assert!(builder
            .require_clear_approach(point, 3.0, "chamfer")
            .is_err());
        let heights = (0..50 * 50)
            .map(|i| {
                let (x, y) = ((i % 50) as f64 - 24.5, (i / 50) as f64 - 24.5);
                if x.hypot(y) < 10.0 {
                    22.0
                } else {
                    15.0
                }
            })
            .collect();
        builder.remaining_stock = Some(crate::simulation::RestHeightMap::from_heights(
            [-25.0, -25.0],
            [1.0, 1.0],
            [50, 50],
            heights,
        ));
        builder
            .require_clear_approach(point, 3.0, "chamfer")
            .unwrap();
        assert!(builder
            .require_clear_approach(Point2Dto::new(11.0, 0.0), 3.0, "chamfer")
            .is_err());
    }

    use super::*;
    use crate::model::{
        CamHoleDto, CamResolvedStockDto, CamSetupDto, CamStockSpecDto, CamToolKind,
        CuttingParametersDto, Rect2Dto, StockBoxDto, WcsOriginSpecDto, WorkCoordinateSystemDto,
    };
    include!("linking_controls_tests.rs");
    include!("chamfer_linking_tests.rs");
    include!("preview_regression_tests.rs");
    include!("milling_corner_tests.rs");

    pub(super) fn cutting() -> CuttingParametersDto {
        CuttingParametersDto {
            spindle_rpm: 12_000,
            feed_xy: 800.0,
            feed_z: 200.0,
            coolant: CoolantMode::Flood,
        }
    }

    pub(super) fn tool(id: u64, kind: CamToolKind, diameter: f64) -> CamToolDto {
        CamToolDto {
            id,
            number: Some(id as u32),
            name: format!("Tool {id}"),
            kind,
            diameter,
            flute_length: 20.0,
            overall_length: 50.0,
            center_cutting: true,
            flute_count: 4,
            point_angle_degrees: (kind == CamToolKind::ChamferMill).then_some(90.0),
            corner_radius: None,
            corner_chamfer: None,
            cutting: CuttingParametersDto::default(),
            cutting_presets: vec![],
            maximum_axial_depth: None,
            default_step_down: None,
            default_step_over: None,
        }
    }

    pub(super) fn document(
        operations: Vec<CamOperationDto>,
        tools: Vec<CamToolDto>,
    ) -> CamDocumentDto {
        CamDocumentDto {
            load_warnings: Vec::new(),
            toolpath_generations: Vec::new(),
            height_expressions: Vec::new(),
            linking: Vec::new(),
            setups: vec![CamSetupDto {
                id: 1,
                name: "Setup 1".into(),
                wcs: WorkCoordinateSystemDto::default(),
                wcs_origin: WcsOriginSpecDto::Explicit,
                work_offset: WorkOffset::G54,
                work_offset_count: 1,
                stock_spec: CamStockSpecDto::LegacyBox,
                resolved_stock: CamResolvedStockDto::Box,
                stock: StockBoxDto {
                    min: Point3Dto::new(0.0, 0.0, -20.0),
                    max: Point3Dto::new(40.0, 30.0, 0.0),
                },
                stock_model_box: None,
                body_ids: vec![],
                machine: None,
                legacy_clearance_z: None,
                legacy_retract_z: None,
                operations,
            }],
            active_setup_id: Some(1),
            next_setup_id: 2,
            next_operation_id: 10,
            next_tool_id: tools.iter().map(|tool| tool.id).max().unwrap_or(0) + 1,
            tools,
            units: crate::model::CamUnits::Millimeters,
            post_defaults: crate::model::CamPostConfigDto::default(),
        }
    }

    #[test]
    fn m3_direction_contract_keeps_climb_material_on_the_right() {
        let tangent_at_right = |ccw: bool| {
            if ccw {
                Point2Dto::new(0.0, 1.0)
            } else {
                Point2Dto::new(0.0, -1.0)
            }
        };
        let cross_to_material = |tangent: Point2Dto, material: Point2Dto| {
            tangent.x * material.y - tangent.y * material.x
        };
        for (material_inside, material_vector) in [
            (true, Point2Dto::new(-1.0, 0.0)),
            (false, Point2Dto::new(1.0, 0.0)),
        ] {
            let climb = tangent_at_right(m3_closed_cut_is_ccw(
                MillingDirection::Climb,
                material_inside,
            ));
            let conventional = tangent_at_right(m3_closed_cut_is_ccw(
                MillingDirection::Conventional,
                material_inside,
            ));
            assert!(cross_to_material(climb, material_vector) < 0.0);
            assert!(cross_to_material(conventional, material_vector) > 0.0);
        }
    }

    #[test]
    fn facing_is_zigzagged_at_each_depth_and_never_rapids_below_the_feed_plane() {
        let operation = CamOperationDto::Face {
            id: 1,
            name: "Face".into(),
            enabled: true,
            tool_id: 1,
            bounds: Rect2Dto {
                min: Point2Dto::new(0.0, 0.0),
                max: Point2Dto::new(40.0, 30.0),
            },
            top_z: 0.0,
            target_z: -2.0,
            step_over: 3.0,
            step_down: 1.0,
            safe_distance: 5.0,
            direction: FaceDirection::BothWays,
            clearance_z: 10.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting: cutting(),
        };
        let program = plan_setup(
            &document(
                vec![operation],
                vec![tool(1, CamToolKind::FlatEndMill, 6.0)],
            ),
            1,
        )
        .unwrap();
        let cut_depths = program
            .commands
            .iter()
            .filter_map(|command| match command {
                CamCommandDto::Linear { to, .. } if to.z < 0.0 => Some(to.z),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(cut_depths.contains(&-1.0));
        assert!(cut_depths.contains(&-2.0));

        assert!(program.commands.iter().all(|command| match command {
            CamCommandDto::Rapid { to } => to.z >= 1.0,
            _ => true,
        }));
        assert_eq!(program.stats.operation_count, 1);
        assert!(program.stats.cutting_distance > 0.0);
    }

    #[test]
    fn face_makes_a_single_pass_when_one_band_spans_the_face() {
        let operation = CamOperationDto::Face {
            id: 1,
            name: "Face".into(),
            enabled: true,
            tool_id: 1,
            bounds: Rect2Dto {
                min: Point2Dto::new(0.0, 0.0),
                max: Point2Dto::new(40.0, 19.0),
            },
            top_z: 0.0,
            target_z: -1.0,
            step_over: 31.0,
            step_down: 1.0,
            safe_distance: 5.0,
            direction: FaceDirection::BothWays,
            clearance_z: 10.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting: cutting(),
        };
        let program = plan_setup(
            &document(vec![operation], vec![tool(1, CamToolKind::FaceMill, 63.0)]),
            1,
        )
        .unwrap();
        let cuts: Vec<Point3Dto> = program
            .commands
            .iter()
            .filter_map(|command| match command {
                CamCommandDto::Linear { to, .. } if to.z < 0.0 => Some(*to),
                _ => None,
            })
            .collect();

        assert_eq!(cuts.len(), 2);
        assert!(cuts.iter().all(|point| (point.y - 9.5).abs() < 1.0e-9));
    }

    #[test]
    fn face_entry_moves_outward_with_safe_distance() {
        let plan_with = |safe_distance: f64| {
            let operation = CamOperationDto::Face {
                id: 1,
                name: "Face".into(),
                enabled: true,
                tool_id: 1,
                bounds: Rect2Dto {
                    min: Point2Dto::new(0.0, 0.0),
                    max: Point2Dto::new(40.0, 19.0),
                },
                top_z: 0.0,
                target_z: -1.0,
                step_over: 31.0,
                step_down: 1.0,
                safe_distance,
                direction: FaceDirection::BothWays,
                clearance_z: 10.0,
                retract_z: 3.0,
                feed_height_z: 1.0,
                cutting: cutting(),
            };
            plan_setup(
                &document(vec![operation], vec![tool(1, CamToolKind::FaceMill, 63.0)]),
                1,
            )
            .unwrap()
        };
        let first_cut_x = |program: &CamProgramDto| {
            program
                .commands
                .iter()
                .find_map(|command| match command {
                    CamCommandDto::Linear { to, .. } if to.z < 0.0 => Some(to.x),
                    _ => None,
                })
                .unwrap()
        };
        let radius = 63.0 * 0.5;
        assert!((first_cut_x(&plan_with(5.0)) - (-radius - 5.0)).abs() < 1.0e-9);
        assert!((first_cut_x(&plan_with(20.0)) - (-radius - 20.0)).abs() < 1.0e-9);
    }

    #[test]
    fn face_centers_a_single_row_when_one_band_spans_the_strip() {
        let operation = CamOperationDto::Face {
            id: 1,
            name: "Face".into(),
            enabled: true,
            tool_id: 1,
            bounds: Rect2Dto {
                min: Point2Dto::new(0.0, 0.0),
                max: Point2Dto::new(40.0, 30.0),
            },
            top_z: 0.0,
            target_z: -1.0,
            step_over: 28.0,
            step_down: 1.0,
            safe_distance: 5.0,
            direction: FaceDirection::BothWays,
            clearance_z: 10.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting: cutting(),
        };
        let program = plan_setup(
            &document(
                vec![operation],
                vec![tool(1, CamToolKind::FlatEndMill, 32.0)],
            ),
            1,
        )
        .unwrap();
        let row_ys: Vec<f64> = program
            .commands
            .iter()
            .filter_map(|command| match command {
                CamCommandDto::Linear { to, .. } if to.z < 0.0 => Some(to.y),
                _ => None,
            })
            .collect();
        assert_eq!(row_ys, vec![15.0, 15.0]);
    }

    #[test]
    fn face_centers_multi_row_layouts_on_the_face() {
        let operation = CamOperationDto::Face {
            id: 1,
            name: "Face".into(),
            enabled: true,
            tool_id: 1,
            bounds: Rect2Dto {
                min: Point2Dto::new(0.0, 0.0),
                max: Point2Dto::new(40.0, 30.0),
            },
            top_z: 0.0,
            target_z: -1.0,
            step_over: 8.0,
            step_down: 1.0,
            safe_distance: 5.0,
            direction: FaceDirection::BothWays,
            clearance_z: 10.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting: cutting(),
        };
        let program = plan_setup(
            &document(
                vec![operation],
                vec![tool(1, CamToolKind::FlatEndMill, 10.0)],
            ),
            1,
        )
        .unwrap();
        let row_ys: Vec<f64> = program
            .commands
            .iter()
            .filter_map(|command| match command {
                CamCommandDto::Linear { to, .. } if to.z < 0.0 => Some(to.y),
                _ => None,
            })
            .collect();
        assert_eq!(row_ys, vec![3.0, 3.0, 11.0, 11.0, 19.0, 19.0, 27.0, 27.0]);
    }

    #[test]
    fn face_accepts_a_non_center_cutting_face_mill() {
        let operation = CamOperationDto::Face {
            id: 1,
            name: "Face".into(),
            enabled: true,
            tool_id: 1,
            bounds: Rect2Dto {
                min: Point2Dto::new(0.0, 0.0),
                max: Point2Dto::new(40.0, 19.0),
            },
            top_z: 0.0,
            target_z: -1.0,
            step_over: 31.0,
            step_down: 1.0,
            safe_distance: 5.0,
            direction: FaceDirection::BothWays,
            clearance_z: 10.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting: cutting(),
        };
        let mut shell_mill = tool(1, CamToolKind::FaceMill, 63.0);
        shell_mill.center_cutting = false;
        let program = plan_setup(&document(vec![operation], vec![shell_mill]), 1).unwrap();

        let plunge = program
            .commands
            .iter()
            .filter_map(|command| match command {
                CamCommandDto::Linear { to, .. } if to.z < 0.0 => Some(*to),
                _ => None,
            })
            .next()
            .expect("a facing plunge");
        let radius = 63.0 * 0.5;
        assert!(
            plunge.x + radius < 0.0,
            "plunge at x={} must keep the cutter clear of the stock edge",
            plunge.x
        );
    }

    #[test]
    fn face_kind_gate_admits_flat_bottom_mills_only() {
        let make_op = || CamOperationDto::Face {
            id: 1,
            name: "Face".into(),
            enabled: true,
            tool_id: 1,
            bounds: Rect2Dto {
                min: Point2Dto::new(0.0, 0.0),
                max: Point2Dto::new(40.0, 19.0),
            },
            top_z: 0.0,
            target_z: -1.0,
            step_over: 12.0,
            step_down: 1.0,
            safe_distance: 5.0,
            direction: FaceDirection::BothWays,
            clearance_z: 10.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting: cutting(),
        };

        for kind in [
            CamToolKind::FlatEndMill,
            CamToolKind::BullNoseEndMill,
            CamToolKind::FaceMill,
        ] {
            let mut milling = tool(1, kind, 16.0);
            milling.center_cutting = false;
            if kind == CamToolKind::BullNoseEndMill {
                milling.corner_radius = Some(3.0);
            }
            plan_setup(&document(vec![make_op()], vec![milling]), 1)
                .unwrap_or_else(|err| panic!("{kind:?} should face: {err}"));
        }

        for kind in [
            CamToolKind::BallEndMill,
            CamToolKind::ChamferMill,
            CamToolKind::ThreadMill,
            CamToolKind::Drill,
            CamToolKind::Tap,
            CamToolKind::Reamer,
            CamToolKind::BoringBar,
            CamToolKind::TurningGeneral,
        ] {
            let milling = tool(1, kind, 16.0);
            let err = plan_setup(&document(vec![make_op()], vec![milling]), 1)
                .expect_err(&format!("{kind:?} must not face"));
            assert!(
                err.to_string().contains("flat, bull-nose, or face mill"),
                "{kind:?} should hit the kind gate: {err}"
            );
        }
    }

    #[test]
    fn outside_contour_offsets_a_ccw_rectangle_by_tool_radius() {
        let points = vec![
            Point2Dto::new(10.0, 10.0),
            Point2Dto::new(30.0, 10.0),
            Point2Dto::new(30.0, 20.0),
            Point2Dto::new(10.0, 20.0),
        ];
        let offset = offset_polygon(&points, 2.0, false).unwrap();
        assert_eq!(offset[0], Point2Dto::new(8.0, 8.0));
        assert_eq!(offset[2], Point2Dto::new(32.0, 22.0));
    }

    #[test]
    fn peck_drill_fully_retracts_between_pecks() {
        let operation = CamOperationDto::Drill {
            id: 1,
            name: "Drill".into(),
            enabled: true,
            tool_id: 2,
            points: vec![Point2Dto::new(20.0, 15.0)],
            holes: Vec::new(),
            top_z: 0.0,
            bottom_z: -7.0,
            retract_z: 3.0,
            drill_tip_through: false,
            breakthrough_depth: 0.0,
            peck_depth: Some(3.0),
            dwell_seconds: 0.1,
            clearance_z: 10.0,
            feed_height_z: 1.0,
            cycle: DrillCycle::DeepHole,
            peck_retract: None,
            thread_pitch: None,
            floating_tap_holder: false,
            feed_out: None,
            cutting: cutting(),
        };
        let program = plan_setup(
            &document(vec![operation], vec![tool(2, CamToolKind::Drill, 5.0)]),
            1,
        )
        .unwrap();
        let plunge_depths = program
            .commands
            .iter()
            .filter_map(|command| match command {
                CamCommandDto::Linear { to, .. } => Some(to.z),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(plunge_depths, vec![-3.0, -6.0, -7.0]);
        assert_eq!(
            program
                .commands
                .iter()
                .filter(|command| matches!(command, CamCommandDto::Dwell { .. }))
                .count(),
            3
        );
    }

    #[test]
    fn flute_length_is_a_hard_planning_limit() {
        let operation = CamOperationDto::Contour2d {
            id: 1,
            name: "Deep contour".into(),
            enabled: true,
            tool_id: 1,
            path: vec![
                Point2Dto::new(5.0, 5.0),
                Point2Dto::new(35.0, 5.0),
                Point2Dto::new(35.0, 25.0),
                Point2Dto::new(5.0, 25.0),
            ],
            closed: true,
            top_z: 0.0,
            bottom_z: -15.0,
            step_down: 2.0,
            compensation: ContourCompensation::Outside,
            compensation_mode: CompensationMode::InSoftware,
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
            clearance_z: 10.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting: cutting(),
        };
        let mut short_tool = tool(1, CamToolKind::FlatEndMill, 6.0);
        short_tool.flute_length = 10.0;
        let error = plan_setup(&document(vec![operation], vec![short_tool]), 1).unwrap_err();
        assert!(error.0.contains("flute length"));
    }

    fn open_chain_operation(compensation: ContourCompensation) -> CamOperationDto {
        CamOperationDto::Contour2d {
            id: 1,
            name: "Open wall".into(),
            enabled: true,
            tool_id: 1,

            path: vec![
                Point2Dto::new(5.0, 5.0),
                Point2Dto::new(30.0, 5.0),
                Point2Dto::new(30.0, 20.0),
            ],
            closed: false,
            top_z: 0.0,
            bottom_z: -2.0,
            step_down: 2.0,
            compensation,
            compensation_mode: CompensationMode::InSoftware,
            lead_in: 5.0,
            lead_out: 5.0,
            lead_arc_radius: None,

            direction: if matches!(compensation, ContourCompensation::Left) {
                MillingDirection::Climb
            } else {
                MillingDirection::Conventional
            },
            roughing_passes: 1,
            roughing_step_over: None,
            finishing_pass: false,
            finish_allowance: 0.0,
            finish_feed: None,
            spring_pass: false,
            chain_ref: None,
            clearance_z: 10.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting: cutting(),
        }
    }

    fn cutting_targets(program: &CamProgramDto) -> Vec<Point3Dto> {
        program
            .commands
            .iter()
            .filter_map(|command| match command {
                CamCommandDto::Linear { to, .. } => Some(*to),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn open_contour_chain_never_closes() {
        let program = plan_setup(
            &document(
                vec![open_chain_operation(ContourCompensation::On)],
                vec![tool(1, CamToolKind::FlatEndMill, 6.0)],
            ),
            1,
        )
        .expect("plan");
        let targets = cutting_targets(&program);
        let near = |point: Point3Dto, x: f64, y: f64| {
            (point.x - x).abs() < 1.0e-9 && (point.y - y).abs() < 1.0e-9
        };

        assert_eq!(
            targets
                .iter()
                .filter(|point| near(**point, 5.0, 5.0))
                .count(),
            1
        );

        let last = targets.last().expect("cutting moves");
        assert!(near(*last, 30.0, 25.0));
        assert!(near(targets[targets.len() - 2], 30.0, 20.0));
    }

    #[test]
    fn open_contour_chain_offsets_left_and_right_of_travel() {
        let left = plan_setup(
            &document(
                vec![open_chain_operation(ContourCompensation::Left)],
                vec![tool(1, CamToolKind::FlatEndMill, 6.0)],
            ),
            1,
        )
        .expect("left plan");
        let targets = cutting_targets(&left);
        let bottom = targets
            .iter()
            .filter(|point| (point.z + 2.0).abs() < 1.0e-9)
            .copied()
            .collect::<Vec<_>>();

        assert!((bottom[0].x - 0.0).abs() < 1.0e-9 && (bottom[0].y - 8.0).abs() < 1.0e-9);
        assert!((bottom[1].x - 5.0).abs() < 1.0e-9 && (bottom[1].y - 8.0).abs() < 1.0e-9);
        let last = bottom.last().expect("offset moves");
        assert!((last.x - 27.0).abs() < 1.0e-9 && (last.y - 25.0).abs() < 1.0e-9);
        let chain_end = bottom[bottom.len() - 2];
        assert!((chain_end.x - 27.0).abs() < 1.0e-9 && (chain_end.y - 20.0).abs() < 1.0e-9);

        let right = plan_setup(
            &document(
                vec![open_chain_operation(ContourCompensation::Right)],
                vec![tool(1, CamToolKind::FlatEndMill, 6.0)],
            ),
            1,
        )
        .expect("right plan");
        let bottom_right = cutting_targets(&right)
            .into_iter()
            .filter(|point| (point.z + 2.0).abs() < 1.0e-9)
            .collect::<Vec<_>>();
        assert!(
            (bottom_right[1].x - 5.0).abs() < 1.0e-9 && (bottom_right[1].y - 2.0).abs() < 1.0e-9
        );
        let chain_end_right = bottom_right[bottom_right.len() - 2];
        assert!(
            (chain_end_right.x - 33.0).abs() < 1.0e-9 && (chain_end_right.y - 20.0).abs() < 1.0e-9
        );
    }

    #[test]
    fn open_chain_with_inside_compensation_fails_closed() {
        let error = plan_setup(
            &document(
                vec![open_chain_operation(ContourCompensation::Inside)],
                vec![tool(1, CamToolKind::FlatEndMill, 6.0)],
            ),
            1,
        )
        .unwrap_err();
        assert!(error.0.contains("no interior"));
    }

    fn closed_boss_operation(
        mode: CompensationMode,
        compensation: ContourCompensation,
    ) -> CamOperationDto {
        CamOperationDto::Contour2d {
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
            compensation,
            compensation_mode: mode,
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
            clearance_z: 10.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting: cutting(),
        }
    }

    #[test]
    fn in_control_compensation_keeps_the_part_contour_in_the_program() {
        let program = plan_setup(
            &document(
                vec![closed_boss_operation(
                    CompensationMode::InControl,
                    ContourCompensation::Outside,
                )],
                vec![tool(1, CamToolKind::FlatEndMill, 6.0)],
            ),
            1,
        )
        .expect("plan");

        let on_index = program
            .commands
            .iter()
            .position(|command| matches!(command, CamCommandDto::CutterCompensationOn { .. }))
            .expect("activation");
        let off_index = program
            .commands
            .iter()
            .position(|command| matches!(command, CamCommandDto::CutterCompensationOff))
            .expect("cancellation");
        assert!(matches!(
            program.commands[on_index],
            CamCommandDto::CutterCompensationOn { left: true }
        ));
        assert!(matches!(
            program.commands[on_index + 1],
            CamCommandDto::Linear { .. }
        ));
        assert!(matches!(
            program.commands[off_index + 1],
            CamCommandDto::Linear { .. }
        ));

        let targets = cutting_targets(&program);
        let near = |point: Point3Dto, x: f64, y: f64| {
            (point.x - x).abs() < 1.0e-9 && (point.y - y).abs() < 1.0e-9
        };
        assert!(near(targets[0], 2.0, 0.0));
        assert!(targets.iter().any(|point| near(*point, 15.0, 5.0)));
        assert!(targets.iter().any(|point| near(*point, 15.0, 15.0)));
        assert!(!targets.iter().any(|point| near(*point, 18.0, 2.0)));
        assert!(near(*targets.last().expect("cutting moves"), 0.0, 2.0));
    }

    #[test]
    fn in_software_compensation_offsets_the_path_and_emits_no_compensation_words() {
        let program = plan_setup(
            &document(
                vec![closed_boss_operation(
                    CompensationMode::InSoftware,
                    ContourCompensation::Outside,
                )],
                vec![tool(1, CamToolKind::FlatEndMill, 6.0)],
            ),
            1,
        )
        .expect("plan");
        assert!(program.commands.iter().all(|command| {
            !matches!(
                command,
                CamCommandDto::CutterCompensationOn { .. } | CamCommandDto::CutterCompensationOff
            )
        }));

        let targets = cutting_targets(&program);
        let near = |point: Point3Dto, x: f64, y: f64| {
            (point.x - x).abs() < 1.0e-9 && (point.y - y).abs() < 1.0e-9
        };
        assert!(targets.iter().any(|point| near(*point, 18.0, 2.0)));
        assert!(targets.iter().any(|point| near(*point, 18.0, 18.0)));
    }

    #[test]
    fn in_control_compensation_allows_leads_shorter_than_the_tool_radius() {
        let mut operation =
            closed_boss_operation(CompensationMode::InControl, ContourCompensation::Outside);
        if let CamOperationDto::Contour2d { lead_in, .. } = &mut operation {
            *lead_in = 2.0;
        }
        let program = plan_setup(
            &document(
                vec![operation],
                vec![tool(1, CamToolKind::FlatEndMill, 6.0)],
            ),
            1,
        )
        .unwrap();
        assert!(program
            .commands
            .iter()
            .any(|command| matches!(command, CamCommandDto::CutterCompensationOn { .. })));
    }

    #[test]
    fn inside_control_compensation_requires_room_for_both_leads_and_the_cutter() {
        let error = plan_setup(
            &document(
                vec![closed_boss_operation(
                    CompensationMode::InControl,
                    ContourCompensation::Inside,
                )],
                vec![tool(1, CamToolKind::FlatEndMill, 6.0)],
            ),
            1,
        )
        .unwrap_err();
        assert!(error.0.contains("at least 16.000 mm"));
        assert!(error.0.contains("shorten the leads"));
    }

    #[test]
    fn contour_requires_positive_leads() {
        let mut operation =
            closed_boss_operation(CompensationMode::InSoftware, ContourCompensation::On);
        if let CamOperationDto::Contour2d { lead_out, .. } = &mut operation {
            *lead_out = 0.0;
        }
        let error = plan_setup(
            &document(
                vec![operation],
                vec![tool(1, CamToolKind::FlatEndMill, 6.0)],
            ),
            1,
        )
        .unwrap_err();
        assert!(error.0.contains("positive lead-in and lead-out"));
    }

    #[test]
    fn drill_retract_must_stay_between_cut_top_and_clearance() {
        let operation = CamOperationDto::Drill {
            id: 1,
            name: "Unsafe drill".into(),
            enabled: true,
            tool_id: 2,
            points: vec![Point2Dto::new(20.0, 15.0)],
            holes: Vec::new(),
            top_z: -5.0,
            bottom_z: -10.0,
            retract_z: -6.0,
            drill_tip_through: false,
            breakthrough_depth: 0.0,
            peck_depth: Some(2.0),
            dwell_seconds: 0.0,
            clearance_z: 10.0,
            feed_height_z: -5.0,
            cycle: DrillCycle::DeepHole,
            peck_retract: None,
            thread_pitch: None,
            floating_tap_holder: false,
            feed_out: None,
            cutting: cutting(),
        };
        let error = plan_setup(
            &document(vec![operation], vec![tool(2, CamToolKind::Drill, 5.0)]),
            1,
        )
        .unwrap_err();
        assert!(error
            .0
            .contains("retract Z must be above every effective cut/hole top"));
    }

    #[test]
    fn drill_top_may_start_above_the_stock() {
        let mut operation = drill_operation(DrillCycle::Drill);
        if let CamOperationDto::Drill {
            top_z,
            feed_height_z,
            retract_z,
            ..
        } = &mut operation
        {
            *top_z = 3.0;
            *feed_height_z = 3.0;
            *retract_z = 5.0;
        }
        let program = plan_setup(
            &document(vec![operation], vec![tool(2, CamToolKind::Drill, 5.0)]),
            1,
        )
        .unwrap();
        assert_eq!(drill_cut_depths(&program, 120.0), vec![-7.0]);

        let mut operation = drill_operation(DrillCycle::Drill);
        if let CamOperationDto::Drill { bottom_z, .. } = &mut operation {
            *bottom_z = -25.0;
        }
        let error = plan_setup(
            &document(vec![operation], vec![tool(2, CamToolKind::Drill, 5.0)]),
            1,
        )
        .unwrap_err();
        assert!(
            error
                .0
                .contains("bottom height Z-25.000 is outside the stock"),
            "{}",
            error.0
        );
    }

    fn drill_operation(cycle: DrillCycle) -> CamOperationDto {
        CamOperationDto::Drill {
            id: 1,
            name: "Hole cycle".into(),
            enabled: true,
            tool_id: 2,
            drill_tip_through: false,
            breakthrough_depth: 0.0,
            points: vec![Point2Dto::new(20.0, 15.0)],
            holes: Vec::new(),
            top_z: 0.0,
            bottom_z: -7.0,
            retract_z: 3.0,
            clearance_z: 10.0,
            feed_height_z: 1.0,
            cycle,
            peck_depth: None,
            peck_retract: None,
            thread_pitch: None,
            floating_tap_holder: matches!(
                cycle,
                DrillCycle::TappingRight | DrillCycle::TappingLeft
            ),
            feed_out: None,
            dwell_seconds: 0.0,
            cutting: CuttingParametersDto {
                spindle_rpm: 400,
                feed_xy: 300.0,
                feed_z: 120.0,
                coolant: CoolantMode::Off,
            },
        }
    }

    fn picked_hole(x: f64, y: f64, top: f64, bottom: f64) -> CamHoleDto {
        CamHoleDto {
            point: Point2Dto::new(x, y),
            top_z: top,
            bottom_z: bottom,
            axis: [0.0, 0.0, -1.0],
            face_key: Some("hole-face".into()),
        }
    }

    /// Z targets of every feed-rate descent, in program order.
    fn drill_cut_depths(program: &CamProgramDto, feed_z: f64) -> Vec<f64> {
        program
            .commands
            .iter()
            .filter_map(|command| match command {
                CamCommandDto::Linear { to, feed } if (*feed - feed_z).abs() < 1.0e-9 => Some(to.z),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn picked_holes_keep_their_bottoms_without_assuming_material_above_them_is_removed() {
        let mut operation = drill_operation(DrillCycle::ChipBreaking);
        let CamOperationDto::Drill {
            points,
            holes,
            peck_depth,
            ..
        } = &mut operation
        else {
            unreachable!();
        };
        points.clear();
        holes.push(picked_hole(10.0, 10.0, 0.0, -5.0));
        holes.push(picked_hole(30.0, 20.0, -2.0, -9.0));
        *peck_depth = Some(4.0);
        let program = plan_setup(
            &document(vec![operation], vec![tool(2, CamToolKind::Drill, 5.0)]),
            1,
        )
        .unwrap();

        assert_eq!(
            drill_cut_depths(&program, 120.0),
            [-4.0, -5.0, -4.0, -8.0, -9.0]
        );
    }

    #[test]
    fn tip_through_drives_the_point_below_the_hole_bottom() {
        let mut operation = drill_operation(DrillCycle::Drill);
        let CamOperationDto::Drill {
            points,
            holes,
            drill_tip_through,
            breakthrough_depth,
            ..
        } = &mut operation
        else {
            unreachable!();
        };
        points.clear();
        holes.push(picked_hole(20.0, 15.0, 0.0, -7.0));
        *drill_tip_through = true;
        *breakthrough_depth = 1.0;

        let program = plan_setup(
            &document(
                vec![operation.clone()],
                vec![tool(2, CamToolKind::Drill, 10.0)],
            ),
            1,
        )
        .unwrap();
        let expected = -7.0 - 5.0 / 59.0_f64.to_radians().tan() - 1.0;
        let depths = drill_cut_depths(&program, 120.0);
        assert_eq!(depths.len(), 1);
        assert!(
            (depths[0] - expected).abs() < 1.0e-9,
            "cut bottom {} should be {expected}",
            depths[0]
        );

        let mut flat = tool(2, CamToolKind::Drill, 10.0);
        flat.point_angle_degrees = Some(90.0);
        let program = plan_setup(&document(vec![operation], vec![flat]), 1).unwrap();
        let depths = drill_cut_depths(&program, 120.0);
        assert_eq!(depths.len(), 1);
        assert!(
            (depths[0] - -13.0).abs() < 1.0e-9,
            "cut bottom {} should be -13",
            depths[0]
        );
    }

    #[test]
    fn drill_without_any_target_is_rejected() {
        let mut operation = drill_operation(DrillCycle::Drill);
        let CamOperationDto::Drill { points, holes, .. } = &mut operation else {
            unreachable!();
        };
        points.clear();
        holes.clear();
        let error = plan_setup(
            &document(vec![operation], vec![tool(2, CamToolKind::Drill, 5.0)]),
            1,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("needs 1..="),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn tip_through_is_rejected_outside_the_drilling_cycle_family() {
        let mut operation = drill_operation(DrillCycle::TappingRight);
        let CamOperationDto::Drill {
            drill_tip_through,
            thread_pitch,
            ..
        } = &mut operation
        else {
            unreachable!();
        };
        *drill_tip_through = true;
        *thread_pitch = Some(1.0);
        let error = plan_setup(
            &document(vec![operation], vec![tool(2, CamToolKind::Tap, 5.0)]),
            1,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("tip-through applies to the drilling cycle family"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn negative_breakthrough_depth_is_rejected() {
        let mut operation = drill_operation(DrillCycle::Drill);
        let CamOperationDto::Drill {
            breakthrough_depth, ..
        } = &mut operation
        else {
            unreachable!();
        };
        *breakthrough_depth = -0.5;
        let error = plan_setup(
            &document(vec![operation], vec![tool(2, CamToolKind::Drill, 5.0)]),
            1,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("break-through depth must be zero or positive"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn picked_hole_with_tilted_axis_is_rejected() {
        let mut operation = drill_operation(DrillCycle::Drill);
        let CamOperationDto::Drill { points, holes, .. } = &mut operation else {
            unreachable!();
        };
        points.clear();
        let mut hole = picked_hole(20.0, 15.0, 0.0, -7.0);
        hole.axis = [0.0, 0.5, -0.866];
        holes.push(hole);
        let error = plan_setup(
            &document(vec![operation], vec![tool(2, CamToolKind::Drill, 5.0)]),
            1,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("parallel to setup Z"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn chip_breaking_partially_retracts_inside_the_hole() {
        let mut operation = drill_operation(DrillCycle::ChipBreaking);
        let CamOperationDto::Drill {
            peck_depth,
            peck_retract,
            ..
        } = &mut operation
        else {
            unreachable!();
        };
        *peck_depth = Some(3.0);
        *peck_retract = Some(0.8);
        let program = plan_setup(
            &document(vec![operation], vec![tool(2, CamToolKind::Drill, 5.0)]),
            1,
        )
        .unwrap();
        let rapid_zs: Vec<f64> = program
            .commands
            .iter()
            .filter_map(|command| match command {
                CamCommandDto::Rapid { to } => Some(to.z),
                _ => None,
            })
            .collect();

        let expected = [10.0, 3.0, 1.0, -2.2, -5.2, 3.0, 10.0];
        assert_eq!(rapid_zs.len(), expected.len());
        for (actual, expected) in rapid_zs.iter().zip(expected.iter()) {
            assert!(
                (actual - expected).abs() < 1.0e-9,
                "rapid at {actual} should be {expected}"
            );
        }
    }

    #[test]
    fn face_contour_and_drill_sections_leave_by_way_of_the_retract_plane() {
        let face = CamOperationDto::Face {
            id: 1,
            name: "Face".into(),
            enabled: true,
            tool_id: 1,
            bounds: Rect2Dto {
                min: Point2Dto::new(0.0, 0.0),
                max: Point2Dto::new(40.0, 30.0),
            },
            top_z: 0.0,
            target_z: -1.0,
            step_over: 3.0,
            step_down: 1.0,
            safe_distance: 5.0,
            direction: FaceDirection::BothWays,
            clearance_z: 10.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting: cutting(),
        };
        let contour = CamOperationDto::Contour2d {
            id: 2,
            name: "Contour".into(),
            enabled: true,
            tool_id: 1,
            path: vec![
                Point2Dto::new(5.0, 5.0),
                Point2Dto::new(35.0, 5.0),
                Point2Dto::new(35.0, 25.0),
                Point2Dto::new(5.0, 25.0),
            ],
            closed: true,
            top_z: 0.0,
            bottom_z: -2.0,
            step_down: 2.0,
            compensation: ContourCompensation::Outside,
            compensation_mode: CompensationMode::InSoftware,
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
            clearance_z: 10.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting: cutting(),
        };
        let mut drill = drill_operation(DrillCycle::Drill);
        let CamOperationDto::Drill { id, .. } = &mut drill else {
            unreachable!();
        };
        *id = 3;
        let program = plan_setup(
            &document(
                vec![face, contour, drill],
                vec![
                    tool(1, CamToolKind::FlatEndMill, 6.0),
                    tool(2, CamToolKind::Drill, 5.0),
                ],
            ),
            1,
        )
        .unwrap();

        let mut cursor = 0;
        for _section in 0..3 {
            let mut last_feed = None;
            while cursor < program.commands.len() {
                let command = &program.commands[cursor];
                cursor += 1;
                match command {
                    CamCommandDto::Linear { .. } | CamCommandDto::Circular { .. } => {
                        last_feed = Some(cursor)
                    }
                    CamCommandDto::SectionEnd => break,
                    _ => {}
                }
            }
            let last_feed = last_feed.expect("section has feed moves");
            let exit_rapids: Vec<f64> = program.commands[last_feed..]
                .iter()
                .take_while(|command| !matches!(command, CamCommandDto::SectionEnd))
                .filter_map(|command| match command {
                    CamCommandDto::Rapid { to } => Some(to.z),
                    _ => None,
                })
                .take(2)
                .collect();
            assert_eq!(
                exit_rapids.len(),
                2,
                "section should rapid to retract then clearance"
            );
            assert!(
                (exit_rapids[0] - 3.0).abs() < 1.0e-9,
                "first exit rapid should stop at the retract plane, got {}",
                exit_rapids[0]
            );
            assert!(
                (exit_rapids[1] - 10.0).abs() < 1.0e-9,
                "second exit rapid should reach clearance, got {}",
                exit_rapids[1]
            );
        }
    }

    #[test]
    fn tapping_feeds_at_pitch_and_reverses_the_spindle() {
        let mut operation = drill_operation(DrillCycle::TappingRight);
        let CamOperationDto::Drill { thread_pitch, .. } = &mut operation else {
            unreachable!();
        };
        *thread_pitch = Some(1.25);
        let program = plan_setup(
            &document(vec![operation], vec![tool(2, CamToolKind::Tap, 6.0)]),
            1,
        )
        .unwrap();

        let feeds: Vec<f64> = program
            .commands
            .iter()
            .filter_map(|command| match command {
                CamCommandDto::Linear { feed, .. } => Some(*feed),
                _ => None,
            })
            .collect();
        assert_eq!(feeds, vec![500.0, 500.0]);
        let spindle_turns: Vec<SpindleDirection> = program
            .commands
            .iter()
            .filter_map(|command| match command {
                CamCommandDto::Spindle { direction, .. } => Some(*direction),
                _ => None,
            })
            .collect();
        assert_eq!(
            spindle_turns,
            vec![
                SpindleDirection::Clockwise,
                SpindleDirection::Counterclockwise,
                SpindleDirection::Clockwise,
                SpindleDirection::Off
            ]
        );
    }

    #[test]
    fn tapping_without_a_floating_holder_contract_fails_closed() {
        let mut operation = drill_operation(DrillCycle::TappingRight);
        let CamOperationDto::Drill {
            thread_pitch,
            floating_tap_holder,
            ..
        } = &mut operation
        else {
            unreachable!();
        };
        *thread_pitch = Some(1.25);
        *floating_tap_holder = false;
        let error = plan_setup(
            &document(vec![operation], vec![tool(2, CamToolKind::Tap, 6.0)]),
            1,
        )
        .unwrap_err();
        assert!(error.0.contains("floating tap holder"));
        assert!(error.0.contains("not rigid tapping"));
    }

    #[test]
    fn left_hand_tapping_enters_counterclockwise() {
        let mut operation = drill_operation(DrillCycle::TappingLeft);
        let CamOperationDto::Drill { thread_pitch, .. } = &mut operation else {
            unreachable!();
        };
        *thread_pitch = Some(1.0);
        let program = plan_setup(
            &document(vec![operation], vec![tool(2, CamToolKind::Tap, 6.0)]),
            1,
        )
        .unwrap();
        let spindle_turns: Vec<SpindleDirection> = program
            .commands
            .iter()
            .filter_map(|command| match command {
                CamCommandDto::Spindle { direction, .. } => Some(*direction),
                _ => None,
            })
            .collect();

        assert_eq!(
            spindle_turns,
            vec![
                SpindleDirection::Clockwise,
                SpindleDirection::Counterclockwise,
                SpindleDirection::Clockwise,
                SpindleDirection::Off
            ]
        );
    }

    #[test]
    fn reaming_feeds_back_out_at_the_feed_out_rate() {
        let mut operation = drill_operation(DrillCycle::Reaming);
        let CamOperationDto::Drill { feed_out, .. } = &mut operation else {
            unreachable!();
        };
        *feed_out = Some(60.0);
        let program = plan_setup(
            &document(vec![operation], vec![tool(2, CamToolKind::Reamer, 6.0)]),
            1,
        )
        .unwrap();
        let moves: Vec<(f64, f64)> = program
            .commands
            .iter()
            .filter_map(|command| match command {
                CamCommandDto::Linear { to, feed } => Some((to.z, *feed)),
                _ => None,
            })
            .collect();

        assert_eq!(moves, vec![(-7.0, 120.0), (3.0, 60.0)]);
    }

    #[test]
    fn cycle_specific_fields_fail_closed_when_mismatched() {
        let tapping = drill_operation(DrillCycle::TappingRight);
        let error = plan_setup(
            &document(vec![tapping.clone()], vec![tool(2, CamToolKind::Tap, 6.0)]),
            1,
        )
        .unwrap_err();
        assert!(error.0.contains("thread pitch"));
        let error = plan_setup(
            &document(vec![tapping], vec![tool(2, CamToolKind::Drill, 6.0)]),
            1,
        )
        .unwrap_err();
        assert!(error.0.contains("requires a tap tool"));

        let error = plan_setup(
            &document(
                vec![drill_operation(DrillCycle::ChipBreaking)],
                vec![tool(2, CamToolKind::Drill, 5.0)],
            ),
            1,
        )
        .unwrap_err();
        assert!(error.0.contains("peck depth"));

        let mut plain = drill_operation(DrillCycle::Drill);
        let CamOperationDto::Drill { peck_depth, .. } = &mut plain else {
            unreachable!();
        };
        *peck_depth = Some(2.0);
        let error = plan_setup(
            &document(vec![plain], vec![tool(2, CamToolKind::Drill, 5.0)]),
            1,
        )
        .unwrap_err();
        assert!(error.0.contains("only pecking cycles"));
    }

    #[test]
    fn repeated_work_offsets_duplicate_motion_under_each_offset_code() {
        let face = || CamOperationDto::Face {
            id: 1,
            name: "Face".into(),
            enabled: true,
            tool_id: 1,
            bounds: Rect2Dto {
                min: Point2Dto::new(0.0, 0.0),
                max: Point2Dto::new(40.0, 30.0),
            },
            top_z: 0.0,
            target_z: -1.0,
            step_over: 5.0,
            step_down: 1.0,
            safe_distance: 5.0,
            direction: FaceDirection::BothWays,
            clearance_z: 10.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting: cutting(),
        };
        let tools = || vec![tool(1, CamToolKind::FlatEndMill, 6.0)];
        let mut repeated = document(vec![face()], tools());
        repeated.setups[0].work_offset_count = 3;
        let program = plan_setup(&repeated, 1).unwrap();
        assert_eq!(
            program.work_offsets,
            vec![WorkOffset::G54, WorkOffset::G55, WorkOffset::G56]
        );
        let offset_commands = program
            .commands
            .iter()
            .filter_map(|command| match command {
                CamCommandDto::WorkOffset { offset } => Some(*offset),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            offset_commands,
            vec![WorkOffset::G54, WorkOffset::G55, WorkOffset::G56]
        );

        assert_eq!(program.stats.operation_count, 3);

        assert_eq!(
            program
                .commands
                .iter()
                .filter(|command| matches!(command, CamCommandDto::ToolChange { .. }))
                .count(),
            1
        );

        let single = plan_setup(&document(vec![face()], tools()), 1).unwrap();
        assert!(
            (program.stats.cutting_distance - single.stats.cutting_distance * 3.0).abs() < 1.0e-6
        );
    }

    #[test]
    fn tiny_stepover_fails_closed_before_generating_an_unbounded_path() {
        let operation = CamOperationDto::Face {
            id: 1,
            name: "Too many rows".into(),
            enabled: true,
            tool_id: 1,
            bounds: Rect2Dto {
                min: Point2Dto::new(0.0, 0.0),
                max: Point2Dto::new(40.0, 30.0),
            },
            top_z: 0.0,
            target_z: -1.0,
            step_over: 0.000_001,
            step_down: 1.0,
            safe_distance: 5.0,
            direction: FaceDirection::BothWays,
            clearance_z: 10.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting: cutting(),
        };
        let error = plan_setup(
            &document(
                vec![operation],
                vec![tool(1, CamToolKind::FlatEndMill, 6.0)],
            ),
            1,
        )
        .unwrap_err();
        assert!(error.0.contains("stepover rows"));
    }

    #[test]
    fn pocket_clears_every_depth_and_finishes_the_wall_inside_the_outline() {
        let operation = CamOperationDto::Pocket2d {
            id: 1,
            name: "Pocket".into(),
            enabled: true,
            tool_id: 1,
            chain_ref: None,
            outline: vec![
                Point2Dto::new(10.0, 5.0),
                Point2Dto::new(30.0, 5.0),
                Point2Dto::new(30.0, 25.0),
                Point2Dto::new(10.0, 25.0),
            ],
            top_z: 0.0,
            bottom_z: -2.0,
            step_down: 1.0,
            step_over: 2.4,
            direction: MillingDirection::Climb,
            clearance_z: 10.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting: cutting(),
        };
        let program = plan_setup(
            &document(
                vec![operation],
                vec![tool(1, CamToolKind::FlatEndMill, 6.0)],
            ),
            1,
        )
        .unwrap();

        let mut cut_depths = Vec::new();
        for command in &program.commands {
            if let CamCommandDto::Linear { to, .. } = command {
                if to.z < 0.0 {
                    assert!((13.0 - 1.0e-9..=27.0 + 1.0e-9).contains(&to.x));
                    assert!((8.0 - 1.0e-9..=22.0 + 1.0e-9).contains(&to.y));
                    cut_depths.push(to.z);
                }
            }
        }
        assert!(cut_depths.contains(&-1.0));
        assert!(cut_depths.contains(&-2.0));

        assert!(program.commands.iter().all(|command| match command {
            CamCommandDto::Rapid { to } => to.z >= 1.0,
            _ => true,
        }));
    }

    #[test]
    fn pocket_rejects_a_tool_that_cannot_fit_the_outline() {
        let operation = CamOperationDto::Pocket2d {
            id: 1,
            name: "Tiny pocket".into(),
            enabled: true,
            tool_id: 1,
            chain_ref: None,
            outline: vec![
                Point2Dto::new(10.0, 10.0),
                Point2Dto::new(14.0, 10.0),
                Point2Dto::new(14.0, 14.0),
                Point2Dto::new(10.0, 14.0),
            ],
            top_z: 0.0,
            bottom_z: -1.0,
            step_down: 1.0,
            step_over: 1.0,
            direction: MillingDirection::Climb,
            clearance_z: 10.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting: cutting(),
        };
        let error = plan_setup(
            &document(
                vec![operation],
                vec![tool(1, CamToolKind::FlatEndMill, 6.0)],
            ),
            1,
        )
        .unwrap_err();
        assert!(error.0.contains("too small") || error.0.contains("miter"));
    }

    #[test]
    fn concave_pocket_never_feeds_a_link_through_the_wall() {
        let outline = vec![
            Point2Dto::new(2.0, 2.0),
            Point2Dto::new(38.0, 2.0),
            Point2Dto::new(38.0, 28.0),
            Point2Dto::new(26.0, 28.0),
            Point2Dto::new(26.0, 12.0),
            Point2Dto::new(14.0, 12.0),
            Point2Dto::new(14.0, 28.0),
            Point2Dto::new(2.0, 28.0),
        ];
        let operation = CamOperationDto::Pocket2d {
            id: 1,
            name: "Concave pocket".into(),
            enabled: true,
            tool_id: 1,
            chain_ref: None,
            outline: outline.clone(),
            top_z: 0.0,
            bottom_z: -1.0,
            step_down: 1.0,
            step_over: 2.0,
            direction: MillingDirection::Climb,
            clearance_z: 10.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting: cutting(),
        };
        let program = plan_setup(
            &document(
                vec![operation],
                vec![tool(1, CamToolKind::FlatEndMill, 4.0)],
            ),
            1,
        )
        .expect("concave pocket plan");
        let mut position = None;
        let mut retracts_between_regions = 0usize;
        for command in &program.commands {
            match command {
                CamCommandDto::Rapid { to } => {
                    if position.is_some_and(|from: Point3Dto| from.z < 0.0 && to.z >= 1.0) {
                        retracts_between_regions += 1;
                    }
                    position = Some(*to);
                }
                CamCommandDto::Linear { to, feed }
                    if (*feed - cutting().feed_xy).abs() < 1.0e-9 =>
                {
                    let from = position.expect("motion has a start");
                    assert!((from.z - to.z).abs() < 1.0e-9);
                    assert!(
                        cutter_center_segment_is_clear(
                            Point2Dto::new(from.x, from.y),
                            Point2Dto::new(to.x, to.y),
                            &outline,
                            2.0 - 1.0e-6,
                        ),
                        "unsafe same-depth feed {from:?} -> {to:?}"
                    );
                    position = Some(*to);
                }
                _ => {
                    if let Some(to) = command.endpoint() {
                        position = Some(to);
                    }
                }
            }
        }
        assert!(
            retracts_between_regions > 2,
            "disconnected concave spans must use the safe retract fallback"
        );
    }

    #[test]
    fn chamfer_offsets_by_tip_offset_and_cuts_width_plus_tip_offset_deep() {
        let operation = CamOperationDto::Chamfer2d {
            id: 1,
            name: "Chamfer".into(),
            additional_chains: Vec::new(),
            closed: true,
            modeled_chamfer: None,
            enabled: true,
            tool_id: 3,
            chain_ref: None,
            path: vec![
                Point2Dto::new(10.0, 10.0),
                Point2Dto::new(30.0, 10.0),
                Point2Dto::new(30.0, 20.0),
                Point2Dto::new(10.0, 20.0),
            ],
            top_z: 0.0,
            chamfer_width: 1.0,
            tip_offset: 0.5,
            wall_side: ContourCompensation::Inside,
            direction: MillingDirection::Climb,
            clearance_z: 10.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting: cutting(),
        };
        let program = plan_setup(
            &document(
                vec![operation],
                vec![tool(3, CamToolKind::ChamferMill, 10.0)],
            ),
            1,
        )
        .unwrap();
        let cuts = program
            .commands
            .iter()
            .filter_map(|command| match command {
                CamCommandDto::Linear { to, .. } if to.z < 0.0 => Some(*to),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(!cuts.is_empty());
        assert!(cuts.iter().all(|point| (point.z - (-1.5)).abs() < 1.0e-9));

        assert!(cuts.iter().any(|point| (point.x - 30.5).abs() < 1.0e-9));
        assert!(cuts.iter().all(|point| point.x >= 9.5 - 1.0e-9));
    }

    #[test]
    fn chamfer_requires_a_chamfer_mill() {
        let operation = CamOperationDto::Chamfer2d {
            id: 1,
            name: "Wrong tool".into(),
            additional_chains: Vec::new(),
            closed: true,
            modeled_chamfer: None,
            enabled: true,
            tool_id: 1,
            chain_ref: None,
            path: vec![
                Point2Dto::new(10.0, 10.0),
                Point2Dto::new(30.0, 10.0),
                Point2Dto::new(30.0, 20.0),
                Point2Dto::new(10.0, 20.0),
            ],
            top_z: 0.0,
            chamfer_width: 1.0,
            tip_offset: 0.5,
            wall_side: ContourCompensation::Outside,
            direction: MillingDirection::Climb,
            clearance_z: 10.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting: cutting(),
        };
        let error = plan_setup(
            &document(
                vec![operation],
                vec![tool(1, CamToolKind::FlatEndMill, 6.0)],
            ),
            1,
        )
        .unwrap_err();
        assert!(error.0.contains("chamfer mill"));
    }

    #[test]
    fn modeled_upper_rim_matches_virtual_sharp_edge_without_corner_overcut() {
        let modeled: CamOperationDto = serde_json::from_value(serde_json::json!({
            "kind":"chamfer2d","id":1,"name":"Bevel","enabled":true,"tool_id":3,
            "path":[{"x":10.,"y":10.},{"x":30.,"y":10.},{"x":30.,"y":20.},{"x":10.,"y":20.}],
            "closed":true,"modeled_chamfer":{"additional_width":0.},
            "chain_ref":{"source":"model","keys":["edge:1:upper"]},
            "top_z":0.,"chamfer_width":1.,"tip_offset":0.5,"wall_side":"inside","direction":"climb",
            "clearance_z":10.,"retract_z":3.,"feed_height_z":1.,"cutting":cutting()
        }))
        .unwrap();
        let mut sharp = modeled.clone();
        if let CamOperationDto::Chamfer2d {
            path,
            modeled_chamfer,
            chain_ref,
            ..
        } = &mut sharp
        {
            *path = offset_polygon(path, 1., false).unwrap();
            *modeled_chamfer = None;
            *chain_ref = None;
        }
        let plan = |op| {
            plan_setup(
                &document(vec![op], vec![tool(3, CamToolKind::ChamferMill, 6.)]),
                1,
            )
            .unwrap()
        };
        assert_eq!(plan(modeled.clone()).commands, plan(sharp).commands);
        let mut extra = modeled;
        if let CamOperationDto::Chamfer2d {
            chamfer_width,
            modeled_chamfer,
            ..
        } = &mut extra
        {
            *chamfer_width = 1.2;
            modeled_chamfer.as_mut().unwrap().additional_width = 0.2;
        }
        let program = plan(extra);
        let profile = program
            .commands
            .iter()
            .filter_map(|c| match c {
                CamCommandDto::Linear { to, .. }
                    if (to.z + 1.7).abs() < 1e-8
                        && ((to.x - 8.5).abs() < 1e-8 || (to.x - 31.5).abs() < 1e-8) =>
                {
                    Some(*to)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            profile.len() >= 4,
            "additional width lowers the tool, not its upper-rim XY offset"
        );
    }

    #[test]
    fn modeled_hole_rim_accepts_six_mm_chamfer_tool_and_one_mm_tip_offset() {
        let path: Vec<_> = (0..96)
            .map(|i| {
                let angle = i as f64 * std::f64::consts::TAU / 96.;
                Point2Dto::new(20. + 3.25 * angle.cos(), 15. + 3.25 * angle.sin())
            })
            .collect();
        let operation: CamOperationDto = serde_json::from_value(serde_json::json!({
            "kind":"chamfer2d","id":1,"name":"Hole rim","enabled":true,"tool_id":3,
            "path":path,"closed":true,"modeled_chamfer":{"additional_width":0.},
            "chain_ref":{"source":"model","keys":["edge:1:rim"]},
            "top_z":0.,"chamfer_width":0.5,"tip_offset":1.,"wall_side":"outside","direction":"climb",
            "clearance_z":10.,"retract_z":3.,"feed_height_z":1.,"cutting":cutting()
        })).unwrap();
        let mut center_path = offset_polygon(&path, 1.5, true).unwrap();
        if signed_area(&center_path) < 0. {
            center_path.reverse();
        }
        center_path = split_closed_path_on_longest_edge(&center_path).unwrap();
        let old_leads = contour_leads(
            &center_path,
            ContourLeadOptions {
                closed: true,
                inside_closed: false,
                lead_in: 1.5,
                lead_out: 1.5,
                arc_radius: Some(1.5),
                bend_left: true,
                control_compensation: None,
            },
        )
        .unwrap();
        assert!(
            !chamfer_leads_clear_profile(&old_leads, &path, false, 1.5),
            "reproduce the old fixed-size lead failure"
        );
        let program = plan_setup(
            &document(vec![operation], vec![tool(3, CamToolKind::ChamferMill, 6.)]),
            1,
        )
        .unwrap();
        assert!(program
            .warnings
            .iter()
            .any(|w| w.contains("1.500 to 1.125")));
        let mut position = None;
        for command in &program.commands {
            if let Some(to) = command.endpoint() {
                if to.z < 0. {
                    assert!(
                        (to.z + 1.5).abs() < 1e-9,
                        "do not change chamfer depth to make a lead fit"
                    );
                    if let Some(from) = position {
                        match command {
                            CamCommandDto::Circular {
                                center, clockwise, ..
                            } => {
                                let arc = LeadArc {
                                    center: Point2Dto::new(center.x, center.y),
                                    clockwise: *clockwise,
                                    arc_end: Point2Dto::new(to.x, to.y),
                                };
                                assert!((0..path.len()).all(|i| arc_segment_distance(
                                    from,
                                    &arc,
                                    path[i],
                                    path[(i + 1) % path.len()]
                                ) >= 1.5 - 1e-6));
                            }
                            CamCommandDto::Linear { .. } => {
                                assert!((0..path.len()).all(|i| segment_segment_distance(
                                    from,
                                    Point2Dto::new(to.x, to.y),
                                    path[i],
                                    path[(i + 1) % path.len()]
                                ) >= 1.5 - 1e-6))
                            }
                            _ => {}
                        }
                    }
                }
                position = Some(Point2Dto::new(to.x, to.y));
            }
        }
    }

    #[test]
    fn multiple_chamfer_chains_keep_distinct_depths_and_retract_between_them() {
        let make_chain = |x: f64, z: f64, width: f64| {
            serde_json::json!({
                "path":(0..64).map(|i| { let a=i as f64*std::f64::consts::TAU/64.; Point2Dto::new(x+3.25*a.cos(),15.+3.25*a.sin()) }).collect::<Vec<_>>(),
                "closed":true,"top_z":z,"chamfer_width":width,"wall_side":"outside"
            })
        };
        let mut value = make_chain(10., 0., 0.5);
        for (key, val) in serde_json::json!({
            "kind":"chamfer2d","id":1,"name":"Two rims","enabled":true,"tool_id":3,
            "tip_offset":1.,"direction":"climb","clearance_z":10.,"retract_z":3.,"feed_height_z":1.,"cutting":cutting(),
            "additional_chains":[make_chain(30., -2., 0.25)]
        }).as_object().unwrap() { value[key] = val.clone(); }
        let operation: CamOperationDto = serde_json::from_value(value).unwrap();
        let doc = document(
            vec![operation.clone()],
            vec![tool(3, CamToolKind::ChamferMill, 6.)],
        );
        let program = plan_setup(&doc, 1).unwrap();
        assert_eq!(program.stats.operation_count, 1);
        assert_eq!(
            program
                .commands
                .iter()
                .filter(|c| matches!(c, CamCommandDto::Circular { .. }))
                .count(),
            4
        );
        let mut previous: Option<Point3Dto> = None;
        let mut saw_left = false;
        let mut saw_right = false;
        let mut crossed_at_clearance = false;
        for c in &program.commands {
            if let Some(to) = c.endpoint() {
                if let Some(from) = previous {
                    if (to.x - from.x).abs() > 10. {
                        assert_eq!(from.z, 10.);
                        assert_eq!(to.z, 10.);
                        crossed_at_clearance = true;
                    }
                }
                if to.z < 0. {
                    if to.x < 20. {
                        saw_left = true;
                        assert_eq!(to.z, -1.5);
                    } else {
                        saw_right = true;
                        assert_eq!(to.z, -3.25);
                    }
                }
                previous = Some(to);
            }
        }
        assert!(saw_left && saw_right && crossed_at_clearance);
        assert_eq!(
            serde_json::from_str::<CamOperationDto>(&serde_json::to_string(&operation).unwrap())
                .unwrap(),
            operation
        );
        let mut invalid = doc.clone();
        if let CamOperationDto::Chamfer2d {
            additional_chains, ..
        } = &mut invalid.setups[0].operations[0]
        {
            additional_chains[0].chamfer_width = 3.;
        }
        let error = plan_setup(&invalid, 1).unwrap_err().0;
        assert!(
            error.contains("Chain 2") && error.contains("exceeds the tool radius"),
            "{error}"
        );
        let mut high = doc;
        if let CamOperationDto::Chamfer2d {
            additional_chains, ..
        } = &mut high.setups[0].operations[0]
        {
            additional_chains[0].top_z = 2.;
        }
        assert!(plan_setup(&high, 1).unwrap_err().0.contains("feed height"));
    }

    #[test]
    fn chamfer_clearance_rejects_crossing_segments_and_folded_offsets() {
        let p = Point2Dto::new;

        let boundary = vec![
            p(0., 0.),
            p(10., 0.),
            p(10., 10.),
            p(7., 10.),
            p(7., 3.),
            p(3., 3.),
            p(3., 10.),
            p(0., 10.),
        ];
        let path = vec![p(1., 1.), p(9., 1.), p(9., 9.), p(1., 9.)];
        assert!(path.iter().all(|p| point_in_polygon(*p, &boundary)));
        assert!(!chamfer_profile_is_clear(
            &path, &boundary, true, false, 0.5
        ));
        let folded = vec![p(1., 1.), p(9., 9.), p(1., 9.), p(9., 1.)];
        assert!(!chamfer_profile_is_clear(
            &folded, &boundary, false, false, 0.5
        ));
        let square = vec![p(0., 0.), p(10., 0.), p(10., 10.), p(0., 10.)];
        assert!(chamfer_profile_is_clear(&path, &square, true, false, 0.5));
    }

    #[test]
    fn open_chamfers_keep_the_material_side_and_never_close_the_path() {
        for wall in ["left", "right"] {
            for direction in ["climb", "conventional"] {
                let operation: CamOperationDto = serde_json::from_value(serde_json::json!({
                    "kind":"chamfer2d","id":1,"name":"Open bevel","enabled":true,"tool_id":3,
                    "path":[{"x":10.,"y":10.},{"x":30.,"y":10.}],"closed":false,
                    "top_z":0.,"chamfer_width":0.2,"tip_offset":0.5,"wall_side":wall,"direction":direction,
                    "clearance_z":10.,"retract_z":3.,"feed_height_z":1.,"cutting":cutting()
                })).unwrap();
                let program = plan_setup(
                    &document(vec![operation], vec![tool(3, CamToolKind::ChamferMill, 6.)]),
                    1,
                )
                .unwrap();
                let y = if wall == "left" { 9.5 } else { 10.5 };
                let forward = (wall == "right") == (direction == "climb");
                let last = if forward { 30. } else { 10. };
                let profile_ends = program
                    .commands
                    .iter()
                    .filter_map(|c| match c {
                        CamCommandDto::Linear { to, .. }
                            if (to.z + 0.7).abs() < 1e-8 && (to.y - y).abs() < 1e-8 =>
                        {
                            Some(to.x)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(profile_ends, vec![last], "{wall} {direction}");
                assert_eq!(
                    program
                        .commands
                        .iter()
                        .filter(|c| matches!(c, CamCommandDto::Circular { .. }))
                        .count(),
                    2
                );
            }
        }
    }

    fn thread_operation(hand: ThreadHand, direction: MillingDirection) -> CamOperationDto {
        CamOperationDto::Thread {
            id: 3,
            name: "Thread".into(),
            enabled: true,
            tool_id: 7,
            points: vec![Point2Dto::new(20.0, 15.0)],
            holes: Vec::new(),
            top_z: 0.0,
            bottom_z: -8.0,
            pitch: 1.0,
            major_diameter: 6.0,
            minor_diameter: 5.035,
            hand,
            direction,
            radial_passes: 1,
            step_over: None,
            clearance_z: 10.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting: cutting(),
        }
    }

    fn thread_program(operation: CamOperationDto, tool: CamToolDto) -> CamProgramDto {
        plan_setup(&document(vec![operation], vec![tool]), 1).unwrap()
    }

    #[test]
    fn picked_holes_thread_from_their_own_top_and_bottom_planes() {
        let mut operation = thread_operation(ThreadHand::Right, MillingDirection::Climb);
        let CamOperationDto::Thread { points, holes, .. } = &mut operation else {
            unreachable!();
        };
        points.clear();
        holes.push(picked_hole(10.0, 10.0, 0.0, -6.0));
        holes.push(picked_hole(30.0, 20.0, -1.0, -8.0));
        let program = thread_program(operation, tool(7, CamToolKind::ThreadMill, 4.8));

        let plunge_zs: Vec<f64> = program
            .commands
            .iter()
            .filter_map(|command| match command {
                CamCommandDto::Linear { to, feed } if (*feed - 200.0).abs() < 1.0e-9 => Some(to.z),
                _ => None,
            })
            .collect();
        assert_eq!(plunge_zs, [-6.0, -8.0]);

        let arcs = circular_moves(&program);
        let deepest = |center_x: f64| {
            arcs.iter()
                .filter(|(_, command, _)| match command {
                    CamCommandDto::Circular { center, .. } => (center.x - center_x).abs() < 1.0e-9,
                    _ => false,
                })
                .map(|(from, command, _)| match command {
                    CamCommandDto::Circular { to, .. } => from.z.min(to.z),
                    _ => unreachable!(),
                })
                .fold(f64::INFINITY, f64::min)
        };
        assert!((deepest(10.0) - -6.0).abs() < 1.0e-9);
        assert!((deepest(30.0) - -8.0).abs() < 1.0e-9);
    }

    /// Circular commands with their start points and sweep angles, walking the
    /// program's motion so each arc can be measured.
    fn circular_moves(program: &CamProgramDto) -> Vec<(Point3Dto, CamCommandDto, f64)> {
        let mut position = Point3Dto::new(0.0, 0.0, 0.0);
        let mut moves = Vec::new();
        for command in &program.commands {
            match command {
                CamCommandDto::Rapid { to } | CamCommandDto::Linear { to, .. } => position = *to,
                CamCommandDto::Circular {
                    to,
                    center,
                    clockwise,
                    ..
                } => {
                    let start = (position.y - center.y).atan2(position.x - center.x);
                    let end = (to.y - center.y).atan2(to.x - center.x);
                    let mut sweep = end - start;
                    if *clockwise {
                        while sweep >= 0.0 {
                            sweep -= std::f64::consts::TAU;
                        }
                    } else {
                        while sweep <= 0.0 {
                            sweep += std::f64::consts::TAU;
                        }
                    }
                    moves.push((position, command.clone(), sweep));
                    position = *to;
                }
                _ => {}
            }
        }
        moves
    }

    #[test]
    fn right_hand_climb_thread_orbits_counterclockwise_and_ascends() {
        let program = thread_program(
            thread_operation(ThreadHand::Right, MillingDirection::Climb),
            tool(7, CamToolKind::ThreadMill, 4.8),
        );
        let arcs = circular_moves(&program)
            .into_iter()
            .filter(|(from, command, _)| match command {
                CamCommandDto::Circular { to, .. } => (to.z - from.z).abs() > 1.0e-9,
                _ => false,
            })
            .collect::<Vec<_>>();
        assert!(arcs.len() >= 16, "8 revolutions split into semicircles");

        let zs: Vec<f64> = arcs
            .iter()
            .map(|(_, command, _)| match command {
                CamCommandDto::Circular { to, .. } => to.z,
                _ => unreachable!(),
            })
            .collect();

        assert!(zs[0] >= -8.0 - 1.0e-9 && zs[0] < -8.0 + 1.0);
        assert!(zs[zs.len() - 1].abs() < 1.0e-9);
        assert!(zs.windows(2).all(|pair| pair[1] >= pair[0] - 1.0e-9));
        for (_, command, sweep) in &arcs {
            let CamCommandDto::Circular { clockwise, .. } = command else {
                unreachable!();
            };
            assert!(!clockwise, "internal climb milling orbits counterclockwise");
            assert!(
                sweep.abs() <= std::f64::consts::PI + 1.0e-9,
                "arcs split at 180 degrees, got {sweep}"
            );
        }
    }

    #[test]
    fn thread_entry_and_exit_are_tangent_to_the_helix() {
        let program = thread_program(
            thread_operation(ThreadHand::Right, MillingDirection::Climb),
            tool(7, CamToolKind::ThreadMill, 4.8),
        );
        let arcs = circular_moves(&program);
        assert!(arcs.len() >= 3, "entry, helix, and exit arcs are present");
        let tangent = |point: Point3Dto, center: Point3Dto, clockwise: bool| {
            let radial = Point2Dto::new(point.x - center.x, point.y - center.y);
            let raw = if clockwise {
                Point2Dto::new(radial.y, -radial.x)
            } else {
                Point2Dto::new(-radial.y, radial.x)
            };
            let length = raw.x.hypot(raw.y);
            Point2Dto::new(raw.x / length, raw.y / length)
        };
        for pair in arcs.windows(2) {
            let (left_from, left, _) = &pair[0];
            let (right_from, right, _) = &pair[1];
            let CamCommandDto::Circular {
                to: left_to,
                center: left_center,
                clockwise: left_clockwise,
                ..
            } = left
            else {
                unreachable!();
            };
            let CamCommandDto::Circular {
                center: right_center,
                clockwise: right_clockwise,
                ..
            } = right
            else {
                unreachable!();
            };
            let join_distance = ((left_to.x - right_from.x).powi(2)
                + (left_to.y - right_from.y).powi(2)
                + (left_to.z - right_from.z).powi(2))
            .sqrt();
            assert!(join_distance < 1.0e-9);
            let left_tangent = tangent(*left_to, *left_center, *left_clockwise);
            let right_tangent = tangent(*right_from, *right_center, *right_clockwise);
            assert!(
                left_tangent.x * right_tangent.x + left_tangent.y * right_tangent.y >= 1.0 - 1.0e-9,
                "thread arc join is not tangent: {left_from:?} -> {left_to:?} -> {right_from:?}"
            );
        }
    }

    #[test]
    fn right_hand_conventional_thread_orbits_clockwise_and_descends() {
        let program = thread_program(
            thread_operation(ThreadHand::Right, MillingDirection::Conventional),
            tool(7, CamToolKind::ThreadMill, 4.8),
        );
        let arcs = circular_moves(&program)
            .into_iter()
            .filter(|(from, command, _)| match command {
                CamCommandDto::Circular { to, .. } => (to.z - from.z).abs() > 1.0e-9,
                _ => false,
            })
            .collect::<Vec<_>>();
        assert!(!arcs.is_empty());
        let zs: Vec<f64> = arcs
            .iter()
            .map(|(_, command, _)| match command {
                CamCommandDto::Circular { to, .. } => to.z,
                _ => unreachable!(),
            })
            .collect();
        assert!(zs[0] <= 1.0e-9 && zs[0] > -1.0);
        assert!((zs[zs.len() - 1] + 8.0).abs() < 1.0e-9);
        assert!(zs.windows(2).all(|pair| pair[1] <= pair[0] + 1.0e-9));
        for (_, command, _) in &arcs {
            let CamCommandDto::Circular { clockwise, .. } = command else {
                unreachable!();
            };
            assert!(clockwise, "internal conventional milling orbits clockwise");
        }
    }

    #[test]
    fn left_hand_thread_reverses_the_z_travel() {
        let program = thread_program(
            thread_operation(ThreadHand::Left, MillingDirection::Climb),
            tool(7, CamToolKind::ThreadMill, 4.8),
        );
        let arcs = circular_moves(&program)
            .into_iter()
            .filter(|(from, command, _)| match command {
                CamCommandDto::Circular { to, .. } => (to.z - from.z).abs() > 1.0e-9,
                _ => false,
            })
            .collect::<Vec<_>>();
        assert!(!arcs.is_empty());
        let zs: Vec<f64> = arcs
            .iter()
            .map(|(_, command, _)| match command {
                CamCommandDto::Circular { to, .. } => to.z,
                _ => unreachable!(),
            })
            .collect();
        assert!(zs[0] <= 1.0e-9 && zs[0] > -1.0);
        assert!((zs[zs.len() - 1] + 8.0).abs() < 1.0e-9);
        assert!(zs.windows(2).all(|pair| pair[1] <= pair[0] + 1.0e-9));
    }

    #[test]
    fn thread_radial_passes_open_up_and_finish_at_the_full_orbit() {
        let mut operation = thread_operation(ThreadHand::Right, MillingDirection::Climb);
        let CamOperationDto::Thread {
            radial_passes,
            step_over,
            ..
        } = &mut operation
        else {
            unreachable!();
        };
        *radial_passes = 3;
        *step_over = Some(0.2);
        let program = thread_program(operation, tool(7, CamToolKind::ThreadMill, 4.8));

        let lead_radii: Vec<f64> = program
            .commands
            .iter()
            .scan(Point3Dto::new(0.0, 0.0, 0.0), |position, command| {
                let from = *position;
                if let Some(to) = command.endpoint() {
                    *position = to;
                }
                match command {
                    CamCommandDto::Circular { to, .. }
                        if (from.x - 20.0).abs() < 1.0e-9
                            && (from.y - 15.0).abs() < 1.0e-9
                            && (from.z - to.z).abs() < 1.0e-9
                            && to.x > 20.0 + 1.0e-9 =>
                    {
                        Some(Some(to.x - 20.0))
                    }
                    _ => Some(None),
                }
            })
            .flatten()
            .collect();
        assert_eq!(lead_radii.len(), 3);
        for (actual, expected) in lead_radii.iter().zip([0.2, 0.4, 0.6].iter()) {
            assert!(
                (actual - expected).abs() < 1.0e-9,
                "orbit radius {actual} should be {expected}"
            );
        }
    }

    #[test]
    fn thread_motion_is_reported_per_operation() {
        let program = thread_program(
            thread_operation(ThreadHand::Right, MillingDirection::Climb),
            tool(7, CamToolKind::ThreadMill, 4.8),
        );
        let entry = program
            .per_operation
            .iter()
            .find(|entry| entry.operation_id == 3)
            .expect("the thread operation reports its own totals");
        assert!(entry.cutting_distance > 0.0);
        assert!(entry.estimated_seconds > 0.0);
    }

    #[test]
    fn thread_validation_fails_closed() {
        let error = plan_setup(
            &document(
                vec![thread_operation(ThreadHand::Right, MillingDirection::Climb)],
                vec![tool(7, CamToolKind::FlatEndMill, 4.8)],
            ),
            1,
        )
        .unwrap_err();
        assert!(error.0.contains("thread mill"));

        let error = plan_setup(
            &document(
                vec![thread_operation(ThreadHand::Right, MillingDirection::Climb)],
                vec![tool(7, CamToolKind::ThreadMill, 5.5)],
            ),
            1,
        )
        .unwrap_err();
        assert!(error.0.contains("minor diameter"));

        let mut operation = thread_operation(ThreadHand::Right, MillingDirection::Climb);
        let CamOperationDto::Thread { radial_passes, .. } = &mut operation else {
            unreachable!();
        };
        *radial_passes = 2;
        let error = plan_setup(
            &document(vec![operation], vec![tool(7, CamToolKind::ThreadMill, 4.8)]),
            1,
        )
        .unwrap_err();
        assert!(error.0.contains("needs a stepover"));

        let mut operation = thread_operation(ThreadHand::Right, MillingDirection::Climb);
        let CamOperationDto::Thread {
            radial_passes,
            step_over,
            ..
        } = &mut operation
        else {
            unreachable!();
        };
        *radial_passes = 3;
        *step_over = Some(0.5);
        let error = plan_setup(
            &document(vec![operation], vec![tool(7, CamToolKind::ThreadMill, 4.8)]),
            1,
        )
        .unwrap_err();
        assert!(error.0.contains("consume the whole orbit"));

        let mut operation = thread_operation(ThreadHand::Right, MillingDirection::Climb);
        let CamOperationDto::Thread { step_over, .. } = &mut operation else {
            unreachable!();
        };
        *step_over = Some(0.2);
        let error = plan_setup(
            &document(vec![operation], vec![tool(7, CamToolKind::ThreadMill, 4.8)]),
            1,
        )
        .unwrap_err();
        assert!(error.0.contains("takes a stepover only"));
    }

    fn contour_pass_program(operation: CamOperationDto) -> CamProgramDto {
        plan_setup(
            &document(
                vec![operation],
                vec![tool(1, CamToolKind::FlatEndMill, 6.0)],
            ),
            1,
        )
        .expect("plan")
    }

    /// Linear moves (target, feed) at the given depth.
    fn linears_at(program: &CamProgramDto, depth: f64) -> Vec<(Point3Dto, f64)> {
        program
            .commands
            .iter()
            .filter_map(|command| match command {
                CamCommandDto::Linear { to, feed } if (to.z - depth).abs() < 1.0e-9 => {
                    Some((*to, *feed))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn contour_roughing_passes_step_to_the_wall_then_finish_and_spring() {
        let mut operation =
            closed_boss_operation(CompensationMode::InSoftware, ContourCompensation::Outside);
        let CamOperationDto::Contour2d {
            roughing_passes,
            roughing_step_over,
            finishing_pass,
            finish_allowance,
            finish_feed,
            spring_pass,
            ..
        } = &mut operation
        else {
            unreachable!();
        };
        *roughing_passes = 2;
        *roughing_step_over = Some(2.0);
        *finishing_pass = true;
        *finish_allowance = 0.5;
        *finish_feed = Some(300.0);
        *spring_pass = true;
        let program = contour_pass_program(operation);
        let cuts = linears_at(&program, -2.0);

        assert!(cuts.iter().any(|(p, _)| (p.x + 0.5).abs() < 1.0e-9));
        assert!(cuts.iter().any(|(p, _)| (p.x - 1.5).abs() < 1.0e-9));
        assert!(cuts.iter().any(|(p, _)| (p.x - 2.0).abs() < 1.0e-9));

        assert_eq!(
            cuts.iter()
                .filter(|(_, feed)| (*feed - 300.0).abs() < 1.0e-9)
                .count(),
            10
        );

        assert_eq!(
            cuts.iter()
                .filter(|(_, feed)| (*feed - 800.0).abs() < 1.0e-9)
                .count(),
            12
        );
    }

    #[test]
    fn climb_outside_preserves_a_cw_loop_and_keeps_material_right() {
        let mut operation =
            closed_boss_operation(CompensationMode::InSoftware, ContourCompensation::Outside);
        let CamOperationDto::Contour2d { path, .. } = &mut operation else {
            unreachable!();
        };

        *path = vec![
            Point2Dto::new(5.0, 5.0),
            Point2Dto::new(5.0, 15.0),
            Point2Dto::new(15.0, 15.0),
            Point2Dto::new(15.0, 5.0),
        ];
        let program = contour_pass_program(operation);
        let cuts = linears_at(&program, -2.0);

        assert!((cuts[0].0.x - 2.0).abs() < 1.0e-9 && (cuts[0].0.y + 3.0).abs() < 1.0e-9);
        assert!((cuts[1].0.x - 2.0).abs() < 1.0e-9 && (cuts[1].0.y - 2.0).abs() < 1.0e-9);
        assert!((cuts[2].0.x - 2.0).abs() < 1.0e-9 && (cuts[2].0.y - 18.0).abs() < 1.0e-9);
    }

    #[test]
    fn climb_reverses_an_open_chain_but_keeps_the_physical_tool_side() {
        let mut operation = open_chain_operation(ContourCompensation::Left);
        let CamOperationDto::Contour2d { direction, .. } = &mut operation else {
            unreachable!();
        };
        *direction = MillingDirection::Climb;
        let program = contour_pass_program(operation);
        let cuts = linears_at(&program, -2.0);

        assert!(cuts[0].0.x.abs() < 1.0e-9 && (cuts[0].0.y - 8.0).abs() < 1.0e-9);
        assert!(cuts
            .iter()
            .any(|(p, _)| (p.x - 27.0).abs() < 1.0e-9 && (p.y - 8.0).abs() < 1.0e-9));
        assert!(cuts
            .iter()
            .any(|(p, _)| (p.x - 5.0).abs() < 1.0e-9 && (p.y - 8.0).abs() < 1.0e-9));
    }

    #[test]
    fn arc_leads_round_the_straight_lead_onto_the_profile() {
        let mut operation =
            closed_boss_operation(CompensationMode::InSoftware, ContourCompensation::Outside);
        let CamOperationDto::Contour2d {
            lead_arc_radius, ..
        } = &mut operation
        else {
            unreachable!();
        };
        *lead_arc_radius = Some(2.0);
        let program = contour_pass_program(operation);

        let arcs: Vec<(Point3Dto, Point3Dto, bool)> = program
            .commands
            .iter()
            .filter_map(|command| match command {
                CamCommandDto::Circular {
                    to,
                    center,
                    clockwise,
                    ..
                } => Some((*to, *center, *clockwise)),
                _ => None,
            })
            .collect();
        assert_eq!(arcs.len(), 2);
        assert!((arcs[0].0.x - 2.0).abs() < 1.0e-9 && (arcs[0].0.y - 2.0).abs() < 1.0e-9);
        assert!(arcs[0].1.x.abs() < 1.0e-9 && (arcs[0].1.y - 2.0).abs() < 1.0e-9);
        assert!(!arcs[0].2);
        assert!(arcs[1].0.x.abs() < 1.0e-9 && arcs[1].0.y.abs() < 1.0e-9);
        assert!((arcs[1].1.x - 2.0).abs() < 1.0e-9 && arcs[1].1.y.abs() < 1.0e-9);
        assert!(!arcs[1].2);
        let cuts = linears_at(&program, -2.0);
        assert!(cuts
            .iter()
            .any(|(p, _)| (p.x + 5.0).abs() < 1.0e-9 && p.y.abs() < 1.0e-9));
        assert!(cuts
            .iter()
            .any(|(p, _)| p.x.abs() < 1.0e-9 && p.y.abs() < 1.0e-9));
        assert!(cuts
            .iter()
            .any(|(p, _)| p.x.abs() < 1.0e-9 && (p.y + 5.0).abs() < 1.0e-9));
    }

    #[test]
    fn in_control_arc_lead_keeps_activation_on_the_straight_lead() {
        let mut operation =
            closed_boss_operation(CompensationMode::InControl, ContourCompensation::Outside);
        let CamOperationDto::Contour2d {
            lead_arc_radius, ..
        } = &mut operation
        else {
            unreachable!();
        };
        *lead_arc_radius = Some(2.0);
        let program = contour_pass_program(operation);
        let on_index = program
            .commands
            .iter()
            .position(|command| matches!(command, CamCommandDto::CutterCompensationOn { .. }))
            .expect("activation");
        let off_index = program
            .commands
            .iter()
            .position(|command| matches!(command, CamCommandDto::CutterCompensationOff))
            .expect("cancellation");

        assert!(matches!(
            program.commands[on_index + 1],
            CamCommandDto::Linear { .. }
        ));
        assert!(matches!(
            program.commands[off_index + 1],
            CamCommandDto::Linear { .. }
        ));
        assert!(program.commands[on_index..off_index]
            .iter()
            .any(|command| matches!(command, CamCommandDto::Circular { .. })));
        let arc_centers: Vec<Point3Dto> = program.commands[on_index..off_index]
            .iter()
            .filter_map(|command| match command {
                CamCommandDto::Circular { center, .. } => Some(*center),
                _ => None,
            })
            .collect();
        assert_eq!(arc_centers.len(), 2);

        assert!(arc_centers[0].x.abs() < 1.0e-9);
        assert!((arc_centers[0].y - 5.0).abs() < 1.0e-9);
        assert!((arc_centers[1].x - 5.0).abs() < 1.0e-9);
        assert!(arc_centers[1].y.abs() < 1.0e-9);

        let cuts = linears_at(&program, -2.0);
        assert!(cuts
            .iter()
            .any(|(p, _)| (p.x - 15.0).abs() < 1.0e-9 && (p.y - 5.0).abs() < 1.0e-9));
        assert!(!cuts
            .iter()
            .any(|(p, _)| (p.x - 18.0).abs() < 1.0e-9 && (p.y - 2.0).abs() < 1.0e-9));
    }

    #[test]
    fn one_way_facing_cuts_one_direction_and_returns_above_stock() {
        let operation = CamOperationDto::Face {
            id: 1,
            name: "Face".into(),
            enabled: true,
            tool_id: 1,
            bounds: Rect2Dto {
                min: Point2Dto::new(0.0, 0.0),
                max: Point2Dto::new(40.0, 30.0),
            },
            top_z: 0.0,
            target_z: -1.0,
            step_over: 6.0,
            step_down: 1.0,
            safe_distance: 5.0,
            direction: FaceDirection::Climb,
            clearance_z: 10.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting: cutting(),
        };
        let program = contour_pass_program(operation);

        let mut position: Option<Point3Dto> = None;
        let mut saw_clearance_return = false;
        for command in &program.commands {
            match command {
                CamCommandDto::Rapid { to } => {
                    if let Some(from) = position {
                        if distance_2d(Point2Dto::new(from.x, from.y), Point2Dto::new(to.x, to.y))
                            > EPSILON
                        {
                            assert!(
                                (from.z - 10.0).abs() < EPSILON && (to.z - 10.0).abs() < EPSILON
                            );
                            saw_clearance_return = true;
                        }
                    }
                    position = Some(*to);
                }
                CamCommandDto::Linear { to, .. } => {
                    if let Some(from) = position {
                        let horizontal = (to.z - from.z).abs() < 1.0e-9 && from.z < -0.5;
                        if horizontal && (to.x - from.x).abs() > 1.0 {
                            assert!(
                                to.x < from.x,
                                "climb facing rows must all run -X: {from:?} -> {to:?}"
                            );
                        }
                    }
                    position = Some(*to);
                }
                _ => {}
            }
        }
        assert!(saw_clearance_return);
    }

    #[test]
    fn pocket_wall_finish_follows_the_milling_direction() {
        let operation = CamOperationDto::Pocket2d {
            id: 1,
            name: "Pocket".into(),
            enabled: true,
            tool_id: 1,
            chain_ref: None,

            outline: vec![
                Point2Dto::new(10.0, 5.0),
                Point2Dto::new(30.0, 5.0),
                Point2Dto::new(30.0, 25.0),
                Point2Dto::new(10.0, 25.0),
            ],
            top_z: 0.0,
            bottom_z: -1.0,
            step_down: 1.0,
            step_over: 6.0,
            direction: MillingDirection::Climb,
            clearance_z: 10.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting: cutting(),
        };
        let program = contour_pass_program(operation);

        let arcs: Vec<_> = program
            .commands
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c, CamCommandDto::Circular { .. }))
            .collect();
        assert_eq!(arcs.len(), 2);
        let first = arcs[0].1.endpoint().unwrap();
        let mut polygon = vec![Point2Dto::new(first.x, first.y)];
        for command in &program.commands[arcs[0].0 + 1..arcs[1].0] {
            let point = command.endpoint().unwrap();
            assert!(
                (point.x - 13.0).abs() < 1e-9
                    || (point.x - 27.0).abs() < 1e-9
                    || (point.y - 8.0).abs() < 1e-9
                    || (point.y - 22.0).abs() < 1e-9
            );
            polygon.push(Point2Dto::new(point.x, point.y));
        }
        assert!((signed_area(&polygon) - 196.0).abs() < 1e-9);
    }

    #[test]
    fn contour_pass_and_lead_validation_fails_closed() {
        let mut operation = open_chain_operation(ContourCompensation::Left);
        let CamOperationDto::Contour2d { spring_pass, .. } = &mut operation else {
            unreachable!();
        };
        *spring_pass = true;
        let error = plan_setup(
            &document(
                vec![operation],
                vec![tool(1, CamToolKind::FlatEndMill, 6.0)],
            ),
            1,
        )
        .unwrap_err();
        assert!(error.0.contains("spring pass"));

        let mut operation =
            closed_boss_operation(CompensationMode::InSoftware, ContourCompensation::Outside);
        let CamOperationDto::Contour2d {
            roughing_passes, ..
        } = &mut operation
        else {
            unreachable!();
        };
        *roughing_passes = 2;
        let error = plan_setup(
            &document(
                vec![operation],
                vec![tool(1, CamToolKind::FlatEndMill, 6.0)],
            ),
            1,
        )
        .unwrap_err();
        assert!(error.0.contains("radial step-over"));

        let mut operation =
            closed_boss_operation(CompensationMode::InSoftware, ContourCompensation::Outside);
        let CamOperationDto::Contour2d { finishing_pass, .. } = &mut operation else {
            unreachable!();
        };
        *finishing_pass = true;
        let error = plan_setup(
            &document(
                vec![operation],
                vec![tool(1, CamToolKind::FlatEndMill, 6.0)],
            ),
            1,
        )
        .unwrap_err();
        assert!(error.0.contains("finish allowance"));

        let mut operation =
            closed_boss_operation(CompensationMode::InSoftware, ContourCompensation::Inside);
        let CamOperationDto::Contour2d {
            lead_arc_radius, ..
        } = &mut operation
        else {
            unreachable!();
        };
        *lead_arc_radius = Some(2.0);
        let error = plan_setup(
            &document(
                vec![operation],
                vec![tool(1, CamToolKind::FlatEndMill, 6.0)],
            ),
            1,
        )
        .unwrap_err();
        assert!(error.0.contains("cannot fit"), "{error}");

        let mut operation =
            closed_boss_operation(CompensationMode::InSoftware, ContourCompensation::On);
        let CamOperationDto::Contour2d {
            roughing_passes,
            roughing_step_over,
            ..
        } = &mut operation
        else {
            unreachable!();
        };
        *roughing_passes = 2;
        *roughing_step_over = Some(2.0);
        let error = plan_setup(
            &document(
                vec![operation],
                vec![tool(1, CamToolKind::FlatEndMill, 6.0)],
            ),
            1,
        )
        .unwrap_err();
        assert!(error.0.contains("on-path"));
    }
}
