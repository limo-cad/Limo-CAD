//! Sweep and Loft retain stable, ordered profile/curve references and use the
//! same transaction as the other Build tools. No tessellation becomes a solid.
use super::*;
use limo_cad_solid::{
    EditLoftRequest, EditSweepRequest, LoftContinuity, LoftDefinitionDto, LoftRequest, PathRefDto,
    ProfileRefDto, SweepDefinitionDto, SweepOrientation, SweepRequest, SweepTransition,
};

#[derive(Debug)]
pub(super) struct PathFields {
    pub kind: SolidFormKind,
    sections: Vec<ProfileRefDto>,
    path: Option<PathRefDto>,
    guide: Option<PathRefDto>,
    guide_enabled: bool,
    centerline_enabled: bool,
    orientation: SweepOrientation,
    transition: SweepTransition,
    force_c1: bool,
    ruled: bool,
    continuity: LoftContinuity,
}
impl PathFields {
    pub fn new(kind: SolidFormKind) -> Self {
        Self {
            kind,
            sections: vec![],
            path: None,
            guide: None,
            guide_enabled: false,
            centerline_enabled: false,
            orientation: SweepOrientation::CorrectedFrenet,
            transition: if kind == SolidFormKind::Sweep {
                SweepTransition::RightCorner
            } else {
                SweepTransition::Transformed
            },
            force_c1: false,
            ruled: false,
            continuity: LoftContinuity::G0,
        }
    }
    pub(super) fn notes(&self) -> Vec<String> {
        if self.kind == SolidFormKind::Sweep && self.transition == SweepTransition::Transformed {
            vec!["Transformed can self-intersect at sharp path bends. Choose Right corner or Round corner for bends.".into()]
        } else {
            vec![]
        }
    }
    pub fn set(&mut self, field: SolidField, value: &str) -> Result<(), String> {
        let toggle = || match value {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err("Choose true or false".to_owned()),
        };
        match field {
            SolidField::GuideEnabled => self.guide_enabled = toggle()?,
            SolidField::CenterlineEnabled if self.kind == SolidFormKind::Loft => {
                self.centerline_enabled = toggle()?
            }
            SolidField::Orientation if self.kind == SolidFormKind::Sweep => {
                self.orientation =
                    serde_json::from_value(json!(value)).map_err(|e| e.to_string())?
            }
            SolidField::Transition if self.kind == SolidFormKind::Sweep => {
                self.transition = serde_json::from_value(json!(value)).map_err(|e| e.to_string())?
            }
            SolidField::ForceC1 if self.kind == SolidFormKind::Sweep => self.force_c1 = toggle()?,
            SolidField::Ruled if self.kind == SolidFormKind::Loft => self.ruled = toggle()?,
            SolidField::Continuity if self.kind == SolidFormKind::Loft => {
                self.continuity = serde_json::from_value(json!(value)).map_err(|e| e.to_string())?
            }
            _ => return Err("This feature does not have that option".into()),
        }
        Ok(())
    }
}

pub(super) fn validate_path(path: &PathRefDto, model: &FormModel<'_>) -> Result<(), String> {
    let sketch = model
        .profiles
        .iter()
        .find(|s| s.sketch_name == path.sketch_name)
        .ok_or("The path sketch no longer exists")?;
    if path.entity_ids.is_empty() {
        return Err("Select at least one path curve".into());
    }
    for (index, id) in path.entity_ids.iter().enumerate() {
        if path.entity_ids[..index].contains(id) {
            return Err("A path curve was selected twice".into());
        }
        if !sketch.path_curves.iter().any(|c| c.entity_id() == *id) {
            return Err(format!("Curve {id} is no longer a usable path"));
        }
    }
    Ok(())
}

