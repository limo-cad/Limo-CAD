//! External threads retain the shared request, standards data and kernel validation.
use super::thread_sizes::presets;
use super::*;
use limo_cad_core::UnitSystem;
use limo_cad_solid::{
    BodyFeatureDefinitionDto, CylindricalSurfaceDto, ExternalThreadRequest, HoleThreadDto,
    HoleThreadHand, HoleThreadRepresentation, HoleThreadSeries, HoleThreadStandard,
    RoundedThreadProfile,
};

#[derive(Debug, Clone)]
pub(super) struct ThreadSurface {
    pub face: PlanarFaceSourceDto,
    pub cylinder: CylindricalSurfaceDto,
    pub bounds: [f64; 2],
    pub body_name: String,
}

impl ThreadSurface {
    fn resolve(face: PlanarFaceSourceDto, model: &FormModel<'_>) -> Result<Self, String> {
        let body = model
            .scene
            .bodies
            .iter()
            .find(|b| b.id == face.body_id)
            .ok_or("The selected body no longer exists")?;
        let surface = body
            .faces
            .iter()
            .find(|f| f.id == face.face_id)
            .ok_or("The selected face no longer exists")?;
        let cylinder = surface
            .cylinder
            .ok_or("Select an exterior cylindrical face")?;
        let axis = [cylinder.axis.x, cylinder.axis.y, cylinder.axis.z];
        let origin = [cylinder.origin.x, cylinder.origin.y, cylinder.origin.z];
        if !axis.iter().chain(origin.iter()).all(|v| v.is_finite())
            || !cylinder.radius.is_finite()
            || cylinder.radius <= 0.
            || (axis.iter().map(|v| v * v).sum::<f64>() - 1.).abs() > 1e-5
        {
            return Err("The cylindrical surface is invalid".into());
        }
        let mut bounds = [f64::INFINITY, f64::NEG_INFINITY];
        let mut score = 0.;
        let mut samples = 0;
        let start = surface.first_index as usize;
        let end = start
            .checked_add(surface.index_count as usize)
            .ok_or("The face is too large")?;
        for &index in body
            .mesh
            .indices
            .get(start..end)
            .ok_or("The face mesh is incomplete")?
        {
            let i = (index as usize)
                .checked_mul(3)
                .ok_or("The face mesh is too large")?;
            let p = body
                .mesh
                .positions
                .get(i..i + 3)
                .ok_or("The face mesh is incomplete")?;
            let n = body
                .mesh
                .normals
                .get(i..i + 3)
                .ok_or("The face normals are incomplete")?;
            if p.iter().chain(n).any(|v| !v.is_finite()) {
                return Err("The face mesh is invalid".into());
            }
            let delta: [f64; 3] = std::array::from_fn(|j| p[j] as f64 - origin[j]);
            let z = (0..3).map(|j| delta[j] * axis[j]).sum::<f64>();
            bounds[0] = bounds[0].min(z);
            bounds[1] = bounds[1].max(z);
            let radial: [f64; 3] = std::array::from_fn(|j| delta[j] - z * axis[j]);
            let rn = radial.iter().map(|v| v * v).sum::<f64>().sqrt()
                * n.iter().map(|v| (*v as f64).powi(2)).sum::<f64>().sqrt();
            if rn > 1e-12 {
                score += (0..3).map(|j| radial[j] * n[j] as f64).sum::<f64>() / rn;
                samples += 1;
            }
        }
        if samples == 0 || score / samples as f64 <= 0.1 {
            return Err(
                "Select an exterior shaft; internal hole walls are not external threads".into(),
            );
        }
        if !bounds.iter().all(|v| v.is_finite()) || bounds[1] - bounds[0] <= 1e-7 {
            return Err("The selected shaft has no axial length".into());
        }
        Ok(Self {
            face,
            cylinder,
            bounds,
            body_name: body.name.clone(),
        })
    }
}

#[derive(Debug)]
pub(super) struct ThreadFields {
    pub surface: Option<ThreadSurface>,
    external: bool,
    standard: HoleThreadStandard,
    series: HoleThreadSeries,
    preset: String,
    diameter: MeasurementInput,
    pitch: MeasurementInput,
    class: String,
    designation: String,
    hand: HoleThreadHand,
    representation: HoleThreadRepresentation,
    full: bool,
    depth: MeasurementInput,
    flip: bool,
    rounded: [MeasurementInput; 4],
}

