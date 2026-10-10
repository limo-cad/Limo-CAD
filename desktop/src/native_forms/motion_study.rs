//! Typed motion-study drafts. Apply validates a clone through the shared model.
use super::{DimensionKind, MeasurementInput};
use limo_cad_core::UnitSystem;
use limo_cad_sketch::{
    AssemblyDocumentDto, JointDefinitionDto, MotionCoordinateDto, MotionDriverDto, MotionDriverId,
    MotionDriverLawDto, MotionInterpolationDto, MotionKeyframeDto, MotionStudyDto,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Edit {
    Name,
    Duration,
    Speed,
    DriverName(u64),
    Motor(u64, usize),
    KeyTime(u64, usize),
    KeyValue(u64, usize),
}
#[derive(Clone)]
pub(crate) struct Keyframe {
    pub time: String,
    pub value: String,
    pub interpolation: MotionInterpolationDto,
}
#[derive(Clone)]
pub(crate) struct Driver {
    pub record: MotionDriverDto,
    pub name: String,
    pub motor: [String; 3],
    pub keys: Vec<Keyframe>,
    pub is_motor: bool,
}
impl Driver {
    pub(crate) fn new(record: MotionDriverDto, duration: f64) -> Self {
        let (motor, keys, is_motor) = match &record.law {
            MotionDriverLawDto::Motor {
                initial_value,
                velocity_per_second,
                acceleration_per_second2,
            } => (
                [
                    *initial_value,
                    *velocity_per_second,
                    *acceleration_per_second2,
                ],
                vec![],
                true,
            ),
            MotionDriverLawDto::Keyframes { keyframes } => (
                [keyframes.first().map_or(0., |k| k.value), 1., 0.],
                keyframes.clone(),
                false,
            ),
        };
        let keys = if keys.is_empty() {
            vec![
                MotionKeyframeDto {
                    time_seconds: 0.,
                    value: motor[0],
                    interpolation: MotionInterpolationDto::Smooth,
                },
                MotionKeyframeDto {
                    time_seconds: duration,
                    value: motor[0],
                    interpolation: MotionInterpolationDto::Smooth,
                },
            ]
        } else {
            keys
        };
        Self {
            name: record.name.clone(),
            record,
            motor: motor.map(|v| v.to_string()),
            keys: keys
                .into_iter()
                .map(|k| Keyframe {
                    time: k.time_seconds.to_string(),
                    value: k.value.to_string(),
                    interpolation: k.interpolation,
                })
                .collect(),
            is_motor,
        }
    }
    pub(crate) fn value(&self, scale: f64) -> Result<MotionDriverDto, String> {
        let mut result = self.record.clone();
        result.name = self.name.trim().into();
        result.law = if self.is_motor {
            MotionDriverLawDto::Motor {
                initial_value: number(&self.motor[0])?,
                velocity_per_second: number(&self.motor[1])?,
                acceleration_per_second2: number(&self.motor[2])?,
            }
        } else {
            let mut keyframes = self
                .keys
                .iter()
                .map(|k| {
                    Ok(MotionKeyframeDto {
                        time_seconds: number(&k.time)? * scale,
                        value: number(&k.value)?,
                        interpolation: k.interpolation,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            keyframes.sort_by(|a, b| a.time_seconds.total_cmp(&b.time_seconds));
            MotionDriverLawDto::Keyframes { keyframes }
        };
        Ok(result)
    }
    pub(crate) fn add_key(&mut self, duration: f64) -> Result<(), String> {
        let mut times = self
            .keys
            .iter()
            .map(|k| number(&k.time))
            .collect::<Result<Vec<_>, _>>()?;
        times.extend([0., duration]);
        times.sort_by(f64::total_cmp);
        let gap = times
            .windows(2)
            .max_by(|a, b| (a[1] - a[0]).total_cmp(&(b[1] - b[0])))
            .ok_or("No room for another keyframe")?;
        if gap[1] - gap[0] <= 1e-9 {
            return Err("No distinct keyframe time is available".into());
        }
        let time = (gap[0] + gap[1]) * 0.5;
        let value = self
            .keys
            .first()
            .map(|k| k.value.clone())
            .unwrap_or_else(|| "0".into());
        self.keys.push(Keyframe {
            time: time.to_string(),
            value,
            interpolation: MotionInterpolationDto::Smooth,
        });
        self.keys.sort_by(|a, b| {
            number(&a.time)
                .unwrap()
                .total_cmp(&number(&b.time).unwrap())
        });
        Ok(())
    }
}

#[derive(Clone)]
pub(crate) struct Form {
    pub original: MotionStudyDto,
    pub name: String,
    pub duration: String,
    pub speed: String,
    pub looped: bool,
    pub drivers: Vec<Driver>,
    pub next_driver_id: u64,
}
impl Form {
    pub(crate) fn new(study: MotionStudyDto) -> Self {
        Self {
            name: study.name.clone(),
            duration: study.duration_seconds.to_string(),
            speed: study.playback_speed.to_string(),
            looped: study.looped,
            drivers: study
                .drivers
                .iter()
                .cloned()
                .map(|d| Driver::new(d, study.duration_seconds))
                .collect(),
            next_driver_id: study.next_driver_id,
            original: study,
        }
    }
    pub(crate) fn driver(&self, id: u64) -> Result<&Driver, String> {
        self.drivers
            .iter()
            .find(|d| d.record.id.0 == id)
            .ok_or_else(|| "Motion driver no longer exists".into())
    }
    pub(crate) fn driver_mut(&mut self, id: u64) -> Result<&mut Driver, String> {
        self.drivers
            .iter_mut()
            .find(|d| d.record.id.0 == id)
            .ok_or_else(|| "Motion driver no longer exists".into())
    }
    pub(crate) fn set(&mut self, edit: Edit, text: String) -> Result<(), String> {
        let field = match edit {
            Edit::Name => &mut self.name,
            Edit::Duration => &mut self.duration,
            Edit::Speed => &mut self.speed,
            Edit::DriverName(id) => &mut self.driver_mut(id)?.name,
            Edit::Motor(id, i) => self
                .driver_mut(id)?
                .motor
                .get_mut(i)
                .ok_or("Unknown motor field")?,
            Edit::KeyTime(id, i) => {
                &mut self
                    .driver_mut(id)?
                    .keys
                    .get_mut(i)
                    .ok_or("Keyframe no longer exists")?
                    .time
            }
            Edit::KeyValue(id, i) => {
                &mut self
                    .driver_mut(id)?
                    .keys
                    .get_mut(i)
                    .ok_or("Keyframe no longer exists")?
                    .value
            }
        };
        *field = text;
        Ok(())
    }
    pub(crate) fn value(&self, a: &AssemblyDocumentDto) -> Result<MotionStudyDto, String> {
        let mut study = self.original.clone();
        study.name = self.name.trim().into();
        study.duration_seconds = number(&self.duration)?;
        study.playback_speed = number(&self.speed)?;
        study.looped = self.looped;
        study.next_driver_id = self.next_driver_id;
        let scale = study.duration_seconds / self.original.duration_seconds;
        study.drivers = self
            .drivers
            .iter()
            .map(|d| d.value(scale))
            .collect::<Result<_, _>>()?;
        a.clone().update_motion_study(study.clone())?;
        Ok(study)
    }
    pub(crate) fn add_driver(&mut self, a: &AssemblyDocumentDto) -> Result<(), String> {
        let (joint, coordinate) = a
            .joints
            .iter()
            .filter(|j| j.enabled)
            .find_map(|j| {
                coordinates(a, j)
                    .into_iter()
                    .find(|(c, _)| {
                        !self
                            .drivers
                            .iter()
                            .any(|d| d.record.joint_id == j.id && d.record.coordinate == *c)
                    })
                    .map(|(c, _)| (j, c))
            })
            .ok_or("All available joint coordinates already have drivers")?;
        let id = self.next_driver_id;
        self.next_driver_id = id.checked_add(1).ok_or("Motion driver ids exhausted")?;
        let initial = match coordinate {
            MotionCoordinateDto::PrimaryAngle => joint.angle_offset_deg,
            MotionCoordinateDto::PrimaryLinear => joint.linear_offset_mm,
            MotionCoordinateDto::SecondaryAngle => joint.advanced.secondary_angle_offset_deg,
            MotionCoordinateDto::TertiaryAngle => joint.advanced.tertiary_angle_offset_deg,
            MotionCoordinateDto::SecondaryLinear => joint.advanced.secondary_linear_offset_mm,
        };
        let record = MotionDriverDto {
            id: MotionDriverId(id),
            name: format!("Driver {id}"),
            joint_id: joint.id,
            coordinate,
            enabled: true,
            law: MotionDriverLawDto::Keyframes {
                keyframes: vec![
                    MotionKeyframeDto {
                        time_seconds: 0.,
                        value: initial,
                        interpolation: MotionInterpolationDto::Smooth,
                    },
                    MotionKeyframeDto {
                        time_seconds: self.original.duration_seconds,
                        value: initial,
                        interpolation: MotionInterpolationDto::Smooth,
                    },
                ],
            },
        };
        self.drivers
            .push(Driver::new(record, self.original.duration_seconds));
        Ok(())
    }
}
pub(crate) fn number(text: &str) -> Result<f64, String> {
    let mut value = MeasurementInput::new(DimensionKind::Unitless, 0., UnitSystem::Mm);
    value.set_text(text.into());
    value.evaluate(UnitSystem::Mm, &[])
}
pub(crate) fn coordinates(
    a: &AssemblyDocumentDto,
    joint: &JointDefinitionDto,
) -> Vec<(MotionCoordinateDto, &'static str)> {
    super::joint::Form::new(a, Some(joint.clone()), UnitSystem::Mm)
        .axes()
        .into_iter()
        .map(|(index, label)| {
            (
                match index {
                    0 => MotionCoordinateDto::PrimaryAngle,
                    1 => MotionCoordinateDto::PrimaryLinear,
                    2 => MotionCoordinateDto::SecondaryAngle,
                    3 => MotionCoordinateDto::TertiaryAngle,
                    _ => MotionCoordinateDto::SecondaryLinear,
                },
                label,
            )
        })
        .collect()
}
