use super::*;
use crate::six_dof_mouse::MotionPacket;
use std::{
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

type ServiceFixture = (
    Arc<Service>,
    Receiver<SixDofEventSink>,
    Sender<Result<SixDofMouseInfo, String>>,
    Receiver<()>,
    Receiver<()>,
);

struct Fake {
    opened: Sender<SixDofEventSink>,
    release: Receiver<Result<SixDofMouseInfo, String>>,
    closed: Sender<()>,
}
impl Backend for Fake {
    fn connect(&mut self, sink: SixDofEventSink) -> Result<SixDofMouseInfo, String> {
        self.opened.send(sink).unwrap();
        self.release
            .recv_timeout(Duration::from_secs(3))
            .unwrap_or_else(|_| Err("test gate timed out".into()))
    }
    fn disconnect(&mut self) -> Result<(), String> {
        let _ = self.closed.send(());
        Ok(())
    }
}
fn info() -> SixDofMouseInfo {
    SixDofMouseInfo {
        vendor_id: 1,
        product_id: 2,
        product_name: "Fake device".into(),
        serial_number: None,
    }
}
fn fixture() -> ServiceFixture {
    let (opened, opens) = mpsc::channel();
    let (release, releases) = mpsc::channel();
    let (closed, closes) = mpsc::channel();
    let (wake, wakes) = mpsc::channel();
    let service = Service::new(Fake {
        opened,
        release: releases,
        closed,
    })
    .unwrap();
    service.register(
        "one".into(),
        Arc::new(move || {
            let _ = wake.send(());
        }),
    );
    (service, opens, release, closes, wakes)
}
fn wait(service: &Service, wakes: &Receiver<()>, state: &str) -> Status {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let status = service.status();
        if status.state == state {
            return status;
        }
        wakes
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
    }
}
fn owner(window: &str, epoch: u64) -> DocumentContext {
    DocumentContext {
        window_id: window.into(),
        document_id: "design".into(),
        epoch,
    }
}
fn motion(sink: &SixDofEventSink) {
    sink(SixDofEvent::Motion(MotionPacket {
        translation: Some([350, 0, 0]),
        rotation: None,
    }));
}

#[test]
fn connection_is_explicit_and_motion_is_fenced_by_window_document_and_generation() {
    let (service, opens, release, _closes, wakes) = fixture();
    assert!(
        opens.try_recv().is_err(),
        "registering a window must not open hardware"
    );
    assert_eq!(service.status().state, "disconnected");
    service.request_at(0, true).unwrap();
    let sink = opens.recv_timeout(Duration::from_secs(3)).unwrap();
    motion(&sink);
    release.send(Ok(info())).unwrap();
    let connected = wait(&service, &wakes, "connected");
    let first = owner("one", 1);
    assert!(!service
        .sample("one", Some(&first), Instant::now())
        .0
        .active());
    for _ in 0..10_000 {
        motion(&sink);
    }
    assert_eq!(
        service
            .sample("one", Some(&first), Instant::now())
            .0
            .translation,
        [1., 0., 0.]
    );
    let changed = owner("one", 2);
    assert!(!service
        .sample("one", Some(&changed), Instant::now())
        .0
        .active());
    motion(&sink);
    service.sample("one", None, Instant::now());
    motion(&sink);
    assert!(!service
        .sample("one", Some(&changed), Instant::now())
        .0
        .active());
    service.register("two".into(), Arc::new(|| {}));
    let second = owner("two", 1);
    service.sample("two", Some(&second), Instant::now());
    motion(&sink);
    service.sample("one", None, Instant::now());
    assert!(service
        .sample("two", Some(&second), Instant::now())
        .0
        .active());
    assert!(
        service.request_at(0, false).is_err(),
        "stale retained Connect cannot disconnect a later generation"
    );
    service.request_at(connected.generation, false).unwrap();
    motion(&sink);
    assert!(!service
        .sample("two", Some(&second), Instant::now())
        .0
        .active());
    wait(&service, &wakes, "disconnected");
}

#[test]
fn last_window_invalidates_inflight_connect_before_a_new_window_can_reconnect() {
    let (service, opens, release, closes, _wakes) = fixture();
    service.request_at(0, true).unwrap();
    let old_sink = opens.recv_timeout(Duration::from_secs(3)).unwrap();
    service.unregister("one");
    let status = service.status();
    assert_eq!((status.generation, status.state), (2, "disconnecting"));
    let (wake, wakes) = mpsc::channel();
    service.register(
        "two".into(),
        Arc::new(move || {
            let _ = wake.send(());
        }),
    );
    assert!(service.request_at(status.generation, true).is_err());
    release.send(Ok(info())).unwrap();
    closes.recv_timeout(Duration::from_secs(3)).unwrap();
    let closed = wait(&service, &wakes, "disconnected");
    service.request_at(closed.generation, true).unwrap();
    let new_sink = opens.recv_timeout(Duration::from_secs(3)).unwrap();
    release.send(Ok(info())).unwrap();
    wait(&service, &wakes, "connected");
    let second = owner("two", 1);
    service.sample("two", Some(&second), Instant::now());
    motion(&old_sink);
    assert!(!service
        .sample("two", Some(&second), Instant::now())
        .0
        .active());
    motion(&new_sink);
    assert!(service
        .sample("two", Some(&second), Instant::now())
        .0
        .active());
}

#[test]
fn synchronous_reader_failure_is_not_overwritten_by_connect_success() {
    let (service, opens, release, closes, wakes) = fixture();
    service.request_at(0, true).unwrap();
    let sink = opens.recv_timeout(Duration::from_secs(3)).unwrap();
    sink(SixDofEvent::Error("unplugged".into()));
    release.send(Ok(info())).unwrap();
    closes.recv_timeout(Duration::from_secs(3)).unwrap();
    let status = wait(&service, &wakes, "error");
    assert!(status.message.contains("unplugged"));
    service.request_at(status.generation, true).unwrap();
    let _next = opens.recv_timeout(Duration::from_secs(3)).unwrap();
    release.send(Err("permission denied".into())).unwrap();
    assert!(wait(&service, &wakes, "error")
        .message
        .contains("permission denied"));
}

#[test]
fn dropping_last_owner_never_waits_for_hardware_and_late_success_is_closed() {
    let (service, opens, release, closes, _wakes) = fixture();
    service.request_at(0, true).unwrap();
    let sink = opens.recv_timeout(Duration::from_secs(3)).unwrap();
    let started = Instant::now();
    drop(service);
    assert!(started.elapsed() < Duration::from_millis(250));
    motion(&sink);
    release.send(Ok(info())).unwrap();
    closes.recv_timeout(Duration::from_secs(3)).unwrap();
}
