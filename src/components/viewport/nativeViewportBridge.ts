import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useAppStore } from '../../store/appStore';
import type {
  BodyPoseDto,
  InstanceBodyPoseDto,
  Point3Dto,
  ProfileRefDto,
  SketchPointRefDto,
} from '../../engine/types';
import type { ConstraintIconKind } from '../../sketch/constraintIcons';
import type { MoveCopyCommandPreview } from '../../store/appStore';
import type { BrowserNode } from '../../types/document';
import { constraintReferencedEntityIds } from '../../sketch/constraintRefs';
import { activeEdgePickMode, pickAccepts } from '../../modeling/viewportPicker';
import {
  collectAppViewportPickFeedback,
  type FinishedSketchEntityPickRef,
} from '../../modeling/viewportPickFeedback';
import { revolveProfileAcceptsAxis } from '../../lib/revolveAxis';
import { presentation as playbackPresentation } from '../../operationPlayback';

export interface NativeCameraState {
  position: [number, number, number];
  target: [number, number, number];
  up: [number, number, number];
  verticalFovDegrees: number;
}

export interface NativeViewportMetrics {
  available: boolean;
  ready: boolean;
  startupError: string | null;
  backend: string;
  logicalWidth: number;
  logicalHeight: number;
  scaleFactor: number;
  physicalWidth: number;
  physicalHeight: number;
  renderedFrames: number;
  wakeups: number;
  averageFrameMs: number;
  lastPointerLatencyMs: number;
  bodyCount: number;
  triangleCount: number;
}

export interface NativeViewportPick {
  bodyId: number;
  occurrenceId: number | null;
  faceId: number;
  edgeId: number | null;
  point: [number, number, number];
  distance: number;
  connectorKind: 'planar_face' | 'cylindrical_face' | 'virtual_circular_face' | 'circular_edge' | null;
  /** Connector frame remains body-local even when the displayed body is posed. */
  connectorOrigin: [number, number, number] | null;
  connectorPrimaryAxis: [number, number, number] | null;
  connectorSecondaryAxis: [number, number, number] | null;
  connectorRadius: number | null;
}

export interface NativeRect {
  x: number;
  y: number;
  width: number;
  height: number;
  /** CSS border radius used by the native compositor for this DOM island. */
  cornerRadius?: number;
}

interface NativePalette {
  background: [number, number, number];
  panel: [number, number, number];
  header: [number, number, number];
  uiEdge: [number, number, number];
  ink: [number, number, number];
  mute: [number, number, number];
  accent: [number, number, number];
  gridFine: [number, number, number];
  gridMajor: [number, number, number];
  body: [number, number, number];
  bodySelected: [number, number, number];
  bodyTool: [number, number, number];
  bodySelectedEdge: [number, number, number];
  faceHover: [number, number, number];
  faceSelected: [number, number, number];
  edge: [number, number, number];
  edgeHover: [number, number, number];
  edgeSelected: [number, number, number];
  pickHalo: [number, number, number];
  originPlaneXy: [number, number, number];
  originPlaneXz: [number, number, number];
  originPlaneYz: [number, number, number];
  activeSketch: [number, number, number];
  definedSketch: [number, number, number];
  hover: [number, number, number];
  selection: [number, number, number];
  constraintRelated: [number, number, number];
  finishedSketch: [number, number, number];
  finishedSketchPoint: [number, number, number];
  finishedSketchPointOutline: [number, number, number];
  preview: [number, number, number];
}

interface NativeHudSelection {
  title: string;
  subject: string;
  rows: Array<{ label: string; value: string }>;
  footer: string | null;
}

interface NativeHud {
  renderNativeChrome: boolean;
  navTool: string;
  sketchMode: boolean;
  canUndo: boolean;
  canRedo: boolean;
  sixDofState: string;
  hoveredControl: string;
  pressedControl: string;
  prompt: string | null;
  dofLabel: string | null;
  coordinateReadout: string | null;
  dimOpacity: number;
  selection: NativeHudSelection | null;
}

interface NativePresentation {
  mode: 'solid' | 'pick_plane' | 'sketch';
  hoveredOriginPlane: 'xy' | 'xz' | 'yz' | null;
  hoveredDatumPlaneId: number | null;
  selectedOriginPlane: 'xy' | 'xz' | 'yz' | null;
  selectedDatumPlaneId: number | null;
  selectedBodyIds: number[];
  selectedOccurrenceId: number | null;
  hoveredOccurrenceId: number | null;
  hoveredBodyId: number | null;
  selectedFaceIds: number[];
  hoveredFaceId: number | null;
  selectedEdgeIds: number[];
  hoveredEdgeId: number | null;
  pickRefinableEdges: boolean;
  pickStraightEdges: boolean;
  selectedSketchEntityIds: number[];
  /** Entities owned by the selected geometric constraint (not true selection). */
  constraintRelatedSketchEntityIds: number[];
  hoveredSketchEntityId: number | null;
  selectedFinishedSketchEntities: FinishedSketchEntityPickRef[];
  hoveredFinishedSketchEntity: FinishedSketchEntityPickRef | null;
  selectedSketchPoints: SketchPointRefDto[];
  hoveredSketchPoint: SketchPointRefDto | null;
  selectedSurfacePoint: Point3Dto | null;
  hoveredSurfacePoint: Point3Dto | null;
  hiddenBodyIds: number[];
  hiddenDatumPlaneIds: number[];
  hiddenSketchNames: string[];
  profilePickerActive: boolean;
  candidateProfiles: ProfileRefDto[];
  selectedProfiles: ProfileRefDto[];
  hoveredProfile: ProfileRefDto | null;
  bodyPoses: import('../../engine/types').BodyPoseDto[];
  instanceBodyPoses: import('../../engine/types').InstanceBodyPoseDto[];
}

export type NativeViewportLinePattern = 'solid' | 'dotted';

export interface NativeViewportLineLayer {
  color: [number, number, number, number];
  width: number;
  /** Presentation meaning, kept semantic so Bevy can preserve screen-space spacing. */
  pattern: NativeViewportLinePattern;
  segments: number[];
}

export interface NativeViewportPointLayer {
  color: [number, number, number, number];
  radius: number;
  /** Hollow rings retain geometric freedom; filled dots are fully constrained. */
  hollow?: boolean;
  positions: number[];
}

/** Triangle-list presentation geometry rendered by Bevy, never the kernel. */
export interface NativeViewportTriangleLayer {
  color: [number, number, number, number];
  /** World-space triangle vertices, packed as x, y, z. */
  positions: number[];
  /** Render after model depth so an internal selected profile remains visible. */
  xray: boolean;
}

/** Semantic CAD direction arrow. Bevy owns its shaft and arrowhead pixels. */
export interface NativeViewportArrow {
  start: [number, number, number];
  end: [number, number, number];
  color: [number, number, number, number];
  /** Approximate screen-space width in logical pixels. */
  width: number;
  xray: boolean;
}

export interface NativeViewportAnnotation {
  screen: [number, number];
  color: [number, number, number, number];
  text: string;
  kind: 'dimension' | 'constraint' | 'tool';
  selected?: boolean;
  /** Semantic constraint artwork shared with the command toolbar. */
  icon?: ConstraintIconKind;
  /** Semantic sketch-command artwork shown beside an active-tool cursor. */
  toolIcon?: NativeViewportToolIcon;
}

