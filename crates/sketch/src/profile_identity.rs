//! Persistent identities for discovered regions, independent of catalog order.
//!
//! A directed cycle of source curves survives dimension edits and translations.
//! It deliberately does not survive a split/merge that changes its boundary.
//! Ambiguous cycles (possible with self-intersecting splines) only match their
//! unchanged geometry; guessing by area/centroid could cut the wrong material.
use std::collections::{BTreeMap, BTreeSet};

use limo_cad_solid::{Point2Dto, ProfileCurveDto, ProfileLoopDto};
use serde::{Deserialize, Serialize};

use crate::dto::{EntityDto, SketchDto};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct ProfileIdentities {
    next: u32,
    records: Vec<Record>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Record {
    id: u32,
    key: Vec<(Vec<SourceIdentity>, i8)>,
    shape: Vec<(i64, i64)>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SourceIdentity {
    Entity(u64),
    Edge(u64),
}

fn canonical_cycle<T: Ord + Clone>(items: &[T]) -> Vec<T> {
    (0..items.len())
        .map(|start| {
            items[start..]
                .iter()
                .chain(&items[..start])
                .cloned()
                .collect()
        })
        .min()
        .unwrap_or_default()
}

fn shape(points: &[Point2Dto]) -> Vec<(i64, i64)> {
    canonical_cycle(
        &points
            .iter()
            .map(|p| ((p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64))
            .collect::<Vec<_>>(),
    )
}

fn key(sketch: &SketchDto, profile: &ProfileLoopDto) -> Vec<(Vec<SourceIdentity>, i8)> {
    let edges = profile
        .curves
        .iter()
        .map(|curve| {
            let (id, sources, direction) = match curve {
                ProfileCurveDto::Line {
                    entity_id,
                    source_entity_ids,
                    start,
                    end,
                } => {
                    let original = sketch
                        .entities
                        .iter()
                        .find_map(|e| match e {
                            EntityDto::Line { id, start, end, .. } if id.0 == *entity_id => {
                                Some((end.x - start.x, end.y - start.y))
                            }
                            _ => None,
                        })
                        .or_else(|| {
                            sketch
                                .projected_edges
                                .iter()
                                .find(|e| e.id == *entity_id)
                                .and_then(|e| e.points.first().zip(e.points.last()))
                                .map(|(a, b)| (b.x - a.x, b.y - a.y))
                        });
                    let direction = original.map_or(0, |(x, y)| {
                        if (end.x - start.x) * x + (end.y - start.y) * y >= 0.0 {
                            1
                        } else {
                            -1
                        }
                    });
                    (*entity_id, source_entity_ids, direction)
                }
                ProfileCurveDto::Arc {
                    entity_id,
                    source_entity_ids,
                    start,
                    mid,
                    end,
                } => (
                    *entity_id,
                    source_entity_ids,
                    if (mid.x - start.x) * (end.y - mid.y) - (mid.y - start.y) * (end.x - mid.x)
                        >= 0.0
                    {
                        1
                    } else {
                        -1
                    },
                ),
                ProfileCurveDto::Circle {
                    entity_id,
                    source_entity_ids,
                    ..
                } => (*entity_id, source_entity_ids, 1),
                ProfileCurveDto::Polyline {
                    entity_id,
                    source_entity_ids,
                    ..
                } => (*entity_id, source_entity_ids, 0),
            };
            let mut sources: Vec<_> = sources
                .iter()
                .copied()
                .chain([id])
                .map(|source| {
                    sketch
                        .projected_edges
                        .iter()
                        .find(|edge| edge.id == source)
                        .map_or(SourceIdentity::Entity(source), |edge| {
                            SourceIdentity::Edge(edge.edge_id.0)
                        })
                })
                .collect();
            sources.sort_unstable();
            sources.dedup();
            (sources, direction)
        })
        .collect::<Vec<_>>();
    canonical_cycle(&edges)
}

impl ProfileIdentities {
    pub(crate) fn validate(&self) -> Result<(), String> {
        let mut used = BTreeSet::new();
        for record in &self.records {
            if record.id >= self.next || !used.insert(record.id) || record.key.is_empty() {
                return Err("invalid saved profile identities".into());
            }
        }
        Ok(())
    }

    pub(crate) fn assign(&mut self, sketch: &SketchDto, profiles: &mut [ProfileLoopDto]) {
        let keys: Vec<_> = profiles.iter().map(|p| key(sketch, p)).collect();
        let mut counts = BTreeMap::new();
        for key in &keys {
            *counts.entry(key.clone()).or_insert(0) += 1;
        }
        let mut remap = BTreeMap::new();
        let mut assigned = BTreeSet::new();
        for (profile, key) in profiles.iter_mut().zip(keys) {
            let geometry = shape(&profile.points);
            let candidates: Vec<_> = self
                .records
                .iter()
                .enumerate()
                .filter(|(_, r)| r.key == key && !assigned.contains(&r.id))
                .map(|(i, _)| i)
                .collect();
            let found = if !key.is_empty() && candidates.len() == 1 && counts[&key] == 1 {
                candidates.first().copied()
            } else {
                let exact: Vec<_> = candidates
                    .into_iter()
                    .filter(|i| self.records[*i].shape == geometry)
                    .collect();
                (exact.len() == 1).then(|| exact[0])
            };
            let id = if let Some(i) = found {
                self.records[i].shape = geometry;
                self.records[i].id
            } else {
                let id = self.next;
                self.next = self
                    .next
                    .checked_add(1)
                    .expect("profile identity space exhausted");
                self.records.push(Record {
                    id,
                    key,
                    shape: geometry,
                });
                id
            };
            remap.insert(profile.index, id);
            assigned.insert(id);
            profile.index = id;
        }
        for profile in profiles {
            profile.parent_index = profile
                .parent_index
                .and_then(|old| remap.get(&old).copied());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProjectedEdgeDto, SketchSession, Vec2};
    use limo_cad_core::{EdgeId, FaceId, FeatureId, OriginPlane, PlaneRef};

    #[test]
    fn projected_identity_uses_body_edges_not_transient_catalog_positions() {
        let basis = PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        }
        .basis()
        .unwrap();
        let mut session = SketchSession::new(
            "face",
            PlaneRef::PlanarFace { face_id: FaceId(1) },
            basis,
            false,
        );
        let corners = [
            Vec2::new(0., 0.),
            Vec2::new(20., 0.),
            Vec2::new(20., 10.),
            Vec2::new(0., 10.),
        ];
        let mut edges: Vec<_> = (0..4)
            .map(|i| ProjectedEdgeDto {
                id: (1 << 40) + i as u64,
                edge_id: EdgeId(i as u64 + 1),
                points: vec![corners[i], corners[(i + 1) % 4]],
                circle: None,
            })
            .collect();
        session.set_projected_edges(edges.clone());
        session
            .add_line(Vec2::new(0., 4.), Vec2::new(20., 4.), true)
            .unwrap();
        session.refresh_profile_identities();
        let before = session.profile_catalog(FeatureId(1)).profiles;
        assert_eq!(before.len(), 2);

        edges.reverse();
        for (i, edge) in edges.iter_mut().enumerate() {
            edge.id = (1 << 40) + 10 + i as u64;
        }
        session.set_projected_edges(edges.clone());
        let after = session.profile_catalog(FeatureId(1)).profiles;
        for original in &before {
            assert_eq!(
                after
                    .iter()
                    .find(|p| (p.area - original.area).abs() < 1e-8)
                    .unwrap()
                    .index,
                original.index,
                "renumbering discovery ids must not break the selected region"
            );
        }

        for edge in &mut edges {
            edge.edge_id = EdgeId(edge.edge_id.0 + 100);
        }
        session.set_projected_edges(edges);
        assert!(session
            .profile_catalog(FeatureId(1))
            .profiles
            .iter()
            .all(|p| before.iter().all(|old| p.index != old.index)));

        let sketch = session.dto();
        let mut registry = ProfileIdentities::default();
        let mut profiles = session.profile_catalog(FeatureId(1)).profiles;
        registry.assign(&sketch, &mut profiles);
        let mut loaded: ProfileIdentities =
            serde_json::from_str(&serde_json::to_string(&registry).unwrap()).unwrap();
        loaded.validate().unwrap();
        let mut reopened = profiles.clone();
        loaded.assign(&sketch, &mut reopened);
        assert_eq!(reopened, profiles);
    }
}
