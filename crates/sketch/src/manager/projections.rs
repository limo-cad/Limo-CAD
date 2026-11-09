use super::*;
use crate::projection::{
    project_sketch_curve, ProjectCurvesRequest, ProjectionBinding, SketchCurveRef, MAX_PROJECTIONS,
    MAX_PROJECTION_ID,
};

impl SketchManager {
    pub(super) fn resolve_projection(
        &self,
        target: FeatureId,
        basis: PlaneBasis,
        binding: &ProjectionBinding,
    ) -> Result<ProjectedEdgeDto, String> {
        let history = &self.document.features().features;
        let position = |id| history.iter().position(|feature| feature.id == id);
        let source_index =
            position(binding.source.feature).ok_or("Projection source feature is missing")?;
        let target_index = position(target).ok_or("Projection target sketch is missing")?;
        if source_index >= target_index {
            return Err("Projection source must precede its target sketch".into());
        }
        let target_session = self
            .finished
            .iter()
            .find(|finished| finished.feature_id == target)
            .map(|finished| &finished.session)
            .or_else(|| {
                (self.active_feature_id == Some(target))
                    .then_some(self.active.as_ref())
                    .flatten()
            });
        if target_session.is_some_and(|session| session.support_has_edge(binding.edge_id())) {
            return Err(
                "Projection identity conflicts with a support edge; recreate the projection".into(),
            );
        }
        if history[source_index].suppressed
            || !matches!(history[source_index].status, FeatureStatus::Ok)
        {
            return Err("Projection source is suppressed or invalid".into());
        }
        let source = self
            .finished
            .iter()
            .find(|source| source.feature_id == binding.source.feature)
            .ok_or("Finish editing the source sketch before using its projection")?;
        project_sketch_curve(
            binding.source,
            source.feature_id,
            source.session.sketch(),
            &source.session.basis(),
            target,
            &basis,
            binding,
        )
        .map_err(|error| format!("Projection source cannot be resolved: {error:?}"))
    }

    pub fn projection_sources(&self) -> Result<serde_json::Value, SessionError> {
        let target = self.active_feature_id.ok_or(SessionError::NoActiveSketch)?;
        let basis = self
            .active
            .as_ref()
            .ok_or(SessionError::NoActiveSketch)?
            .basis();
        let mut sources = Vec::new();
        for finished in &self.finished {
            for (entity, geometry) in finished.session.sketch().entities() {
                if matches!(geometry, crate::Entity::Point { .. }) {
                    continue;
                }
                let source = SketchCurveRef {
                    feature: finished.feature_id,
                    entity,
                };
                let binding = ProjectionBinding { id: 1, source };
                if self.resolve_projection(target, basis, &binding).is_ok() {
                    let kind = match geometry {
                        crate::Entity::Line { .. } => "line",
                        crate::Entity::Circle { .. } => "circle",
                        crate::Entity::Arc { .. } => "arc",
                        crate::Entity::Spline { .. } => "spline",
                        _ => unreachable!(),
                    };
                    sources.push(serde_json::json!({"source":source,"sketch":finished.session.name(),"kind":kind,"entity":entity.0}));
                }
                if sources.len() >= 512 {
                    break;
                }
            }
            if sources.len() >= 512 {
                break;
            }
        }
        Ok(serde_json::json!({"sources":sources,"supported":"sketch_curves","maximum":512}))
    }

    pub fn sketch_projections(&self) -> Result<serde_json::Value, SessionError> {
        let target = self.active_feature_id.ok_or(SessionError::NoActiveSketch)?;
        let session = self.active.as_ref().ok_or(SessionError::NoActiveSketch)?;
        Ok(
            serde_json::json!({"projections":session.projection_bindings().iter().map(|binding|
            serde_json::json!({"id":binding.id,"source":binding.source,"error":self.resolve_projection(target,session.basis(),binding).err()})
        ).collect::<Vec<_>>() }),
        )
    }

