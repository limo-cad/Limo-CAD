//! Private draft authority. Owned export snapshots do not keep a cancelled
//! dialog alive. The mutex is held only across the final registration/write.
use super::*;

#[derive(Clone, Debug, Default)]
pub(super) struct Authority(Arc<Mutex<State>>);

#[derive(Debug, Default)]
struct State {
    sequence: u64,
    active: Option<(u64, u64, DocumentReceipt)>,
}

#[derive(Clone)]
pub(super) struct Lease {
    authority: Authority,
    sequence: u64,
    token: u64,
    generation: u64,
    receipt: DocumentReceipt,
}

impl Authority {
    pub(super) fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub(super) fn revoke(&self) {
        // Poisoning also fails closed when an operation attempts to commit.
        if let Ok(mut state) = self.0.lock() {
            state.active = None;
        }
    }

    fn begin(
        &self,
        token: u64,
        generation: u64,
        receipt: &DocumentReceipt,
    ) -> Result<Lease, String> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| "Export operation lock poisoned")?;
        state.active = None;
        state.sequence = state
            .sequence
            .checked_add(1)
            .ok_or("Export operation sequence exhausted")?;
        state.active = Some((token, generation, receipt.clone()));
        Ok(Lease {
            authority: self.clone(),
            sequence: state.sequence,
            token,
            generation,
            receipt: receipt.clone(),
        })
    }
}

impl Lease {
    pub(super) fn commit<T>(
        &self,
        commit: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let mut state = self
            .authority
            .0
            .lock()
            .map_err(|_| "Export operation lock poisoned")?;
        if state.sequence != self.sequence
            || !state
                .active
                .as_ref()
                .is_some_and(|(token, generation, receipt)| {
                    *token == self.token
                        && *generation == self.generation
                        && receipt == &self.receipt
                })
        {
            return Err("The Bambu export was cancelled or its choices were replaced".into());
        }
        // Consume before committing: a failed write cannot replay registration.
        state.active = None;
        commit()
    }
}

pub(super) fn begin(
    world: &World,
    receipt: &DocumentReceipt,
    intent: &io::ExportIntent,
) -> Result<Lease, String> {
    let dialog = world
        .resource::<Files>()
        .dialog
        .as_ref()
        .ok_or("Bambu export dialog closed")?;
    let DialogKind::Export(current) = &dialog.kind else {
        return Err("Bambu export dialog replaced".into());
    };
    if dialog.receipt != *receipt
        || current.bambu.generation != intent.bambu.generation
        || !current.bambu.operation.same(&intent.bambu.operation)
    {
        return Err("Bambu export choices or source were replaced".into());
    }
    intent
        .bambu
        .operation
        .begin(dialog.token, intent.bambu.generation, receipt)
}
