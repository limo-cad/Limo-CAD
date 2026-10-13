//! Persisted linking intent. Absent records preserve legacy programs; new
//! records are validated by Rust and participate in generation fingerprints.
use crate::model::{CamOperationDto, CompensationMode, Point2Dto};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CamHighFeedMode {
    #[default]
    Preserve,
    AxialRadial,
    Axial,
    Radial,
    SingleAxis,
    Always,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CamRetractionPolicy {
    #[default]
    Full,
    Minimum,
    Shortest,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CamFaceTransition {
    NoContact,
    Straight,
    Shortest,
    #[default]
    Smooth,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CamRampType {
    Predrill,
    Plunge,
    #[default]
    Helix,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CamLeadDto {
    pub enabled: bool,
    /// Physical cutter-center radii, also in controller compensation mode.
    pub horizontal_radius: f64,
    pub sweep_degrees: f64,
    pub linear_distance: f64,
    pub perpendicular: bool,
    pub vertical_radius: f64,
}
impl Default for CamLeadDto {
    fn default() -> Self {
        Self {
            enabled: true,
            horizontal_radius: 0.0,
            sweep_degrees: 90.0,
            linear_distance: 5.0,
            perpendicular: false,
            vertical_radius: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CamLinkingDto {
    pub operation_id: u64,
    pub high_feed_mode: CamHighFeedMode,
    pub high_feed: f64,
    pub allow_rapid_retract: bool,
    pub keep_tool_down: bool,
    pub maximum_stay_down: f64,
    pub minimum_clearance: f64,
    /// Bounded search effort, 0..100; never weakens a clearance predicate.
    pub stay_down_level: u32,
    pub lift_height: f64,
    pub retraction_policy: CamRetractionPolicy,
    pub safe_distance: f64,
    pub extend_before_retract: bool,
    pub transition: CamFaceTransition,
    pub lead_in: CamLeadDto,
    pub lead_out: CamLeadDto,
    pub same_as_lead_in: bool,
    pub lead_in_feed: f64,
    pub lead_out_feed: f64,
    pub no_engagement_feed: f64,
    /// False for legacy projects: their explicit numeric feeds remain intact.
    pub lead_in_feed_auto: bool,
    pub lead_out_feed_auto: bool,
    pub no_engagement_feed_auto: bool,
    pub ramp_enabled: bool,
    pub ramp_type: CamRampType,
    pub ramp_angle: f64,
    pub ramp_stepdown: f64,
    pub ramp_clearance: f64,
    pub ramp_taper_angle: f64,
    /// Diameter of the tool-center helix, not of the resulting hole.
    pub helix_diameter: f64,
    pub minimum_helix_diameter: f64,
    pub ramp_feed: f64,
    pub predrill_positions: Vec<Point2Dto>,
    pub entry_positions: Vec<Point2Dto>,
    pub exit_positions: Vec<Point2Dto>,
}
impl Default for CamLinkingDto {
    fn default() -> Self {
        Self {
            operation_id: 0,
            high_feed_mode: CamHighFeedMode::Preserve,
            high_feed: 5000.0,
            allow_rapid_retract: true,
            keep_tool_down: false,
            maximum_stay_down: 60.0,
            minimum_clearance: 0.0,
            stay_down_level: 0,
            lift_height: 0.0,
            retraction_policy: CamRetractionPolicy::Full,
            safe_distance: 1.0,
            extend_before_retract: true,
            transition: CamFaceTransition::Smooth,
            lead_in: CamLeadDto::default(),
            lead_out: CamLeadDto::default(),
            same_as_lead_in: true,
            lead_in_feed: 600.0,
            lead_out_feed: 600.0,
            no_engagement_feed: 600.0,
            lead_in_feed_auto: false,
            lead_out_feed_auto: false,
            no_engagement_feed_auto: false,
            ramp_enabled: false,
            ramp_type: CamRampType::Helix,
            ramp_angle: 3.0,
            ramp_stepdown: 1.0,
            ramp_clearance: 1.0,
            ramp_taper_angle: 0.0,
            helix_diameter: 5.0,
            minimum_helix_diameter: 2.5,
            ramp_feed: 180.0,
            predrill_positions: Vec::new(),
            entry_positions: Vec::new(),
            exit_positions: Vec::new(),
        }
    }
}
impl CamLinkingDto {
    pub fn resolve_feeds(&mut self, cutting_feed: f64) {
        if self.lead_in_feed_auto {
            self.lead_in_feed = cutting_feed;
        }
        if self.lead_out_feed_auto {
            self.lead_out_feed = cutting_feed;
        }
        if self.no_engagement_feed_auto {
            self.no_engagement_feed = cutting_feed;
        }
    }

    pub fn exit(&self) -> CamLeadDto {
        if self.same_as_lead_in {
            CamLeadDto {
                enabled: self.lead_out.enabled,
                ..self.lead_in.clone()
            }
        } else {
            self.lead_out.clone()
        }
    }
    pub fn validate(&self, operation: &CamOperationDto) -> Result<(), String> {
        if !matches!(
            operation,
            CamOperationDto::Face { .. }
                | CamOperationDto::Contour2d { .. }
                | CamOperationDto::Chamfer2d { .. }
                | CamOperationDto::Adaptive3d { .. }
        ) {
            return Err(
                "Custom linking is available for Face, 2D Contour, 2D Chamfer and High Speed Roughing.".into(),
            );
        }
        if matches!(operation, CamOperationDto::Chamfer2d { .. }) {
            if self.keep_tool_down || self.retraction_policy != CamRetractionPolicy::Full {
                return Err("2D Chamfer retracts to clearance between chains; keep-down and reduced retraction are not supported.".into());
            }
            if self.ramp_enabled
                || !self.predrill_positions.is_empty()
                || !self.entry_positions.is_empty()
                || !self.exit_positions.is_empty()
            {
                return Err("2D Chamfer supports manual leads, but not ramps or preferred entry/exit positions. Closed chains join at a straight-edge station; open chains use their endpoints.".into());
            }
        }
        for (name, value) in [
            ("high feed", self.high_feed),
            ("lead-in feed", self.lead_in_feed),
            ("lead-out feed", self.lead_out_feed),
            ("no-engagement feed", self.no_engagement_feed),
            ("ramp feed", self.ramp_feed),
        ] {
            if !value.is_finite() || value <= 0.0 || value > 1e7 {
                return Err(format!(
                    "{name} must be finite and positive (maximum 10,000,000 mm/min)"
                ));
            }
        }
        for (name, value) in [
            ("maximum stay-down distance", self.maximum_stay_down),
            ("minimum clearance", self.minimum_clearance),
            ("lift height", self.lift_height),
            ("safe distance", self.safe_distance),
            ("ramp clearance", self.ramp_clearance),
            ("helix diameter", self.helix_diameter),
            ("minimum helix diameter", self.minimum_helix_diameter),
        ] {
            if !value.is_finite() || !(0.0..=1e6).contains(&value) {
                return Err(format!(
                    "{name} must be finite and non-negative (maximum 1,000,000 mm)"
                ));
            }
        }
        if self.stay_down_level > 100 {
            return Err("Stay-down search level must be 0..100.".into());
        }
        for lead in [&self.lead_in, &self.lead_out] {
            for value in [
                lead.horizontal_radius,
                lead.vertical_radius,
                lead.linear_distance,
            ] {
                if !value.is_finite() || !(0.0..=1e6).contains(&value) {
                    return Err("Lead lengths and radii must be finite and non-negative.".into());
                }
            }
            if !lead.sweep_degrees.is_finite() || !(0.0..=180.0).contains(&lead.sweep_degrees) {
                return Err("Lead sweep must be between 0 and 180 degrees.".into());
            }
        }
        if matches!(
            operation,
            CamOperationDto::Contour2d {
                compensation_mode: CompensationMode::InControl,
                ..
            }
        ) {
            for lead in [&self.lead_in, &self.exit()] {
                if !lead.enabled || lead.linear_distance <= 1e-9 {
                    return Err("In-control compensation requires enabled entry/exit and positive linear distances for G41/G42 and G40.".into());
                }
                if lead.perpendicular {
                    return Err("Perpendicular linear leads require software compensation; controller activation must remain tangent to its arc.".into());
                }
            }
        }
        if !self.ramp_angle.is_finite()
            || !(0.0..30.0).contains(&self.ramp_angle)
            || self.ramp_angle <= 0.0
            || !self.ramp_stepdown.is_finite()
            || self.ramp_stepdown <= 0.0
            || self.ramp_stepdown > 1e6
            || !self.ramp_taper_angle.is_finite()
            || !(0.0..15.0).contains(&self.ramp_taper_angle)
        {
            return Err(
                "Ramp angle must be >0 and <30°, stepdown positive, and taper between 0 and 15°."
                    .into(),
            );
        }
        if self.minimum_helix_diameter <= 0.0 || self.minimum_helix_diameter > self.helix_diameter {
            return Err("Minimum helix diameter must be positive and no larger than the preferred diameter.".into());
        }
        for points in [
            &self.predrill_positions,
            &self.entry_positions,
            &self.exit_positions,
        ] {
            if points.len() > 32 || points.iter().any(|p| !p.is_finite()) {
                return Err("Link positions need at most 32 finite setup-XY points.".into());
            }
        }
        if self.entry_positions.len() > 1 || self.exit_positions.len() > 1 {
            return Err("Use one preferred entry station and one preferred exit station per operation; multiple predrill holes are supported.".into());
        }
        if matches!(operation, CamOperationDto::Contour2d { closed: false, .. })
            && (!self.entry_positions.is_empty()
                || !self.exit_positions.is_empty()
                || !self.predrill_positions.is_empty())
        {
            return Err("Preferred stations and predrill selection currently require a closed contour; open chains use their selected endpoints.".into());
        }
        if matches!(operation, CamOperationDto::Adaptive3d { .. }) && self.ramp_angle > 10.0 {
            return Err("High Speed Roughing ramp angle must be at most 10°.".into());
        }
        if self.ramp_enabled
            && self.ramp_type == CamRampType::Predrill
            && self.predrill_positions.is_empty()
        {
            return Err("Predrill entry requires a position backed by an earlier enabled drilling operation.".into());
        }
        Ok(())
    }
}
