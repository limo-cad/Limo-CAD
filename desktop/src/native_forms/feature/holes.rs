//! Hole controls retain the shared feature, thread specification and associative references.
use super::*;
use limo_cad_core::{PlaneBasis, UnitSystem};
use limo_cad_solid::{
    HoleBottomStyle, HoleDefinitionDto, HoleExtent, HolePositionDto, HoleRequest, HoleStyle,
    Point2Dto, SketchPointRefDto,
};

#[derive(Debug)]
pub(super) struct HoleFields {
    support: Option<PlanarFaceSourceDto>,
    positions: Vec<Position>,
    selected: Option<u64>,
    last_position_id: u64,
    diameter: MeasurementInput,
    extent: HoleExtent,
    depth: MeasurementInput,
    style: HoleStyle,
    counterbore: [MeasurementInput; 2],
    countersink: [MeasurementInput; 2],
    bottom: HoleBottomStyle,
    drill_angle: MeasurementInput,
    threaded: bool,
    thread: ThreadFields,
    flip: bool,
}

const NEW_POSITION_LIMIT: usize = 256;

#[derive(Debug)]
struct Position {
    id: u64,
    uv: [Coordinate; 2],
    reference: Option<SketchPointRefDto>,
}

#[derive(Debug)]
struct Coordinate {
    input: MeasurementInput,
    canonical: Option<f64>,
}

impl Coordinate {
    fn new(canonical: Option<f64>, units: UnitSystem) -> Self {
        let mut input = length(canonical.unwrap_or(0.), units);
        if canonical.is_none() {
            input.set_text(String::new());
        }
        Self { input, canonical }
    }

    fn text(&self) -> &str {
        self.input.text()
    }

    fn set_text(&mut self, text: String) {
        self.input.set_text(text);
        self.canonical = None;
    }

    /// Untouched source coordinates retain their exact canonical value across display-unit conversion.
    fn evaluate(&self, model: &FormModel<'_>) -> Result<f64, String> {
        self.canonical.map_or_else(
            || {
                self.input
                    .evaluate(model.document.settings.units, model.parameters)
            },
            Ok,
        )
    }
}

impl Position {
    fn new(
        id: u64,
        uv: Option<[f64; 2]>,
        reference: Option<SketchPointRefDto>,
        units: UnitSystem,
    ) -> Self {
        Self {
            id,
            uv: std::array::from_fn(|axis| Coordinate::new(uv.map(|p| p[axis]), units)),
            reference,
        }
    }

    fn display_uv(&self, model: &FormModel<'_>, basis: Option<PlaneBasis>) -> [String; 2] {
        if let Some(reference) = &self.reference {
            return basis
                .and_then(|basis| reference_position(reference, model, basis).ok())
                .map(|uv| uv.map(|v| length(v, model.document.settings.units).text().to_owned()))
                .unwrap_or_else(|| std::array::from_fn(|_| "Unavailable reference".into()));
        }
        std::array::from_fn(|axis| self.uv[axis].text().to_owned())
    }

    fn summary_uv(&self, model: &FormModel<'_>, basis: Option<PlaneBasis>) -> [String; 2] {
        let reference = self.reference.as_ref().and_then(|reference| {
            basis.and_then(|basis| reference_position(reference, model, basis).ok())
        });
        std::array::from_fn(|axis| {
            let value = if self.reference.is_some() {
                reference.map(|uv| uv[axis])
            } else {
                self.uv[axis].evaluate(model).ok()
            };
            value
                .map(|v| MeasurementInput::display_length(v, model.document.settings.units))
                .unwrap_or_else(|| "—".into())
        })
    }
}
fn length(v: f64, units: UnitSystem) -> MeasurementInput {
    MeasurementInput::new(DimensionKind::Length, v, units)
}
fn angle(v: f64, units: UnitSystem) -> MeasurementInput {
    MeasurementInput::new(DimensionKind::Angle, v, units)
}
fn option<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, String> {
    serde_json::from_value(json!(value)).map_err(|_| "Choose an available hole option".into())
}
fn key<T: serde::Serialize>(v: T) -> String {
    serde_json::to_value(v).unwrap().as_str().unwrap().into()
}

