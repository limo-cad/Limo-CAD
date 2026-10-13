//! Print-only primitives in a stable source body's local CAD coordinates.
use crate::{BodyId, PrintSettingsDto};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintLocalPoseDto {
    pub translation_mm: [f64; 3],
    /// Unit quaternion [x, y, z, w], composed after the resolved parent occurrence pose.
    pub rotation: [f64; 4],
}
impl Default for PrintLocalPoseDto {
    fn default() -> Self {
        Self {
            translation_mm: [0.; 3],
            rotation: [0., 0., 0., 1.],
        }
    }
}
impl PrintLocalPoseDto {
    pub fn validate(&self) -> Result<(), String> {
        if self
            .translation_mm
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
            || self.rotation.iter().any(|v| !v.is_finite())
            || (self.rotation.iter().map(|v| v * v).sum::<f64>() - 1.).abs() > 1e-6
        {
            return Err(
                "Print modifier pose needs finite local coordinates and a unit quaternion".into(),
            );
        }
        Ok(())
    }
}

/// Box and cylinder origins are their centers. Cylinder local Z is its height axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PrintModifierPrimitiveDto {
    Box { size_mm: [f64; 3] },
    Cylinder { radius_mm: f64, height_mm: f64 },
}
impl PrintModifierPrimitiveDto {
    pub fn validate(&self) -> Result<(), String> {
        let valid = |v: f64| v.is_finite() && v > 0. && v <= 1_000_000.;
        let okay = match self {
            Self::Box { size_mm } => size_mm.iter().copied().all(valid),
            Self::Cylinder {
                radius_mm,
                height_mm,
            } => valid(*radius_mm) && valid(*height_mm),
        };
        if !okay {
            return Err(
                "Modifier dimensions must be finite positive millimeters, at most 1,000,000".into(),
            );
        }
        Ok(())
    }
}

/// Definition-level zone repeated deliberately for every occurrence; no physical body is allocated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintModifierDto {
    pub id: String,
    pub name: String,
    pub body_id: BodyId,
    pub enabled: bool,
    #[serde(default)]
    pub local_pose: PrintLocalPoseDto,
    pub primitive: PrintModifierPrimitiveDto,
    pub settings: PrintSettingsDto,
}
impl PrintModifierDto {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.len() != 36
            || !self.id.bytes().enumerate().all(|(i, v)| {
                if [8, 13, 18, 23].contains(&i) {
                    v == b'-'
                } else {
                    v.is_ascii_hexdigit()
                }
            })
        {
            return Err("Modifier identity must be a stable UUID".into());
        }
        if self.name.trim().is_empty()
            || self.name.len() > 256
            || self.name.chars().any(char::is_control)
        {
            return Err("Modifier names require 1–256 printable bytes".into());
        }
        if self.body_id.0 == 0 || self.body_id.0 >= 9_007_199_254_740_991 {
            return Err("Modifier needs a valid source body identity".into());
        }
        self.local_pose.validate()?;
        self.primitive.validate()?;
        self.settings.validate()
    }
}

/// Conservative bounds in the source body's definition frame, used for overlap review.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PrintModifierBoundsDto {
    pub min_mm: [f64; 3],
    pub max_mm: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrintModifierEffectiveDto {
    pub modifier: PrintModifierDto,
    pub binding: crate::PrintPartBindingDto,
    /// Deliberate assembly repeats inherit this definition-level zone.
    pub occurrence_ids: Vec<u64>,
    pub local_bounds: PrintModifierBoundsDto,
    pub settings: PrintSettingsDto,
    pub sources: crate::PrintSettingSourcesDto,
    pub unsupported: Vec<crate::PrintSettingFieldDto>,
    pub warnings: Vec<String>,
}
impl PrintModifierBoundsDto {
    pub fn overlaps(self, other: Self) -> bool {
        (0..3).all(|axis| {
            self.min_mm[axis] < other.max_mm[axis] - 1e-7
                && other.min_mm[axis] < self.max_mm[axis] - 1e-7
        })
    }
}

pub fn print_modifier_local_bounds(
    modifier: &PrintModifierDto,
) -> Result<PrintModifierBoundsDto, String> {
    modifier.validate()?;
    let [x, y, z, w] = modifier.local_pose.rotation;
    let rotation = [
        [
            1. - 2. * (y * y + z * z),
            2. * (x * y - z * w),
            2. * (x * z + y * w),
        ],
        [
            2. * (x * y + z * w),
            1. - 2. * (x * x + z * z),
            2. * (y * z - x * w),
        ],
        [
            2. * (x * z - y * w),
            2. * (y * z + x * w),
            1. - 2. * (x * x + y * y),
        ],
    ];
    let half: [f64; 3] = std::array::from_fn(|axis| match modifier.primitive {
        PrintModifierPrimitiveDto::Box { size_mm } => rotation[axis]
            .iter()
            .zip(size_mm)
            .map(|(r, size)| r.abs() * size * 0.5)
            .sum(),
        PrintModifierPrimitiveDto::Cylinder {
            radius_mm,
            height_mm,
        } => {
            radius_mm * rotation[axis][0].hypot(rotation[axis][1])
                + height_mm * 0.5 * rotation[axis][2].abs()
        }
    });
    Ok(PrintModifierBoundsDto {
        min_mm: std::array::from_fn(|axis| modifier.local_pose.translation_mm[axis] - half[axis]),
        max_mm: std::array::from_fn(|axis| modifier.local_pose.translation_mm[axis] + half[axis]),
    })
}

