//! Receive GetURL Apple events without replacing Winit's application delegate.
//! The bundle's CFBundleURLTypes owns registration. Callbacks only queue the
//! direct-object URL for the existing document open path.

use crate::native_viewport::interface_shell::NativeInterfaceHandle;
use bevy::prelude::*;
use objc2::{
    define_class, msg_send, rc::Retained, sel, DefinedClass, MainThreadMarker, MainThreadOnly,
};
use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager, NSObject, NSObjectProtocol};
use std::{cell::RefCell, ptr::NonNull};

use super::{enqueue, GetUrlPayload, Pending, Queued, GET_URL};

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Delivery]
    struct Receiver;
    unsafe impl NSObjectProtocol for Receiver {}
    impl Receiver {
        #[unsafe(method(receiveRecipe:withReply:))]
        fn receive(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
            let payload = match payload_from_event(event) {
                Ok(payload) => payload,
                Err(error) => {
                    eprintln!("Recipe link rejected: {error}");
                    return;
                }
            };
            match enqueue(&self.ivars().pending, &payload) {
                Ok(Queued::Fresh) => self.ivars().wake.request_redraw(),
                Ok(Queued::Duplicate) => {}
                Err(error) => eprintln!("Recipe link rejected: {error}"),
            }
        }
    }
);

struct Delivery {
    pending: Pending,
    wake: NativeInterfaceHandle,
}

thread_local! {
    static RECEIVER: RefCell<Option<Retained<Receiver>>> = const { RefCell::new(None) };
}

pub(super) fn install(app: &mut App) {
    let mtm = MainThreadMarker::new().expect("Native recipe URLs install on the AppKit thread");
    let pending = Pending::default();
    let wake = app.world().resource::<NativeInterfaceHandle>().clone();
    let allocated = Receiver::alloc(mtm).set_ivars(Delivery {
        pending: pending.clone(),
        wake,
    });
    let receiver: Retained<Receiver> = unsafe { msg_send![super(allocated), init] };
    unsafe {
        NSAppleEventManager::sharedAppleEventManager()
            .setEventHandler_andSelector_forEventClass_andEventID(
                &receiver,
                sel!(receiveRecipe:withReply:),
                u32::from_be_bytes(GET_URL),
                u32::from_be_bytes(GET_URL),
            );
    }
    RECEIVER.with(|slot| *slot.borrow_mut() = Some(receiver));
    app.insert_resource(pending)
        .add_systems(Update, super::deliver);
}

fn payload_from_event(event: &NSAppleEventDescriptor) -> Result<GetUrlPayload, String> {
    let Some(direct) = event.paramDescriptorForKeyword(u32::from_be_bytes(*b"----")) else {
        return Err("GetURL is missing its direct object".into());
    };
    let mut payload = GetUrlPayload {
        event_class: event.eventClass().to_be_bytes(),
        event_id: event.eventID().to_be_bytes(),
        keyword: *b"----",
        descriptor_type: direct.descriptorType().to_be_bytes(),
        data: descriptor_bytes(&direct),
    };
    if super::url_from_get_url(&payload).is_err() {
        if let Some(value) = direct.stringValue() {
            payload.descriptor_type = *b"utf8";
            payload.data = value.to_string().into_bytes();
        }
    }
    Ok(payload)
}

fn descriptor_bytes(descriptor: &NSAppleEventDescriptor) -> Vec<u8> {
    let data = descriptor.data();
    let length = data.length();
    if length == 0 {
        return Vec::new();
    }
    let mut bytes = vec![0u8; length];
    unsafe {
        data.getBytes_length(
            NonNull::new(bytes.as_mut_ptr().cast()).expect("descriptor buffer"),
            length,
        );
    }
    bytes
}