impl HoleFields {
    pub fn new(units: UnitSystem) -> Self {
        Self {
            support: None,
            positions: vec![],
            selected: None,
            last_position_id: 0,
            diameter: length(5., units),
            extent: HoleExtent::ThroughAll,
            depth: length(10., units),
            style: HoleStyle::Simple,
            counterbore: [9., 3.].map(|v| length(v, units)),
            countersink: [length(9., units), angle(90., units)],
            bottom: HoleBottomStyle::DrillPoint,
            drill_angle: angle(118., units),
            threaded: false,
            thread: ThreadFields::new_internal(units),
            flip: false,
        }
    }
    pub fn set(
        &mut self,
        field: SolidField,
        value: &str,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        use SolidField::*;
        match field {
            HolePositionSelection => {
                let id = value
                    .parse::<u64>()
                    .map_err(|_| "Choose an available hole position")?;
                if !self.positions.iter().any(|p| p.id == id) {
                    return Err("The selected hole position no longer exists".into());
                }
                self.selected = Some(id);
            }
            HolePositionU(id) | HolePositionV(id) => {
                let position = self.selected_mut(id)?;
                if position.reference.is_some() {
                    return Err(
                        "Make this position independent before editing its coordinates".into(),
                    );
                }
                position.uv[usize::from(field == HolePositionV(id))].set_text(value.into());
            }
            HoleDiameter => self.diameter.set_text(value.into()),
            HoleDepth => self.depth.set_text(value.into()),
            Extent => {
                self.extent = serde_json::from_value(json!({"type":value,"depth":10.}))
                    .map_err(|_| "Choose Through all or Distance")?
            }
            HoleStyle => self.style = option(value)?,
            CounterboreDiameter => self.counterbore[0].set_text(value.into()),
            CounterboreDepth => self.counterbore[1].set_text(value.into()),
            CountersinkDiameter => self.countersink[0].set_text(value.into()),
            CountersinkAngle => self.countersink[1].set_text(value.into()),
            BottomStyle => self.bottom = option(value)?,
            DrillPointAngle => self.drill_angle.set_text(value.into()),
            Flip => self.flip = value.parse().map_err(|_| "Flip expects true or false")?,
            Threaded => {
                self.threaded = value
                    .parse()
                    .map_err(|_| "Threaded expects true or false")?;
                if self.threaded {
                    self.use_drill(model);
                }
            }
            _ => {
                self.thread.set(field, value, model)?;
                if matches!(field, ThreadStandard | ThreadSeries | ThreadPreset) {
                    self.use_drill(model);
                }
            }
        }
        Ok(())
    }
    fn use_drill(&mut self, model: &FormModel<'_>) {
        if let Some(d) = self.thread.preset_drill() {
            self.diameter = length(d, model.document.settings.units);
        }
    }

    fn selected_mut(&mut self, id: u64) -> Result<&mut Position, String> {
        if self.selected != Some(id) {
            return Err("The selected hole position changed".into());
        }
        self.positions
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| "The selected hole position no longer exists".into())
    }

    fn add(
        &mut self,
        uv: Option<[f64; 2]>,
        reference: Option<SketchPointRefDto>,
        units: UnitSystem,
    ) -> Result<u64, String> {
        if self.positions.len() >= NEW_POSITION_LIMIT {
            return Err(format!("The native editor adds up to {NEW_POSITION_LIMIT} positions; existing larger lists are preserved"));
        }
        let id = self
            .last_position_id
            .checked_add(1)
            .ok_or("Hole position identities exhausted")?;
        self.positions.push(Position::new(id, uv, reference, units));
        self.last_position_id = id;
        self.selected = Some(id);
        Ok(id)
    }

    fn remove(&mut self, index: usize) {
        let removed = self.positions.remove(index);
        if self.selected == Some(removed.id) {
            self.selected = self
                .positions
                .get(index.min(self.positions.len().saturating_sub(1)))
                .map(|p| p.id);
        }
    }
}