fn length(value: f64, units: UnitSystem) -> MeasurementInput {
    MeasurementInput::new(DimensionKind::Length, value, units)
}
fn key<T: serde::Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .unwrap()
        .as_str()
        .unwrap()
        .into()
}
fn choice<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, String> {
    serde_json::from_value(json!(value)).map_err(|_| "Choose an available thread option".into())
}
impl ThreadFields {
    pub fn new(units: UnitSystem) -> Self {
        let preset = presets()
            .iter()
            .find(|p| p.id == "metric_coarse-6-1")
            .unwrap();
        Self::from_thread(preset.external(), false, units, true)
    }
    pub fn new_internal(units: UnitSystem) -> Self {
        let preset = presets()
            .iter()
            .find(|p| p.id == "metric_coarse-6-1")
            .unwrap();
        let mut fields = Self::from_thread(preset.thread.clone(), false, units, false);
        fields.representation = HoleThreadRepresentation::Modeled;
        fields.depth = length(8., units);
        fields
    }
    pub fn from_thread(t: HoleThreadDto, flip: bool, units: UnitSystem, external: bool) -> Self {
        let preset = presets()
            .iter()
            .find(|p| {
                let v = if external {
                    p.external()
                } else {
                    p.thread.clone()
                };
                v.standard == t.standard
                    && v.series == t.series
                    && v.nominal_diameter == t.nominal_diameter
                    && v.pitch == t.pitch
                    && v.class == t.class
                    && v.designation == t.designation
            })
            .map(|p| p.id.clone())
            .unwrap_or_else(|| "custom".into());
        let r = t.rounded_profile.unwrap_or(RoundedThreadProfile {
            radial_depth: t.pitch * 0.5,
            corner_radius: t.pitch * 0.075,
            radial_clearance: t.pitch * 0.0625,
            axial_clearance: t.pitch * 0.05,
        });
        Self {
            surface: None,
            external,
            standard: t.standard,
            series: t.series,
            preset,
            diameter: length(t.nominal_diameter, units),
            pitch: length(t.pitch, units),
            class: t.class,
            designation: t.designation,
            hand: t.hand,
            representation: t.representation,
            full: t.depth.is_none(),
            depth: length(t.depth.unwrap_or(10.), units),
            flip,
            rounded: [
                r.radial_depth,
                r.corner_radius,
                r.radial_clearance,
                r.axial_clearance,
            ]
            .map(|v| length(v, units)),
        }
    }
    fn use_preset(&mut self, id: &str, units: UnitSystem) -> Result<(), String> {
        let p = presets()
            .iter()
            .find(|p| p.id == id)
            .ok_or("Choose an available thread size")?;
        let t = if self.external {
            p.external()
        } else {
            p.thread.clone()
        };
        self.standard = t.standard;
        self.series = t.series;
        self.preset = p.id.clone();
        self.diameter = length(t.nominal_diameter, units);
        self.pitch = length(t.pitch, units);
        self.class = t.class;
        self.designation = t.designation;
        Ok(())
    }
    fn match_diameter(&mut self, diameter: f64, current_series_only: bool, units: UnitSystem) {
        if !self.external {
            let id = presets()
                .iter()
                .find(|p| p.thread.series == self.series)
                .ok_or("No sizes in the selected series")
                .unwrap()
                .id
                .clone();
            self.use_preset(&id, units).unwrap();
            return;
        }
        let matches = |p: &&super::thread_sizes::Preset| {
            (p.thread.nominal_diameter - diameter).abs() <= (diameter.abs() * 0.002).max(0.01)
        };
        let preset = presets()
            .iter()
            .filter(matches)
            .find(|p| p.thread.series == self.series)
            .or_else(|| {
                (!current_series_only)
                    .then(|| {
                        presets()
                            .iter()
                            .filter(matches)
                            .find(|p| p.thread.series == HoleThreadSeries::MetricCoarse)
                            .or_else(|| presets().iter().find(matches))
                    })
                    .flatten()
            });
        if let Some(p) = preset {
            self.use_preset(&p.id, units).unwrap();
        } else {
            self.preset = "custom".into();
            self.diameter = length(diameter, units);
            self.class = if self.standard == HoleThreadStandard::IsoMetric {
                "6g"
            } else {
                "2A"
            }
            .into();
            self.designation = format!("Custom Ø{diameter:.3} mm - {}", self.class);
        }
    }
    pub fn set(
        &mut self,
        field: SolidField,
        value: &str,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        use SolidField::*;
        let units = model.document.settings.units;
        match field {
            ThreadStandard => {
                self.standard = choice(value)?;
                if self.standard == HoleThreadStandard::CustomTrapezoidal {
                    self.series = HoleThreadSeries::Rounded;
                    self.preset = "custom".into();
                    self.class = "custom".into();
                    self.designation = "Custom rounded trapezoidal".into();
                    self.representation = HoleThreadRepresentation::Modeled;
                    let pitch = self
                        .pitch
                        .evaluate(units, model.parameters)
                        .ok()
                        .filter(|p| *p > 0.)
                        .unwrap_or(1.);
                    self.pitch = length(pitch, units);
                    self.rounded = [0.5, 0.075, 0.0625, 0.05].map(|v| length(v * pitch, units));
                    if let Some(s) = &self.surface {
                        self.diameter = length(s.cylinder.radius * 2., units);
                    }
                } else {
                    self.series = if self.standard == HoleThreadStandard::IsoMetric {
                        HoleThreadSeries::MetricCoarse
                    } else {
                        HoleThreadSeries::Unc
                    };
                    let d = self
                        .surface
                        .as_ref()
                        .map(|s| s.cylinder.radius * 2.)
                        .unwrap_or(
                            self.diameter
                                .evaluate(units, model.parameters)
                                .unwrap_or(6.),
                        );
                    self.match_diameter(d, true, units);
                }
            }
            ThreadSeries => {
                let series = choice(value)?;
                if !matches!(
                    (self.standard, series),
                    (
                        HoleThreadStandard::IsoMetric,
                        HoleThreadSeries::MetricCoarse | HoleThreadSeries::MetricFine
                    ) | (
                        HoleThreadStandard::UnifiedInch,
                        HoleThreadSeries::Unc | HoleThreadSeries::Unf
                    )
                ) {
                    return Err("The series must match the thread standard".into());
                }
                self.series = series;
                let d = self
                    .surface
                    .as_ref()
                    .map(|s| s.cylinder.radius * 2.)
                    .unwrap_or(
                        self.diameter
                            .evaluate(units, model.parameters)
                            .unwrap_or(6.),
                    );
                self.match_diameter(d, true, units);
            }
            ThreadPreset if value == "custom" => self.preset = value.into(),
            ThreadPreset => {
                if !presets()
                    .iter()
                    .any(|p| p.id == value && p.thread.series == self.series)
                {
                    return Err("Choose a size from the selected series".into());
                }
                self.use_preset(value, units)?;
            }
            Diameter => self.diameter.set_text(value.into()),
            Pitch => self.pitch.set_text(value.into()),
            ThreadClass => self.class = value.into(),
            Designation => self.designation = value.into(),
            ThreadHand => self.hand = choice(value)?,
            Representation => self.representation = choice(value)?,
            FullThread => {
                self.full = value
                    .parse()
                    .map_err(|_| "Full thread expects true or false")?
            }
            Distance => self.depth.set_text(value.into()),
            Flip => self.flip = value.parse().map_err(|_| "Flip expects true or false")?,
            RadialDepth => self.rounded[0].set_text(value.into()),
            CornerRadius => self.rounded[1].set_text(value.into()),
            RadialClearance => self.rounded[2].set_text(value.into()),
            AxialClearance => self.rounded[3].set_text(value.into()),
            _ => return Err("This thread field is not editable".into()),
        }
        Ok(())
    }
}