export type NativeViewportToolIcon =
  | 'line'
  | 'midpoint_line'
  | 'point'
  | 'rectangle'
  | 'circle'
  | 'arc'
  | 'dimension'
  | 'fillet'
  | 'chamfer'
  | 'offset'
  | 'trim'
  | 'extend'
  | 'break'
  | 'mirror'
  | 'move_copy'
  | 'scale'
  | 'polygon'
  | 'slot'
  | 'spline';

export type NativeViewportSnapKind =
  | 'grid'
  | 'origin'
  | 'point'
  | 'midpoint'
  | 'reference_midpoint'
  | 'curve';

export interface NativeViewportSnapMarker {
  position: [number, number, number];
  kind: NativeViewportSnapKind;
}

export interface NativeViewportTransient {
  lines: NativeViewportLineLayer[];
  points: NativeViewportPointLayer[];
  triangles: NativeViewportTriangleLayer[];
  arrows: NativeViewportArrow[];
  annotations: NativeViewportAnnotation[];
  marker: NativeViewportSnapMarker | null;
}

const overlaySelector = [
  '[data-native-viewport-overlay]',
  '.feature-dialog',
  '[role="dialog"]',
  '[role="alertdialog"]',
  '[data-ribbon-menu]',
  '[data-testid="extrude-dialog"]',
  '[data-testid="revolve-dialog"]',
  '[data-testid="construction-plane-dialog"]',
  '[data-testid="body-feature-dialog"]',
].join(',');

let probeInFlight: Promise<boolean> | null = null;
let active = false;
let latestMetrics: NativeViewportMetrics | null = null;
let pendingCamera: NativeCameraState | null = null;
// Updated synchronously with the visible Three-compatible camera. Picks carry
// this snapshot directly so they cannot race the coalesced render-camera IPC.
let latestCamera: NativeCameraState | null = null;
let cameraPumpQueued = false;
let cameraInFlight = false;
let lastCameraKey = '';
let lastPreviewKey = '';
let pendingPreview: NativeViewportTransient | null = null;
let previewInFlight = false;
let lastLayoutKey = '';
let layoutRevision = Date.now() * 1000;
let pendingPresentation: NativePresentation | null = null;
let presentationInFlight = false;
let lastPresentationKey = '';
let hoveredHudControl = '';
let pressedHudControl = '';

function pumpCamera(): void {
  if (cameraPumpQueued || cameraInFlight || !pendingCamera) return;
  cameraPumpQueued = true;
  // Combine matrix + target callbacks from one device sample without waiting
  // for a full browser animation frame.
  queueMicrotask(() => {
    cameraPumpQueued = false;
    if (cameraInFlight) return;
    const camera = pendingCamera;
    pendingCamera = null;
    if (!camera) return;
    cameraInFlight = true;
    void invoke('native_viewport_set_camera', { camera })
      .catch(() => undefined)
      .finally(() => {
        cameraInFlight = false;
        pumpCamera();
      });
  });
}

function isTauriRuntime(): boolean {
  return '__TAURI_INTERNALS__' in window;
}

function probe(): Promise<boolean> {
  if (active) return Promise.resolve(true);
  if (probeInFlight) return probeInFlight;
  probeInFlight = (async () => {
    if (!isTauriRuntime()) return false;
    try {
      latestMetrics = await invoke<NativeViewportMetrics>('native_viewport_metrics');
      active = latestMetrics.available && latestMetrics.ready;
      return active;
    } catch {
      active = false;
      return false;
    }
  })().finally(() => {
    probeInFlight = null;
  });
  return probeInFlight;
}

export function nativeViewportIsActive(): boolean {
  return active;
}

export function nativeViewportMetrics(): NativeViewportMetrics | null {
  return latestMetrics;
}

function rectFor(element: Element): NativeRect | null {
  // Viewport HUD descendants can still report their own opacity as `1` even
  // when an ancestor proxy is transparent. Keep the production ownership
  // boundary explicit so no child of a Bevy-owned HUD is ever punched back
  // through the native surface as a DOM island.
  if (
    document.documentElement.dataset.nativeViewport === 'bevy' &&
    element.closest('[data-native-hud]')
  ) {
    return null;
  }
  const style = getComputedStyle(element);
  if (
    style.display === 'none' ||
    style.visibility === 'hidden' ||
    Number(style.opacity) === 0
  ) {
    return null;
  }
  const rect = element.getBoundingClientRect();
  if (rect.width < 1 || rect.height < 1) return null;
  const radius = (value: string, extent: number) => {
    const parsed = Number.parseFloat(value);
    if (!Number.isFinite(parsed)) return 0;
    return value.trim().endsWith('%') ? (parsed / 100) * extent : parsed;
  };
  // The native hosts currently support one conservative radius per island.
  // Using the smallest CSS corner ensures Bevy never covers actual DOM pixels.
  const cornerRadius = Math.max(
    0,
    Math.min(
      radius(style.borderTopLeftRadius, Math.min(rect.width, rect.height)),
      radius(style.borderTopRightRadius, Math.min(rect.width, rect.height)),
      radius(style.borderBottomRightRadius, Math.min(rect.width, rect.height)),
      radius(style.borderBottomLeftRadius, Math.min(rect.width, rect.height)),
      rect.width / 2,
      rect.height / 2,
    ),
  );
  return {
    x: rect.left,
    y: rect.top,
    width: rect.width,
    height: rect.height,
    cornerRadius,
  };
}

function overlaps(a: NativeRect, b: NativeRect): boolean {
  return (
    a.x < b.x + b.width &&
    a.x + a.width > b.x &&
    a.y < b.y + b.height &&
    a.y + a.height > b.y
  );
}

function subtractRect(source: NativeRect, cut: NativeRect): NativeRect[] {
  if (!overlaps(source, cut)) return [source];
  const left = Math.max(source.x, cut.x);
  const top = Math.max(source.y, cut.y);
  const right = Math.min(source.x + source.width, cut.x + cut.width);
  const bottom = Math.min(source.y + source.height, cut.y + cut.height);
  const sourceRight = source.x + source.width;
  const sourceBottom = source.y + source.height;
  return [
    {
      x: source.x,
      y: source.y,
      width: source.width,
      height: top - source.y,
      cornerRadius: 0,
    },
    {
      x: source.x,
      y: bottom,
      width: source.width,
      height: sourceBottom - bottom,
      cornerRadius: 0,
    },
    {
      x: source.x,
      y: top,
      width: left - source.x,
      height: bottom - top,
      cornerRadius: 0,
    },
    {
      x: right,
      y: top,
      width: sourceRight - right,
      height: bottom - top,
      cornerRadius: 0,
    },
  ].filter((rect) => rect.width > 0.01 && rect.height > 0.01);
}

/**
 * CAShapeLayer's even/odd mask requires disjoint islands. A bounding-box union
 * is not equivalent to a geometric union: when a ribbon flyout overlaps the
 * full-width project tab, it would expose the entire rectangular band between
 * them. Subtracting previously accepted rectangles preserves the exact union
 * while guaranteeing that no output rectangles overlap.
 */
export function disjointOverlayRects(rects: NativeRect[]): NativeRect[] {
  const disjoint: NativeRect[] = [];
  for (const rect of rects) {
    let candidates = [rect];
    for (const existing of disjoint) {
      const next: NativeRect[] = [];
      for (const candidate of candidates) {
        next.push(...subtractRect(candidate, existing));
      }
      candidates = next;
      if (candidates.length === 0) break;
    }
    disjoint.push(...candidates);
  }
  return disjoint;
}

function overlayStackingPriority(element: Element): number {
  let priority = 0;
  let current: Element | null = element;
  while (current) {
    const zIndex = Number.parseInt(getComputedStyle(current).zIndex, 10);
    if (Number.isFinite(zIndex)) priority = Math.max(priority, zIndex);
    current = current.parentElement;
  }
  return priority;
}

