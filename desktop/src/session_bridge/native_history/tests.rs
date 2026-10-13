use super::*;

#[test]
fn visibility_revisions_preserve_edit_models_and_redo_but_expire_prepared_tickets() {
    let mut history = SolidHistory::default();
    history
        .record_edit(&state(1, 1), "before edit".into(), state(1, 2))
        .unwrap();
    let stale = history.peek_edit_undo(&state(1, 2)).unwrap();
    let original = history.edits[0].clone();
    history
        .advance_visibility(&state(1, 2), state(1, 3))
        .unwrap();
    assert!(Arc::ptr_eq(&original, &history.edits[0]));
    assert!(history.preserves_visibility());
    assert!(history
        .commit_edit_undo(stale, "after edit".into(), state(2, 1))
        .is_err());
    let current = history.peek_edit_undo(&state(1, 3)).unwrap();
    history
        .commit_edit_undo(current, "after edit".into(), state(2, 1))
        .unwrap();
    let stale = history.peek_redo(&state(2, 1)).unwrap();
    let original = history.redo[0].model.clone();
    history
        .advance_visibility(&state(2, 1), state(2, 2))
        .unwrap();
    assert!(Arc::ptr_eq(&original, &history.redo[0].model));
    assert!(history.can_redo(&state(2, 2)));
    assert!(history.commit_redo(stale, state(3, 1)).is_err());
    assert!(history
        .advance_visibility(&state(2, 2), state(3, 3))
        .is_err());
    assert!(history.can_redo(&state(2, 2)));
    history.observe(&state(2, 3)).unwrap();
    assert!(!history.can_redo(&state(2, 3)));
    assert!(!history.preserves_visibility());
}

#[test]
fn edit_snapshots_are_bounded_and_failed_or_stale_loads_do_not_pop_them() {
    let mut history = SolidHistory::default();
    for revision in 1..=40 {
        history
            .record_edit(
                &state(1, revision),
                format!("model-{revision}"),
                state(1, revision + 1),
            )
            .unwrap();
    }
    assert_eq!(history.edits.len(), SOLID_REDO_LIMIT);
    let ticket = history.peek_edit_undo(&state(1, 41)).unwrap();
    assert!(history
        .commit_edit_undo(ticket.clone(), String::new(), state(2, 1))
        .is_err());
    assert_eq!(
        history.peek_edit_undo(&state(1, 41)).unwrap().model_json(),
        "model-40"
    );
    history
        .commit_edit_undo(ticket.clone(), "current".into(), state(2, 1))
        .unwrap();
    assert!(history
        .commit_edit_undo(ticket, "current".into(), state(3, 1))
        .is_err());
    let redo = history.peek_redo(&state(2, 1)).unwrap();
    history.commit_redo(redo, state(3, 1)).unwrap();
    assert_eq!(
        history.peek_edit_undo(&state(3, 1)).unwrap().model_json(),
        "model-40"
    );
    history.observe(&state(3, 2)).unwrap();
    assert!(history.peek_edit_undo(&state(3, 2)).is_none());
}

fn state(epoch: u64, engine_revision: u64) -> HistoryState {
    HistoryState {
        context: DocumentContext {
            window_id: "main".into(),
            document_id: "project-A".into(),
            epoch,
        },
        engine_revision,
    }
}

fn undo(history: &mut SolidHistory, before: &HistoryState, after: HistoryState, model: &str) {
    let ticket = history.prepare_undo(before, model.into()).unwrap();
    history.commit_undo(ticket, after).unwrap();
}

#[test]
fn latest_marker_undo_deletes_a_feature_while_earlier_markers_move_one_step() {
    assert_eq!(undo_step(0, 0).unwrap(), None);
    assert_eq!(undo_step(0, 3).unwrap(), None);
    assert_eq!(undo_step(3, 3).unwrap(), Some(UndoStep::DeleteLatest));
    assert_eq!(undo_step(2, 3).unwrap(), Some(UndoStep::Rollback(1)));
    assert!(undo_step(4, 3).is_err());
    let history = SolidHistory::default();
    assert!(matches!(
        history.redo_step(&state(1, 1), 2, 3).unwrap(),
        Some(RedoStep::Rollback(3))
    ));
    assert!(history.redo_step(&state(1, 1), 3, 3).unwrap().is_none());
    assert!(history.redo_step(&state(1, 1), 4, 3).is_err());
}

