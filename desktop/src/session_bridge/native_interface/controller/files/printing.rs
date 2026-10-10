//! One immutable print snapshot from the existing drawing export service.
//! Preparation never edits the document or inserts a history record.
use super::*;
use crate::{native_print, session_bridge::parse_engine_envelope};
use bevy::window::RawHandleWrapper;
use limo_cad_occt::drawing_export::{DrawingExportFormat, DrawingExportRequest};

#[derive(Resource, Default)]
struct State {
    pending: bool,
    running: Option<native_print::Running>,
    owner: Option<DocumentContext>,
    status: Value,
    message: String,
}

fn prepare(
    services: &NativeServices,
    receipt: &DocumentReceipt,
    check: impl FnOnce() -> Result<(), String>,
) -> Result<(native_print::Page, u64), String> {
    services
        .bridge
        .with_native_document_receipt(&services.engine, &receipt.owner, |revision| {
            if revision != receipt.revision {
                return Err("The drawing changed before printing could start".into());
            }
            check()?;
            let drawing = services.engine.drawing_snapshot();
            let sheet = drawing
                .sheets
                .iter()
                .find(|sheet| Some(sheet.id) == drawing.active_sheet_id)
                .ok_or("Select a drawing sheet to print")?;
            let request = DrawingExportRequest {
                sheet_id: sheet.id,
                format: DrawingExportFormat::Svg,
            };
            let output = parse_engine_envelope(
                services
                    .engine
                    .drawing_export(&serde_json::to_string(&request).map_err(|e| e.to_string())?),
            )?;
            let svg = output["content"]
                .as_str()
                .ok_or("Drawing export returned no page")?;
            let title = format!("{} — {}", services.engine.document_name(), sheet.name);
            Ok((native_print::Page::prepare(title, svg)?, sheet.id))
        })
}

pub(super) fn request(world: &mut World, receipt: DocumentReceipt) -> Result<Value, String> {
    world.init_resource::<State>();
    if world.resource::<State>().pending || world.resource::<State>().running.is_some() {
        return Err("Finish the current print operation first".into());
    }
    let parent = world
        .query_filtered::<&RawHandleWrapper, With<PrimaryWindow>>()
        .single(world)
        .map_err(|_| "Native printing requires the active desktop window")?
        .clone();
    let slot = Arc::new(Mutex::new(None));
    let prepared = slot.clone();
    let complete_receipt = receipt.clone();
    let owner = receipt.owner.clone();
    let result = worker::enqueue_document_io(
        world,
        "print_drawing".into(),
        move |services, guard| {
            let page = prepare(services, &receipt, || guard.validate_preparation())?;
            services.bridge.with_native_document_receipt(
                &services.engine,
                &receipt.owner,
                |revision| {
                    if revision != receipt.revision {
                        return Err("The drawing changed while preparing print output".into());
                    }
                    guard.validate()?;
                    let sheet_id = page.1;
                    *prepared
                        .lock()
                        .map_err(|_| "Prepared print storage poisoned")? = Some(page.0);
                    Ok(NativeMutationResult {
                        context: receipt.owner.clone(),
                        engine_revision: revision,
                        value: json!({"print_pending":true,"sheet_id":sheet_id}),
                    })
                },
            )
        },
        move |world, services, result| {
            world.resource_mut::<State>().pending = false;
            let completed: Result<Value, String> = (|| {
                let result = result?;
                let page = slot
                    .lock()
                    .map_err(|_| "Prepared print storage poisoned")?
                    .take()
                    .ok_or("Prepared print page missing")?;
                let handle = world.resource::<NativeInterfaceHandle>().clone();
                services.bridge.with_native_document_receipt(
                    &services.engine,
                    &complete_receipt.owner,
                    |revision| {
                        if revision != complete_receipt.revision {
                            return Err("The drawing changed before its print dialog opened".into());
                        }
                        Ok(())
                    },
                )?;
                let running = native_print::start(parent, page, handle)?;
                let mut state = world.resource_mut::<State>();
                state.running = Some(running);
                state.status = json!({"state":"awaiting_print_dialog","sheet_id":result.value["sheet_id"],
            "source_revision":result.engine_revision});
                state.message = "Choose a printer or Save as PDF in the system print dialog".into();
                Ok(result.value)
            })();
            if let Err(error) = &completed {
                let mut state = world.resource_mut::<State>();
                state.status = json!({"state":"failed","message":error});
                state.message = error.clone();
            }
            completed
        },
    )?;
    let mut state = world.resource_mut::<State>();
    state.pending = true;
    state.owner = Some(owner);
    state.message = "Preparing drawing for the system print dialog…".into();
    state.status = json!({"state":"preparing"});
    Ok(result)
}

pub(super) fn poll(world: &mut World) {
    let Some(mut state) = world.get_resource_mut::<State>() else {
        return;
    };
    let Some(outcome) = state.running.as_ref().and_then(native_print::Running::poll) else {
        return;
    };
    state.running = None;
    native_print::retire();
    let (status, message, pdf_path) = match outcome {
        native_print::Outcome::Submitted { pdf_path } => (
            "submitted",
            if pdf_path.is_some() {
                "Drawing saved as a PDF".to_owned()
            } else {
                "Drawing submitted to the print system".to_owned()
            },
            pdf_path,
        ),
        native_print::Outcome::Cancelled => ("cancelled", "Printing cancelled".to_owned(), None),
        native_print::Outcome::Failed(error) => ("failed", error, None),
    };
    state.status["state"] = json!(status);
    state.status["message"] = json!(message);
    if let Some(path) = pdf_path {
        state.status["pdf_path"] = json!(path.display().to_string());
    }
    state.message = message;
}

pub(super) fn status(world: &World, owner: &DocumentContext) -> Value {
    world
        .get_resource::<State>()
        .filter(|state| state.owner.as_ref() == Some(owner))
        .map(|state| state.status.clone())
        .unwrap_or_else(|| json!({"state":"idle"}))
}
pub(in crate::session_bridge::native_interface::controller) fn message(
    world: &World,
    owner: &DocumentContext,
) -> Option<String> {
    world
        .get_resource::<State>()
        .filter(|state| state.owner.as_ref() == Some(owner))
        .map(|state| state.message.clone())
}
