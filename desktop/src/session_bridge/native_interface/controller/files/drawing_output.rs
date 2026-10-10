//! Active-sheet output through the existing exact projection/export command.
use super::*;
use crate::session_bridge::parse_engine_envelope;
use limo_cad_occt::drawing_export::{DrawingExportFormat, DrawingExportRequest};
use limo_cad_sketch::DrawingDocumentDto;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Svg,
    Dxf,
}
impl Format {
    fn extension(self) -> &'static str {
        match self {
            Self::Svg => "svg",
            Self::Dxf => "dxf",
        }
    }
    fn shared(self) -> DrawingExportFormat {
        match self {
            Self::Svg => DrawingExportFormat::Svg,
            Self::Dxf => DrawingExportFormat::Dxf,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct ExportIntent {
    format: Format,
    sheet_id: u64,
    sheet_name: String,
}

fn check_revision(receipt: &DocumentReceipt, revision: u64) -> Result<(), String> {
    if receipt.revision != revision {
        return Err(
            "The document changed while choosing the drawing file. Start export again.".into(),
        );
    }
    Ok(())
}

pub(super) fn capture(
    services: &NativeServices,
    receipt: &DocumentReceipt,
    format: Format,
) -> Result<ExportIntent, String> {
    services
        .bridge
        .with_native_document_receipt(&services.engine, &receipt.owner, |revision| {
            check_revision(receipt, revision)?;
            let drawing: DrawingDocumentDto = serde_json::from_value(parse_engine_envelope(
                services.engine.engine_call("drawing_document", ""),
            )?)
            .map_err(|error| error.to_string())?;
            let sheet = drawing
                .sheets
                .iter()
                .find(|sheet| Some(sheet.id) == drawing.active_sheet_id)
                .ok_or("Select a drawing sheet to export")?;
            Ok(ExportIntent {
                format,
                sheet_id: sheet.id,
                sheet_name: sheet.name.clone(),
            })
        })
}

pub(super) fn choose(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    receipt: DocumentReceipt,
    intent: ExportIntent,
) -> Result<Value, String> {
    if awaiting(world) {
        return Err("Finish the current file chooser first".into());
    }
    let active = tabs(world, services, &receipt.owner)?
        .into_iter()
        .find(|tab| tab.active)
        .ok_or("Active tab disappeared")?;
    let (send, receive) = mpsc::channel();
    let wake = handle.clone();
    let filename = format!(
        "{}-{}.{}",
        active.name,
        intent.sheet_name,
        intent.format.extension()
    )
    .replace(['<', '>', ':', '"', '/', '\\', '|', '?', '*'], "_");
    let extension = intent.format.extension();
    let (dialog, parent) = parented_dialog(
        world,
        rfd::FileDialog::new()
            .add_filter("Drawing sheet", &[extension])
            .set_file_name(filename),
    )?;
    std::thread::Builder::new()
        .name("cad-drawing-picker".into())
        .spawn(move || {
            let _parent = parent;
            let mut dialog = dialog;
            if let Some(parent) = active.path.as_ref().and_then(|path| path.parent()) {
                dialog = dialog.set_directory(parent);
            }
            let _ = send.send(dialog.save_file());
            wake.request_redraw();
        })
        .map_err(|error| format!("Cannot open file chooser: {error}"))?;
    world.resource_mut::<Files>().picker = Some(Picker {
        receipt,
        kind: PickerKind::Drawing(intent),
        result: Mutex::new(receive),
    });
    Ok(json!({"awaiting_input":true}))
}

pub(super) fn export(
    world: &mut World,
    receipt: DocumentReceipt,
    intent: ExportIntent,
    path: PathBuf,
    overwrite: bool,
) -> Result<Value, String> {
    if !path.is_absolute()
        || !path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case(intent.format.extension()))
    {
        return Err(format!(
            "Choose an absolute .{} file path",
            intent.format.extension()
        ));
    }
    worker::enqueue_document_io(
        world,
        format!("export_drawing_{}", intent.format.extension()),
        move |services, guard| {
            services.bridge.with_native_document_receipt(
                &services.engine,
                &receipt.owner,
                |revision| {
                    check_revision(&receipt, revision)?;
                    guard.validate_preparation()?;
                    let payload = serde_json::to_string(&DrawingExportRequest {
                        sheet_id: intent.sheet_id,
                        format: intent.format.shared(),
                    })
                    .map_err(|error| error.to_string())?;
                    let result = parse_engine_envelope(services.engine.drawing_export(&payload))?;
                    let content = result["content"]
                        .as_str()
                        .ok_or("Drawing export returned no content")?;
                    guard.validate()?;
                    if overwrite {
                        limo_cad_project_file::write_binary_file_atomic(&path, content.as_bytes())
                    } else {
                        limo_cad_project_file::write_binary_file_new(&path, content.as_bytes())
                    }
                    .map_err(|error| error.to_string())?;
                    Ok(NativeMutationResult {
                        context: receipt.owner.clone(),
                        engine_revision: revision,
                        value: json!({"exported":true,"path":path,"bytes":content.len(),
                            "sheet_id":intent.sheet_id,"format":intent.format.extension()}),
                    })
                },
            )
        },
        |_, _, result| Ok(result?.value),
    )
}

#[cfg(test)]
mod tests;