export function collectNativeViewportOverlayRects(): NativeRect[] {
  // Subtract lower islands from the visibly topmost element, not vice versa.
  // This preserves a modal's rounded outline when it overlaps the project tab
  // or another lower shell island in a compact window.
  const elements = [...document.querySelectorAll(overlaySelector)]
    .map((element, documentOrder) => ({
      element,
      documentOrder,
      stackingPriority: overlayStackingPriority(element),
    }))
    .sort(
      (a, b) =>
        b.stackingPriority - a.stackingPriority ||
        b.documentOrder - a.documentOrder,
    )
    .map(({ element }) => element);
  return disjointOverlayRects(
    elements
      .map(rectFor)
      .filter((rect): rect is NativeRect => rect !== null),
  );
}

function cssRgb(variable: string, fallback: string): [number, number, number] {
  const value =
    getComputedStyle(document.documentElement).getPropertyValue(variable).trim() ||
    fallback;
  const match = /^#([0-9a-f]{6})$/i.exec(value);
  const hex = match?.[1] ?? fallback.slice(1);
  return [
    Number.parseInt(hex.slice(0, 2), 16) / 255,
    Number.parseInt(hex.slice(2, 4), 16) / 255,
    Number.parseInt(hex.slice(4, 6), 16) / 255,
  ];
}

function collectPalette(): NativePalette {
  return {
    background: cssRgb('--viewport', '#2a2d33'),
    panel: cssRgb('--panel', '#22262c'),
    header: cssRgb('--header', '#282d34'),
    uiEdge: cssRgb('--edge', '#3a3e46'),
    ink: cssRgb('--ink', '#e7ebef'),
    mute: cssRgb('--mute', '#9aa3ad'),
    accent: cssRgb('--accent', '#7c6df2'),
    gridFine: cssRgb('--cad-ground-fine', '#3a3f47'),
    gridMajor: cssRgb('--cad-ground-major', '#4d545f'),
    body: cssRgb('--cad-body', '#8b9bac'),
    bodySelected: cssRgb('--cad-body-selected', '#a96725'),
    bodyTool: cssRgb('--cad-body-tool', '#b58a43'),
    bodySelectedEdge: cssRgb('--cad-pick-selected', '#ffd000'),
    faceHover: cssRgb('--cad-face-hover', '#238a9d'),
    faceSelected: cssRgb('--cad-face-selected', '#cf7715'),
    edge: cssRgb('--cad-edge', '#29333d'),
    edgeHover: cssRgb('--cad-pick-hover', '#00f5ff'),
    edgeSelected: cssRgb('--cad-pick-selected', '#ffd000'),
    pickHalo: cssRgb('--cad-pick-halo', '#ffffff'),
    originPlaneXy: cssRgb('--cad-origin-plane-xy', '#57a8ff'),
    originPlaneXz: cssRgb('--cad-origin-plane-xz', '#55c978'),
    originPlaneYz: cssRgb('--cad-origin-plane-yz', '#ff7078'),
    activeSketch: cssRgb('--cad-pick-normal', '#86a9c7'),
    definedSketch: cssRgb('--cad-defined', '#e8e9ec'),
    hover: cssRgb('--cad-pick-hover', '#00f5ff'),
    selection: cssRgb('--cad-pick-selected', '#ffd000'),
    constraintRelated: cssRgb('--cad-constraint-related', '#3ecf9a'),
    finishedSketch: cssRgb('--cad-pick-normal', '#86a9c7'),
    finishedSketchPoint: cssRgb('--cad-pick-normal', '#86a9c7'),
    finishedSketchPointOutline: cssRgb(
      '--cad-finished-point-outline',
      '#15191f',
    ),
    preview: cssRgb('--cad-preview', '#8fc4ff'),
  };
}

function elementText(element: Element | null): string {
  return element?.textContent?.trim().replace(/\s+/g, ' ') ?? '';
}

function nativeHudControl(target: EventTarget | null): string {
  if (!(target instanceof Element)) return '';
  const explicit = target.closest<HTMLElement>('[data-native-hud-control]');
  if (explicit) return explicit.dataset.nativeHudControl ?? '';
  const nav = target.closest<HTMLElement>('[data-native-nav-id]');
  if (nav) return `nav:${nav.dataset.nativeNavId ?? ''}`;
  const orientation = target.closest<HTMLElement>('[data-orientation-preset]');
  if (orientation) {
    return `orientation:${orientation.dataset.orientationPreset ?? ''}`;
  }
  return '';
}

function collectSelectionHud(): NativeHudSelection | null {
  const root = document.querySelector('[data-native-hud="selection"]');
  if (!root) return null;
  const rows = [...root.querySelectorAll('[data-native-hud-row]')].map((row) => ({
    label: elementText(row.querySelector('[data-native-hud-label]')),
    value: elementText(row.querySelector('[data-native-hud-value]')),
  }));
  return {
    title: elementText(root.querySelector('[data-native-hud-title]')) || 'SELECTION',
    subject: elementText(root.querySelector('[data-native-hud-subject]')),
    rows,
    footer: elementText(root.querySelector('[data-native-hud-footer]')) || null,
  };
}

export function collectNativeViewportDimOpacity(): number {
  let opacity = 0;
  for (const element of document.querySelectorAll(
    '[data-native-viewport-dim]',
  )) {
    const style = getComputedStyle(element);
    if (
      style.display === 'none' ||
      style.visibility === 'hidden' ||
      Number(style.opacity) === 0
    ) {
      continue;
    }
    const requested = Number.parseFloat(
      element.getAttribute('data-native-viewport-dim') ?? '0',
    );
    if (Number.isFinite(requested)) {
      opacity = Math.max(opacity, requested);
    }
  }
  return Math.max(0, Math.min(0.85, opacity));
}

function collectHud(): NativeHud {
  const state = useAppStore.getState();
  const navigation = document.querySelector('[data-native-hud="navigation"]');
  const prompt = elementText(document.querySelector('[data-native-hud="prompt"]'));
  const dofLabel = elementText(document.querySelector('[data-native-hud="dof"]'));
  const coordinateReadout = elementText(
    document.querySelector('[data-native-hud="coordinate"]'),
  );
  return {
    renderNativeChrome: true,
    navTool: state.navTool,
    sketchMode: state.mode === 'sketch',
    canUndo: state.activeSketch?.can_undo ?? false,
    canRedo: state.activeSketch?.can_redo ?? false,
    sixDofState: navigation?.getAttribute('data-native-six-dof-state') ?? 'disconnected',
    hoveredControl: hoveredHudControl,
    pressedControl: pressedHudControl,
    prompt: prompt || null,
    dofLabel: dofLabel || null,
    coordinateReadout: coordinateReadout || null,
    dimOpacity: collectNativeViewportDimOpacity(),
    selection: collectSelectionHud(),
  };
}

function hiddenReferences(
  nodes: BrowserNode[],
  hidden: Record<number, boolean>,
  kind: BrowserNode['kind'],
): number[] {
  const ids: number[] = [];
  const visit = (entries: BrowserNode[]) => {
    for (const node of entries) {
      if (node.kind === kind && node.reference_id !== null && hidden[node.id]) {
        ids.push(node.reference_id);
      }
      visit(node.children);
    }
  };
  visit(nodes);
  return ids;
}