    pub fn project_curves(
        &mut self,
        request: ProjectCurvesRequest,
    ) -> Result<SketchDto, SessionError> {
        if self.pending_project.is_some() {
            return Err(SessionError::Solid(
                "Finish the pending project open before changing projections".into(),
            ));
        }
        let target = self.active_feature_id.ok_or(SessionError::NoActiveSketch)?;
        let active = self.active.as_ref().ok_or(SessionError::NoActiveSketch)?;
        if request.sources.is_empty()
            || request.sources.len() > MAX_PROJECTIONS
            || active.projection_bindings().len() + request.sources.len() > MAX_PROJECTIONS
        {
            return Err(SessionError::Solid(
                "Select between 1 and 128 projection sources".into(),
            ));
        }
        let mut bindings = active.projection_bindings().to_vec();
        let mut seen = bindings
            .iter()
            .map(|binding| binding.source)
            .collect::<BTreeSet<_>>();
        let first = active.next_projection_id();
        for (index, source) in request.sources.into_iter().enumerate() {
            if !seen.insert(source) {
                return Err(SessionError::Solid(
                    "This source is already projected into the sketch".into(),
                ));
            }
            let id = first
                .checked_add(index as u32)
                .filter(|id| *id <= MAX_PROJECTION_ID)
                .ok_or_else(|| {
                    SessionError::Solid("Projection identity budget exhausted".into())
                })?;
            let binding = ProjectionBinding { id, source };
            let edge = self
                .resolve_projection(target, active.basis(), &binding)
                .map_err(SessionError::Solid)?;
            if active.sketch().reference_edge(edge.edge_id).is_some() {
                return Err(SessionError::Solid(
                    "Projection identity conflicts with an existing reference".into(),
                ));
            }
            bindings.push(binding);
        }
        let edges: Vec<_> = bindings
            .iter()
            .filter_map(|binding| {
                self.resolve_projection(target, active.basis(), binding)
                    .ok()
            })
            .collect();
        self.active
            .as_mut()
            .unwrap()
            .install_projections(bindings, &edges)?;
        self.refresh_projection_references();
        Ok(self.active.as_ref().unwrap().dto())
    }

    pub fn remove_projection(&mut self, id: u32) -> Result<SketchDto, SessionError> {
        if self.pending_project.is_some() {
            return Err(SessionError::Solid(
                "Finish the pending project open before changing projections".into(),
            ));
        }
        let target = self.active_feature_id.ok_or(SessionError::NoActiveSketch)?;
        let active = self.active.as_ref().ok_or(SessionError::NoActiveSketch)?;
        if !active
            .projection_bindings()
            .iter()
            .any(|binding| binding.id == id)
        {
            return Err(SessionError::Solid("Projection no longer exists".into()));
        }
        if active.projection_has_constraints(id) {
            return Err(SessionError::Solid(
                "Remove constraints using this projection before deleting it".into(),
            ));
        }
        let bindings: Vec<_> = active
            .projection_bindings()
            .iter()
            .filter(|binding| binding.id != id)
            .cloned()
            .collect();
        let edges: Vec<_> = bindings
            .iter()
            .filter_map(|binding| {
                self.resolve_projection(target, active.basis(), binding)
                    .ok()
            })
            .collect();
        self.active
            .as_mut()
            .unwrap()
            .install_projections(bindings, &edges)?;
        self.refresh_projection_references();
        Ok(self.active.as_ref().unwrap().dto())
    }

