//! Profile choice is transient. The shared current catalog and DXF writer own
//! boundary geometry; ordered document I/O owns every destination write.
use super::*;
use crate::session_bridge::parse_engine_envelope;
use limo_cad_solid::ProfileCatalogItemDto;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ExportIntent {
    pub feature_id: u64,
    pub profile_index: u32,
    pub sketch_name: String,
}
impl ExportIntent {
    pub fn key(&self) -> String {
        format!("{}:{}", self.feature_id, self.profile_index)
    }
    pub fn label(&self) -> String {
        format!(
            "{} / Profile {}",
            self.sketch_name,
            u64::from(self.profile_index) + 1
        )
    }
}
#[derive(Clone, Debug)]
pub(super) struct Selection {
    pub choices: Vec<ExportIntent>,
    pub selected: usize,
}
impl Selection {
    pub fn select(&mut self, input: &ControlInput) -> Result<(), String> {
        self.selected = match input {
            ControlInput::SetValue(value) => self
                .choices
                .iter()
                .position(|c| c.key() == *value)
                .ok_or("Choose an available material profile")?,
            ControlInput::Key(key) if !key.ctrl && !key.meta && !key.alt && !key.shift => {
                match key.key.as_str() {
                    "Home" => 0,
                    "End" => self.choices.len() - 1,
                    "ArrowUp" | "ArrowLeft" => self.selected.saturating_sub(1),
                    "ArrowDown" | "ArrowRight" => (self.selected + 1).min(self.choices.len() - 1),
                    _ => return Ok(()),
                }
            }
            input if super::super::super::is_activation(input) => {
                (self.selected + 1) % self.choices.len()
            }
            _ => return Err("Choose an available material profile".into()),
        };
        Ok(())
    }
}
fn check(receipt: &DocumentReceipt, revision: u64) -> Result<(), String> {
    if receipt.revision != revision {
        return Err(
            "The document changed while choosing the profile file. Start export again.".into(),
        );
    }
    Ok(())
}
fn catalog(services: &NativeServices) -> Result<Vec<ProfileCatalogItemDto>, String> {
    serde_json::from_value(parse_engine_envelope(
        services.engine.engine_call("profile_catalog", ""),
    )?)
    .map_err(|e| e.to_string())
}
pub(super) fn capture(
    services: &NativeServices,
    receipt: &DocumentReceipt,
) -> Result<Selection, String> {
    services
        .bridge
        .with_native_document_receipt(&services.engine, &receipt.owner, |revision| {
            check(receipt, revision)?;
            let mut choices = Vec::new();
            for sketch in catalog(services)? {
                for profile in sketch.profiles.iter().filter(|p| p.nesting_depth % 2 == 0) {
                    choices.push(ExportIntent {
                        feature_id: sketch.feature_id.0,
                        profile_index: profile.index,
                        sketch_name: sketch.sketch_name.clone(),
                    });
                    if choices.len() > 16_384 {
                        return Err("Too many material profiles to choose from".into());
                    }
                }
            }
            if choices.is_empty() {
                return Err("Finish a sketch with a closed material profile to export".into());
            }
            Ok(Selection {
                choices,
                selected: 0,
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
    if world.resource::<Files>().picker.is_some() {
        return Err("Finish the current file chooser first".into());
    }
    let active = tabs(world, services, &receipt.owner)?
        .into_iter()
        .find(|t| t.active)
        .ok_or("Active tab disappeared")?;
    let filename = format!(
        "{}-{}-profile-{}.dxf",
        active.name,
        intent.sketch_name,
        u64::from(intent.profile_index) + 1
    )
    .replace(['<', '>', ':', '"', '/', '\\', '|', '?', '*'], "_");
    let (dialog, parent) = parented_dialog(
        world,
        rfd::FileDialog::new()
            .add_filter("Manufacturing profile DXF", &["dxf"])
            .set_file_name(filename),
    )?;
    let (send, receive) = mpsc::channel();
    let wake = handle.clone();
    std::thread::Builder::new()
        .name("cad-profile-picker".into())
        .spawn(move || {
            let _parent = parent;
            let mut dialog = dialog;
            if let Some(parent) = active.path.as_ref().and_then(|p| p.parent()) {
                dialog = dialog.set_directory(parent);
            }
            let _ = send.send(dialog.save_file());
            wake.request_redraw();
        })
        .map_err(|e| format!("Cannot open file chooser: {e}"))?;
    world.resource_mut::<Files>().dialog = None;
    world.resource_mut::<Files>().picker = Some(Picker {
        receipt,
        kind: PickerKind::Profile(intent),
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
            .and_then(|v| v.to_str())
            .is_some_and(|v| v.eq_ignore_ascii_case("dxf"))
    {
        return Err("Choose an absolute .dxf file path".into());
    }
    worker::enqueue_document_io(
        world,
        "export_profile_dxf".into(),
        move |services, guard| {
            services.bridge.with_native_document_receipt(
                &services.engine,
                &receipt.owner,
                |revision| {
                    check(&receipt, revision)?;
                    guard.validate_preparation()?;
                    let catalog = catalog(services)?;
                    let sketch = catalog
                        .iter()
                        .find(|sketch| {
                            sketch.feature_id.0 == intent.feature_id
                                && sketch.sketch_name == intent.sketch_name
                        })
                        .ok_or("The selected sketch was removed or replaced")?;
                    let content = limo_cad_export::profile_dxf::write_profile_dxf(
                        sketch,
                        intent.profile_index,
                    )?;
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
                            "feature_id":intent.feature_id,"sketch_name":intent.sketch_name,
                            "profile_index":intent.profile_index,"units":"mm","scale":1}),
                    })
                },
            )
        },
        |_, _, result| Ok(result?.value),
    )
}

#[cfg(test)]
mod tests;
