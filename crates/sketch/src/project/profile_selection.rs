//! Preserve the evaluated regions of features saved before schema 15.
use super::ProjectModelV9;
use crate::session::SketchSession;
use limo_cad_core::FeatureId;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn migrate(
    model: &mut ProjectModelV9,
    direct_regions: &BTreeSet<FeatureId>,
) -> Result<(), String> {
    // Include suppressed sketches and sketches beyond the rollback cursor:
    // their consumers must also retain their meaning when enabled later.
    let mut aliases = BTreeMap::new();
    for saved in &model.sketches {
        let session =
            SketchSession::from_project_state(saved.clone()).map_err(|error| error.to_string())?;
        let catalog = session.profile_catalog(saved.feature_id);
        let parents = catalog
            .profiles
            .iter()
            .filter(|profile| profile.nesting_depth % 2 == 1)
            .filter_map(|profile| profile.parent_index.map(|parent| (profile.index, parent)))
            .collect::<BTreeMap<_, _>>();
        aliases.insert(saved.name.clone(), parents);
    }
    let resolve = |sketch: &str, index: u32| {
        aliases
            .get(sketch)
            .and_then(|parents| parents.get(&index))
            .copied()
            .unwrap_or(index)
    };
    let migrate_indices = |sketch: &str, indices: &mut Vec<u32>| {
        let mut seen = BTreeSet::new();
        indices.retain_mut(|index| {
            *index = resolve(sketch, *index);
            seen.insert(*index)
        });
    };
    // Old replay mapped a hole to its parent, then deduplicated in selection
    // order. Keep that order and every reserved body ID so downstream targets
    // keep the same identity. Missing references still fail normal replay.
    for definition in &mut model.extrudes {
        if definition.source_face.is_none() && !direct_regions.contains(&definition.feature_id) {
            migrate_indices(&definition.sketch_name, &mut definition.profile_indices);
        }
    }
    for definition in &mut model.revolves {
        if !direct_regions.contains(&definition.feature_id) {
            migrate_indices(&definition.sketch_name, &mut definition.profile_indices);
        }
    }
    for definition in &mut model.sweeps {
        let profile = &mut definition.profile;
        profile.profile_index = resolve(&profile.sketch_name, profile.profile_index);
    }
    for definition in &mut model.lofts {
        for profile in &mut definition.sections {
            profile.profile_index = resolve(&profile.sketch_name, profile.profile_index);
        }
    }
    Ok(())
}
