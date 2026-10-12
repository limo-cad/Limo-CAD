import { create } from 'zustand';

export function usesNativeStartup(nativeRuntime: boolean, mode: string): boolean {
  return nativeRuntime || mode === 'desktop';
}

export type StartupPhase = 'document' | 'renderer' | 'assets' | 'gpu' | 'pipelines' | 'frame' | 'font' | 'initialized' | 'ready' | 'failed';
interface StartupState {
  phase: StartupPhase;
  /** Initial engine getters and project session/tab binding have completed.
   * This does not prove rendering or completion of an optional file open. */
  appPrepared: boolean;
  documentReady: boolean;
  error: string | null;
  failureSource: 'document' | 'renderer';
  delayed: boolean;
  attempt: number;
}

export const useStartupStore = create<StartupState>(() => ({
  phase: 'document', appPrepared: false, documentReady: false, error: null, delayed: false, failureSource: 'renderer', attempt: 0,
}));

/** Application initialization and a file Open can overlap. A file operation
 * never chooses or resets this application status. Native readiness is still
 * required independently before the workspace can become interactive. */
export function startupApplicationPhase(
  appPrepared: boolean, fontsReady: boolean, metrics: { ready: boolean; startupPhase: string },
): StartupPhase {
  if (metrics.ready) return !fontsReady ? 'font' : appPrepared ? 'initialized' : 'document';
  return ['renderer', 'font', 'assets', 'gpu', 'pipelines', 'frame'].includes(metrics.startupPhase)
    ? metrics.startupPhase as StartupPhase : appPrepared ? 'renderer' : 'document';
}

export function startupFailed(error: unknown, failureSource: 'document' | 'renderer' = 'renderer'): void {
  useStartupStore.setState({ phase: 'failed', failureSource, error: error instanceof Error ? error.message : String(error) });
}

export interface RenderReadiness {
  available: boolean;
  ready: boolean;
  renderedFrames: number;
  startupError: string | null;
  modelSessionId: string | null;
  geometryRevision: number;
}

/** Rendering the bootstrap scene does not prove that the current document is
 * visible. Require the backend's successful-frame readiness and tab identity. */
export function workspaceIsReady(
  documentReady: boolean,
  fontsReady: boolean,
  sessionId: string | null,
  metrics: RenderReadiness,
  requiredRevision: number,
): boolean {
  return documentReady && fontsReady && sessionId !== null
    && metrics.available && metrics.ready && metrics.renderedFrames > 0
    && !metrics.startupError && metrics.modelSessionId === sessionId
    && metrics.geometryRevision >= requiredRevision;
}

/** A prepared compact frame cannot prove that the expanded CAD layout is
 * rendered. Match current DOM geometry in native logical units, including zoom.
 * Native readiness independently proves presentation of that applied extent. */
export function workspaceLayoutIsReady(
  metrics: { logicalWidth: number; logicalHeight: number },
  viewport: { width: number; height: number } | null,
  windowSize: { width: number; height: number },
  scale: number,
): boolean {
  if (!viewport || !Number.isFinite(scale) || scale <= 0) return false;
  return windowSize.width * scale >= 1198 && windowSize.height * scale >= 758
    && viewport.width > 0 && viewport.height > 0
    && Math.abs(metrics.logicalWidth - viewport.width * scale) <= 1
    && Math.abs(metrics.logicalHeight - viewport.height * scale) <= 1;
}

declare global {
  interface Window {
    __startupNoteIndex?: number;
    __startupNotes?: Record<string, string[]>;
    __rendererStartPromise?: Promise<void>;
    __startupTiming?: {
      feedbackRequestedMs: number;
      firstFeedbackMs?: number;
      documentReadyMs?: number;
      usableWorkspaceMs?: number;
    };
  }
}
