const fs = require('fs');
const path = require('path');
const root = 'C:/Users/jeffg/.t3/worktrees/noBS-CAD/t3code-028ff4c7';
const baseline = 'D:/limo-startup-baseline-e4015de';
function read(base, name) { return fs.readFileSync(path.join(base, name), 'utf8'); }
function write(name, value) { fs.writeFileSync(path.join(baseline, name), value); }
function replaceOnce(text, from, to) { if (!text.includes(from)) throw new Error('Missing backport anchor '+from); return text.replace(from, to); }
write('src-tauri/src/startup_metrics.rs', read(root, 'src-tauri/src/startup_metrics.rs'));
let main = read(baseline, 'src-tauri/src/main.rs');
main = replaceOnce(main, 'fn main() -> std::process::ExitCode {', 'fn main() -> std::process::ExitCode {\n    nbcad_lib::startup_metrics::begin();');
write('src-tauri/src/main.rs', main);
let lib = read(baseline, 'src-tauri/src/lib.rs');
lib = replaceOnce(lib, 'mod native_menu;', 'mod native_menu;\npub mod startup_metrics;');
lib = replaceOnce(lib, '            ping,', '            ping,\n            startup_metrics::startup_record,\n            startup_metrics::startup_measurement_settings,');
write('src-tauri/src/lib.rs', lib);
// Observe exactly the same asset/pipeline/frame criteria. Do not backport the
// candidate's asynchronous initialization, deferred start, or frame scheduling.
write('src-tauri/src/native_viewport/loading.rs', read(root, 'src-tauri/src/native_viewport/loading.rs'));
let platform = read(baseline, 'src-tauri/src/native_viewport/platform.rs');
const currentPlatform = read(root, 'src-tauri/src/native_viewport/platform.rs');
platform = replaceOnce(platform, '#[path = "script_preview.rs"]', '#[path = "loading.rs"]\nmod loading;\n\n#[path = "script_preview.rs"]');
const fields = '\n    renderer_initialized: bool,\n    startup_phase: String,\n    model_session_id: String,\n    geometry_revision: u64,\n    ready_pipeline_count: usize,\n    uploaded_mesh_count: usize,';
platform = replaceOnce(platform, 'struct MetricsState {\r\n    ready: bool,', 'struct MetricsState {\r\n    ready: bool,'+fields);
platform = replaceOnce(platform, '    app.finish();\r\n    app.cleanup();', '    app.finish();\r\n    app.cleanup();\n    loading::install(&mut app);');
const oldFramesStart = platform.indexOf('fn render_frames(');
const oldFramesEnd = platform.indexOf('\r\nfn pick_occt_scene(', oldFramesStart);
const newFramesStart = currentPlatform.indexOf('fn render_frames(');
const newFramesEnd = currentPlatform.indexOf('\r\nfn startup_continuation(', newFramesStart);
platform = platform.slice(0,oldFramesStart)+currentPlatform.slice(newFramesStart,newFramesEnd)+platform.slice(oldFramesEnd);
platform = replaceOnce(platform, '            let physical_width = (logical_width * scale_factor)', '            if logical_width > 1.0 && logical_height > 1.0 { loading::layout_received(&mut runtime.app); }\n            let physical_width = (logical_width * scale_factor)');
platform = replaceOnce(platform, '            ready: metrics.ready,', '            ready: metrics.ready,\n            renderer_initialized: metrics.renderer_initialized,\n            startup_phase: metrics.startup_phase.clone(),\n            model_session_id: metrics.model_session_id.clone(),\n            geometry_revision: metrics.geometry_revision,\n            ready_pipeline_count: metrics.ready_pipeline_count,\n            uploaded_mesh_count: metrics.uploaded_mesh_count,');
// The original install sets ready eagerly. Mark initialization separately and
// leave readiness to the observer after the original render scheduling.
platform = replaceOnce(platform, '                    current.ready = true;', '                    current.renderer_initialized = true;');
write('src-tauri/src/native_viewport/platform.rs', platform);
let mod = read(baseline, 'src-tauri/src/native_viewport/mod.rs');
mod = replaceOnce(mod, '    pub ready: bool,', '    pub ready: bool,'+fields.replaceAll('    ', '    pub '));
write('src-tauri/src/native_viewport/mod.rs', mod);
console.log('Backported telemetry and readiness observer; startup scheduling and compiler profiles unchanged.');
