use super::{DimensionKind, MeasurementInput, ParameterValue};
use limo_cad_core::{BodyId, DocumentDto, FeatureId, FeatureKind};
use limo_cad_interface::{ChoiceOption, DocumentContext, Field};
use limo_cad_solid::{
    EditExtrudeRequest, ExtrudeDefinitionDto, ExtrudeExtent, ExtrudeOperation, ExtrudeRequest,
    PlanarFaceSourceDto, ProfileCatalogItemDto, SolidSceneDto,
};
use serde_json::{json, Value};
use std::sync::Arc;
mod revolve;
use revolve::RevolveFields;
mod paths;
use paths::PathFields;
mod rib;
use rib::RibFields;
mod edges;
use edges::EdgeFields;
mod shell;
use shell::ShellFields;
mod combine;
use combine::CombineFields;
mod planes;
use planes::PlaneFields;
mod body_planes;
use body_planes::BodyPlaneFields;
mod patterns;
use patterns::PatternFields;
mod thread_sizes;
mod threads;
use threads::ThreadFields;
mod holes;
use holes::HoleFields;
mod move_copy;
use move_copy::MoveFields;
pub(crate) use move_copy::MoveMode;
mod presentation;
pub(crate) use presentation::SolidFormPresentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SolidFormKind {
    Extrude,
    Revolve,
    Sweep,
    Loft,
    Rib,
    Fillet,
    Chamfer,
    Shell,
    ExternalThread,
    Hole,
    MoveCopy,
    Combine,
    OffsetPlane,
    Midplane,
    AnglePlane,
    Mirror,
    SplitBody,
    RectangularPattern,
    CircularPattern,
}
impl SolidFormKind {
    pub(crate) fn from_feature_kind(kind: FeatureKind) -> Option<Self> {
        Some(match kind {
            FeatureKind::Extrude => Self::Extrude,
            FeatureKind::Revolve => Self::Revolve,
            FeatureKind::Sweep => Self::Sweep,
            FeatureKind::Loft => Self::Loft,
            FeatureKind::Rib => Self::Rib,
            FeatureKind::Fillet => Self::Fillet,
            FeatureKind::Chamfer => Self::Chamfer,
            FeatureKind::Shell => Self::Shell,
            FeatureKind::ExternalThread => Self::ExternalThread,
            FeatureKind::Hole => Self::Hole,
            FeatureKind::MoveCopy => Self::MoveCopy,
            FeatureKind::Combine => Self::Combine,
            FeatureKind::ConstructionPlane => Self::OffsetPlane,
            FeatureKind::Mirror => Self::Mirror,
            FeatureKind::SplitBody => Self::SplitBody,
            FeatureKind::RectangularPattern => Self::RectangularPattern,
            FeatureKind::CircularPattern => Self::CircularPattern,
            _ => return None,
        })
    }
    pub(crate) fn group(self) -> &'static str {
        limo_cad_interface::catalog::group_for(self.operation())
            .expect("Solid feature is in the shared catalog")
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Extrude => "Extrude",
            Self::Revolve => "Revolve",
            Self::Sweep => "Sweep",
            Self::Loft => "Loft",
            Self::Rib => "Rib",
            Self::Fillet => "Fillet",
            Self::Chamfer => "Chamfer",
            Self::Shell => "Shell",
            Self::ExternalThread => "External Thread",
            Self::Hole => "Hole",
            Self::MoveCopy => "Move/Copy",
            Self::Combine => "Combine",
            Self::OffsetPlane => "Offset Plane",
            Self::Midplane => "Midplane",
            Self::AnglePlane => "Plane at Angle",
            Self::Mirror => "Mirror",
            Self::SplitBody => "Split Body",
            Self::RectangularPattern => "Rectangular Pattern",
            Self::CircularPattern => "Circular Pattern",
        }
    }
    pub(crate) fn operation(self) -> &'static str {
        match self {
            Self::Extrude => "solid_extrude",
            Self::Revolve => "solid_revolve",
            Self::Sweep => "solid_sweep",
            Self::Loft => "solid_loft",
            Self::Rib => "solid_rib",
            Self::Fillet => "solid_fillet",
            Self::Chamfer => "solid_chamfer",
            Self::Shell => "solid_shell",
            Self::ExternalThread => "solid_external_thread",
            Self::Hole => "solid_hole",
            Self::MoveCopy => "solid_move_copy",
            Self::Combine => "solid_combine",
            Self::OffsetPlane => "construction_plane_offset",
            Self::Midplane => "construction_plane_midplane",
            Self::AnglePlane => "construction_plane_at_angle",
            Self::Mirror => "solid_mirror",
            Self::SplitBody => "solid_split_body",
            Self::RectangularPattern => "solid_rectangular_pattern",
            Self::CircularPattern => "solid_circular_pattern",
        }
    }
}