function hiddenNames(
  nodes: BrowserNode[],
  hidden: Record<number, boolean>,
  kind: BrowserNode['kind'],
): string[] {
  const names: string[] = [];
  const visit = (entries: BrowserNode[]) => {
    for (const node of entries) {
      if (node.kind === kind && node.name !== null && hidden[node.id]) {
        names.push(node.name);
      }
      visit(node.children);
    }
  };
  visit(nodes);
  return names;
}

function quaternionMultiply(
  a: [number, number, number, number],
  b: [number, number, number, number],
): [number, number, number, number] {
  return [
    a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
    a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
    a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
    a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
  ];
}

function rotateByQuaternion(
  value: [number, number, number],
  quaternion: [number, number, number, number],
): [number, number, number] {
  const [x, y, z, w] = quaternion;
  const [vx, vy, vz] = value;
  const tx = 2 * (y * vz - z * vy);
  const ty = 2 * (z * vx - x * vz);
  const tz = 2 * (x * vy - y * vx);
  return [
    vx + w * tx + (y * tz - z * ty),
    vy + w * ty + (z * tx - x * tz),
    vz + w * tz + (x * ty - y * tx),
  ];
}

function samePose(
  translation: [number, number, number],
  rotation: [number, number, number, number],
  target: MoveCopyCommandPreview['targets'][number],
): boolean {
  return translation.every((value, index) =>
    Math.abs(value - target.baseTranslation[index]) <= 1e-7)
    && rotation.every((value, index) =>
      Math.abs(value - target.baseRotation[index]) <= 1e-7);
}

function movedPreviewPose(
  target: MoveCopyCommandPreview['targets'][number],
  preview: MoveCopyCommandPreview,
): {
  translation: [number, number, number];
  rotation: [number, number, number, number];
} {
  const pivot: [number, number, number] = [
    preview.pivot.x,
    preview.pivot.y,
    preview.pivot.z,
  ];
  const delta: [number, number, number] = [
    preview.translation.x,
    preview.translation.y,
    preview.translation.z,
  ];
  if (preview.transformInBodySpace) {
    const rotatedNegativePivot = rotateByQuaternion(
      [-pivot[0], -pivot[1], -pivot[2]],
      preview.rotation,
    );
    const localOffset: [number, number, number] = [
      pivot[0] + delta[0] + rotatedNegativePivot[0],
      pivot[1] + delta[1] + rotatedNegativePivot[1],
      pivot[2] + delta[2] + rotatedNegativePivot[2],
    ];
    const worldOffset = rotateByQuaternion(localOffset, target.baseRotation);
    return {
      translation: [
        target.baseTranslation[0] + worldOffset[0],
        target.baseTranslation[1] + worldOffset[1],
        target.baseTranslation[2] + worldOffset[2],
      ],
      rotation: quaternionMultiply(target.baseRotation, preview.rotation),
    };
  }
  const rotatedBase = rotateByQuaternion(
    [
      target.baseTranslation[0] - pivot[0],
      target.baseTranslation[1] - pivot[1],
      target.baseTranslation[2] - pivot[2],
    ],
    preview.rotation,
  );
  return {
    translation: [
      pivot[0] + delta[0] + rotatedBase[0],
      pivot[1] + delta[1] + rotatedBase[1],
      pivot[2] + delta[2] + rotatedBase[2],
    ],
    rotation: quaternionMultiply(preview.rotation, target.baseRotation),
  };
}

function moveCopyPresentationPoses(
  bodyPoses: BodyPoseDto[],
  instanceBodyPoses: InstanceBodyPoseDto[],
  preview: MoveCopyCommandPreview | null,
): [BodyPoseDto[], InstanceBodyPoseDto[]] {
  if (!preview || preview.copy) return [bodyPoses, instanceBodyPoses];
  const targetFor = (
    bodyId: number,
    occurrenceId: number | null,
    translation: [number, number, number],
    rotation: [number, number, number, number],
  ) => preview.targets.find((target) =>
    target.bodyId === bodyId
      && target.occurrenceId === occurrenceId
      && samePose(translation, rotation, target));
  return [
    bodyPoses.map((pose) => {
      const target = targetFor(pose.body_id, null, pose.translation, pose.rotation);
      return target ? { ...pose, ...movedPreviewPose(target, preview) } : pose;
    }),
    instanceBodyPoses.map((pose) => {
      const target = targetFor(
        pose.body_id,
        pose.occurrence_id,
        pose.translation,
        pose.rotation,
      );
      return target ? { ...pose, ...movedPreviewPose(target, preview) } : pose;
    }),
  ];
}

export function collectNativeViewportPresentation(): NativePresentation {
  const state = useAppStore.getState();
  const pickerFeedback = collectAppViewportPickFeedback(state);
  const playback = playbackPresentation.snapshot();
  const emphasize = playback.active && playback.mode === 'present' && !playback.finished && !playback.stopped;
  const edgePickMode = activeEdgePickMode(pickerFeedback.activePick);
  const selectedSketchEntityIds = [...new Set([...state.selectedEntities,
    ...(emphasize ? playback.highlighted_sketch_entity_ids : [])])];
  if (
    state.selectedEntity !== null &&
    !selectedSketchEntityIds.includes(state.selectedEntity)
  ) {
    selectedSketchEntityIds.push(state.selectedEntity);
  }
  const constraintRelatedSketchEntityIds: number[] = [];
  if (state.selectedConstraint !== null && state.activeSketch) {
    const constraint = state.activeSketch.constraints.find(
      (candidate) => candidate.id === state.selectedConstraint,
    );
    if (constraint) {
      for (const id of constraintReferencedEntityIds(constraint)) {
        if (!constraintRelatedSketchEntityIds.includes(id)) {
          constraintRelatedSketchEntityIds.push(id);
        }
      }
    }
  }
  const browser = state.document?.browser ?? [];
  const solved = state.jointDialogOpen && state.jointPreviewSolution
    ? state.jointPreviewSolution
    : state.mechanismPreview?.solution
      ?? state.jointMotionPreview?.solution
      ?? state.assemblySolution;
  const movePreview = state.solidCommandPreview?.kind === 'move_copy'
    ? state.solidCommandPreview
    : null;
  const [bodyPoses, instanceBodyPoses] = moveCopyPresentationPoses(
    solved.body_poses,
    solved.instance_body_poses,
    movePreview,
  );
  return {
    mode:
      state.mode === 'pickPlane' ||
      pickAccepts(pickerFeedback.activePick, 'reference-plane')
        ? 'pick_plane'
        : state.mode,
    hoveredOriginPlane: state.hoveredPlane,
    hoveredDatumPlaneId: state.hoveredDatumPlane,
    selectedOriginPlane:
      pickerFeedback.selectedReferencePlane?.type === 'origin_plane'
        ? pickerFeedback.selectedReferencePlane.plane
        : null,
    selectedDatumPlaneId:
      pickerFeedback.selectedReferencePlane?.type === 'datum_plane'
        ? pickerFeedback.selectedReferencePlane.datum_id
        : null,
    // This is a render-only emphasis layer. It never changes application
    // selection, picking state, operation targets, or the saved model.
    selectedBodyIds: [...new Set([...pickerFeedback.selectedBodyIds,
      ...(emphasize ? playback.highlighted_body_ids : [])])],
    selectedOccurrenceId: state.selectedOccurrenceId,
    hoveredOccurrenceId: state.hoveredOccurrenceId,
    hoveredBodyId: pickerFeedback.hoveredBodyId,
    selectedFaceIds: pickerFeedback.selectedFaceIds,
    hoveredFaceId: pickerFeedback.hoveredFaceId,
    selectedEdgeIds: pickerFeedback.selectedEdgeIds,
    hoveredEdgeId: pickerFeedback.hoveredEdgeId,
    pickRefinableEdges: edgePickMode === 'refinable',
    pickStraightEdges: edgePickMode === 'straight',
    selectedSketchEntityIds,
    constraintRelatedSketchEntityIds,
    hoveredSketchEntityId: state.hoveredEntity,
    selectedFinishedSketchEntities:
      pickerFeedback.selectedFinishedSketchEntities,
    hoveredFinishedSketchEntity:
      pickerFeedback.hoveredFinishedSketchEntity,
    selectedSketchPoints: pickerFeedback.selectedSketchPoints.map(
      ({ world: _world, ...reference }) => reference,
    ),
    hoveredSketchPoint: pickerFeedback.hoveredSketchPoint
      ? (({ world: _world, ...reference }) => reference)(
          pickerFeedback.hoveredSketchPoint,
        )
      : null,
    selectedSurfacePoint: pickerFeedback.selectedSurfacePoint,
    hoveredSurfacePoint: pickerFeedback.hoveredSurfacePoint,
    hiddenBodyIds: hiddenReferences(browser, state.hidden, 'body'),
    hiddenDatumPlaneIds: hiddenReferences(
      browser,
      state.hidden,
      'construction_plane',
    ),
    hiddenSketchNames: hiddenNames(browser, state.hidden, 'sketch'),
    profilePickerActive: state.profilePicker !== null,
    candidateProfiles:
      state.profilePicker?.catalog.flatMap((entry) => {
        if (
          state.profilePicker?.owner === 'revolve'
          && !revolveProfileAcceptsAxis(
            state.profilePicker.catalog,
            entry.sketch_name,
            state.revolveAxisSelection,
          )
        ) return [];
        return entry.profiles
          .filter((profile) => profile.nesting_depth % 2 === 0)
          .map((profile) => ({
            sketch_name: entry.sketch_name,
            profile_index: profile.index,
          }));
      }) ?? [],
    selectedProfiles: pickerFeedback.selectedProfiles,
    hoveredProfile: pickerFeedback.hoveredProfile,
    bodyPoses,
    instanceBodyPoses,
  };
}

