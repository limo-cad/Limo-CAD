//! The file watcher probes physical memory; the ordered worker evicts engines.
use super::*;
use std::{sync::atomic::AtomicU8, time::Instant};
use workspace::MemoryPressure;

const PROBE_INTERVAL: Duration = Duration::from_secs(30);
#[derive(Clone, Default)]
pub(super) struct Wake {
    due: Arc<AtomicBool>,
    pressure: Arc<AtomicU8>,
}
pub(super) struct Watch {
    memory: sysinfo::System,
    next: Instant,
    wake: Wake,
}
impl Watch {
    pub(super) fn new(wake: Wake) -> Self {
        Self {
            memory: sysinfo::System::new(),
            next: Instant::now() + PROBE_INTERVAL,
            wake,
        }
    }
    pub(super) fn poll(&mut self, now: Instant) -> bool {
        if now < self.next {
            return false;
        }
        self.next = now + PROBE_INTERVAL;
        self.memory.refresh_memory();
        let pressure = MemoryPressure::from_physical_memory(
            self.memory.total_memory(),
            self.memory.available_memory(),
        );
        self.wake.pressure.store(
            match pressure {
                MemoryPressure::Normal => 0,
                MemoryPressure::Constrained => 1,
                MemoryPressure::Critical => 2,
            },
            Ordering::Relaxed,
        );
        self.wake.due.store(true, Ordering::Release);
        true
    }
}
pub(super) fn tick(
    world: &mut World,
    services: &NativeServices,
    state: &Controller,
) -> Result<bool, String> {
    if !worker::available(world)
        || worker::busy(world)
        || state.pending.is_some()
        || state.close_pending
        || state.exit_after_receipt
        || files::awaiting(world)
    {
        return Ok(false);
    }
    if !state.retention_wake.due.swap(false, Ordering::Acquire) {
        return Ok(false);
    }
    let owner = services
        .bridge
        .native_document_context(&state.window_id, &services.engine)?;
    let pressure = match state.retention_wake.pressure.load(Ordering::Relaxed) {
        1 => MemoryPressure::Constrained,
        2 => MemoryPressure::Critical,
        _ => MemoryPressure::Normal,
    };
    if !state
        .workspace
        .lock()
        .map_err(|_| "Document workspace lock poisoned")?
        .has_eviction_candidates(&services.engine, &owner, pressure, Instant::now())
    {
        return Ok(false);
    }
    let expected = services
        .bridge
        .native_document_receipt(&services.engine, &owner)?;
    let workspace = state.workspace.clone();
    let completion_workspace = workspace.clone();
    worker::enqueue_retention(
        world,
        move |services, guard| {
            let released = workspace
                .lock()
                .map_err(|_| "Document workspace lock poisoned")?
                .evict_inactive(
                    &services.bridge,
                    &services.engine,
                    &expected,
                    pressure,
                    Instant::now(),
                    || guard.validate(),
                )?;
            Ok(NativeMutationResult {
                context: expected.owner,
                engine_revision: expected.revision,
                value: json!({"evicted_tabs":released}),
            })
        },
        move |world, services, result| {
            let cold = completion_workspace
                .lock()
                .map_err(|_| "Document workspace lock poisoned")?
                .cold_documents(&services.engine);
            for owner in cold {
                native_viewport::retire_interface_model_session(world, &owner.document_id);
                workbench::evict_document_geometry(world, &owner);
            }
            result.map(|result| result.value)
        },
    )?;
    Ok(true)
}
