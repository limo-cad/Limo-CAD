use std::collections::HashSet;

use limo_cad_core::BodyId;
use serde::{Deserialize, Serialize};

const MAX_SETUPS: usize = 64;
const MAX_TOOLS: usize = 256;
const MAX_OPERATIONS_PER_SETUP: usize = 2_048;
const MAX_PATH_POINTS: usize = 250_000;
const EPSILON: f64 = 1.0e-9;

fn first_id() -> u64 {
    1
}

fn default_true() -> bool {
    true
}

/// Facing plunge clearance when a document predates the field.
fn default_face_safe_distance() -> f64 {
    5.0
}

fn one_u8() -> u8 {
    1
}

fn valid_toolpath_fingerprint(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn default_flute_count() -> u32 {
    2
}

/// Unit system a CAM document is displayed and posted in.
///
/// All persisted geometry and the neutral motion program stay canonical
/// millimetres regardless of this setting; the unit converts operator-facing
/// input/output and posted controller words. Switching units therefore never
/// rewrites saved coordinates and cannot accumulate rounding error.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CamUnits {
    #[default]
    Millimeters,
    Inches,
}

impl CamUnits {
    /// Multiply a canonical millimetre value by this to obtain document units.
    pub fn from_mm(self, value_mm: f64) -> f64 {
        match self {
            Self::Millimeters => value_mm,
            Self::Inches => value_mm / 25.4,
        }
    }

    /// Multiply a document-unit value by this to obtain canonical millimetres.
    pub fn to_mm(self, value: f64) -> f64 {
        match self {
            Self::Millimeters => value,
            Self::Inches => value * 25.4,
        }
    }

