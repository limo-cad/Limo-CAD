//! Coalesce CAD service wakes without depending on compositor redraw delivery.

use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Only explicit CAD service wakes are paced; native input/redraws are untouched.
const MIN_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Default)]
struct Pending {
    requested: bool,
    stopped: bool,
    last_sent: Option<Instant>,
}

impl Pending {
    fn delay(&self, now: Instant) -> Duration {
        self.last_sent
            .map(|last| MIN_INTERVAL.saturating_sub(now.saturating_duration_since(last)))
            .unwrap_or_default()
    }
}

struct Worker {
    shared: Arc<(Mutex<Pending>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        let (state, changed) = &*self.shared;
        // The worker never calls user code while holding this lock.
        state.lock().unwrap().stopped = true;
        changed.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Owns one sleeping worker. The final owner cancels a pending wake and joins it.
#[derive(Clone)]
pub(super) struct ServiceWake(Arc<Worker>);

impl ServiceWake {
    pub(super) fn new(send: impl Fn() -> bool + Send + 'static) -> std::io::Result<Self> {
        let shared = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
        let worker_shared = shared.clone();
        let thread = thread::Builder::new()
            .name("cad-service-wake".into())
            .spawn(move || {
                let (state, changed) = &*worker_shared;
                let mut pending = state.lock().unwrap();
                loop {
                    if pending.stopped {
                        break;
                    }
                    if !pending.requested {
                        pending = changed.wait(pending).unwrap();
                        continue;
                    }
                    let now = Instant::now();
                    let delay = pending.delay(now);
                    if !delay.is_zero() {
                        // Notifications and spurious wakes must not shorten the interval.
                        pending = changed.wait_timeout(pending, delay).unwrap().0;
                        continue;
                    }
                    pending.requested = false;
                    drop(pending);
                    let alive = send();
                    pending = state.lock().unwrap();
                    pending.last_sent = Some(Instant::now());
                    if !alive {
                        pending.stopped = true;
                    }
                }
            })?;
        Ok(Self(Arc::new(Worker {
            shared,
            thread: Some(thread),
        })))
    }

    pub(super) fn request(&self) {
        let (state, changed) = &*self.0.shared;
        let mut pending = state.lock().unwrap();
        if !pending.stopped && !pending.requested {
            pending.requested = true;
            changed.notify_one();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn service_wake_deadline_is_fixed_and_idle_requests_are_immediate() {
        let now = Instant::now();
        let mut pending = Pending::default();
        assert_eq!(pending.delay(now), Duration::ZERO);
        pending.last_sent = Some(now);
        for _ in 0..10_000 {
            pending.requested = true;
            assert_eq!(
                pending.delay(now + Duration::from_millis(5)),
                Duration::from_millis(11)
            );
        }
        assert_eq!(pending.delay(now + MIN_INTERVAL), Duration::ZERO);
        assert_eq!(pending.delay(now + Duration::from_secs(1)), Duration::ZERO);
    }

    #[test]
    fn service_wake_retains_requests_during_send_and_returns_to_idle() {
        let (sent, receive) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let first = std::sync::atomic::AtomicBool::new(true);
        let wake = ServiceWake::new(move || {
            sent.send(Instant::now()).unwrap();
            if first.swap(false, std::sync::atomic::Ordering::Relaxed) {
                wait.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            true
        })
        .unwrap();
        wake.request();
        let first = receive.recv_timeout(Duration::from_secs(5)).unwrap();
        // Block the sender so all requests are demonstrably in the same burst.
        std::thread::scope(|scope| {
            for _ in 0..10 {
                let wake = &wake;
                scope.spawn(move || {
                    for _ in 0..1_000 {
                        wake.request();
                    }
                });
            }
        });
        release.send(()).unwrap();
        let second = receive.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(second.duration_since(first) >= MIN_INTERVAL);
        assert!(receive.recv_timeout(MIN_INTERVAL * 3).is_err());
        wake.request();
        receive.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn service_wake_shutdown_cancels_pending_and_closed_loop_stops_worker() {
        let (sent, receive) = mpsc::channel();
        let wake = ServiceWake::new(move || {
            sent.send(()).unwrap();
            false
        })
        .unwrap();
        wake.request();
        receive.recv_timeout(Duration::from_secs(5)).unwrap();
        wake.request();
        drop(wake); // joins the worker, so disconnection is deterministic
        assert!(receive.recv().is_err());

        let (sent, receive) = mpsc::channel();
        let wake = ServiceWake::new(move || {
            sent.send(()).unwrap();
            true
        })
        .unwrap();
        // Arrange a long cooldown so only dropping the last owner can retire
        // this pending request. No wall-clock sleep is needed for shutdown.
        {
            let mut pending = wake.0.shared.0.lock().unwrap();
            pending.requested = true;
            pending.last_sent = Some(Instant::now() + Duration::from_secs(60));
        }
        drop(wake);
        assert!(receive.recv().is_err());
    }
}
