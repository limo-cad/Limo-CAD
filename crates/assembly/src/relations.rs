//! Driven assembly coordinates reuse the mechanism-drag closure solver.
//! Gear relations constrain unwrapped coordinates, never just rendered poses.
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateGearRelationRequestDto {
    pub name: String,
    pub joint_a: JointId,
    pub joint_b: JointId,
    pub teeth_a: u32,
    pub teeth_b: u32,
    #[serde(default = "default_true")]
    pub reverse: bool,
    #[serde(default)]
    pub phase_deg: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GearRelationDto {
    pub id: u64,
    #[serde(flatten)]
    pub specification: CreateGearRelationRequestDto,
}

impl std::ops::Deref for GearRelationDto {
    type Target = CreateGearRelationRequestDto;
    fn deref(&self) -> &Self::Target {
        &self.specification
    }
}

impl GearRelationDto {
    pub(super) fn ratio(&self) -> f64 {
        (if self.reverse { -1.0 } else { 1.0 }) * self.teeth_a as f64 / self.teeth_b as f64
    }

    pub(super) fn error_deg(&self, document: &AssemblyDocumentDto) -> Option<f64> {
        let a = document
            .joints
            .iter()
            .find(|joint| joint.id == self.joint_a)?;
        let b = document
            .joints
            .iter()
            .find(|joint| joint.id == self.joint_b)?;
        if !a.enabled || !b.enabled {
            return None;
        }
        Some(b.angle_offset_deg - self.phase_deg - self.ratio() * a.angle_offset_deg)
    }
}

impl AssemblyDocumentDto {
    pub(super) fn validate_gear_relations(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        let mut pairs = HashSet::new();
        for relation in &self.gear_relations {
            if relation.id == 0 || !ids.insert(relation.id) {
                return Err("Gear relation IDs must be unique and nonzero".into());
            }
            if relation.name.trim().is_empty() || !names.insert(relation.name.trim()) {
                return Err("Gear relation names must be nonempty and unique".into());
            }
            if relation.teeth_a == 0 || relation.teeth_b == 0 || !relation.phase_deg.is_finite() {
                return Err("Gear tooth counts must be positive and phase must be finite".into());
            }
            if relation.joint_a == relation.joint_b {
                return Err("A gear relation requires two different revolute joints".into());
            }
            let pair = if relation.joint_a.0 < relation.joint_b.0 {
                (relation.joint_a, relation.joint_b)
            } else {
                (relation.joint_b, relation.joint_a)
            };
            if !pairs.insert(pair) {
                return Err("A gear pair already has a relation".into());
            }
            let mut parent = None;
            for id in [relation.joint_a, relation.joint_b] {
                let joint = self
                    .joints
                    .iter()
                    .find(|joint| joint.id == id)
                    .ok_or_else(|| {
                        format!(
                            "Gear relation '{}' references missing joint {}",
                            relation.name, id.0
                        )
                    })?;
                if joint.kind != JointKindDto::Revolute {
                    return Err("Gear relations require revolute joints".into());
                }
                let occurrence = joint
                    .advanced
                    .connector_a_occurrence_id
                    .and_then(|id| self.component_structure.occurrence(id))
                    .ok_or("Gear relation joint must bind a component occurrence")?;
                let current = occurrence.parent_occurrence_id;
                if parent.is_some_and(|expected| expected != current) {
                    return Err("Gear relations cannot cross subassembly boundaries".into());
                }
                parent = Some(current);
            }
        }
        if self.next_gear_relation_id == 0 || ids.iter().any(|id| *id >= self.next_gear_relation_id)
        {
            return Err("Gear relation ID counter must exceed every saved relation ID".into());
        }
        Ok(())
    }

    pub fn create_gear_relation(
        &mut self,
        request: CreateGearRelationRequestDto,
        scene: &SolidSceneDto,
    ) -> Result<GearRelationDto, String> {
        let mut candidate = self.clone();
        let relation = GearRelationDto {
            id: candidate.next_gear_relation_id,
            specification: request,
        };
        candidate.next_gear_relation_id = candidate
            .next_gear_relation_id
            .checked_add(1)
            .ok_or("Gear relation IDs exhausted")?;
        candidate.gear_relations.push(relation.clone());
        candidate.validate_gear_relations()?;
        candidate.solve_driven_coordinates(
            &[(relation.joint_a, JointCoordinate::PrimaryAngle)],
            scene,
        )?;
        *self = candidate;
        Ok(relation)
    }

    pub fn update_gear_relation(
        &mut self,
        relation: GearRelationDto,
        scene: &SolidSceneDto,
    ) -> Result<GearRelationDto, String> {
        let mut candidate = self.clone();
        let saved = candidate
            .gear_relations
            .iter_mut()
            .find(|saved| saved.id == relation.id)
            .ok_or("Gear relation does not exist")?;
        *saved = relation.clone();
        candidate.validate_gear_relations()?;
        candidate.solve_driven_coordinates(
            &[(relation.joint_a, JointCoordinate::PrimaryAngle)],
            scene,
        )?;
        *self = candidate;
        Ok(relation)
    }

    pub fn delete_gear_relation(&mut self, id: u64) -> Result<(), String> {
        let before = self.gear_relations.len();
        self.gear_relations.retain(|relation| relation.id != id);
        if before == self.gear_relations.len() {
            return Err("Gear relation does not exist".into());
        }
        Ok(())
    }

    /// The selected primary coordinates are prescribed. Passive coordinates
    /// solve against all mechanism closures before the saved document changes.
    pub fn drive_joint_motion(
        &mut self,
        request: SetJointMotionRequestDto,
        scene: &SolidSceneDto,
    ) -> Result<(), String> {
        let mut candidate = self.clone();
        candidate.set_joint_motion(
            request.joint_id,
            request.angle_offset_deg,
            request.linear_offset_mm,
        )?;
        let joint = candidate
            .joints
            .iter()
            .find(|joint| joint.id == request.joint_id)
            .ok_or("Joint does not exist")?;
        if !joint.enabled {
            return Err("Cannot drive a suppressed joint".into());
        }
        let held: Vec<_> = active_coordinates(joint.kind)
            .iter()
            .copied()
            .filter(|coordinate| {
                matches!(
                    coordinate,
                    JointCoordinate::PrimaryAngle | JointCoordinate::PrimaryLinear
                )
            })
            .map(|coordinate| (joint.id, coordinate))
            .collect();
        candidate.solve_driven_coordinates(&held, scene)?;
        *self = candidate;
        Ok(())
    }

    pub fn drive_joint_coordinates(
        &mut self,
        motion: JointMotionStateDto,
        scene: &SolidSceneDto,
    ) -> Result<(), String> {
        let mut candidate = self.clone();
        candidate.set_joint_coordinates(motion.joint_id, motion)?;
        let joint = candidate
            .joints
            .iter()
            .find(|joint| joint.id == motion.joint_id)
            .ok_or("Joint does not exist")?;
        if !joint.enabled {
            return Err("Cannot drive a suppressed joint".into());
        }
        let held: Vec<_> = active_coordinates(joint.kind)
            .iter()
            .copied()
            .map(|coordinate| (joint.id, coordinate))
            .collect();
        candidate.solve_driven_coordinates(&held, scene)?;
        *self = candidate;
        Ok(())
    }

    pub(super) fn solve_driven_coordinates(
        &mut self,
        held: &[(JointId, JointCoordinate)],
        scene: &SolidSceneDto,
    ) -> Result<(), String> {
        self.validate_gear_relations()?;
        // History rollback can temporarily hide bodies and their joints. Solve
        // an active projection, then merge coordinates only; never discard
        // retained assembly intent needed by redo or a later feature marker.
        let mut active = self.clone();
        active.project_active_scene(scene)?;
        let held = held
            .iter()
            .copied()
            .filter(|(id, _)| active.joints.iter().any(|joint| joint.id == *id))
            .collect::<Vec<_>>();
        if held.is_empty() {
            return Ok(());
        }
        active.solve_active_driven_coordinates(&held, scene)?;
        self.apply_joint_motions(&active.current_joint_motions())
    }

    fn solve_active_driven_coordinates(
        &mut self,
        held: &[(JointId, JointCoordinate)],
        scene: &SolidSceneDto,
    ) -> Result<(), String> {
        let mut held = held.to_vec();
        // Propagate exact unwrapped gear coordinates in both directions. The
        // dependent gear coordinates also become fixed in the closure solve.
        let mut driven: HashMap<JointId, f64> = held
            .iter()
            .filter(|(_, coordinate)| *coordinate == JointCoordinate::PrimaryAngle)
            .map(|(id, _)| {
                (
                    *id,
                    self.joints
                        .iter()
                        .find(|joint| joint.id == *id)
                        .unwrap()
                        .angle_offset_deg,
                )
            })
            .collect();
        loop {
            let before = driven.len();
            for relation in &self.gear_relations {
                let a = self
                    .joints
                    .iter()
                    .find(|joint| joint.id == relation.joint_a)
                    .unwrap();
                let b = self
                    .joints
                    .iter()
                    .find(|joint| joint.id == relation.joint_b)
                    .unwrap();
                if !a.enabled || !b.enabled {
                    continue;
                }
                let next = if let Some(value) = driven.get(&relation.joint_a) {
                    Some((
                        relation.joint_b,
                        relation.phase_deg + relation.ratio() * value,
                    ))
                } else {
                    driven.get(&relation.joint_b).map(|value| {
                        (
                            relation.joint_a,
                            (value - relation.phase_deg) / relation.ratio(),
                        )
                    })
                };
                if let Some((id, value)) = next {
                    if let Some(previous) = driven.get(&id) {
                        if (value - previous).abs() > 1e-7 {
                            return Err(
                                "Prescribed gear coordinates or gear cycle are inconsistent".into(),
                            );
                        }
                    } else {
                        driven.insert(id, value);
                    }
                }
            }
            if before == driven.len() {
                break;
            }
        }
        let mut driven = driven.into_iter().collect::<Vec<_>>();
        driven.sort_by_key(|(id, _)| id.0);
        for (id, value) in driven {
            let joint = self.joints.iter_mut().find(|joint| joint.id == id).unwrap();
            ensure_motion_in_limits(&joint.name, "angle", value, effective_angle_limits(joint))?;
            joint.angle_offset_deg = value;
            if !held.contains(&(id, JointCoordinate::PrimaryAngle)) {
                held.push((id, JointCoordinate::PrimaryAngle));
            }
        }
        // Acyclic mechanisms and exactly propagated gears need no numerical solve.
        if self.solve(scene).solved {
            return Ok(());
        }
        let mut visited = HashSet::new();
        for (id, _) in held.clone() {
            if visited.contains(&id) {
                continue;
            }
            let joint = self
                .joints
                .iter()
                .find(|joint| joint.id == id)
                .ok_or("Driven joint disappeared")?;
            let occurrence = joint
                .advanced
                .connector_b_occurrence_id
                .ok_or("Driven joint has no occurrence")?;
            visited.extend(connected_joint_ids(self, occurrence));
            let request = MechanismDragRequestDto {
                body_id: joint.connector_b.body_id,
                occurrence_id: Some(occurrence),
                target_pose: BodyPoseDto {
                    body_id: joint.connector_b.body_id,
                    translation: [0.0; 3],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                },
                grab_point_local: None,
                target_point_world: None,
                initial_joint_motions: vec![],
                solve_orientation: false,
                maximum_iterations: 96,
            };
            let solved = solve_mechanism_coordinates(self, request, scene, &held, true)?;
            if !solved.converged {
                return Err(format!(
                    "Driven mechanism cannot close within joint limits: {:?}",
                    solved.solution.diagnostics
                ));
            }
            self.apply_joint_motions(&solved.joint_motions)?;
        }
        let solution = self.solve(scene);
        if !solution.solved {
            return Err(format!(
                "Driven mechanism remains inconsistent: {:?}",
                solution.diagnostics
            ));
        }
        Ok(())
    }
}