/// Borrow one coherent native snapshot under the document publisher lease.
/// Parameters belong to the accepted source sketch and use canonical units.
pub(crate) struct FormModel<'a> {
    pub owner: &'a DocumentContext,
    pub engine_revision: u64,
    pub document: &'a DocumentDto,
    pub profiles: &'a [ProfileCatalogItemDto],
    pub scene: &'a SolidSceneDto,
    pub datum_planes: &'a [limo_cad_solid::DatumPlaneDefinitionDto],
    pub parameters: &'a [ParameterValue],
    pub assembly: Option<&'a limo_cad_sketch::AssemblyDocumentDto>,
    pub assembly_solution: Option<&'a limo_cad_sketch::AssemblySolutionDto>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SolidField {
    Source,
    Operation,
    Extent,
    Distance,
    SecondDistance,
    Taper,
    Flip,
    Targets,
    StopFace,
    Axis,
    AxisLine,
    OriginX,
    OriginY,
    DirectionX,
    DirectionY,
    Angle,
    Path,
    Guide,
    GuideEnabled,
    CenterlineEnabled,
    Orientation,
    Transition,
    ForceC1,
    Ruled,
    Continuity,
    Thickness,
    Symmetric,
    Edges,
    Radius,
    TangentChain,
    Faces,
    Inward,
    TargetBody,
    ToolBodies,
    KeepTools,
    FirstPlane,
    SecondPlane,
    AxisEdge,
    Bodies,
    DirectionEdge,
    SecondDirectionEdge,
    OriginZ,
    DirectionZ,
    SecondDirectionX,
    SecondDirectionY,
    SecondDirectionZ,
    SecondEnabled,
    Count,
    SecondCount,
    MoveMode,
    MoveObjectType,
    Copy,
    TranslationX,
    TranslationY,
    TranslationZ,
    RotationX,
    RotationY,
    RotationZ,
    PivotX,
    PivotY,
    PivotZ,
    FromX,
    FromY,
    FromZ,
    ToX,
    ToY,
    ToZ,
    FromPoint,
    ToPoint,
    PivotPoint,
    HoleSupport,
    HolePositions,
    HolePositionSelection,
    HolePositionU(u64),
    HolePositionV(u64),
    HolePositionAdd,
    HolePositionRemove(u64),
    HolePositionIndependent(u64),
    HoleStyle,
    Threaded,
    HoleDiameter,
    HoleDepth,
    CounterboreDiameter,
    CounterboreDepth,
    CountersinkDiameter,
    CountersinkAngle,
    BottomStyle,
    DrillPointAngle,
    Cylinder,
    ThreadStandard,
    ThreadSeries,
    ThreadPreset,
    Diameter,
    Pitch,
    ThreadClass,
    Designation,
    ThreadHand,
    Representation,
    FullThread,
    RadialDepth,
    CornerRadius,
    RadialClearance,
    AxialClearance,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProfileSource {
    None,
    Profiles {
        sketch_name: String,
        indices: Vec<u32>,
    },
    Face(PlanarFaceSourceDto),
}

#[derive(Clone, Debug)]
pub(crate) struct SolidFieldView {
    pub field: SolidField,
    pub label: String,
    pub value: Field,
    pub error: Option<String>,
    pub visible: bool,
    pub enabled: bool,
}

