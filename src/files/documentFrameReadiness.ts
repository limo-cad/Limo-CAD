import { invoke } from '@tauri-apps/api/core';
import { isTauriRuntime } from '../engine';
import { useAppStore } from '../store/appStore';
import { currentUiScale } from '../uiScale';
import type { NativeViewportMetrics } from '../components/viewport/nativeViewportBridge';
import type { ProjectFramingOwner } from './projectFraming';

type FrameMetrics = Pick<NativeViewportMetrics,
  'available' | 'ready' | 'startupError' | 'modelSessionId' | 'geometryRevision'
  | 'renderedFrames' | 'logicalWidth' | 'logicalHeight'>;

export interface DocumentFrameOptions {
  timeoutMs?: number;
  pollIntervalMs?: number;
  timeoutMessage?: string;
}

/** Explicit dependencies keep asynchronous ownership/frame races testable. */
export interface DocumentFrameDependencies {
  native(): boolean;
  current(): ProjectFramingOwner & { activeTab: string };
  metrics(): Promise<FrameMetrics>;
  syncModel(): Promise<number>;
  viewport(): { width: number; height: number } | null;
  scale(): number;
  requestLayout(): void;
  afterPaint(): Promise<void>;
  delay(ms: number): Promise<void>;
  now(): number;
}

export function createDocumentFrameWaiter(deps: DocumentFrameDependencies) {
  return async (owner: ProjectFramingOwner, options: DocumentFrameOptions = {}): Promise<boolean> => {
    const timeoutMs = options.timeoutMs ?? 60_000;
    const intervalMs = options.pollIntervalMs ?? 16;
    if (!Number.isFinite(timeoutMs) || timeoutMs <= 0 || !Number.isFinite(intervalMs) || intervalMs <= 0) {
      throw new Error('Document frame wait requires positive finite timing bounds.');
    }
    const deadline = deps.now() + timeoutMs;
    const owned = () => {
      const current = deps.current();
      return current.activeProjectTabId === owner.activeProjectTabId && current.document === owner.document;
    };
    const applicable = () => owned() && deps.current().activeTab !== 'drawing';
    const timeoutError = () => new Error(options.timeoutMessage
      ?? `The loaded document did not finish rendering within ${Math.ceil(timeoutMs / 1000)} seconds.`);
    const bounded = async <T,>(operation: () => Promise<T>): Promise<T> => {
      const remaining = deadline - deps.now();
      if (remaining <= 0) throw timeoutError();
      let timer: ReturnType<typeof setTimeout> | undefined;
      try {
        return await Promise.race([operation(), new Promise<never>((_, reject) => {
          timer = setTimeout(() => reject(timeoutError()), remaining);
        })]);
      } finally {
        clearTimeout(timer);
      }
    };
    if (!applicable()) return false;
    try {
      // Let React mount the adopted document and consume its framing request.
      await bounded(deps.afterPaint);
      if (!applicable()) return false;
      // Browser rendering has no native presented-frame contract. This branch
      // acknowledges a browser paint only, without issuing native commands.
      if (!deps.native()) return true;
      if (owner.activeProjectTabId === null) throw new Error('The loaded document has no viewport session.');
      const before = await bounded(deps.metrics);
      if (!applicable()) return false;
      if (before.startupError) throw new Error(before.startupError);
      const revision = await bounded(deps.syncModel);
      if (!applicable()) return false;
      deps.requestLayout();
      for (;;) {
        const metrics = await bounded(deps.metrics);
        if (!applicable()) return false;
        if (metrics.startupError) throw new Error(metrics.startupError);
        const viewport = deps.viewport();
        const scale = deps.scale();
        const extentMatches = viewport !== null && Number.isFinite(scale) && scale > 0
          && viewport.width > 0 && viewport.height > 0
          && Math.abs(metrics.logicalWidth - viewport.width * scale) <= 1
          && Math.abs(metrics.logicalHeight - viewport.height * scale) <= 1;
        if (metrics.available && metrics.ready && metrics.modelSessionId === owner.activeProjectTabId
          && metrics.geometryRevision >= revision && metrics.renderedFrames > before.renderedFrames
          && extentMatches) return true;
        if (deadline <= deps.now()) throw timeoutError();
        deps.requestLayout();
        await bounded(() => deps.delay(intervalMs));
      }
    } catch (error) {
      // Superseded work must not report a renderer failure for the new owner.
      if (!applicable()) return false;
      throw error;
    }
  };
}

/** true: current native document presented at the applied viewport extent, or
 * a nonnative browser paint. false: owner superseded or Drawing's 3D viewport
 * is inapplicable. Failures are operation-local and never reset startup. */
export const waitForDocumentFrame = createDocumentFrameWaiter({
  native: isTauriRuntime,
  current: () => useAppStore.getState(),
  metrics: () => invoke<NativeViewportMetrics>('native_viewport_metrics'),
  syncModel: () => invoke<number>('native_viewport_sync_model'),
  viewport: () => document.querySelector('.native-viewport-surface')?.getBoundingClientRect() ?? null,
  scale: currentUiScale,
  requestLayout: () => document.dispatchEvent(new Event('nbcad:native-viewport-layout')),
  afterPaint: () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))),
  delay: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
  now: () => performance.now(),
});
