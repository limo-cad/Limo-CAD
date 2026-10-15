/** Opt-in launch observer shared with the unchanged baseline. Normal launches
 * never open a fixture, override language, or synthesize CAD interaction. */
import { invoke } from '@tauri-apps/api/core';
import { getEngine, isTauriRuntime } from './engine';
import { isSupportedLocale } from './i18n/locales';
import { useLocaleStore } from './i18n/localeStore';
import { setActiveLocale } from './i18n';
import { openProject } from './files/projectFiles';
import { useAppStore } from './store/appStore';
import { currentNativeViewportCamera, type NativeViewportMetrics } from './components/viewport/nativeViewportBridge';
import { projectModelDifference } from './engine/projectModelComparison';

export interface StartupMeasurementSettings {
  locale: string | null;
  documentPath: string | null;
}
declare global {
  interface Window {
    __startupMeasurementSettingsPromise?: Promise<StartupMeasurementSettings | null>;
  }
}
let settingsPromise: Promise<StartupMeasurementSettings | null> | null = null;

export function startupMeasurementSettings(): Promise<StartupMeasurementSettings | null> {
  if (!isTauriRuntime()) return Promise.resolve(null);
  settingsPromise ??= window.__startupMeasurementSettingsPromise
    ?? invoke<StartupMeasurementSettings | null>('startup_measurement_settings');
  return settingsPromise;
}

export async function applyStartupMeasurementLocale(): Promise<void> {
  const settings = await startupMeasurementSettings();
  if (settings?.locale && isSupportedLocale(settings.locale)) {
    useLocaleStore.setState({ locale: settings.locale });
    setActiveLocale(settings.locale);
    document.documentElement.lang = settings.locale;
  }
}

export async function loadStartupMeasurementDocument(): Promise<void> {
  const settings = await startupMeasurementSettings();
  if (settings?.documentPath) {
    const opened = await openProject({ filePath: settings.documentPath, discardChanges: true });
    if (!opened) throw new Error(`Could not open startup measurement document: ${settings.documentPath}`);
  }
}


export async function measureInitialViewportInteraction(): Promise<void> {
  try {
    await runInitialViewportInteraction();
  } catch (error) {
    await invoke('startup_record', {
      phase: 'error', elapsedMs: performance.now(),
      detail: error instanceof Error ? error.message : String(error),
    }).catch(() => undefined);
    throw error;
  }
}

async function runInitialViewportInteraction(): Promise<void> {
  if (!(await startupMeasurementSettings())) return;
  const camera = currentNativeViewportCamera();
  if (!camera) throw new Error('The startup measurement requires an initialized viewport camera.');
  const before = await invoke<NativeViewportMetrics>('native_viewport_metrics');
  const changed = { ...camera, position: [...camera.position] as [number, number, number] };
  changed.position[0] += 0.5;
  const started = performance.now();
  try {
    await invoke('native_viewport_set_camera', { camera: changed });
    let after: NativeViewportMetrics;
    do {
      after = await invoke<NativeViewportMetrics>('native_viewport_metrics');
      if (after.startupError) throw new Error(after.startupError);
      if (performance.now() - started > 10_000) throw new Error('Initial viewport interaction did not render within 10 seconds.');
      if (after.renderedFrames <= before.renderedFrames) await new Promise((resolve) => window.setTimeout(resolve, 10));
    } while (after.renderedFrames <= before.renderedFrames);
    await invoke('native_viewport_pick', {
      x: after.logicalWidth / 2,
      y: after.logicalHeight / 2,
      camera: changed,
      logicalWidth: after.logicalWidth,
      logicalHeight: after.logicalHeight,
      purpose: 'geometry',
    });
    const elapsedMs = performance.now();
    await invoke('startup_record', { phase: 'interaction', elapsedMs, durationMs: elapsedMs - started });
  } finally {
    await invoke('native_viewport_set_camera', { camera });
  }
  const first = await measureCadEdit();
  const repeated = await measureCadEdit();
  await invoke('startup_record', {
    phase: 'cadInteraction', elapsedMs: first.elapsedMs, durationMs: first.durationMs,
    detail: JSON.stringify({ ...first.timings, repeatedCadInteractionMs: repeated.durationMs,
      repeatedTimings: repeated.timings }),
  });
}

/** Exercise the real solver, first sketch frame, OCCT extrusion, and first
 * opaque solid frame. Restore the model exactly; never save the fixture file. */