#[derive(Clone, Debug)]
struct Stamp {
    identity: Arc<()>,
    owner: DocumentContext,
    model_revision: u64,
    edit: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct PreviewTicket {
    stamp: Stamp,
    request: ExtrudeRequest,
}
impl PreviewTicket {
    pub(crate) fn request(&self) -> &ExtrudeRequest {
        &self.request
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ApplyTicket {
    stamp: Stamp,
    operation: &'static str,
    arguments: Value,
}
impl ApplyTicket {
    pub(crate) fn operation(&self) -> &'static str {
        self.operation
    }
    pub(crate) fn arguments(&self) -> &Value {
        &self.arguments
    }
    pub(crate) fn owner(&self) -> &DocumentContext {
        &self.stamp.owner
    }
    pub(crate) fn model_revision(&self) -> u64 {
        self.stamp.model_revision
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Editing,
    Applying,
    Closed,
}

/// Profile feature fields, separated from rendering and event delivery. The
/// payload is always the established limo-cad-solid DTO consumed by MCP too.
#[derive(Debug)]
pub(crate) struct SolidForm {
    stamp: Stamp,
    feature: Option<FeatureId>,
    source: ProfileSource,
    operation: ExtrudeOperation,
    operation_manual: bool,
    extent: ExtrudeExtent,
    distance: MeasurementInput,
    second_distance: MeasurementInput,
    taper: MeasurementInput,
    flip: bool,
    targets: Vec<BodyId>,
    stop_face: Option<PlanarFaceSourceDto>,
    phase: Phase,
    engine_error: Option<String>,
    revolve: Option<RevolveFields>,
    paths: Option<PathFields>,
    rib: Option<RibFields>,
    edges: Option<EdgeFields>,
    shell: Option<ShellFields>,
    combine: Option<CombineFields>,
    planes: Option<PlaneFields>,
    body_planes: Option<BodyPlaneFields>,
    patterns: Option<PatternFields>,
    thread: Option<ThreadFields>,
    hole: Option<HoleFields>,
    move_copy: Option<MoveFields>,
}

impl SolidForm {
    pub(crate) fn new(model: &FormModel<'_>) -> Self {
        let units = model.document.settings.units;
        Self {
            stamp: Stamp {
                identity: Arc::new(()),
                owner: model.owner.clone(),
                model_revision: model.engine_revision,
                edit: 0,
            },
            feature: None,
            source: ProfileSource::None,
            operation: ExtrudeOperation::NewBody,
            operation_manual: false,
            extent: ExtrudeExtent::default(),
            distance: MeasurementInput::new(DimensionKind::Length, 10., units),
            second_distance: MeasurementInput::new(DimensionKind::Length, 10., units),
            taper: MeasurementInput::new(DimensionKind::Angle, 0., units),
            flip: false,
            targets: Vec::new(),
            stop_face: None,
            phase: Phase::Editing,
            engine_error: None,
            revolve: None,
            paths: None,
            rib: None,
            edges: None,
            shell: None,
            combine: None,
            planes: None,
            body_planes: None,
            patterns: None,
            thread: None,
            hole: None,
            move_copy: None,
        }
    }

    pub(crate) fn new_kind(kind: SolidFormKind, model: &FormModel<'_>) -> Self {
        let mut form = Self::new(model);
        if kind == SolidFormKind::Revolve {
            form.revolve = Some(RevolveFields::new(model.document.settings.units));
        } else if matches!(kind, SolidFormKind::Sweep | SolidFormKind::Loft) {
            form.paths = Some(PathFields::new(kind));
        } else if kind == SolidFormKind::Rib {
            form.rib = Some(RibFields::new(model.document.settings.units));
        } else if matches!(kind, SolidFormKind::Fillet | SolidFormKind::Chamfer) {
            form.edges = Some(EdgeFields::new(kind, model.document.settings.units));
        } else if kind == SolidFormKind::MoveCopy {
            form.move_copy = Some(MoveFields::new(model.document.settings.units));
        } else if kind == SolidFormKind::Hole {
            form.hole = Some(HoleFields::new(model.document.settings.units));
        } else if kind == SolidFormKind::ExternalThread {
            form.thread = Some(ThreadFields::new(model.document.settings.units));
        } else if kind == SolidFormKind::Shell {
            form.shell = Some(ShellFields::new(model.document.settings.units));
        } else if kind == SolidFormKind::Combine {
            form.combine = Some(CombineFields::default());
        } else if kind.is_pattern() {
            form.patterns = Some(PatternFields::new(kind, model.document.settings.units));
        } else if kind.is_body_plane() {
            form.body_planes = Some(BodyPlaneFields::new(kind));
        } else if kind.is_plane() {
            form.planes = Some(PlaneFields::new(kind, model.document.settings.units));
        }
        form
    }
    pub(crate) fn kind(&self) -> SolidFormKind {
        if self.move_copy.is_some() {
            return SolidFormKind::MoveCopy;
        }
        if self.hole.is_some() {
            return SolidFormKind::Hole;
        }
        if self.thread.is_some() {
            return SolidFormKind::ExternalThread;
        }
        if let Some(fields) = &self.patterns {
            return fields.kind;
        }
        if let Some(fields) = &self.body_planes {
            return fields.kind;
        }
        if let Some(planes) = &self.planes {
            return planes.kind;
        }
        if self.combine.is_some() {
            return SolidFormKind::Combine;
        }
        if self.shell.is_some() {
            return SolidFormKind::Shell;
        }
        if let Some(edges) = &self.edges {
            return edges.kind;
        }
        if self.rib.is_some() {
            return SolidFormKind::Rib;
        }
        if let Some(paths) = &self.paths {
            return paths.kind;
        }
        if self.revolve.is_some() {
            SolidFormKind::Revolve
        } else {
            SolidFormKind::Extrude
        }
    }

    pub(crate) fn edit(
        definition: &ExtrudeDefinitionDto,
        model: &FormModel<'_>,
    ) -> Result<Self, String> {
        if !model.document.features.iter().any(|feature| {
            feature.id == definition.feature_id && feature.kind == FeatureKind::Extrude
        }) {
            return Err("The Extrude feature is no longer in this document".into());
        }
        let mut form = Self::new(model);
        form.feature = Some(definition.feature_id);
        form.source = definition
            .source_face
            .map(ProfileSource::Face)
            .unwrap_or_else(|| ProfileSource::Profiles {
                sketch_name: definition.sketch_name.clone(),
                indices: definition.profile_indices.clone(),
            });
        form.operation = definition.operation;
        form.operation_manual = true;
        form.extent = definition.extent;
        form.flip = definition.flip;
        form.targets = definition.target_body_ids.clone();
        let units = model.document.settings.units;
        match definition.extent {
            ExtrudeExtent::Distance { distance } | ExtrudeExtent::Symmetric { distance } => {
                form.distance = MeasurementInput::new(DimensionKind::Length, distance, units)
            }
            ExtrudeExtent::TwoSides {
                distance,
                second_distance,
            } => {
                form.distance = MeasurementInput::new(DimensionKind::Length, distance, units);
                form.second_distance =
                    MeasurementInput::new(DimensionKind::Length, second_distance, units);
            }
            ExtrudeExtent::ToFace { face_id } => {
                let mut found = model.scene.bodies.iter().flat_map(|body| {
                    body.faces
                        .iter()
                        .filter(move |face| face.id == face_id && face.plane.is_some())
                        .map(move |_| PlanarFaceSourceDto {
                            body_id: body.id,
                            face_id,
                        })
                });
                form.stop_face = found.next();
                if found.next().is_some() {
                    return Err("The stop-face reference is ambiguous in the current model".into());
                }
            }
            ExtrudeExtent::ThroughAll => (),
        }
        form.taper = MeasurementInput::new(DimensionKind::Angle, definition.taper_angle_deg, units);
        Ok(form)
    }

    pub(crate) fn owner(&self) -> &DocumentContext {
        &self.stamp.owner
    }
    pub(crate) fn model_revision(&self) -> u64 {
        self.stamp.model_revision
    }
    pub(crate) fn advance_profile_feature_visibility(
        &mut self,
        owner: &DocumentContext,
        from: u64,
        to: u64,
    ) -> Result<(), String> {
        if !matches!(self.kind(), SolidFormKind::Revolve | SolidFormKind::Sweep)
            || self.feature.is_some()
            || self.phase != Phase::Editing
            || self.stamp.owner != *owner
            || self.stamp.model_revision != from
            || from.checked_add(1) != Some(to)
        {
            return Err("The visibility change no longer owns this feature draft".into());
        }
        self.stamp.identity = Arc::new(());
        self.stamp.model_revision = to;
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn is_open(&self) -> bool {
        self.phase != Phase::Closed
    }
    pub(crate) fn is_busy(&self) -> bool {
        self.phase == Phase::Applying
    }
    pub(crate) fn is_feature_edit(&self) -> bool {
        self.feature.is_some()
    }
    pub(crate) fn engine_error(&self) -> Option<&str> {
        self.engine_error.as_deref()
    }
    pub(crate) fn parameter_sketch(&self) -> Option<&str> {
        if let Some(rib) = &self.rib {
            return rib
                .centerline
                .as_ref()
                .map(|path| path.sketch_name.as_str());
        }
        match &self.source {
            ProfileSource::Profiles { sketch_name, .. } => Some(sketch_name),
            _ => None,
        }
    }

    fn check_model(&self, model: &FormModel<'_>) -> Result<(), String> {
        if self.stamp.owner != *model.owner || self.stamp.model_revision != model.engine_revision {
            return Err(
                "The document changed; reopen the feature with its current references".into(),
            );
        }
        if self.phase == Phase::Closed {
            return Err("The feature form is closed".into());
        }
        Ok(())
    }

    fn editing(&self, model: &FormModel<'_>) -> Result<(), String> {
        self.check_model(model)?;
        if self.phase != Phase::Editing {
            return Err("The feature is still applying".into());
        }
        self.stamp
            .edit
            .checked_add(1)
            .ok_or("Feature form revision exhausted")?;
        Ok(())
    }

    fn changed(&mut self) {
        self.stamp.edit += 1;
        self.engine_error = None;
    }

    pub(crate) fn set_value(
        &mut self,
        field: SolidField,
        value: &str,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        if let Some(fields) = &mut self.move_copy {
            fields.set(field, value, self.feature.is_some())?;
            self.changed();
            return Ok(());
        }
        if let Some(hole) = &mut self.hole {
            hole.set(field, value, model)?;
            self.changed();
            return Ok(());
        }
        if let Some(thread) = &mut self.thread {
            thread.set(field, value, model)?;
            self.changed();
            return Ok(());
        }
        if let Some(patterns) = &mut self.patterns {
            patterns.set(field, value)?;
            self.changed();
            return Ok(());
        }
        if let Some(planes) = &mut self.planes {
            planes.set(field, value)?;
            self.changed();
            return Ok(());
        }
        if let Some(combine) = &mut self.combine {
            combine.set(field, value)?;
            self.changed();
            return Ok(());
        }
        if let Some(shell) = &mut self.shell {
            shell.set(field, value)?;
            self.changed();
            return Ok(());
        }
        if let Some(edges) = &mut self.edges {
            edges.set(field, value)?;
            self.changed();
            return Ok(());
        }
        if let Some(rib) = &mut self.rib {
            if matches!(
                field,
                SolidField::Thickness
                    | SolidField::Distance
                    | SolidField::Extent
                    | SolidField::Symmetric
            ) {
                rib.set(field, value)?;
                self.changed();
                return Ok(());
            }
        }
        if matches!(
            field,
            SolidField::GuideEnabled
                | SolidField::CenterlineEnabled
                | SolidField::Orientation
                | SolidField::Transition
                | SolidField::ForceC1
                | SolidField::Ruled
                | SolidField::Continuity
        ) {
            self.paths
                .as_mut()
                .ok_or("This feature has no path options")?
                .set(field, value)?;
            self.changed();
            return Ok(());
        }
        if matches!(
            field,
            SolidField::Axis
                | SolidField::OriginX
                | SolidField::OriginY
                | SolidField::DirectionX
                | SolidField::DirectionY
                | SolidField::Angle
        ) {
            self.revolve
                .as_mut()
                .ok_or("This feature has no revolution axis")?
                .set(field, value)?;
            self.changed();
            return Ok(());
        }
        match field {
            SolidField::Distance => self.distance.set_text(value.into()),
            SolidField::SecondDistance => self.second_distance.set_text(value.into()),
            SolidField::Taper => self.taper.set_text(value.into()),
            SolidField::Operation => {
                self.operation =
                    serde_json::from_value(json!(value)).map_err(|error| error.to_string())?;
                self.operation_manual = true;
            }
            SolidField::Extent => {
                self.extent = serde_json::from_value(
                    json!({"type":value,"distance":10.,"second_distance":10.,"face_id":0}),
                )
                .map_err(|error| error.to_string())?;
            }
            SolidField::Flip => {
                self.flip = match value {
                    "true" => true,
                    "false" => false,
                    _ => return Err("Flip expects true or false".into()),
                }
            }
            _ => return Err("Use the geometry picker to change this reference".into()),
        }
        self.changed();
        Ok(())
    }

    pub(crate) fn set_source(
        &mut self,
        source: ProfileSource,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        if self.kind() != SolidFormKind::Extrude && matches!(source, ProfileSource::Face(_)) {
            return Err("This feature needs closed sketch profiles".into());
        }
        if source != ProfileSource::None {
            validate_source(&source, model)?;
        }
        if !self.operation_manual {
            if let ProfileSource::Face(face) = &source {
                self.operation = ExtrudeOperation::Join;
                self.targets = vec![face.body_id];
            } else {
                self.operation = ExtrudeOperation::NewBody;
                self.targets.clear();
            }
        }
        self.source = source;
        self.changed();
        Ok(())
    }

    pub(crate) fn set_targets(
        &mut self,
        targets: Vec<BodyId>,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        validate_targets(&targets, model)?;
        self.targets = targets;
        self.operation_manual = true;
        self.changed();
        Ok(())
    }

    pub(crate) fn set_stop_face(
        &mut self,
        face: Option<PlanarFaceSourceDto>,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        if let Some(face) = face {
            validate_face(face, model)?;
        }
        self.stop_face = face;
        self.changed();
        Ok(())
    }

    fn request(&self, model: &FormModel<'_>) -> Result<ExtrudeRequest, Vec<(SolidField, String)>> {
        use SolidField as F;
        let mut errors = Vec::new();
        if let Err(error) = self.check_model(model) {
            return Err(vec![(F::Source, error)]);
        }
        if let Some(feature) = self.feature {
            if !model
                .document
                .features
                .iter()
                .any(|entry| entry.id == feature && entry.kind == FeatureKind::Extrude)
            {
                errors.push((
                    F::Source,
                    "The edited Extrude feature no longer exists".into(),
                ));
            }
        }
        if let Err(error) = validate_source(&self.source, model) {
            errors.push((F::Source, error));
        }
        let units = model.document.settings.units;
        let mut measure =
            |field, input: &MeasurementInput| match input.evaluate(units, model.parameters) {
                Ok(value) => Some(value),
                Err(error) => {
                    errors.push((field, error));
                    None
                }
            };
        let distance = if matches!(
            self.extent,
            ExtrudeExtent::Distance { .. }
                | ExtrudeExtent::Symmetric { .. }
                | ExtrudeExtent::TwoSides { .. }
        ) {
            measure(F::Distance, &self.distance)
        } else {
            None
        };
        let second = if matches!(self.extent, ExtrudeExtent::TwoSides { .. }) {
            measure(F::SecondDistance, &self.second_distance)
        } else {
            None
        };
        let taper = measure(F::Taper, &self.taper);
        let extent = match self.extent {
            ExtrudeExtent::Distance { .. } => {
                if distance.is_some_and(|value| value.abs() <= 1e-6) {
                    errors.push((F::Distance, "Distance must be nonzero".into()));
                }
                ExtrudeExtent::Distance {
                    distance: distance.unwrap_or(0.),
                }
            }
            ExtrudeExtent::Symmetric { .. } => {
                if distance.is_some_and(|value| value <= 1e-6) {
                    errors.push((F::Distance, "Symmetric distance must be positive".into()));
                }
                ExtrudeExtent::Symmetric {
                    distance: distance.unwrap_or(0.),
                }
            }
            ExtrudeExtent::TwoSides { .. } => {
                if distance.is_some_and(|value| value <= 1e-6) {
                    errors.push((F::Distance, "First distance must be positive".into()));
                }
                if second.is_some_and(|value| value <= 1e-6) {
                    errors.push((F::SecondDistance, "Second distance must be positive".into()));
                }
                ExtrudeExtent::TwoSides {
                    distance: distance.unwrap_or(0.),
                    second_distance: second.unwrap_or(0.),
                }
            }
            ExtrudeExtent::ThroughAll => ExtrudeExtent::ThroughAll,
            ExtrudeExtent::ToFace { .. } => {
                match self.stop_face {
                    Some(face) => {
                        if let Err(error) = validate_stop_face(&self.source, face, model) {
                            errors.push((F::StopFace, error));
                        }
                    }
                    None => errors.push((F::StopFace, "Select a planar stop face".into())),
                }
                ExtrudeExtent::ToFace {
                    face_id: self
                        .stop_face
                        .map(|face| face.face_id)
                        .unwrap_or(limo_cad_core::FaceId(0)),
                }
            }
        };
        if taper.is_some_and(|value| value.abs() >= 89.) {
            errors.push((F::Taper, "Taper must be between -89 and 89 degrees".into()));
        }
        if self.operation != ExtrudeOperation::NewBody {
            if let Err(error) = validate_targets(&self.targets, model) {
                errors.push((F::Targets, error));
            }
            let joined_profiles = self.operation == ExtrudeOperation::Join
                && matches!(&self.source,ProfileSource::Profiles{indices,..} if indices.len()>1);
            if self.targets.is_empty() && !joined_profiles {
                errors.push((F::Targets, "Select a target body for this operation".into()));
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        let (source_face, sketch_name, profile_indices) = match &self.source {
            ProfileSource::Face(face) => (Some(*face), String::new(), Vec::new()),
            ProfileSource::Profiles {
                sketch_name,
                indices,
            } => (None, sketch_name.clone(), indices.clone()),
            ProfileSource::None => unreachable!("validated above"),
        };
        Ok(ExtrudeRequest {
            source_face,
            sketch_name,
            profile_indices,
            operation: self.operation,
            extent,
            taper_angle_deg: taper.unwrap(),
            flip: self.flip,
            target_body_ids: if self.operation == ExtrudeOperation::NewBody {
                Vec::new()
            } else {
                self.targets.clone()
            },
        })
    }

    pub(crate) fn can_apply(&self, model: &FormModel<'_>) -> bool {
        self.phase == Phase::Editing && self.payload(model).is_ok()
    }

    pub(crate) fn prepare_preview(&self, model: &FormModel<'_>) -> Result<PreviewTicket, String> {
        self.editing(model)?;
        if self.kind() != SolidFormKind::Extrude {
            return Err("This feature uses reference highlighting".into());
        }
        let request = self.request(model).map_err(first_error)?;
        Ok(PreviewTicket {
            stamp: self.stamp.clone(),
            request,
        })
    }

    pub(crate) fn accepts_preview(&self, ticket: &PreviewTicket, model: &FormModel<'_>) -> bool {
        self.phase == Phase::Editing
            && self.check_model(model).is_ok()
            && self.matches(&ticket.stamp)
    }

    pub(crate) fn prepare_apply(&mut self, model: &FormModel<'_>) -> Result<ApplyTicket, String> {
        self.editing(model)?;
        let (operation, arguments) = self.payload(model).map_err(first_error)?;
        self.changed();
        self.phase = Phase::Applying;
        Ok(ApplyTicket {
            stamp: self.stamp.clone(),
            operation,
            arguments,
        })
    }

    pub(crate) fn apply_failed(
        &mut self,
        ticket: &ApplyTicket,
        model: &FormModel<'_>,
        error: String,
    ) -> Result<(), String> {
        if self.phase != Phase::Applying || !self.matches(&ticket.stamp) {
            return Err("This completion belongs to another feature operation".into());
        }
        if let Err(changed) = self.check_model(model) {
            self.phase = Phase::Closed;
            return Err(changed);
        }
        self.phase = Phase::Editing;
        self.engine_error = Some(error);
        Ok(())
    }

    pub(crate) fn apply_succeeded(
        &mut self,
        ticket: &ApplyTicket,
        owner: &DocumentContext,
        engine_revision: u64,
    ) -> Result<(), String> {
        if self.phase != Phase::Applying || !self.matches(&ticket.stamp) {
            return Err("This completion belongs to another feature operation".into());
        }
        self.phase = Phase::Closed;
        if self.stamp.owner != *owner || engine_revision <= self.stamp.model_revision {
            return Err("Feature completion no longer owns the edited document".into());
        }
        Ok(())
    }

    /// Close even a stale form. Restore the host's previously captured preview
    /// only when true: an old owner must never paint over its replacement.
    pub(crate) fn cancel(&mut self, model: &FormModel<'_>) -> Result<bool, String> {
        if self.phase == Phase::Applying {
            return Err("The feature is still applying".into());
        }
        let restore_previous_preview = self.check_model(model).is_ok();
        self.phase = Phase::Closed;
        Ok(restore_previous_preview)
    }

    fn matches(&self, other: &Stamp) -> bool {
        Arc::ptr_eq(&self.stamp.identity, &other.identity)
            && self.stamp.owner == other.owner
            && self.stamp.model_revision == other.model_revision
            && self.stamp.edit == other.edit
    }

    pub(crate) fn fields(&self, model: &FormModel<'_>) -> Vec<SolidFieldView> {
        if self.move_copy.is_some() {
            return self.move_fields(model);
        }
        if self.hole.is_some() {
            return self.hole_fields(model);
        }
        if self.thread.is_some() {
            return self.thread_fields(model);
        }
        if self.patterns.is_some() {
            return self.pattern_fields(model);
        }
        if self.body_planes.is_some() {
            return self.body_plane_fields(model);
        }
        if self.planes.is_some() {
            return self.plane_fields(model);
        }
        if self.combine.is_some() {
            return self.combine_fields(model);
        }
        if self.shell.is_some() {
            return self.shell_fields(model);
        }
        if self.edges.is_some() {
            return self.edge_fields(model);
        }
        if self.rib.is_some() {
            return self.rib_fields(model);
        }
        if self.paths.is_some() {
            return self.path_fields(model);
        }
        if self.revolve.is_some() {
            return self.revolve_fields(model);
        }
        use SolidField as F;
        let issues = self.request(model).err().unwrap_or_default();
        let enabled = self.phase == Phase::Editing && self.check_model(model).is_ok();
        let text = |value: &MeasurementInput| Field::Text {
            value: value.text().into(),
            read_only: false,
            selection: None,
        };
        let choice = |value: Value, options: Vec<(&str, &str)>| Field::Choice {
            value: value.as_str().unwrap().into(),
            options: options
                .into_iter()
                .map(|(value, label)| ChoiceOption {
                    value: value.into(),
                    label: label.into(),
                    disabled: false,
                })
                .collect(),
        };
        let source_label = match &self.source {
            ProfileSource::None => "Select profiles or a planar face".into(),
            ProfileSource::Face(face) => {
                format!("Source: body {} · face {}", face.body_id.0, face.face_id.0)
            }
            ProfileSource::Profiles {
                sketch_name,
                indices,
            } => format!("{sketch_name} · {} profile(s)", indices.len()),
        };
        let stop_label = self
            .stop_face
            .map(|face| format!("Stop: body {} · face {}", face.body_id.0, face.face_id.0))
            .unwrap_or_else(|| "Select stop face".into());
        let rows = vec![
            (F::Source, source_label, Field::None, true),
            (
                F::Operation,
                "Operation".into(),
                choice(
                    json!(self.operation),
                    vec![
                        ("new_body", "Create Body"),
                        ("join", "Add"),
                        ("cut", "Subtract"),
                        ("intersect", "Common"),
                    ],
                ),
                true,
            ),
            (
                F::Extent,
                "Extent".into(),
                choice(
                    json!(self.extent)["type"].clone(),
                    vec![
                        ("distance", "Distance"),
                        ("two_sides", "Two sides"),
                        ("symmetric", "Symmetric"),
                        ("through_all", "Through all"),
                        ("to_face", "To face"),
                    ],
                ),
                true,
            ),
            (
                F::Distance,
                format!(
                    "Distance ({})",
                    json!(model.document.settings.units).as_str().unwrap()
                ),
                text(&self.distance),
                matches!(
                    self.extent,
                    ExtrudeExtent::Distance { .. }
                        | ExtrudeExtent::TwoSides { .. }
                        | ExtrudeExtent::Symmetric { .. }
                ),
            ),
            (
                F::SecondDistance,
                format!(
                    "Second distance ({})",
                    json!(model.document.settings.units).as_str().unwrap()
                ),
                text(&self.second_distance),
                matches!(self.extent, ExtrudeExtent::TwoSides { .. }),
            ),
            (
                F::Taper,
                "Taper angle (deg)".into(),
                text(&self.taper),
                true,
            ),
            (
                F::Flip,
                "Flip direction".into(),
                Field::Toggle(self.flip),
                !matches!(self.extent, ExtrudeExtent::ToFace { .. }),
            ),
            (
                F::Targets,
                format!("Target bodies ({})", self.targets.len()),
                Field::None,
                self.operation != ExtrudeOperation::NewBody,
            ),
            (
                F::StopFace,
                stop_label,
                Field::None,
                matches!(self.extent, ExtrudeExtent::ToFace { .. }),
            ),
        ];
        rows.into_iter()
            .map(|(field, label, value, visible)| SolidFieldView {
                field,
                label,
                value,
                error: issues
                    .iter()
                    .find(|(item, _)| *item == field)
                    .map(|(_, error)| error.clone()),
                visible,
                enabled,
            })
            .collect()
    }
}

impl SolidForm {
    fn payload(
        &self,
        model: &FormModel<'_>,
    ) -> Result<(&'static str, Value), Vec<(SolidField, String)>> {
        if self.move_copy.is_some() {
            return self.move_payload(model);
        }
        if self.hole.is_some() {
            return self.hole_payload(model);
        }
        if self.thread.is_some() {
            return self.thread_payload(model);
        }
        if self.patterns.is_some() {
            return self.pattern_payload(model);
        }
        if self.body_planes.is_some() {
            return self.body_plane_payload(model);
        }
        if self.planes.is_some() {
            return self.plane_payload(model);
        }
        if self.combine.is_some() {
            return self.combine_payload(model);
        }
        if self.shell.is_some() {
            return self.shell_payload(model);
        }
        if self.edges.is_some() {
            return self.edge_payload(model);
        }
        if self.rib.is_some() {
            return self.rib_payload(model);
        }
        if self.paths.is_some() {
            return self.path_payload(model);
        }
        if self.revolve.is_some() {
            return self.revolve_payload(model);
        }
        let request = self.request(model)?;
        let (op, value) = if let Some(feature_id) = self.feature {
            (
                "solid_edit_extrude",
                serde_json::to_value(EditExtrudeRequest {
                    feature_id,
                    extrude: request,
                }),
            )
        } else {
            ("solid_extrude", serde_json::to_value(request))
        };
        value
            .map(|v| (op, v))
            .map_err(|e| vec![(SolidField::Source, e.to_string())])
    }
}

fn first_error(errors: Vec<(SolidField, String)>) -> String {
    errors
        .into_iter()
        .next()
        .map(|(_, error)| error)
        .unwrap_or_else(|| "The feature is not ready".into())
}

fn validate_targets(targets: &[BodyId], model: &FormModel<'_>) -> Result<(), String> {
    for (index, target) in targets.iter().enumerate() {
        if targets[..index].contains(target) {
            return Err("A target body was selected more than once".into());
        }
        if !model.scene.bodies.iter().any(|body| body.id == *target) {
            return Err(format!("Target body {} no longer exists", target.0));
        }
    }
    Ok(())
}

fn validate_face(source: PlanarFaceSourceDto, model: &FormModel<'_>) -> Result<(), String> {
    let face = model
        .scene
        .bodies
        .iter()
        .find(|body| body.id == source.body_id)
        .and_then(|body| body.faces.iter().find(|face| face.id == source.face_id));
    if !face.is_some_and(|face| face.plane.is_some()) {
        return Err("The selected planar face no longer belongs to this body".into());
    }
    if model
        .scene
        .bodies
        .iter()
        .flat_map(|body| &body.faces)
        .filter(|face| face.id == source.face_id)
        .count()
        != 1
    {
        return Err("The face identifier is ambiguous in the current model".into());
    }
    Ok(())
}

fn validate_stop_face(
    source: &ProfileSource,
    stop: PlanarFaceSourceDto,
    model: &FormModel<'_>,
) -> Result<(), String> {
    validate_face(stop, model)?;
    let face_basis = |reference: PlanarFaceSourceDto| {
        model
            .scene
            .bodies
            .iter()
            .find(|body| body.id == reference.body_id)
            .and_then(|body| body.faces.iter().find(|face| face.id == reference.face_id))
            .and_then(|face| face.plane)
    };
    let source_basis = match source {
        ProfileSource::Face(face) => face_basis(*face),
        ProfileSource::Profiles { sketch_name, .. } => model
            .profiles
            .iter()
            .find(|catalog| catalog.sketch_name == *sketch_name)
            .map(|catalog| catalog.basis),
        ProfileSource::None => None,
    };
    if let (Some(source), Some(stop)) = (source_basis, face_basis(stop)) {
        let alignment: f64 = source
            .normal
            .iter()
            .zip(stop.normal)
            .map(|(a, b)| a * b)
            .sum();
        if alignment.abs() < 1. - 1e-6 {
            return Err("To Face currently requires a parallel planar face".into());
        }
        let distance: f64 = stop
            .origin
            .iter()
            .zip(source.origin)
            .zip(source.normal)
            .map(|((stop, source), normal)| (stop - source) * normal)
            .sum();
        if distance.abs() <= 1e-7 {
            return Err("The stop face lies on the source plane".into());
        }
    }
    Ok(())
}

fn validate_source(source: &ProfileSource, model: &FormModel<'_>) -> Result<(), String> {
    match source {
        ProfileSource::None => Err("Select a sketch profile or planar source face".into()),
        ProfileSource::Face(face) => validate_face(*face, model),
        ProfileSource::Profiles {
            sketch_name,
            indices,
        } => {
            let catalog = model
                .profiles
                .iter()
                .find(|entry| entry.sketch_name == *sketch_name)
                .ok_or("The source sketch no longer exists")?;
            if indices.is_empty() {
                return Err("Select at least one material profile".into());
            }
            for (position, index) in indices.iter().enumerate() {
                if indices[..position].contains(index) {
                    return Err("A profile was selected more than once".into());
                }
                if !catalog
                    .profiles
                    .iter()
                    .any(|profile| profile.index == *index && profile.nesting_depth % 2 == 0)
                {
                    return Err(format!(
                        "Profile {index} is not an available material region"
                    ));
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests;
