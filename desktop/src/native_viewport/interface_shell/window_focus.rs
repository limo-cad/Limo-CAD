//! Window deactivation suspends focus; explicit UI blur still retires it.
use super::*;

pub(super) struct ReturnTarget {
    action: NativeInterfaceAction,
    modal_generation: u64,
}

impl NativeInterfaceHandle {
    pub(crate) fn suspend_window_focus(&self) {
        if let Ok(mut shared) = self.shared.lock() {
            if let Some(key) = shared.focused {
                shared.window_focus_return = current_context(&shared).ok().and_then(|context| {
                    let control = shared
                        .registry
                        .resolve_key(key, ControlInput::Click, &context)
                        .ok()?;
                    Some(ReturnTarget {
                        action: NativeInterfaceAction { context, control },
                        modal_generation: shared.modal_generation,
                    })
                });
            }
            // A duplicate keyboard-focus-loss notification must not overwrite
            // the original return target. Drags and hover never resume.
            shared.capture = None;
            let _ = set_focus(&mut shared, None);
            shared.hovered = None;
            shared.revision = shared.revision.wrapping_add(1);
        }
        (self.wake)();
    }

    pub(crate) fn resume_window_focus(&self) {
        if let Ok(mut shared) = self.shared.lock() {
            if let Some(target) = shared.window_focus_return.take() {
                // Never steal newly assigned focus, or revive a control from a
                // replaced document, modal, binding, or hidden/disabled widget.
                if shared.focused.is_none()
                    && shared.modal_generation == target.modal_generation
                    && current_context(&shared).ok().as_ref() == Some(&target.action.context)
                    && shared
                        .registry
                        .validate_resolved(&target.action.control, &target.action.context)
                        .is_ok()
                {
                    let _ = set_focus(&mut shared, Some(target.action.control.key));
                    shared.revision = shared.revision.wrapping_add(1);
                }
            }
        }
        (self.wake)();
    }
}