function pumpPresentation(): void {
  if (presentationInFlight || !pendingPresentation || !active) return;
  const presentation = pendingPresentation;
  pendingPresentation = null;
  presentationInFlight = true;
  void invoke('native_viewport_set_presentation', { presentation })
    .catch(() => {
      lastPresentationKey = '';
    })
    .finally(() => {
      presentationInFlight = false;
      pumpPresentation();
    });
}

function syncPresentation(): void {
  if (!active) return;
  const presentation = collectNativeViewportPresentation();
  const key = JSON.stringify(presentation);
  if (key === lastPresentationKey) return;
  lastPresentationKey = key;
  pendingPresentation = presentation;
  pumpPresentation();
}

async function syncModel(): Promise<void> {
  if (!(await probe())) return;
  await invoke('native_viewport_sync_model');
}

const WINDOWS_NATIVE_INPUT_PREFIX = '__nbcad_native_input__|';

type WindowsWebViewMessageHost = {
  addEventListener: (
    type: 'message',
    listener: (event: MessageEvent<unknown>) => void,
  ) => void;
  removeEventListener: (
    type: 'message',
    listener: (event: MessageEvent<unknown>) => void,
  ) => void;
};

/**
 * The opaque Bevy HWND sits above WebView2 on Windows. WebView2's renderer is
 * owned by another UI thread, so Win32's HTTRANSPARENT hand-off stops before
 * it reaches the DOM. Rust forwards compact mouse records through WebView2's
 * message channel and this adapter dispatches them onto the unchanged DOM
 * interaction surface and HUD proxies.
 */