#[test]
fn consecutive_redos_restore_their_models_and_survive_deliberate_owner_retirement() {
    let mut history = SolidHistory::default();
    undo(
        &mut history,
        &state(1, 1),
        state(1, 2),
        "full model with two features",
    );
    undo(
        &mut history,
        &state(1, 2),
        state(1, 3),
        "full model with one feature",
    );
    let Some(RedoStep::Restore(first)) = history.redo_step(&state(1, 3), 0, 0).unwrap() else {
        panic!("expected restore");
    };
    assert_eq!(first.model_json(), "full model with one feature");
    let mut replacement_history = history.clone();
    replacement_history.commit_redo(first, state(2, 1)).unwrap();
    assert!(!replacement_history.can_redo(&state(1, 3)));
    let second = replacement_history.peek_redo(&state(2, 1)).unwrap();
    assert_eq!(second.model_json(), "full model with two features");
    replacement_history
        .commit_redo(second, state(3, 1))
        .unwrap();
    assert!(!replacement_history.can_redo(&state(3, 1)));
}

#[test]
fn branch_changes_and_ordinary_open_expire_redo_without_crossing_project_ownership() {
    let mut history = SolidHistory::default();
    undo(&mut history, &state(1, 1), state(1, 2), "original");
    assert!(history.can_redo(&state(1, 2)));
    history.observe(&state(1, 2)).unwrap();
    assert!(
        history.can_redo(&state(1, 2)),
        "unchanged refresh preserves the branch"
    );
    assert!(
        !history.can_redo(&state(1, 3)),
        "a changed model cannot use an unobserved stale stack"
    );
    history.observe(&state(1, 3)).unwrap();
    assert!(history.redo.is_empty());
    undo(&mut history, &state(1, 3), state(1, 4), "new branch");
    let mut another = state(1, 4);
    another.context.document_id = "project-B".into();
    assert!(!history.can_redo(&another));
    history.observe(&state(2, 1)).unwrap();
    assert!(
        history.redo.is_empty(),
        "ordinary Open does not inherit deleted-feature snapshots"
    );
}

#[test]
fn cancelled_failed_or_stale_operations_do_not_pop_or_corrupt_history() {
    let mut history = SolidHistory::default();
    undo(&mut history, &state(1, 1), state(1, 2), "original");
    let cancelled = history
        .prepare_undo(&state(1, 2), "pending model".into())
        .unwrap();
    drop(cancelled);
    assert_eq!(history.redo.len(), 1);
    let redo = history.peek_redo(&state(1, 2)).unwrap();
    drop(redo);
    assert_eq!(
        history.peek_redo(&state(1, 2)).unwrap().model_json(),
        "original"
    );
    let invalid = history.peek_redo(&state(1, 2)).unwrap();
    assert!(
        history.commit_redo(invalid, state(1, 3)).is_err(),
        "Redo must retire old incarnation"
    );
    let stale = history.peek_redo(&state(1, 2)).unwrap();
    let current = history.peek_redo(&state(1, 2)).unwrap();
    history.commit_redo(current, state(2, 1)).unwrap();
    assert!(history.commit_redo(stale, state(3, 1)).is_err());
    assert!(history.redo.is_empty());
    let alien = history
        .prepare_undo(&state(2, 1), "same-looking owner".into())
        .unwrap();
    assert!(SolidHistory::default()
        .commit_undo(alien, state(2, 2))
        .is_err());
}

#[test]
fn failed_undo_keeps_previous_branch_and_successful_undo_prunes_stale_branch() {
    let mut history = SolidHistory::default();
    undo(&mut history, &state(1, 1), state(1, 2), "original");
    let invalid = history
        .prepare_undo(&state(1, 2), "failed operation".into())
        .unwrap();
    assert!(history.commit_undo(invalid, state(2, 1)).is_err());
    assert_eq!(
        history.peek_redo(&state(1, 2)).unwrap().model_json(),
        "original"
    );
    undo(&mut history, &state(1, 3), state(1, 4), "branch model");
    assert_eq!(history.redo.len(), 1);
    assert_eq!(
        history.peek_redo(&state(1, 4)).unwrap().model_json(),
        "branch model"
    );
}

#[test]
fn snapshot_bound_and_shared_storage_survive_repeated_undo_redo() {
    let mut history = SolidHistory::default();
    for index in 0..35 {
        undo(
            &mut history,
            &state(1, index + 1),
            state(1, index + 2),
            &format!("model {index}"),
        );
    }
    assert_eq!(history.redo.len(), SOLID_REDO_LIMIT);
    let copy = history.clone();
    assert!(Arc::ptr_eq(
        &history.redo.last().unwrap().model,
        &copy.redo.last().unwrap().model
    ));
    let mut current = state(1, 36);
    for index in (3..35).rev() {
        let redo = history.peek_redo(&current).unwrap();
        assert_eq!(redo.model_json(), format!("model {index}"));
        current = state(current.context.epoch + 1, 1);
        history.commit_redo(redo, current.clone()).unwrap();
    }
    assert!(!history.can_redo(&current));
}
