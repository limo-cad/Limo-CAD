//! One ordered kernel worker for the native host. Preparing a typed intent
//! never grants access to a later document: dispatch rechecks its receipt and
//! original control stamp under the existing publisher -> engine fence.

use super::*;
use std::sync::{mpsc, Mutex};

type Completion = Box<
    dyn FnOnce(
            &mut World,
            &NativeServices,
            Result<NativeMutationResult, String>,
        ) -> Result<Value, String>
        + Send,
>;
type Transaction =
    Box<dyn FnOnce(&NativeServices, &DispatchGuard) -> Result<NativeMutationResult, String> + Send>;

pub(crate) struct DispatchGuard {
    handle: NativeInterfaceHandle,
    action: Option<NativeInterfaceAction>,
    started: Arc<AtomicBool>,
}
impl DispatchGuard {
    /// Check a read-only preparation phase without admitting the busy frame.
    /// Call `validate` again inside the owner/revision fence immediately before
    /// dispatch or writing output; preparation never grants mutation access.
    pub(crate) fn validate_preparation(&self) -> Result<(), String> {
        if let Some(action) = &self.action {
            self.handle.validate_dispatch_action(action)?;
        }
        Ok(())
    }

    /// Call inside the owner/revision fence, immediately before dispatch.
    /// Busy presentation may disable controls only after this point, so it
    /// cannot invalidate the transaction that caused it to become busy.
    pub(crate) fn validate(&self) -> Result<(), String> {
        self.validate_preparation()?;
        self.started.store(true, Ordering::Release);
        self.handle.request_redraw();
        Ok(())
    }
}

/// Present only while reducing an actual human/MCP control. Canvas commands
/// instead carry their gesture's document and exact engine revision.
#[derive(Resource, Clone)]
pub(crate) struct ActiveControl(pub NativeInterfaceAction);

struct Job {
    id: u64,
    operation: String,
    action: Option<NativeInterfaceAction>,
    transaction: Transaction,
    started: Arc<AtomicBool>,
    prepare_presentation: fn(&NativeMutationResult) -> bool,
}
struct Completed {
    id: u64,
    result: Result<NativeMutationResult, String>,
    presentation: Option<PreparedNativePresentation>,
}
struct Pending {
    id: u64,
    operation: String,
    completion: Mutex<Option<Completion>>,
    started: Arc<AtomicBool>,
}

#[derive(Resource)]
pub(crate) struct NativeMutationWorker {
    jobs: mpsc::SyncSender<Job>,
    completed: Mutex<mpsc::Receiver<Completed>>,
    pending: Option<Pending>,
    next_id: u64,
}

pub(crate) struct Outcome {
    pub id: u64,
    pub operation: String,
    pub value: Result<Value, String>,
}

pub(crate) fn install(
    world: &mut World,
    services: NativeServices,
    handle: NativeInterfaceHandle,
) -> Result<(), String> {
    let (jobs, incoming) = mpsc::sync_channel::<Job>(1);
    let (results, completed) = mpsc::channel();
    std::thread::Builder::new().name("cad-native-kernel".into()).spawn(move || {
        while let Ok(job) = incoming.recv() {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let guard = DispatchGuard { handle: handle.clone(), action: job.action, started: job.started };
                (job.transaction)(&services, &guard)
            }));
            let mut panicked = outcome.is_err();
            let result = outcome.unwrap_or_else(|_| Err("The modeling worker stopped unexpectedly; its transaction must be reviewed before continuing".into()));
            let presentation = result.as_ref().ok().filter(|result| (job.prepare_presentation)(result)).map(|result| {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| prepare_native_presentation(&services.engine, &services.bridge, result, &job.operation)))
                    .unwrap_or_else(|_| {
                        panicked = true;
                        PreparedNativePresentation { owner: result.context.clone(), revision: result.engine_revision, scene: Err("Committed model snapshot preparation stopped unexpectedly".into()), publication: Err("Committed model publication preparation stopped unexpectedly".into()) }
                    })
            });
            if results.send(Completed { id: job.id, result, presentation }).is_err() { break; }
            handle.request_redraw();
            if panicked { break; }
        }
    }).map_err(|error| format!("Could not start the modeling worker: {error}"))?;
    world.insert_resource(NativeMutationWorker {
        jobs,
        completed: Mutex::new(completed),
        pending: None,
        next_id: 0,
    });
    Ok(())
}

pub(crate) fn available(world: &World) -> bool {
    world.contains_resource::<NativeMutationWorker>()
}
pub(crate) fn busy(world: &World) -> bool {
    world
        .get_resource::<NativeMutationWorker>()
        .is_some_and(|worker| worker.pending.is_some())
}
pub(super) fn pending_operation(world: &World) -> Option<&str> {
    world
        .get_resource::<NativeMutationWorker>()
        .and_then(|worker| worker.pending.as_ref())
        .map(|pending| pending.operation.as_str())
}
pub(crate) fn started(world: &World) -> bool {
    world
        .get_resource::<NativeMutationWorker>()
        .and_then(|worker| worker.pending.as_ref())
        .is_some_and(|pending| pending.started.load(Ordering::Acquire))
}