function attachWindowsNativeInput(container: HTMLElement): () => void {
  const webview = (
    window as Window & {
      chrome?: { webview?: WindowsWebViewMessageHost };
    }
  ).chrome?.webview;
  if (!webview) return () => undefined;

  let hoveredTarget: Element | null = null;
  let pressedTarget: Element | null = null;
  let pressedButton = -1;
  let doublePress = false;
  let moveFrame = 0;
  let pendingMove:
    | {
        clientX: number;
        clientY: number;
        buttons: number;
        modifiers: number;
        hitTarget: Element | null;
      }
    | null = null;

  const pointerEvent = (
    type: string,
    target: Element,
    clientX: number,
    clientY: number,
    button: number,
    buttons: number,
    modifiers: number,
    relatedTarget: EventTarget | null = null,
    detail = 0,
  ) =>
    target.dispatchEvent(
      new PointerEvent(type, {
        bubbles: type !== 'pointerenter' && type !== 'pointerleave',
        cancelable: true,
        composed: true,
        pointerId: 1,
        pointerType: 'mouse',
        isPrimary: true,
        width: 1,
        height: 1,
        pressure: buttons === 0 ? 0 : 0.5,
        clientX,
        clientY,
        screenX: window.screenX + clientX,
        screenY: window.screenY + clientY,
        button,
        buttons,
        detail,
        shiftKey: (modifiers & 1) !== 0,
        ctrlKey: (modifiers & 2) !== 0,
        altKey: (modifiers & 4) !== 0,
        metaKey: (modifiers & 8) !== 0,
        relatedTarget,
      }),
    );

  const mouseEvent = (
    type: string,
    target: Element,
    clientX: number,
    clientY: number,
    button: number,
    buttons: number,
    modifiers: number,
    detail: number,
  ) =>
    target.dispatchEvent(
      new MouseEvent(type, {
        bubbles: true,
        cancelable: true,
        composed: true,
        view: window,
        clientX,
        clientY,
        screenX: window.screenX + clientX,
        screenY: window.screenY + clientY,
        button,
        buttons,
        detail,
        shiftKey: (modifiers & 1) !== 0,
        ctrlKey: (modifiers & 2) !== 0,
        altKey: (modifiers & 4) !== 0,
        metaKey: (modifiers & 8) !== 0,
      }),
    );

  const updateHoveredTarget = (
    target: Element | null,
    clientX: number,
    clientY: number,
    buttons: number,
    modifiers: number,
  ) => {
    if (target === hoveredTarget) return;
    const previous = hoveredTarget;
    hoveredTarget = target;
    if (previous) {
      pointerEvent(
        'pointerout',
        previous,
        clientX,
        clientY,
        -1,
        buttons,
        modifiers,
        target,
      );
      pointerEvent(
        'pointerleave',
        previous,
        clientX,
        clientY,
        -1,
        buttons,
        modifiers,
        target,
      );
    }
    if (target) {
      pointerEvent(
        'pointerover',
        target,
        clientX,
        clientY,
        -1,
        buttons,
        modifiers,
        previous,
      );
      pointerEvent(
        'pointerenter',
        target,
        clientX,
        clientY,
        -1,
        buttons,
        modifiers,
        previous,
      );
    }
  };

  const dispatchPendingMove = () => {
    moveFrame = 0;
    const move = pendingMove;
    pendingMove = null;
    if (!move) return;
    const target =
      move.buttons !== 0 && pressedTarget ? pressedTarget : move.hitTarget;
    updateHoveredTarget(
      target,
      move.clientX,
      move.clientY,
      move.buttons,
      move.modifiers,
    );
    if (target) {
      pointerEvent(
        'pointermove',
        target,
        move.clientX,
        move.clientY,
        -1,
        move.buttons,
        move.modifiers,
      );
    }
  };

  const flushPendingMove = () => {
    if (moveFrame !== 0) cancelAnimationFrame(moveFrame);
    dispatchPendingMove();
  };

  const onMessage = (event: MessageEvent<unknown>) => {
    if (
      typeof event.data !== 'string' ||
      !event.data.startsWith(WINDOWS_NATIVE_INPUT_PREFIX)
    ) {
      return;
    }
    const fields = event.data
      .slice(WINDOWS_NATIVE_INPUT_PREFIX.length)
      .split('|');
    if (fields.length !== 9) return;
    const kind = fields[0];
    const values = fields.slice(1).map(Number);
    if (values.some((value) => !Number.isFinite(value))) return;
    const [x, y, physicalWidth, physicalHeight, button, buttons, modifiers, delta] =
      values;
    if (physicalWidth < 1 || physicalHeight < 1) return;

    const rect = container.getBoundingClientRect();
    const clientX = rect.left + (x / physicalWidth) * rect.width;
    const clientY = rect.top + (y / physicalHeight) * rect.height;
    const hit = document.elementFromPoint(clientX, clientY);
    const hitTarget = hit && container.contains(hit) ? hit : null;

    // Mouse-move records can arrive much faster than the native renderer can
    // present on an integrated GPU. Keep the newest absolute coordinate and
    // dispatch at display cadence; no distance is lost and no stale queue can
    // continue moving the camera after the physical mouse stops.
    if (kind === 'm') {
      pendingMove = { clientX, clientY, buttons, modifiers, hitTarget };
      if (moveFrame === 0) moveFrame = requestAnimationFrame(dispatchPendingMove);
      return;
    }

    // A Logitech wheel's left/right tilt is deliberately not a viewport
    // gesture. Keep this defensive branch for older native hosts that still
    // send the legacy horizontal-wheel record.
    if (kind === 'h') return;

    // Preserve event order at button/wheel/capture boundaries.
    flushPendingMove();
    const target = buttons !== 0 && pressedTarget ? pressedTarget : hitTarget;

    if (kind === 'l') {
      updateHoveredTarget(null, clientX, clientY, 0, modifiers);
      return;
    }
    if (kind === 'v') {
      const wheelTarget = hitTarget ?? hoveredTarget ?? container;
      wheelTarget.dispatchEvent(
        new WheelEvent('wheel', {
          bubbles: true,
          cancelable: true,
          composed: true,
          view: window,
          clientX,
          clientY,
          screenX: window.screenX + clientX,
          screenY: window.screenY + clientY,
          buttons,
          deltaX: 0,
          deltaY: -delta,
          deltaMode: WheelEvent.DOM_DELTA_PIXEL,
          shiftKey: (modifiers & 1) !== 0,
          ctrlKey: (modifiers & 2) !== 0,
          altKey: (modifiers & 4) !== 0,
          metaKey: (modifiers & 8) !== 0,
        }),
      );
      return;
    }

    if (kind === 'c') {
      const cancelTarget = pressedTarget ?? hoveredTarget ?? container;
      if (pressedTarget || pressedButton >= 0) {
        pointerEvent(
          'pointercancel',
          cancelTarget,
          clientX,
          clientY,
          pressedButton,
          0,
          modifiers,
        );
      }
      pressedTarget = null;
      pressedButton = -1;
      doublePress = false;
      updateHoveredTarget(hitTarget, clientX, clientY, 0, modifiers);
      return;
    }

    if (kind === 'd' || kind === 'b') {
      const downTarget = hitTarget ?? container;
      updateHoveredTarget(downTarget, clientX, clientY, buttons, modifiers);
      pressedTarget = downTarget;
      pressedButton = button;
      doublePress = kind === 'b';
      pointerEvent(
        'pointerdown',
        downTarget,
        clientX,
        clientY,
        button,
        buttons,
        modifiers,
        null,
        doublePress ? 2 : 1,
      );
      return;
    }

    if (kind !== 'u') return;
    const upTarget = pressedTarget ?? hitTarget ?? container;
    pointerEvent(
      'pointerup',
      upTarget,
      clientX,
      clientY,
      button,
      buttons,
      modifiers,
      null,
      doublePress ? 2 : 1,
    );
    const sameTarget =
      hitTarget !== null &&
      (hitTarget === pressedTarget ||
        pressedTarget?.contains(hitTarget) === true ||
        hitTarget.contains(pressedTarget));
    if (button === 0 && button === pressedButton && sameTarget) {
      const detail = doublePress ? 2 : 1;
      mouseEvent(
        'click',
        upTarget,
        clientX,
        clientY,
        button,
        buttons,
        modifiers,
        detail,
      );
      if (doublePress) {
        mouseEvent(
          'dblclick',
          upTarget,
          clientX,
          clientY,
          button,
          buttons,
          modifiers,
          2,
        );
      }
    } else if (button === 2) {
      mouseEvent(
        'contextmenu',
        upTarget,
        clientX,
        clientY,
        button,
        buttons,
        modifiers,
        1,
      );
    }
    pressedTarget = null;
    pressedButton = -1;
    doublePress = false;
    updateHoveredTarget(hitTarget, clientX, clientY, buttons, modifiers);
  };

  webview.addEventListener('message', onMessage);
  return () => {
    webview.removeEventListener('message', onMessage);
    if (moveFrame !== 0) cancelAnimationFrame(moveFrame);
    moveFrame = 0;
    pendingMove = null;
    hoveredTarget = null;
    pressedTarget = null;
  };
}

/**
 * Binds native layout/model synchronization to the existing viewport. The
 * mutation observer watches only layout-bearing attributes; the orientation
 * dial's continuously changing SVG coordinates do not wake the Rust worker.
 */