    pub fn length_label(self) -> &'static str {
        match self {
            Self::Millimeters => "mm",
            Self::Inches => "in",
        }
    }

    pub fn feed_label(self) -> &'static str {
        match self {
            Self::Millimeters => "mm/min",
            Self::Inches => "in/min",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point2Dto {
    pub x: f64,
    pub y: f64,
}

impl Point2Dto {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub(crate) fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point3Dto {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Point3Dto {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub(crate) fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect2Dto {
    pub min: Point2Dto,
    pub max: Point2Dto,
}

impl Rect2Dto {
    pub fn validate(self, label: &str) -> Result<(), String> {
        if !self.min.is_finite() || !self.max.is_finite() {
            return Err(format!("{label} must contain finite coordinates"));
        }
        if self.max.x - self.min.x <= EPSILON || self.max.y - self.min.y <= EPSILON {
            return Err(format!("{label} must have positive width and height"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StockBoxDto {
    pub min: Point3Dto,
    pub max: Point3Dto,
}

impl StockBoxDto {
    pub fn validate(self) -> Result<(), String> {
        if !self.min.is_finite() || !self.max.is_finite() {
            return Err("stock bounds must contain finite coordinates".to_string());
        }
        if self.max.x - self.min.x <= EPSILON
            || self.max.y - self.min.y <= EPSILON
            || self.max.z - self.min.z <= EPSILON
        {
            return Err("stock must have positive X, Y, and Z dimensions".to_string());
        }
        Ok(())
    }

    pub fn xy_bounds(self) -> Rect2Dto {
        Rect2Dto {
            min: Point2Dto::new(self.min.x, self.min.y),
            max: Point2Dto::new(self.max.x, self.max.y),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WorkCoordinateSystemDto {
    /// WCS origin in model coordinates, expressed in model millimetres.
    pub origin: Point3Dto,
    /// Orthonormal fixed-axis milling frame in model coordinates.
    pub x_axis: [f64; 3],
    pub y_axis: [f64; 3],
    pub z_axis: [f64; 3],
}

impl Default for WorkCoordinateSystemDto {
    fn default() -> Self {
        Self {
            origin: Point3Dto::new(0.0, 0.0, 0.0),
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
            z_axis: [0.0, 0.0, 1.0],
        }
    }
}

impl WorkCoordinateSystemDto {
    pub fn to_model(self, p: Point3Dto) -> Point3Dto {
        Point3Dto::new(
            self.origin.x + p.x * self.x_axis[0] + p.y * self.y_axis[0] + p.z * self.z_axis[0],
            self.origin.y + p.x * self.x_axis[1] + p.y * self.y_axis[1] + p.z * self.z_axis[1],
            self.origin.z + p.x * self.x_axis[2] + p.y * self.y_axis[2] + p.z * self.z_axis[2],
        )
    }

    pub fn from_model(self, p: Point3Dto) -> Point3Dto {
        let d = [
            p.x - self.origin.x,
            p.y - self.origin.y,
            p.z - self.origin.z,
        ];
        let dot = |a: [f64; 3]| (0..3).map(|i| d[i] * a[i]).sum();
        Point3Dto::new(dot(self.x_axis), dot(self.y_axis), dot(self.z_axis))
    }

    /// Tight axis-aligned envelope of a source setup's box in this frame.
    pub fn stock_from(self, source: Self, stock: &StockBoxDto) -> StockBoxDto {
        let mut result = StockBoxDto {
            min: Point3Dto::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            max: Point3Dto::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        };
        for x in [stock.min.x, stock.max.x] {
            for y in [stock.min.y, stock.max.y] {
                for z in [stock.min.z, stock.max.z] {
                    let p = self.from_model(source.to_model(Point3Dto::new(x, y, z)));
                    result.min.x = result.min.x.min(p.x);
                    result.max.x = result.max.x.max(p.x);
                    result.min.y = result.min.y.min(p.y);
                    result.max.y = result.max.y.max(p.y);
                    result.min.z = result.min.z.min(p.z);
                    result.max.z = result.max.z.max(p.z);
                }
            }
        }
        result
    }

    fn validate(self) -> Result<(), String> {
        if !self.origin.is_finite()
            || !self
                .x_axis
                .iter()
                .chain(self.y_axis.iter())
                .chain(self.z_axis.iter())
                .all(|value| value.is_finite())
        {
            return Err("WCS origin and axes must be finite".to_string());
        }
        let norm = |axis: [f64; 3]| axis.iter().map(|v| v * v).sum::<f64>().sqrt();
        let dot = |a: [f64; 3], b: [f64; 3]| {
            a.iter()
                .zip(b)
                .map(|(left, right)| left * right)
                .sum::<f64>()
        };
        for (name, axis) in [("X", self.x_axis), ("Y", self.y_axis), ("Z", self.z_axis)] {
            if (norm(axis) - 1.0).abs() > 1.0e-6 {
                return Err(format!("WCS {name} axis must be normalized"));
            }
        }
        if dot(self.x_axis, self.y_axis).abs() > 1.0e-6
            || dot(self.x_axis, self.z_axis).abs() > 1.0e-6
            || dot(self.y_axis, self.z_axis).abs() > 1.0e-6
        {
            return Err("WCS axes must be mutually perpendicular".to_string());
        }
        let cross_xy = [
            self.x_axis[1] * self.y_axis[2] - self.x_axis[2] * self.y_axis[1],
            self.x_axis[2] * self.y_axis[0] - self.x_axis[0] * self.y_axis[2],
            self.x_axis[0] * self.y_axis[1] - self.x_axis[1] * self.y_axis[0],
        ];
        if dot(cross_xy, self.z_axis) < 0.999_999 {
            return Err("WCS axes must form a right-handed frame".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkOffset {
    #[default]
    G54,
    G55,
    G56,
    G57,
    G58,
    G59,
}

impl WorkOffset {
    pub fn code(self) -> &'static str {
        match self {
            Self::G54 => "G54",
            Self::G55 => "G55",
            Self::G56 => "G56",
            Self::G57 => "G57",
            Self::G58 => "G58",
            Self::G59 => "G59",
        }
    }

    pub fn index(self) -> u8 {
        match self {
            Self::G54 => 0,
            Self::G55 => 1,
            Self::G56 => 2,
            Self::G57 => 3,
            Self::G58 => 4,
            Self::G59 => 5,
        }
    }

    pub fn from_index(index: u8) -> Option<Self> {
        match index {
            0 => Some(Self::G54),
            1 => Some(Self::G55),
            2 => Some(Self::G56),
            3 => Some(Self::G57),
            4 => Some(Self::G58),
            5 => Some(Self::G59),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostDialect {
    #[default]
    Grbl,
    LinuxCnc,
    Fanuc,
    Haas,
    Mitsubishi,
    Mazak,
    Syntec,
    Okuma,
    Heidenhain,
    HermleHeidenhain,
    /// Native SINUMERIK language for Siemens 828D controls.
    Siemens828d,
}

impl PostDialect {
    pub fn requires_machine_retract(self) -> bool {
        matches!(
            self,
            Self::Fanuc
                | Self::Haas
                | Self::Mitsubishi
                | Self::Mazak
                | Self::Syntec
                | Self::Okuma
                | Self::Heidenhain
                | Self::HermleHeidenhain
        )
    }

    pub fn supports_named_tools(self) -> bool {
        matches!(
            self,
            Self::Siemens828d | Self::Heidenhain | Self::HermleHeidenhain
        )
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Grbl => "nc",
            Self::LinuxCnc => "ngc",
            Self::Fanuc | Self::Haas | Self::Mitsubishi | Self::Syntec => "nc",
            Self::Okuma => "min",
            Self::Mazak => "eia",
            Self::Heidenhain | Self::HermleHeidenhain => "h",
            Self::Siemens828d => "mpf",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CamToolCallMode {
    /// Prefer the project-library number, otherwise use the exact name on
    /// controls supporting named calls. Internal database ids are never T ids.
    #[default]
    Automatic,
    Number,
    Name,
}

fn default_siemens_tool_length_offset() -> u32 {
    1
}

/// Physical magazine/changer layout. This is descriptive metadata used for
/// guidance and examples; it must not silently choose machine motion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Siemens828dAtcStyle {
    #[default]
    DoubleArm,
    Umbrella,
    CarouselChain,
    Other,
}

/// Verified responsibility for positioning the spindle before `M6`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Siemens828dToolChangePositioning {
    /// The NC program retracts Z in machine coordinates, then calls `M6`.
    #[default]
    SupaZ,
    /// The machine builder's `M6` PLC/cycle owns all station positioning.
    ControllerManaged,
    /// The NC program retracts Z first, then moves to a verified machine XY
    /// station before calling `M6`.
    SupaZThenXy,
}

/// Machine-specific safety values used by the native SINUMERIK 828D post.
///
/// `SUPA` addresses the machine coordinate system, so a value that is safe on
/// one 828D machine must never be silently assumed safe on another. The
/// profile is optional in persisted projects and the post fails closed when
/// it is absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Siemens828dPostConfigDto {
    /// Informational changer layout. This never selects motion by itself.
    #[serde(default)]
    pub atc_style: Siemens828dAtcStyle,
    /// Explicit, machine-manual-verified positioning behavior.
    #[serde(default)]
    pub tool_change_positioning: Siemens828dToolChangePositioning,
    /// Machine-coordinate Z used by `G0 SUPA Z... D0` before a tool change and
    /// at program end.
    pub supa_retract_z: f64,
    /// Required only for `supa_z_then_xy`; expressed in machine coordinates.
    #[serde(default)]
    pub station_x: Option<f64>,
    /// Required only for `supa_z_then_xy`; expressed in machine coordinates.
    #[serde(default)]
    pub station_y: Option<f64>,
    /// SINUMERIK tool edge / length-offset number activated after `M6`.
    #[serde(default = "default_siemens_tool_length_offset")]
    pub tool_length_offset: u32,
    /// Opt in to `M1` before the second and later tool changes.
    #[serde(default)]
    pub optional_stop_on_tool_change: bool,
    /// Emit a `T...` call for the next tool immediately after the active
    /// tool's `M6`/`D...` blocks. This may move a magazine, so it is disabled
    /// unless the machine profile explicitly allows it.
    #[serde(default)]
    pub preload_next_tool: bool,
    /// Private machine subprogram called immediately before an explicit M5.
    /// Only a bare identifier is accepted, never arbitrary NC text. Its body
    /// is controller-resident and cannot be certified by workpiece simulation.
    #[serde(default)]
    pub spindle_stop_subprogram: Option<String>,
}

impl Default for Siemens828dPostConfigDto {
    fn default() -> Self {
        Self {
            atc_style: Siemens828dAtcStyle::default(),
            tool_change_positioning: Siemens828dToolChangePositioning::default(),
            supa_retract_z: 0.0,
            station_x: None,
            station_y: None,
            tool_length_offset: default_siemens_tool_length_offset(),
            optional_stop_on_tool_change: false,
            preload_next_tool: false,
            spindle_stop_subprogram: None,
        }
    }
}

impl Siemens828dPostConfigDto {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !self.supa_retract_z.is_finite() {
            return Err("Siemens 828D SUPA retract Z must be finite".to_string());
        }
        for (axis, value) in [("X", self.station_x), ("Y", self.station_y)] {
            if value.is_some_and(|coordinate| !coordinate.is_finite()) {
                return Err(format!(
                    "Siemens 828D tool-change station {axis} must be finite"
                ));
            }
        }
        if self.tool_length_offset == 0 || self.tool_length_offset > 999 {
            return Err("Siemens 828D tool length offset must be between D1 and D999".to_string());
        }
        if let Some(name) = &self.spindle_stop_subprogram {
            if !(3..=31).contains(&name.len())
                || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || !name.as_bytes()[0].is_ascii_alphabetic()
                || !name.contains('_')
            {
                return Err("Spindle-stop subprogram must be a 3–31 character ASCII identifier starting with a letter and containing an underscore; no parameters, paths or NC blocks are accepted.".into());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamPostConfigDto {
    #[serde(default)]
    pub dialect: PostDialect,
    #[serde(default)]
    pub program_number: Option<u32>,
    #[serde(default)]
    pub sequence_numbers: bool,
    /// Present only after a user deliberately selects and confirms a native
    /// Siemens machine profile. Older projects deserialize with `None`.
    #[serde(default)]
    pub siemens_828d: Option<Siemens828dPostConfigDto>,
    #[serde(default)]
    pub tool_call_mode: CamToolCallMode,
    /// Machine-coordinate Z used by the fixed-axis brand posts. None is
    /// unknown, not zero. The setup stores the operator's chosen value.
    #[serde(default)]
    pub machine_retract_z: Option<f64>,
}

impl Default for CamPostConfigDto {
    fn default() -> Self {
        Self {
            dialect: PostDialect::Grbl,
            program_number: Some(1001),
            sequence_numbers: false,
            siemens_828d: None,
            tool_call_mode: CamToolCallMode::Automatic,
            machine_retract_z: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CamToolKind {
    FlatEndMill,
    BallEndMill,
    /// Flat end mill with a corner radius; the radius drives the effective
    /// cutting diameter used by the speeds & feeds calculator.
    BullNoseEndMill,
    /// Indexable-insert face/shell mill for large facing passes.
    FaceMill,
    Drill,
    ChamferMill,
    Tap,
    Reamer,
    BoringBar,
    /// Orbital thread-milling tool; never center-cutting, always works in a
    /// pre-machined hole.
    ThreadMill,
    /// Lathe tooling, reserved for the turning workspace; no milling
    /// operation accepts it.
    TurningGeneral,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamToolDto {
    /// Internal identity. Operations reference tools by this id, never by the
    /// machine-facing number or name, so renumbering or renaming a tool never
    /// breaks existing operations.
    pub id: u64,
    /// Machine-facing tool number. Optional because not every control calls
    /// tools numerically: number-based posts (Fanuc/GRBL/LinuxCNC style) fail
    /// closed when it is missing. Siemens native uses the setup's explicit
    /// controller tool-call binding, not this number or the display label.
    #[serde(default)]
    pub number: Option<u32>,
    /// Operator-facing description. Never an implicit controller identifier.
    pub name: String,
    pub kind: CamToolKind,
    pub diameter: f64,
    pub flute_length: f64,
    /// Maximum axial engagement per pass, independent of cutting length/reach.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum_axial_depth: Option<f64>,
    pub overall_length: f64,
    #[serde(default = "default_true")]
    pub center_cutting: bool,
    /// Cutting edge count. Drives chip-load reasoning and is shown in the
    /// library; it never changes motion by itself.
    #[serde(default = "default_flute_count")]
    pub flute_count: u32,
    /// Included point angle for drills/chamfer mills. Legacy drills without
    /// an angle use 118 degrees with a simulation warning.
    #[serde(default)]
    pub point_angle_degrees: Option<f64>,
    /// Corner (nose) radius in mm for flat/bull-nose/face mills; `None`
    /// means a sharp corner. The speeds & feeds calculator uses it for the
    /// effective cutting diameter at a given depth of cut, which matters
    /// most on high-feed tooling with large corner radii.
    #[serde(default)]
    pub corner_radius: Option<f64>,
    /// Explicit chamfered corner on a flat end/face mill (exclusive of radius).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner_chamfer: Option<crate::cutter::CamCornerChamferDto>,
    /// Library cutting defaults captured with the tool. Creating an
    /// operation copies these into the operation, where they remain
    /// independently editable; later library edits never rewrite existing
    /// operations.
    #[serde(default)]
    pub cutting: CuttingParametersDto,
    /// Additional named cutting-data profiles (e.g. per material). The plain
    /// `cutting` field above is the default profile; operation creation lets
    /// the operator pick any profile and copies its values.
    #[serde(default)]
    pub cutting_presets: Vec<CamCuttingPresetDto>,
    /// Planner-step defaults captured with the tool. Creating an operation
    /// copies them into the operation's own step-down / step-over fields when
    /// the operator has not typed a value; the planner only ever reads the
    /// operation's numbers, so later library edits never rewrite motion.
    #[serde(default)]
    pub default_step_down: Option<f64>,
    #[serde(default)]
    pub default_step_over: Option<f64>,
}

/// A named cutting-data profile on a library tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamCuttingPresetDto {
    pub name: String,
    pub cutting: CuttingParametersDto,
}

impl CamToolDto {
    /// Human-facing label for diagnostics: `T<number>` when a machine number
    /// is assigned, otherwise the tool name.
    pub fn label(&self) -> String {
        match self.number {
            Some(number) => format!("T{number}"),
            None => self.name.clone(),
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.id == 0 {
            return Err("CAM tool ids must be non-zero".to_string());
        }
        if let Some(number) = self.number {
            if number == 0 {
                return Err(format!(
                    "tool '{}' tool number must be positive when assigned",
                    self.name
                ));
            }
        }
        if self.name.trim().is_empty() {
            return Err(format!("CAM tool {} must have a name", self.id));
        }
        for (label, value) in [
            ("diameter", self.diameter),
            ("flute length", self.flute_length),
            ("overall length", self.overall_length),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(format!("tool '{}' {label} must be positive", self.name));
            }
        }
        if self.flute_length > self.overall_length {
            return Err(format!(
                "tool '{}' flute length cannot exceed overall length",
                self.name
            ));
        }
        if self
            .maximum_axial_depth
            .is_some_and(|ap| !ap.is_finite() || ap <= 0.0 || ap > self.flute_length)
        {
            return Err(format!(
                "tool '{}' maximum axial depth must be positive and within its cutting length",
                self.name
            ));
        }
        if self.flute_count == 0 || self.flute_count > 12 {
            return Err(format!(
                "tool '{}' flute count must be between 1 and 12",
                self.name
            ));
        }
        if let Some(angle) = self.point_angle_degrees {
            if !angle.is_finite() || !(10.0..=170.0).contains(&angle) {
                return Err(format!(
                    "tool '{}' point angle must be between 10 and 170 degrees",
                    self.name
                ));
            }
        }
        if self.kind == CamToolKind::ChamferMill && self.point_angle_degrees.is_none() {
            return Err(format!(
                "chamfer mill '{}' must declare a point angle",
                self.name
            ));
        }
        if let Some(radius) = self.corner_radius {
            let radius_capable = matches!(
                self.kind,
                CamToolKind::FlatEndMill | CamToolKind::BullNoseEndMill | CamToolKind::FaceMill
            );
            if !radius_capable {
                return Err(format!(
                    "tool '{}' carries a corner radius, which only flat, bull-nose, and face mills support",
                    self.name
                ));
            }
            if !radius.is_finite() || radius <= 0.0 || radius > self.diameter * 0.5 + EPSILON {
                return Err(format!(
                    "tool '{}' corner radius must be positive and no more than half the diameter",
                    self.name
                ));
            }
        }

        if self.kind == CamToolKind::BullNoseEndMill && self.corner_radius.is_none() {
            return Err(format!(
                "bull-nose end mill '{}' must declare a corner radius",
                self.name
            ));
        }
        crate::CutterProfile::new(self.into())
            .map_err(|reason| format!("tool '{}': {reason}", self.name))?;
        self.cutting
            .validate(&format!("tool '{}' library cutting data", self.name))?;
        for preset in &self.cutting_presets {
            if preset.name.trim().is_empty() {
                return Err(format!(
                    "tool '{}' cutting-data profiles must have names",
                    self.name
                ));
            }
            preset.cutting.validate(&format!(
                "tool '{}' cutting-data profile '{}'",
                self.name, preset.name
            ))?;
        }
        let mut seen = std::collections::HashSet::new();
        for preset in &self.cutting_presets {
            if !seen.insert(preset.name.trim().to_string()) {
                return Err(format!(
                    "tool '{}' cutting-data profile names must be unique",
                    self.name
                ));
            }
        }
        for (label, value) in [
            ("default step-down", self.default_step_down),
            ("default step-over", self.default_step_over),
        ] {
            if let Some(value) = value {
                if !value.is_finite() || value <= 0.0 {
                    return Err(format!(
                        "tool '{}' {label} must be positive when set",
                        self.name
                    ));
                }
            }
        }
        if let Some(step_over) = self.default_step_over {
            if step_over > self.diameter {
                return Err(format!(
                    "tool '{}' default step-over cannot exceed the tool diameter",
                    self.name
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpindleDirection {
    Off,
    Clockwise,
    Counterclockwise,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoolantMode {
    #[default]
    Off,
    Mist,
    Flood,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CuttingParametersDto {
    pub spindle_rpm: u32,
    pub feed_xy: f64,
    pub feed_z: f64,
    #[serde(default)]
    pub coolant: CoolantMode,
}

/// Conservative placeholder used when a pre-tool-library project is loaded.
/// Operators are expected to replace these with their own proven values; the
/// library UI never treats them as recommendations.
impl Default for CuttingParametersDto {
    fn default() -> Self {
        Self {
            spindle_rpm: 5_000,
            feed_xy: 500.0,
            feed_z: 150.0,
            coolant: CoolantMode::Off,
        }
    }
}

impl CuttingParametersDto {
    fn validate(self, operation: &str) -> Result<(), String> {
        if self.spindle_rpm == 0 {
            return Err(format!("{operation} spindle speed must be positive"));
        }
        if !self.feed_xy.is_finite() || self.feed_xy <= 0.0 {
            return Err(format!("{operation} cutting feed must be positive"));
        }
        if !self.feed_z.is_finite() || self.feed_z <= 0.0 {
            return Err(format!("{operation} plunge feed must be positive"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContourCompensation {
    /// The stored polyline is already the tool-center path.
    On,
    /// Offset toward the polygon interior by the tool radius.
    Inside,
    /// Offset away from the polygon interior by the tool radius.
    Outside,
    /// Offset to the left of the travel direction by the tool radius — the
    /// only unambiguous side for an OPEN chain (it has no interior).
    Left,
    /// Offset to the right of the travel direction by the tool radius.
    Right,
}

/// Who turns the contour into the tool-center path. In control is the
/// default: the program carries the part contour and the CNC offsets the
/// tool by its own diameter register, so the shop can fine-tune size and
/// swap cutter diameters at the machine without reposting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompensationMode {
    /// The CNC applies the radius offset (posted as G41/G42 activated on the
    /// lead-in move, cancelled with G40 on the lead-out). The programmed
    /// path stays the part contour; the simulator still cuts to the contour
    /// by applying the nominal radius itself.
    #[default]
    InControl,
    /// The planner offsets the path by the tool radius here; the posted
    /// coordinates are already the tool-center path and no machine
    /// compensation is used.
    InSoftware,
}

fn default_contour_lead() -> f64 {
    5.0
}

/// Canned-cycle family of a drill operation. Most cycles expand to explicit
/// neutral motion. Tapping longhand is only allowed when the operation
/// explicitly records a suitable floating holder; equal feed and pitch alone
/// must never be presented as controller-synchronized rigid tapping.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrillCycle {
    /// Single feed to depth, optional dwell, rapid out.
    #[default]
    Drill,
    /// Peck with a small partial retract that stays inside the hole, only
    /// breaking the chip. Requires `peck_depth`; the partial retract distance
    /// is `peck_retract` (default 0.5 mm).
    ChipBreaking,
    /// Peck with a full retract to the retract plane to clear chips.
    /// Requires `peck_depth`.
    DeepHole,
    /// Right-hand tapping: feed in at pitch x rpm, spindle reverse, feed out
    /// at the same nominal pitch feed. Requires `thread_pitch`, a tap tool,
    /// and an explicit floating-holder contract; this is not rigid tapping.
    TappingRight,
    /// Left-hand tapping: the same with both spindle senses swapped.
    TappingLeft,
    /// Feed in, optional dwell, feed back out at `feed_out` (default: the
    /// plunge feed). Requires a reamer.
    Reaming,
    /// Feed in, dwell, feed back out at `feed_out` (default: the plunge
    /// feed). Requires a boring bar.
    Boring,
}

/// Hand of a thread's helix.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadHand {
    /// Standard thread: the groove advances away when turned clockwise viewed
    /// from the thread's entry face.
    #[default]
    Right,
    Left,
}

/// Cutting direction of a milling pass relative to the tool's own rotation.
/// The planner assumes a clockwise spindle (M3); counter-clockwise spindles
/// flip every case and are a documented limitation of this round.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MillingDirection {
    /// Tool rotation matches the feed direction at the contact point. With a
    /// clockwise spindle (M3), remaining material is on the right of travel:
    /// clockwise around an outside profile and counter-clockwise around an
    /// inside profile/thread bore.
    #[default]
    Climb,
    Conventional,
}

/// Row-to-row cutting direction of a facing operation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaceDirection {
    /// Alternate directions row to row (zigzag): fastest, mixes climb and
    /// conventional engagement.
    #[default]
    BothWays,
    /// Every row cuts in the climb direction; the tool repositions above the
    /// feed plane between rows.
    Climb,
    /// Every row cuts in the conventional direction, same repositioning.
    Conventional,
}

/// Where a picked edge/curve chain lives, recorded so an edit session can
/// re-resolve the same geometry instead of dropping to raw coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CamChainSource {
    /// Solid model edges.
    Model,
    /// Sketch curves.
    Sketch,
}

/// Provenance of a contour/chamfer profile picked in the viewport. The
/// planner reads the resolved point list; this reference lets the host
/// re-select the same chain while editing and requires the backend to
/// re-resolve current coordinates transactionally before regeneration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamChainRefDto {
    pub source: CamChainSource,
    /// Stable keys of the picked entities, in chain order.
    pub keys: Vec<String>,
    /// True when the stored path walks the chain opposite to its natural
    /// entity order.
    #[serde(default)]
    pub reversed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamModeledChamferDto {
    #[serde(default)]
    pub additional_width: f64,
}

/// One independent chamfer boundary. Cutting data, tip offset, direction and
/// transfer heights belong to the operation; bevel dimensions and association
/// belong to each chain. The legacy operation fields remain the first chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamChamferChainDto {
    pub path: Vec<Point2Dto>,
    #[serde(default = "default_true")]
    pub closed: bool,
    #[serde(default)]
    pub chain_ref: Option<CamChainRefDto>,
    #[serde(default)]
    pub modeled_chamfer: Option<CamModeledChamferDto>,
    pub top_z: f64,
    pub chamfer_width: f64,
    pub wall_side: ContourCompensation,
}

/// One hole picked as a cylindrical solid face in the viewport: the center in
/// setup XY plus the face's own top/bottom in setup Z, so hole-making
/// operations cut each hole between its true opening and its true depth
/// instead of one operation-wide pair of planes. `axis` (unit length, setup
/// space) is reserved for indexed/5-axis tool orientation; fixed-axis picking
/// only offers ±Z faces today. `face_key` (`bodyId:faceId`) is pure
/// provenance — the planner reads the resolved coordinates, while the host
/// must re-resolve this face before regeneration and re-select it on edit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamHoleDto {
    pub point: Point2Dto,
    pub top_z: f64,
    pub bottom_z: f64,
    pub axis: [f64; 3],
    #[serde(default)]
    pub face_key: Option<String>,
}

fn default_radial_passes() -> u32 {
    1
}

/// Parameters for the original fixed-axis, engagement-limited rougher.
/// Lengths are canonical millimetres; feedrates are millimetres/minute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamAdaptiveParametersDto {
    pub optimal_load: f64,
    pub maximum_stepdown: f64,
    pub minimum_cutting_radius: f64,
    pub radial_stock_to_leave: f64,
    pub axial_stock_to_leave: f64,
    /// Maximum XY cell width of the conservative target envelope.
    pub tolerance: f64,
    pub ramp_angle_degrees: f64,
    pub maximum_ramp_stepdown: f64,
    pub ramp_feed: f64,
    pub linking_feed: f64,
    pub stay_down_distance: f64,
    pub machine_cavities: bool,
}

/// 3D flat-area finishing (Fusion's "Flat"): every horizontal planar
/// target area between top and bottom is finished at its own Z with
/// contour-parallel offset passes. The cutter keeps radial stock from every
/// part surface above the flat and may overhang open flat edges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamFlatParametersDto {
    /// Distance between neighbouring offset passes.
    pub step_over: f64,
    #[serde(default)]
    pub radial_stock_to_leave: f64,
    #[serde(default)]
    pub axial_stock_to_leave: f64,
    /// Maximum XY cell width of the flat-detection grid; walls are placed
    /// from exact target geometry, not the grid.
    pub tolerance: f64,
    #[serde(default)]
    pub direction: MillingDirection,
    /// Longest move between passes that stays at depth instead of retracting.
    pub stay_down_distance: f64,
}

/// Current target/stock geometry captured by the host during explicit
/// regeneration. The planner, simulator, post and WASM use the same snapshot.
/// Coordinates are model-space millimetres, transformed with the setup WCS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamAdaptiveGeometryDto {
    pub targets: Vec<crate::simulation::CamStockMeshDto>,
    /// Modeled raw stock, including the root body of a rest-from-setup chain.
    #[serde(default)]
    pub stock: Option<crate::simulation::CamStockMeshDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CamOperationDto {
    Adaptive3d {
        id: u64,
        name: String,
        #[serde(default = "default_true")]
        enabled: bool,
        tool_id: u64,
        top_z: f64,
        bottom_z: f64,
        clearance_z: f64,
        retract_z: f64,
        feed_height_z: f64,
        cutting: CuttingParametersDto,
        parameters: CamAdaptiveParametersDto,
        #[serde(default)]
        geometry: Option<CamAdaptiveGeometryDto>,
    },
    Flat3d {
        id: u64,
        name: String,
        #[serde(default = "default_true")]
        enabled: bool,
        tool_id: u64,
        top_z: f64,
        bottom_z: f64,
        clearance_z: f64,
        retract_z: f64,
        feed_height_z: f64,
        cutting: CuttingParametersDto,
        parameters: CamFlatParametersDto,
        #[serde(default)]
        geometry: Option<CamAdaptiveGeometryDto>,
    },
    Face {
        id: u64,
        name: String,
        #[serde(default = "default_true")]
        enabled: bool,
        tool_id: u64,
        bounds: Rect2Dto,
        top_z: f64,
        target_z: f64,
        step_over: f64,
        step_down: f64,
        /// Horizontal clearance (mm) the facing plunge keeps from the stock
        /// boundary: the cutter descends at least this far clear of the
        /// material, so the entry never becomes plunge-milling. Operator
        /// field, 5 mm by default.
        #[serde(default = "default_face_safe_distance")]
        safe_distance: f64,
        /// Row direction: zigzag (default) or single-direction climb /
        /// conventional with a reposition above the feed plane between rows.
        #[serde(default)]
        direction: FaceDirection,
        /// Safe travel heights for this operation (setup Z, mm).
        #[serde(default)]
        clearance_z: f64,
        #[serde(default)]
        retract_z: f64,
        /// Feed-engagement plane (setup Z, mm): rapids reach down to this
        /// height, everything below happens at feed rate. Sits between the
        /// cut top and the retract plane.
        #[serde(default)]
        feed_height_z: f64,
        cutting: CuttingParametersDto,
    },
    Contour2d {
        id: u64,
        name: String,
        #[serde(default = "default_true")]
        enabled: bool,
        tool_id: u64,
        path: Vec<Point2Dto>,
        /// False for an open chain of picked edges: the planner never closes
        /// it, and compensation reads left/right of travel instead of
        /// inside/outside (an open chain has no interior). Defaults true so
        /// documents written before open chains existed keep their behavior.
        #[serde(default = "default_true")]
        closed: bool,
        top_z: f64,
        bottom_z: f64,
        step_down: f64,
        compensation: ContourCompensation,
        /// Who offsets the tool radius: the machine (default) or the planner.
        #[serde(default)]
        compensation_mode: CompensationMode,
        /// Physical cutter-center entry/exit lengths (mm) in the setup plane.
        /// A contour always reaches the profile through a straight tangential
        /// lead — it is what lets the tool edge (not the centerline) meet the
        /// wall.
        /// Leads carry no tool-diameter rule; controls that activate radius
        /// compensation on the lead may demand their own minimum run, which
        /// is the control's business, not this document's. Defaults keep
        /// legacy documents plannable.
        #[serde(default = "default_contour_lead")]
        lead_in: f64,
        #[serde(default = "default_contour_lead")]
        lead_out: f64,
        /// Optional physical cutter-center radius (mm) for a horizontal arc
        /// that rounds the end of the straight lead into a tangential meet
        /// with the profile. With machine-side compensation the planner
        /// enlarges the programmed arc by the active tool radius, so the
        /// controller reconstructs this requested center-path radius. `None`
        /// keeps the plain straight lead. Inside-compensated closed loops
        /// keep straight leads on a split wall segment (arc leads into a
        /// pocket are a later round).
        #[serde(default)]
        lead_arc_radius: Option<f64>,
        /// Climb/conventional travel direction. Closed loops are re-wound
        /// around their start point when the stored winding does not match;
        /// open chains reverse (with the compensation side reinterpreted so
        /// the physical tool side never changes).
        #[serde(default)]
        direction: MillingDirection,
        /// Number of radial roughing passes stepping toward the wall, before
        /// the finish pass. 1 (default) means straight to the finish offset.
        #[serde(default = "default_radial_passes")]
        roughing_passes: u32,
        /// Radial step between roughing passes (mm); required when
        /// `roughing_passes` is greater than 1.
        #[serde(default)]
        roughing_step_over: Option<f64>,
        /// Separate finishing pass at the final offset: the roughing passes
        /// stop `finish_allowance` short of the wall and the finish pass
        /// takes that allowance at `finish_feed`.
        #[serde(default)]
        finishing_pass: bool,
        /// Radial stock (mm) the roughing passes leave for the finish pass.
        #[serde(default)]
        finish_allowance: f64,
        /// Feed rate (mm/min) of the finish pass; defaults to the XY feed.
        #[serde(default)]
        finish_feed: Option<f64>,
        /// Repeat the last profile lap once (spring pass): with the finishing
        /// pass enabled it repeats the finish lap, otherwise it repeats the
        /// last roughing lap. Closed loops only — an open chain cannot re-lap
        /// without a return move.
        #[serde(default)]
        spring_pass: bool,
        /// Provenance of a viewport-picked chain. The host must resolve it
        /// against current CAD before a regeneration can be certified.
        #[serde(default)]
        chain_ref: Option<CamChainRefDto>,
        #[serde(default)]
        clearance_z: f64,
        #[serde(default)]
        retract_z: f64,
        /// Feed-engagement plane (setup Z, mm); see the face operation.
        #[serde(default)]
        feed_height_z: f64,
        cutting: CuttingParametersDto,
    },
    Drill {
        id: u64,
        name: String,
        #[serde(default = "default_true")]
        enabled: bool,
        tool_id: u64,
        points: Vec<Point2Dto>,
        top_z: f64,
        bottom_z: f64,
        retract_z: f64,
        #[serde(default)]
        clearance_z: f64,
        /// Feed-engagement plane (setup Z, mm): the tool rapids down to this
        /// height and feeds from here, so air cutting above the part costs
        /// rapid time instead of feed time.
        #[serde(default)]
        feed_height_z: f64,
        /// Viewport-picked holes carrying their own top/bottom (setup Z).
        /// Empty for manual-center operations written before per-hole heights
        /// existed; those cut every point between the operation's top and
        /// bottom. When both picked holes and manual centers exist, each kind
        /// is cut with its own heights.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        holes: Vec<CamHoleDto>,
        /// Drill past the bottom height so the full diameter (the lip, not
        /// the point) clears the hole bottom: the tip travels the point
        /// length plus `breakthrough_depth` beyond the bottom plane. Applies
        /// to the drilling cycle family (drill/chip-breaking/deep-hole);
        /// defaults off so documents written before it existed keep cutting
        /// exactly tip-to-bottom.
        #[serde(default)]
        drill_tip_through: bool,
        /// Extra distance the drill lip passes the bottom height when
        /// tip-through is enabled (setup Z, mm).
        #[serde(default)]
        breakthrough_depth: f64,
        /// Hole-machining cycle family; see `DrillCycle`.
        #[serde(default)]
        cycle: DrillCycle,
        #[serde(default)]
        peck_depth: Option<f64>,
        /// Partial retract distance for `ChipBreaking` (setup Z, mm).
        #[serde(default)]
        peck_retract: Option<f64>,
        /// Thread pitch (mm/rev) for tapping cycles; the in/out feed is
        /// derived as pitch x spindle rpm.
        #[serde(default)]
        thread_pitch: Option<f64>,
        /// Explicit shop contract for longhand tapping. False means the
        /// operation has not established either controller synchronization
        /// or a suitable floating holder and therefore fails closed.
        #[serde(default)]
        floating_tap_holder: bool,
        /// Feed-out rate (mm/min) for reaming/boring; defaults to the plunge
        /// feed when unset.
        #[serde(default)]
        feed_out: Option<f64>,
        #[serde(default)]
        dwell_seconds: f64,
        cutting: CuttingParametersDto,
    },
    Pocket2d {
        id: u64,
        name: String,
        #[serde(default = "default_true")]
        enabled: bool,
        tool_id: u64,
        /// Closed pocket boundary selected by the operator, in setup XY.
        outline: Vec<Point2Dto>,
        /// Provenance of a viewport-picked sketch loop. Manual outlines keep
        /// this empty; associated loops are re-resolved during regeneration.
        #[serde(default)]
        chain_ref: Option<CamChainRefDto>,
        top_z: f64,
        bottom_z: f64,
        step_down: f64,
        step_over: f64,
        /// Climb/conventional direction of the wall finish pass; the zigzag
        /// clearing itself always alternates.
        #[serde(default)]
        direction: MillingDirection,
        #[serde(default)]
        clearance_z: f64,
        #[serde(default)]
        retract_z: f64,
        /// Feed-engagement plane (setup Z, mm); see the face operation.
        #[serde(default)]
        feed_height_z: f64,
        cutting: CuttingParametersDto,
    },
    Chamfer2d {
        id: u64,
        name: String,
        #[serde(default = "default_true")]
        enabled: bool,
        tool_id: u64,
        /// Sharp-edge profile, or the resolved upper rim for a modeled bevel.
        path: Vec<Point2Dto>,
        /// Independent boundaries after the first (legacy) chain. Never
        /// concatenate these points: transfers must retract above stock.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        additional_chains: Vec<CamChamferChainDto>,
        #[serde(default = "default_true")]
        closed: bool,
        /// Modeled chamfers re-resolve the selected upper/lower rim and its
        /// adjacent bevel. None retains explicit sharp-edge width semantics.
        #[serde(default)]
        modeled_chamfer: Option<CamModeledChamferDto>,
        /// Provenance of a viewport-picked sketch loop. Manual profiles keep
        /// this empty; associated loops are re-resolved during regeneration.
        #[serde(default)]
        chain_ref: Option<CamChainRefDto>,
        /// Z of the sharp top edge being chamfered.
        top_z: f64,
        /// Radial width of the 45 degree chamfer leg.
        chamfer_width: f64,
        /// Extra distance the tool tip travels past the chamfer root so the
        /// tip never rubs the corner. Also the radial offset of the tool
        /// axis from the finished profile for a 90 degree tool.
        tip_offset: f64,
        /// Which side of the path the remaining material wall is on.
        wall_side: ContourCompensation,
        /// Climb/conventional travel direction along the profile.
        #[serde(default)]
        direction: MillingDirection,
        #[serde(default)]
        clearance_z: f64,
        #[serde(default)]
        retract_z: f64,
        /// Feed-engagement plane (setup Z, mm); see the face operation.
        #[serde(default)]
        feed_height_z: f64,
        cutting: CuttingParametersDto,
    },
    /// Helical thread milling of pre-machined internal threads — a
    /// standalone 2D-group operation, not a drill canned cycle. The tool
    /// orbits on a helical path advancing one pitch per revolution; radial
    /// stock is removed in `radial_passes` orbital passes from the smallest
    /// orbit out to the full one.
    Thread {
        id: u64,
        name: String,
        #[serde(default = "default_true")]
        enabled: bool,
        tool_id: u64,
        /// Hole centers the threads are milled into, in setup XY.
        points: Vec<Point2Dto>,
        top_z: f64,
        bottom_z: f64,
        /// Viewport-picked holes carrying their own top/bottom (setup Z);
        /// see the drill operation. Empty falls back to the operation-wide
        /// top/bottom for every center.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        holes: Vec<CamHoleDto>,
        /// Thread pitch (mm/rev). The host resolves it from the chosen
        /// designation and stores it explicitly; the planner never derives it.
        pitch: f64,
        /// Groove-root diameter the tool teeth reach (internal major).
        major_diameter: f64,
        /// Pre-machined hole diameter (internal minor); the tool must fit
        /// inside it with the orbit starting at the hole center.
        minor_diameter: f64,
        #[serde(default)]
        hand: ThreadHand,
        #[serde(default)]
        direction: MillingDirection,
        /// Orbital passes, smallest orbit radius first, full radius last.
        #[serde(default = "default_radial_passes")]
        radial_passes: u32,
        /// Radial depth per pass; required with multiple passes.
        #[serde(default)]
        step_over: Option<f64>,
        #[serde(default)]
        clearance_z: f64,
        #[serde(default)]
        retract_z: f64,
        /// Feed-engagement plane (setup Z, mm); see the face operation.
        #[serde(default)]
        feed_height_z: f64,
        cutting: CuttingParametersDto,
    },
}

impl CamOperationDto {
    pub fn chamfer_chains(&self) -> Vec<CamChamferChainDto> {
        match self {
            Self::Chamfer2d {
                path,
                closed,
                chain_ref,
                modeled_chamfer,
                top_z,
                chamfer_width,
                wall_side,
                additional_chains,
                ..
            } => std::iter::once(CamChamferChainDto {
                path: path.clone(),
                closed: *closed,
                chain_ref: chain_ref.clone(),
                modeled_chamfer: modeled_chamfer.clone(),
                top_z: *top_z,
                chamfer_width: *chamfer_width,
                wall_side: *wall_side,
            })
            .chain(additional_chains.iter().cloned())
            .collect(),
            _ => Vec::new(),
        }
    }

    /// Materialize only one boundary with the operation's common settings.
    /// Used for per-chain validation/planning without manufacturing extra ops.
    pub fn with_chamfer_chain(&self, chain: CamChamferChainDto) -> Self {
        let Self::Chamfer2d {
            id,
            name,
            enabled,
            tool_id,
            tip_offset,
            direction,
            clearance_z,
            retract_z,
            feed_height_z,
            cutting,
            ..
        } = self
        else {
            panic!("chamfer chain requires a chamfer operation");
        };
        Self::Chamfer2d {
            id: *id,
            name: name.clone(),
            enabled: *enabled,
            tool_id: *tool_id,
            path: chain.path,
            closed: chain.closed,
            chain_ref: chain.chain_ref,
            modeled_chamfer: chain.modeled_chamfer,
            top_z: chain.top_z,
            chamfer_width: chain.chamfer_width,
            wall_side: chain.wall_side,
            additional_chains: Vec::new(),
            tip_offset: *tip_offset,
            direction: *direction,
            clearance_z: *clearance_z,
            retract_z: *retract_z,
            feed_height_z: *feed_height_z,
            cutting: *cutting,
        }
    }

    pub fn set_chamfer_chains(&mut self, chains: Vec<CamChamferChainDto>) {
        let mut chains = chains.into_iter();
        let first = chains.next().expect("chamfer requires at least one chain");
        *self = self.with_chamfer_chain(first);
        if let Self::Chamfer2d {
            additional_chains, ..
        } = self
        {
            *additional_chains = chains.collect();
        }
    }
    pub fn id(&self) -> u64 {
        match self {
            Self::Adaptive3d { id, .. }
            | Self::Flat3d { id, .. }
            | Self::Face { id, .. }
            | Self::Contour2d { id, .. }
            | Self::Drill { id, .. }
            | Self::Pocket2d { id, .. }
            | Self::Chamfer2d { id, .. }
            | Self::Thread { id, .. } => *id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Adaptive3d { name, .. }
            | Self::Flat3d { name, .. }
            | Self::Face { name, .. }
            | Self::Contour2d { name, .. }
            | Self::Drill { name, .. }
            | Self::Pocket2d { name, .. }
            | Self::Chamfer2d { name, .. }
            | Self::Thread { name, .. } => name,
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            Self::Adaptive3d { enabled, .. }
            | Self::Flat3d { enabled, .. }
            | Self::Face { enabled, .. }
            | Self::Contour2d { enabled, .. }
            | Self::Drill { enabled, .. }
            | Self::Pocket2d { enabled, .. }
            | Self::Chamfer2d { enabled, .. }
            | Self::Thread { enabled, .. } => *enabled,
        }
    }

    /// Load-time leniency uses this to park an invalid operation: disabled
    /// operations are skipped by the planner, the post, and strict
    /// validation until the operator fixes and resumes them.
    pub(crate) fn set_enabled(&mut self, value: bool) {
        match self {
            Self::Adaptive3d { enabled, .. }
            | Self::Flat3d { enabled, .. }
            | Self::Face { enabled, .. }
            | Self::Contour2d { enabled, .. }
            | Self::Drill { enabled, .. }
            | Self::Pocket2d { enabled, .. }
            | Self::Chamfer2d { enabled, .. }
            | Self::Thread { enabled, .. } => *enabled = value,
        }
    }

    /// Mutable (cut top, retract, feed plane) triple for legacy migration.
    pub(crate) fn feed_plane_parts_mut(&mut self) -> (&mut f64, &mut f64, &mut f64) {
        match self {
            Self::Adaptive3d {
                top_z,
                retract_z,
                feed_height_z,
                ..
            }
            | Self::Flat3d {
                top_z,
                retract_z,
                feed_height_z,
                ..
            }
            | Self::Face {
                top_z,
                retract_z,
                feed_height_z,
                ..
            }
            | Self::Contour2d {
                top_z,
                retract_z,
                feed_height_z,
                ..
            }
            | Self::Drill {
                top_z,
                retract_z,
                feed_height_z,
                ..
            }
            | Self::Pocket2d {
                top_z,
                retract_z,
                feed_height_z,
                ..
            }
            | Self::Chamfer2d {
                top_z,
                retract_z,
                feed_height_z,
                ..
            }
            | Self::Thread {
                top_z,
                retract_z,
                feed_height_z,
                ..
            } => (top_z, retract_z, feed_height_z),
        }
    }

    pub fn tool_id(&self) -> u64 {
        match self {
            Self::Adaptive3d { tool_id, .. }
            | Self::Flat3d { tool_id, .. }
            | Self::Face { tool_id, .. }
            | Self::Contour2d { tool_id, .. }
            | Self::Drill { tool_id, .. }
            | Self::Pocket2d { tool_id, .. }
            | Self::Chamfer2d { tool_id, .. }
            | Self::Thread { tool_id, .. } => *tool_id,
        }
    }

    pub fn cutting(&self) -> CuttingParametersDto {
        match self {
            Self::Adaptive3d { cutting, .. }
            | Self::Flat3d { cutting, .. }
            | Self::Face { cutting, .. }
            | Self::Contour2d { cutting, .. }
            | Self::Drill { cutting, .. }
            | Self::Pocket2d { cutting, .. }
            | Self::Chamfer2d { cutting, .. }
            | Self::Thread { cutting, .. } => *cutting,
        }
    }

    /// Per-operation clearance plane (setup Z, mm): safe travel height.
    pub fn clearance_z(&self) -> f64 {
        match self {
            Self::Adaptive3d { clearance_z, .. }
            | Self::Flat3d { clearance_z, .. }
            | Self::Face { clearance_z, .. }
            | Self::Contour2d { clearance_z, .. }
            | Self::Drill { clearance_z, .. }
            | Self::Pocket2d { clearance_z, .. }
            | Self::Chamfer2d { clearance_z, .. }
            | Self::Thread { clearance_z, .. } => *clearance_z,
        }
    }

    /// Per-operation retract plane (setup Z, mm): approach/peck-return height.
    pub fn retract_z(&self) -> f64 {
        match self {
            Self::Adaptive3d { retract_z, .. }
            | Self::Flat3d { retract_z, .. }
            | Self::Face { retract_z, .. }
            | Self::Contour2d { retract_z, .. }
            | Self::Drill { retract_z, .. }
            | Self::Pocket2d { retract_z, .. }
            | Self::Chamfer2d { retract_z, .. }
            | Self::Thread { retract_z, .. } => *retract_z,
        }
    }

    /// Per-operation feed-engagement plane (setup Z, mm): rapids stop here,
    /// everything below runs at feed rate.
    pub fn feed_height_z(&self) -> f64 {
        match self {
            Self::Adaptive3d { feed_height_z, .. }
            | Self::Flat3d { feed_height_z, .. }
            | Self::Face { feed_height_z, .. }
            | Self::Contour2d { feed_height_z, .. }
            | Self::Drill { feed_height_z, .. }
            | Self::Pocket2d { feed_height_z, .. }
            | Self::Chamfer2d { feed_height_z, .. }
            | Self::Thread { feed_height_z, .. } => *feed_height_z,
        }
    }

    pub fn validate(&self, setup: &CamSetupDto, tools: &[CamToolDto]) -> Result<(), String> {
        self.validate_with_tool_checks(setup, tools, true)
    }

    fn validate_with_tool_checks(
        &self,
        setup: &CamSetupDto,
        tools: &[CamToolDto],
        check_tool: bool,
    ) -> Result<(), String> {
        let label = self.name().trim();
        if let Self::Chamfer2d {
            path,
            chain_ref,
            additional_chains,
            ..
        } = self
        {
            if additional_chains.len() >= 64
                || path.len()
                    + additional_chains
                        .iter()
                        .map(|c| c.path.len())
                        .sum::<usize>()
                    > MAX_PATH_POINTS
            {
                return Err(format!("chamfer operation '{label}' exceeds 64 chains or {MAX_PATH_POINTS} total path points"));
            }
            let mut keys = std::collections::HashSet::new();
            for reference in chain_ref.iter().chain(
                additional_chains
                    .iter()
                    .filter_map(|c| c.chain_ref.as_ref()),
            ) {
                for key in &reference.keys {
                    if !keys.insert(key) {
                        return Err(format!("chamfer operation '{label}' selects the same edge more than once; remove the overlapping chain"));
                    }
                }
            }
            if !additional_chains.is_empty() {
                for (i, chain) in self.chamfer_chains().into_iter().enumerate() {
                    self.with_chamfer_chain(chain)
                        .validate_with_tool_checks(setup, tools, check_tool)
                        .map_err(|error| format!("Chain {}: {error}", i + 1))?;
                }
                return Ok(());
            }
        }
        if self.id() == 0 {
            return Err("CAM operation ids must be non-zero".to_string());
        }
        if label.is_empty() {
            return Err(format!("CAM operation {} must have a name", self.id()));
        }
        let tool = tools
            .iter()
            .find(|tool| tool.id == self.tool_id())
            .ok_or_else(|| format!("operation '{label}' references a missing tool"))?;
        self.cutting().validate(label)?;

        let clearance_z = self.clearance_z();
        if !clearance_z.is_finite() || clearance_z <= setup.stock.max.z {
            return Err(format!(
                "operation '{label}' clearance Z must be above the stock"
            ));
        }
        let cut_top = match self {
            Self::Adaptive3d { top_z, .. }
            | Self::Flat3d { top_z, .. }
            | Self::Face { top_z, .. }
            | Self::Contour2d { top_z, .. }
            | Self::Pocket2d { top_z, .. }
            | Self::Chamfer2d { top_z, .. } => *top_z,
            Self::Drill { top_z, holes, .. } | Self::Thread { top_z, holes, .. } => holes
                .iter()
                .fold(*top_z, |highest, hole| highest.max(hole.top_z)),
        };
        let retract_z = self.retract_z();
        if !retract_z.is_finite() || retract_z <= cut_top || retract_z > clearance_z {
            return Err(format!(
                "operation '{label}' retract Z must be above every effective cut/hole top ({cut_top:.3} mm) and no higher than its clearance Z"
            ));
        }

        let feed_height_z = self.feed_height_z();
        if !feed_height_z.is_finite()
            || feed_height_z < cut_top - EPSILON
            || feed_height_z > retract_z + EPSILON
        {
            return Err(format!(
                "operation '{label}' feed height Z must sit between every effective cut/hole top ({cut_top:.3} mm) and the retract plane"
            ));
        }

        let within_z = |value: f64| {
            value.is_finite()
                && value >= setup.stock.min.z - EPSILON
                && value <= setup.stock.max.z + EPSILON
        };
        let within_xy = |point: Point2Dto| {
            point.is_finite()
                && point.x >= setup.stock.min.x - EPSILON
                && point.x <= setup.stock.max.x + EPSILON
                && point.y >= setup.stock.min.y - EPSILON
                && point.y <= setup.stock.max.y + EPSILON
        };

        if check_tool {
            let step = match self {
                Self::Adaptive3d { .. } => None,
                Self::Face { step_down, .. }
                | Self::Contour2d { step_down, .. }
                | Self::Pocket2d { step_down, .. } => Some(*step_down),
                _ => None,
            };
            if let (Some(ap), Some(step)) = (tool.maximum_axial_depth, step) {
                if step > ap + EPSILON {
                    return Err(format!("operation '{label}' stepdown {step:.3} mm exceeds tool {} maximum axial depth {ap:.3} mm", tool.label()));
                }
            }
        }
        match self {
            Self::Flat3d {
                top_z,
                bottom_z,
                parameters,
                ..
            } => {
                if check_tool
                    && (!matches!(
                        tool.kind,
                        CamToolKind::FlatEndMill
                            | CamToolKind::BullNoseEndMill
                            | CamToolKind::FaceMill
                    ) || (tool.kind != CamToolKind::FaceMill && !tool.center_cutting))
                {
                    return Err(format!("flat finishing operation '{label}' requires a face mill or a center-cutting flat or bull-nose end mill"));
                }
                let land = tool.diameter * 0.5 - tool.corner_radius.unwrap_or(0.0);
                if check_tool && land <= EPSILON {
                    return Err(format!(
                        "flat finishing operation '{label}' needs a tool with a flat bottom land"
                    ));
                }
                if !top_z.is_finite() || !within_z(*bottom_z) || *bottom_z >= *top_z - EPSILON {
                    return Err(format!("flat finishing operation '{label}' top must be above bottom, with bottom inside the setup stock"));
                }
                if !parameters.step_over.is_finite()
                    || parameters.step_over <= 0.0
                    || (check_tool && parameters.step_over > 2.0 * land + EPSILON)
                {
                    return Err(format!("flat finishing operation '{label}' stepover must be positive and no wider than the tool's flat land ({:.3} mm)", 2.0 * land));
                }
                if !(parameters.radial_stock_to_leave.is_finite()
                    && parameters.radial_stock_to_leave >= 0.0)
                    || !(parameters.axial_stock_to_leave.is_finite()
                        && parameters.axial_stock_to_leave >= 0.0)
                {
                    return Err(format!("flat finishing operation '{label}' stock to leave must be zero or positive"));
                }
                if !(parameters.tolerance.is_finite()
                    && (0.005..=1.0).contains(&parameters.tolerance))
                {
                    return Err(format!("flat finishing operation '{label}' tolerance must be between 0.005 and 1 mm"));
                }
                if !(parameters.stay_down_distance.is_finite()
                    && parameters.stay_down_distance >= 0.0)
                {
                    return Err(format!("flat finishing operation '{label}' stay-down distance must be zero or positive"));
                }
            }
            Self::Adaptive3d {
                top_z,
                bottom_z,
                parameters,
                ..
            } => {
                if check_tool
                    && (!matches!(
                        tool.kind,
                        CamToolKind::FlatEndMill
                            | CamToolKind::BullNoseEndMill
                            | CamToolKind::FaceMill
                    ) || (tool.kind != CamToolKind::FaceMill && !tool.center_cutting))
                {
                    return Err(format!("high-speed roughing operation '{label}' requires a face mill or a center-cutting flat or bull-nose end mill"));
                }
                if check_tool
                    && tool
                        .corner_radius
                        .is_some_and(|corner| corner >= tool.diameter * 0.5 - EPSILON)
                {
                    return Err(format!("high-speed roughing operation '{label}' needs a nonzero flat land; a full ball nose does not provide the required floor-clearance proof"));
                }

                if !top_z.is_finite() || !within_z(*bottom_z) || *bottom_z >= *top_z - EPSILON {
                    return Err(format!("high-speed roughing operation '{label}' top must be above bottom, with bottom inside the setup stock"));
                }
                if !parameters.maximum_stepdown.is_finite() || parameters.maximum_stepdown <= 0.0 {
                    return Err(format!(
                        "high-speed roughing operation '{label}' stepdown must be positive"
                    ));
                }
                if !parameters.optimal_load.is_finite()
                    || parameters.optimal_load <= 0.0
                    || (check_tool
                        && parameters.optimal_load
                            > tool.diameter
                                * if tool.kind == CamToolKind::FaceMill {
                                    1.0
                                } else {
                                    0.5
                                })
                {
                    return Err(format!("high-speed roughing operation '{label}' optimal load must be positive and no larger than the permitted cutting width (diameter for face mills, radius for end mills)"));
                }
                if !parameters.minimum_cutting_radius.is_finite()
                    || parameters.minimum_cutting_radius <= 0.0
                    || (check_tool && parameters.minimum_cutting_radius > tool.diameter * 0.4)
                {
                    return Err(format!("high-speed roughing operation '{label}' cutting radius must be positive and no larger than 40% of the tool diameter"));
                }
                for (field, value) in [
                    ("radial stock to leave", parameters.radial_stock_to_leave),
                    ("axial stock to leave", parameters.axial_stock_to_leave),
                    ("stay-down distance", parameters.stay_down_distance),
                ] {
                    if !value.is_finite() || value < 0.0 {
                        return Err(format!(
                            "high-speed roughing operation '{label}' {field} must be finite and non-negative"
                        ));
                    }
                }
                for (field, value) in [
                    ("tolerance", parameters.tolerance),
                    ("ramp stepdown", parameters.maximum_ramp_stepdown),
                    ("ramp feed", parameters.ramp_feed),
                    ("linking feed", parameters.linking_feed),
                ] {
                    if !value.is_finite() || value <= 0.0 {
                        return Err(format!(
                            "high-speed roughing operation '{label}' {field} must be finite and positive"
                        ));
                    }
                }
                if !parameters.ramp_angle_degrees.is_finite()
                    || parameters.ramp_angle_degrees <= 0.0
                    || parameters.ramp_angle_degrees > 10.0
                {
                    return Err(format!("high-speed roughing operation '{label}' ramp angle must be greater than zero and no larger than 10 degrees"));
                }
                if parameters.maximum_ramp_stepdown > parameters.maximum_stepdown {
                    return Err(format!("high-speed roughing operation '{label}' ramp stepdown must not exceed roughing stepdown"));
                }
            }
            Self::Face {
                bounds,
                top_z,
                target_z,
                step_over,
                step_down,
                safe_distance,
                ..
            } => {
                if check_tool
                    && !matches!(
                        tool.kind,
                        CamToolKind::FlatEndMill
                            | CamToolKind::BullNoseEndMill
                            | CamToolKind::FaceMill
                    )
                {
                    return Err(format!(
                        "face operation '{label}' needs a flat, bull-nose, or face mill"
                    ));
                }
                bounds.validate(&format!("face operation '{label}' bounds"))?;
                if bounds.min.x < setup.stock.min.x - EPSILON
                    || bounds.min.y < setup.stock.min.y - EPSILON
                    || bounds.max.x > setup.stock.max.x + EPSILON
                    || bounds.max.y > setup.stock.max.y + EPSILON
                {
                    return Err(format!(
                        "face operation '{label}' bounds must lie within stock"
                    ));
                }
                validate_depth_range(label, *top_z, *target_z, *step_down, within_z)?;
                if !step_over.is_finite()
                    || *step_over <= 0.0
                    || (check_tool && *step_over > tool.diameter)
                {
                    return Err(format!(
                        "face operation '{label}' stepover must be positive and no larger than the tool diameter"
                    ));
                }
                if !safe_distance.is_finite() || *safe_distance < 0.0 {
                    return Err(format!(
                        "face operation '{label}' safe distance must be zero or positive"
                    ));
                }
            }
            Self::Contour2d {
                path,
                closed,
                top_z,
                bottom_z,
                step_down,
                compensation,
                lead_in,
                lead_out,
                lead_arc_radius,
                roughing_passes,
                roughing_step_over,
                finishing_pass,
                finish_allowance,
                finish_feed,
                spring_pass,
                ..
            } => {
                if check_tool
                    && (!matches!(
                        tool.kind,
                        CamToolKind::FlatEndMill
                            | CamToolKind::BullNoseEndMill
                            | CamToolKind::BallEndMill
                            | CamToolKind::FaceMill
                    ) || !tool.center_cutting)
                {
                    return Err(format!(
                        "contour operation '{label}' requires a center-cutting milling tool: the entry plunges at the lead start, which the operator places"
                    ));
                }
                if path.len() < 2 || path.len() > MAX_PATH_POINTS {
                    return Err(format!(
                        "contour operation '{label}' needs 2..={MAX_PATH_POINTS} path points"
                    ));
                }
                if !path.iter().copied().all(within_xy) {
                    return Err(format!(
                        "contour operation '{label}' path must lie within stock"
                    ));
                }
                if *closed {
                    if path.len() < 3 {
                        return Err(format!(
                            "contour operation '{label}' closed paths need at least 3 points"
                        ));
                    }
                    if signed_area(path).abs() <= EPSILON {
                        return Err(format!("contour operation '{label}' path has zero area"));
                    }
                    if matches!(
                        compensation,
                        ContourCompensation::Left | ContourCompensation::Right
                    ) {
                        return Err(format!(
                            "contour operation '{label}' closed paths compensate inside/outside; left/right is for open chains"
                        ));
                    }
                } else if matches!(
                    compensation,
                    ContourCompensation::Inside | ContourCompensation::Outside
                ) {
                    return Err(format!(
                        "contour operation '{label}' is an open chain — it has no interior; compensate left/right of travel direction"
                    ));
                }
                if !lead_in.is_finite()
                    || *lead_in <= 0.0
                    || !lead_out.is_finite()
                    || *lead_out <= 0.0
                {
                    return Err(format!(
                        "contour operation '{label}' needs a positive lead-in and lead-out — the tool must reach and leave the profile on a straight tangential move"
                    ));
                }

                if let Some(arc_radius) = lead_arc_radius {
                    if !arc_radius.is_finite() || *arc_radius <= 0.0 {
                        return Err(format!(
                            "contour operation '{label}' lead arc radius must be positive when set"
                        ));
                    }
                }
                if *roughing_passes == 0 || *roughing_passes > 64 {
                    return Err(format!(
                        "contour operation '{label}' roughing passes must be between 1 and 64"
                    ));
                }
                if *roughing_passes > 1 {
                    match roughing_step_over {
                        Some(step)
                            if step.is_finite()
                                && *step > 0.0
                                && (!check_tool || *step <= tool.diameter) => {}
                        _ => {
                            return Err(format!(
                                "contour operation '{label}' with multiple roughing passes needs a radial step-over that is positive and no larger than the tool diameter"
                            ));
                        }
                    }
                }
                if *finishing_pass {
                    if !finish_allowance.is_finite() || *finish_allowance <= 0.0 {
                        return Err(format!(
                            "contour operation '{label}' with a finishing pass needs a positive finish allowance"
                        ));
                    }
                    if let Some(feed) = finish_feed {
                        if !feed.is_finite() || *feed <= 0.0 {
                            return Err(format!(
                                "contour operation '{label}' finish feed must be positive when set"
                            ));
                        }
                    }
                }
                if *spring_pass && !closed {
                    return Err(format!(
                        "contour operation '{label}' spring pass repeats the final profile lap, which only closed loops support — an open chain would need a return move"
                    ));
                }
                if matches!(compensation, ContourCompensation::On)
                    && (*roughing_passes > 1 || *finishing_pass)
                {
                    return Err(format!(
                        "contour operation '{label}' follows the stored centerline (compensation on-path): radial roughing passes and a finishing pass need a material side — pick inside/outside (closed) or left/right (open)"
                    ));
                }
                validate_depth_range(label, *top_z, *bottom_z, *step_down, within_z)?;
            }
            Self::Drill {
                points,
                holes,
                top_z,
                bottom_z,
                cycle,
                peck_depth,
                peck_retract,
                thread_pitch,
                floating_tap_holder,
                feed_out,
                dwell_seconds,
                drill_tip_through,
                breakthrough_depth,
                ..
            } => {
                match cycle {
                    DrillCycle::Drill | DrillCycle::ChipBreaking | DrillCycle::DeepHole => {
                        if check_tool && tool.kind != CamToolKind::Drill && !tool.center_cutting {
                            return Err(format!(
                                "drill operation '{label}' requires a drill or center-cutting tool"
                            ));
                        }
                    }
                    DrillCycle::TappingRight | DrillCycle::TappingLeft => {
                        if check_tool && tool.kind != CamToolKind::Tap {
                            return Err(format!("tapping operation '{label}' requires a tap tool"));
                        }
                    }
                    DrillCycle::Reaming => {
                        if check_tool && tool.kind != CamToolKind::Reamer {
                            return Err(format!(
                                "reaming operation '{label}' requires a reamer tool"
                            ));
                        }
                    }
                    DrillCycle::Boring => {
                        if check_tool && tool.kind != CamToolKind::BoringBar {
                            return Err(format!(
                                "boring operation '{label}' requires a boring bar tool"
                            ));
                        }
                    }
                }
                if points.len() + holes.len() > MAX_PATH_POINTS
                    || (points.is_empty() && holes.is_empty())
                {
                    return Err(format!(
                        "drill operation '{label}' needs 1..={MAX_PATH_POINTS} points"
                    ));
                }
                if !points.iter().copied().all(within_xy)
                    || !holes.iter().all(|hole| within_xy(hole.point))
                {
                    return Err(format!(
                        "drill operation '{label}' points must lie within stock"
                    ));
                }
                for hole in holes {
                    validate_depth_span(label, hole.top_z, hole.bottom_z, within_z, true)?;
                    let axis_len = hole.axis.iter().map(|a| a * a).sum::<f64>().sqrt();
                    if !hole.axis.iter().all(|a| a.is_finite())
                        || (axis_len - 1.0).abs() > 1.0e-3
                        || hole.axis[2].abs() <= 1.0 - 1.0e-3
                    {
                        return Err(format!(
                            "drill operation '{label}' picked holes must have a unit axis parallel to setup Z (fixed-axis planning)"
                        ));
                    }
                }
                if !breakthrough_depth.is_finite() || *breakthrough_depth < 0.0 {
                    return Err(format!(
                        "drill operation '{label}' break-through depth must be zero or positive"
                    ));
                }
                if *drill_tip_through
                    && !matches!(
                        cycle,
                        DrillCycle::Drill | DrillCycle::ChipBreaking | DrillCycle::DeepHole
                    )
                {
                    return Err(format!(
                        "drill operation '{label}' tip-through applies to the drilling cycle family (drill, chip breaking, deep hole)"
                    ));
                }
                validate_depth_span(label, *top_z, *bottom_z, within_z, true)?;
                let pecking = matches!(cycle, DrillCycle::ChipBreaking | DrillCycle::DeepHole);
                if pecking && peck_depth.is_none() {
                    return Err(format!(
                        "drill operation '{label}' pecking cycles require a peck depth"
                    ));
                }
                if !pecking && peck_depth.is_some() {
                    return Err(format!(
                        "drill operation '{label}' only pecking cycles take a peck depth"
                    ));
                }
                if let Some(peck) = peck_depth {
                    if !peck.is_finite() || *peck <= 0.0 {
                        return Err(format!(
                            "drill operation '{label}' peck depth must be positive"
                        ));
                    }
                }
                if let Some(retract) = peck_retract {
                    let valid = retract.is_finite()
                        && *retract > 0.0
                        && peck_depth.is_some_and(|peck| *retract < peck);
                    if *cycle != DrillCycle::ChipBreaking || !valid {
                        return Err(format!(
                            "drill operation '{label}' peck retract only applies to chip breaking and must be positive and smaller than the peck depth"
                        ));
                    }
                }
                match cycle {
                    DrillCycle::TappingRight | DrillCycle::TappingLeft => {
                        let valid =
                            thread_pitch.is_some_and(|pitch| pitch.is_finite() && pitch > 0.0);
                        if !valid {
                            return Err(format!(
                                "tapping operation '{label}' requires a positive thread pitch"
                            ));
                        }
                        if !floating_tap_holder {
                            return Err(format!(
                                "tapping operation '{label}' must explicitly confirm a suitable floating tap holder; ordinary G1 feed/reverse is not rigid tapping"
                            ));
                        }
                    }
                    _ => {
                        if thread_pitch.is_some() {
                            return Err(format!(
                                "drill operation '{label}' only tapping cycles take a thread pitch"
                            ));
                        }
                        if *floating_tap_holder {
                            return Err(format!(
                                "drill operation '{label}' floating tap holder confirmation only applies to tapping cycles"
                            ));
                        }
                    }
                }
                if let Some(out) = feed_out {
                    let feeds_out = matches!(cycle, DrillCycle::Reaming | DrillCycle::Boring);
                    if !feeds_out || !out.is_finite() || *out <= 0.0 {
                        return Err(format!(
                            "drill operation '{label}' feed-out only applies to reaming/boring and must be positive"
                        ));
                    }
                }
                if !dwell_seconds.is_finite() || *dwell_seconds < 0.0 || *dwell_seconds > 60.0 {
                    return Err(format!(
                        "drill operation '{label}' dwell must be between 0 and 60 seconds"
                    ));
                }
            }
            Self::Pocket2d {
                outline,
                top_z,
                bottom_z,
                step_down,
                step_over,
                ..
            } => {
                if check_tool
                    && (!matches!(
                        tool.kind,
                        CamToolKind::FlatEndMill
                            | CamToolKind::BullNoseEndMill
                            | CamToolKind::BallEndMill
                            | CamToolKind::FaceMill
                    ) || !tool.center_cutting)
                {
                    return Err(format!(
                        "pocket operation '{label}' requires a center-cutting milling tool until ramp or helical entries are supported"
                    ));
                }
                if outline.len() < 3 || outline.len() > MAX_PATH_POINTS {
                    return Err(format!(
                        "pocket operation '{label}' needs 3..={MAX_PATH_POINTS} outline points"
                    ));
                }
                if !outline.iter().copied().all(within_xy) {
                    return Err(format!(
                        "pocket operation '{label}' outline must lie within stock"
                    ));
                }
                if signed_area(outline).abs() <= EPSILON {
                    return Err(format!("pocket operation '{label}' outline has zero area"));
                }
                validate_depth_range(label, *top_z, *bottom_z, *step_down, within_z)?;
                if !step_over.is_finite()
                    || *step_over <= 0.0
                    || (check_tool && *step_over > tool.diameter)
                {
                    return Err(format!(
                        "pocket operation '{label}' stepover must be positive and no larger than the tool diameter"
                    ));
                }
            }
            Self::Chamfer2d {
                path,
                closed,
                modeled_chamfer,
                chain_ref,
                top_z,
                chamfer_width,
                tip_offset,
                wall_side,
                ..
            } => {
                if check_tool && tool.kind != CamToolKind::ChamferMill {
                    return Err(format!(
                        "chamfer operation '{label}' requires a chamfer mill"
                    ));
                }
                let point_angle = tool.point_angle_degrees.unwrap_or(90.0);
                if check_tool && (point_angle - 90.0).abs() > 1.0e-6 {
                    return Err(format!(
                        "chamfer operation '{label}' supports 90 degree chamfer mills only"
                    ));
                }
                if matches!(wall_side, ContourCompensation::On) {
                    return Err(format!(
                        "chamfer operation '{label}' must declare which side of the path the material wall is on"
                    ));
                }
                if *closed
                    == matches!(
                        wall_side,
                        ContourCompensation::Left | ContourCompensation::Right
                    )
                {
                    return Err(format!(
                        "chamfer operation '{label}' needs inside/outside material for a closed chain and left/right material for an open chain"
                    ));
                }
                let minimum_points = if *closed { 3 } else { 2 };
                if path.len() < minimum_points || path.len() > MAX_PATH_POINTS {
                    return Err(format!(
                        "chamfer operation '{label}' needs {minimum_points}..={MAX_PATH_POINTS} path points"
                    ));
                }
                if !path.iter().copied().all(within_xy) {
                    return Err(format!(
                        "chamfer operation '{label}' path must lie within stock"
                    ));
                }
                if *closed && signed_area(path).abs() <= EPSILON {
                    return Err(format!("chamfer operation '{label}' path has zero area"));
                }
                if !chamfer_width.is_finite() || *chamfer_width <= 0.0 {
                    return Err(format!(
                        "chamfer operation '{label}' width must be positive"
                    ));
                }
                if let Some(modeled) = modeled_chamfer {
                    if !modeled.additional_width.is_finite()
                        || modeled.additional_width < 0.0
                        || modeled.additional_width >= *chamfer_width
                        || !chain_ref.as_ref().is_some_and(|r| {
                            r.source == CamChainSource::Model && !r.keys.is_empty()
                        })
                    {
                        return Err(format!("chamfer operation '{label}' modeled geometry needs associated model edges and a nonnegative added width"));
                    }
                }
                if !tip_offset.is_finite() || *tip_offset <= 0.0 {
                    return Err(format!(
                        "chamfer operation '{label}' tip offset must be positive"
                    ));
                }
                let tip_depth = chamfer_width + tip_offset;
                if check_tool && tip_depth > tool.diameter * 0.5 + EPSILON {
                    return Err(format!(
                        "chamfer operation '{label}' width plus tip offset exceeds the tool radius"
                    ));
                }
                if !within_z(*top_z) || !within_z(top_z - tip_depth) {
                    return Err(format!(
                        "chamfer operation '{label}' cut depth must stay within the stock"
                    ));
                }
                if check_tool && tip_depth > tool.flute_length + EPSILON {
                    return Err(format!(
                        "chamfer operation '{label}' reaches beyond the tool's flute length"
                    ));
                }
            }
            Self::Thread {
                points,
                holes,
                top_z,
                bottom_z,
                pitch,
                major_diameter,
                minor_diameter,
                radial_passes,
                step_over,
                ..
            } => {
                if check_tool && tool.kind != CamToolKind::ThreadMill {
                    return Err(format!(
                        "thread operation '{label}' requires a thread mill tool"
                    ));
                }
                if points.len() + holes.len() > MAX_PATH_POINTS
                    || (points.is_empty() && holes.is_empty())
                {
                    return Err(format!(
                        "thread operation '{label}' needs 1..={MAX_PATH_POINTS} hole centers"
                    ));
                }
                if !points.iter().copied().all(within_xy)
                    || !holes.iter().all(|hole| within_xy(hole.point))
                {
                    return Err(format!(
                        "thread operation '{label}' hole centers must lie within stock"
                    ));
                }
                for hole in holes {
                    if !within_z(hole.top_z)
                        || !within_z(hole.bottom_z)
                        || hole.bottom_z >= hole.top_z - EPSILON
                    {
                        return Err(format!(
                            "thread operation '{label}' picked holes must descend within the stock"
                        ));
                    }
                }
                if !within_z(*top_z) || !within_z(*bottom_z) || *bottom_z >= *top_z - EPSILON {
                    return Err(format!(
                        "thread operation '{label}' depth range must descend within the stock"
                    ));
                }
                if !pitch.is_finite() || *pitch <= 0.0 {
                    return Err(format!("thread operation '{label}' pitch must be positive"));
                }
                if !major_diameter.is_finite()
                    || !minor_diameter.is_finite()
                    || *minor_diameter <= 0.0
                    || *minor_diameter >= *major_diameter - EPSILON
                {
                    return Err(format!(
                        "thread operation '{label}' needs a positive minor diameter smaller than the major diameter"
                    ));
                }

                if check_tool && tool.diameter >= *minor_diameter - EPSILON {
                    return Err(format!(
                        "thread operation '{label}' tool diameter must be smaller than the {minor_diameter:.3} mm minor diameter"
                    ));
                }

                if check_tool && *top_z - *bottom_z + *pitch > tool.flute_length + EPSILON {
                    return Err(format!(
                        "thread operation '{label}' thread depth plus one pitch of overtravel exceeds the tool's flute length"
                    ));
                }
                if *radial_passes == 0 || *radial_passes > 20 {
                    return Err(format!(
                        "thread operation '{label}' radial passes must be between 1 and 20"
                    ));
                }
                match step_over {
                    Some(step) if *radial_passes > 1 => {
                        if !step.is_finite() || *step <= 0.0 {
                            return Err(format!(
                                "thread operation '{label}' stepover must be positive"
                            ));
                        }
                        let orbit = (major_diameter - tool.diameter) * 0.5;
                        if check_tool && f64::from(*radial_passes - 1) * *step >= orbit - EPSILON {
                            return Err(format!(
                                "thread operation '{label}' stepovers consume the whole orbit radius; add passes or shrink the stepover"
                            ));
                        }
                    }
                    None if *radial_passes > 1 => {
                        return Err(format!(
                            "thread operation '{label}' with multiple radial passes needs a stepover"
                        ));
                    }
                    Some(_) => {
                        return Err(format!(
                            "thread operation '{label}' takes a stepover only with multiple radial passes"
                        ));
                    }
                    None => {}
                }
            }
        }
        Ok(())
    }
}

fn validate_depth_range(
    label: &str,
    top_z: f64,
    bottom_z: f64,
    step_down: f64,
    within_z: impl Fn(f64) -> bool,
) -> Result<(), String> {
    validate_depth_span(label, top_z, bottom_z, within_z, false)?;
    if !step_down.is_finite() || step_down <= 0.0 {
        return Err(format!("operation '{label}' stepdown must be positive"));
    }
    Ok(())
}

/// Top and bottom of a cut, each named in the error so the operator knows
/// which height to fix. `top_in_air` lets a cycle that only starts feeding
/// at its top (drilling) begin above the stock.
fn validate_depth_span(
    label: &str,
    top_z: f64,
    bottom_z: f64,
    within_z: impl Fn(f64) -> bool,
    top_in_air: bool,
) -> Result<(), String> {
    if !within_z(bottom_z) {
        return Err(format!(
            "operation '{label}' bottom height Z{bottom_z:.3} is outside the stock"
        ));
    }
    if !top_z.is_finite() || bottom_z >= top_z - EPSILON {
        return Err(format!(
            "operation '{label}' bottom height Z{bottom_z:.3} must be below its top height Z{top_z:.3}"
        ));
    }
    if !top_in_air && !within_z(top_z) {
        return Err(format!(
            "operation '{label}' top height Z{top_z:.3} is above the stock top"
        ));
    }
    Ok(())
}

pub(crate) fn signed_area(points: &[Point2Dto]) -> f64 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| a.x * b.y - b.x * a.y)
        .sum::<f64>()
        * 0.5
}

/// Per-axis anchor on a bounding box, used when an operator picks a WCS
/// origin from a stock or model box.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoxAnchor {
    #[default]
    Min,
    Center,
    Max,
}

/// Stock geometry family the operator chooses for a setup.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CamStockShape {
    /// Rectangular billet.
    #[default]
    Box,
    /// Cylindrical bar, axis along setup Z.
    Cylinder,
    /// Hexagonal bar (across-flats), axis along setup Z.
    Hex,
    /// A modeled solid body used as the stock, voxelized from its mesh.
    ModelBody,
}

/// A bounding-box face of the model, used to park the model against one
/// stock face instead of centering it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CamStockFace {
    XMin,
    XMax,
    YMin,
    YMax,
    ZMin,
    ZMax,
}

/// How a fixed-size stock holds the model: centered in XY (bottom of the
/// stock at the model's Z floor unless a Z face is chosen), or pushed
/// against one named model-box face with an explicit gap.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CamStockPlacementDto {
    #[serde(default = "default_true")]
    pub center: bool,
    #[serde(default)]
    pub face: Option<CamStockFace>,
    #[serde(default)]
    pub offset: f64,
}

impl Default for CamStockPlacementDto {
    fn default() -> Self {
        Self {
            center: true,
            face: None,
            offset: 0.0,
        }
    }
}

/// Per-face allowances when stock grows out of the model bounding box.
/// Cylinder/hex shapes consume `x_min..y_max` as the radial allowance and
/// `z_min`/`z_max` as the axial allowances.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct CamStockOffsetsDto {
    #[serde(default)]
    pub x_min: f64,
    #[serde(default)]
    pub x_max: f64,
    #[serde(default)]
    pub y_min: f64,
    #[serde(default)]
    pub y_max: f64,
    #[serde(default)]
    pub z_min: f64,
    #[serde(default)]
    pub z_max: f64,
}

/// How the operator defines the stock. Resolution to concrete geometry
/// happens where the model scene is available (the workspace host); the
/// resolved envelope and shape are persisted on the setup.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum CamStockSpecDto {
    /// Fixed-size stock; the model is placed inside it.
    Fixed {
        shape: CamStockShape,
        /// Box: full XYZ size. Cylinder: X = diameter, Z = height.
        /// Hex: X = across-flats, Z = height.
        size: Point3Dto,
        #[serde(default)]
        placement: CamStockPlacementDto,
    },
    /// Stock grown from the model bounding box by per-face allowances.
    FromModel {
        shape: CamStockShape,
        #[serde(default)]
        offsets: CamStockOffsetsDto,
    },
    /// Continue from the remaining stock of an earlier setup that shares
    /// this setup's WCS (same clamping, second operation group).
    RestFromSetup { setup_id: u64 },
    /// A modeled body used as the stock solid.
    ModelBody { body_id: u64 },
    /// Legacy documents predate the spec; the resolved box in `stock` is
    /// authoritative for them.
    #[default]
    LegacyBox,
}

/// Resolved stock geometry in setup coordinates, persisted so the planner,
/// simulator, and viewport never re-derive it behind the operator's back.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum CamResolvedStockDto {
    /// Rectangular billet filling the `stock` box.
    #[default]
    Box,
    /// Cylinder along setup Z within the `stock` Z range.
    Cylinder { center: Point2Dto, radius: f64 },
    /// Hexagonal bar along setup Z within the `stock` Z range.
    Hex {
        center: Point2Dto,
        across_flats: f64,
    },
    /// Remaining stock inherited from another setup's simulation. The
    /// `stock` box equals the source setup's envelope.
    Rest { source_setup_id: u64 },
    /// A modeled body, voxelized at simulation time from the mesh the host
    /// supplies in the simulation request.
    ModelBody { body_id: u64 },
}

/// Records how the operator chose the WCS origin. The resolved frame stays in
/// `CamSetupDto::wcs`; this provenance lets the workspace re-resolve the
/// origin on demand without any hidden recomputation behind the operator's
/// back.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum WcsOriginSpecDto {
    /// Raw model-space coordinates entered by the operator.
    #[default]
    Explicit,
    /// A corner/edge/center point of the operator-defined stock box.
    StockBoxPoint {
        x: BoxAnchor,
        y: BoxAnchor,
        z: BoxAnchor,
    },
    /// A point on the bounding box of the setup's selected model bodies.
    ModelBoxPoint {
        x: BoxAnchor,
        y: BoxAnchor,
        z: BoxAnchor,
    },
    /// A point entity drawn earlier in a sketch.
    SketchPoint { sketch: String, entity_id: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamSetupDto {
    /// None means generic programming, not a default or commissioned machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<crate::machine::CamMachineAssignmentDto>,
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub wcs: WorkCoordinateSystemDto,
    /// How the operator picked the WCS origin. Display and re-resolution
    /// metadata only; planners always use the resolved `wcs` frame.
    #[serde(default)]
    pub wcs_origin: WcsOriginSpecDto,
    /// First work offset this setup posts with. `G54` is the conventional
    /// first fixture offset on most controls.
    #[serde(default)]
    pub work_offset: WorkOffset,
    /// How many consecutive work offsets the posted program repeats the
    /// toolpath with (`G54`, `G55`, ... from `work_offset`). One means the
    /// parts are machined one at a time in a single clamping position.
    #[serde(default = "one_u8")]
    pub work_offset_count: u8,
    /// How the operator defined the stock, kept for re-editing.
    #[serde(default)]
    pub stock_spec: CamStockSpecDto,
    /// Resolved stock shape the planner, simulator, and viewport consume.
    /// The Z extent always comes from `stock`.
    #[serde(default)]
    pub resolved_stock: CamResolvedStockDto,
    pub stock: StockBoxDto,
    /// The operator-entered stock box in model coordinates, kept so the setup
    /// dialog can re-edit and re-anchor it. `stock` (setup coordinates)
    /// remains the value planners and simulators consume.
    #[serde(default)]
    pub stock_model_box: Option<StockBoxDto>,
    #[serde(default)]
    pub body_ids: Vec<BodyId>,
    /// Pre-per-operation documents stored one clearance plane on the setup.
    /// Captured only to seed operation heights during migration; never
    /// serialized back out.
    #[serde(default, rename = "clearance_z", skip_serializing)]
    pub legacy_clearance_z: Option<f64>,
    #[serde(default, rename = "retract_z", skip_serializing)]
    pub legacy_retract_z: Option<f64>,
    #[serde(default)]
    pub operations: Vec<CamOperationDto>,
}

impl CamSetupDto {
    /// Consecutive work offsets the posted program repeats with, starting at
    /// `work_offset`.
    pub fn work_offsets(&self) -> Vec<WorkOffset> {
        (0..self.work_offset_count)
            .filter_map(|step| WorkOffset::from_index(self.work_offset.index() + step))
            .collect()
    }

    fn validate(&self, tools: &[CamToolDto], check_tool: bool) -> Result<(), String> {
        self.validate_structure()?;
        for operation in &self.operations {
            if operation.enabled() {
                operation.validate_with_tool_checks(self, tools, check_tool)?;
            }
        }
        Ok(())
    }

    /// Everything except the operation checks. Load-time leniency reports
    /// structural issues as warnings instead of blocking the open.
    pub(crate) fn validate_structure(&self) -> Result<(), String> {
        if let Some(machine) = &self.machine {
            machine.validate()?;
        }
        if self.id == 0 {
            return Err("CAM setup ids must be non-zero".to_string());
        }
        if self.name.trim().is_empty() {
            return Err(format!("CAM setup {} must have a name", self.id));
        }
        self.wcs.validate()?;
        if let WcsOriginSpecDto::SketchPoint { sketch, .. } = &self.wcs_origin {
            if sketch.trim().is_empty() {
                return Err(format!(
                    "setup '{}' WCS sketch-point reference must name a sketch",
                    self.name
                ));
            }
        }
        if self.work_offset_count == 0 || self.work_offset.index() + self.work_offset_count > 6 {
            return Err(format!(
                "setup '{}' work offsets must stay within G54..=G59",
                self.name
            ));
        }
        if let Some(model_box) = &self.stock_model_box {
            model_box.validate()?;
        }
        self.stock.validate()?;
        self.validate_stock_shape()?;
        if self.operations.len() > MAX_OPERATIONS_PER_SETUP {
            return Err(format!(
                "setup '{}' can contain at most {MAX_OPERATIONS_PER_SETUP} operations",
                self.name
            ));
        }
        let mut body_ids = HashSet::new();
        for id in &self.body_ids {
            if id.0 == 0 || !body_ids.insert(*id) {
                return Err(format!(
                    "setup '{}' contains a duplicate or zero body id",
                    self.name
                ));
            }
        }
        Ok(())
    }

    /// The operator-facing stock spec and the persisted resolved geometry
    /// must tell the same story; the host resolves one from the other, and a
    /// disagreement means a hand-edited or corrupted document.
    fn validate_stock_shape(&self) -> Result<(), String> {
        let consistent = match (&self.stock_spec, &self.resolved_stock) {
            (CamStockSpecDto::LegacyBox, CamResolvedStockDto::Box) => true,
            (
                CamStockSpecDto::Fixed {
                    shape,
                    size,
                    placement,
                },
                resolved,
            ) => {
                let shape_matches = matches!(
                    (shape, resolved),
                    (CamStockShape::Box, CamResolvedStockDto::Box)
                        | (
                            CamStockShape::Cylinder,
                            CamResolvedStockDto::Cylinder { .. }
                        )
                        | (CamStockShape::Hex, CamResolvedStockDto::Hex { .. })
                );
                if !shape_matches || !size.is_finite() || size.x <= 0.0 || size.z <= 0.0 {
                    return Err(format!(
                        "setup '{}' fixed stock needs a box/cylinder/hex shape with positive size",
                        self.name
                    ));
                }
                if matches!(shape, CamStockShape::Box) && size.y <= 0.0 {
                    return Err(format!(
                        "setup '{}' fixed box stock needs a positive Y size",
                        self.name
                    ));
                }
                if !placement.offset.is_finite() || (!placement.center && placement.face.is_none())
                {
                    return Err(format!(
                        "setup '{}' fixed stock placement must center the model or name a face",
                        self.name
                    ));
                }
                true
            }
            (CamStockSpecDto::FromModel { shape, offsets }, resolved) => {
                let shape_matches = matches!(
                    (shape, resolved),
                    (CamStockShape::Box, CamResolvedStockDto::Box)
                        | (
                            CamStockShape::Cylinder,
                            CamResolvedStockDto::Cylinder { .. }
                        )
                        | (CamStockShape::Hex, CamResolvedStockDto::Hex { .. })
                );
                if !shape_matches {
                    return Err(format!(
                        "setup '{}' model-grown stock needs a box/cylinder/hex shape",
                        self.name
                    ));
                }
                for value in [
                    offsets.x_min,
                    offsets.x_max,
                    offsets.y_min,
                    offsets.y_max,
                    offsets.z_min,
                    offsets.z_max,
                ] {
                    if !value.is_finite() || value < 0.0 {
                        return Err(format!(
                            "setup '{}' stock allowances must be finite and non-negative",
                            self.name
                        ));
                    }
                }
                true
            }
            (
                CamStockSpecDto::RestFromSetup { setup_id },
                CamResolvedStockDto::Rest { source_setup_id },
            ) => {
                if setup_id != source_setup_id {
                    return Err(format!(
                        "setup '{}' rest-stock spec and resolved source disagree",
                        self.name
                    ));
                }
                if *setup_id == self.id {
                    return Err(format!(
                        "setup '{}' cannot inherit remaining stock from itself",
                        self.name
                    ));
                }
                true
            }
            (
                CamStockSpecDto::ModelBody { body_id },
                CamResolvedStockDto::ModelBody {
                    body_id: resolved_body,
                },
            ) => {
                if body_id != resolved_body || *body_id == 0 {
                    return Err(format!(
                        "setup '{}' model-body stock must reference a non-zero body",
                        self.name
                    ));
                }
                true
            }
            _ => false,
        };
        if !consistent {
            return Err(format!(
                "setup '{}' stock definition and resolved stock shape disagree",
                self.name
            ));
        }

        match &self.resolved_stock {
            CamResolvedStockDto::Cylinder { center, radius } => {
                if !center.is_finite() || !radius.is_finite() || *radius <= 0.0 {
                    return Err(format!(
                        "setup '{}' cylinder stock needs a finite center and positive radius",
                        self.name
                    ));
                }
                if center.x - radius < self.stock.min.x - EPSILON
                    || center.x + radius > self.stock.max.x + EPSILON
                    || center.y - radius < self.stock.min.y - EPSILON
                    || center.y + radius > self.stock.max.y + EPSILON
                {
                    return Err(format!(
                        "setup '{}' cylinder stock must lie inside the stock envelope",
                        self.name
                    ));
                }
            }
            CamResolvedStockDto::Hex {
                center,
                across_flats,
            } => {
                if !center.is_finite() || !across_flats.is_finite() || *across_flats <= 0.0 {
                    return Err(format!(
                        "setup '{}' hex stock needs a finite center and positive across-flats size",
                        self.name
                    ));
                }

                let half = across_flats / 2.0;
                let vertex = across_flats / 3.0_f64.sqrt();
                if center.x - half < self.stock.min.x - EPSILON
                    || center.x + half > self.stock.max.x + EPSILON
                    || center.y - vertex < self.stock.min.y - EPSILON
                    || center.y + vertex > self.stock.max.y + EPSILON
                {
                    return Err(format!(
                        "setup '{}' hex stock must lie inside the stock envelope",
                        self.name
                    ));
                }
            }
            CamResolvedStockDto::Box
            | CamResolvedStockDto::Rest { .. }
            | CamResolvedStockDto::ModelBody { .. } => {}
        }
        Ok(())
    }
}

/// Dependency fingerprints captured only after the planner successfully
/// regenerates an operation. Toolpath motion remains derived (and is not
/// persisted); this stamp proves that the derived path was reviewed against
/// the current manufacturing inputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CamToolpathGenerationDto {
    pub operation_id: u64,
    pub planner_revision: u32,
    pub model_fingerprint: String,
    pub setup_fingerprint: String,
    pub operation_fingerprint: String,
    pub tool_fingerprint: String,
    pub upstream_fingerprint: String,
    /// None denotes the legacy whole-prefix operation fingerprint. A valid
    /// legacy stamp may be translated only against its unchanged old inputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order_dependencies: Option<CamToolpathOrderDependenciesDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CamToolpathOrderDependenciesDto {
    pub rules_revision: u32,
    pub stock_height_fingerprint: String,
    pub predrill_fingerprint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CamToolpathStateDto {
    Current,
    NeverGenerated,
    Stale,
    Invalid,
}

/// Live comparison between a saved generation stamp and the current CAD/CAM
/// dependencies. Reasons are deliberately actionable and safe to show in the
/// setup browser, plan warnings, and posting errors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CamToolpathStatusDto {
    pub setup_id: u64,
    pub operation_id: u64,
    pub state: CamToolpathStateDto,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
}

/// Associative plane used by one operation height. These values mirror the
/// programming UI but remain engine-owned so regeneration can resolve them
/// against current CAD instead of trusting the last baked Z coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CamHeightReferenceDto {
    ModelTop,
    ModelBottom,
    StockTop,
    StockBottom,
    Origin,
    HoleTop,
    HoleBottom,
    Bottom,
    Top,
    Feed,
    Retract,
    Selection,
    Geometry,
}

/// Stable topology/sketch identity for one independently picked height.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CamHeightGeometryDto {
    Face {
        body_id: u64,
        key: String,
    },
    Edge {
        body_id: u64,
        key: String,
    },
    Vertex {
        body_id: u64,
        key: String,
        end: bool,
    },
    SketchPoint {
        sketch: String,
        entity_id: u64,
    },
    SketchLine {
        sketch: String,
        entity_id: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamHeightExpressionDto {
    pub reference: CamHeightReferenceDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<CamHeightGeometryDto>,
    /// Signed canonical-millimetre offset from `reference`.
    pub offset: f64,
}

/// Persisted height intent for an operation. Bottom is absent for operations
/// such as chamfer that derive their cut depth from width/tool geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamOperationHeightExpressionsDto {
    pub operation_id: u64,
    pub clearance: CamHeightExpressionDto,
    pub retract: CamHeightExpressionDto,
    pub feed: CamHeightExpressionDto,
    pub top: CamHeightExpressionDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<CamHeightExpressionDto>,
}

impl CamOperationHeightExpressionsDto {
    fn validate_structure(&self, needs_bottom: bool) -> Result<(), String> {
        let rank = |reference: CamHeightReferenceDto| match reference {
            CamHeightReferenceDto::Bottom => 1,
            CamHeightReferenceDto::Top => 2,
            CamHeightReferenceDto::Feed => 3,
            CamHeightReferenceDto::Retract => 4,
            _ => 0,
        };
        let check = |name: &str, expression: &CamHeightExpressionDto, field_rank: u8| {
            if !expression.offset.is_finite() {
                return Err(format!("{name} height offset must be finite"));
            }
            if (expression.reference == CamHeightReferenceDto::Geometry)
                != expression.geometry.is_some()
            {
                return Err(format!(
                    "{name} height geometry must accompany a Geometry reference"
                ));
            }
            if rank(expression.reference) >= field_rank {
                return Err(format!(
                    "{name} height may only reference a lower operation height"
                ));
            }
            Ok(())
        };
        match (&self.bottom, needs_bottom) {
            (Some(expression), true) => check("bottom", expression, 1)?,
            (None, false) => {}
            (Some(_), false) => return Err("this operation does not take a bottom height".into()),
            (None, true) => return Err("associative bottom height is missing".into()),
        }
        check("top", &self.top, 2)?;
        check("feed", &self.feed, 3)?;
        check("retract", &self.retract, 4)?;
        check("clearance", &self.clearance, 5)
    }

    pub fn validate_for_operation(&self, operation: &CamOperationDto) -> Result<(), String> {
        self.validate_structure(!matches!(operation, CamOperationDto::Chamfer2d { .. }))?;
        let expressions = [&self.clearance, &self.retract, &self.feed, &self.top]
            .into_iter()
            .chain(self.bottom.iter());
        for expression in expressions {
            match expression.reference {
                CamHeightReferenceDto::HoleTop | CamHeightReferenceDto::HoleBottom => {
                    let has_associative_holes = match operation {
                        CamOperationDto::Drill { holes, .. }
                        | CamOperationDto::Thread { holes, .. } => {
                            holes.iter().any(|hole| hole.face_key.is_some())
                        }
                        _ => false,
                    };
                    if !has_associative_holes {
                        return Err(
                            "picked-hole height requires an associated cylindrical face".into()
                        );
                    }
                }
                CamHeightReferenceDto::Selection => {
                    let reference = match operation {
                        CamOperationDto::Contour2d { chain_ref, .. }
                        | CamOperationDto::Pocket2d { chain_ref, .. }
                        | CamOperationDto::Chamfer2d { chain_ref, .. } => chain_ref.as_ref(),
                        _ => None,
                    };
                    if !reference.is_some_and(|reference| !reference.keys.is_empty()) {
                        return Err(
                            "Selection height requires associated edge or sketch geometry".into(),
                        );
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamDocumentDto {
    #[serde(default)]
    pub setups: Vec<CamSetupDto>,
    #[serde(default)]
    pub active_setup_id: Option<u64>,
    #[serde(default)]
    pub tools: Vec<CamToolDto>,
    /// Successful per-operation regeneration stamps. Older projects omit the
    /// field and therefore open with every enabled path marked as needing an
    /// explicit first regeneration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub toolpath_generations: Vec<CamToolpathGenerationDto>,
    /// Associative height intent keyed by globally unique operation id.
    /// Legacy/manual operations omit it and retain explicit absolute Z values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub height_expressions: Vec<CamOperationHeightExpressionsDto>,
    /// Shared lead/link intent keyed by operation id; absent means legacy.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linking: Vec<crate::linking::CamLinkingDto>,
    /// Non-fatal issues found when the document was loaded. A project file
    /// must always open: operations that fail validation are disabled (the
    /// planner and post skip them) and carry a warning here until the
    /// operator fixes and re-saves them. Recomputed on every validated
    /// write, so fixed entries clear immediately.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub load_warnings: Vec<CamLoadWarningDto>,
    /// Operator-facing unit system. Persisted geometry and planned motion
    /// remain canonical millimetres; posts convert controller words when
    /// this is inches.
    #[serde(default)]
    pub units: CamUnits,
    /// Post settings remembered from the last export. Posting always asks
    /// again; these only pre-fill the dialog so the machine profile choice
    /// is an explicit, at-export decision.
    #[serde(default)]
    pub post_defaults: CamPostConfigDto,
    #[serde(default = "first_id")]
    pub next_setup_id: u64,
    #[serde(default = "first_id")]
    pub next_operation_id: u64,
    #[serde(default = "first_id")]
    pub next_tool_id: u64,
}

/// A non-fatal CAM document issue found at load time. `setup_id` /
/// `operation_id` locate the row the host badges; both `None` means a
/// document/tool-level issue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CamLoadWarningDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<u64>,
    /// Human-readable, actionable description (the validation message).
    pub message: String,
}

impl Default for CamDocumentDto {
    fn default() -> Self {
        Self {
            setups: Vec::new(),
            active_setup_id: None,
            tools: Vec::new(),
            toolpath_generations: Vec::new(),
            height_expressions: Vec::new(),
            linking: Vec::new(),
            load_warnings: Vec::new(),
            units: CamUnits::Millimeters,
            post_defaults: CamPostConfigDto::default(),
            next_setup_id: 1,
            next_operation_id: 1,
            next_tool_id: 1,
        }
    }
}

impl CamDocumentDto {
    /// Seed per-operation safe heights on documents saved before heights
    /// moved from the setup onto each operation. The legacy setup planes are
    /// copied into every operation still at zero (zero is never a valid
    /// plane) and then cleared; validation reports any operation left
    /// without heights, so nothing silently plans with a zero plane.
    pub fn migrate_legacy(&mut self) {
        for setup in &mut self.setups {
            let legacy_clearance = setup.legacy_clearance_z.take();
            let legacy_retract = setup.legacy_retract_z.take();
            let has_legacy_heights = legacy_clearance.is_some() || legacy_retract.is_some();
            for operation in &mut setup.operations {
                if has_legacy_heights {
                    match operation {
                        CamOperationDto::Adaptive3d {
                            clearance_z,
                            retract_z,
                            ..
                        }
                        | CamOperationDto::Flat3d {
                            clearance_z,
                            retract_z,
                            ..
                        }
                        | CamOperationDto::Face {
                            clearance_z,
                            retract_z,
                            ..
                        }
                        | CamOperationDto::Contour2d {
                            clearance_z,
                            retract_z,
                            ..
                        }
                        | CamOperationDto::Pocket2d {
                            clearance_z,
                            retract_z,
                            ..
                        }
                        | CamOperationDto::Chamfer2d {
                            clearance_z,
                            retract_z,
                            ..
                        }
                        | CamOperationDto::Thread {
                            clearance_z,
                            retract_z,
                            ..
                        } => {
                            if *clearance_z == 0.0 {
                                if let Some(value) = legacy_clearance {
                                    *clearance_z = value;
                                }
                            }
                            if *retract_z == 0.0 {
                                if let Some(value) = legacy_retract {
                                    *retract_z = value;
                                }
                            }
                        }

                        CamOperationDto::Drill { clearance_z, .. } => {
                            if *clearance_z == 0.0 {
                                if let Some(value) = legacy_clearance {
                                    *clearance_z = value;
                                }
                            }
                        }
                    }
                }

                let (top_z, retract_z, feed_height_z) = operation.feed_plane_parts_mut();
                if top_z.is_finite() && retract_z.is_finite() && *top_z <= *retract_z {
                    let clamped = feed_height_z.clamp(*top_z, *retract_z);
                    *feed_height_z = if clamped.is_finite() { clamped } else { *top_z };
                }
            }
        }

        let operation_ids = self
            .setups
            .iter()
            .flat_map(|setup| setup.operations.iter().map(CamOperationDto::id))
            .collect::<HashSet<_>>();
        let mut seen = HashSet::new();
        self.toolpath_generations.retain(|generation| {
            operation_ids.contains(&generation.operation_id)
                && seen.insert(generation.operation_id)
                && generation.planner_revision > 0
                && valid_toolpath_fingerprint(&generation.model_fingerprint)
                && valid_toolpath_fingerprint(&generation.setup_fingerprint)
                && valid_toolpath_fingerprint(&generation.operation_fingerprint)
                && valid_toolpath_fingerprint(&generation.tool_fingerprint)
                && valid_toolpath_fingerprint(&generation.upstream_fingerprint)
                && generation.order_dependencies.as_ref().is_none_or(|order| {
                    order.rules_revision > 0
                        && valid_toolpath_fingerprint(&order.stock_height_fingerprint)
                        && valid_toolpath_fingerprint(&order.predrill_fingerprint)
                })
        });
    }

    /// Load-time leniency: a project file must ALWAYS open. Migrations and
    /// safe auto-fixes run first (a stale active-setup pointer selects the
    /// first setup; id counters move past every saved id); operations that
    /// have malformed parameters are parked. Tool-dependent mismatches stay
    /// enabled and explicitly invalid, so reopening cannot silently skip a
    /// required machining step. Every remaining issue lands in
    /// `load_warnings` for the host to badge. Warnings clear on the
    /// next validated write (`refresh_load_warnings`).
    pub fn soften_for_load(&mut self) {
        self.migrate_legacy();
        if !self.setups.is_empty()
            && self
                .active_setup_id
                .is_none_or(|active| !self.setups.iter().any(|setup| setup.id == active))
        {
            self.active_setup_id = Some(self.setups[0].id);
        }
        let max_setup_id = self.setups.iter().map(|setup| setup.id).max().unwrap_or(0);
        let max_operation_id = self
            .setups
            .iter()
            .flat_map(|setup| setup.operations.iter().map(|operation| operation.id()))
            .max()
            .unwrap_or(0);
        let max_tool_id = self.tools.iter().map(|tool| tool.id).max().unwrap_or(0);

        self.next_setup_id = self
            .next_setup_id
            .max(max_setup_id.saturating_add(1))
            .max(1);
        self.next_operation_id = self
            .next_operation_id
            .max(max_operation_id.saturating_add(1))
            .max(1);
        self.next_tool_id = self.next_tool_id.max(max_tool_id.saturating_add(1)).max(1);
        let tools = self.tools.clone();
        for setup in &mut self.setups {
            let failing: Vec<usize> = setup
                .operations
                .iter()
                .enumerate()
                .filter(|(_, operation)| {
                    operation
                        .validate_with_tool_checks(setup, &tools, false)
                        .is_err()
                })
                .map(|(index, _)| index)
                .collect();
            for index in failing {
                setup.operations[index].set_enabled(false);
            }
        }

        let mut seen_heights = HashSet::new();
        let invalid_height_operations = self
            .height_expressions
            .iter()
            .filter_map(|expressions| {
                let operation = self
                    .setups
                    .iter()
                    .flat_map(|setup| setup.operations.iter())
                    .find(|operation| operation.id() == expressions.operation_id);
                let invalid = operation.is_none()
                    || !seen_heights.insert(expressions.operation_id)
                    || operation.is_some_and(|operation| {
                        expressions.validate_for_operation(operation).is_err()
                    });
                invalid.then_some(expressions.operation_id)
            })
            .collect::<HashSet<_>>();
        self.toolpath_generations
            .retain(|generation| !invalid_height_operations.contains(&generation.operation_id));
        let mut seen_linking = HashSet::new();
        let invalid_linking = self
            .linking
            .iter()
            .filter_map(|link| {
                let operation = self
                    .setups
                    .iter()
                    .flat_map(|s| &s.operations)
                    .find(|o| o.id() == link.operation_id);
                let invalid = !seen_linking.insert(link.operation_id)
                    || operation.is_none_or(|o| link.validate(o).is_err());
                invalid.then_some(link.operation_id)
            })
            .collect::<HashSet<_>>();
        self.toolpath_generations
            .retain(|g| !invalid_linking.contains(&g.operation_id));
        self.load_warnings = self.collect_load_warnings();
    }

    /// Recompute non-fatal issues after a validated write: warnings for
    /// fixed operations clear, still-broken disabled operations keep theirs.
    pub fn refresh_load_warnings(&mut self) {
        self.load_warnings = self.collect_load_warnings();
    }

    /// Every currently detectable non-fatal issue, without mutating anything.
    /// Structural corruption that strict validation rejects on writes (id
    /// collisions, rest-link breaks) is reported here so a loaded file can
    /// still be inspected and repaired piece by piece.
    fn collect_load_warnings(&self) -> Vec<CamLoadWarningDto> {
        let mut warnings = Vec::new();
        let document_warning = |message: String| CamLoadWarningDto {
            setup_id: None,
            operation_id: None,
            message,
        };
        for (kind, exhausted) in [
            (
                "setup",
                self.setups.iter().any(|setup| setup.id == u64::MAX),
            ),
            (
                "operation",
                self.setups.iter().any(|setup| {
                    setup
                        .operations
                        .iter()
                        .any(|operation| operation.id() == u64::MAX)
                }),
            ),
            ("tool", self.tools.iter().any(|tool| tool.id == u64::MAX)),
        ] {
            if exhausted {
                warnings.push(document_warning(format!(
                    "CAM {kind} id space is exhausted; repair the saved ids and references before editing or machining"
                )));
            }
        }
        let mut tool_ids = HashSet::new();
        let mut tool_numbers = HashSet::new();
        for tool in &self.tools {
            if let Err(message) = tool.validate() {
                warnings.push(document_warning(format!(
                    "tool '{}' library entry: {message}",
                    tool.name
                )));
            }
            if !tool_ids.insert(tool.id) {
                warnings.push(document_warning(format!(
                    "duplicate CAM tool id {}",
                    tool.id
                )));
            }
            if let Some(number) = tool.number {
                if !tool_numbers.insert(number) {
                    warnings.push(document_warning(format!(
                        "duplicate CAM tool number {number}"
                    )));
                }
            }
        }
        let mut setup_ids = HashSet::new();
        let mut operation_ids = HashSet::new();
        for setup in &self.setups {
            if !setup_ids.insert(setup.id) {
                warnings.push(document_warning(format!(
                    "duplicate CAM setup id {}",
                    setup.id
                )));
            }
            if let Err(message) = setup.validate_structure() {
                warnings.push(CamLoadWarningDto {
                    setup_id: Some(setup.id),
                    operation_id: None,
                    message,
                });
            }
            for operation in &setup.operations {
                if !operation_ids.insert(operation.id()) {
                    warnings.push(CamLoadWarningDto {
                        setup_id: Some(setup.id),
                        operation_id: None,
                        message: format!("duplicate CAM operation id {}", operation.id()),
                    });
                }
                if let Err(message) = operation.validate(setup, &self.tools) {
                    warnings.push(CamLoadWarningDto {
                        setup_id: Some(setup.id),
                        operation_id: Some(operation.id()),
                        message,
                    });
                }
            }
        }
        let mut height_operation_ids = HashSet::new();
        let mut linking_ids = HashSet::new();
        for linking in &self.linking {
            if !linking_ids.insert(linking.operation_id) {
                warnings.push(document_warning("Duplicate CAM linking record.".into()));
            }
            if let Some(operation) = self
                .setups
                .iter()
                .flat_map(|s| &s.operations)
                .find(|o| o.id() == linking.operation_id)
            {
                if let Err(message) = linking.validate(operation) {
                    warnings.push(CamLoadWarningDto {
                        setup_id: None,
                        operation_id: Some(operation.id()),
                        message,
                    });
                }
            }
        }
        for expressions in &self.height_expressions {
            let owner = self.setups.iter().find_map(|setup| {
                setup
                    .operations
                    .iter()
                    .find(|operation| operation.id() == expressions.operation_id)
                    .map(|operation| (setup.id, operation))
            });
            if owner.is_none() {
                warnings.push(document_warning(format!(
                    "CAM height expressions reference missing operation {}",
                    expressions.operation_id
                )));
                continue;
            }
            let (setup_id, operation) = owner.expect("owner checked above");
            if !height_operation_ids.insert(expressions.operation_id) {
                warnings.push(CamLoadWarningDto {
                    setup_id: Some(setup_id),
                    operation_id: Some(expressions.operation_id),
                    message: format!(
                        "duplicate CAM height expressions for operation {}",
                        expressions.operation_id
                    ),
                });
                continue;
            }
            if let Err(message) = expressions.validate_for_operation(operation) {
                warnings.push(CamLoadWarningDto {
                    setup_id: Some(setup_id),
                    operation_id: Some(expressions.operation_id),
                    message: format!(
                        "invalid CAM height expressions for operation {}: {message}",
                        expressions.operation_id
                    ),
                });
            }
        }
        if let Err(message) = self.validate_rest_links() {
            warnings.push(document_warning(message));
        }
        warnings
    }

    pub fn validate(&self) -> Result<(), String> {
        self.validate_with_tool_checks(true)
    }

    /// Preserve valid tool-library edits even when their consumers now need
    /// repair. Planning, simulation and posting always call strict validate.
    pub fn validate_for_editing(&self) -> Result<(), String> {
        self.validate_with_tool_checks(false)
    }

    fn validate_with_tool_checks(&self, check_tool: bool) -> Result<(), String> {
        if self.setups.len() > MAX_SETUPS {
            return Err(format!(
                "a project can contain at most {MAX_SETUPS} CAM setups"
            ));
        }
        if self.tools.len() > MAX_TOOLS {
            return Err(format!(
                "a project can contain at most {MAX_TOOLS} CAM tools"
            ));
        }
        if self.next_setup_id == 0 || self.next_operation_id == 0 || self.next_tool_id == 0 {
            return Err("CAM id counters must be non-zero".to_string());
        }

        let mut tool_ids = HashSet::new();
        let mut tool_numbers = HashSet::new();
        let mut max_tool_id = 0;
        for tool in &self.tools {
            tool.validate()?;
            if !tool_ids.insert(tool.id) {
                return Err(format!("duplicate CAM tool id {}", tool.id));
            }
            if let Some(number) = tool.number {
                if !tool_numbers.insert(number) {
                    return Err(format!("duplicate CAM tool number {number}"));
                }
            }
            max_tool_id = max_tool_id.max(tool.id);
        }

        let mut setup_ids = HashSet::new();
        let mut operation_ids = HashSet::new();
        let mut max_setup_id = 0;
        let mut max_operation_id = 0;
        for setup in &self.setups {
            if !setup_ids.insert(setup.id) {
                return Err(format!("duplicate CAM setup id {}", setup.id));
            }
            setup.validate(&self.tools, check_tool)?;
            max_setup_id = max_setup_id.max(setup.id);
            for operation in &setup.operations {
                if !operation_ids.insert(operation.id()) {
                    return Err(format!("duplicate CAM operation id {}", operation.id()));
                }
                max_operation_id = max_operation_id.max(operation.id());
            }
        }
        let mut height_operation_ids = HashSet::new();
        for expressions in &self.height_expressions {
            let operation = self
                .setups
                .iter()
                .flat_map(|setup| setup.operations.iter())
                .find(|operation| operation.id() == expressions.operation_id)
                .ok_or_else(|| {
                    format!(
                        "CAM height expressions reference missing operation {}",
                        expressions.operation_id
                    )
                })?;
            if !height_operation_ids.insert(expressions.operation_id) {
                return Err(format!(
                    "duplicate CAM height expressions for operation {}",
                    expressions.operation_id
                ));
            }
            expressions
                .validate_for_operation(operation)
                .map_err(|message| {
                    format!(
                        "invalid CAM height expressions for operation {}: {message}",
                        expressions.operation_id
                    )
                })?;
        }
        let mut linking_ids = HashSet::new();
        for linking in &self.linking {
            if !linking_ids.insert(linking.operation_id) {
                return Err("Duplicate CAM linking record.".into());
            }
            if let Some(operation) = self
                .setups
                .iter()
                .flat_map(|s| &s.operations)
                .find(|o| o.id() == linking.operation_id)
            {
                if operation.enabled() {
                    linking
                        .validate(operation)
                        .map_err(|e| format!("Operation '{}': {e}", operation.name()))?;
                }
            }
        }
        if let Some(active) = self.active_setup_id {
            if !setup_ids.contains(&active) {
                return Err("active CAM setup does not exist".to_string());
            }
        } else if !self.setups.is_empty() {
            return Err("a CAM document with setups must select an active setup".to_string());
        }
        if self.next_setup_id <= max_setup_id
            || self.next_operation_id <= max_operation_id
            || self.next_tool_id <= max_tool_id
        {
            return Err("CAM id counters must be greater than every saved id".to_string());
        }
        if let Some(profile) = &self.post_defaults.siemens_828d {
            profile.validate()?;
        }
        if self
            .post_defaults
            .machine_retract_z
            .is_some_and(|z| !z.is_finite())
        {
            return Err("Machine retract Z must be finite".into());
        }

        self.validate_rest_links()
    }

    /// Rest-stock links between setups: the source setup must exist with
    /// an envelope containing the source stock in the new WCS, and the
    /// link graph must be acyclic so simulation can resolve it.
    fn validate_rest_links(&self) -> Result<(), String> {
        for (setup_index, setup) in self.setups.iter().enumerate() {
            let CamResolvedStockDto::Rest { source_setup_id } = &setup.resolved_stock else {
                continue;
            };
            let source = self.setup(*source_setup_id).ok_or_else(|| {
                format!(
                    "setup '{}' inherits remaining stock from a missing setup",
                    setup.name
                )
            })?;
            let required = setup.wcs.stock_from(source.wcs, &source.stock);
            if setup.stock.min.x > required.min.x + 1e-6
                || setup.stock.min.y > required.min.y + 1e-6
                || setup.stock.min.z > required.min.z + 1e-6
                || setup.stock.max.x < required.max.x - 1e-6
                || setup.stock.max.y < required.max.y - 1e-6
                || setup.stock.max.z < required.max.z - 1e-6
            {
                return Err(format!("setup '{}' rest-stock envelope must contain the source stock transformed from setup '{}'", setup.name, source.name));
            }

            let mut seen = HashSet::from([setup.id]);
            let mut cursor = source;
            while let CamResolvedStockDto::Rest { source_setup_id } = &cursor.resolved_stock {
                if !seen.insert(*source_setup_id) {
                    return Err(format!(
                        "setup '{}' rest-stock chain loops back on itself",
                        setup.name
                    ));
                }
                cursor = self.setup(*source_setup_id).ok_or_else(|| {
                    format!(
                        "setup '{}' rest-stock chain references a missing setup",
                        setup.name
                    )
                })?;
            }
            if self.setups.iter().position(|s| s.id == source.id).unwrap() >= setup_index {
                return Err(format!(
                    "setup '{}' must follow its rest-stock source '{}' in the browser order",
                    setup.name, source.name
                ));
            }
        }
        Ok(())
    }

    pub fn setup(&self, id: u64) -> Option<&CamSetupDto> {
        self.setups.iter().find(|setup| setup.id == id)
    }

    pub fn tool(&self, id: u64) -> Option<&CamToolDto> {
        self.tools.iter().find(|tool| tool.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cutting() -> CuttingParametersDto {
        CuttingParametersDto {
            spindle_rpm: 8_000,
            feed_xy: 600.0,
            feed_z: 180.0,
            coolant: CoolantMode::Off,
        }
    }

    fn tool() -> CamToolDto {
        CamToolDto {
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
        }
    }

    fn face(id: u64, clearance_z: f64, retract_z: f64) -> CamOperationDto {
        CamOperationDto::Face {
            id,
            name: format!("Face {id}"),
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
            direction: FaceDirection::BothWays,
            clearance_z,
            retract_z,
            feed_height_z: 0.5,
            cutting: cutting(),
        }
    }

    fn setup(
        id: u64,
        stock_spec: CamStockSpecDto,
        resolved_stock: CamResolvedStockDto,
    ) -> CamSetupDto {
        CamSetupDto {
            id,
            name: format!("Setup {id}"),
            wcs: WorkCoordinateSystemDto::default(),
            wcs_origin: WcsOriginSpecDto::Explicit,
            work_offset: WorkOffset::G54,
            work_offset_count: 1,
            stock_spec,
            resolved_stock,
            stock: StockBoxDto {
                min: Point3Dto::new(0.0, 0.0, -10.0),
                max: Point3Dto::new(20.0, 20.0, 0.0),
            },
            stock_model_box: None,
            body_ids: vec![],
            machine: None,
            legacy_clearance_z: None,
            legacy_retract_z: None,
            operations: vec![face(id, 8.0, 2.0)],
        }
    }

    fn document_with(setups: Vec<CamSetupDto>) -> CamDocumentDto {
        let next_setup_id = setups.iter().map(|setup| setup.id).max().unwrap_or(0) + 1;
        CamDocumentDto {
            active_setup_id: setups.first().map(|setup| setup.id),
            next_setup_id,
            next_operation_id: 100,
            next_tool_id: 2,
            tools: vec![tool()],
            setups,
            ..CamDocumentDto::default()
        }
    }

    #[test]
    fn legacy_setup_heights_migrate_into_operations() {
        let legacy = r#"{
            "setups": [{
                "id": 1,
                "name": "Old setup",
                "wcs": {
                    "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                    "x_axis": [1.0, 0.0, 0.0],
                    "y_axis": [0.0, 1.0, 0.0],
                    "z_axis": [0.0, 0.0, 1.0]
                },
                "work_offset": "g54",
                "stock": {
                    "min": {"x": 0.0, "y": 0.0, "z": -10.0},
                    "max": {"x": 20.0, "y": 20.0, "z": 0.0}
                },
                "clearance_z": 8.0,
                "retract_z": 2.0,
                "rapid_feed": 3000.0,
                "post": {"dialect": "grbl", "program_number": 7},
                "operations": [{
                    "kind": "face",
                    "id": 1,
                    "name": "Face",
                    "tool_id": 1,
                    "bounds": {"min": {"x": 0.0, "y": 0.0}, "max": {"x": 20.0, "y": 20.0}},
                    "top_z": 0.0,
                    "target_z": -1.0,
                    "step_over": 3.0,
                    "step_down": 1.0,
                    "cutting": {"spindle_rpm": 8000, "feed_xy": 600.0, "feed_z": 180.0}
                }]
            }],
            "active_setup_id": 1,
            "tools": [{
                "id": 1,
                "number": 1,
                "name": "6 mm flat",
                "kind": "flat_end_mill",
                "diameter": 6.0,
                "flute_length": 15.0,
                "overall_length": 50.0
            }],
            "next_setup_id": 2,
            "next_operation_id": 2,
            "next_tool_id": 2
        }"#;
        let mut document: CamDocumentDto = serde_json::from_str(legacy).unwrap();
        assert_eq!(document.setups[0].legacy_clearance_z, Some(8.0));
        document.migrate_legacy();
        document.validate().unwrap();
        let operation = &document.setups[0].operations[0];
        assert_eq!(operation.clearance_z(), 8.0);
        assert_eq!(operation.retract_z(), 2.0);
        assert_eq!(document.setups[0].legacy_clearance_z, None);

        let serialized = serde_json::to_string(&document).unwrap();
        let setup_json: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        let setup_json = setup_json["setups"][0].clone();
        assert!(setup_json.get("clearance_z").is_none());
        assert!(setup_json.get("rapid_feed").is_none());
        assert!(setup_json.get("post").is_none());
    }

    #[test]
    fn cutting_data_profiles_validate_and_default_to_empty() {
        let base = tool();
        assert!(base.cutting_presets.is_empty());
        document_with(vec![setup(
            1,
            CamStockSpecDto::LegacyBox,
            CamResolvedStockDto::Box,
        )])
        .validate()
        .unwrap();

        let mut profiled = tool();
        profiled.cutting_presets = vec![CamCuttingPresetDto {
            name: "Aluminum".into(),
            cutting: cutting(),
        }];
        let mut document = document_with(vec![setup(
            1,
            CamStockSpecDto::LegacyBox,
            CamResolvedStockDto::Box,
        )]);
        document.tools = vec![profiled];
        document.validate().unwrap();

        let mut duplicated = tool();
        duplicated.cutting_presets = vec![
            CamCuttingPresetDto {
                name: "Aluminum".into(),
                cutting: cutting(),
            },
            CamCuttingPresetDto {
                name: "Aluminum".into(),
                cutting: cutting(),
            },
        ];
        document.tools = vec![duplicated];
        let error = document.validate().unwrap_err();
        assert!(error.contains("unique"));

        let mut blank = tool();
        blank.cutting_presets = vec![CamCuttingPresetDto {
            name: "  ".into(),
            cutting: cutting(),
        }];
        document.tools = vec![blank];
        let error = document.validate().unwrap_err();
        assert!(error.contains("must have names"));
    }

    #[test]
    fn corner_radius_is_checked_against_kind_and_diameter() {
        let setup = || setup(1, CamStockSpecDto::LegacyBox, CamResolvedStockDto::Box);

        let mut bull = tool();
        bull.kind = CamToolKind::BullNoseEndMill;
        let mut document = document_with(vec![setup()]);
        document.tools = vec![bull.clone()];
        let error = document.validate().unwrap_err();
        assert!(error.contains("corner radius"));

        let mut valid = bull.clone();
        valid.corner_radius = Some(1.5);
        document.tools = vec![valid];
        document.validate().unwrap();

        let mut oversized = bull.clone();
        oversized.corner_radius = Some(4.0);
        document.tools = vec![oversized];
        let error = document.validate().unwrap_err();
        assert!(error.contains("half the diameter"));

        let mut tap_tool = tool();
        tap_tool.kind = CamToolKind::Tap;
        tap_tool.corner_radius = Some(0.5);
        document.tools = vec![tap_tool];
        let error = document.validate().unwrap_err();
        assert!(error.contains("only flat, bull-nose, and face mills"));
    }

    #[test]
    fn rest_stock_requires_transformed_envelope() {
        let first = setup(1, CamStockSpecDto::LegacyBox, CamResolvedStockDto::Box);
        let mut second = setup(
            2,
            CamStockSpecDto::RestFromSetup { setup_id: 1 },
            CamResolvedStockDto::Rest { source_setup_id: 1 },
        );
        document_with(vec![first.clone(), second.clone()])
            .validate()
            .unwrap();

        second.wcs.origin = Point3Dto::new(1.0, 0.0, 0.0);
        let error = document_with(vec![first.clone(), second.clone()])
            .validate()
            .unwrap_err();
        assert!(error.contains("must contain"));

        second.wcs.origin = Point3Dto::new(0.0, 0.0, 0.0);
        second.stock.min.x = first.stock.min.x + 1.0;
        second.operations.clear();
        let error = document_with(vec![first.clone(), second.clone()])
            .validate()
            .unwrap_err();
        assert!(error.contains("must contain"));

        let angle = 37.0_f64.to_radians();
        second.wcs = WorkCoordinateSystemDto {
            origin: Point3Dto::new(7., -3., 9.),
            x_axis: [angle.cos(), 0., -angle.sin()],
            y_axis: [0., 1., 0.],
            z_axis: [angle.sin(), 0., angle.cos()],
        };
        second.stock = second.wcs.stock_from(first.wcs, &first.stock);
        document_with(vec![first.clone(), second.clone()])
            .validate()
            .unwrap();

        let mut looping_a = setup(
            1,
            CamStockSpecDto::RestFromSetup { setup_id: 2 },
            CamResolvedStockDto::Rest { source_setup_id: 2 },
        );
        looping_a.operations = vec![face(1, 8.0, 2.0)];
        let looping_b = setup(
            2,
            CamStockSpecDto::RestFromSetup { setup_id: 1 },
            CamResolvedStockDto::Rest { source_setup_id: 1 },
        );
        let error = document_with(vec![looping_a, looping_b])
            .validate()
            .unwrap_err();
        assert!(error.contains("loops back"));
    }

    #[test]
    fn work_offset_repetition_stays_within_g59() {
        let mut setup = setup(1, CamStockSpecDto::LegacyBox, CamResolvedStockDto::Box);
        setup.work_offset = WorkOffset::G58;
        setup.work_offset_count = 2;
        document_with(vec![setup.clone()]).validate().unwrap();
        assert_eq!(setup.work_offsets(), vec![WorkOffset::G58, WorkOffset::G59]);
        setup.work_offset_count = 3;
        let error = document_with(vec![setup]).validate().unwrap_err();
        assert!(error.contains("G54..=G59"));
    }

    #[test]
    fn stock_spec_and_resolved_shape_must_agree() {
        let mut setup = setup(
            1,
            CamStockSpecDto::FromModel {
                shape: CamStockShape::Cylinder,
                offsets: CamStockOffsetsDto::default(),
            },
            CamResolvedStockDto::Cylinder {
                center: Point2Dto::new(10.0, 10.0),
                radius: 9.0,
            },
        );
        document_with(vec![setup.clone()]).validate().unwrap();

        setup.resolved_stock = CamResolvedStockDto::Box;
        let error = document_with(vec![setup.clone()]).validate().unwrap_err();
        assert!(error.contains("box/cylinder/hex"));

        setup.resolved_stock = CamResolvedStockDto::Cylinder {
            center: Point2Dto::new(10.0, 10.0),
            radius: 12.0,
        };
        setup.stock_spec = CamStockSpecDto::FromModel {
            shape: CamStockShape::Cylinder,
            offsets: CamStockOffsetsDto::default(),
        };
        let error = document_with(vec![setup]).validate().unwrap_err();
        assert!(error.contains("inside the stock envelope"));
    }

    #[test]
    fn legacy_document_without_feed_height_opens_clean() {
        let legacy = r#"{
            "setups": [{
                "id": 1,
                "name": "Setup 1",
                "wcs": {
                    "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                    "x_axis": [1.0, 0.0, 0.0],
                    "y_axis": [0.0, 1.0, 0.0],
                    "z_axis": [0.0, 0.0, 1.0]
                },
                "work_offset": "g54",
                "stock": {
                    "min": {"x": -17.0, "y": -9.5, "z": 0.0},
                    "max": {"x": 17.0, "y": 9.5, "z": 14.0}
                },
                "operations": [{
                    "kind": "face",
                    "id": 1,
                    "name": "Face 1",
                    "enabled": true,
                    "tool_id": 1,
                    "bounds": {"min": {"x": -17.0, "y": -9.5}, "max": {"x": 17.0, "y": 9.5}},
                    "top_z": 14.0,
                    "target_z": 12.0,
                    "step_over": 3.0,
                    "step_down": 0.5,
                    "safe_distance": 5.0,
                    "clearance_z": 24.0,
                    "retract_z": 17.0,
                    "cutting": {"spindle_rpm": 606, "feed_xy": 121.2, "feed_z": 300.0, "coolant": "flood"}
                }]
            }],
            "active_setup_id": 1,
            "tools": [{
                "id": 1,
                "number": 1,
                "name": "63 mm face mill",
                "kind": "face_mill",
                "diameter": 63.0,
                "flute_length": 30.0,
                "overall_length": 80.0,
                "flute_count": 5
            }],
            "next_setup_id": 2,
            "next_operation_id": 2,
            "next_tool_id": 2
        }"#;
        let mut document: CamDocumentDto = serde_json::from_str(legacy).unwrap();

        assert!(document.validate().is_err());
        document.soften_for_load();
        document.validate().unwrap();
        let operation = &document.setups[0].operations[0];
        assert!(operation.enabled());
        assert_eq!(operation.feed_height_z(), 14.0);
        assert!(document.load_warnings.is_empty());
    }

    #[test]
    fn malformed_height_intent_is_never_silently_converted_to_absolute_z() {
        let mut document = document_with(vec![setup(
            1,
            CamStockSpecDto::LegacyBox,
            CamResolvedStockDto::Box,
        )]);
        document.height_expressions = vec![CamOperationHeightExpressionsDto {
            operation_id: 1,
            clearance: CamHeightExpressionDto {
                geometry: None,
                reference: CamHeightReferenceDto::Retract,
                offset: 5.0,
            },
            retract: CamHeightExpressionDto {
                geometry: None,
                reference: CamHeightReferenceDto::Feed,
                offset: 2.0,
            },
            feed: CamHeightExpressionDto {
                geometry: None,
                reference: CamHeightReferenceDto::Top,
                offset: 1.0,
            },

            top: CamHeightExpressionDto {
                geometry: None,
                reference: CamHeightReferenceDto::Top,
                offset: 0.0,
            },
            bottom: Some(CamHeightExpressionDto {
                geometry: None,
                reference: CamHeightReferenceDto::StockBottom,
                offset: 1.0,
            }),
        }];
        let fingerprint = "0".repeat(32);
        document.toolpath_generations = vec![CamToolpathGenerationDto {
            operation_id: 1,
            planner_revision: 1,
            model_fingerprint: fingerprint.clone(),
            setup_fingerprint: fingerprint.clone(),
            operation_fingerprint: fingerprint.clone(),
            tool_fingerprint: fingerprint.clone(),
            upstream_fingerprint: fingerprint,
            order_dependencies: None,
        }];

        document.migrate_legacy();
        assert_eq!(document.height_expressions.len(), 1);
        assert!(document
            .validate()
            .unwrap_err()
            .contains("top height may only reference a lower"));

        document.soften_for_load();
        assert_eq!(document.height_expressions.len(), 1);
        assert!(document.toolpath_generations.is_empty());
        assert!(document.load_warnings.iter().any(|warning| {
            warning.operation_id == Some(1)
                && warning.message.contains("invalid CAM height expressions")
        }));
    }

    #[test]
    fn invalid_operations_are_parked_with_warnings_not_rejected() {
        let mut document = document_with(vec![setup(
            1,
            CamStockSpecDto::LegacyBox,
            CamResolvedStockDto::Box,
        )]);
        if let CamOperationDto::Face { step_over, .. } = &mut document.setups[0].operations[0] {
            *step_over = -1.0;
        }
        assert!(document.validate().is_err());
        document.soften_for_load();
        document.validate().unwrap();
        let operation = &document.setups[0].operations[0];
        assert!(!operation.enabled());
        assert_eq!(document.load_warnings.len(), 1);
        assert_eq!(document.load_warnings[0].operation_id, Some(operation.id()));
        assert_eq!(document.load_warnings[0].setup_id, Some(1));
        assert!(document.load_warnings[0].message.contains("stepover"));

        if let CamOperationDto::Face {
            step_over, enabled, ..
        } = &mut document.setups[0].operations[0]
        {
            *step_over = 3.0;
            *enabled = true;
        }
        document.validate().unwrap();
        document.refresh_load_warnings();
        assert!(document.load_warnings.is_empty());
    }

    #[test]
    fn soften_repairs_stale_active_setup_and_id_counters() {
        let mut document = document_with(vec![setup(
            1,
            CamStockSpecDto::LegacyBox,
            CamResolvedStockDto::Box,
        )]);
        document.active_setup_id = Some(99);
        document.next_setup_id = 1;
        document.soften_for_load();
        assert_eq!(document.active_setup_id, Some(1));
        assert!(document.next_setup_id > 1);
        document.validate().unwrap();
    }

    #[test]
    fn load_preserves_exhausted_ids_and_rejects_them_for_machining() {
        for kind in ["setup", "operation", "tool"] {
            let mut document = saved_document_with_boundary_id(kind, u64::MAX);
            document.soften_for_load();
            let saved = serde_json::to_value(&document).unwrap();
            let (id, next_id) = match kind {
                "setup" => (saved["setups"][0]["id"].as_u64(), document.next_setup_id),
                "operation" => (
                    saved["setups"][0]["operations"][0]["id"].as_u64(),
                    document.next_operation_id,
                ),
                "tool" => (saved["tools"][0]["id"].as_u64(), document.next_tool_id),
                _ => unreachable!(),
            };
            assert_eq!(id, Some(u64::MAX), "must not renumber {kind} identity");
            assert_eq!(next_id, u64::MAX, "must not wrap {kind} allocation");
            assert!(document.load_warnings.iter().any(|warning| {
                warning
                    .message
                    .contains(&format!("{kind} id space is exhausted"))
            }));
            assert!(document
                .validate_for_editing()
                .unwrap_err()
                .contains("id counters must be greater"));
            assert!(crate::planner::plan_setup(&document, document.setups[0].id).is_err());
        }
    }

    #[test]
    fn load_advances_last_available_ids_without_rejecting_valid_documents() {
        for kind in ["setup", "operation", "tool"] {
            let mut document = saved_document_with_boundary_id(kind, u64::MAX - 1);
            document.soften_for_load();
            let next_id = match kind {
                "setup" => document.next_setup_id,
                "operation" => document.next_operation_id,
                "tool" => document.next_tool_id,
                _ => unreachable!(),
            };
            assert_eq!(next_id, u64::MAX, "must retain the final {kind} counter");
            document.validate_for_editing().unwrap();
            assert!(document.load_warnings.is_empty());
            assert!(crate::planner::plan_setup(&document, document.setups[0].id).is_ok());
        }
    }

    /// Exercise the actual saved DTO shape, keeping references consistent so
    /// the only corruption at MAX is the exhausted allocation space.
    fn saved_document_with_boundary_id(kind: &str, id: u64) -> CamDocumentDto {
        let document = document_with(vec![setup(
            1,
            CamStockSpecDto::LegacyBox,
            CamResolvedStockDto::Box,
        )]);
        let mut saved = serde_json::to_value(document).unwrap();
        match kind {
            "setup" => {
                saved["setups"][0]["id"] = id.into();
                saved["active_setup_id"] = id.into();
            }
            "operation" => saved["setups"][0]["operations"][0]["id"] = id.into(),
            "tool" => {
                saved["tools"][0]["id"] = id.into();
                saved["setups"][0]["operations"][0]["tool_id"] = id.into();
            }
            _ => unreachable!(),
        }
        serde_json::from_value(saved).unwrap()
    }
}