impl SolidForm {
    pub(crate) fn selected_profiles(&self) -> Vec<ProfileRefDto> {
        if let Some(paths) = &self.paths {
            return paths.sections.clone();
        }
        match &self.source {
            ProfileSource::Profiles {
                sketch_name,
                indices,
            } => indices
                .iter()
                .map(|index| ProfileRefDto {
                    sketch_name: sketch_name.clone(),
                    profile_index: *index,
                })
                .collect(),
            _ => vec![],
        }
    }
    pub(crate) fn targets(&self) -> &[BodyId] {
        &self.targets
    }
    pub(crate) fn set_profiles(
        &mut self,
        profiles: Vec<ProfileRefDto>,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        if let Some(paths) = &self.paths {
            if paths.kind == SolidFormKind::Sweep && profiles.len() > 1 {
                return Err("Sweep needs exactly one profile".into());
            }
            for (index, profile) in profiles.iter().enumerate() {
                if profiles[..index].contains(profile) {
                    return Err("A section was selected twice".into());
                }
                validate_source(
                    &ProfileSource::Profiles {
                        sketch_name: profile.sketch_name.clone(),
                        indices: vec![profile.profile_index],
                    },
                    model,
                )?;
            }
        } else if profiles.iter().any(|p| {
            profiles
                .first()
                .is_some_and(|f| f.sketch_name != p.sketch_name)
        }) {
            return Err("Choose profiles from one sketch".into());
        }
        let source = profiles
            .first()
            .map(|first| ProfileSource::Profiles {
                sketch_name: first.sketch_name.clone(),
                indices: profiles
                    .iter()
                    .filter(|p| p.sketch_name == first.sketch_name)
                    .map(|p| p.profile_index)
                    .collect(),
            })
            .unwrap_or(ProfileSource::None);
        self.set_source(source, model)?;
        if let Some(paths) = &mut self.paths {
            paths.sections = profiles;
        }
        Ok(())
    }
    pub(crate) fn path(&self, field: SolidField) -> Option<&PathRefDto> {
        if let Some(rib) = &self.rib {
            return (field == SolidField::Path)
                .then_some(rib.centerline.as_ref())
                .flatten();
        }
        let p = self.paths.as_ref()?;
        match field {
            SolidField::Path => p.path.as_ref(),
            SolidField::Guide => p.guide.as_ref(),
            _ => None,
        }
    }
    pub(crate) fn selected_paths(&self) -> Vec<&PathRefDto> {
        if let Some(rib) = &self.rib {
            return rib.centerline.iter().collect();
        }
        let Some(p) = &self.paths else { return vec![] };
        let mut paths = vec![];
        if p.kind == SolidFormKind::Sweep || p.centerline_enabled {
            paths.extend(p.path.as_ref());
        }
        if p.guide_enabled {
            paths.extend(p.guide.as_ref());
        }
        paths
    }
    pub(crate) fn set_path(
        &mut self,
        field: SolidField,
        path: Option<PathRefDto>,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        if let Some(path) = &path {
            validate_path(path, model)?;
        }
        if let Some(rib) = &mut self.rib {
            if field != SolidField::Path {
                return Err("Rib uses centerline curves".into());
            }
            rib.centerline = path;
            self.changed();
            return Ok(());
        }
        let p = self
            .paths
            .as_mut()
            .ok_or("This feature has no curve path")?;
        match field {
            SolidField::Path => p.path = path,
            SolidField::Guide => p.guide = path,
            _ => return Err("Choose a path field".into()),
        }
        self.changed();
        Ok(())
    }
    pub(crate) fn edit_sweep(
        d: &SweepDefinitionDto,
        model: &FormModel<'_>,
    ) -> Result<Self, String> {
        let mut form = Self::new_kind(SolidFormKind::Sweep, model);
        form.load_path_feature(d.feature_id, FeatureKind::Sweep, model)?;
        form.operation = d.operation;
        form.operation_manual = true;
        form.targets = d.target_body_ids.clone();
        form.source = ProfileSource::Profiles {
            sketch_name: d.profile.sketch_name.clone(),
            indices: vec![d.profile.profile_index],
        };
        let p = form.paths.as_mut().unwrap();
        p.sections = vec![d.profile.clone()];
        p.path = Some(PathRefDto {
            sketch_name: d.path_sketch_name.clone(),
            entity_ids: d.path_entity_ids.clone(),
        });
        p.guide_enabled = d.guide_rail.is_some();
        p.guide = d.guide_rail.clone();
        p.orientation = d.orientation;
        p.transition = d.transition;
        p.force_c1 = d.force_c1;
        Ok(form)
    }
    pub(crate) fn edit_loft(d: &LoftDefinitionDto, model: &FormModel<'_>) -> Result<Self, String> {
        let mut form = Self::new_kind(SolidFormKind::Loft, model);
        form.load_path_feature(d.feature_id, FeatureKind::Loft, model)?;
        form.operation = d.operation;
        form.operation_manual = true;
        form.targets = d.target_body_ids.clone();
        form.source = d
            .sections
            .first()
            .map(|p| ProfileSource::Profiles {
                sketch_name: p.sketch_name.clone(),
                indices: vec![p.profile_index],
            })
            .unwrap_or(ProfileSource::None);
        let p = form.paths.as_mut().unwrap();
        p.sections = d.sections.clone();
        p.path = d.centerline.clone();
        p.centerline_enabled = p.path.is_some();
        p.guide = d.guide_rail.clone();
        p.guide_enabled = p.guide.is_some();
        p.ruled = d.ruled;
        p.continuity = d.continuity;
        Ok(form)
    }
    fn load_path_feature(
        &mut self,
        id: FeatureId,
        kind: FeatureKind,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        if !model
            .document
            .features
            .iter()
            .any(|f| f.id == id && f.kind == kind)
        {
            return Err("The selected feature no longer exists".into());
        }
        self.feature = Some(id);
        Ok(())
    }
    pub(super) fn path_payload(
        &self,
        model: &FormModel<'_>,
    ) -> Result<(&'static str, Value), Vec<(SolidField, String)>> {
        use SolidField as F;
        let p = self.paths.as_ref().unwrap();
        let mut errors = vec![];
        if let Err(e) = self.check_model(model) {
            return Err(vec![(F::Source, e)]);
        }
        let kind = if p.kind == SolidFormKind::Sweep {
            FeatureKind::Sweep
        } else {
            FeatureKind::Loft
        };
        if self.feature.is_some_and(|id| {
            !model
                .document
                .features
                .iter()
                .any(|f| f.id == id && f.kind == kind)
        }) {
            errors.push((F::Source, "The edited feature no longer exists".into()));
        }
        if (p.kind == SolidFormKind::Sweep && p.sections.len() != 1)
            || (p.kind == SolidFormKind::Loft && p.sections.len() < 2)
        {
            errors.push((
                F::Source,
                if p.kind == SolidFormKind::Sweep {
                    "Select one closed profile"
                } else {
                    "Select at least two sections in order"
                }
                .into(),
            ));
        }
        for section in &p.sections {
            if let Err(e) = validate_source(
                &ProfileSource::Profiles {
                    sketch_name: section.sketch_name.clone(),
                    indices: vec![section.profile_index],
                },
                model,
            ) {
                errors.push((F::Source, e));
            }
        }
        for (field, enabled, path) in [
            (
                F::Path,
                p.kind == SolidFormKind::Sweep || p.centerline_enabled,
                p.path.as_ref(),
            ),
            (F::Guide, p.guide_enabled, p.guide.as_ref()),
        ] {
            if enabled {
                match path {
                    Some(path) => {
                        if let Err(e) = validate_path(path, model) {
                            errors.push((field, e));
                        } else if let Some(sketch) = model
                            .profiles
                            .iter()
                            .find(|s| s.sketch_name == path.sketch_name)
                        {
                            if let Err(e) = limo_cad_solid::ordered_path(sketch, &path.entity_ids) {
                                errors.push((field, e.to_string()));
                            }
                        }
                    }
                    None => errors.push((field, "Select the path curves".into())),
                }
            }
        }
        if self.operation != ExtrudeOperation::NewBody {
            if self.targets.is_empty() {
                errors.push((F::Targets, "Select a target body".into()));
            }
            if let Err(e) = validate_targets(&self.targets, model) {
                errors.push((F::Targets, e));
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        let targets = if self.operation == ExtrudeOperation::NewBody {
            vec![]
        } else {
            self.targets.clone()
        };
        let guide = p.guide_enabled.then(|| p.guide.clone()).flatten();
        let (op, value) = if p.kind == SolidFormKind::Sweep {
            let path = p.path.as_ref().unwrap();
            let request = SweepRequest {
                profile: p.sections[0].clone(),
                path_sketch_name: path.sketch_name.clone(),
                path_entity_ids: path.entity_ids.clone(),
                operation: self.operation,
                target_body_ids: targets,
                guide_rail: guide,
                orientation: p.orientation,
                transition: p.transition,
                force_c1: p.force_c1,
            };
            if let Some(feature_id) = self.feature {
                (
                    "solid_edit_sweep",
                    serde_json::to_value(EditSweepRequest {
                        feature_id,
                        sweep: request,
                    }),
                )
            } else {
                ("solid_sweep", serde_json::to_value(request))
            }
        } else {
            let request = LoftRequest {
                sections: p.sections.clone(),
                ruled: p.ruled,
                continuity: p.continuity,
                operation: self.operation,
                target_body_ids: targets,
                guide_rail: guide,
                centerline: p.centerline_enabled.then(|| p.path.clone()).flatten(),
            };
            if let Some(feature_id) = self.feature {
                (
                    "solid_edit_loft",
                    serde_json::to_value(EditLoftRequest {
                        feature_id,
                        loft: request,
                    }),
                )
            } else {
                ("solid_loft", serde_json::to_value(request))
            }
        };
        value
            .map(|v| (op, v))
            .map_err(|e| vec![(F::Source, e.to_string())])
    }
    pub(super) fn path_fields(&self, model: &FormModel<'_>) -> Vec<SolidFieldView> {
        use SolidField as F;
        let p = self.paths.as_ref().unwrap();
        let errors = self.path_payload(model).err().unwrap_or_default();
        let enabled = self.phase == Phase::Editing && self.check_model(model).is_ok();
        let choice = |value: Value, options: &[(&str, &str)]| Field::Choice {
            value: value.as_str().unwrap().into(),
            options: options
                .iter()
                .map(|(value, label)| ChoiceOption {
                    value: (*value).into(),
                    label: (*label).into(),
                    disabled: false,
                })
                .collect(),
        };
        let path_label = |path: Option<&PathRefDto>, empty: &str| {
            path.map(|p| format!("{} · {} curve(s)", p.sketch_name, p.entity_ids.len()))
                .unwrap_or_else(|| empty.into())
        };
        let source = if p.sections.is_empty() {
            if p.kind == SolidFormKind::Sweep {
                "Select profile".into()
            } else {
                "Select sections in order".into()
            }
        } else {
            p.sections
                .iter()
                .enumerate()
                .map(|(i, s)| format!("{}. {} · {}", i + 1, s.sketch_name, s.profile_index + 1))
                .collect::<Vec<_>>()
                .join("; ")
        };
        let sweep = p.kind == SolidFormKind::Sweep;
        let rows = vec![
            (F::Source, source, Field::None, true),
            (
                F::Path,
                path_label(
                    p.path.as_ref(),
                    if sweep {
                        "Select path curves"
                    } else {
                        "Select centerline curves"
                    },
                ),
                Field::None,
                sweep,
            ),
            (
                F::Orientation,
                "Orientation".into(),
                choice(
                    json!(p.orientation),
                    &[
                        ("corrected_frenet", "Corrected Frenet"),
                        ("frenet", "Frenet"),
                        ("fixed", "Fixed profile"),
                    ],
                ),
                sweep,
            ),
            (
                F::Transition,
                "Corner transition".into(),
                choice(
                    json!(p.transition),
                    &[
                        ("transformed", "Transformed"),
                        ("right_corner", "Right corner"),
                        ("round_corner", "Round corner"),
                    ],
                ),
                sweep,
            ),
            (
                F::ForceC1,
                "Force C1 continuity".into(),
                Field::Toggle(p.force_c1),
                sweep,
            ),
            (
                F::Ruled,
                "Ruled surfaces".into(),
                Field::Toggle(p.ruled),
                !sweep,
            ),
            (
                F::Continuity,
                "Section continuity".into(),
                choice(
                    json!(p.continuity),
                    &[
                        ("g0", "G0 — position"),
                        ("g1", "G1 — tangent"),
                        ("g2", "G2 — curvature"),
                    ],
                ),
                !sweep,
            ),
            (
                F::CenterlineEnabled,
                "Use centerline".into(),
                Field::Toggle(p.centerline_enabled),
                !sweep,
            ),
            (
                F::Path,
                path_label(p.path.as_ref(), "Select centerline curves"),
                Field::None,
                !sweep && p.centerline_enabled,
            ),
            (
                F::GuideEnabled,
                "Use guide rail".into(),
                Field::Toggle(p.guide_enabled),
                true,
            ),
            (
                F::Guide,
                path_label(p.guide.as_ref(), "Select guide curves"),
                Field::None,
                p.guide_enabled,
            ),
            (
                F::Operation,
                "Operation".into(),
                choice(
                    json!(self.operation),
                    &[
                        ("new_body", "Create Body"),
                        ("join", "Add"),
                        ("cut", "Subtract"),
                        ("intersect", "Common"),
                    ],
                ),
                true,
            ),
            (
                F::Targets,
                format!("Target bodies ({})", self.targets.len()),
                Field::None,
                self.operation != ExtrudeOperation::NewBody,
            ),
        ];
        rows.into_iter()
            .map(|(field, label, value, visible)| SolidFieldView {
                field,
                label,
                value,
                visible,
                enabled,
                error: errors
                    .iter()
                    .find(|(f, _)| *f == field)
                    .map(|(_, e)| e.clone()),
            })
            .collect()
    }
}
