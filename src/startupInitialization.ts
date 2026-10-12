import { getEngine } from './engine';
import { invoke } from '@tauri-apps/api/core';
import { initializeProjectTabs, recordActiveProjectOpen } from './files/projectTabs';
import { useAppStore } from './store/appStore';
import { startupFailed, useStartupStore } from './startup';
import { applyStartupMeasurementLocale, loadStartupMeasurementDocument } from './startupMeasurement';

let initializationInFlight: Promise<void> | null = null;

/** Keep the frontend's existing tab UUID through recovery. Reloading the
 * WebView loses that UUID while its native project session remains bound. */
export function initializeStartupDocument(empty = false): Promise<void> {
  if (initializationInFlight) return initializationInFlight;
  useStartupStore.setState((state) => ({
    attempt: state.attempt + 1, phase: 'document', appPrepared: false, documentReady: false,
    error: null, delayed: false, failureSource: 'document',
  }));
  initializationInFlight = (async () => {
    const started = performance.now();
    await applyStartupMeasurementLocale();
    const appStarted = performance.now();
    if (empty) {
      const update = await (await getEngine()).newProject();
      useAppStore.getState().loadProjectState(update, [], [], null);
    }
    await useAppStore.getState().loadDocument();
    await initializeProjectTabs();
    useStartupStore.setState({ appPrepared: true });
    const diagnostics = window.__startupMeasurementSettingsPromise;
    if (diagnostics) {
      const elapsedMs = performance.now();
      const durationMs = elapsedMs - appStarted;
      void diagnostics.then((settings) => settings
        ? invoke('startup_record', { phase: 'appDocumentSetup', elapsedMs, durationMs }) : undefined)
        .catch((error) => console.warn('Application document setup measurement failed', error));
    }
    if (empty) {
      // A blank recovered document must never save over the failed file.
      await recordActiveProjectOpen(await (await getEngine()).exportProjectModel(), null);
    } else {
      await loadStartupMeasurementDocument();
    }
    useStartupStore.setState((state) => ({
      documentReady: true,
      ...(!state.error ? { phase: 'renderer' as const, failureSource: 'renderer' as const } : {}),
    }));
    if (window.__startupTiming) window.__startupTiming.documentReadyMs = performance.now();
    if (window.__startupTiming) void invoke('startup_record', {
      phase: 'document', elapsedMs: performance.now(), durationMs: performance.now() - started,
    }).catch((error) => console.warn('Document preparation measurement failed', error));
  })().catch((error) => {
    if (!useStartupStore.getState().error) startupFailed(error, 'document');
    throw error;
  }).finally(() => { initializationInFlight = null; });
  return initializationInFlight;
}