export function attachNativeViewport(container: HTMLElement): () => void {
  let disposed = false;
  let layoutFrame = 0;
  let layoutEpoch = 0;
  let layoutInFlight = false;
  let layoutRequested = false;
  let probeTimer = 0;
  let startupStatusTimer = 0;
  let startupStatus: HTMLDivElement | null = null;
  let settleTimers: number[] = [];
  let nativeWindowUnlisteners: Array<() => void> = [];
  const detachWindowsNativeInput = attachWindowsNativeInput(container);

  const clearStartupStatus = () => {
    if (startupStatusTimer !== 0) {
      window.clearTimeout(startupStatusTimer);
      startupStatusTimer = 0;
    }
    startupStatus?.remove();
    startupStatus = null;
  };
  const showStartupStatus = (message: string, failed = false) => {
    if (disposed) return;
    if (!startupStatus) {
      startupStatus = document.createElement('div');
      startupStatus.dataset.nativeViewportStartup = failed ? 'error' : 'waiting';
      startupStatus.className =
        'pointer-events-none absolute inset-0 z-10 flex items-center justify-center bg-viewport px-8 text-center text-sm text-mute';
      container.append(startupStatus);
    }
    startupStatus.dataset.nativeViewportStartup = failed ? 'error' : 'waiting';
    startupStatus.textContent = message;
  };

  const flushLayout = async () => {
    if (disposed) return;
    if (layoutInFlight) {
      layoutRequested = true;
      return;
    }
    layoutInFlight = true;
    try {
      do {
        layoutRequested = false;
        if (!(await probe()) || disposed) break;
        const viewport = rectFor(container);
        if (!viewport) break;
        const payload = {
          viewport,
          overlays: collectNativeViewportOverlayRects(),
          palette: collectPalette(),
          hud: collectHud(),
        };
        // CSS geometry can stay identical while the native backing scale
        // changes after moving between Retina/DPI monitors. Keep the ratio in
        // the deduplication key so the platform host gets a fresh layout and
        // rebuilds its physical swapchain.
        const key = JSON.stringify({
          ...payload,
          devicePixelRatio: window.devicePixelRatio,
          layoutEpoch,
        });
        if (key === lastLayoutKey) continue;
        const layout = {
          revision: ++layoutRevision,
          ...payload,
        };
        try {
          await invoke('native_viewport_set_layout', { layout });
          lastLayoutKey = key;
        } catch {
          lastLayoutKey = '';
        }
      } while (layoutRequested && !disposed);
    } finally {
      layoutInFlight = false;
      if (layoutRequested && !disposed) void flushLayout();
    }
  };

  const scheduleLayout = () => {
    if (disposed || layoutFrame !== 0) return;
    layoutFrame = requestAnimationFrame(() => {
      layoutFrame = 0;
      void flushLayout();
    });
  };
  const flushResizeLayout = () => {
    if (disposed) return;
    // WebKit can defer requestAnimationFrame while AppKit is in its live-resize
    // run loop. ResizeObserver and Tauri window callbacks still arrive, so
    // send their latest geometry immediately. flushLayout coalesces callbacks
    // while an IPC request is in flight and prevents a resize-event backlog.
    layoutRequested = true;
    void flushLayout();
  };
  const onImmediateLayoutRequest = () => flushResizeLayout();
  const onHudPointerOver = (event: PointerEvent) => {
    const next = nativeHudControl(event.target);
    if (next === hoveredHudControl) return;
    hoveredHudControl = next;
    scheduleLayout();
  };
  const onHudPointerOut = (event: PointerEvent) => {
    const next = nativeHudControl(event.relatedTarget);
    if (next === hoveredHudControl) return;
    hoveredHudControl = next;
    scheduleLayout();
  };
  const onHudPointerDown = (event: PointerEvent) => {
    if (event.button !== 0) return;
    const next = nativeHudControl(event.target);
    if (!next || next === pressedHudControl) return;
    pressedHudControl = next;
    scheduleLayout();
  };
  const clearHudPointerPress = () => {
    if (!pressedHudControl) return;
    pressedHudControl = '';
    scheduleLayout();
  };
  const settleLayout = () => {
    const forceLayout = () => {
      // Native full-screen transitions can replace AppKit layers or reparent
      // the WKWebView without changing the final DOM rectangle. Epoch makes
      // those lifecycle passes reach Rust even when CSS geometry deduplicates.
      layoutEpoch += 1;
      flushResizeLayout();
    };
    forceLayout();
    for (const timer of settleTimers) window.clearTimeout(timer);
    settleTimers = [80, 180, 350, 700, 1_200].map((delay) =>
      window.setTimeout(forceLayout, delay),
    );
    requestAnimationFrame(() => requestAnimationFrame(forceLayout));
  };

  const observedLayoutElements = new Set<Element>();
  const viewportResize = new ResizeObserver(flushResizeLayout);
  const overlayResize = new ResizeObserver(flushResizeLayout);
  viewportResize.observe(container);
  const refreshObservedLayoutElements = () => {
    const next = new Set(
      document.querySelectorAll(
        `${overlaySelector}, [data-native-hud], [data-native-viewport-dim]`,
      ),
    );
    for (const element of observedLayoutElements) {
      if (!next.has(element)) {
        overlayResize.unobserve(element);
        observedLayoutElements.delete(element);
      }
    }
    for (const element of next) {
      if (observedLayoutElements.has(element)) continue;
      observedLayoutElements.add(element);
      overlayResize.observe(element);
    }
  };
  refreshObservedLayoutElements();
  const mutation = new MutationObserver(() => {
    refreshObservedLayoutElements();
    // Native viewport cutouts are part of the platform view hierarchy, not
    // the DOM stacking context. A deferred frame leaves newly-mounted menus
    // behind the child view long enough to flash or consume the first click.
    flushResizeLayout();
  });
  mutation.observe(document.documentElement, {
    subtree: true,
    childList: true,
    characterData: true,
    attributes: true,
    attributeFilter: [
      'class',
      'style',
      'data-theme',
      'hidden',
      'disabled',
      'data-native-nav-active',
      'data-native-six-dof-state',
      'data-native-viewport-dim',
    ],
  });
  window.addEventListener('resize', settleLayout);
  window.visualViewport?.addEventListener('resize', settleLayout);
  document.addEventListener('fullscreenchange', settleLayout);
  document.addEventListener('nbcad:native-viewport-layout', onImmediateLayoutRequest);
  document.addEventListener('input', scheduleLayout, true);
  document.addEventListener('change', scheduleLayout, true);
  document.addEventListener('transitionend', scheduleLayout, true);
  document.addEventListener('pointerover', onHudPointerOver, true);
  document.addEventListener('pointerout', onHudPointerOut, true);
  document.addEventListener('pointerdown', onHudPointerDown, true);
  window.addEventListener('pointerup', clearHudPointerPress, true);
  window.addEventListener('pointercancel', clearHudPointerPress, true);
  window.addEventListener('focus', settleLayout);

  if (isTauriRuntime()) {
    const appWindow = getCurrentWindow();
    void Promise.all([
      appWindow.onResized(settleLayout),
      appWindow.onScaleChanged(settleLayout),
    ])
      .then((unlisteners) => {
        if (disposed) {
          for (const unlisten of unlisteners) unlisten();
        } else {
          nativeWindowUnlisteners = unlisteners;
        }
      })
      .catch(() => undefined);
  }

  let previous = useAppStore.getState();
  const unsubscribePlayback = playbackPresentation.subscribe(syncPresentation);
  const unsubscribe = useAppStore.subscribe((next) => {
    if (
      next.activeSketch !== previous.activeSketch ||
      next.finishedSketches !== previous.finishedSketches ||
      next.solidScene !== previous.solidScene ||
      next.datumPlanes !== previous.datumPlanes ||
      next.bodyAppearances !== previous.bodyAppearances
    ) {
      void syncModel().catch(() => undefined);
    }
    if (
      next.mode !== previous.mode ||
      next.navTool !== previous.navTool ||
      next.activeSketch !== previous.activeSketch ||
      next.selectedEntity !== previous.selectedEntity ||
      next.selectedEntities !== previous.selectedEntities ||
      next.selectedBody !== previous.selectedBody ||
      next.selectedBodies !== previous.selectedBodies ||
      next.selectedFace !== previous.selectedFace ||
      next.selectedFaces !== previous.selectedFaces ||
      next.selectedEdges !== previous.selectedEdges
    ) {
      scheduleLayout();
    }
    syncPresentation();
    previous = next;
  });

  let probeAttempt = 0;
  const activate = async () => {
    if (disposed || !isTauriRuntime()) return;
    if (await probe()) {
      if (disposed) return;
      clearStartupStatus();
      container.dataset.nativeViewport = 'bevy';
      document.documentElement.dataset.nativeViewport = 'bevy';
      lastPreviewKey = '';
      lastLayoutKey = '';
      lastPresentationKey = '';
      // Do the first cut immediately. requestAnimationFrame may be throttled
      // while a newly launched desktop window is still behind another app.
      void flushLayout();
      void syncModel().catch(() => undefined);
      syncPresentation();
      // Web fonts and SVG icon metrics can settle after the first native cut.
      // Observed overlay roots catch the size change; these extra passes cover
      // engines that batch font layout without emitting ResizeObserver yet.
      requestAnimationFrame(scheduleLayout);
      window.setTimeout(scheduleLayout, 120);
      void document.fonts?.ready.then(scheduleLayout);
      return;
    }
    if (latestMetrics?.startupError) {
      showStartupStatus(
        `Native viewport failed to start. ${latestMetrics.startupError}`,
        true,
      );
      return;
    }
    probeAttempt += 1;
    // GPU/driver initialization on first launch can exceed ten seconds on
    // Windows. Keep probing for the lifetime of the mounted viewport instead
    // of permanently abandoning a renderer that becomes ready later.
    const retryDelay = probeAttempt < 100 ? 100 : 1_000;
    probeTimer = window.setTimeout(() => void activate(), retryDelay);
  };
  if (isTauriRuntime()) {
    startupStatusTimer = window.setTimeout(
      () => showStartupStatus('Starting native viewport…'),
      1_500,
    );
    void activate();
  }

  return () => {
    disposed = true;
    // The Bevy child view is a native sibling of the webview, so unmounting
    // the DOM viewport cannot hide it by itself. Collapse the native layout
    // before a drawing sheet (or any future non-3D workspace) takes its place.
    if (active && isTauriRuntime()) {
      const layout = {
        revision: ++layoutRevision,
        viewport: { x: 0, y: 0, width: 0, height: 0 },
        overlays: [],
        palette: collectPalette(),
        hud: collectHud(),
      };
      void invoke('native_viewport_set_layout', { layout }).catch(() => undefined);
      lastLayoutKey = '';
    }
    if (layoutFrame !== 0) cancelAnimationFrame(layoutFrame);
    if (probeTimer !== 0) window.clearTimeout(probeTimer);
    clearStartupStatus();
    for (const timer of settleTimers) window.clearTimeout(timer);
    viewportResize.disconnect();
    overlayResize.disconnect();
    mutation.disconnect();
    window.removeEventListener('resize', settleLayout);
    window.visualViewport?.removeEventListener('resize', settleLayout);
    document.removeEventListener('fullscreenchange', settleLayout);
    document.removeEventListener('nbcad:native-viewport-layout', onImmediateLayoutRequest);
    document.removeEventListener('input', scheduleLayout, true);
    document.removeEventListener('change', scheduleLayout, true);
    document.removeEventListener('transitionend', scheduleLayout, true);
    document.removeEventListener('pointerover', onHudPointerOver, true);
    document.removeEventListener('pointerout', onHudPointerOut, true);
    document.removeEventListener('pointerdown', onHudPointerDown, true);
    window.removeEventListener('pointerup', clearHudPointerPress, true);
    window.removeEventListener('pointercancel', clearHudPointerPress, true);
    window.removeEventListener('focus', settleLayout);
    detachWindowsNativeInput();
    for (const unlisten of nativeWindowUnlisteners) unlisten();
    nativeWindowUnlisteners = [];
    unsubscribe();
    unsubscribePlayback();
    hoveredHudControl = '';
    pressedHudControl = '';
    delete document.documentElement.dataset.nativeViewport;
    delete container.dataset.nativeViewport;
  };
}

