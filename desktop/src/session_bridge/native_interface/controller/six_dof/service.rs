//! One process transport. Driver discovery/open/close and joins run off Bevy.
use super::mailbox::{Mailbox, Motion};
use crate::six_dof_mouse::{SixDofEvent, SixDofEventSink, SixDofMouseInfo, SixDofMouseState};
use limo_cad_interface::DocumentContext;
use std::{
    collections::BTreeMap,
    sync::{Arc, Condvar, Mutex, OnceLock, Weak},
    time::Instant,
};

pub(super) type Wake = Arc<dyn Fn() + Send + Sync>;
pub(super) trait Backend: Send + 'static {
    fn connect(&mut self, sink: SixDofEventSink) -> Result<SixDofMouseInfo, String>;
    fn disconnect(&mut self) -> Result<(), String>;
}
impl Backend for SixDofMouseState {
    fn connect(&mut self, sink: SixDofEventSink) -> Result<SixDofMouseInfo, String> {
        SixDofMouseState::connect(self, sink)
    }
    fn disconnect(&mut self) -> Result<(), String> {
        SixDofMouseState::disconnect(self)
    }
}
#[derive(Clone, Debug, serde::Serialize)]
pub(in super::super) struct Status {
    pub generation: u64,
    pub state: &'static str,
    pub message: String,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            generation: 0,
            state: "disconnected",
            message: "Connect 3D mouse".into(),
        }
    }
}
#[derive(Default)]
struct State {
    command: u64,
    desired: bool,
    stop: bool,
    status: Status,
    windows: BTreeMap<String, Wake>,
    route: Option<DocumentContext>,
    mailbox: Mailbox,
}
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}
pub(super) struct Service {
    shared: Arc<Shared>,
}

impl Service {
    pub fn process() -> Result<Arc<Self>, String> {
        static INSTANCE: OnceLock<Mutex<Option<Arc<Service>>>> = OnceLock::new();
        let mut instance = INSTANCE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "3D mouse service lock poisoned")?;
        if let Some(service) = instance.as_ref() {
            return Ok(service.clone());
        }
        let service = Self::new(SixDofMouseState::default())?;
        *instance = Some(service.clone());
        Ok(service)
    }
    pub(super) fn new(backend: impl Backend) -> Result<Arc<Self>, String> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
        });
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("cad-3d-mouse-connection".into())
            .spawn(move || run(worker, backend))
            .map_err(|e| format!("Cannot start 3D mouse worker: {e}"))?;
        Ok(Arc::new(Self { shared }))
    }
    pub fn register(&self, window: String, wake: Wake) {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .windows
            .insert(window, wake);
    }
    pub fn unregister(&self, window: &str) {
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        state.windows.remove(window);
        if state.route.as_ref().is_some_and(|r| r.window_id == window) {
            state.route = None;
            state.mailbox.clear();
        }
        if state.windows.is_empty() {
            state.command = state.command.wrapping_add(1);
            state.desired = false;
            state.status = Status {
                generation: state.command,
                state: "disconnecting",
                message: "Disconnecting 3D mouse…".into(),
            };
            state.route = None;
            state.mailbox.clear();
            self.shared.changed.notify_one();
        }
    }
    pub fn status(&self) -> Status {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .status
            .clone()
    }
    pub fn request_at(&self, generation: u64, connect: bool) -> Result<Status, String> {
        let (status, wake) = {
            let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.status.generation != generation
                || matches!(state.status.state, "connecting" | "disconnecting")
                || connect == (state.status.state == "connected")
            {
                return Err("The 3D mouse connection changed; inspect again".into());
            }
            state.command = state.command.wrapping_add(1);
            state.desired = connect;
            state.route = None;
            state.mailbox.clear();
            state.status = Status {
                generation: state.command,
                state: if connect {
                    "connecting"
                } else {
                    "disconnecting"
                },
                message: if connect {
                    "Connecting 3D mouse…"
                } else {
                    "Disconnecting 3D mouse…"
                }
                .into(),
            };
            (
                state.status.clone(),
                state.windows.values().cloned().collect::<Vec<_>>(),
            )
        };
        self.shared.changed.notify_one();
        for wake in wake {
            wake();
        }
        Ok(status)
    }
    pub fn sample(
        &self,
        window: &str,
        owner: Option<&DocumentContext>,
        now: Instant,
    ) -> (Motion, bool) {
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        let owner = owner.filter(|_| state.status.state == "connected");
        match owner {
            Some(owner) if state.route.as_ref() != Some(owner) => {
                state.route = Some(owner.clone());
                state.mailbox.clear();
            }
            None if state.route.as_ref().is_some_and(|r| r.window_id == window) => {
                state.route = None;
                state.mailbox.clear();
            }
            _ => (),
        }
        if owner.is_some() && state.route.as_ref() == owner {
            state.mailbox.sample(now)
        } else {
            (Motion::default(), false)
        }
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        state.stop = true;
        state.command = state.command.wrapping_add(1);
        state.route = None;
        state.mailbox.clear();
        self.shared.changed.notify_one();
    }
}
fn receive(shared: &Weak<Shared>, generation: u64, event: SixDofEvent) {
    let Some(shared) = shared.upgrade() else {
        return;
    };
    let wake = {
        let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stop || state.command != generation {
            return;
        }
        if let SixDofEvent::Error(error) = event {
            state.status = Status {
                generation,
                state: "error",
                message: format!("3D mouse input stopped: {error}"),
            };
            state.route = None;
            state.mailbox.clear();
            state.desired = false;
            shared.changed.notify_one();
            state.windows.values().cloned().collect::<Vec<_>>()
        } else if state.status.state == "connected" {
            let wake = state
                .route
                .as_ref()
                .and_then(|r| state.windows.get(&r.window_id))
                .cloned();
            if wake.is_some() {
                state.mailbox.push(event, Instant::now());
            }
            wake.into_iter().collect()
        } else {
            vec![]
        }
    };
    for wake in wake {
        wake();
    }
}
fn run(shared: Arc<Shared>, mut backend: impl Backend) {
    let mut completed = 0;
    let mut connected = false;
    loop {
        let (generation, connect) = {
            let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
            while !state.stop && state.command == completed && !(connected && !state.desired) {
                state = shared
                    .changed
                    .wait(state)
                    .unwrap_or_else(|e| e.into_inner());
            }
            if state.stop {
                drop(state);
                let _ = backend.disconnect();
                break;
            }
            (state.command, state.desired)
        };
        let result = if connect {
            let source = Arc::downgrade(&shared);
            backend
                .connect(Arc::new(move |event| receive(&source, generation, event)))
                .map(|info| format!("{} · Click to disconnect", info.product_name))
        } else {
            backend.disconnect().map(|_| "Connect 3D mouse".into())
        };
        connected = connect && result.is_ok();
        let wake = {
            let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
            completed = generation;
            if state.command != generation || state.stop {
                drop(state);
                let _ = backend.disconnect();
                connected = false;
                continue;
            }
            if state.status.state != "error" {
                state.status = Status {
                    generation,
                    state: if result.is_err() {
                        "error"
                    } else if connect {
                        "connected"
                    } else {
                        "disconnected"
                    },
                    message: result.unwrap_or_else(|e| e),
                };
            }
            state.windows.values().cloned().collect::<Vec<_>>()
        };
        for wake in wake {
            wake();
        }
    }
}

#[cfg(test)]
mod tests;