pub(crate) fn enqueue_operation(
    world: &mut World,
    owner: DocumentContext,
    revision: u64,
    operation: String,
    arguments: Value,
    complete: impl FnOnce(
            &mut World,
            &NativeServices,
            Result<NativeMutationResult, String>,
        ) -> Result<Value, String>
        + Send
        + 'static,
) -> Result<Value, String> {
    let label = operation.clone();
    enqueue_transaction(
        world,
        label,
        move |services, guard| {
            services.bridge.apply_native_mutation_at(
                &services.engine,
                &owner,
                revision,
                &operation,
                &arguments,
                || guard.validate(),
            )
        },
        complete,
    )
}

/// Shared history/inbox transactions use their existing dispatcher here;
/// there is no worker-specific operation switch or second modeling schema.
pub(crate) fn enqueue_transaction(
    world: &mut World,
    operation: String,
    transaction: impl FnOnce(&NativeServices, &DispatchGuard) -> Result<NativeMutationResult, String>
        + Send
        + 'static,
    complete: impl FnOnce(
            &mut World,
            &NativeServices,
            Result<NativeMutationResult, String>,
        ) -> Result<Value, String>
        + Send
        + 'static,
) -> Result<Value, String> {
    enqueue(world, operation, transaction, complete, |_| true)
}

/// Memory maintenance changes only inactive caches, preserving the active scene.
pub(crate) fn enqueue_retention(
    world: &mut World,
    transaction: impl FnOnce(&NativeServices, &DispatchGuard) -> Result<NativeMutationResult, String>
        + Send
        + 'static,
    complete: impl FnOnce(
            &mut World,
            &NativeServices,
            Result<NativeMutationResult, String>,
        ) -> Result<Value, String>
        + Send
        + 'static,
) -> Result<Value, String> {
    enqueue(
        world,
        "memory-retention".into(),
        transaction,
        complete,
        |_| false,
    )
}

/// Inbox reads retain the current scene and snapshot as well as the model
/// receipt. Do not clone all geometry merely to present an unchanged CAM query.
pub(crate) fn enqueue_inbox(
    world: &mut World,
    transaction: impl FnOnce(&NativeServices, &DispatchGuard) -> Result<NativeMutationResult, String>
        + Send
        + 'static,
    complete: impl FnOnce(
            &mut World,
            &NativeServices,
            Result<NativeMutationResult, String>,
        ) -> Result<Value, String>
        + Send
        + 'static,
) -> Result<Value, String> {
    enqueue(world, "inbox".into(), transaction, complete, |result| {
        result.value["applied"] == true && result.value["model_changed"] != false
    })
}

pub(crate) fn enqueue_control_poll(
    world: &mut World,
    owner: DocumentContext,
    request_id: String,
) -> Result<Value, String> {
    enqueue(
        world,
        "interface".into(),
        move |services, _guard| {
            let request = crate::session_bridge::control_for_window_owned(
                &services.bridge,
                &owner.window_id,
                &services.engine,
                None,
                Some((&owner, &request_id)),
            )?;
            let receipt = services
                .bridge
                .native_document_receipt(&services.engine, &owner)?;
            Ok(NativeMutationResult {
                context: owner,
                engine_revision: receipt.revision,
                value: json!({"control_request":request}),
            })
        },
        |_, _, result| Ok(result?.value),
        |_| false,
    )
}

/// Saving an unchanged document requires no new mesh or presentation snapshot.
pub(crate) fn enqueue_document_io(
    world: &mut World,
    operation: String,
    transaction: impl FnOnce(&NativeServices, &DispatchGuard) -> Result<NativeMutationResult, String>
        + Send
        + 'static,
    complete: impl FnOnce(
            &mut World,
            &NativeServices,
            Result<NativeMutationResult, String>,
        ) -> Result<Value, String>
        + Send
        + 'static,
) -> Result<Value, String> {
    enqueue(world, operation, transaction, complete, |_| false)
}

/// A read-only solver preview keeps the kernel off the input/render thread and
/// does not publish, advance history or prepare a replacement geometry snapshot.
pub(crate) fn enqueue_query(
    world: &mut World,
    owner: DocumentContext,
    revision: u64,
    operation: String,
    arguments: Value,
    complete: impl FnOnce(
            &mut World,
            &NativeServices,
            Result<NativeMutationResult, String>,
        ) -> Result<Value, String>
        + Send
        + 'static,
) -> Result<Value, String> {
    enqueue_prepared_query(
        world,
        owner,
        revision,
        operation,
        arguments,
        Ok,
        move |world, services, result| {
            complete(
                world,
                services,
                result.map(|(mut receipt, value)| {
                    receipt.value = value;
                    receipt
                }),
            )
        },
    )
}

