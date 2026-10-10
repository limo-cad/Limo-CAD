//! Capture this application's rendered window through the same MCP lane.
//! Bevy reads its own render target; no desktop pixels or other windows enter
//! the image. Encoding runs off the presentation thread.

use super::*;
use bevy::render::view::screenshot::{Capturing, Screenshot, ScreenshotCaptured};
use std::{fs::OpenOptions, io::BufWriter, path::PathBuf, time::Instant};
mod paper_diagnostics;

type Outcome = Arc<Mutex<Option<Result<Value, String>>>>;

#[derive(Clone, Copy)]
enum CaptureStage {
    WaitingForImage,
    CheckingDocument,
    EncodingPng,
    WritingFile,
}

#[derive(Resource)]
struct Capture {
    entity: Entity,
    started: Instant,
    polls: u32,
    outcome: Outcome,
    stage: Arc<Mutex<CaptureStage>>,
    diagnostic_capture_path: Option<PathBuf>,
}

fn destination(ui: &Value) -> Result<(PathBuf, bool), String> {
    let path = PathBuf::from(
        ui["path"]
            .as_str()
            .ok_or("Capture requires an absolute PNG path")?,
    );
    if !path.is_absolute()
        || !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
    {
        return Err("Capture requires an absolute PNG path".into());
    }
    let overwrite = match ui.get("overwrite") {
        None => false,
        Some(value) => value.as_bool().ok_or("overwrite must be boolean")?,
    };
    if !path.parent().is_some_and(|parent| parent.is_dir()) {
        return Err("Capture output directory does not exist".into());
    }
    if path.exists() && !overwrite {
        return Err("Capture file exists; choose a new path or set overwrite: true".into());
    }
    Ok((path, overwrite))
}

pub(super) fn begin(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    owner: &DocumentContext,
    ui: &Value,
) -> Result<Value, String> {
    if world.contains_resource::<Capture>() {
        return Err("A window capture is already in progress".into());
    }
    let (path, overwrite) = destination(ui)?;
    let diagnostics = paper_diagnostics::enabled().then(|| {
        workbench::capture_paper_diagnostics(world).unwrap_or(json!({"paper_view_missing":true}))
    });
    let diagnostic_capture_path = diagnostics.as_ref().map(|_| path.clone());
    if !world
        .get_resource::<crate::native_viewport::winit_host::NativeRenderAvailability>()
        .is_some_and(|availability| availability.drawable)
    {
        return Err("Restore the CAD window before capturing its rendered interface".into());
    }
    let owner = owner.clone();
    let services = services.clone();
    let handle = handle.clone();
    let outcome: Outcome = Arc::new(Mutex::new(None));
    let completed = outcome.clone();
    let stage = Arc::new(Mutex::new(CaptureStage::WaitingForImage));
    let progress = stage.clone();
    let entity = world.spawn(Screenshot::primary_window()).observe(
        move |event: On<ScreenshotCaptured>| {
            *progress.lock().unwrap_or_else(|e| e.into_inner()) = CaptureStage::CheckingDocument;
            let progress = progress.clone();
            let image = event.image.clone();
            let path = path.clone();
            let services = services.clone();
            let owner = owner.clone();
            let completed = completed.clone();
            let wake = handle.clone();
            let fallback = completed.clone();
            let fallback_wake = wake.clone();
            let diagnostics = diagnostics.clone();
            if let Err(error) = std::thread::Builder::new()
                .name("cad-window-capture".into())
                .spawn(move || {
                    let result = (|| {
                        services.bridge.with_native_document_owner(&services.engine, &owner, || Ok(()))?;
                        let size = image.texture_descriptor.size;
                        *progress.lock().unwrap_or_else(|e| e.into_inner()) = CaptureStage::EncodingPng;
                        let bytes = crate::native_viewport::screenshot::png_bytes(&image)?;
                        *progress.lock().unwrap_or_else(|e| e.into_inner()) = CaptureStage::WritingFile;
                        let file = OpenOptions::new().write(true).create(overwrite)
                            .truncate(overwrite).create_new(!overwrite).open(&path)
                            .map_err(|e| format!("Capture output: {e}"))?;
                        let mut output = BufWriter::new(file);
                        std::io::Write::write_all(&mut output, &bytes).map_err(|e| format!("Capture output: {e}"))?;
                        std::io::Write::flush(&mut output).map_err(|e| format!("Flush capture: {e}"))?;
                        let mut result=json!({"path":path,"width":size.width,"height":size.height,"source":"bevy_window"});
                        if let Some(snapshot) = diagnostics {
                            let sample=paper_diagnostics::sample(&image,&snapshot)
                                .unwrap_or_else(|error|json!({"error":error,"fitted_paper_white":false}));
                            let diagnostic_path=path.with_extension("paper.json");
                            let file=OpenOptions::new().write(true).create(overwrite).truncate(overwrite)
                                .create_new(!overwrite).open(&diagnostic_path)
                                .map_err(|error|format!("Paper diagnostics output: {error}"))?;
                            let mut output=BufWriter::new(file);
                            serde_json::to_writer_pretty(&mut output,&json!({"stage":"before_screenshot_request",
                                "capture":path,"layout":snapshot,"pixels":sample}))
                                .map_err(|error|format!("Paper diagnostics output: {error}"))?;
                            std::io::Write::flush(&mut output).map_err(|error|format!("Flush paper diagnostics: {error}"))?;
                            result["paper_probe"]=sample;
                            result["paper_diagnostics_path"]=json!(diagnostic_path);
                        }
                        Ok(result)
                    })();
                    *completed.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
                    wake.request_redraw();
                }) {
                    *fallback.lock().unwrap_or_else(|e| e.into_inner()) = Some(Err(format!("Start capture writer: {error}")));
                    fallback_wake.request_redraw();
                }
        },
    ).id();
    world.insert_resource(Capture {
        entity,
        started: Instant::now(),
        polls: 0,
        outcome,
        stage,
        diagnostic_capture_path,
    });
    Ok(json!({"capture_pending":true}))
}