/// Block conflicting overlapping definitions until target overlap priority is qualified.
/// The AABB check is conservative: rotated zones can require review even if their exact
/// intersections are disjoint. Identical requests never add wall or shell counts together.
pub fn validate_print_modifiers(modifiers: &[PrintModifierDto]) -> Result<(), String> {
    if modifiers.len() > 256 {
        return Err("A document supports at most 256 print modifier definitions".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for modifier in modifiers {
        modifier.validate()?;
        if !ids.insert(modifier.id.to_ascii_lowercase()) {
            return Err(format!("Duplicate print modifier identity {}", modifier.id));
        }
    }
    for (index, left) in modifiers.iter().enumerate().filter(|(_, modifier)| {
        modifier.enabled && !crate::configured_print_fields(&modifier.settings).is_empty()
    }) {
        let left_bounds = print_modifier_local_bounds(left)?;
        for right in modifiers.iter().skip(index + 1).filter(|right| {
            right.enabled && !crate::configured_print_fields(&right.settings).is_empty()
        }) {
            if left.body_id == right.body_id
                && left.settings != right.settings
                && left_bounds.overlaps(print_modifier_local_bounds(right)?)
            {
                return Err(format!("Print modifier bounds '{}' and '{}' overlap with different requested settings; separate or disable a zone", left.name, right.name));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modifiers_reject_unknown_mechanical_or_slicer_fields_and_invalid_poses() {
        let mut modifier = PrintModifierDto {
            id: "01234567-89ab-4cde-8123-456789abcdef".into(),
            name: "drive reinforcement".into(),
            body_id: BodyId(7),
            enabled: true,
            local_pose: Default::default(),
            primitive: PrintModifierPrimitiveDto::Box {
                size_mm: [10., 10., 20.],
            },
            settings: PrintSettingsDto {
                infill_density_percent: Some(0.),
                ..Default::default()
            },
        };
        assert!(modifier.validate().is_ok());
        let mut json = serde_json::to_value(&modifier).unwrap();
        json["settings"]["nozzle_temperature"] = serde_json::json!(255);
        assert!(serde_json::from_value::<PrintModifierDto>(json).is_err());
        modifier.local_pose.rotation = [0.; 4];
        assert!(modifier.validate().is_err());
        modifier.local_pose = Default::default();
        modifier.primitive = PrintModifierPrimitiveDto::Cylinder {
            radius_mm: f64::NAN,
            height_mm: 10.,
        };
        assert!(modifier.validate().is_err());
        modifier.primitive = PrintModifierPrimitiveDto::Box {
            size_mm: [0., 10., 10.],
        };
        assert!(modifier.validate().is_err());
    }
    #[test]
    fn rotated_bounds_and_conservative_conflicts_preserve_reset_and_disabled_zones() {
        let mut left = PrintModifierDto {
            id: "01234567-89ab-4cde-8123-456789abcdef".into(),
            name: "Left".into(),
            body_id: BodyId(1),
            enabled: true,
            local_pose: PrintLocalPoseDto {
                translation_mm: [10., 0., 0.],
                rotation: [
                    0.,
                    0.,
                    std::f64::consts::FRAC_1_SQRT_2,
                    std::f64::consts::FRAC_1_SQRT_2,
                ],
            },
            primitive: PrintModifierPrimitiveDto::Box {
                size_mm: [2., 4., 6.],
            },
            settings: PrintSettingsDto {
                wall_count: Some(6),
                ..Default::default()
            },
        };
        let bounds = print_modifier_local_bounds(&left).unwrap();
        for (actual, expected) in bounds
            .min_mm
            .into_iter()
            .chain(bounds.max_mm)
            .zip([8., -1., -3., 12., 1., 3.])
        {
            assert!((actual - expected).abs() < 1e-8);
        }
        let mut right = left.clone();
        right.id = "abcdef01-89ab-4cde-8123-456789abcdef".into();
        right.name = "Right".into();
        right.settings.infill_density_percent = Some(80.);
        assert!(validate_print_modifiers(&[left.clone(), right.clone()]).is_err());
        right.settings = PrintSettingsDto::default();
        validate_print_modifiers(&[left.clone(), right.clone()]).unwrap();
        right.settings.wall_count = Some(8);
        right.enabled = false;
        validate_print_modifiers(&[left.clone(), right]).unwrap();
        left.primitive = PrintModifierPrimitiveDto::Cylinder {
            radius_mm: 2.,
            height_mm: 10.,
        };
        left.local_pose.translation_mm = [5., 8., 0.];
        left.local_pose.rotation = [
            0.,
            std::f64::consts::FRAC_1_SQRT_2,
            0.,
            std::f64::consts::FRAC_1_SQRT_2,
        ];
        let bounds = print_modifier_local_bounds(&left).unwrap();
        for (actual, expected) in bounds
            .min_mm
            .into_iter()
            .chain(bounds.max_mm)
            .zip([0., 6., -2., 10., 10., 2.])
        {
            assert!((actual - expected).abs() < 1e-8);
        }
        left.local_pose.rotation = [1e308, 0., 0., 1e308];
        assert!(left.validate().is_err());
    }
}
