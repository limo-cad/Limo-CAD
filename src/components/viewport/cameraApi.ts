/**
 * Camera API shared between the Viewport and its navigation overlays.
 * View changes are animated (~250 ms) and every interaction
 * respects free-orbit: the camera is never locked, including inside an
 * active sketch.
 */
export interface CameraSnapshot {
  position: [number, number, number];
  target: [number, number, number];
  up: [number, number, number];
}

export interface ScreenPoint {
  x: number;
  y: number;
}

export interface SixDofMotion {
  /** Normalized cap translation: right, forward, up. */
  translation: [number, number, number];
  /** Normalized cap rotation about right, forward, and up. */
  rotation: [number, number, number];
  /** Integration interval supplied by the device adapter. */
  deltaSeconds: number;
}

export interface CameraFocus {
  target?: 'active_sketch';
  body_id?: number;
  component_id?: number;
}

export interface ViewportCameraApi {
  pointer(action: import('../../uiPointer').UiGesture, point: [number, number], shift?: boolean, to?: [number, number]): Promise<void>;
  bounds(): { x: number; y: number; width: number; height: number };
  /** Current camera pose (copies; safe to mutate). */
  getSnapshot(): CameraSnapshot;
  /** True until the renderer has completed the current camera animation. */
  isAnimating(): boolean;
  /** Native wake events advance navigation even while WebView RAF is suspended. */
  advanceAnimation(): void;
  /** Animated snap to look at the target from a world direction. */
  snapToDirection(direction: [number, number, number], durationMs?: number): void;
  /** Animated return to the default axonometric home view. */
  home(durationMs?: number): void;
  /** Animated frame of the currently visible model/sketch geometry. */
  fit(durationMs?: number): void;
  /** Frame actual visible geometry for a scripted explanation, without editing it. */
  focus(target: CameraFocus, durationMs?: number, direction?: [number, number, number] | 'isometric'): void;
  /** Immediate free-orbit delta from navigation input, in pixels. */
  orbitBy(dxPx: number, dyPx: number): void;
  /** Immediate six-degree-of-freedom navigation from a 3D mouse. */
  navigateSixDof(motion: SixDofMotion): void;
  /**
   * Camera/property adapter consumed by the browser Navigation Library
   * bridge. Kept separate from raw six-axis motion so the desktop driver can
   * provide its normal CAD navigation model when it owns the HID interface.
   */
  getSixDofDriverView(): import('../../input/threeDConnexionBridge').SixDofDriverView;
  /** Animated snap to look normal at the active sketch plane. */
  lookAtActivePlane(): void;
  /** Project one Z-up world point into application-window pixels. */
  worldToScreen(point: [number, number, number]): ScreenPoint | null;
}

let sessionCamera: ViewportCameraApi | null = null;
const sessionCameraListeners = new Set<() => void>();
/** Notify mount, unmount and actual animation completion; no polling delay. */
export function notifySessionCameraChanged(): void {
  for (const changed of [...sessionCameraListeners]) changed();
}
export function subscribeSessionCamera(changed: () => void): () => void {
  sessionCameraListeners.add(changed);
  return () => { sessionCameraListeners.delete(changed); };
}
export function registerSessionCamera(api: ViewportCameraApi): void {
  sessionCamera = api;
  notifySessionCameraChanged();
}
export function unregisterSessionCamera(api: ViewportCameraApi): void {
  if (sessionCamera === api) {
    sessionCamera = null;
    notifySessionCameraChanged();
  }
}
export function getSessionCamera(): ViewportCameraApi | null { return sessionCamera; }

/** easeInOutCubic — used by all camera animations. */
export function easeInOutCubic(t: number): number {
  return t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;
}
