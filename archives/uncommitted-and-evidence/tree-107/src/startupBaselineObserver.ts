/** Measurement only: preserve the baseline shell and renderer scheduling. */
import { invoke } from '@tauri-apps/api/core';
import { useAppStore } from './store/appStore';
import type { NativeViewportMetrics } from './components/viewport/nativeViewportBridge';
import { measureInitialViewportInteraction } from './startupMeasurement';
export { applyStartupMeasurementLocale, loadStartupMeasurementDocument } from './startupMeasurement';

export async function observeBaselineWorkspace(): Promise<void> {
  const settings = await invoke('startup_measurement_settings');
  if (!settings) return;
  await document.fonts.ready;
  const requiredRevision = await invoke<number>('native_viewport_sync_model');
  const deadline = performance.now() + 120_000;
  while (performance.now() < deadline) {
    const metrics = await invoke<NativeViewportMetrics>('native_viewport_metrics');
    if (metrics.startupError) throw new Error(metrics.startupError);
    const sessionId = useAppStore.getState().activeProjectTabId;
    if (sessionId !== null && metrics.available && metrics.ready && metrics.renderedFrames > 0
      && metrics.modelSessionId === sessionId && metrics.geometryRevision >= requiredRevision) {
      await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
      await invoke('startup_record', { phase: 'workspace', elapsedMs: performance.now() });
      await measureInitialViewportInteraction();
      return;
    }
    await new Promise((resolve) => window.setTimeout(resolve, 50));
  }
  throw new Error('Baseline did not reach the same successful rendered-workspace criterion.');
}