/// Consume the query response while preparing CPU presentation data on the
/// ordered worker. Only prepared data and the ownership receipt reach the UI.
pub(crate) fn enqueue_prepared_query<P: Send + 'static>(
    world: &mut World,
    owner: DocumentContext,
    revision: u64,
    operation: String,
    arguments: Value,
    prepare: impl FnOnce(Value) -> Result<P, String> + Send + 'static,
    complete: impl FnOnce(
            &mut World,
            &NativeServices,
            Result<(NativeMutationResult, P), String>,
        ) -> Result<Value, String>
        + Send
        + 'static,
) -> Result<Value, String> {
    let (output, prepared) = mpsc::sync_channel(1);
    let label = operation.clone();
    enqueue(
        world,
        label,
        move |services, guard| {
            let mut result = services.bridge.with_native_document_receipt(
                &services.engine,
                &owner,
                |current| {
                    if current != revision {
                        return Err("The model changed before the preview could run".into());
                    }
                    guard.validate()?;
                    let value = crate::session_bridge::parse_engine_envelope(
                        services
                            .engine
                            .engine_call(&operation, &arguments.to_string()),
                    )?;
                    Ok(NativeMutationResult {
                        context: owner.clone(),
                        engine_revision: revision,
                        value,
                    })
                },
            )?;
            // Dispatch already admitted this control. The host may now disable
            // it in its busy frame; CPU preparation grants no new authority.
            let data = prepare(std::mem::take(&mut result.value))?;
            output
                .send(data)
                .map_err(|_| "Query preparation receiver retired")?;
            Ok(result)
        },
        move |world, services, result| {
            let result = result.and_then(|result| {
                let data = prepared
                    .try_recv()
                    .map_err(|_| "Query returned no prepared data")?;
                Ok((result, data))
            });
            complete(world, services, result)
        },
        |_| false,
    )
}

fn enqueue(
    world: &mut World,
    operation: String,
    transaction: impl FnOnce(&NativeServices, &DispatchGuard) -> Result<NativeMutationResult, String>
        + Send
        + 'static,
    complete: impl FnOnce(
            &mut World,
            &NativeServices,
            Result<NativeMutationResult, String>,
        ) -> Result<Value, String>
        + Send
        + 'static,
    prepare_presentation: fn(&NativeMutationResult) -> bool,
) -> Result<Value, String> {
    let action = world
        .get_resource::<ActiveControl>()
        .map(|action| action.0.clone());
    let mut worker = world
        .get_resource_mut::<NativeMutationWorker>()
        .ok_or("This host does not have a native mutation worker")?;
    if worker.pending.is_some() {
        return Err("Wait for the current modeling operation to finish".into());
    }
    let id = worker
        .next_id
        .checked_add(1)
        .ok_or("Native mutation sequence exhausted")?;
    let started = Arc::new(AtomicBool::new(false));
    worker
        .jobs
        .try_send(Job {
            id,
            operation: operation.clone(),
            action,
            transaction: Box::new(transaction),
            started: started.clone(),
            prepare_presentation,
        })
        .map_err(|error| format!("Modeling operation was not queued: {error}"))?;
    worker.next_id = id;
    worker.pending = Some(Pending {
        id,
        operation,
        completion: Mutex::new(Some(Box::new(complete))),
        started,
    });
    Ok(json!({"mutation_pending":true,"mutation_id":id}))
}

/// Nonblocking receive only. Controller must not query the engine, publisher,
/// forms or editor stamps while this reports a still-running transaction.
pub(crate) fn poll(world: &mut World, services: &NativeServices) -> Option<Outcome> {
    if !busy(world) {
        return None;
    }
    let received = {
        let worker = world.resource::<NativeMutationWorker>();
        let receiver = worker
            .completed
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        receiver.try_recv()
    };
    let completed = match received {
        Ok(completed) => completed,
        Err(mpsc::TryRecvError::Empty) => return None,
        Err(mpsc::TryRecvError::Disconnected) => {
            let pending = world
                .resource_mut::<NativeMutationWorker>()
                .pending
                .take()?;
            let error = Err("The modeling worker disconnected before returning a result".into());
            return Some(Outcome {
                id: pending.id,
                operation: pending.operation,
                value: pending
                    .completion
                    .into_inner()
                    .map_err(|_| "Model completion storage was poisoned".to_owned())
                    .and_then(|completion| {
                        completion.ok_or("Model completion was already consumed".to_owned())
                    })
                    .and_then(|complete| complete(world, services, error)),
            });
        }
    };
    let pending = world
        .resource_mut::<NativeMutationWorker>()
        .pending
        .take()?;
    if completed.id != pending.id {
        return Some(Outcome {
            id: pending.id,
            operation: pending.operation,
            value: Err("Modeling completion does not match its pending transaction".into()),
        });
    }
    if let Some(presentation) = completed.presentation {
        world.insert_resource(presentation);
    }
    let value = pending
        .completion
        .into_inner()
        .map_err(|_| "Model completion storage was poisoned".to_owned())
        .and_then(|completion| completion.ok_or("Model completion was already consumed".to_owned()))
        .and_then(|complete| complete(world, services, completed.result));
    world.remove_resource::<PreparedNativePresentation>();
    Some(Outcome {
        id: pending.id,
        operation: pending.operation,
        value,
    })
}
