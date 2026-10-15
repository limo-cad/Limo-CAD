//! Opt-in, local sketch cursor/readout recording for a diagnostic app bundle.
//! The input thread never waits for disk; ordinary launches install no recorder.

use super::Editor;
use crate::native_viewport::{
    self,
    interface_shell::{fields, InterfaceControl, InterfaceLayout, NativeInterfaceHandle},
    winit_host::NativeHostInput,
};
use bevy::{prelude::*, window::WindowEvent};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
    sync::{mpsc, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_BYTES: u64 = 256 * 1024 * 1024;

struct Writer {
    sender: Option<mpsc::SyncSender<Value>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    start: Instant,
    sequence: u64,
    dropped: u64,
}

impl Writer {
    fn open(path: &Path) -> std::io::Result<Self> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        let (sender, receiver) = mpsc::sync_channel(8192);
        let worker = std::thread::Builder::new()
            .name("sketch-diagnostics".into())
            .spawn(move || write_records(file, receiver))?;
        Ok(Self {
            sender: Some(sender),
            worker: Mutex::new(Some(worker)),
            start: Instant::now(),
            sequence: 0,
            dropped: 0,
        })
    }

    fn record(&mut self, kind: &str, data: Value) -> u64 {
        self.sequence += 1;
        let value = json!({
            "sequence": self.sequence,
            "elapsed_us": self.start.elapsed().as_micros() as u64,
            "kind": kind,
            "dropped_records": self.dropped,
            "data": data,
        });
        if self
            .sender
            .as_ref()
            .is_some_and(|sender| sender.try_send(value).is_err())
        {
            self.dropped += 1;
        }
        self.sequence
    }

    fn finish(&mut self) {
        self.sender.take();
        if let Ok(mut worker) = self.worker.lock() {
            if let Some(worker) = worker.take() {
                let _ = worker.join();
            }
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.finish();
    }
}

fn write_records(file: File, receiver: mpsc::Receiver<Value>) {
    let mut file = BufWriter::new(file);
    let mut bytes = 0;
    let mut flushed = Instant::now();
    loop {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(value) => {
                let Ok(mut line) = serde_json::to_vec(&value) else {
                    break;
                };
                line.push(b'\n');
                bytes += line.len() as u64;
                if bytes > MAX_BYTES {
                    let _ = file.write_all(
                        b"{\"kind\":\"recording_limit_reached\",\"limit_bytes\":268435456}\n",
                    );
                    break;
                }
                if file.write_all(&line).is_err() {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if flushed.elapsed() >= Duration::from_millis(100) {
            if file.flush().is_err() {
                break;
            }
            flushed = Instant::now();
        }
    }
    let _ = file.flush();
}

#[derive(Clone)]
struct RawCursor {
    sequence: u64,
    physical: [f64; 2],
    backing_scale: f64,
}

impl RawCursor {
    fn matches(&self, logical: Vec2) -> bool {
        self.physical
            .into_iter()
            .zip(logical.to_array())
            .all(|(raw, converted)| {
                (raw / self.backing_scale - f64::from(converted)).abs() <= 0.002
            })
    }
}

#[derive(Resource)]
struct Trace {
    writer: Writer,
    raw: VecDeque<RawCursor>,
    current_raw: Option<RawCursor>,
    pointer: Value,
    preview: Value,
    previous_sample: Value,
}

pub(crate) fn requested() -> bool {
    std::env::var_os("LIMO_CAD_SKETCH_TRACE_DIR").is_some()
}

pub(crate) fn install(app: &mut App) {
    let Some(directory) = std::env::var_os("LIMO_CAD_SKETCH_TRACE_DIR") else {
        return;
    };
    let unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let path = Path::new(&directory).join(format!(
        "sketch-trace-{unix_ms}-{}.jsonl",
        std::process::id()
    ));
    let writer = match Writer::open(&path) {
        Ok(writer) => writer,
        Err(error) => {
            eprintln!(
                "Cannot start sketch recording at {}: {error}",
                path.display()
            );
            return;
        }
    };
    let mut trace = Trace {
        writer,
        raw: VecDeque::new(),
        current_raw: None,
        pointer: Value::Null,
        preview: Value::Null,
        previous_sample: Value::Null,
    };
    trace.writer.record("session", json!({
        "schema": 1, "unix_ms": unix_ms as u64,
        "build": limo_cad_build_info::build_info(),
        "units": {"raw_window_physical_px": "physical window pixels, f64, top left origin",
            "window_ui": "application UI coordinates, top left origin",
            "viewport_ui": "viewport local UI coordinates, top left origin",
            "sketch_mm": "millimetres on active sketch plane"},
        "scope": "This app window's pointer, camera, sketch preview and drawing dimension fields only",
        "maximum_bytes": MAX_BYTES,
    }));
    app.insert_resource(trace)
        .add_systems(
            PostUpdate,
            sample
                .after(InterfaceLayout)
                .after(bevy::ui::widget::update_editable_text_layout),
        )
        .add_systems(Last, stop_on_exit);
}

/// Preserve the OS's f64 values before Bevy's f32/logical/UI conversion.
pub(crate) fn raw_events(
    world: &mut World,
    events: &[winit::event::WindowEvent],
    backing_scale: f64,
) {
    let Some(mut trace) = world.get_resource_mut::<Trace>() else {
        return;
    };
    for event in events {
        match event {
            winit::event::WindowEvent::CursorMoved { position, .. } => {
                let physical = [position.x, position.y];
                let sequence = trace.writer.record(
                    "raw_cursor",
                    json!({
                        "window_physical_px": physical, "backing_scale": backing_scale,
                    }),
                );
                trace.raw.push_back(RawCursor {
                    sequence,
                    physical,
                    backing_scale,
                });
                // Bound correlation memory even if the controller retires an input batch.
                if trace.raw.len() > 8192 {
                    trace.raw.pop_front();
                }
            }
            winit::event::WindowEvent::MouseWheel { delta, phase, .. } => {
                trace.writer.record(
                    "raw_wheel",
                    json!({"delta": format!("{delta:?}"), "phase": format!("{phase:?}")}),
                );
            }
            winit::event::WindowEvent::PinchGesture { delta, phase, .. } => {
                trace.writer.record(
                    "raw_pinch",
                    json!({"delta": delta, "phase": format!("{phase:?}")}),
                );
            }
            winit::event::WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                trace.writer.record(
                    "raw_backing_scale_changed",
                    json!({"scale_factor": scale_factor}),
                );
            }
            winit::event::WindowEvent::Resized(size) => {
                trace.writer.record(
                    "raw_resize",
                    json!({"physical_px": [size.width, size.height]}),
                );
            }
            _ => {}
        }
    }
}

/// Called in controller order, including inputs subsequently consumed by navigation/UI.
pub(crate) fn input(world: &mut World, handle: &NativeInterfaceHandle, input: &NativeHostInput) {
    if !world.contains_resource::<Trace>() {
        return;
    }
    let event = match &input.event {
        WindowEvent::CursorMoved(_) => "cursor_moved".to_owned(),
        WindowEvent::MouseButtonInput(event) => {
            format!("mouse_{:?}_{:?}", event.button, event.state)
        }
        WindowEvent::MouseWheel(event) => format!("wheel_{:?}_{}_{}", event.unit, event.x, event.y),
        WindowEvent::CursorLeft(_) => "cursor_left".to_owned(),
        _ => return,
    };
    let bounds = handle
        .presented_pointer_context()
        .and_then(|(_, _, bounds)| bounds);
    let viewport = input
        .cursor
        .zip(bounds)
        .map(|(point, bounds)| [f64::from(point.x) - bounds.x, f64::from(point.y) - bounds.y]);
    let (_, camera, _, size) = native_viewport::interface_view(world);
    let mut trace = world.resource_mut::<Trace>();
    let raw_matched = if matches!(input.event, WindowEvent::CursorMoved(_)) {
        let index = input.cursor.and_then(|point| {
            trace
                .raw
                .iter()
                .position(|raw| raw.matches(point * input.ui_scale))
        });
        trace.current_raw = index.and_then(|index| trace.raw.drain(..=index).next_back());
        trace.current_raw.is_some()
    } else {
        false
    };
    let raw = trace.current_raw.as_ref().map(|raw| {
        let physical_per_ui = raw.backing_scale * f64::from(input.ui_scale);
        json!({
            "raw_sequence": raw.sequence, "window_physical_px": raw.physical,
            "backing_scale": raw.backing_scale,
            "viewport_physical_px": bounds.map(|bounds| [raw.physical[0] - bounds.x * physical_per_ui, raw.physical[1] - bounds.y * physical_per_ui]),
            "viewport_ui_from_raw": bounds.map(|bounds| [raw.physical[0] / physical_per_ui - bounds.x, raw.physical[1] / physical_per_ui - bounds.y]),
        })
    });
    let mut pointer = json!({
        "event": event, "raw": raw, "raw_move_matched": raw_matched,
        "window_ui": input.cursor.map(|point| point.to_array()),
        "window_logical_px": input.cursor.map(|point| (point * input.ui_scale).to_array()),
        "viewport_ui": viewport, "viewport_bounds_ui": bounds,
        "ui_scale": input.ui_scale, "viewport_size_ui": size, "camera": camera,
        "ui_owns_pointer": input.cursor.map(|point| handle.owns_pointer(point.as_dvec2().to_array())),
        "ui_blocks_motion": input.cursor.map(|point| handle.blocks_pointer_motion(point.as_dvec2().to_array())),
        "pointer_captured": handle.has_capture(),
        "modifiers": {"ctrl": input.modifiers.ctrl, "shift": input.modifiers.shift, "alt": input.modifiers.alt, "meta": input.modifiers.meta},
    });
    let sequence = trace.writer.record("input", pointer.clone());
    pointer["input_sequence"] = json!(sequence);
    trace.pointer = pointer;
}

pub(crate) fn routed(world: &mut World, input: &NativeHostInput) {
    let Some(mut trace) = world.get_resource_mut::<Trace>() else {
        return;
    };
    if matches!(
        input.event,
        WindowEvent::CursorMoved(_) | WindowEvent::MouseButtonInput(_)
    ) {
        let sequence = trace.pointer["input_sequence"].clone();
        trace.writer.record("input_routed", json!({"input_sequence": sequence, "consumed": input.consumed, "action_count": input.actions.len()}));
    }
}

pub(super) fn preview(
    world: &mut World,
    editor: &Editor,
    snapped: limo_cad_sketch::Vec2,
    snap_kind: Option<native_viewport::ViewportSnapKind>,
) {
    let Some(mut trace) = world.get_resource_mut::<Trace>() else {
        return;
    };
    let preview = json!({
        "input_sequence": trace.pointer["input_sequence"],
        "tool": editor.draft.tool.map(|tool| tool.label()), "generation": editor.draft.generation,
        "raw_sketch_mm": editor.draft.cursor, "acquired_sketch_mm": snapped,
        "resolved_preview_mm": editor.draft.resolved_preview,
        "snap_context": editor.draft.snap_context, "snap_kind": format!("{snap_kind:?}"),
    });
    trace.writer.record("preview", preview.clone());
    trace.preview = preview;
}

/// After field synchronization/layout, pair the current pointer with real visible text.
fn sample(world: &mut World) {
    let mut query = world.query::<(Entity, &fields::DrawingDimension, &InterfaceControl)>();
    let mut dimensions: Vec<_> = query
        .iter(world)
        .map(|(entity, dimension, control)| {
            json!({
                "generation": dimension.generation, "index": dimension.index,
                "label": control.label, "field": fields::diagnostic_value(world, entity),
            })
        })
        .collect();
    dimensions.sort_by_key(|value| value["index"].as_u64());
    let (_, camera, _, size) = native_viewport::interface_view(world);
    let draft = world.get_resource::<Editor>().map(|editor| json!({
        "tool": editor.draft.tool.map(|tool| tool.label()), "generation": editor.draft.generation,
        "anchors_mm": editor.draft.points, "raw_sketch_mm": editor.draft.cursor,
        "resolved_preview_mm": editor.draft.resolved_preview, "snap_context": editor.draft.snap_context,
    }));
    let mut trace = world.resource_mut::<Trace>();
    let value = json!({"pointer": trace.pointer, "preview": trace.preview,
        "dimensions": dimensions, "draft": draft, "camera": camera, "viewport_size_ui": size});
    if value != trace.previous_sample {
        trace.writer.record("display_sample", value.clone());
        trace.previous_sample = value;
    }
}

fn stop_on_exit(mut exits: MessageReader<AppExit>, mut trace: ResMut<Trace>) {
    if exits.read().next().is_some() {
        trace.writer.record("session_end", json!({}));
        trace.writer.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_sample_pairs_last_processed_raw_move_with_actual_field_text() {
        let directory =
            std::env::temp_dir().join(format!("sketch-trace-pair-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("trace.jsonl");
        let mut app = native_viewport::interface_scene_fixture();
        app.insert_resource(Trace {
            writer: Writer::open(&path).unwrap(),
            raw: VecDeque::new(),
            current_raw: None,
            pointer: Value::Null,
            preview: Value::Null,
            previous_sample: Value::Null,
        });
        let window = app.world_mut().spawn_empty().id();
        let handle = NativeInterfaceHandle::new(|| {});
        let positions = [
            [501.123456789, 782.987654321],
            [1102.7654321, 812.123456789],
        ];
        let raw: Vec<_> = positions
            .iter()
            .map(|point| winit::event::WindowEvent::CursorMoved {
                device_id: winit::event::DeviceId::dummy(),
                position: winit::dpi::PhysicalPosition::new(point[0], point[1]),
            })
            .collect();
        raw_events(app.world_mut(), &raw, 2.);
        // The OS batches both raw events before either is reduced. The first
        // sample must retain its own physical position rather than the newest.
        for physical in positions {
            let cursor = Vec2::new((physical[0] / 2.) as f32, (physical[1] / 2.) as f32) / 1.25;
            input(
                app.world_mut(),
                &handle,
                &NativeHostInput {
                    ui_scale: 1.25,
                    context: None,
                    cursor: Some(cursor),
                    modifiers: default(),
                    event: WindowEvent::CursorMoved(bevy::window::CursorMoved {
                        window,
                        position: cursor,
                        delta: None,
                    }),
                    consumed: false,
                    actions: Vec::new(),
                },
            );
        }
        let mut control = InterfaceControl::button("viewport", "Drawing width");
        control.field = limo_cad_interface::Field::Text {
            value: "12".into(),
            read_only: false,
            selection: None,
        };
        app.world_mut().spawn((
            control,
            fields::DrawingDimension {
                generation: 42,
                index: 0,
            },
            bevy::text::EditableText::new("4.125"),
        ));
        sample(app.world_mut());
        // Unchanged frames need no duplicate row, including when input stalls.
        sample(app.world_mut());
        app.world_mut().resource_mut::<Trace>().writer.finish();
        let records: Vec<Value> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let inputs: Vec<_> = records
            .iter()
            .filter(|record| record["kind"] == "input")
            .collect();
        for (record, physical) in inputs.iter().zip(positions) {
            assert_eq!(record["data"]["raw"]["window_physical_px"], json!(physical));
            assert_eq!(record["data"]["raw_move_matched"], true);
        }
        let samples: Vec<_> = records
            .iter()
            .filter(|record| record["kind"] == "display_sample")
            .collect();
        assert_eq!(samples.len(), 1);
        let sample = &samples[0]["data"];
        assert_eq!(sample["pointer"]["input_sequence"], inputs[1]["sequence"]);
        assert_eq!(
            sample["pointer"]["raw"]["window_physical_px"],
            json!(positions[1])
        );
        assert_eq!(sample["dimensions"][0]["field"]["display_text"], "4.125");
        assert_eq!(sample["dimensions"][0]["field"]["control_text"], "12");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn raw_cursor_correlation_preserves_physical_precision_across_ui_scaling() {
        let raw = RawCursor {
            sequence: 7,
            physical: [501.123456789, 782.987654321],
            backing_scale: 2.,
        };
        let ui_scale = 1.25;
        let ui = Vec2::new((raw.physical[0] / 2.) as f32, (raw.physical[1] / 2.) as f32) / ui_scale;
        assert!(raw.matches(ui * ui_scale));
        assert!(!raw.matches(ui * ui_scale + Vec2::X));
        assert_eq!(raw.physical, [501.123456789, 782.987654321]);
    }

    #[test]
    fn recorder_flushes_ordered_paired_samples_on_shutdown() {
        let directory =
            std::env::temp_dir().join(format!("sketch-trace-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("trace.jsonl");
        let mut writer = Writer::open(&path).unwrap();
        let sequence = writer.record(
            "raw_cursor",
            json!({"window_physical_px": [501.123456789, 782.987654321]}),
        );
        writer.record(
            "display_sample",
            json!({"raw_sequence": sequence, "display_text": "14.25"}),
        );
        writer.finish();
        let records: Vec<Value> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records.len(), 2);
        assert_eq!(
            records[0]["data"]["window_physical_px"][0],
            json!(501.123456789)
        );
        assert_eq!(records[1]["data"]["raw_sequence"], records[0]["sequence"]);
        assert_eq!(records[1]["data"]["display_text"], "14.25");
        assert!(records[1]["elapsed_us"].as_u64() >= records[0]["elapsed_us"].as_u64());
        assert_eq!(records[1]["dropped_records"], 0);
        assert!(
            Writer::open(&path).is_err(),
            "Never overwrite a prior recording"
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