async function measureCadEdit() {
  const engine = await getEngine();
  const model = await engine.exportProjectModel();
  const state = useAppStore.getState();
  const previous = { mode: state.mode, activeTab: state.activeTab, activeSketch: state.activeSketch, dirty: state.dirty };
  const before = await invoke<NativeViewportMetrics>('native_viewport_metrics');
  const started = performance.now();
  let measurement: { elapsedMs: number; durationMs: number; timings: {
    sketchCommandsMs: number; sketchFrameMs: number; solidCommandsMs: number; solidFrameMs: number;
  }} | null = null;
  try {
    const sketch = await engine.beginSketch({ type: 'origin_plane', plane: 'xy' });
    useAppStore.setState({ activeSketch: sketch, mode: 'sketch', activeTab: 'sketch' });
    const result = await engine.addRectangle({
      mode: 'two_point', p1: { x: 0, y: 0 }, p2: { x: 10, y: 8 }, ctrl_held: false,
    });
    useAppStore.getState().setActiveSketch(result.sketch);
    const afterRectangle = performance.now();
    const revision = await invoke<number>('native_viewport_sync_model');
    let rendered: NativeViewportMetrics;
    do {
      rendered = await invoke<NativeViewportMetrics>('native_viewport_metrics');
      if (rendered.startupError) throw new Error(rendered.startupError);
      if (performance.now() - started > 10_000) throw new Error('The first CAD edit did not render within 10 seconds.');
      if (!rendered.ready || rendered.geometryRevision < revision || rendered.renderedFrames <= before.renderedFrames) {
        await new Promise((resolve) => window.setTimeout(resolve, 10));
      }
    } while (!rendered.ready || rendered.geometryRevision < revision || rendered.renderedFrames <= before.renderedFrames);
    const afterSketchFrame = performance.now();
    await engine.endSketch();
    const profiles = (await engine.profileCatalog()).find((item) => item.sketch_name === sketch.name);
    const profile = profiles?.profiles.find((item) => item.nesting_depth === 0);
    if (!profile) throw new Error('The diagnostic rectangle did not produce a bounded extrusion profile.');
    const update = await engine.extrude({
      sketch_name: sketch.name, profile_indices: [profile.index], operation: 'new_body',
      extent: { type: 'distance', distance: 5 }, taper_angle_deg: 0, flip: false, target_body_ids: [],
    });
    const afterExtrude = performance.now();
    useAppStore.getState().applySolidUpdate(update);
    useAppStore.setState({ activeSketch: null, mode: 'solid', activeTab: 'solid' });
    const solidRevision = await invoke<number>('native_viewport_sync_model');
    do {
      rendered = await invoke<NativeViewportMetrics>('native_viewport_metrics');
      if (rendered.startupError) throw new Error(rendered.startupError);
      if (performance.now() - started > 10_000) throw new Error('The first solid CAD operation did not render within 10 seconds.');
      if (!rendered.ready || rendered.geometryRevision < solidRevision || rendered.bodyCount <= before.bodyCount) {
        await new Promise((resolve) => window.setTimeout(resolve, 10));
      }
    } while (!rendered.ready || rendered.geometryRevision < solidRevision || rendered.bodyCount <= before.bodyCount);
    const elapsedMs = performance.now();
    measurement = { elapsedMs, durationMs: elapsedMs - started, timings: {
      sketchCommandsMs: afterRectangle - started,
      sketchFrameMs: afterSketchFrame - afterRectangle,
      solidCommandsMs: afterExtrude - afterSketchFrame,
      solidFrameMs: elapsedMs - afterExtrude,
    }};
  } finally {
    await engine.loadProjectModel(model);
    await useAppStore.getState().loadDocument();
    useAppStore.setState(previous);
    const restoredRevision = await invoke<number>('native_viewport_sync_model');
    const restorationStarted = performance.now();
    // A second trial must begin with the restored frame, rather than stale
    // body counts from its predecessor's temporary extrusion.
    for (;;) {
      const restored = await invoke<NativeViewportMetrics>('native_viewport_metrics');
      if (restored.startupError) throw new Error(restored.startupError);
      if (restored.ready && restored.geometryRevision >= restoredRevision) break;
      if (performance.now() - restorationStarted > 10_000) throw new Error('The restored CAD document did not render within 10 seconds.');
      await new Promise((resolve) => window.setTimeout(resolve, 10));
    }
  }
  const difference = projectModelDifference(model, await engine.exportProjectModel());
  if (difference) throw new Error(`The diagnostic CAD interaction did not restore its original model: ${difference}`);
  if (!measurement) throw new Error('The diagnostic CAD interaction did not complete.');
  return measurement;
}
