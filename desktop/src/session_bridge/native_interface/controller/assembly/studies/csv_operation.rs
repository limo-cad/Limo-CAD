//! Picker-owned authority never keeps a cancelled study export alive.
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub(super) struct Authority(Arc<Mutex<State>>);
#[derive(Default)]
struct State {
    sequence: u64,
    active: bool,
}
#[derive(Clone)]
pub(super) struct Lease {
    state: Arc<Mutex<State>>,
    sequence: u64,
}
impl Authority {
    pub(super) fn revoke(&self) {
        if let Ok(mut state) = self.0.lock() {
            state.active = false;
        }
    }
    pub(super) fn begin(&self) -> Result<Lease, String> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| "Motion export operation lock poisoned")?;
        state.active = false;
        state.sequence = state
            .sequence
            .checked_add(1)
            .ok_or("Motion export sequence exhausted")?;
        state.active = true;
        Ok(Lease {
            state: self.0.clone(),
            sequence: state.sequence,
        })
    }
}
impl Drop for Authority {
    fn drop(&mut self) {
        self.revoke();
    }
}
impl Lease {
    pub(super) fn commit<T>(
        &self,
        commit: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Motion export operation lock poisoned")?;
        if !state.active || state.sequence != self.sequence {
            return Err("The motion export was cancelled or replaced".into());
        }
        state.active = false;
        commit()
    }
}