impl SolidForm {
    pub(crate) fn edit_hole_position_list(
        &mut self,
        field: SolidField,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        let basis = self
            .hole_basis(model)
            .ok_or("Select a planar support face first")?;
        let f = self
            .hole
            .as_mut()
            .ok_or("This feature has no hole positions")?;
        match field {
            SolidField::HolePositionAdd => {
                f.add(None, None, model.document.settings.units)?;
            }
            SolidField::HolePositionRemove(id) => {
                f.selected_mut(id)?;
                let index = f.positions.iter().position(|p| p.id == id).unwrap();
                f.remove(index);
            }
            SolidField::HolePositionIndependent(id) => {
                let position = f.selected_mut(id)?;
                let reference = position
                    .reference
                    .as_ref()
                    .ok_or("This position is already independent")?;
                let uv = reference_position(reference, model, basis)?;
                position.uv = uv.map(|v| Coordinate::new(Some(v), model.document.settings.units));
                position.reference = None;
            }
            _ => return Err("This control does not edit the hole position list".into()),
        }
        self.changed();
        Ok(())
    }
    pub(super) fn hole_position_count(&self) -> usize {
        self.hole
            .as_ref()
            .filter(|fields| fields.support.is_some())
            .map_or(0, |fields| fields.positions.len())
    }
    pub(super) fn hole_notes(&self) -> Vec<String> {
        let Some(f) = &self.hole else {
            return vec![];
        };
        let mut notes = if f.threaded { f.thread.notes() } else { vec![] };
        if f.positions.iter().any(|p| p.reference.is_some()) {
            notes.insert(
                0,
                "Sketch point references move these holes when the source sketch changes.".into(),
            );
        }
        if f.positions.len() >= NEW_POSITION_LIMIT {
            notes.push(format!("Add position is unavailable at the native editor's {NEW_POSITION_LIMIT}-position limit. Existing positions remain editable and removable."));
        }
        notes
    }