/** Request a synchronous platform cutout refresh after a React layout effect
 * mounts or repositions a transient DOM island above the native viewport. */
export function requestNativeViewportLayout(): void {
  if (typeof document === 'undefined') return;
  document.dispatchEvent(new Event('nbcad:native-viewport-layout'));
}

function previewKey(preview: NativeViewportTransient): string {
  // Quantization avoids waking the native renderer for insignificant
  // float noise while preserving sub-micron precision in millimeter models.
  let hash = 2_166_136_261;
  let numericCount = 0;
  const addNumber = (value: number) => {
    hash ^= Math.round(value * 10_000);
    hash = Math.imul(hash, 16_777_619);
    numericCount += 1;
  };
  const addString = (value: string) => {
    for (let index = 0; index < value.length; index += 1) {
      hash ^= value.charCodeAt(index);
      hash = Math.imul(hash, 16_777_619);
    }
  };
  for (const layer of preview.lines) {
    layer.color.forEach(addNumber);
    addNumber(layer.width);
    addString(layer.pattern);
    layer.segments.forEach(addNumber);
  }
  for (const layer of preview.points) {
    layer.color.forEach(addNumber);
    addNumber(layer.radius);
    layer.positions.forEach(addNumber);
  }
  for (const layer of preview.triangles) {
    layer.color.forEach(addNumber);
    layer.positions.forEach(addNumber);
    addNumber(layer.xray ? 1 : 0);
  }
  for (const arrow of preview.arrows) {
    arrow.start.forEach(addNumber);
    arrow.end.forEach(addNumber);
    arrow.color.forEach(addNumber);
    addNumber(arrow.width);
    addNumber(arrow.xray ? 1 : 0);
  }
  for (const annotation of preview.annotations) {
    annotation.screen.forEach(addNumber);
    annotation.color.forEach(addNumber);
    addString(annotation.text);
    addString(annotation.kind);
    if (annotation.icon) addString(annotation.icon);
    if (annotation.toolIcon) addString(annotation.toolIcon);
    addNumber(annotation.selected ? 1 : 0);
  }
  preview.marker?.position.forEach(addNumber);
  if (preview.marker) addString(preview.marker.kind);
  return [
    preview.lines.length,
    preview.points.length,
    preview.triangles.length,
    preview.arrows.length,
    preview.annotations.length,
    numericCount,
    preview.marker ? 1 : 0,
    hash >>> 0,
  ].join(':');
}

function pumpPreview(): void {
  if (previewInFlight || !pendingPreview) return;
  const preview = pendingPreview;
  pendingPreview = null;
  previewInFlight = true;
  void invoke('native_viewport_set_preview', { preview })
    .catch(() => undefined)
    .finally(() => {
      previewInFlight = false;
      pumpPreview();
    });
}

/**
 * Sends only transient presentation geometry through IPC: tool previews,
 * dialog-owned highlights, point grips, and dimension/constraint annotations.
 * Committed sketches and OCCT meshes stay on the direct Rust path.
 */
export function syncNativeViewportPreview(preview: NativeViewportTransient): void {
  if (!active) return;
  const key = previewKey(preview);
  if (key === lastPreviewKey) return;
  lastPreviewKey = key;
  pendingPreview = preview;
  pumpPreview();
}

export function syncNativeViewportCamera(
  camera: {
    position: { toArray(): number[] };
    up: { toArray(): number[] };
    fov: number;
  },
  target: { toArray(): number[] },
): void {
  if (!active) return;
  const next: NativeCameraState = {
    position: camera.position.toArray() as [number, number, number],
    target: target.toArray() as [number, number, number],
    up: camera.up.toArray() as [number, number, number],
    verticalFovDegrees: camera.fov,
  };
  latestCamera = next;
  const key = [
    ...next.position,
    ...next.target,
    ...next.up,
    next.verticalFovDegrees,
  ]
    .map((value) => value.toFixed(4))
    .join(',');
  if (key === lastCameraKey) return;
  lastCameraKey = key;
  window.dispatchEvent(new CustomEvent('nbcad:camera-change'));
  pendingCamera = next;
  pumpCamera();
}

export async function pickNativeViewport(
  event: PointerEvent,
  container: HTMLElement,
  purpose: 'geometry' | 'jointConnector' = 'geometry',
): Promise<NativeViewportPick | null> {
  if (!active) return null;
  const rect = container.getBoundingClientRect();
  return invoke<NativeViewportPick | null>('native_viewport_pick', {
    x: event.clientX - rect.left,
    y: event.clientY - rect.top,
    camera: latestCamera,
    logicalWidth: rect.width,
    logicalHeight: rect.height,
    purpose,
  });
}
