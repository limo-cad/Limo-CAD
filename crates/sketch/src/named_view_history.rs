use crate::NamedViewConfigurationDto;

/// Preserve assigned saved-layout identities during authenticated same-document
/// history restoration. Ordinary project Open/load must not call this helper.
/// It changes only IDs, never poses, visibility, camera, names or collection size.
pub fn normalize_named_view_history_ids(
    current: &[NamedViewConfigurationDto],
    historical: &mut [NamedViewConfigurationDto],
) -> Result<(), String> {
    crate::dto::validate_named_views(current)?;
    crate::dto::validate_named_views(historical)?;
    let mut normalized = historical.to_vec();
    for target in &mut normalized {
        let by_name = current.iter().find(|view| view.name == target.name);
        let matched = by_name.or_else(|| {
            let equal_content = |view: &&NamedViewConfigurationDto| {
                let mut left = (*view).clone();
                let mut right = target.clone();
                left.id = None;
                right.id = None;
                left.name.clear();
                right.name.clear();
                left == right
            };
            let mut matches = current.iter().filter(equal_content);
            let first = matches.next()?;
            matches.next().is_none().then_some(first)
        });
        if let Some(assigned) = matched.and_then(|view| view.id.as_ref()) {
            if target.id.as_ref().is_some_and(|id| id != assigned) {
                return Err("Saved-layout history contains a different assigned identity".into());
            }
            target.id = Some(assigned.clone());
        }
    }
    crate::dto::validate_named_views(&normalized)?;
    historical.clone_from_slice(&normalized);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn legacy(name: &str) -> NamedViewConfigurationDto {
        NamedViewConfigurationDto {
            id: None,
            name: name.into(),
            camera: crate::ViewCameraDto {
                position: [10., -10., 10.],
                target: [0.; 3],
                up: [0., 0., 1.],
            },
            visible_body_ids: vec![1],
            part_offsets: Vec::new(),
            occurrence_offsets: Vec::new(),
            print_layout: true,
            print_bed: Default::default(),
        }
    }
    #[test]
    fn history_preserves_owned_ids_through_legacy_update_and_first_rename() {
        let mut current = legacy("Horizontal");
        current.id = Some("a851d1cd-70be-4524-b907-ae62d18e9f83".into());
        let mut historical = vec![legacy("Initial")];
        normalize_named_view_history_ids(&[current.clone()], &mut historical).unwrap();
        assert_eq!(historical[0].id, current.id);
        assert_eq!(historical[0].name, "Initial");
        let mut historical = vec![legacy("Horizontal")];
        historical[0].camera.position[0] = 20.;
        normalize_named_view_history_ids(&[current.clone()], &mut historical).unwrap();
        assert_eq!(historical[0].id, current.id);
        assert_eq!(historical[0].camera.position[0], 20.);
        let mut absent = Vec::new();
        normalize_named_view_history_ids(&[current], &mut absent).unwrap();
        assert!(absent.is_empty(), "Undo first-created view removes it");
    }
    #[test]
    fn history_rejects_identity_conflict_atomically_and_does_not_guess_rename() {
        let mut current = legacy("Horizontal");
        current.id = Some("a851d1cd-70be-4524-b907-ae62d18e9f83".into());
        let mut conflict = vec![current.clone()];
        conflict[0].id = Some("b851d1cd-70be-4524-b907-ae62d18e9f83".into());
        let before = conflict.clone();
        assert!(normalize_named_view_history_ids(&[current.clone()], &mut conflict).is_err());
        assert_eq!(conflict, before);
        let mut second = current.clone();
        second.id = Some("b851d1cd-70be-4524-b907-ae62d18e9f83".into());
        second.name = "Vertical".into();
        let mut ambiguous = vec![legacy("Initial")];
        normalize_named_view_history_ids(&[current, second], &mut ambiguous).unwrap();
        assert_eq!(ambiguous[0].id, None);
    }
}
