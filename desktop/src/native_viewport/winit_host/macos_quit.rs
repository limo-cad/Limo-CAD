//! AppKit Quit must enter the same guarded close flow as the window button.
//! Keep Winit's delegate and all of its lifecycle handlers; add only the
//! optional termination decision through a layout-identical subclass.

use super::{route_window_input, NativeInterfaceHandle};
use bevy::{
    prelude::*,
    window::{PrimaryWindow, WindowCloseRequested, WindowEvent},
};
use objc2::{
    runtime::{AnyClass, AnyObject, ClassBuilder, Sel},
    sel, MainThreadMarker,
};
use objc2_app_kit::{NSApplication, NSApplicationTerminateReply};
use std::{
    cell::RefCell,
    ffi::CString,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[derive(Clone, Default, Resource)]
struct QuitRequested(Arc<AtomicBool>);

struct Delivery {
    requested: QuitRequested,
    wake: NativeInterfaceHandle,
}

thread_local! {
    static DELIVERY: RefCell<Option<Delivery>> = const { RefCell::new(None) };
}

extern "C-unwind" fn should_terminate(
    _delegate: &AnyObject,
    _selector: Sel,
    _application: &AnyObject,
) -> NSApplicationTerminateReply {
    DELIVERY.with_borrow(|delivery| {
        if let Some(delivery) = delivery {
            delivery.requested.0.store(true, Ordering::Release);
            delivery.wake.request_redraw();
        }
    });
    // Returning Later runs AppKit's nested modal loop. Return to Winit's normal
    // event loop instead: only the existing Save/Discard continuation may emit
    // AppExit, after its UI receipt. Cancel keeps the document and window alive.
    NSApplicationTerminateReply::TerminateCancel
}

fn guard_delegate(delegate: &AnyObject) {
    const PREFIX: &str = "LimoCadGuardedQuit_";
    let original = delegate.class();
    if original.name().to_bytes().starts_with(PREFIX.as_bytes()) {
        return;
    }
    // Installation is on AppKit's main thread. This does not replace or inspect
    // any Winit ivars, and inherited methods retain Winit's original behavior.
    let name = CString::new(format!("{PREFIX}{}", original.name().to_string_lossy())).unwrap();
    let guarded = AnyClass::get(&name).unwrap_or_else(|| {
        let mut builder = ClassBuilder::new(&name, original).expect("native Quit delegate class");
        // SAFETY: This optional NSApplicationDelegate selector takes one object
        // and returns the NSUInteger-backed NSApplicationTerminateReply.
        unsafe {
            builder.add_method(
                sel!(applicationShouldTerminate:),
                should_terminate as extern "C-unwind" fn(_, _, _) -> _,
            );
        }
        builder.register()
    });
    assert_eq!(guarded.superclass(), Some(original));
    assert_eq!(guarded.instance_size(), original.instance_size());
    // SAFETY: This is a direct subclass with no new ivars, allocation/deallocation
    // overrides or changed lifecycle methods. AppKit owns this main-thread-only
    // object and no other code can change its class during installation.
    let previous = unsafe { AnyObject::set_class(delegate, guarded) };
    assert_eq!(previous, original);
}

pub(super) fn install(app: &mut App) {
    let main = MainThreadMarker::new().expect("native Quit installs on the AppKit thread");
    let application = NSApplication::sharedApplication(main);
    let delegate = application.delegate().expect("Winit application delegate");
    let requested = QuitRequested::default();
    let wake = app.world().resource::<NativeInterfaceHandle>().clone();
    DELIVERY.with_borrow_mut(|delivery| {
        *delivery = Some(Delivery {
            requested: requested.clone(),
            wake,
        })
    });
    guard_delegate((*delegate).as_ref());
    // Refresh AppKit's optional-selector cache with the same retained delegate.
    application.setDelegate(None);
    application.setDelegate(Some(&delegate));
    app.insert_resource(requested)
        .add_systems(PreUpdate, deliver.before(route_window_input));
}

fn deliver(world: &mut World) {
    let Ok(window) = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
    else {
        return;
    };
    if !world
        .resource::<QuitRequested>()
        .0
        .swap(false, Ordering::AcqRel)
    {
        return;
    }
    let event = WindowCloseRequested { window };
    world.write_message(event.clone());
    world.write_message(WindowEvent::WindowCloseRequested(event));
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::{msg_send, runtime::NSObject, ClassType};

    extern "C-unwind" fn inherited_handler(_delegate: &AnyObject, _selector: Sel) -> usize {
        42
    }

    #[test]
    fn appkit_quit_cancels_immediate_exit_and_delivers_one_owned_window_close() {
        let mut builder =
            ClassBuilder::new(c"LimoCadQuitDelegateFixture", NSObject::class()).unwrap();
        unsafe {
            builder.add_method(
                sel!(limoExistingLifecycleHandler),
                inherited_handler as extern "C-unwind" fn(_, _) -> _,
            );
        }
        let original = builder.register();
        let delegate = NSObject::new();
        // The synthetic delegate has no ivars and only adds an inherited method.
        unsafe {
            AnyObject::set_class(&delegate, original);
        }
        guard_delegate(&delegate);
        guard_delegate(&delegate);
        let inherited: usize = unsafe { msg_send![&*delegate, limoExistingLifecycleHandler] };
        assert_eq!(
            inherited, 42,
            "Winit lifecycle handlers must remain inherited"
        );

        let requested = QuitRequested::default();
        let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = wakes.clone();
        DELIVERY.with_borrow_mut(|delivery| {
            *delivery = Some(Delivery {
                requested: requested.clone(),
                wake: NativeInterfaceHandle::new(move || {
                    count.fetch_add(1, Ordering::Relaxed);
                }),
            })
        });
        for _ in 0..2 {
            // The sender is opaque to the callback; use the synthetic object.
            let reply: NSApplicationTerminateReply =
                unsafe { msg_send![&*delegate, applicationShouldTerminate: &*delegate] };
            assert_eq!(reply, NSApplicationTerminateReply::TerminateCancel);
        }
        assert_eq!(wakes.load(Ordering::Relaxed), 2);
        let mut app = App::new();
        app.add_message::<WindowCloseRequested>()
            .add_message::<WindowEvent>()
            .insert_resource(requested.clone());
        deliver(app.world_mut());
        assert!(
            requested.0.load(Ordering::Acquire),
            "Quit waits for the owned window"
        );
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        deliver(app.world_mut());
        deliver(app.world_mut());
        assert_eq!(
            app.world()
                .resource::<Messages<WindowCloseRequested>>()
                .len(),
            1
        );
        let events = app.world().resource::<Messages<WindowEvent>>();
        let mut cursor = events.get_cursor();
        let events: Vec<_> = cursor.read(events).collect();
        assert!(
            matches!(events.as_slice(), [WindowEvent::WindowCloseRequested(event)] if event.window == window)
        );
        DELIVERY.with_borrow_mut(|delivery| *delivery = None);
    }
}
