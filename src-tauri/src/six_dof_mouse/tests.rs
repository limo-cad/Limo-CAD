use super::*;
use std::sync::mpsc;

fn synthetic(closed: Arc<AtomicBool>) -> (SixDofConnection, SixDofMouseInfo) {
    struct ReaderLifetime(Arc<AtomicBool>);
    impl Drop for ReaderLifetime {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let lifetime = ReaderLifetime(closed);
    let worker = raw::spawn(
        move |_, timeout| {
            let _keep_until_reader_exits = &lifetime;
            std::thread::park_timeout(Duration::from_millis(timeout as u64));
            Ok(0)
        },
        Arc::new(|_| panic!("idle synthetic reader emitted input")),
    )
    .unwrap();
    (
        SixDofConnection::RawHid(worker),
        SixDofMouseInfo {
            vendor_id: CURRENT_VENDOR_ID,
            product_id: 17,
            product_name: "Synthetic multi-axis device".into(),
            serial_number: Some("fixture-only".into()),
        },
    )
}

#[test]
fn replacement_and_disconnect_join_the_previous_reader_before_returning() {
    let state = SixDofMouseState::default();
    assert!(
        state.connection.lock().unwrap().is_none(),
        "construction must not open a device"
    );
    let first_closed = Arc::new(AtomicBool::new(false));
    let info = state
        .connect_with(|| Ok(synthetic(first_closed.clone())))
        .unwrap();
    assert_eq!(info.product_name, "Synthetic multi-axis device");
    assert_eq!(info.serial_number.as_deref(), Some("fixture-only"));
    let second_closed = Arc::new(AtomicBool::new(false));
    state
        .connect_with(|| {
            assert!(
                first_closed.load(Ordering::SeqCst),
                "old reader still alive during new discovery"
            );
            Ok(synthetic(second_closed.clone()))
        })
        .unwrap();
    state.disconnect().unwrap();
    assert!(second_closed.load(Ordering::SeqCst));
    assert!(state.connection.lock().unwrap().is_none());
    state.disconnect().unwrap();
}

#[test]
fn failed_replacement_leaves_no_reader_and_dropping_state_closes_its_device() {
    let state = SixDofMouseState::default();
    let closed = Arc::new(AtomicBool::new(false));
    state
        .connect_with(|| Ok(synthetic(closed.clone())))
        .unwrap();
    assert_eq!(
        state
            .connect_with(|| Err("No supported 3D mouse was found.".into()))
            .unwrap_err(),
        "No supported 3D mouse was found."
    );
    assert!(closed.load(Ordering::SeqCst));
    assert!(state.connection.lock().unwrap().is_none());
    let final_closed = Arc::new(AtomicBool::new(false));
    state
        .connect_with(|| Ok(synthetic(final_closed.clone())))
        .unwrap();
    drop(state);
    assert!(
        final_closed.load(Ordering::SeqCst),
        "dropping a window must not detach the reader"
    );
}

#[test]
fn a_disconnect_waits_for_an_in_flight_explicit_connection_then_closes_it() {
    let state = Arc::new(SixDofMouseState::default());
    let connector = state.clone();
    let (opening, entered) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let closed = Arc::new(AtomicBool::new(false));
    let reader_closed = closed.clone();
    let connect = std::thread::spawn(move || {
        connector.connect_with(|| {
            opening.send(()).unwrap();
            released.recv_timeout(Duration::from_secs(2)).unwrap();
            Ok(synthetic(reader_closed))
        })
    });
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    let disconnector = state.clone();
    let disconnect = std::thread::spawn(move || disconnector.disconnect());
    release.send(()).unwrap();
    connect.join().unwrap().unwrap();
    disconnect.join().unwrap().unwrap();
    assert!(closed.load(Ordering::SeqCst));
    assert!(state.connection.lock().unwrap().is_none());
}

#[test]
fn selects_only_the_supported_multi_axis_hid_interface() {
    assert!(supported_descriptor(
        CURRENT_VENDOR_ID,
        GENERIC_DESKTOP_USAGE_PAGE,
        MULTI_AXIS_CONTROLLER_USAGE,
        "SpaceMouse Wireless BT"
    ));
    assert!(!supported_descriptor(
        CURRENT_VENDOR_ID,
        GENERIC_DESKTOP_USAGE_PAGE,
        0x02,
        "3Dconnexion Virtual Mouse"
    ));
    assert!(!supported_descriptor(
        CURRENT_VENDOR_ID,
        0xff00,
        0x01,
        "3Dconnexion Virtual Data"
    ));
    assert!(supported_descriptor(
        LEGACY_VENDOR_ID,
        GENERIC_DESKTOP_USAGE_PAGE,
        MULTI_AXIS_CONTROLLER_USAGE,
        "Logitech SpaceNavigator"
    ));
    assert!(!supported_descriptor(
        LEGACY_VENDOR_ID,
        GENERIC_DESKTOP_USAGE_PAGE,
        MULTI_AXIS_CONTROLLER_USAGE,
        "Ordinary mouse"
    ));
    assert!(!supported_descriptor(
        0x1234,
        GENERIC_DESKTOP_USAGE_PAGE,
        MULTI_AXIS_CONTROLLER_USAGE,
        "SpaceMouse"
    ));
}