pub(super) fn poll(world: &mut World) -> Option<Result<Value, String>> {
    let polls = {
        let mut capture = world.get_resource_mut::<Capture>()?;
        capture.polls += 1;
        capture.polls
    };
    let capture = world.resource::<Capture>();
    let result = capture
        .outcome
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    let elapsed = capture.started.elapsed();
    if result.is_none() && elapsed < Duration::from_secs(15) {
        return None;
    }
    let entity = capture.entity;
    let stage = match *capture.stage.lock().unwrap_or_else(|e| e.into_inner()) {
        CaptureStage::WaitingForImage if world.get::<Capturing>(entity).is_none() => {
            "waiting for render extraction"
        }
        CaptureStage::WaitingForImage => "waiting for GPU readback",
        CaptureStage::CheckingDocument => "checking document ownership",
        CaptureStage::EncodingPng => "encoding PNG",
        CaptureStage::WritingFile => "writing capture output",
    };
    if let Some(path) = capture.diagnostic_capture_path.as_ref() {
        eprintln!(
            "LIMO_CAD_PAPER_DIAGNOSTICS {}",
            json!({"stage":"after_capture_layout",
            "capture":path,"layout":workbench::capture_paper_diagnostics(world)})
        );
    }
    world.despawn(entity);
    world.remove_resource::<Capture>();
    Some(result.unwrap_or_else(|| {
        Err(format!(
            "Window capture exceeded 15 seconds ({} ms, {polls} polls) while {stage}",
            elapsed.as_millis()
        ))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_timeout_identifies_the_unfinished_stage_and_cleans_up() {
        for (stage, extracted, expected) in [
            (
                CaptureStage::WaitingForImage,
                false,
                "waiting for render extraction",
            ),
            (
                CaptureStage::WaitingForImage,
                true,
                "waiting for GPU readback",
            ),
            (
                CaptureStage::CheckingDocument,
                true,
                "checking document ownership",
            ),
            (CaptureStage::EncodingPng, true, "encoding PNG"),
            (CaptureStage::WritingFile, true, "writing capture output"),
        ] {
            let mut world = World::new();
            let entity = world.spawn(Screenshot::primary_window()).id();
            if extracted {
                world.entity_mut(entity).insert(Capturing);
            }
            world.insert_resource(Capture {
                entity,
                started: Instant::now() - Duration::from_secs(16),
                polls: 0,
                outcome: Arc::new(Mutex::new(None)),
                stage: Arc::new(Mutex::new(stage)),
                diagnostic_capture_path: None,
            });
            let error = poll(&mut world).unwrap().unwrap_err();
            assert!(error.ends_with(expected), "{error}");
            assert!(!world.contains_resource::<Capture>());
            assert!(world.get_entity(entity).is_err());
            assert!(poll(&mut world).is_none());
        }
    }

    #[test]
    fn capture_never_overwrites_without_explicit_permission() {
        let root = std::env::temp_dir().join(format!("cad-capture-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("frame.png");
        assert!(destination(&json!({"path":path})).is_ok());
        std::fs::write(&path, b"existing image").unwrap();
        assert!(destination(&json!({"path":path})).is_err());
        assert!(destination(&json!({"path":path,"overwrite":true})).is_ok());
        for ui in [
            json!({}),
            json!({"path":"relative.png"}),
            json!({"path":root.join("model.limo")}),
            json!({"path":path,"overwrite":"yes"}),
        ] {
            assert!(destination(&ui).is_err());
        }
        assert_eq!(std::fs::read(&path).unwrap(), b"existing image");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
