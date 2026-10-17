//! One parked timer per native host, with a single replaceable deadline.
use super::*;
use std::sync::Condvar;

#[derive(Default)]
struct Pending {
    job: Option<(Instant, Arc<AtomicBool>)>,
    stop: bool,
}
#[derive(Resource)]
pub(super) struct Timer {
    state: Arc<(Mutex<Pending>, Condvar)>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Timer {
    fn new(handle: NativeInterfaceHandle) -> Result<Self, String> {
        let state = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
        let shared = state.clone();
        let worker = std::thread::Builder::new()
            .name("cad-lesson-hover".into())
            .stack_size(64 * 1024)
            .spawn(move || {
                let (lock, changed) = &*shared;
                let Ok(mut pending) = lock.lock() else { return };
                loop {
                    if pending.stop {
                        return;
                    }
                    let Some((due, _)) = &pending.job else {
                        let Ok(next) = changed.wait(pending) else {
                            return;
                        };
                        pending = next;
                        continue;
                    };
                    let delay = due.saturating_duration_since(Instant::now());
                    if !delay.is_zero() {
                        let Ok((next, _)) = changed.wait_timeout(pending, delay) else {
                            return;
                        };
                        pending = next;
                        continue;
                    }
                    let (_, cancelled) = pending.job.take().unwrap();
                    drop(pending);
                    if !cancelled.load(Ordering::Acquire) {
                        handle.request_redraw();
                    }
                    let Ok(next) = lock.lock() else { return };
                    pending = next;
                }
            })
            .map_err(|error| format!("Cannot schedule operation hint: {error}"))?;
        Ok(Self {
            state,
            worker: Some(worker),
        })
    }
    fn arm(&self, due: Instant, cancelled: Arc<AtomicBool>) -> Result<(), String> {
        let mut state = self
            .state
            .0
            .lock()
            .map_err(|_| "Operation hint timer stopped")?;
        if let Some((_, old)) = state.job.replace((due, cancelled)) {
            old.store(true, Ordering::Release);
        }
        self.state.1.notify_one();
        Ok(())
    }
}
impl Drop for Timer {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.0.lock() {
            state.stop = true;
            if let Some((_, token)) = state.job.take() {
                token.store(true, Ordering::Release);
            }
            self.state.1.notify_one();
        }
        // Host teardown never waits on a callback into that same host.
        drop(self.worker.take());
    }
}
pub(super) fn arm(
    world: &mut World,
    handle: NativeInterfaceHandle,
    due: Instant,
    cancelled: Arc<AtomicBool>,
) -> Result<(), String> {
    if !world.contains_resource::<Timer>() {
        world.insert_resource(Timer::new(handle)?);
    }
    world.resource::<Timer>().arm(due, cancelled)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_script_preview_hover_replaces_deadlines_without_spawning_more_workers() {
        let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = wakes.clone();
        let timer = Timer::new(NativeInterfaceHandle::new(move || {
            counted.fetch_add(1, Ordering::Relaxed);
        }))
        .unwrap();
        let worker = timer.worker.as_ref().unwrap().thread().id();
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut tokens = Vec::new();
        for _ in 0..10_000 {
            let token = Arc::new(AtomicBool::new(false));
            timer.arm(deadline, token.clone()).unwrap();
            tokens.push(token);
        }
        assert_eq!(timer.worker.as_ref().unwrap().thread().id(), worker);
        assert!(tokens[..tokens.len() - 1]
            .iter()
            .all(|token| token.load(Ordering::Acquire)));
        assert_eq!(wakes.load(Ordering::Relaxed), 0);
        drop(timer);
        assert!(tokens.last().unwrap().load(Ordering::Acquire));
    }
}