impl SolidForm {
    pub(crate) fn thread_guide(
        &self,
        model: &FormModel<'_>,
    ) -> Option<(CylindricalSurfaceDto, [f64; 2])> {
        let fields = self.thread.as_ref()?;
        let request = self.thread_request(model).ok()?;
        let surface = fields.surface.as_ref()?;
        let [min, max] = surface.bounds;
        let length = request.thread.depth.unwrap_or(max - min);
        Some((
            surface.cylinder,
            if request.flip {
                [max, max - length]
            } else {
                [min, min + length]
            },
        ))
    }
    pub(crate) fn set_thread_face(
        &mut self,
        face: Option<PlanarFaceSourceDto>,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        let surface = face.map(|f| ThreadSurface::resolve(f, model)).transpose()?;
        let f = self
            .thread
            .as_mut()
            .ok_or("This feature has no cylindrical surface")?;
        if let Some(s) = &surface {
            if f.surface.as_ref().is_none_or(|old| old.face != s.face) {
                let units = model.document.settings.units;
                if f.standard == HoleThreadStandard::CustomTrapezoidal {
                    f.diameter = length(s.cylinder.radius * 2., units);
                } else {
                    f.match_diameter(s.cylinder.radius * 2., false, units);
                }
                f.depth = length(s.bounds[1] - s.bounds[0], units);
            }
        }
        f.surface = surface;
        self.changed();
        Ok(())
    }
    pub(crate) fn thread_face(&self) -> Option<PlanarFaceSourceDto> {
        Some(self.thread.as_ref()?.surface.as_ref()?.face)
    }
    pub(crate) fn edit_thread(definition: &Value, model: &FormModel<'_>) -> Result<Self, String> {
        let BodyFeatureDefinitionDto::ExternalThread {
            feature_id,
            body_id,
            face_id,
            thread,
            flip,
            ..
        } = serde_json::from_value(definition.clone()).map_err(|e| e.to_string())?
        else {
            return Err("The selected feature is not an External Thread".into());
        };
        let mut form = Self::new_kind(SolidFormKind::ExternalThread, model);
        form.feature = Some(feature_id);
        let mut fields =
            ThreadFields::from_thread(thread, flip, model.document.settings.units, true);
        fields.surface = Some(ThreadSurface::resolve(
            PlanarFaceSourceDto { body_id, face_id },
            model,
        )?);
        form.thread = Some(fields);
        form.thread_request(model).map_err(first_error)?;
        Ok(form)
    }
    fn thread_request(
        &self,
        model: &FormModel<'_>,
    ) -> Result<ExternalThreadRequest, Vec<(SolidField, String)>> {
        use SolidField as F;
        let f = self.thread.as_ref().unwrap();
        let mut issues = Vec::new();
        if let Err(e) = self.check_model(model) {
            issues.push((F::Cylinder, e));
        }
        if self.feature.is_some_and(|id| {
            !model
                .document
                .features
                .iter()
                .any(|v| v.id == id && v.kind == FeatureKind::ExternalThread)
        }) {
            issues.push((F::Cylinder, "The edited thread no longer exists".into()));
        }
        let thread = match f.evaluate(model) {
            Ok(value) => value,
            Err(errors) => {
                issues.extend(errors);
                return Err(issues);
            }
        };
        let d = thread.nominal_diameter;
        let depth = thread.depth;
        if let Err(e) = limo_cad_solid::validate_external_thread(
            &thread,
            f.surface
                .as_ref()
                .map(|s| s.cylinder.radius * 2.)
                .unwrap_or(d),
        ) {
            issues.push((F::ThreadPreset, e.to_string()));
        }
        if let Some(s) = &f.surface {
            if !model
                .scene
                .bodies
                .iter()
                .any(|b| b.id == s.face.body_id && b.faces.iter().any(|v| v.id == s.face.face_id))
            {
                issues.push((
                    F::Cylinder,
                    "The selected cylindrical face no longer exists".into(),
                ));
            }
            if depth.is_some_and(|v| v > s.bounds[1] - s.bounds[0] + 1e-5) {
                issues.push((
                    F::Distance,
                    "Thread length exceeds the selected shaft".into(),
                ));
            }
        } else {
            issues.push((F::Cylinder, "Select an exterior cylindrical face".into()));
        }
        if !issues.is_empty() {
            return Err(issues);
        }
        let s = f.surface.as_ref().unwrap();
        Ok(ExternalThreadRequest {
            body_id: s.face.body_id,
            face_id: s.face.face_id,
            thread,
            flip: f.flip,
        })
    }
    pub(super) fn thread_payload(
        &self,
        model: &FormModel<'_>,
    ) -> Result<(&'static str, Value), Vec<(SolidField, String)>> {
        let request = self.thread_request(model)?;
        Ok(if let Some(feature_id) = self.feature {
            (
                "solid_edit_external_thread",
                json!({"feature_id": feature_id,"request": request}),
            )
        } else {
            ("solid_external_thread", json!(request))
        })
    }
    pub(super) fn thread_fields(&self, model: &FormModel<'_>) -> Vec<SolidFieldView> {
        self.thread.as_ref().unwrap().fields(
            &self.thread_request(model).err().unwrap_or_default(),
            self.phase == Phase::Editing && self.check_model(model).is_ok(),
        )
    }
    pub(crate) fn feature_notes(&self) -> Vec<String> {
        if let Some(f) = &self.thread {
            f.notes()
        } else if let Some(paths) = &self.paths {
            paths.notes()
        } else {
            self.hole_notes()
        }
    }
}

