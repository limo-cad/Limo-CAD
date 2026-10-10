//! Typed joint fields over the shared assembly DTOs. The engine remains the
//! authority for connector canonicalization, constraints, solving and commits.
use super::{DimensionKind, MeasurementInput};
use limo_cad_core::UnitSystem;
use limo_cad_sketch::{
    AssemblyDocumentDto, CreateJointRequestDto, JointAdvancedDto, JointConnectorDto,
    JointDefinitionDto, JointKindDto, JointLimitsDto, OccurrenceId, UpdateJointRequestDto,
};
use serde_json::Value;

pub(crate) const KINDS: [(JointKindDto, &str, &str); 9] = [
    (JointKindDto::Rigid, "rigid", "Rigid"),
    (JointKindDto::Revolute, "revolute", "Revolute"),
    (JointKindDto::Slider, "slider", "Slider"),
    (JointKindDto::Cylindrical, "cylindrical", "Cylindrical"),
    (JointKindDto::Planar, "planar", "Planar"),
    (JointKindDto::Ball, "ball", "Ball"),
    (JointKindDto::PinSlot, "pin_slot", "Pin-slot"),
    (JointKindDto::Screw, "screw", "Screw"),
    (JointKindDto::Universal, "universal", "Universal"),
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    Name,
    Kind,
    Coordinate(usize, usize),
    Limited(usize),
    Pitch,
    Twist(usize),
    Flipped,
}
#[derive(Clone)]
pub(crate) struct Connector {
    pub connector: JointConnectorDto,
    pub occurrence: OccurrenceId,
    pub label: String,
}
pub(crate) struct Coordinate {
    pub values: [MeasurementInput; 3],
    pub limited: bool,
}
impl Coordinate {
    fn new(value: f64, limits: Option<JointLimitsDto>, linear: bool, units: UnitSystem) -> Self {
        let kind = if linear {
            DimensionKind::Length
        } else {
            DimensionKind::Angle
        };
        let fallback = if linear { 25. } else { 90. };
        let l = limits.unwrap_or(JointLimitsDto {
            min: -fallback,
            max: fallback,
        });
        Self {
            values: [value, l.min, l.max].map(|v| MeasurementInput::new(kind, v, units)),
            limited: limits.is_some(),
        }
    }
    fn value(&self, units: UnitSystem) -> Result<(f64, Option<JointLimitsDto>), String> {
        let v = self.values[0].evaluate(units, &[])?;
        let limits = if self.limited {
            let min = self.values[1].evaluate(units, &[])?;
            let max = self.values[2].evaluate(units, &[])?;
            if min > max || v < min || v > max {
                return Err("Offset must lie between the minimum and maximum limits".into());
            }
            Some(JointLimitsDto { min, max })
        } else {
            None
        };
        Ok((v, limits))
    }
}
pub(crate) struct Form {
    pub original: Option<JointDefinitionDto>,
    pub name: String,
    pub kind: JointKindDto,
    pub connectors: [Option<Connector>; 2],
    pub coordinates: [Coordinate; 5],
    pub pitch: MeasurementInput,
    pub twists: [MeasurementInput; 2],
    pub flipped: bool,
    pub fixed: Option<usize>,
    pub ground_changed: bool,
    pub units: UnitSystem,
}
impl Form {
    pub(crate) fn new(
        a: &AssemblyDocumentDto,
        original: Option<JointDefinitionDto>,
        units: UnitSystem,
    ) -> Self {
        let j = original.as_ref();
        let advanced = j.map(|j| j.advanced).unwrap_or_default();
        let coordinates = [
            Coordinate::new(
                j.map_or(0., |j| j.angle_offset_deg),
                j.and_then(|j| {
                    j.angle_limits.or(j
                        .limits
                        .filter(|_| matches!(j.kind, JointKindDto::Revolute | JointKindDto::Screw)))
                }),
                false,
                units,
            ),
            Coordinate::new(
                j.map_or(0., |j| j.linear_offset_mm),
                j.and_then(|j| {
                    j.linear_limits
                        .or(j.limits.filter(|_| j.kind == JointKindDto::Slider))
                }),
                true,
                units,
            ),
            Coordinate::new(
                advanced.secondary_angle_offset_deg,
                advanced.secondary_angle_limits,
                false,
                units,
            ),
            Coordinate::new(
                advanced.tertiary_angle_offset_deg,
                advanced.tertiary_angle_limits,
                false,
                units,
            ),
            Coordinate::new(
                advanced.secondary_linear_offset_mm,
                advanced.secondary_linear_limits,
                true,
                units,
            ),
        ];
        let connectors = std::array::from_fn(|i| {
            let j = j?;
            let connector = if i == 0 {
                &j.connector_a
            } else {
                &j.connector_b
            };
            let occurrence = (if i == 0 {
                j.advanced.connector_a_occurrence_id
            } else {
                j.advanced.connector_b_occurrence_id
            })?;
            let label = a
                .component_structure
                .occurrences
                .iter()
                .find(|o| o.id == occurrence)?
                .name
                .clone();
            Some(Connector {
                connector: connector.clone(),
                occurrence,
                label,
            })
        });
        let mut form = Self {
            name: j.map_or_else(|| format!("Joint{}", a.next_joint_id), |j| j.name.clone()),
            kind: j.map_or(JointKindDto::Rigid, |j| j.kind),
            connectors,
            coordinates,
            pitch: MeasurementInput::new(
                DimensionKind::Length,
                advanced.screw_pitch_mm_per_revolution,
                units,
            ),
            twists: [
                advanced.connector_a_twist_deg,
                advanced.connector_b_twist_deg,
            ]
            .map(|v| MeasurementInput::new(DimensionKind::Angle, v, units)),
            flipped: j.is_none_or(|j| j.flipped),
            fixed: None,
            ground_changed: false,
            units,
            original,
        };
        form.infer_ground(a);
        form
    }
    pub(crate) fn infer_ground(&mut self, a: &AssemblyDocumentDto) {
        if self.ground_changed {
            return;
        }
        let parent = self.connectors[0]
            .as_ref()
            .and_then(|c| {
                a.component_structure
                    .occurrences
                    .iter()
                    .find(|o| o.id == c.occurrence)
            })
            .map(|o| o.parent_occurrence_id);
        let grounded = a
            .component_structure
            .occurrences
            .iter()
            .find(|o| o.grounded && parent.is_none_or(|p| o.parent_occurrence_id == p));
        self.fixed = grounded.map_or(Some(0), |o| {
            self.connectors
                .iter()
                .position(|c| c.as_ref().is_some_and(|c| c.occurrence == o.id))
        });
    }
    pub(crate) fn set(&mut self, field: Field, value: &str) -> Result<(), String> {
        match field {
            Field::Name => self.name = value.into(),
            Field::Kind => {
                self.kind = KINDS
                    .iter()
                    .find(|(_, v, _)| *v == value)
                    .ok_or("Choose a joint type")?
                    .0
            }
            Field::Coordinate(i, j) => self
                .coordinates
                .get_mut(i)
                .and_then(|c| c.values.get_mut(j))
                .ok_or("Unknown motion coordinate")?
                .set_text(value.into()),
            Field::Limited(i) => {
                self.coordinates
                    .get_mut(i)
                    .ok_or("Unknown motion coordinate")?
                    .limited = value
                    .parse()
                    .map_err(|_| "Choose whether to limit motion")?
            }
            Field::Pitch => self.pitch.set_text(value.into()),
            Field::Twist(i) => self
                .twists
                .get_mut(i)
                .ok_or("Unknown connector")?
                .set_text(value.into()),
            Field::Flipped => {
                self.flipped = value.parse().map_err(|_| "Choose a joint direction")?
            }
        }
        Ok(())
    }
    pub(crate) fn axes(&self) -> Vec<(usize, &'static str)> {
        use JointKindDto::*;
        let mut a = vec![];
        if matches!(
            self.kind,
            Revolute | Cylindrical | Planar | Ball | PinSlot | Screw | Universal
        ) {
            a.push((
                0,
                if self.kind == Screw {
                    "Rotation travel"
                } else {
                    "Primary rotation"
                },
            ));
        }
        if matches!(self.kind, Slider | Cylindrical | Planar | PinSlot) {
            a.push((
                1,
                if matches!(self.kind, Planar | PinSlot) {
                    "X slide"
                } else {
                    "Slide"
                },
            ));
        }
        if matches!(self.kind, Ball | Universal) {
            a.push((2, "Secondary rotation"));
        }
        if self.kind == Ball {
            a.push((3, "Tertiary rotation"));
        }
        if self.kind == Planar {
            a.push((4, "Y slide"));
        }
        a
    }
    pub(crate) fn request(&self, a: &AssemblyDocumentDto) -> Result<(&'static str, Value), String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("Enter a joint name".into());
        }
        if a.joints
            .iter()
            .any(|j| j.name == name && self.original.as_ref().is_none_or(|o| o.id != j.id))
        {
            return Err("Choose a unique joint name".into());
        }
        let ca = self.connectors[0].as_ref().ok_or("Pick connector A")?;
        let cb = self.connectors[1].as_ref().ok_or("Pick connector B")?;
        if ca.occurrence == cb.occurrence {
            return Err("Pick two different component instances".into());
        }
        let parent = |c: &Connector| {
            a.component_structure
                .occurrences
                .iter()
                .find(|o| o.id == c.occurrence)
                .map(|o| o.parent_occurrence_id)
                .ok_or("The selected component no longer exists")
        };
        if parent(ca)? != parent(cb)? {
            return Err("Joint connectors must belong to the same subassembly".into());
        }
        let mut v = [(0., None); 5];
        for (i, _) in self.axes() {
            v[i] = self.coordinates[i].value(self.units)?;
        }
        let pitch = if self.kind == JointKindDto::Screw {
            self.pitch.evaluate(self.units, &[])?
        } else {
            1.
        };
        if pitch <= 0. {
            return Err("Screw pitch must be positive".into());
        }
        let advanced = JointAdvancedDto {
            secondary_angle_offset_deg: v[2].0,
            tertiary_angle_offset_deg: v[3].0,
            secondary_linear_offset_mm: v[4].0,
            screw_pitch_mm_per_revolution: pitch,
            connector_a_twist_deg: self.twists[0].evaluate(self.units, &[])?,
            connector_b_twist_deg: self.twists[1].evaluate(self.units, &[])?,
            secondary_angle_limits: v[2].1,
            tertiary_angle_limits: v[3].1,
            secondary_linear_limits: v[4].1,
            connector_a_occurrence_id: Some(ca.occurrence),
            connector_b_occurrence_id: Some(cb.occurrence),
        };
        let fixed = self
            .fixed
            .filter(|_| self.ground_changed)
            .and_then(|i| self.connectors.get(i))
            .and_then(Option::as_ref);
        let create = CreateJointRequestDto {
            name: name.into(),
            kind: self.kind,
            connector_a: ca.connector.clone(),
            connector_b: cb.connector.clone(),
            flipped: self.flipped,
            angle_offset_deg: v[0].0,
            linear_offset_mm: v[1].0,
            limits: None,
            angle_limits: v[0].1,
            linear_limits: v[1].1,
            advanced,
            grounded_body_id: fixed.map(|c| c.connector.body_id),
            grounded_occurrence_id: fixed.map(|c| c.occurrence),
        };
        if let Some(original) = &self.original {
            let mut fields = serde_json::to_value(&create).map_err(|e| e.to_string())?;
            fields["id"] = serde_json::json!(original.id);
            fields["enabled"] = serde_json::json!(original.enabled);
            let joint = serde_json::from_value(fields).map_err(|e| e.to_string())?;
            Ok((
                "assembly_update_joint",
                serde_json::to_value(UpdateJointRequestDto {
                    joint,
                    grounded_body_id: create.grounded_body_id,
                    grounded_occurrence_id: create.grounded_occurrence_id,
                })
                .map_err(|e| e.to_string())?,
            ))
        } else {
            Ok((
                "assembly_create_joint",
                serde_json::to_value(create).map_err(|e| e.to_string())?,
            ))
        }
    }
}