    pub(crate) fn hole_support(&self) -> Option<PlanarFaceSourceDto> {
        self.hole.as_ref()?.support
    }
    pub(crate) fn hole_basis(&self, model: &FormModel<'_>) -> Option<PlaneBasis> {
        let s = self.hole_support()?;
        model
            .scene
            .bodies
            .iter()
            .find(|b| b.id == s.body_id)?
            .faces
            .iter()
            .find(|f| f.id == s.face_id)?
            .plane
    }
    pub(crate) fn set_hole_support(
        &mut self,
        support: Option<PlanarFaceSourceDto>,
        point: Option<[f64; 3]>,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        let position = if let Some(s) = support {
            validate_face(s, model)?;
            let body = model
                .scene
                .bodies
                .iter()
                .find(|b| b.id == s.body_id)
                .unwrap();
            let face = body.faces.iter().find(|f| f.id == s.face_id).unwrap();
            let point = point
                .or_else(|| face_center(body, face))
                .ok_or("The support face has no usable surface")?;
            if !point.iter().all(|v| v.is_finite()) {
                return Err("Hole position must be finite".into());
            }
            face.plane.unwrap().to_2d(point)
        } else {
            [0., 0.]
        };
        let f = self
            .hole
            .as_mut()
            .ok_or("This feature has no hole support")?;
        let first = if support.is_some() {
            let id = f
                .last_position_id
                .checked_add(1)
                .ok_or("Hole position identities exhausted")?;
            Some(Position::new(
                id,
                Some(position),
                None,
                model.document.settings.units,
            ))
        } else {
            None
        };
        f.support = support;
        f.positions.clear();
        f.selected = None;
        if let Some(first) = first {
            f.last_position_id = first.id;
            f.selected = Some(first.id);
            f.positions.push(first);
        }
        self.changed();
        Ok(())
    }
    pub(crate) fn set_hole_position(
        &mut self,
        local: [f64; 3],
        reference: Option<SketchPointRefDto>,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        let basis = self
            .hole_basis(model)
            .ok_or("Select a planar support face first")?;
        let mut uv = basis.to_2d(local);
        if let Some(r) = &reference {
            uv = reference_position(r, model, basis)?;
        }
        if !uv.iter().all(|v| v.is_finite()) {
            return Err("Hole position must be finite".into());
        }
        let f = self.hole.as_mut().unwrap();
        if let Some(reference) = reference {
            if let Some(index) = f
                .positions
                .iter()
                .position(|p| p.reference.as_ref() == Some(&reference))
            {
                f.remove(index);
            } else if let Some(position) = f.positions.iter_mut().find(|p| {
                Some(p.id) == f.selected
                    && p.reference.is_none()
                    && p.uv.iter().all(|v| v.text().trim().is_empty())
            }) {
                position.reference = Some(reference);
                position.uv = uv.map(|v| Coordinate::new(Some(v), model.document.settings.units));
            } else {
                f.add(Some(uv), Some(reference), model.document.settings.units)?;
            }
            self.changed();
            return Ok(());
        }
        let id = f
            .selected
            .ok_or("Add a position before picking its location")?;
        let position = f.selected_mut(id)?;
        if position.reference.is_some() {
            return Err("Make this position independent before picking a free location".into());
        }
        position.uv = uv.map(|v| Coordinate::new(Some(v), model.document.settings.units));
        self.changed();
        Ok(())
    }
    pub(crate) fn clear_hole_positions(&mut self, model: &FormModel<'_>) -> Result<(), String> {
        self.editing(model)?;
        let f = self
            .hole
            .as_mut()
            .ok_or("This feature has no hole positions")?;
        f.positions.clear();
        f.selected = None;
        self.changed();
        Ok(())
    }
    pub(crate) fn edit_hole(definition: &Value, model: &FormModel<'_>) -> Result<Self, String> {
        let d: HoleDefinitionDto =
            serde_json::from_value(definition.clone()).map_err(|e| e.to_string())?;
        let units = model.document.settings.units;
        let mut form = Self::new_kind(SolidFormKind::Hole, model);
        form.feature = Some(d.feature_id);
        let mut f = HoleFields::new(units);
        f.support = Some(PlanarFaceSourceDto {
            body_id: d.body_id,
            face_id: d.face_id,
        });
        let positions = if d.positions.is_empty() {
            vec![HolePositionDto {
                position: d.position,
                position_reference: d.position_reference,
            }]
        } else {
            d.positions
        };
        for position in positions {
            let id = f
                .last_position_id
                .checked_add(1)
                .ok_or("Hole position identities exhausted")?;
            f.positions.push(Position::new(
                id,
                Some([position.position.x, position.position.y]),
                position.position_reference,
                units,
            ));
            f.last_position_id = id;
        }
        f.selected = f.positions.first().map(|p| p.id);
        f.diameter = length(d.diameter, units);
        f.extent = d.extent;
        if let HoleExtent::Distance { depth } = d.extent {
            f.depth = length(depth, units);
        }
        f.style = d.style;
        f.counterbore = [d.counterbore_diameter, d.counterbore_depth].map(|v| length(v, units));
        f.countersink = [
            length(d.countersink_diameter, units),
            angle(d.countersink_angle_deg, units),
        ];
        f.bottom = d.bottom_style;
        f.drill_angle = angle(d.drill_point_angle_deg, units);
        f.flip = d.flip;
        if let Some(t) = d.thread {
            f.threaded = true;
            f.thread = ThreadFields::from_thread(t, false, units, false);
        }
        form.hole = Some(f);
        form.hole_request(model).map_err(first_error)?;
        Ok(form)
    }
    fn hole_request(
        &self,
        model: &FormModel<'_>,
    ) -> Result<HoleRequest, Vec<(SolidField, String)>> {
        use SolidField as F;
        let f = self.hole.as_ref().unwrap();
        let mut errors = vec![];
        if let Err(e) = self.check_model(model) {
            return Err(vec![(F::HoleSupport, e)]);
        }
        if self.feature.is_some_and(|id| {
            !model
                .document
                .features
                .iter()
                .any(|v| v.id == id && v.kind == FeatureKind::Hole)
        }) {
            return Err(vec![(
                F::HoleSupport,
                "The edited hole no longer exists".into(),
            )]);
        }
        let support = f
            .support
            .ok_or_else(|| vec![(F::HoleSupport, "Select a planar support face".into())])?;
        validate_face(support, model).map_err(|e| vec![(F::HoleSupport, e)])?;
        let basis = self.hole_basis(model).unwrap();
        let mut number = |field, v: &MeasurementInput| match v
            .evaluate(model.document.settings.units, model.parameters)
        {
            Ok(v) => v,
            Err(e) => {
                errors.push((field, e));
                0.
            }
        };
        let diameter = number(F::HoleDiameter, &f.diameter);
        let extent = if matches!(f.extent, HoleExtent::ThroughAll) {
            HoleExtent::ThroughAll
        } else {
            HoleExtent::Distance {
                depth: number(F::HoleDepth, &f.depth),
            }
        };
        let (counterbore_diameter, counterbore_depth) = if f.style == HoleStyle::Counterbore {
            (
                number(F::CounterboreDiameter, &f.counterbore[0]),
                number(F::CounterboreDepth, &f.counterbore[1]),
            )
        } else {
            (0., 0.)
        };
        let (countersink_diameter, countersink_angle_deg) = if f.style == HoleStyle::Countersink {
            (
                number(F::CountersinkDiameter, &f.countersink[0]),
                number(F::CountersinkAngle, &f.countersink[1]),
            )
        } else {
            (0., 90.)
        };
        let drill_point_angle_deg = if f.bottom == HoleBottomStyle::DrillPoint {
            number(F::DrillPointAngle, &f.drill_angle)
        } else {
            118.
        };
        if f.positions.is_empty() {
            errors.push((F::HolePositions, "Add at least one hole position".into()));
        }
        let mut positions = Vec::with_capacity(f.positions.len());
        for (index, position) in f.positions.iter().enumerate() {
            let mut uv = [0.; 2];
            if let Some(reference) = &position.reference {
                match reference_position(reference, model, basis) {
                    Ok(point) => uv = point,
                    Err(error) => {
                        errors.push((F::HolePositions, format!("Position {}: {error}", index + 1)))
                    }
                }
            } else {
                for (axis, field) in [F::HolePositionU(position.id), F::HolePositionV(position.id)]
                    .into_iter()
                    .enumerate()
                {
                    match position.uv[axis].evaluate(model) {
                        Ok(value) => uv[axis] = value,
                        Err(error) => {
                            errors.push((field, format!("Position {}: {error}", index + 1)))
                        }
                    }
                }
            }
            positions.push(HolePositionDto {
                position: Point2Dto::new(uv[0], uv[1]),
                position_reference: position.reference.clone(),
            });
        }
        let thread = if f.threaded {
            match f.thread.evaluate(model) {
                Ok(t) => Some(t),
                Err(e) => {
                    errors.extend(e);
                    None
                }
            }
        } else {
            None
        };
        let Some(first) = positions.first() else {
            return Err(errors);
        };
        let request = HoleRequest {
            body_id: support.body_id,
            face_id: support.face_id,
            position: first.position,
            position_reference: first.position_reference.clone(),
            positions,
            diameter,
            extent,
            style: f.style,
            counterbore_diameter,
            counterbore_depth,
            countersink_diameter,
            countersink_angle_deg,
            bottom_style: f.bottom,
            drill_point_angle_deg,
            thread,
            flip: f.flip,
        };
        if let Err(e) = limo_cad_solid::validate_hole(&request) {
            // Preserve the shared validator's message and acceptance rules;
            // attach its known parameter errors to the control that owns them.
            let field = match &e {
                limo_cad_solid::SolidError::InvalidExtent(message) => {
                    if message.starts_with("counterbore diameter") {
                        F::CounterboreDiameter
                    } else if message.starts_with("counterbore depth") {
                        F::CounterboreDepth
                    } else if message.starts_with("countersink diameter") {
                        F::CountersinkDiameter
                    } else if message.starts_with("countersink angle") {
                        F::CountersinkAngle
                    } else if message.starts_with("drill point angle") {
                        F::DrillPointAngle
                    } else if message.starts_with("hole depth") {
                        F::HoleDepth
                    } else if message.starts_with("hole positions") {
                        F::HolePositions
                    } else if message.starts_with("thread nominal diameter") {
                        F::Diameter
                    } else if message.starts_with("thread pitch")
                        || message.starts_with("threads per inch")
                        || message.starts_with("Unified thread pitch")
                    {
                        F::Pitch
                    } else if message.starts_with("thread depth") {
                        F::Distance
                    } else if message.starts_with("thread designation") {
                        F::Designation
                    } else if message.starts_with("thread tolerance class") {
                        F::ThreadClass
                    } else if message.starts_with("thread series") {
                        F::ThreadSeries
                    } else if message.starts_with("predrill") {
                        F::HoleDiameter
                    } else if request.thread.is_some() && !message.starts_with("hole diameter") {
                        F::ThreadPreset
                    } else {
                        F::HoleDiameter
                    }
                }
                limo_cad_solid::SolidError::EmptySelection => F::HolePositions,
                _ => F::HoleDiameter,
            };
            errors.push((field, e.to_string()));
        }
        if errors.is_empty() {
            Ok(request)
        } else {
            Err(errors)
        }
    }
    pub(super) fn hole_payload(
        &self,
        model: &FormModel<'_>,
    ) -> Result<(&'static str, Value), Vec<(SolidField, String)>> {
        let hole = self.hole_request(model)?;
        Ok(if let Some(id) = self.feature {
            ("solid_edit_hole", json!({"feature_id":id,"hole":hole}))
        } else {
            ("solid_hole", json!(hole))
        })
    }
    pub(crate) fn hole_guide(
        &self,
        model: &FormModel<'_>,
    ) -> Option<(HoleRequest, PlaneBasis, f64)> {
        self.hole.as_ref()?;
        let r = self.hole_request(model).ok()?;
        let basis = self.hole_basis(model)?;
        let depth = match r.extent {
            HoleExtent::Distance { depth } => depth,
            HoleExtent::ThroughAll => {
                let body = model.scene.bodies.iter().find(|b| b.id == r.body_id)?;
                body.mesh
                    .positions
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|p| {
                        (0..3)
                            .map(|i| {
                                (p[i] as f64 - basis.origin[i])
                                    * basis.normal[i]
                                    * if r.flip { 1. } else { -1. }
                            })
                            .sum::<f64>()
                    })
                    .fold(0., f64::max)
                    .max(r.diameter)
            }
        };
        Some((r, basis, depth))
    }
    pub(super) fn hole_fields(&self, model: &FormModel<'_>) -> Vec<SolidFieldView> {
        use SolidField as F;
        let f = self.hole.as_ref().unwrap();
        let errors = self.hole_request(model).err().unwrap_or_default();
        let basis = self.hole_basis(model);
        let editable = self.phase == Phase::Editing && self.check_model(model).is_ok();
        let text = |v: &str| Field::Text {
            value: v.into(),
            read_only: false,
            selection: None,
        };
        let choice = |value: String, options: &[(&str, &str)]| Field::Choice {
            value,
            options: options
                .iter()
                .map(|(v, l)| ChoiceOption {
                    value: (*v).into(),
                    label: (*l).into(),
                    disabled: false,
                })
                .collect(),
        };
        let support = f
            .support
            .and_then(|s| model.scene.bodies.iter().find(|b| b.id == s.body_id))
            .map(|b| format!("{} · planar face", b.name))
            .unwrap_or_else(|| "Click a planar face".into());
        let position = if f.support.is_none() {
            "Select a support face first".into()
        } else {
            format!(
                "Pick the selected position · {} positions",
                f.positions.len()
            )
        };
        let mut rows = vec![
            (F::HoleSupport, support, Field::None, true, true),
            (
                F::HolePositions,
                position,
                Field::None,
                true,
                f.support.is_some(),
            ),
            (
                F::HolePositionSelection,
                "Position".into(),
                Field::Choice {
                    value: f.selected.map(|id| id.to_string()).unwrap_or_default(),
                    options: f
                        .positions
                        .iter()
                        .enumerate()
                        .map(|(index, p)| {
                            let uv = p.summary_uv(model, basis);
                            let association = if let Some(reference) = &p.reference {
                                format!(" · linked to {}", reference.sketch_name)
                            } else {
                                String::new()
                            };
                            ChoiceOption {
                                value: p.id.to_string(),
                                label: format!(
                                    "{} · U {} · V {}{association}",
                                    index + 1,
                                    uv[0],
                                    uv[1]
                                ),
                                disabled: false,
                            }
                        })
                        .collect(),
                },
                true,
                f.selected.is_some(),
            ),
            (
                F::HolePositionAdd,
                "Add position".into(),
                Field::None,
                true,
                f.support.is_some() && f.positions.len() < NEW_POSITION_LIMIT,
            ),
        ];
        if let Some(position) = f.positions.iter().find(|p| Some(p.id) == f.selected) {
            let uv = position.display_uv(model, basis);
            for (axis, field) in [F::HolePositionU(position.id), F::HolePositionV(position.id)]
                .into_iter()
                .enumerate()
            {
                rows.push((
                    field,
                    if axis == 0 {
                        "Position U"
                    } else {
                        "Position V"
                    }
                    .into(),
                    Field::Text {
                        value: uv[axis].clone(),
                        read_only: position.reference.is_some(),
                        selection: None,
                    },
                    true,
                    position.reference.is_none(),
                ));
            }
            rows.push((
                F::HolePositionRemove(position.id),
                "Remove position".into(),
                Field::None,
                true,
                true,
            ));
            if position.reference.is_some() {
                rows.push((
                    F::HolePositionIndependent(position.id),
                    "Make independent".into(),
                    Field::None,
                    true,
                    true,
                ));
            }
        }
        rows.extend([
            (
                F::HoleStyle,
                "Hole style".into(),
                choice(
                    key(f.style),
                    &[
                        ("simple", "Simple"),
                        ("counterbore", "Counterbore"),
                        ("countersink", "Countersink"),
                    ],
                ),
                true,
                true,
            ),
            (
                F::Threaded,
                "Threaded hole".into(),
                Field::Toggle(f.threaded),
                true,
                true,
            ),
        ]);
        let mut fields: Vec<_> = rows
            .drain(..)
            .map(|(field, label, value, visible, enabled)| SolidFieldView {
                field,
                label,
                value,
                visible,
                enabled: editable && enabled,
                error: errors
                    .iter()
                    .find(|(k, _)| *k == field)
                    .map(|(_, e)| e.clone()),
            })
            .collect();
        if let Some(row) = fields
            .iter_mut()
            .find(|r| r.field == F::HolePositionSelection)
        {
            row.error = errors
                .iter()
                .find(|(field, _)| {
                    matches!(
                        field,
                        F::HolePositions | F::HolePositionU(_) | F::HolePositionV(_)
                    )
                })
                .map(|(_, error)| error.clone());
        }
        if f.threaded {
            fields.extend(
                f.thread
                    .fields(&errors, editable)
                    .into_iter()
                    .filter(|r| !matches!(r.field, F::Cylinder | F::Flip)),
            );
        }
        rows.extend([
            (
                F::HoleDiameter,
                if f.threaded {
                    "Predrill diameter"
                } else {
                    "Diameter"
                }
                .into(),
                text(f.diameter.text()),
                true,
                true,
            ),
            (
                F::CounterboreDiameter,
                "Counterbore diameter".into(),
                text(f.counterbore[0].text()),
                f.style == HoleStyle::Counterbore,
                true,
            ),
            (
                F::CounterboreDepth,
                "Counterbore depth".into(),
                text(f.counterbore[1].text()),
                f.style == HoleStyle::Counterbore,
                true,
            ),
            (
                F::CountersinkDiameter,
                "Countersink diameter".into(),
                text(f.countersink[0].text()),
                f.style == HoleStyle::Countersink,
                true,
            ),
            (
                F::CountersinkAngle,
                "Included angle (deg)".into(),
                text(f.countersink[1].text()),
                f.style == HoleStyle::Countersink,
                true,
            ),
            (
                F::Extent,
                "Extent".into(),
                choice(
                    if matches!(f.extent, HoleExtent::ThroughAll) {
                        "through_all"
                    } else {
                        "distance"
                    }
                    .into(),
                    &[("through_all", "Through all"), ("distance", "Distance")],
                ),
                true,
                true,
            ),
            (
                F::HoleDepth,
                "Depth".into(),
                text(f.depth.text()),
                matches!(f.extent, HoleExtent::Distance { .. }),
                true,
            ),
            (
                F::BottomStyle,
                "Hole bottom".into(),
                choice(
                    key(f.bottom),
                    &[("drill_point", "Drill point"), ("flat", "Flat bottom")],
                ),
                true,
                true,
            ),
            (
                F::DrillPointAngle,
                "Drill point angle (deg)".into(),
                text(f.drill_angle.text()),
                matches!(f.extent, HoleExtent::Distance { .. })
                    && f.bottom == HoleBottomStyle::DrillPoint,
                true,
            ),
            (
                F::Flip,
                "Flip cutting direction".into(),
                Field::Toggle(f.flip),
                true,
                true,
            ),
        ]);
        fields.extend(
            rows.into_iter()
                .map(|(field, label, value, visible, enabled)| SolidFieldView {
                    field,
                    label,
                    value,
                    visible,
                    enabled: editable && enabled,
                    error: errors
                        .iter()
                        .find(|(k, _)| *k == field)
                        .map(|(_, e)| e.clone()),
                }),
        );
        fields
    }
}