impl ThreadFields {
    pub fn preset_drill(&self) -> Option<f64> {
        presets()
            .iter()
            .find(|p| p.id == self.preset)
            .map(|p| p.drill)
    }
    pub fn evaluate(
        &self,
        model: &FormModel<'_>,
    ) -> Result<HoleThreadDto, Vec<(SolidField, String)>> {
        use SolidField as F;
        let f = self;
        let mut issues = Vec::new();
        let mut number = |field, v: &MeasurementInput| match v
            .evaluate(model.document.settings.units, model.parameters)
        {
            Ok(v) => v,
            Err(e) => {
                issues.push((field, e));
                0.
            }
        };
        let d = number(F::Diameter, &f.diameter);
        let p = number(F::Pitch, &f.pitch);
        let depth = (!f.full).then(|| number(F::Distance, &f.depth));
        let rounded_profile =
            (f.standard == HoleThreadStandard::CustomTrapezoidal).then(|| RoundedThreadProfile {
                radial_depth: number(F::RadialDepth, &f.rounded[0]),
                corner_radius: number(F::CornerRadius, &f.rounded[1]),
                radial_clearance: number(F::RadialClearance, &f.rounded[2]),
                axial_clearance: number(F::AxialClearance, &f.rounded[3]),
            });
        if rounded_profile
            .as_ref()
            .is_some_and(|profile| !profile.radial_depth.is_finite() || profile.radial_depth <= 0.0)
            && !issues.iter().any(|(field, _)| *field == F::RadialDepth)
        {
            issues.push((
                F::RadialDepth,
                "Radial depth must be greater than zero".into(),
            ));
        }
        let thread = HoleThreadDto {
            standard: f.standard,
            series: f.series,
            designation: f.designation.trim().into(),
            class: f.class.trim().into(),
            nominal_diameter: d,
            pitch: p,
            threads_per_inch: (f.standard == HoleThreadStandard::UnifiedInch).then_some(25.4 / p),
            hand: f.hand,
            depth,
            representation: f.representation,
            tap_drill_designation: (!f.external)
                .then(|| {
                    presets()
                        .iter()
                        .find(|p| p.id == f.preset)
                        .and_then(|p| p.thread.tap_drill_designation.clone())
                })
                .flatten(),
            rounded_profile,
        };
        if issues.is_empty() {
            Ok(thread)
        } else {
            Err(issues)
        }
    }
    pub fn fields(&self, errors: &[(SolidField, String)], editable: bool) -> Vec<SolidFieldView> {
        use SolidField as F;
        let f = self;
        let custom = f.preset == "custom";
        let rounded = f.standard == HoleThreadStandard::CustomTrapezoidal;
        let text = |v: &str| Field::Text {
            value: v.into(),
            read_only: false,
            selection: None,
        };
        let choices = |value: String, options: Vec<(String, String)>| Field::Choice {
            value,
            options: options
                .into_iter()
                .map(|(value, label)| ChoiceOption {
                    value,
                    label,
                    disabled: false,
                })
                .collect(),
        };
        let pairs = |values: &[(&str, &str)]| {
            values
                .iter()
                .map(|(v, l)| (v.to_string(), l.to_string()))
                .collect()
        };
        let mut sizes: Vec<_> = presets()
            .iter()
            .filter(|p| p.thread.series == f.series)
            .map(|p| {
                (
                    p.id.clone(),
                    if f.external {
                        p.external().designation
                    } else {
                        p.thread.designation.clone()
                    },
                )
            })
            .collect();
        sizes.push((
            "custom".into(),
            if f.external {
                "Custom shaft"
            } else {
                "Custom thread"
            }
            .into(),
        ));
        let mut rows = vec![
            (
                F::Cylinder,
                f.surface
                    .as_ref()
                    .map(|s| format!("{} · Ø{:.3} mm", s.body_name, s.cylinder.radius * 2.))
                    .unwrap_or_else(|| "Click an exterior cylindrical face".into()),
                Field::None,
                true,
                true,
            ),
            (
                F::ThreadStandard,
                if f.external {
                    "Standard"
                } else {
                    "Thread standard"
                }
                .into(),
                choices(
                    key(f.standard),
                    pairs(&[
                        ("iso_metric", "ISO metric"),
                        ("unified_inch", "Unified inch"),
                        ("custom_trapezoidal", "Trapezoidal"),
                    ]),
                ),
                true,
                true,
            ),
            (
                F::ThreadSeries,
                "Series".into(),
                choices(
                    key(f.series),
                    pairs(if rounded {
                        &[("rounded", "Rounded profile")]
                    } else if f.standard == HoleThreadStandard::IsoMetric {
                        &[
                            ("metric_coarse", "Metric coarse"),
                            ("metric_fine", "Metric fine"),
                        ]
                    } else {
                        &[("unc", "UNC"), ("unf", "UNF")]
                    }),
                ),
                true,
                !rounded,
            ),
            (
                F::ThreadPreset,
                if f.external {
                    "Size and pitch"
                } else {
                    "Thread size"
                }
                .into(),
                choices(f.preset.clone(), sizes),
                true,
                true,
            ),
            (
                F::Diameter,
                "Major diameter".into(),
                text(f.diameter.text()),
                custom,
                true,
            ),
            (F::Pitch, "Pitch".into(), text(f.pitch.text()), custom, true),
            (
                F::ThreadClass,
                if f.external {
                    "Tolerance class"
                } else {
                    "Internal class"
                }
                .into(),
                text(&f.class),
                custom,
                !rounded,
            ),
            (
                F::Designation,
                "Designation".into(),
                text(&f.designation),
                custom,
                true,
            ),
        ];
        for (field, label, i) in [
            (F::RadialDepth, "Radial depth", 0),
            (F::CornerRadius, "Corner radius", 1),
            (F::RadialClearance, "Radial clearance", 2),
            (F::AxialClearance, "Axial clearance", 3),
        ] {
            rows.push((
                field,
                label.into(),
                text(f.rounded[i].text()),
                rounded,
                true,
            ));
        }
        rows.extend([
            (
                F::ThreadHand,
                "Hand".into(),
                choices(
                    key(f.hand),
                    pairs(&[("right", "Right-hand"), ("left", "Left-hand")]),
                ),
                true,
                true,
            ),
            (
                F::Representation,
                if f.external {
                    "Representation"
                } else {
                    "Geometry"
                }
                .into(),
                choices(
                    key(f.representation),
                    pairs(&[("simplified", "Cosmetic"), ("modeled", "Modeled")]),
                ),
                true,
                true,
            ),
            (
                F::FullThread,
                if f.external {
                    "Thread the full cylindrical surface"
                } else {
                    "Thread full cylindrical hole depth"
                }
                .into(),
                Field::Toggle(f.full),
                true,
                true,
            ),
            (
                F::Distance,
                if f.external {
                    "Thread length"
                } else {
                    "Thread depth"
                }
                .into(),
                text(f.depth.text()),
                !f.full,
                true,
            ),
            (
                F::Flip,
                "Flip thread start to the opposite end".into(),
                Field::Toggle(f.flip),
                true,
                true,
            ),
        ]);
        rows.into_iter()
            .map(|(field, label, value, visible, enabled)| SolidFieldView {
                field,
                label,
                value,
                visible,
                enabled: editable && enabled,
                error: errors
                    .iter()
                    .find(|(f, _)| *f == field)
                    .map(|(_, e)| e.clone()),
            })
            .collect()
    }
}

impl ThreadFields {
    pub fn notes(&self) -> Vec<String> {
        let f = self;
        let mut notes = Vec::new();
        if let Some(s) = &f.surface {
            notes.push(format!(
                "Shaft Ø{:.3} mm · length {:.3} mm",
                s.cylinder.radius * 2.,
                s.bounds[1] - s.bounds[0]
            ));
        }
        if f.preset != "custom" {
            notes.push(format!("{} · pitch {}", f.designation, f.pitch.text()));
        }
        if f.representation == HoleThreadRepresentation::Modeled {
            notes.push("Modeled threads add helical geometry. Fine pitches and long threads take longer to build.".into());
        }
        notes
    }
}