    pub(crate) fn refresh_projection_references(&mut self) {
        // Source sketches precede targets. Updating in history order makes
        // longer reference chains deterministic; external projections are not
        // authored entities, so a link cannot impersonate another source line.
        let targets: Vec<_> = self
            .finished
            .iter()
            .map(|finished| {
                (
                    finished.feature_id,
                    finished.session.basis(),
                    finished.session.all_projection_bindings(),
                )
            })
            .chain(self.active.iter().filter_map(|session| {
                self.active_feature_id
                    .map(|id| (id, session.basis(), session.all_projection_bindings()))
            }))
            .collect();
        for (target, basis, bindings) in targets {
            if bindings.is_empty() {
                continue;
            }
            let mut resolved = BTreeMap::new();
            let mut errors = BTreeMap::new();
            for binding in bindings {
                match self.resolve_projection(target, basis, &binding) {
                    Ok(edge) => {
                        resolved.insert(binding.id, edge);
                    }
                    Err(error) => {
                        errors.insert(binding.id, error);
                    }
                }
            }
            let session = if self.active_feature_id == Some(target) {
                self.active.as_mut()
            } else {
                self.finished
                    .iter_mut()
                    .find(|finished| finished.feature_id == target)
                    .map(|finished| &mut finished.session)
            };
            if let Some(session) = session {
                let error = session
                    .projection_bindings()
                    .iter()
                    .find_map(|binding| errors.get(&binding.id))
                    .cloned();
                session.refresh_projection_geometry(&resolved);
                if let Some(error) = error {
                    self.document.set_feature_status(target, FeatureStatus::Error {message:format!("Projection: {error}")});
                } else if self.document.features().features.iter().any(|feature|feature.id == target && matches!(&feature.status,FeatureStatus::Error {message} if message.starts_with("Projection:"))) {
                    self.document.set_feature_status(target,FeatureStatus::Ok);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DragPhase, Vec2};
    use limo_cad_core::OriginPlane;
    use limo_cad_solid::{CommitKernelRequest, KernelSceneDto};

    fn fixture() -> (SketchManager, SketchCurveRef, EntityId) {
        let mut manager = SketchManager::new();
        let plane = PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        };
        manager.begin_sketch(plane).unwrap();
        manager
            .set_grid_snap(crate::SetGridSnapRequest { enabled: false })
            .unwrap();
        let line = manager
            .active_mut()
            .unwrap()
            .add_line(Vec2::new(1., 2.), Vec2::new(8., 6.), true)
            .unwrap();
        let source = SketchCurveRef {
            feature: manager.active_feature_id.unwrap(),
            entity: line.entity_id,
        };
        manager.end_sketch().unwrap();
        manager.begin_sketch(plane).unwrap();
        (manager, source, line.end_point_id)
    }

    #[test]
    fn projection_binding_is_undoable_read_only_geometry_and_ids_are_not_reused() {
        let (mut manager, source, _) = fixture();
        let before = manager.active_snapshot().unwrap();
        let projected = manager
            .project_curves(ProjectCurvesRequest {
                sources: vec![source],
            })
            .unwrap();
        assert_eq!(projected.entities, before.entities);
        assert_eq!(projected.dof, before.dof);
        assert_eq!(projected.projected_edges.len(), 1);
        assert_eq!(
            projected.projected_edges[0].points,
            vec![Vec2::new(1., 2.), Vec2::new(8., 6.)]
        );
        let original = manager.active.as_ref().unwrap().projection_bindings()[0].id;
        assert!(manager.undo().unwrap().sketch.projected_edges.is_empty());
        assert_eq!(
            manager.redo().unwrap().sketch.projected_edges,
            projected.projected_edges
        );
        assert!(manager
            .remove_projection(original)
            .unwrap()
            .projected_edges
            .is_empty());
        manager
            .project_curves(ProjectCurvesRequest {
                sources: vec![source],
            })
            .unwrap();
        assert!(manager.active.as_ref().unwrap().projection_bindings()[0].id > original);
    }

    #[test]
    fn projection_refreshes_after_source_edit_and_missing_source_never_uses_cached_geometry() {
        let (mut manager, source, end) = fixture();
        manager
            .project_curves(ProjectCurvesRequest {
                sources: vec![source],
            })
            .unwrap();
        manager.end_sketch().unwrap();
        manager.edit_sketch("Sketch1").unwrap();
        manager
            .active_mut()
            .unwrap()
            .move_point(MovePointRequest {
                point_id: end,
                to_raw: Vec2::new(12., 9.),
                ctrl_held: true,
                phase: DragPhase::Single,
            })
            .unwrap();
        manager.end_sketch().unwrap();
        assert_eq!(
            manager.finished_sketches()[1].projected_edges[0].points[1],
            Vec2::new(12., 9.)
        );
        manager.edit_sketch("Sketch1").unwrap();
        manager
            .active_mut()
            .unwrap()
            .delete_entity(source.entity)
            .unwrap();
        manager.end_sketch().unwrap();
        assert!(manager.finished_sketches()[1].projected_edges.is_empty());
        assert!(
            matches!(&manager.document.features().features[1].status,FeatureStatus::Error {message} if message.starts_with("Projection:"))
        );
        manager.edit_sketch("Sketch1").unwrap();
        manager.undo().unwrap();
        manager.end_sketch().unwrap();
        assert_eq!(
            manager.finished_sketches()[1].projected_edges[0].points[1],
            Vec2::new(12., 9.)
        );
        assert!(matches!(
            manager.document.features().features[1].status,
            FeatureStatus::Ok
        ));
    }

    #[test]
    fn projection_cold_model_reopen_rebuilds_association_and_old_schema_rejects_it() {
        let (mut manager, source, _) = fixture();
        let expected = manager
            .project_curves(ProjectCurvesRequest {
                sources: vec![source],
            })
            .unwrap()
            .projected_edges;
        manager.end_sketch().unwrap();
        let model = manager.export_project_model().unwrap();
        let mut reopened = SketchManager::new();
        let plan = reopened.prepare_load_project(model.clone()).unwrap();
        assert!(plan.jobs.is_empty());
        reopened
            .commit_solid(CommitKernelRequest {
                transaction_id: plan.transaction_id,
                scene: KernelSceneDto::default(),
            })
            .unwrap();
        assert_eq!(
            reopened.edit_sketch("Sketch2").unwrap().projected_edges,
            expected
        );
        let mut old: serde_json::Value = serde_json::from_str(&model).unwrap();
        old["schema_version"] = 16.into();
        assert!(SketchManager::new()
            .prepare_load_project(old.to_string())
            .is_err());
    }

    #[test]
    fn projection_bad_batch_duplicates_and_reverse_dependency_are_atomic() {
        let (mut manager, source, _) = fixture();
        let before = serde_json::to_value(manager.active_snapshot()).unwrap();
        for sources in [
            vec![],
            vec![source, source],
            vec![
                source,
                SketchCurveRef {
                    entity: EntityId(999),
                    ..source
                },
            ],
        ] {
            assert!(manager
                .project_curves(ProjectCurvesRequest { sources })
                .is_err());
            assert_eq!(
                serde_json::to_value(manager.active_snapshot()).unwrap(),
                before
            );
        }
        manager.end_sketch().unwrap();
        manager.edit_sketch("Sketch1").unwrap();
        let before = serde_json::to_value(manager.active_snapshot()).unwrap();
        assert!(manager
            .project_curves(ProjectCurvesRequest {
                sources: vec![source]
            })
            .is_err());
        assert!(manager
            .project_curves(ProjectCurvesRequest {
                sources: vec![SketchCurveRef {
                    feature: FeatureId(2),
                    entity: EntityId(1)
                }]
            })
            .is_err());
        assert_eq!(
            serde_json::to_value(manager.active_snapshot()).unwrap(),
            before
        );
    }

    #[test]
    fn missing_projection_blocks_profile_replay_and_survives_cold_reopen_for_repair() {
        let (mut manager, source, _) = fixture();
        manager
            .project_curves(ProjectCurvesRequest {
                sources: vec![source],
            })
            .unwrap();
        manager.end_sketch().unwrap();
        let target = manager.finished[1].feature_id;
        assert!(manager
            .profile_catalog_at(2)
            .iter()
            .any(|item| item.feature_id == target));
        manager.edit_sketch("Sketch1").unwrap();
        manager
            .active_mut()
            .unwrap()
            .delete_entity(source.entity)
            .unwrap();
        manager.end_sketch().unwrap();
        assert!(!manager
            .profile_catalog_at(2)
            .iter()
            .any(|item| item.feature_id == target));
        let model = manager.export_project_model().unwrap();
        let mut reopened = SketchManager::new();
        let plan = reopened.prepare_load_project(model).unwrap();
        reopened
            .commit_solid(CommitKernelRequest {
                transaction_id: plan.transaction_id,
                scene: KernelSceneDto::default(),
            })
            .unwrap();
        assert!(reopened
            .edit_sketch("Sketch2")
            .unwrap()
            .projected_edges
            .is_empty());
        assert_eq!(
            reopened.sketch_projections().unwrap()["projections"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(reopened.sketch_projections().unwrap()["projections"][0]["error"].is_string());
        assert!(reopened
            .remove_projection(1)
            .unwrap()
            .projected_edges
            .is_empty());
    }

    #[test]
    fn projected_curve_snaps_and_constraints_follow_source_and_fence_removal() {
        let (mut manager, source, end) = fixture();
        let result = manager
            .project_curves(ProjectCurvesRequest {
                sources: vec![source],
            })
            .unwrap();
        let edge = result.projected_edges[0].edge_id;
        manager
            .set_grid_snap(crate::SetGridSnapRequest { enabled: true })
            .unwrap();
        let point = manager
            .add_point(crate::PointRequest {
                position: Vec2::new(3., 22. / 7.),
                coincident_with: None,
                ctrl_held: false,
            })
            .unwrap();
        let attached = point
            .sketch
            .constraints
            .iter()
            .find_map(|constraint| match constraint.constraint {
                Constraint::ReferenceOnEdge {
                    point,
                    edge: actual,
                } if actual == edge => Some(point),
                _ => None,
            })
            .expect("explicit projection must use production curve snapping");
        assert!(manager.remove_projection(1).is_err());
        manager.end_sketch().unwrap();
        manager.edit_sketch("Sketch1").unwrap();
        manager
            .set_grid_snap(crate::SetGridSnapRequest { enabled: false })
            .unwrap();
        manager
            .active_mut()
            .unwrap()
            .move_point(MovePointRequest {
                point_id: end,
                to_raw: Vec2::new(12., 9.),
                ctrl_held: true,
                phase: DragPhase::Single,
            })
            .unwrap();
        manager.end_sketch().unwrap();
        let target = manager
            .finished
            .iter()
            .find(|finished| finished.session.name() == "Sketch2")
            .unwrap();
        let position = target.session.sketch().point_position(attached).unwrap();
        let line = target.session.sketch().projection_edges()[0].clone();
        assert!(line.closest_point(position).unwrap().distance(position) < 1e-7);
        let model = manager.export_project_model().unwrap();
        let mut reopened = SketchManager::new();
        let plan = reopened.prepare_load_project(model).unwrap();
        reopened
            .commit_solid(CommitKernelRequest {
                transaction_id: plan.transaction_id,
                scene: KernelSceneDto::default(),
            })
            .unwrap();
        let dto = reopened.edit_sketch("Sketch2").unwrap();
        assert!(dto.constraints.iter().any(|constraint|matches!(constraint.constraint,Constraint::ReferenceOnEdge{point,edge:actual} if point==attached && actual==edge)));
        assert!(reopened.remove_projection(1).is_err());
    }
}