fn reference_position(
    reference: &SketchPointRefDto,
    model: &FormModel<'_>,
    basis: PlaneBasis,
) -> Result<[f64; 2], String> {
    let active = model
        .document
        .features
        .iter()
        .filter(|f| !f.suppressed)
        .map(|f| f.id)
        .collect();
    limo_cad_solid::hole_reference_center(reference, model.profiles, &active, basis)
        .map(|p| basis.to_2d(p))
        .map_err(|e| e.to_string())
}

fn face_center(body: &limo_cad_solid::BodyDto, face: &limo_cad_solid::FaceDto) -> Option<[f64; 3]> {
    use bevy::math::DVec3;
    let mut best = None;
    let start = face.first_index as usize;
    let end = start.checked_add(face.index_count as usize)?;
    for t in body.mesh.indices.get(start..end)?.as_chunks::<3>().0 {
        let point = |i: u32| -> Option<DVec3> {
            let k = (i as usize).checked_mul(3)?;
            let p = body.mesh.positions.get(k..k + 3)?;
            Some(DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64))
        };
        let (a, b, c) = (point(t[0])?, point(t[1])?, point(t[2])?);
        let area = (b - a).cross(c - a).length_squared();
        if area.is_finite() && area > 0. && best.as_ref().is_none_or(|(old, _)| area > *old) {
            best = Some((area, ((a + b + c) / 3.).to_array()));
        }
    }
    best.map(|(_, p)| p)
}
