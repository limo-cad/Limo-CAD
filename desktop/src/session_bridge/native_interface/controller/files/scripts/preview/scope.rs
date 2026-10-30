//! Contextual teaching belongs to the active command's own interaction scope.
//! A transient menu may host Book entries; a dialog never admits background tools.
pub(super) fn owner<'a>(
    files: Option<&'a str>,
    workbench: Option<&'a str>,
    detached_workbench_menu: bool,
    history: Option<&'a str>,
    sketch: Option<&'a str>,
    support: Option<&'a str>,
) -> Option<&'a str> {
    files
        .or(workbench)
        .or(detached_workbench_menu.then_some("workbench-menu"))
        .or(history)
        .or(sketch)
        .or(support)
}

pub(super) fn allows(active: Option<&str>, anchor: Option<&str>) -> bool {
    match active {
        None => anchor.is_none(),
        Some(menu @ ("sketch-menu" | "workbench-menu")) => anchor == Some(menu),
        Some(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use limo_cad_interface::{
        Control, ControlInput, ControlKey, DocumentContext, Field, KeyChord, Rect, Surface,
        SurfaceFrame, SurfaceRegistry,
    };

    #[test]
    fn detached_workbench_menu_keeps_controller_modal_precedence() {
        assert_eq!(
            owner(None, None, true, None, None, None),
            Some("workbench-menu")
        );
        assert_eq!(
            owner(Some("file-dialog"), None, true, None, None, None),
            Some("file-dialog")
        );
        assert_eq!(
            owner(None, Some("cam-library"), true, None, None, None),
            Some("cam-library")
        );
        assert_eq!(
            owner(
                None,
                None,
                true,
                Some("history-menu"),
                Some("sketch-menu"),
                None
            ),
            Some("workbench-menu")
        );
        assert_eq!(
            owner(
                None,
                None,
                false,
                None,
                Some("sketch-menu"),
                Some("sketch-origin")
            ),
            Some("sketch-menu")
        );
        assert_eq!(owner(None, None, false, None, None, None), None);
        assert!(!allows(
            owner(Some("app-settings"), None, true, None, None, None),
            Some("workbench-menu")
        ));
    }

    fn button(key: u64, scope: Option<&str>) -> Control {
        Control {
            key: ControlKey(key),
            binding: 1,
            surface: "guidance".into(),
            label: format!("Entry {key}"),
            role: "button".into(),
            bounds: Rect {
                x: key as f64 * 80.,
                y: 40.,
                width: 72.,
                height: 28.,
            },
            visible: true,
            disabled: false,
            expanded: None,
            selected: None,
            field: Field::None,
            modal_scope: scope.map(str::to_owned),
            text_editing: false,
            owned_keys: vec![],
        }
    }

    #[test]
    fn scoped_book_entries_use_the_real_registry_and_do_not_escape_an_unrelated_dialog() {
        for (menu, other) in [
            ("sketch-menu", "workbench-menu"),
            ("workbench-menu", "sketch-menu"),
        ] {
            let owner = DocumentContext {
                window_id: "main".into(),
                document_id: "guide".into(),
                epoch: 1,
            };
            let mut frame = SurfaceFrame {
                client: Rect {
                    width: 1200.,
                    height: 800.,
                    ..Rect::default()
                },
                surfaces: [other, menu, "file-dialog"]
                    .map(|name| Surface {
                        name: name.into(),
                        text: None,
                    })
                    .into(),
                controls: vec![
                    button(1, Some(menu)),
                    button(2, Some(menu)),
                    button(3, None),
                    button(4, Some(other)),
                    button(5, Some("file-dialog")),
                ],
                // The dialog is not active yet; do not register an unrendered modal owner.
                modal_stack: vec![other.into(), "file-dialog".into(), menu.into()],
                focused: Some(ControlKey(1)),
                document_visible: true,
                ..SurfaceFrame::default()
            };
            let mut registry = SurfaceRegistry::new();
            registry.replace(owner.clone(), frame.clone()).unwrap();
            assert!(allows(Some(menu), Some(menu)));
            assert_eq!(
                registry.focus_target(false),
                Some(ControlKey(2)),
                "Tab enters the scoped Book entry"
            );
            let book = registry
                .resolve_key(ControlKey(2), ControlInput::Click, &owner)
                .unwrap();
            assert!(registry
                .resolve_key(
                    ControlKey(2),
                    ControlInput::Key(KeyChord::plain("Enter")),
                    &owner
                )
                .is_ok());
            for blocked in [3, 4, 5] {
                assert!(registry
                    .resolve_key(ControlKey(blocked), ControlInput::Click, &owner)
                    .is_err());
            }
            frame.modal_stack = vec![other.into(), menu.into(), "file-dialog".into()];
            frame.focused = Some(ControlKey(5));
            registry.replace(owner.clone(), frame.clone()).unwrap();
            assert!(!allows(Some("file-dialog"), Some(menu)));
            assert!(
                registry.validate_resolved(&book, &owner).is_err(),
                "An old Book receipt cannot cross the dialog fence"
            );
            assert!(registry
                .resolve_key(
                    ControlKey(2),
                    ControlInput::Key(KeyChord::plain("Enter")),
                    &owner
                )
                .is_err());
            assert_eq!(registry.focus_target(false), Some(ControlKey(5)));
            frame.modal_stack = vec!["file-dialog".into(), menu.into(), other.into()];
            registry.replace(owner.clone(), frame).unwrap();
            assert!(!allows(Some(other), Some(menu)));
            assert!(registry
                .resolve_key(ControlKey(2), ControlInput::Click, &owner)
                .is_err());
        }
    }

    #[test]
    fn only_the_active_menu_owner_can_offer_guidance() {
        let scopes = [
            None,
            Some("sketch-menu"),
            Some("workbench-menu"),
            Some("file-menu"),
        ];
        for active in scopes {
            for anchor in scopes {
                let expected = active == anchor && active != Some("file-menu");
                assert_eq!(allows(active, anchor), expected, "{active:?} / {anchor:?}");
            }
        }
        for dialog in [
            "file-dialog",
            "file-picker",
            "app-settings",
            "close-document",
            "sketch-origin",
            "delete-feature",
            "cam-library",
            "unknown-modal",
        ] {
            for anchor in scopes.into_iter().chain([Some(dialog)]) {
                assert!(!allows(Some(dialog), anchor), "{dialog} / {anchor:?}");
            }
        }
    }
}
