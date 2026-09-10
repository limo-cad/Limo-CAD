/**
 * Global app state (zustand).
 *
 * Holds the application mode, ribbon state, browser-tree UI state, sketch
 * palette options, the document snapshot, and the live sketch-session
 * snapshot (entities/constraints mirrored from the engine after every op).
 * Sketch behavior itself lives in the Rust engine — this store only keeps
 * its latest DTO plus frontend-only interaction state (selection, hover,
 * active tool).
 */
import { create } from 'zustand';
import { presentation } from '../operationPlayback';
import { synchronizeSnapshotVisibility } from '../sessionSnapshot';
import type {
  AssemblyDocumentDto,
  AssemblySolutionDto,
  AssemblyTransformDto,
  BodyPoseDto,
  BodyAppearance,
  ComponentDefinitionDto,
  ComponentOccurrenceDto,
  CreateJointRequestDto,
  CylindricalSurfaceDto,
  DatumPlaneDefinitionDto,
  DatumPlaneUpdateDto,
  DrawingDocumentDto,
  DrawingViewKind,
  ExtrudeOperation,
  HoleBottomStyle,
  HoleStyle,
  HoleThreadDto,
  JointConnectorDto,
  JointDefinitionDto,
  JointMotionStateDto,
  MechanismPreviewDto,
  MotionStudyEvaluationDto,
  OriginPlane,
  PlanarFaceSourceDto,
  PlaneBasis,
  PlaneRef,
  Point3Dto,
  ProfileCatalogItemDto,
  ProfileLoopDto,
  ProfileRefDto,
  ProjectVisibilityDto,
  SketchDto,
  SketchPointRefDto,
  SolidSceneDto,
  SolidUpdateDto,
  UpdateJointRequestDto,
} from '../engine/types';
import { getEngine, type Engine } from '../engine';
import {
  DEFAULT_BODY_COLOR,
  DEFAULT_MATERIAL_NAME,
} from '../engine/types';
import {
  applyThemePreference,
  persistThemePreference,
  readThemePreference,
  resolveTheme,
  type ResolvedTheme,
  type ThemePreference,
} from '../theme';
import {
  persistSixDofSpeed,
  readSixDofSpeed,
} from '../navigationPreferences';
import type { BrowserNode, DocumentDto, NodeId } from '../types/document';
import { stageDatumPlanes, stageFinishedSketches } from '../engine/historyStage';
import { normalizeDrawingDocument } from '../drawing/sheet';
import type {
  ConstructionPlanePickTarget,
  ModelingPickTarget,
} from '../modeling/viewportPicker';

export type {
  ConstructionPlanePickTarget,
  ModelingPickTarget,
} from '../modeling/viewportPicker';

function emptyProjectVisibility(): ProjectVisibilityDto {
  return {
    hidden_body_ids: [],
    hidden_datum_plane_ids: [],
    hidden_sketch_names: [],
  };
}

export function constructionReferencesVisible(
  state: Pick<AppState, 'projectVisibility' | 'finishedSketches' | 'datumPlanes'>,
): boolean {
  const hiddenSketches = new Set(state.projectVisibility.hidden_sketch_names);
  const hiddenDatums = new Set(state.projectVisibility.hidden_datum_plane_ids);
  return state.finishedSketches.some(sketch => !hiddenSketches.has(sketch.name))
    || state.datumPlanes.some(plane => !hiddenDatums.has(plane.datum_id));
}

function persistedVisibilityFromHidden(
  document: DocumentDto | null,
  hidden: Record<NodeId, boolean>,
  retained: ProjectVisibilityDto,
): ProjectVisibilityDto {
  if (!document) return retained;
  // Browser nodes describe the evaluated stage. Preserve stable choices for
  // retained objects outside it, and update only identities represented here.
  const hiddenBodyIds = new Set(retained.hidden_body_ids);
  const hiddenDatumPlaneIds = new Set(retained.hidden_datum_plane_ids);
  const hiddenSketchNames = new Set(retained.hidden_sketch_names);
  const visit = (nodes: BrowserNode[]) => {
    for (const node of nodes) {
      if (node.kind === 'body' && node.reference_id !== null) {
        if (hidden[node.id]) hiddenBodyIds.add(node.reference_id);
        else hiddenBodyIds.delete(node.reference_id);
      } else if (node.kind === 'construction_plane' && node.reference_id !== null) {
        if (hidden[node.id]) hiddenDatumPlaneIds.add(node.reference_id);
        else hiddenDatumPlaneIds.delete(node.reference_id);
      } else if (node.kind === 'sketch' && node.name) {
        if (hidden[node.id]) hiddenSketchNames.add(node.name);
        else hiddenSketchNames.delete(node.name);
      }
      visit(node.children);
    }
  };
  visit(document.browser);
  return {
    hidden_body_ids: [...hiddenBodyIds].sort((a, b) => a - b),
    hidden_datum_plane_ids: [...hiddenDatumPlaneIds].sort((a, b) => a - b),
    hidden_sketch_names: [...hiddenSketchNames].sort(),
  };
}

function hiddenFromPersistedVisibility(
  document: DocumentDto,
  visibility: ProjectVisibilityDto,
): Record<NodeId, boolean> {
  const bodyIds = new Set(visibility.hidden_body_ids);
  const datumIds = new Set(visibility.hidden_datum_plane_ids);
  const sketchNames = new Set(visibility.hidden_sketch_names);
  const hidden: Record<NodeId, boolean> = {};
  const visit = (nodes: BrowserNode[]) => {
    for (const node of nodes) {
      const isHidden =
        (node.kind === 'body' && node.reference_id !== null && bodyIds.has(node.reference_id))
        || (node.kind === 'construction_plane' && node.reference_id !== null && datumIds.has(node.reference_id))
        || (node.kind === 'sketch' && node.name !== null && sketchNames.has(node.name));
      if (isHidden) hidden[node.id] = true;
      visit(node.children);
    }
  };
  visit(document.browser);
  return hidden;
}

function scrubAppearances(
  appearances: BodyAppearance[],
  bodies: { id: number }[],
): BodyAppearance[] {
  const live = new Set(bodies.map((body) => body.id));
  return appearances
    .filter((entry) => live.has(entry.body_id))
    .sort((a, b) => a.body_id - b.body_id);
}

function emptyDrawingDocument(): DrawingDocumentDto {
  return {
    sheets: [],
    active_sheet_id: null,
    next_sheet_id: 1,
    next_view_id: 1,
    next_annotation_id: 1,
    next_revision_id: 1,
    next_bom_item_id: 1,
    templates: [],
    next_template_id: 1,
  };
}

function emptyAssemblyDocument(): AssemblyDocumentDto {
  return {
    joints: [],
    next_joint_id: 1,
    grounded_body_id: null,
    component_structure: {
      definitions: [],
      occurrences: [],
      next_component_id: 1,
      next_occurrence_id: 1,
    },
    positions: [],
    next_position_id: 1,
    motion_studies: [],
    next_motion_study_id: 1,
    contact_sets: [],
    next_contact_set_id: 1,
  };
}

function emptyAssemblySolution(): AssemblySolutionDto {
  return {
    body_poses: [],
    occurrence_poses: [],
    instance_body_poses: [],
    diagnostics: [],
    solved: true,
  };
}

const IDENTITY_ASSEMBLY_TRANSFORM: AssemblyTransformDto = {
  translation: [0, 0, 0],
  rotation: [0, 0, 0, 1],
};

function jointMotionState(joint: JointDefinitionDto): JointMotionStateDto {
  return {
    joint_id: joint.id,
    angle_offset_deg: joint.angle_offset_deg,
    linear_offset_mm: joint.linear_offset_mm,
    secondary_angle_offset_deg: joint.advanced.secondary_angle_offset_deg,
    tertiary_angle_offset_deg: joint.advanced.tertiary_angle_offset_deg,
    secondary_linear_offset_mm: joint.advanced.secondary_linear_offset_mm,
  };
}

function jointConnectorIsLive(
  connector: JointConnectorDto,
  scene: SolidSceneDto,
): boolean {
  const body = scene.bodies.find((candidate) => candidate.id === connector.body_id);
  if (!body) return false;
  if (connector.kind === 'circular_edge') {
    const edge = body.edges.find((candidate) => candidate.id === connector.edge_id);
    return Boolean(edge?.circle?.closed && edge.key === connector.edge_key);
  }
  const face = body.faces.find((candidate) => candidate.id === connector.face_id);
  return Boolean((face?.plane || face?.cylinder) && face.key === connector.face_key);
}

function appearanceFor(
  appearances: BodyAppearance[],
  bodyId: number,
): BodyAppearance {
  return (
    appearances.find((entry) => entry.body_id === bodyId) ?? {
      body_id: bodyId,
      color: DEFAULT_BODY_COLOR,
      material_name: DEFAULT_MATERIAL_NAME,
      filament_type: 'PLA',
      brand: 'Generic',
      color_name: '',
      filament_id: null,
      preset_id: null,
      density_g_cm3: null,
      diameter_mm: 1.75,
    }
  );
}

function bodyBrowserNode(document: DocumentDto | null, bodyId: number | null): NodeId | null {
  if (!document || bodyId === null) return null;
  const stack = [...document.browser];
  while (stack.length > 0) {
    const node = stack.pop()!;
    if (node.kind === 'body' && node.reference_id === bodyId) return node.id;
    stack.push(...node.children);
  }
  return null;
}

/**
 * App modes:
 * - `solid`: default modeling mode.
 * - `pickPlane`: Create Sketch is armed; origin planes are pickable in the
 *   viewport (and browser), Esc cancels.
 * - `sketch`: a sketch session is active in the engine.
 */
export type AppMode = 'solid' | 'pickPlane' | 'sketch';

/** Lightweight window-level document metadata. The active OCCT/Bevy model
 * remains in the engine; inactive model snapshots and native save targets are
 * intentionally kept outside the inspectable UI store. */
export interface ProjectTabSummary {
  id: string;
  name: string;
  fileName: string | null;
  dirty: boolean;
  /** Workspace stage (solid/drawing) the project was last viewed in;
   * restored on tab activation so each project keeps its own stage. */
  workspaceTab: string;
}

/** Sketch tools (M1a–M1c). `null` = select/edit. */
export type SketchTool =
  | 'line'
  | 'midpointLine'
  | 'point'
  | 'rect2pt'
  | 'rectCenter'
  | 'circleCenter'
  | 'circle2pt'
  | 'arc3pt'
  | 'arcCenter'
  | 'dimension'
  | 'fillet'
  | 'chamfer'
  | 'offset'
  | 'trim'
  | 'extend'
  | 'break'
  | 'mirror'
  | 'moveCopy'
  | 'scale'
  | 'polygon'
  | 'slot'
  | 'splineFit'
  | null;

/**
 * Modal navigation tools. `select` = normal left-button
 * behavior (sketch tools/selection). The others capture left-drag:
 * orbit/pan/zoom apply continuously, zoomWindow frames a dragged rect.
 */
export type NavTool = 'select' | 'orbit' | 'pan' | 'zoom' | 'zoomWindow';

/** Modeless tool active in the technical drawing workspace. */
export type DrawingTool =
  | 'place_view'
  | 'dimension'
  | 'diameter'
  | 'radius'
  | 'angle'
  | 'hole_note'
  | 'center_mark'
  | 'center_line'
  | 'symmetry_axis'
  | 'bolt_circle'
  | 'chain_dimension'
  | 'baseline_dimension'
  | 'continued_dimension'
  | 'ordinate_dimension'
  | 'arc_length'
  | 'jogged_radius'
  | 'section_view'
  | 'detail_view'
  | 'auxiliary_view'
  | 'broken_view'
  | 'removed_section'
  | 'datum'
  | 'gdt'
  | 'surface_texture'
  | 'edge_requirement'
  | 'weld'
  | 'balloon'
  | 'revision_cloud'
  | 'reassociate'
  | 'chamfer_note'
  | 'note'
  | null;

/** Sketch palette option keys (labels live in i18n under palette.*). */
export const PALETTE_OPTION_KEYS = [
  'linetype',
  'lookAt',
  'sketchGrid',
  'snap',
  'slice',
  'profile',
  'points',
  'dimensions',
  'constraints',
  'projectedGeometries',
  'constructionGeometries',
  'threeDSketch',
] as const;

export type PaletteOptionKey = (typeof PALETTE_OPTION_KEYS)[number];

/** One dynamic-input field (length / angle / width / height / diameter). */
export interface DynField {
  key: string;
  /** Currently shown text (live-computed or user-typed). */
  value: string;
  /** User typed a value → locked (the rubber band respects it). */
  locked: boolean;
  /** Per-key visibility (angle hides on axis-aligned segments). */
  visible: boolean;
}

/** Floating dynamic-input cluster state (next to the cursor). */
export interface DynInput {
  active: boolean;
  /** Viewport-relative px position of the cluster. */
  x: number;
  y: number;
  fields: DynField[];
  /** Index of the keyboard-focused field, null when none. */
  focus: number | null;
  /** The focused field's complete value is selected for replacement. */
  selectAll: boolean;
  /** Debounce pending (~200 ms live preview while typing, D10). */
  pending: boolean;
}

/** Modal shown for invalid constraint combos and D4.2 relation rejection. */
export interface ConstraintDialog {
  titleKey: string;
  message: string;
  /** Structured D4.2 conflict/redundancy report when available. */
  conflicts?: {
    reason?: 'conflict' | 'redundant';
    rejected: { kind: string; entities: Array<{ label: string }> };
    conflicts_with: Array<{ kind: string; entities: Array<{ label: string }> }>;
  };
}

export interface FinishedSketchLineRef {
  sketchName: string;
  entityId: number;
}

export interface FinishedSketchCurveRef {
  sketchName: string;
  entityId: number;
}

export type FinishedSketchPointPick = SketchPointRefDto & {
  world: Point3Dto;
};

export type SolidProfilePickOwner = 'extrude' | 'revolve' | 'sweep' | 'loft';
export type SolidCurvePickOwner =
  | 'sweep_path'
  | 'sweep_guide'
  | 'loft_centerline'
  | 'loft_guide'
  | 'rib_centerline';

export interface SolidProfilePicker {
  owner: SolidProfilePickOwner;
  catalog: ProfileCatalogItemDto[];
  selected: ProfileRefDto[];
  hovered: ProfileRefDto | null;
  sketchName: string;
}

export interface SolidCurvePicker {
  owner: SolidCurvePickOwner;
  catalog: ProfileCatalogItemDto[];
  selected: FinishedSketchCurveRef[];
  /** Selections retained by every field in the currently open command. The
   * active `selected` list remains the edit target; this map lets the shared
   * viewport keep completed path/guide fields visibly selected. */
  selectionsByOwner: Partial<
    Record<SolidCurvePickOwner, FinishedSketchCurveRef[]>
  >;
  hovered: FinishedSketchCurveRef | null;
  sketchName: string;
}

/**
 * Debounced, presentation-only solid command geometry. The profile basis and
 * offsets are the same values used to build the kernel request, so native
 * previews cannot silently rotate away from the eventual OCCT operation.
 */
export interface ExtrudeCommandPreview {
  kind: 'extrude';
  basis: PlaneBasis;
  /** Exact kernel source identity. Preview geometry may use its tessellation,
   * but the committed feature resolves the original OCCT TopoDS_Face. */
  sourceFace: PlanarFaceSourceDto | null;
  profiles: ProfileLoopDto[];
  selectedProfileIndices: number[];
  startOffset: number;
  endOffset: number;
  /** Signed arrow-tip offset measured from the sketch plane. */
  directionOffset: number;
  operation: ExtrudeOperation;
}

/** Presentation-only construction plane used while editing an offset. */
export interface OffsetPlaneCommandPreview {
  kind: 'offset_plane';
  basis: PlaneBasis;
  distance: number;
  /** Half width/height in the basis u/v directions, in millimetres. */
  halfSize: [number, number];
}

/** Presentation-only hole cutter. Positions use the exact support-face basis
 * submitted to OCCT so flip direction and stepped/countersunk geometry can be
 * inspected before committing the feature. */
export interface HoleCommandPreview {
  kind: 'hole';
  bodyId: number;
  basis: PlaneBasis;
  positions: { x: number; y: number }[];
  diameter: number;
  /** Null means Through All and is resolved against the retained body mesh. */
  depth: number | null;
  style: HoleStyle;
  counterboreDiameter: number;
  counterboreDepth: number;
  countersinkDiameter: number;
  countersinkAngleDeg: number;
  bottomStyle: HoleBottomStyle;
  drillPointAngleDeg: number;
  flip: boolean;
}

/** Presentation-only male thread on an exact analytic cylindrical face. */
export interface ExternalThreadCommandPreview {
  kind: 'external_thread';
  bodyId: number;
  faceId: number;
  cylinder: CylindricalSurfaceDto;
  thread: HoleThreadDto;
  flip: boolean;
}

export interface MoveCopyPreviewTarget {
  bodyId: number;
  /** Disambiguates reusable instances that share one source body. */
  occurrenceId: number | null;
  /** Existing display pose before the command delta is applied. */
  baseTranslation: [number, number, number];
  baseRotation: [number, number, number, number];
}

export interface MoveCopyGizmoInteraction {
  kind: 'translate' | 'rotate';
  axis: 0 | 1 | 2;
  active: boolean;
}

/** Rigid-transform ghost and original six-axis viewport manipulator. */
export interface MoveCopyCommandPreview {
  kind: 'move_copy';
  targets: MoveCopyPreviewTarget[];
  pivot: Point3Dto;
  translation: Point3Dto;
  rotation: [number, number, number, number];
  copy: boolean;
  /** Body features transform model geometry before occurrence placement;
   * component moves transform the already placed occurrence. */
  transformInBodySpace: boolean;
  /** The full six-axis control belongs to Free Move only. */
  showSixAxisGizmo: boolean;
  /** Display-space origin and orientation of the same coordinate frame the
   * command transform uses. These keep the visible rings, pointer targets,
   * and actual rotation axis coincident for already-placed bodies. */
  gizmoPivot: Point3Dto;
  gizmoOrientation: [number, number, number, number];
  /** Hover/drag state is presentation-only and never enters feature history. */
  gizmoInteraction: MoveCopyGizmoInteraction | null;
}

export type SolidCommandPreview =
  | ExtrudeCommandPreview
  | HoleCommandPreview
  | ExternalThreadCommandPreview
  | OffsetPlaneCommandPreview
  | MoveCopyCommandPreview;

export type ConstructionPlaneKind = 'offset' | 'midplane' | 'at_angle';
export interface ConstructionPlanePickedEdge {
  bodyId: number;
  edgeId: number;
}

export type BodyFeatureKind =
  | 'move_copy'
  | 'external_thread'
  | 'shell'
  | 'mirror'
  | 'rectangular_pattern'
  | 'circular_pattern'
  | 'combine'
  | 'split_body';

export type SolidSelectionKind = 'body' | 'face' | 'edge';

/**
 * A transient timeline-edit checkpoint. The displayed scene is evaluated
 * immediately before `featureId`; completing or cancelling the editor
 * returns the build cursor to `restoreRollbackIndex`.
 */
export interface HistoryEditCheckpoint {
  featureId: number;
  restoreRollbackIndex: number;
  /** Dirty state before the temporary history stage was displayed. */
  restoreDirty: boolean;
}

/** Default check states for the sketch palette. */
const DEFAULT_PALETTE: Record<PaletteOptionKey, boolean> = {
  linetype: false,
  lookAt: false,
  sketchGrid: true,
  snap: true,
  slice: false,
  profile: true,
  points: true,
  dimensions: true,
  constraints: true,
  projectedGeometries: true,
  constructionGeometries: true,
  threeDSketch: false,
};

const INITIAL_THEME_PREFERENCE = readThemePreference();
const INITIAL_RESOLVED_THEME = resolveTheme(INITIAL_THEME_PREFERENCE);
const INITIAL_SIX_DOF_SPEED = readSixDofSpeed();

export interface AppState {
  mode: AppMode;
  /** Active ribbon tab id ('solid', 'sketch', ...). */
  activeTab: string;
  document: DocumentDto | null;
  /** Unsaved authoritative model changes (selection/camera are excluded). */
  dirty: boolean;
  /** Current project file name, without exposing the native path. */
  projectFileName: string | null;
  /** Open documents in this application window. */
  projectTabs: ProjectTabSummary[];
  /** Tab whose model is currently hydrated into the single native engine. */
  activeProjectTabId: string | null;
  /** Which engine host the frontend is talking to (D8). */
  engineKind: 'tauri' | 'wasm' | null;
  /** Live snapshot of the active sketch session (null outside sketch mode). */
  activeSketch: SketchDto | null;
  /** Snapshots of finished sketches (M1d): rendered muted in 3D solid mode. */
  finishedSketches: SketchDto[];
  /** Recomputed OCCT tessellation and stable topology table. */
  solidScene: SolidSceneDto;
  datumPlanes: DatumPlaneDefinitionDto[];
  /** Plane currently hovered in pick-plane mode (viewport or browser). */
  hoveredPlane: OriginPlane | null;
  /** User-defined datum plane currently hovered while choosing sketch support. */
  hoveredDatumPlane: number | null;
  activeTool: SketchTool;
  /** Active modal nav tool (`select` = normal left-drag behavior). */
  navTool: NavTool;
  /** Frontend-side selection/hover bookkeeping (engine only stores geometry). */
  selectedEntity: number | null;
  /** Multi-select set for constraint application (shift/ctrl-click). */
  selectedEntities: number[];
  /** Two-entity relation waiting for viewport picks. */
  pendingConstraintTool: string | null;
  /** Selected dimension (constraint id) for edit/delete/drag (D9). */
  selectedDimension: number | null;
  /** Selected geometric constraint for highlight/delete. */
  selectedConstraint: number | null;
  hoveredEntity: number | null;
  /** Inline dimension editor state (double-click a dimension). */
  dimEditor: { dimId: number; initial: string; x: number; y: number } | null;
  /** Show the live "DOF: N" chip in the viewport (D4.3 optional display). */
  showDof: boolean;
  /** Shared translation/rotation multiplier for raw/native 3D mouse input. */
  sixDofSpeed: number;
  /** Global appearance preference; System is the first-run/default value. */
  themePreference: ThemePreference;
  resolvedTheme: ResolvedTheme;
  settingsOpen: boolean;
  /** Polygon creation mode (from the ribbon payload). */
  polygonMode: 'inscribed' | 'circumscribed';
  /** Slot creation mode selected by the ribbon payload. */
  slotMode: 'centerToCenter' | 'overall' | 'centerPoint';
  dynInput: DynInput;
  constraintDialog: ConstraintDialog | null;
  /** Incremented to request a camera "Look At" snap (Sketch Palette). */
  lookAtNonce: number;
  /** Browser nodes explicitly expanded (default: collapsed). */
  expanded: Record<NodeId, boolean>;
  /** Browser nodes explicitly hidden via the eye toggle (default: visible). */
  hidden: Record<NodeId, boolean>;
  /** Stable project representation of Browser visibility for save/tab state. */
  projectVisibility: ProjectVisibilityDto;
  selectedNode: NodeId | null;
  selectedBody: number | null;
  /** Explicit solid-body selections. `selectedBody` remains the active owner. */
  selectedBodies: number[];
  /** Per-body color/material from the project model. */
  bodyAppearances: BodyAppearance[];
  /** Persistent technical drawing sheets and projected-view definitions. */
  drawingDocument: DrawingDocumentDto;
  /** Persistent face-referenced assembly/joint intent. */
  assemblyDocument: AssemblyDocumentDto;
  /** Monotonic marker for assembly snapshots hydrated as part of a solid
   * recompute. Application history uses it to keep that synchronization in
   * the owning solid command instead of creating a second assembly command. */
  assemblySolidSyncRevision: number;
  /** Derived rigid poses; never baked into OCCT or project feature history. */
  assemblySolution: AssemblySolutionDto;
  /** Non-persistent alignment preview for the open joint command. */
  jointPreviewSolution: AssemblySolutionDto | null;
  /** Non-persistent solved pose from animation/direct joint manipulation. */
  jointMotionPreview: {
    jointId: number;
    angleOffsetDeg: number;
    linearOffsetMm: number;
    secondaryAngleOffsetDeg: number;
    tertiaryAngleOffsetDeg: number;
    secondaryLinearOffsetMm: number;
    motion: JointMotionStateDto;
    solution: AssemblySolutionDto;
  } | null;
  /** Transient multi-joint inverse-kinematics result during component drag. */
  mechanismPreview: MechanismPreviewDto | null;
  /** Transient solved timeline sample; never mutates joint design positions. */
  motionStudyPreview: MotionStudyEvaluationDto | null;
  /** Active assembly instance. Bodies remain part-definition selections. */
  selectedOccurrenceId: number | null;
  /** Exact reusable instance under the pointer; topology ids repeat by definition. */
  hoveredOccurrenceId: number | null;
  selectedJointId: number | null;
  /** Joint being edited; null means the dialog is creating a new joint. */
  jointEditingId: number | null;
  jointDialogOpen: boolean;
  /** Model-local connector frames captured by the joint picker. */
  jointConnectorPicks: JointConnectorDto[];
  jointConnectorHover: JointConnectorDto | null;
  /** Assembly is a Solid Modeling sub-function, not a top-level workspace. */
  solidSidebarMode: 'model' | 'assembly';
  selectedDrawingViewId: number | null;
  selectedDrawingAnnotationId: number | null;
  drawingTool: DrawingTool;
  drawingPendingViewKind: DrawingViewKind | null;
  drawingSheetSetupOpen: boolean;
  drawingProfileExportOpen: boolean;
  selectedFace: number | null;
  /** Stable Face IDs selected with Shift/Ctrl/Cmd. */
  selectedFaces: number[];
  hoveredFace: number | null;
  selectedFacePoint: Point3Dto | null;
  /** Exact cursor point shown while a modeling point picker is active. */
  modelingPointHover: Point3Dto | null;
  /** Face awaiting the sketch-coordinate-origin choice. */
  sketchPlaneFace: number | null;
  selectedEdges: number[];
  hoveredEdge: number | null;
  /** Modeless region selector shared by sketch-driven solid dialogs. */
  profilePicker: SolidProfilePicker | null;
  /** Modeless finished-curve selector shared by path-driven solid dialogs. */
  curvePicker: SolidCurvePicker | null;
  /** Native translucent tool-volume preview owned by the open solid command. */
  solidCommandPreview: SolidCommandPreview | null;
  /** null = closed; 0 = new Extrude; positive = edit feature id. */
  extrudeDialogFeature: number | null;
  /** null = closed; 0 = new Revolve; positive = edit feature id. */
  revolveDialogFeature: number | null;
  /** Viewport-picked stable sketch line used by the open Revolve dialog. */
  revolveAxisSelection: FinishedSketchLineRef | null;
  revolveAxisHover: FinishedSketchLineRef | null;
  sweepDialogFeature: number | null;
  loftDialogFeature: number | null;
  ribDialogFeature: number | null;
  filletDialogFeature: number | null;
  chamferDialogFeature: number | null;
  holeDialogFeature: number | null;
  /** Associative sketch points used by the open multi-position Hole dialog. */
  holePositionSelections: FinishedSketchPointPick[];
  holePositionHover: FinishedSketchPointPick | null;
  constructionPlaneDialog: { kind: ConstructionPlaneKind; featureId: number } | null;
  /** Active viewport role for the construction-plane dialog. */
  constructionPlanePickTarget: ConstructionPlanePickTarget;
  constructionPlanePickedReference: PlaneRef | null;
  constructionPlanePickedEdge: ConstructionPlanePickedEdge | null;
  /** Exactly one modeling geometry field owns viewport input at a time. */
  modelingPickTarget: ModelingPickTarget | null;
  /** Generic plane result for Mirror and Split Body viewport picking. */
  modelingPlaneSelection: PlaneRef | null;
  bodyFeatureDialog: { kind: BodyFeatureKind; featureId: number } | null;
  /** Non-persistent history editing boundary for timeline feature editors. */
  historyEdit: HistoryEditCheckpoint | null;
  sketchPatternDialog: 'rectangular' | 'circular' | null;
  /** File chooser/export operation that must keep the active tab stable. */
  projectBusy: boolean;
  solidBusy: boolean;
  palette: Record<PaletteOptionKey, boolean>;

  setMode: (mode: AppMode) => void;
  setActiveTab: (tab: string) => void;
  loadDocument: () => Promise<void>;
  /** Refresh live engine state after MCP inbox apply without clearing dirty. */
  refreshAfterInboxApply: (opName?: string) => Promise<void>;
  setDocument: (doc: DocumentDto) => void;
  setActiveSketch: (sketch: SketchDto | null) => void;
  setFinishedSketches: (sketches: SketchDto[]) => void;
  setSolidScene: (scene: SolidSceneDto) => void;
  applySolidUpdate: (update: SolidUpdateDto) => void;
  applyDatumPlaneUpdate: (update: DatumPlaneUpdateDto) => void;
  setBodyAppearances: (appearances: BodyAppearance[]) => void;
  setBodyAppearance: (appearance: BodyAppearance) => Promise<void>;
  setDrawingDocument: (drawing: DrawingDocumentDto) => Promise<void>;
  createComponent: (name: string, bodyIds: number[]) => Promise<void>;
  updateComponent: (component: ComponentDefinitionDto) => Promise<void>;
  createOccurrence: (
    componentId: number,
    parentOccurrenceId?: number | null,
    name?: string,
  ) => Promise<void>;
  updateOccurrence: (occurrence: ComponentOccurrenceDto) => Promise<void>;
  duplicateOccurrence: (
    occurrenceId: number,
    parentOccurrenceId?: number | null,
  ) => Promise<void>;
  moveCopyOccurrence: (
    occurrenceId: number,
    localPose: AssemblyTransformDto,
    copy: boolean,
  ) => Promise<void>;
  setOccurrenceGrounded: (occurrenceId: number, grounded: boolean) => Promise<void>;
  setOccurrencePose: (occurrenceId: number, localPose: AssemblyTransformDto) => Promise<void>;
  setSelectedOccurrenceId: (occurrenceId: number | null) => void;
  setHoveredOccurrenceId: (occurrenceId: number | null) => void;
  createJoint: (request: CreateJointRequestDto) => Promise<void>;
  updateJoint: (request: UpdateJointRequestDto) => Promise<void>;
  previewJoint: (request: CreateJointRequestDto) => Promise<void>;
  previewJointUpdate: (request: UpdateJointRequestDto) => Promise<void>;
  clearJointPreview: () => void;
  deleteJoint: (id: number) => Promise<void>;
  setJointMotion: (
    jointId: number,
    angleOffsetDeg: number,
    linearOffsetMm: number,
  ) => Promise<void>;
  previewJointMotion: (
    jointId: number,
    angleOffsetDeg: number,
    linearOffsetMm: number,
  ) => Promise<void>;
  previewJointCoordinates: (motion: JointMotionStateDto) => Promise<void>;
  clearJointMotionPreview: () => void;
  captureJointPosition: () => Promise<void>;
  setJointEnabled: (jointId: number, enabled: boolean) => Promise<void>;
  previewMechanismDrag: (
    bodyId: number,
    targetPose: BodyPoseDto,
    occurrenceId?: number | null,
    grabPointLocal?: [number, number, number] | null,
    targetPointWorld?: [number, number, number] | null,
    initialJointMotions?: JointMotionStateDto[],
    maximumIterations?: number,
  ) => Promise<void>;
  clearMechanismPreview: () => void;
  captureMechanismPosition: () => Promise<void>;
  setGroundedBody: (bodyId: number | null) => Promise<void>;
  setSelectedJointId: (id: number | null) => void;
  setJointDialogOpen: (open: boolean) => void;
  openJointEditor: (jointId: number) => void;
  toggleJointConnectorPick: (pick: JointConnectorDto) => void;
  clearJointConnectorPicks: () => void;
  setJointConnectorHover: (pick: JointConnectorDto | null) => void;
  setSolidSidebarMode: (mode: 'model' | 'assembly') => void;
  setSelectedDrawingViewId: (viewId: number | null) => void;
  setSelectedDrawingAnnotationId: (annotationId: number | null) => void;
  setDrawingTool: (tool: DrawingTool) => void;
  setDrawingPendingViewKind: (kind: DrawingViewKind | null) => void;
  setDrawingSheetSetupOpen: (open: boolean) => void;
  setDrawingProfileExportOpen: (open: boolean) => void;
  loadProjectState: (
    update: SolidUpdateDto,
    finishedSketches: SketchDto[],
    datumPlanes: DatumPlaneDefinitionDto[],
    fileName: string | null,
    bodyAppearances?: BodyAppearance[],
    drawingDocument?: DrawingDocumentDto,
    assemblyDocument?: AssemblyDocumentDto,
    projectVisibility?: ProjectVisibilityDto,
    assemblySolution?: AssemblySolutionDto,
  ) => void;
  markClean: (fileName?: string | null) => void;
  markDirty: () => void;
  setHoveredPlane: (plane: OriginPlane | null) => void;
  setHoveredDatumPlane: (datumId: number | null) => void;
  setActiveTool: (tool: SketchTool) => void;
  setNavTool: (tool: NavTool) => void;
  setSelectedEntity: (id: number | null) => void;
  setSelectedEntities: (ids: number[]) => void;
  setPendingConstraintTool: (tool: string | null) => void;
  setSelectedDimension: (id: number | null) => void;
  setSelectedConstraint: (id: number | null) => void;
  setDimEditor: (editor: { dimId: number; initial: string; x: number; y: number } | null) => void;
  setHoveredEntity: (id: number | null) => void;
  setShowDof: (show: boolean) => void;
  setSixDofSpeed: (speed: number) => void;
  setThemePreference: (preference: ThemePreference) => void;
  syncResolvedTheme: () => void;
  setSettingsOpen: (open: boolean) => void;
  setPolygonMode: (mode: 'inscribed' | 'circumscribed') => void;
  setSlotMode: (mode: 'centerToCenter' | 'overall' | 'centerPoint') => void;
  showDynInput: (fieldKeys: string[], x: number, y: number) => void;
  /** Live-update unlocked fields + cluster position (per pointer move). */
  updateDynInput: (values: Record<string, string>, visible: Record<string, boolean>, x: number, y: number) => void;
  setDynField: (key: string, value: string, locked: boolean) => void;
  setDynFocus: (index: number | null, selectAll?: boolean) => void;
  setDynPending: (pending: boolean) => void;
  hideDynInput: () => void;
  clearDynLocks: () => void;
  setConstraintDialog: (dialog: ConstraintDialog | null) => void;
  requestLookAt: () => void;
  toggleExpanded: (id: NodeId) => void;
  toggleHidden: (id: NodeId) => void;
  applyProjectVisibility: (visibility: ProjectVisibilityDto) => void;
  selectNode: (id: NodeId | null) => void;
  setSelectedBody: (id: number | null) => void;
  /** Replace an ordered body selection; index 0 is the primary/target role. */
  replaceSelectedBodies: (ids: number[]) => void;
  /** Replace the selected faces for one owning body. */
  replaceSelectedFaces: (bodyId: number, ids: number[]) => void;
  setSelectedFace: (id: number | null) => void;
  setHoveredFace: (id: number | null) => void;
  setSelectedFacePoint: (point: Point3Dto | null) => void;
  setModelingPointHover: (point: Point3Dto | null) => void;
  openSketchPlaneOrigin: (faceId: number) => void;
  closeSketchPlaneOrigin: () => void;
  setSelectedEdges: (ids: number[]) => void;
  /** Replace or modifier-toggle one solid topology feature atomically. */
  selectSolidFeature: (
    kind: SolidSelectionKind,
    bodyId: number,
    featureId: number,
    point?: Point3Dto | null,
    additive?: boolean,
  ) => void;
  clearSolidSelection: () => void;
  setHoveredEdge: (id: number | null) => void;
  configureProfilePicker: (
    owner: SolidProfilePickOwner,
    catalog: ProfileCatalogItemDto[],
    selected: ProfileRefDto[],
    sketchName: string,
  ) => void;
  replaceProfilePicks: (
    owner: SolidProfilePickOwner,
    selected: ProfileRefDto[],
    sketchName?: string,
  ) => void;
  toggleProfilePick: (profile: ProfileRefDto) => void;
  setHoveredProfilePick: (profile: ProfileRefDto | null) => void;
  clearProfilePicker: (owner?: SolidProfilePickOwner) => void;
  configureCurvePicker: (
    owner: SolidCurvePickOwner,
    catalog: ProfileCatalogItemDto[],
    selected: FinishedSketchCurveRef[],
    sketchName: string,
  ) => void;
  toggleCurvePick: (curve: FinishedSketchCurveRef) => void;
  replaceCurvePicks: (
    owner: SolidCurvePickOwner,
    selected: FinishedSketchCurveRef[],
    sketchName?: string,
  ) => void;
  setHoveredCurvePick: (curve: FinishedSketchCurveRef | null) => void;
  clearCurvePicker: (owner?: SolidCurvePickOwner) => void;
  setSolidCommandPreview: (preview: SolidCommandPreview | null) => void;
  openExtrudeDialog: (featureId?: number) => void;
  closeExtrudeDialog: () => void;
  openRevolveDialog: (featureId?: number) => void;
  closeRevolveDialog: () => void;
  setRevolveAxisSelection: (selection: FinishedSketchLineRef | null) => void;
  setRevolveAxisHover: (selection: FinishedSketchLineRef | null) => void;
  openSweepDialog: (featureId?: number) => void;
  closeSweepDialog: () => void;
  openLoftDialog: (featureId?: number) => void;
  closeLoftDialog: () => void;
  openRibDialog: (featureId?: number) => void;
  closeRibDialog: () => void;
  openFilletDialog: (featureId?: number) => void;
  closeFilletDialog: () => void;
  openChamferDialog: (featureId?: number) => void;
  closeChamferDialog: () => void;
  openHoleDialog: (featureId?: number) => void;
  closeHoleDialog: () => void;
  setHolePositionSelections: (selections: FinishedSketchPointPick[]) => void;
  toggleHolePositionSelection: (selection: FinishedSketchPointPick) => void;
  setHolePositionHover: (selection: FinishedSketchPointPick | null) => void;
  openConstructionPlaneDialog: (kind: ConstructionPlaneKind, featureId?: number) => void;
  closeConstructionPlaneDialog: () => void;
  setConstructionPlanePickTarget: (target: ConstructionPlanePickTarget) => void;
  setConstructionPlanePickedReference: (reference: PlaneRef) => void;
  setConstructionPlanePickedEdge: (edge: ConstructionPlanePickedEdge) => void;
  setModelingPickTarget: (target: ModelingPickTarget | null) => void;
  setModelingPlaneSelection: (reference: PlaneRef | null) => void;
  openBodyFeatureDialog: (kind: BodyFeatureKind, featureId?: number) => void;
  closeBodyFeatureDialog: () => void;
  setHistoryEdit: (checkpoint: HistoryEditCheckpoint | null) => void;
  openSketchPatternDialog: (kind: 'rectangular' | 'circular') => void;
  closeSketchPatternDialog: () => void;
  setProjectBusy: (busy: boolean) => void;
  setSolidBusy: (busy: boolean) => void;
  setPaletteOption: (key: PaletteOptionKey, value: boolean) => void;
}

/** Clear document-owned interaction state while preserving app preferences. */
function resetDocumentUiState(): Partial<AppState> {
  return {
    mode: 'solid',
    activeTab: 'solid',
    activeSketch: null,
    finishedSketches: [],
    solidScene: { bodies: [], errors: [] },
    datumPlanes: [],
    hoveredPlane: null,
    hoveredDatumPlane: null,
    activeTool: null,
    navTool: 'select',
    selectedEntity: null,
    selectedEntities: [],
    pendingConstraintTool: null,
    selectedDimension: null,
    selectedConstraint: null,
    dimEditor: null,
    hoveredEntity: null,
    dynInput: {
      active: false,
      x: 0,
      y: 0,
      fields: [],
      focus: null,
      selectAll: false,
      pending: false,
    },
    constraintDialog: null,
    lookAtNonce: 0,
    expanded: {},
    hidden: {},
    projectVisibility: emptyProjectVisibility(),
    selectedNode: null,
    selectedBody: null,
    selectedBodies: [],
    bodyAppearances: [],
    drawingDocument: emptyDrawingDocument(),
    assemblyDocument: emptyAssemblyDocument(),
    assemblySolidSyncRevision: 0,
    assemblySolution: emptyAssemblySolution(),
    jointPreviewSolution: null,
    jointMotionPreview: null,
    mechanismPreview: null,
    motionStudyPreview: null,
    selectedOccurrenceId: null,
    hoveredOccurrenceId: null,
    selectedJointId: null,
    jointEditingId: null,
    jointDialogOpen: false,
    jointConnectorPicks: [],
    jointConnectorHover: null,
    solidSidebarMode: 'model',
    selectedDrawingViewId: null,
    selectedDrawingAnnotationId: null,
    drawingTool: null,
    drawingPendingViewKind: null,
    drawingSheetSetupOpen: false,
    drawingProfileExportOpen: false,
    selectedFace: null,
    selectedFaces: [],
    hoveredFace: null,
    selectedFacePoint: null,
    modelingPointHover: null,
    sketchPlaneFace: null,
    selectedEdges: [],
    hoveredEdge: null,
    profilePicker: null,
    curvePicker: null,
    solidCommandPreview: null,
    extrudeDialogFeature: null,
    revolveDialogFeature: null,
    revolveAxisSelection: null,
    revolveAxisHover: null,
    sweepDialogFeature: null,
    loftDialogFeature: null,
    ribDialogFeature: null,
    filletDialogFeature: null,
    chamferDialogFeature: null,
    holeDialogFeature: null,
    holePositionSelections: [],
    holePositionHover: null,
    constructionPlaneDialog: null,
    constructionPlanePickTarget: null,
    constructionPlanePickedReference: null,
    constructionPlanePickedEdge: null,
    modelingPickTarget: null,
    modelingPlaneSelection: null,
    bodyFeatureDialog: null,
    historyEdit: null,
    sketchPatternDialog: null,
    solidBusy: false,
  };
}

let jointPreviewGeneration = 0;
let jointMotionPreviewGeneration = 0;
let mechanismPreviewGeneration = 0;
interface MechanismPreviewJob {
  generation: number;
  bodyId: number;
  occurrenceId: number | null;
  targetPose: BodyPoseDto;
  grabPointLocal: [number, number, number] | null;
  targetPointWorld: [number, number, number] | null;
  initialJointMotions: JointMotionStateDto[];
  maximumIterations: number;
}
let pendingMechanismPreview: MechanismPreviewJob | null = null;
let mechanismPreviewPump: Promise<void> | null = null;

// Async engine work can be superseded while it is awaited. Reading through a
// function prevents TypeScript from incorrectly treating the shared queue as
// permanently null after the assignment immediately before that await.
const queuedMechanismPreview = () => pendingMechanismPreview;

export const useAppStore = create<AppState>()((set) => ({
  mode: 'solid',
  activeTab: 'solid',
  document: null,
  dirty: false,
  projectFileName: null,
  projectTabs: [],
  activeProjectTabId: null,
  engineKind: null,
  activeSketch: null,
  finishedSketches: [],
  solidScene: { bodies: [], errors: [] },
  datumPlanes: [],
  hoveredPlane: null,
  hoveredDatumPlane: null,
  activeTool: null,
  navTool: 'select',
  selectedEntity: null,
  selectedEntities: [],
  pendingConstraintTool: null,
  selectedDimension: null,
  selectedConstraint: null,
  dimEditor: null,
  hoveredEntity: null,
  showDof: false,
  sixDofSpeed: INITIAL_SIX_DOF_SPEED,
  themePreference: INITIAL_THEME_PREFERENCE,
  resolvedTheme: INITIAL_RESOLVED_THEME,
  settingsOpen: false,
  polygonMode: 'circumscribed',
  slotMode: 'centerToCenter',
  lookAtNonce: 0,
  expanded: {},
  hidden: {},
  projectVisibility: emptyProjectVisibility(),
  selectedNode: null,
  selectedBody: null,
  selectedBodies: [],
  bodyAppearances: [],
  drawingDocument: emptyDrawingDocument(),
  assemblyDocument: emptyAssemblyDocument(),
  assemblySolidSyncRevision: 0,
  assemblySolution: emptyAssemblySolution(),
  jointPreviewSolution: null,
  jointMotionPreview: null,
  mechanismPreview: null,
  motionStudyPreview: null,
  selectedOccurrenceId: null,
  hoveredOccurrenceId: null,
  selectedJointId: null,
  jointEditingId: null,
  jointDialogOpen: false,
  jointConnectorPicks: [],
  jointConnectorHover: null,
  solidSidebarMode: 'model',
  selectedDrawingViewId: null,
  selectedDrawingAnnotationId: null,
  drawingTool: null,
  drawingPendingViewKind: null,
  drawingSheetSetupOpen: false,
  drawingProfileExportOpen: false,
  selectedFace: null,
  selectedFaces: [],
  hoveredFace: null,
  selectedFacePoint: null,
  modelingPointHover: null,
  sketchPlaneFace: null,
  selectedEdges: [],
  hoveredEdge: null,
  profilePicker: null,
  curvePicker: null,
  solidCommandPreview: null,
  extrudeDialogFeature: null,
  revolveDialogFeature: null,
  revolveAxisSelection: null,
  revolveAxisHover: null,
  sweepDialogFeature: null,
  loftDialogFeature: null,
  ribDialogFeature: null,
  filletDialogFeature: null,
  chamferDialogFeature: null,
  holeDialogFeature: null,
  holePositionSelections: [],
  holePositionHover: null,
  constructionPlaneDialog: null,
  constructionPlanePickTarget: null,
  constructionPlanePickedReference: null,
  constructionPlanePickedEdge: null,
  modelingPickTarget: null,
  modelingPlaneSelection: null,
  bodyFeatureDialog: null,
  historyEdit: null,
  sketchPatternDialog: null,
  projectBusy: false,
  solidBusy: false,
  palette: { ...DEFAULT_PALETTE },

  setMode: (mode) => {
    if (mode !== 'solid') {
      jointPreviewGeneration += 1;
      jointMotionPreviewGeneration += 1;
      mechanismPreviewGeneration += 1;
    }
    set((s) => ({
      mode,
      // In sketch mode the tab strip collapses to a single SKETCH tab.
      activeTab: mode === 'sketch' ? 'sketch' : s.activeTab === 'sketch' ? 'solid' : s.activeTab,
      pendingConstraintTool: mode === 'sketch' ? s.pendingConstraintTool : null,
      jointPreviewSolution: mode === 'solid' ? s.jointPreviewSolution : null,
      jointMotionPreview: mode === 'solid' ? s.jointMotionPreview : null,
      mechanismPreview: mode === 'solid' ? s.mechanismPreview : null,
    }));
  },

  setActiveTab: (tab) => set({
    activeTab: tab === 'assembly' ? 'solid' : tab,
    ...(tab === 'assembly' ? { solidSidebarMode: 'assembly' as const } : {}),
  }),

  loadDocument: async () => {
    const engine = await getEngine();
    const doc = await engine.getDocument();
    const [finishedSketches, solidScene, datumPlanes, bodyAppearances, drawingDocument, assemblyDocument, assemblySolution, projectVisibility] = await Promise.all([
      engine.finishedSketches(),
      engine.solidScene(),
      engine.datumPlaneDefinitions(),
      engine.bodyAppearances(),
      engine.drawingDocument(),
      engine.assemblyDocument(),
      engine.assemblySolution(),
      engine.projectVisibility(),
    ]);
    set({
      document: doc,
      engineKind: engine.kind,
      finishedSketches: stageFinishedSketches(doc, finishedSketches),
      solidScene,
      datumPlanes: stageDatumPlanes(doc, datumPlanes),
      bodyAppearances: scrubAppearances(bodyAppearances, solidScene.bodies),
      drawingDocument,
      assemblyDocument,
      assemblySolution,
      hidden: hiddenFromPersistedVisibility(doc, projectVisibility),
      projectVisibility,
      dirty: false,
    });
    presentation.documentChanged();
  },

  refreshAfterInboxApply: async (opName) => {
    const engine = await getEngine();
    if (opName?.startsWith('drawing_')) {
      const drawingDocument=await engine.drawingDocument();
      set({drawingDocument,dirty:true,activeTab:'drawing',drawingTool:null,drawingPendingViewKind:null,
        selectedDrawingViewId:null,selectedDrawingAnnotationId:null,drawingSheetSetupOpen:drawingDocument.sheets.length===0});
      return;
    }
    // Assembly-only inbox ops: targeted refresh — keep dirty:true so MCP live
    // edits are treated as unsaved (never loadDocument's dirty:false).
    if (opName?.startsWith('assembly_')) {
      jointPreviewGeneration += 1;
      jointMotionPreviewGeneration += 1;
      mechanismPreviewGeneration += 1;
      const [assemblyDocument, assemblySolution, solidScene, doc] = await Promise.all([
        engine.assemblyDocument(),
        engine.assemblySolution(),
        engine.solidScene(),
        engine.getDocument(),
      ]);
      set((state) => ({
        document: doc,
        solidScene,
        assemblyDocument,
        assemblySolution,
        assemblySolidSyncRevision: state.assemblySolidSyncRevision + 1,
        bodyAppearances: scrubAppearances(state.bodyAppearances, solidScene.bodies),
        jointPreviewSolution: null,
        // Stale-preview correction: viewport prefers jointMotionPreview.solution
        // over the refreshed assemblySolution until this is cleared.
        jointMotionPreview: null,
        mechanismPreview: null,
        dirty: true,
      }));
      return;
    }
    const doc = await engine.getDocument();
    const [finishedSketches, solidScene, datumPlanes, bodyAppearances, drawingDocument, assemblyDocument, assemblySolution, projectVisibility, activeSketch] = await Promise.all([
      engine.finishedSketches(),
      engine.solidScene(),
      engine.datumPlaneDefinitions(),
      engine.bodyAppearances(),
      engine.drawingDocument(),
      engine.assemblyDocument(),
      engine.assemblySolution(),
      engine.projectVisibility(),
      engine.activeSketch(),
    ]);
    set({
      document: doc,
      engineKind: engine.kind,
      finishedSketches: stageFinishedSketches(doc, finishedSketches),
      solidScene,
      datumPlanes: stageDatumPlanes(doc, datumPlanes),
      bodyAppearances: scrubAppearances(bodyAppearances, solidScene.bodies),
      drawingDocument,
      assemblyDocument,
      assemblySolution,
      activeSketch,
      hidden: hiddenFromPersistedVisibility(doc, projectVisibility),
      projectVisibility,
      dirty: true,
    });
    if (opName?.startsWith('sketch_') || opName === 'cad_load_project_model') {
      // Inbox commands use the same engine as the interactive controller, but
      // do not call its mode transitions. Reflect the authoritative sketch
      // lifecycle before acknowledging/presenting the operation.
      useAppStore.getState().setMode(activeSketch ? 'sketch' : 'solid');
      useAppStore.getState().setActiveSketch(activeSketch);
      if (!activeSketch) useAppStore.getState().setActiveTool(null);
    }
  },

  setDocument: (doc) =>
    set((state) => ({
      document: doc,
      finishedSketches: stageFinishedSketches(doc, state.finishedSketches),
      datumPlanes: stageDatumPlanes(doc, state.datumPlanes),
      dirty: true,
    })),

  setFinishedSketches: (sketches) =>
    set((state) => ({
      finishedSketches: state.document
        ? stageFinishedSketches(state.document, sketches)
        : sketches,
    })),

  setSolidScene: (solidScene) => {
    jointPreviewGeneration += 1;
    jointMotionPreviewGeneration += 1;
    mechanismPreviewGeneration += 1;
    set((state) => ({
      solidScene,
      bodyAppearances: scrubAppearances(state.bodyAppearances, solidScene.bodies),
      jointPreviewSolution: null,
      jointMotionPreview: null,
      mechanismPreview: null,
    }));
    void getEngine()
      .then((engine) => Promise.all([engine.assemblyDocument(), engine.assemblySolution()]))
      .then(([assemblyDocument, assemblySolution]) =>
        set((state) => (
          state.solidScene === solidScene
            ? {
                assemblyDocument,
                assemblySolution,
                assemblySolidSyncRevision: state.assemblySolidSyncRevision + 1,
              }
            : {}
        )),
      )
      .catch(() => undefined);
  },

  applySolidUpdate: (update) => {
    jointPreviewGeneration += 1;
    jointMotionPreviewGeneration += 1;
    mechanismPreviewGeneration += 1;
    set((state) => ({
      document: update.document,
      solidScene: update.scene,
      finishedSketches: stageFinishedSketches(
        update.document,
        state.finishedSketches,
      ),
      datumPlanes: stageDatumPlanes(update.document, state.datumPlanes),
      dirty: true,
      bodyAppearances: scrubAppearances(state.bodyAppearances, update.scene.bodies),
      selectedBody:
        state.selectedBody !== null && update.scene.bodies.some((body) => body.id === state.selectedBody)
          ? state.selectedBody
          : null,
      selectedBodies: state.selectedBodies.filter((bodyId) =>
        update.scene.bodies.some((body) => body.id === bodyId),
      ),
      selectedFace:
        state.selectedFace !== null &&
        update.scene.bodies.some((body) => body.faces.some((face) => face.id === state.selectedFace))
          ? state.selectedFace
          : null,
      selectedFaces: state.selectedFaces.filter((faceId) =>
        update.scene.bodies.some((body) => body.faces.some((face) => face.id === faceId)),
      ),
      hoveredFace: null,
      selectedFacePoint: null,
      selectedEdges: state.selectedEdges.filter((edgeId) =>
        update.scene.bodies.some((body) => body.edges.some((edge) => edge.id === edgeId)),
      ),
      hoveredEdge: null,
      jointPreviewSolution: null,
      jointMotionPreview: null,
      mechanismPreview: null,
    }));
    void getEngine()
      .then((engine) => Promise.all([engine.assemblyDocument(), engine.assemblySolution()]))
      .then(([assemblyDocument, assemblySolution]) =>
        set((state) => {
          if (state.solidScene !== update.scene) return {};
          const jointStillExists = (jointId: number | null) =>
            jointId !== null && assemblyDocument.joints.some((joint) => joint.id === jointId);
          const selectedJointId = jointStillExists(state.selectedJointId)
            ? state.selectedJointId
            : null;
          const jointEditingId = jointStillExists(state.jointEditingId)
            ? state.jointEditingId
            : null;
          return {
            assemblyDocument,
            assemblySolution,
            assemblySolidSyncRevision: state.assemblySolidSyncRevision + 1,
            selectedJointId,
            jointEditingId,
            jointDialogOpen: state.jointEditingId !== null && jointEditingId === null
              ? false
              : state.jointDialogOpen,
          };
        }),
      )
      .catch(() => undefined);
  },

  applyDatumPlaneUpdate: (update) =>
    set((state) => ({
      document: update.document,
      finishedSketches: stageFinishedSketches(
        update.document,
        state.finishedSketches,
      ),
      datumPlanes: stageDatumPlanes(update.document, update.planes),
      dirty: true,
    })),

  setBodyAppearances: (appearances) => set({ bodyAppearances: appearances }),

  setBodyAppearance: async (appearance) => {
    const engine = await getEngine();
    const bodyAppearances = await engine.setBodyAppearance(appearance);
    set({ bodyAppearances, dirty: true });
  },

  setDrawingDocument: async (drawing) => {
    const engine = await getEngine();
    const drawingDocument = normalizeDrawingDocument(
      await engine.setDrawingDocument(normalizeDrawingDocument(drawing)),
    );
    set({ drawingDocument, dirty: true });
  },

  createComponent: async (name, bodyIds) => {
    jointPreviewGeneration += 1;
    mechanismPreviewGeneration += 1;
    const engine = await getEngine();
    const component = await engine.createComponent({
      name,
      body_ids: bodyIds,
      local_coordinate_system: IDENTITY_ASSEMBLY_TRANSFORM,
      absorb_promoted_bodies: true,
    });
    const [assemblyDocument, assemblySolution] = await Promise.all([
      engine.assemblyDocument(),
      engine.assemblySolution(),
    ]);
    const selectedOccurrenceId = assemblyDocument.component_structure.occurrences.find(
      (occurrence) => occurrence.component_id === component.id,
    )?.id ?? null;
    set({
      assemblyDocument,
      assemblySolution,
      selectedOccurrenceId,
      jointPreviewSolution: null,
      mechanismPreview: null,
      dirty: true,
    });
  },

  updateComponent: async (component) => {
    const engine = await getEngine();
    await engine.updateComponent({ component });
    const [assemblyDocument, assemblySolution] = await Promise.all([
      engine.assemblyDocument(),
      engine.assemblySolution(),
    ]);
    set({ assemblyDocument, assemblySolution, dirty: true });
  },

  createOccurrence: async (componentId, parentOccurrenceId = null, name = '') => {
    const engine = await getEngine();
    const occurrence = await engine.createOccurrence({
      component_id: componentId,
      name,
      parent_occurrence_id: parentOccurrenceId,
      local_pose: IDENTITY_ASSEMBLY_TRANSFORM,
    });
    const [assemblyDocument, assemblySolution] = await Promise.all([
      engine.assemblyDocument(),
      engine.assemblySolution(),
    ]);
    set({
      assemblyDocument,
      assemblySolution,
      selectedOccurrenceId: occurrence.id,
      dirty: true,
    });
  },

  updateOccurrence: async (occurrence) => {
    const engine = await getEngine();
    const updated = await engine.updateOccurrence({ occurrence });
    const [assemblyDocument, assemblySolution] = await Promise.all([
      engine.assemblyDocument(),
      engine.assemblySolution(),
    ]);
    set({
      assemblyDocument,
      assemblySolution,
      selectedOccurrenceId: updated.id,
      dirty: true,
    });
  },

  duplicateOccurrence: async (occurrenceId, parentOccurrenceId = null) => {
    const engine = await getEngine();
    const occurrence = await engine.duplicateOccurrence({
      occurrence_id: occurrenceId,
      parent_occurrence_id: parentOccurrenceId,
      local_pose: null,
    });
    const [assemblyDocument, assemblySolution] = await Promise.all([
      engine.assemblyDocument(),
      engine.assemblySolution(),
    ]);
    set({
      assemblyDocument,
      assemblySolution,
      selectedOccurrenceId: occurrence.id,
      dirty: true,
    });
  },

  setOccurrenceGrounded: async (occurrenceId, grounded) => {
    const engine = await getEngine();
    const assemblyDocument = await engine.setOccurrenceGrounded({
      occurrence_id: occurrenceId,
      grounded,
    });
    const assemblySolution = await engine.assemblySolution();
    set({ assemblyDocument, assemblySolution, selectedOccurrenceId: occurrenceId, dirty: true });
  },

  setOccurrencePose: async (occurrenceId, localPose) => {
    const engine = await getEngine();
    const assemblyDocument = await engine.setOccurrencePose({
      occurrence_id: occurrenceId,
      local_pose: localPose,
    });
    const assemblySolution = await engine.assemblySolution();
    set({ assemblyDocument, assemblySolution, selectedOccurrenceId: occurrenceId, dirty: true });
  },

  moveCopyOccurrence: async (occurrenceId, localPose, copy) => {
    const engine = await getEngine();
    let selectedOccurrenceId = occurrenceId;
    if (copy) {
      const source = useAppStore.getState().assemblyDocument.component_structure.occurrences.find(
        (occurrence) => occurrence.id === occurrenceId,
      );
      if (!source) throw new Error(`Occurrence ${occurrenceId} does not exist`);
      const duplicate = await engine.duplicateOccurrence({
        occurrence_id: occurrenceId,
        parent_occurrence_id: source.parent_occurrence_id,
        local_pose: localPose,
      });
      selectedOccurrenceId = duplicate.id;
    } else {
      await engine.setOccurrencePose({ occurrence_id: occurrenceId, local_pose: localPose });
    }
    const [assemblyDocument, assemblySolution] = await Promise.all([
      engine.assemblyDocument(),
      engine.assemblySolution(),
    ]);
    set({ assemblyDocument, assemblySolution, selectedOccurrenceId, dirty: true });
  },

  setSelectedOccurrenceId: (selectedOccurrenceId) => set({ selectedOccurrenceId }),
  setHoveredOccurrenceId: (hoveredOccurrenceId) => set({ hoveredOccurrenceId }),

  createJoint: async (request) => {
    mechanismPreviewGeneration += 1;
    const engine = await getEngine();
    const createdJoint = await engine.createJoint(request);
    const [assemblyDocument, assemblySolution] = await Promise.all([
      engine.assemblyDocument(),
      engine.assemblySolution(),
    ]);
    set({
      assemblyDocument,
      assemblySolution,
      jointPreviewSolution: null,
      jointMotionPreview: null,
      mechanismPreview: null,
      selectedJointId: createdJoint.id,
      jointEditingId: null,
      dirty: true,
      jointDialogOpen: false,
    });
  },

  updateJoint: async (request) => {
    mechanismPreviewGeneration += 1;
    const engine = await getEngine();
    const updatedJoint = await engine.updateJoint(request);
    const [assemblyDocument, assemblySolution] = await Promise.all([
      engine.assemblyDocument(),
      engine.assemblySolution(),
    ]);
    set({
      assemblyDocument,
      assemblySolution,
      jointPreviewSolution: null,
      jointMotionPreview: null,
      mechanismPreview: null,
      selectedJointId: updatedJoint.id,
      jointEditingId: null,
      dirty: true,
      jointDialogOpen: false,
    });
  },

  previewJoint: async (request) => {
    const generation = ++jointPreviewGeneration;
    const engine = await getEngine();
    const jointPreviewSolution = await engine.previewJoint(request);
    set((state) => (
      generation === jointPreviewGeneration && state.jointDialogOpen
        ? { jointPreviewSolution }
        : {}
    ));
  },

  previewJointUpdate: async (request) => {
    const generation = ++jointPreviewGeneration;
    const engine = await getEngine();
    const jointPreviewSolution = await engine.previewJointUpdate(request);
    set((state) => (
      generation === jointPreviewGeneration
      && state.jointDialogOpen
      && state.jointEditingId === request.joint.id
        ? { jointPreviewSolution }
        : {}
    ));
  },

  clearJointPreview: () => {
    jointPreviewGeneration += 1;
    set({ jointPreviewSolution: null });
  },

  deleteJoint: async (id) => {
    jointMotionPreviewGeneration += 1;
    mechanismPreviewGeneration += 1;
    const engine = await getEngine();
    const assemblyDocument = await engine.deleteJoint(id);
    const assemblySolution = await engine.assemblySolution();
    set((state) => ({
      assemblyDocument,
      assemblySolution,
      dirty: true,
      selectedJointId: state.selectedJointId === id ? null : state.selectedJointId,
      jointEditingId: state.jointEditingId === id ? null : state.jointEditingId,
      jointMotionPreview:
        state.jointMotionPreview?.jointId === id ? null : state.jointMotionPreview,
      mechanismPreview: null,
    }));
  },

  setJointMotion: async (jointId, angleOffsetDeg, linearOffsetMm) => {
    jointMotionPreviewGeneration += 1;
    mechanismPreviewGeneration += 1;
    const engine = await getEngine();
    const assemblyDocument = await engine.setJointMotion({
      joint_id: jointId,
      angle_offset_deg: angleOffsetDeg,
      linear_offset_mm: linearOffsetMm,
    });
    const assemblySolution = await engine.assemblySolution();
    set({
      assemblyDocument,
      assemblySolution,
      jointMotionPreview: null,
      mechanismPreview: null,
      dirty: true,
    });
  },

  previewJointMotion: async (jointId, angleOffsetDeg, linearOffsetMm) => {
    const joint = useAppStore.getState().assemblyDocument.joints.find(
      (candidate) => candidate.id === jointId,
    );
    if (!joint) return;
    const motion = jointMotionState(joint);
    motion.angle_offset_deg = angleOffsetDeg;
    motion.linear_offset_mm = linearOffsetMm;
    await useAppStore.getState().previewJointCoordinates(motion);
  },

  previewJointCoordinates: async (motion) => {
    const generation = ++jointMotionPreviewGeneration;
    const engine = await getEngine();
    const solution = await engine.previewJointCoordinates({ motion });
    set((state) => (
      generation === jointMotionPreviewGeneration
      && state.assemblyDocument.joints.some((joint) => joint.id === motion.joint_id)
        ? {
            jointMotionPreview: {
              jointId: motion.joint_id,
              angleOffsetDeg: motion.angle_offset_deg,
              linearOffsetMm: motion.linear_offset_mm,
              secondaryAngleOffsetDeg: motion.secondary_angle_offset_deg,
              tertiaryAngleOffsetDeg: motion.tertiary_angle_offset_deg,
              secondaryLinearOffsetMm: motion.secondary_linear_offset_mm,
              motion,
              solution,
            },
            mechanismPreview: null,
          }
        : {}
    ));
  },

  clearJointMotionPreview: () => {
    jointMotionPreviewGeneration += 1;
    set({ jointMotionPreview: null });
  },

  captureJointPosition: async () => {
    const state = useAppStore.getState();
    const preview = state.jointMotionPreview;
    if (!preview) return;
    jointMotionPreviewGeneration += 1;
    mechanismPreviewGeneration += 1;
    const engine = await getEngine();
    const motions = state.assemblyDocument.joints.map((joint) => (
      joint.id === preview.motion.joint_id ? preview.motion : jointMotionState(joint)
    ));
    await engine.createAssemblyPosition({
      name: `Position ${state.assemblyDocument.next_position_id}`,
      motions,
    });
    const assemblyDocument = await engine.assemblyDocument();
    set({
      assemblyDocument,
      jointMotionPreview: null,
      mechanismPreview: null,
      dirty: true,
    });
  },

  setJointEnabled: async (jointId, enabled) => {
    jointMotionPreviewGeneration += 1;
    mechanismPreviewGeneration += 1;
    const engine = await getEngine();
    const assemblyDocument = await engine.setJointEnabled({ joint_id: jointId, enabled });
    const assemblySolution = await engine.assemblySolution();
    set({
      assemblyDocument,
      assemblySolution,
      jointMotionPreview: null,
      mechanismPreview: null,
      dirty: true,
    });
  },

  previewMechanismDrag: async (
    bodyId,
    targetPose,
    occurrenceId = null,
    grabPointLocal = null,
    targetPointWorld = null,
    initialJointMotions = [],
    maximumIterations = 12,
  ) => {
    const generation = ++mechanismPreviewGeneration;
    pendingMechanismPreview = {
      generation,
      bodyId,
      occurrenceId,
      targetPose,
      grabPointLocal,
      targetPointWorld,
      initialJointMotions,
      maximumIterations: Math.max(1, Math.min(96, Math.round(maximumIterations))),
    };

    const startPump = () => {
      if (mechanismPreviewPump) return;
      const running = (async () => {
        const engine = await getEngine();
        while (pendingMechanismPreview) {
          const job = pendingMechanismPreview;
          pendingMechanismPreview = null;
          try {
            const mechanismPreview = await engine.previewMechanismDrag({
              body_id: job.bodyId,
              occurrence_id: job.occurrenceId,
              target_pose: job.targetPose,
              grab_point_local: job.grabPointLocal,
              target_point_world: job.targetPointWorld,
              initial_joint_motions: job.initialJointMotions,
              solve_orientation: false,
              maximum_iterations: job.maximumIterations,
            });
            // Pointer targets are deliberately coalesced, but every completed
            // intermediate solve is still the best continuity seed for the
            // newer target waiting behind it. Without carrying that pose
            // forward, a busy multi-joint mechanism repeatedly restarts from
            // the last rendered frame and can appear to freeze until the
            // pointer is released and grabbed again.
            const nextJob = queuedMechanismPreview();
            if (
              nextJob
              && nextJob.generation > job.generation
              && nextJob.bodyId === job.bodyId
              && nextJob.occurrenceId === job.occurrenceId
            ) {
              nextJob.initialJointMotions =
                mechanismPreview.joint_motions.map((motion) => ({ ...motion }));
            }
            const publishAsProgress = nextJob
              && nextJob.generation === mechanismPreviewGeneration
              && nextJob.bodyId === job.bodyId
              && nextJob.occurrenceId === job.occurrenceId;
            set((state) => (
              (job.generation === mechanismPreviewGeneration || publishAsProgress)
              && state.solidScene.bodies.some((body) => body.id === job.bodyId)
                ? { mechanismPreview, jointMotionPreview: null }
                : {}
            ));
          } catch (error) {
            // A slower superseded solve must not abort the pump or surface an
            // error after a newer pointer target is already waiting. Only the
            // newest request owns user-visible failure reporting.
            if (
              job.generation === mechanismPreviewGeneration
              && pendingMechanismPreview === null
            ) {
              throw error;
            }
          }
        }
      })();
      mechanismPreviewPump = running;
      // Register reset before any caller awaits `running`. A target queued as
      // the previous loop drains will then see a null pump and immediately
      // start another pass instead of remaining stranded until the next grab.
      void running.then(
        () => {
          if (mechanismPreviewPump === running) mechanismPreviewPump = null;
        },
        () => {
          if (mechanismPreviewPump === running) mechanismPreviewPump = null;
        },
      );
    };

    // A drag may enqueue while the previous pump is completing. Keep the
    // newest caller responsible for restarting until its target has either
    // been processed or superseded by a newer pointer position.
    while (generation === mechanismPreviewGeneration) {
      startPump();
      const running = mechanismPreviewPump;
      if (!running) break;
      try {
        await running;
      } catch (error) {
        if (generation === mechanismPreviewGeneration) throw error;
        return;
      }
      if (pendingMechanismPreview === null) break;
    }
  },

  clearMechanismPreview: () => {
    mechanismPreviewGeneration += 1;
    pendingMechanismPreview = null;
    set({ mechanismPreview: null });
  },

  captureMechanismPosition: async () => {
    const state = useAppStore.getState();
    const preview = state.mechanismPreview;
    if (!preview) return;
    mechanismPreviewGeneration += 1;
    pendingMechanismPreview = null;
    jointMotionPreviewGeneration += 1;
    const engine = await getEngine();
    const solvedById = new Map(preview.joint_motions.map((motion) => [motion.joint_id, motion]));
    const liveBodyIds = new Set(state.solidScene.bodies.map((body) => body.id));
    const activeJoints = state.assemblyDocument.joints.filter((joint) =>
      liveBodyIds.has(joint.connector_a.body_id)
      && liveBodyIds.has(joint.connector_b.body_id));
    await engine.createAssemblyPosition({
      name: `Position ${state.assemblyDocument.next_position_id}`,
      motions: activeJoints.map((joint) => (
        solvedById.get(joint.id) ?? jointMotionState(joint)
      )),
    });
    const assemblyDocument = await engine.assemblyDocument();
    set({
      assemblyDocument,
      jointMotionPreview: null,
      mechanismPreview: null,
      dirty: true,
    });
  },

  setGroundedBody: async (bodyId) => {
    jointMotionPreviewGeneration += 1;
    mechanismPreviewGeneration += 1;
    const engine = await getEngine();
    const assemblyDocument = await engine.setGroundedBody(bodyId);
    const assemblySolution = await engine.assemblySolution();
    set({
      assemblyDocument,
      assemblySolution,
      jointMotionPreview: null,
      mechanismPreview: null,
      dirty: true,
    });
  },

  setSelectedJointId: (selectedJointId) => {
    jointMotionPreviewGeneration += 1;
    mechanismPreviewGeneration += 1;
    set({ selectedJointId, jointMotionPreview: null, mechanismPreview: null });
  },

  setJointDialogOpen: (jointDialogOpen) => {
    jointPreviewGeneration += 1;
    jointMotionPreviewGeneration += 1;
    mechanismPreviewGeneration += 1;
    set((state) => ({
      jointDialogOpen,
      jointPreviewSolution: null,
      jointMotionPreview: null,
      mechanismPreview: null,
      jointEditingId: null,
      jointConnectorPicks: [],
      jointConnectorHover: null,
      solidSidebarMode: jointDialogOpen ? 'assembly' : state.solidSidebarMode,
      selectedBody: null,
      selectedBodies: [],
      selectedFace: null,
      selectedFaces: [],
      selectedFacePoint: null,
      modelingPointHover: null,
      selectedEdges: [],
      selectedOccurrenceId: null,
    }));
  },

  openJointEditor: (jointId) => {
    const state = useAppStore.getState();
    const joint = state.assemblyDocument.joints.find((candidate) => candidate.id === jointId);
    if (!joint) return;
    jointPreviewGeneration += 1;
    jointMotionPreviewGeneration += 1;
    mechanismPreviewGeneration += 1;
    set({
      jointDialogOpen: true,
      jointEditingId: jointId,
      selectedJointId: jointId,
      jointPreviewSolution: null,
      jointMotionPreview: null,
      mechanismPreview: null,
      jointConnectorPicks: [
        {
          ...joint.connector_a,
          occurrence_id: joint.advanced.connector_a_occurrence_id,
        },
        {
          ...joint.connector_b,
          occurrence_id: joint.advanced.connector_b_occurrence_id,
        },
      ],
      jointConnectorHover: null,
      solidSidebarMode: 'assembly',
      selectedBody: joint.connector_b.body_id,
      selectedBodies: [joint.connector_a.body_id, joint.connector_b.body_id],
      selectedFace: joint.connector_b.face_id || null,
      selectedFaces: [joint.connector_a.face_id, joint.connector_b.face_id]
        .filter((faceId) => faceId !== 0),
      selectedFacePoint: {
        x: joint.connector_b.frame.origin[0],
        y: joint.connector_b.frame.origin[1],
        z: joint.connector_b.frame.origin[2],
      },
      selectedEdges: [joint.connector_a.edge_id, joint.connector_b.edge_id]
        .filter((edgeId): edgeId is number => Boolean(edgeId)),
    });
  },

  toggleJointConnectorPick: (pick) => set((state) => {
    const identity = (candidate: JointConnectorDto) =>
      `${candidate.occurrence_id ?? 0}:${candidate.body_id}:${candidate.face_id}:${candidate.edge_id ?? 0}:${candidate.kind}:${candidate.frame.origin.join(',')}`;
    const key = identity(pick);
    const existing = state.jointConnectorPicks.findIndex(
      (candidate) => identity(candidate) === key,
    );
    let jointConnectorPicks = [...state.jointConnectorPicks];
    if (existing >= 0) jointConnectorPicks.splice(existing, 1);
    else if (jointConnectorPicks.length < 2) jointConnectorPicks.push(pick);
    else {
      const brokenIndex = jointConnectorPicks.findIndex(
        (candidate) => !jointConnectorIsLive(candidate, state.solidScene),
      );
      const replaceIndex = brokenIndex >= 0 ? brokenIndex : 1;
      jointConnectorPicks[replaceIndex] = pick;
    }
    const active = jointConnectorPicks[jointConnectorPicks.length - 1] ?? null;
    return {
      jointConnectorPicks,
      jointPreviewSolution:
        jointConnectorPicks.length === 2 ? state.jointPreviewSolution : null,
      selectedBody: active?.body_id ?? null,
      selectedBodies: [...new Set(jointConnectorPicks.map((candidate) => candidate.body_id))],
      selectedFace: active && active.face_id !== 0 ? active.face_id : null,
      selectedFaces: [...new Set(jointConnectorPicks
        .map((candidate) => candidate.face_id)
        .filter((faceId) => faceId !== 0))],
      selectedFacePoint: active
        ? {
            x: active.frame.origin[0],
            y: active.frame.origin[1],
            z: active.frame.origin[2],
          }
        : null,
      selectedEdges: [...new Set(jointConnectorPicks.flatMap((candidate) =>
        candidate.edge_id ? [candidate.edge_id] : []))],
      selectedNode: bodyBrowserNode(state.document, active?.body_id ?? null),
    };
  }),

  clearJointConnectorPicks: () => set({
    jointConnectorPicks: [],
    jointPreviewSolution: null,
    selectedNode: null,
    selectedBody: null,
    selectedBodies: [],
    selectedFace: null,
    selectedFaces: [],
    selectedFacePoint: null,
    selectedEdges: [],
  }),

  setJointConnectorHover: (jointConnectorHover) => set((state) => ({
    jointConnectorHover,
    hoveredOccurrenceId: jointConnectorHover?.occurrence_id ?? (
      state.jointDialogOpen ? null : state.hoveredOccurrenceId
    ),
    hoveredFace: jointConnectorHover && jointConnectorHover.face_id !== 0
      ? jointConnectorHover.face_id
      : (
      state.jointDialogOpen ? null : state.hoveredFace
    ),
    hoveredEdge: jointConnectorHover?.edge_id ?? (
      state.jointDialogOpen ? null : state.hoveredEdge
    ),
  })),

  setSolidSidebarMode: (solidSidebarMode) => {
    if (solidSidebarMode === 'model') {
      jointMotionPreviewGeneration += 1;
      mechanismPreviewGeneration += 1;
    }
    set({
      solidSidebarMode,
      selectedOccurrenceId:
        solidSidebarMode === 'model' ? null : useAppStore.getState().selectedOccurrenceId,
      hoveredOccurrenceId:
        solidSidebarMode === 'model' ? null : useAppStore.getState().hoveredOccurrenceId,
      jointMotionPreview:
        solidSidebarMode === 'model' ? null : useAppStore.getState().jointMotionPreview,
      mechanismPreview:
        solidSidebarMode === 'model' ? null : useAppStore.getState().mechanismPreview,
    });
  },

  setSelectedDrawingViewId: (selectedDrawingViewId) => set({ selectedDrawingViewId }),

  setSelectedDrawingAnnotationId: (selectedDrawingAnnotationId) =>
    set({ selectedDrawingAnnotationId }),

  setDrawingTool: (drawingTool) => set({ drawingTool }),

  setDrawingPendingViewKind: (drawingPendingViewKind) => set({ drawingPendingViewKind }),

  setDrawingSheetSetupOpen: (drawingSheetSetupOpen) => set({ drawingSheetSetupOpen }),

  setDrawingProfileExportOpen: (drawingProfileExportOpen) => set({ drawingProfileExportOpen }),

  loadProjectState: (
    update,
    finishedSketches,
    datumPlanes,
    fileName,
    bodyAppearances = [],
    drawingDocument = emptyDrawingDocument(),
    assemblyDocument = emptyAssemblyDocument(),
    projectVisibility = emptyProjectVisibility(),
    assemblySolution = emptyAssemblySolution(),
  ) => {
    set({
      ...resetDocumentUiState(),
      document: update.document,
      finishedSketches: stageFinishedSketches(update.document, finishedSketches),
      solidScene: update.scene,
      datumPlanes: stageDatumPlanes(update.document, datumPlanes),
      bodyAppearances: scrubAppearances(bodyAppearances, update.scene.bodies),
      drawingDocument: normalizeDrawingDocument(drawingDocument),
      assemblyDocument,
      assemblySolution,
      hidden: hiddenFromPersistedVisibility(update.document, projectVisibility),
      projectVisibility,
      dirty: false,
      projectFileName: fileName,
    });
    presentation.documentChanged();
  },

  markClean: (fileName) =>
    set((state) => ({
      dirty: false,
      projectFileName: fileName === undefined ? state.projectFileName : fileName,
    })),

  markDirty: () => set({ dirty: true }),

  setActiveSketch: (sketch) =>
    set((s) => ({
      activeSketch: sketch,
      dirty: true,
      // Drop selection/hover of entities that no longer exist.
      selectedEntity:
        s.selectedEntity !== null && sketch?.entities.some((e) => e.id === s.selectedEntity)
          ? s.selectedEntity
          : null,
      selectedEntities: s.selectedEntities.filter((id) =>
        sketch?.entities.some((e) => e.id === id),
      ),
      selectedDimension:
        s.selectedDimension !== null &&
        sketch?.dimensions.some((d) => d.constraint_id === s.selectedDimension)
          ? s.selectedDimension
          : null,
      selectedConstraint:
        s.selectedConstraint !== null &&
        sketch?.constraints.some((c) => c.id === s.selectedConstraint)
          ? s.selectedConstraint
          : null,
      hoveredEntity:
        s.hoveredEntity !== null && sketch?.entities.some((e) => e.id === s.hoveredEntity)
          ? s.hoveredEntity
          : null,
    })),

  setHoveredPlane: (plane) =>
    set((s) => (s.hoveredPlane === plane ? s : { hoveredPlane: plane })),

  setHoveredDatumPlane: (datumId) =>
    set((s) =>
      s.hoveredDatumPlane === datumId ? s : { hoveredDatumPlane: datumId },
    ),

  // Inline dimension editing is transient UI owned by the currently active
  // sketch interaction. Switching tools (including Escape -> Select) must
  // always dismiss it; otherwise the editor can outlive the Dimension tool.
  setActiveTool: (tool) =>
    set({
      activeTool: tool,
      pendingConstraintTool: null,
      dimEditor: null,
      sketchPatternDialog: null,
    }),

  setNavTool: (tool) => set({ navTool: tool }),

  setSelectedEntity: (id) =>
    set((s) => (s.selectedEntity === id ? s : { selectedEntity: id })),

  setSelectedEntities: (ids) => set({ selectedEntities: ids }),

  setPendingConstraintTool: (pendingConstraintTool) =>
    set({ pendingConstraintTool }),

  setSelectedDimension: (id) =>
    set((s) => (s.selectedDimension === id ? s : { selectedDimension: id })),

  setSelectedConstraint: (id) =>
    set((s) => (s.selectedConstraint === id ? s : { selectedConstraint: id })),

  setDimEditor: (editor) => set({ dimEditor: editor }),

  setHoveredEntity: (id) =>
    set((s) => (s.hoveredEntity === id ? s : { hoveredEntity: id })),

  setShowDof: (show) => set({ showDof: show }),

  setSixDofSpeed: (speed) => set({ sixDofSpeed: persistSixDofSpeed(speed) }),

  setThemePreference: (themePreference) => {
    persistThemePreference(themePreference);
    const resolvedTheme = applyThemePreference(themePreference);
    set({ themePreference, resolvedTheme });
  },

  syncResolvedTheme: () =>
    set((state) => {
      const resolvedTheme = applyThemePreference(state.themePreference);
      return resolvedTheme === state.resolvedTheme ? state : { resolvedTheme };
    }),

  setSettingsOpen: (settingsOpen) => set({ settingsOpen }),

  setPolygonMode: (mode) => set({ polygonMode: mode }),

  setSlotMode: (mode) => set({ slotMode: mode }),

  dynInput: {
    active: false,
    x: 0,
    y: 0,
    fields: [],
    focus: null,
    selectAll: false,
    pending: false,
  },
  constraintDialog: null,

  showDynInput: (fieldKeys, x, y) =>
    set({
      dynInput: {
        active: true,
        x,
        y,
        focus: null,
        selectAll: false,
        pending: false,
        fields: fieldKeys.map((key) => ({ key, value: '', locked: false, visible: true })),
      },
    }),

  updateDynInput: (values, visible, x, y) =>
    set((s) => {
      if (!s.dynInput.active) return s;
      return {
        dynInput: {
          ...s.dynInput,
          x,
          y,
          fields: s.dynInput.fields.map((f) => ({
            ...f,
            value: f.locked ? f.value : values[f.key] ?? f.value,
            visible: visible[f.key] ?? true,
          })),
        },
      };
    }),

  setDynField: (key, value, locked) =>
    set((s) => ({
      dynInput: {
        ...s.dynInput,
        fields: s.dynInput.fields.map((f) => (f.key === key ? { ...f, value, locked } : f)),
      },
    })),

  setDynFocus: (index, selectAll = index !== null) =>
    set((s) => ({ dynInput: { ...s.dynInput, focus: index, selectAll } })),

  setDynPending: (pending) =>
    set((s) => ({ dynInput: { ...s.dynInput, pending } })),

  hideDynInput: () =>
    set((s) => ({
      dynInput: { ...s.dynInput, active: false, focus: null, selectAll: false },
    })),

  clearDynLocks: () =>
    set((s) => ({
      dynInput: {
        ...s.dynInput,
        focus: null,
        selectAll: false,
        fields: s.dynInput.fields.map((f) => ({ ...f, locked: false })),
      },
    })),

  setConstraintDialog: (dialog) => set({ constraintDialog: dialog }),

  requestLookAt: () => set((s) => ({ lookAtNonce: s.lookAtNonce + 1 })),

  toggleExpanded: (id) =>
    set((s) => ({ expanded: { ...s.expanded, [id]: !s.expanded[id] } })),

  toggleHidden: (id) =>
    set((state) => {
      const hidden = { ...state.hidden, [id]: !state.hidden[id] };
      if (!hidden[id]) delete hidden[id];
      return {
        hidden,
        projectVisibility: persistedVisibilityFromHidden(state.document, hidden, state.projectVisibility),
        dirty: true,
      };
    }),

  applyProjectVisibility: (projectVisibility) => set((state) => ({
    projectVisibility,
    hidden: state.document ? hiddenFromPersistedVisibility(state.document, projectVisibility) : {},
    dirty: true,
  })),

  selectNode: (id) => set({ selectedNode: id }),

  setSelectedBody: (id) =>
    set((state) => ({
      selectedBody: id,
      selectedBodies: id === null ? [] : [id],
      selectedFace: null,
      selectedFaces: [],
      selectedFacePoint: null,
      selectedEdges: [],
      selectedNode:
        id === null
          ? state.selectedNode !== null &&
              bodyBrowserNode(
                state.document,
                state.selectedBody,
              ) === state.selectedNode
            ? null
            : state.selectedNode
          : bodyBrowserNode(state.document, id),
    })),

  replaceSelectedBodies: (ids) =>
    set((state) => {
      const selectedBodies = [...new Set(ids)].filter((id) =>
        state.solidScene.bodies.some((body) => body.id === id),
      );
      const selectedBody = selectedBodies[selectedBodies.length - 1] ?? null;
      return {
        selectedBody,
        selectedBodies,
        selectedFace: null,
        selectedFaces: [],
        selectedFacePoint: null,
        selectedEdges: [],
        selectedNode: bodyBrowserNode(state.document, selectedBody),
      };
    }),

  replaceSelectedFaces: (bodyId, ids) =>
    set((state) => {
      const body = state.solidScene.bodies.find(
        (candidate) => candidate.id === bodyId,
      );
      const selectedFaces = [
        ...new Set(
          ids.filter((id) => body?.faces.some((face) => face.id === id)),
        ),
      ];
      return {
        selectedBody: body ? bodyId : null,
        selectedBodies: body ? [bodyId] : [],
        selectedFace: selectedFaces[selectedFaces.length - 1] ?? null,
        selectedFaces,
        selectedFacePoint: null,
        selectedEdges: [],
        selectedNode: bodyBrowserNode(
          state.document,
          body ? bodyId : null,
        ),
      };
    }),

  setSelectedFace: (id) =>
    set((state) => ({
      selectedFace: id,
      selectedBodies: id === null ? state.selectedBodies : [],
      selectedFaces: id === null ? [] : [id],
      selectedEdges: id === null ? state.selectedEdges : [],
      selectedFacePoint:
        id !== null && id === state.selectedFace ? state.selectedFacePoint : null,
    })),

  setHoveredFace: (id) => set((state) => (state.hoveredFace === id ? state : { hoveredFace: id })),

  setSelectedFacePoint: (point) => set({ selectedFacePoint: point }),

  setModelingPointHover: (point) =>
    set((state) => {
      const current = state.modelingPointHover;
      if (
        current === point
        || (
          current !== null
          && point !== null
          && Math.abs(current.x - point.x) <= 1e-6
          && Math.abs(current.y - point.y) <= 1e-6
          && Math.abs(current.z - point.z) <= 1e-6
        )
      ) return state;
      return { modelingPointHover: point };
    }),

  openSketchPlaneOrigin: (faceId) => set({ sketchPlaneFace: faceId }),

  closeSketchPlaneOrigin: () => set({ sketchPlaneFace: null }),

  setSelectedEdges: (ids) =>
    set((state) => {
      const selectedEdges = [...new Set(ids)];
      return {
        selectedEdges,
        selectedBodies: selectedEdges.length > 0 ? [] : state.selectedBodies,
        selectedFaces: selectedEdges.length > 0 ? [] : state.selectedFaces,
        selectedFace: selectedEdges.length > 0 ? null : state.selectedFace,
        selectedFacePoint: selectedEdges.length > 0 ? null : state.selectedFacePoint,
      };
    }),

  selectSolidFeature: (
    kind,
    bodyId,
    featureId,
    point = null,
    additive = false,
  ) =>
    set((state) => {
      const selectedBodies = additive ? [...state.selectedBodies] : [];
      const selectedFaces = additive ? [...state.selectedFaces] : [];
      const selectedEdges = additive ? [...state.selectedEdges] : [];
      const target =
        kind === 'body'
          ? selectedBodies
          : kind === 'face'
            ? selectedFaces
            : selectedEdges;
      const existingIndex = target.indexOf(featureId);
      const removing = additive && existingIndex >= 0;
      if (removing) target.splice(existingIndex, 1);
      else target.push(featureId);

      let selectedBody: number | null = null;
      let selectedFace: number | null = null;
      let selectedFacePoint: Point3Dto | null = null;

      if (!removing) {
        selectedBody = bodyId;
        if (kind === 'face') {
          selectedFace = featureId;
          selectedFacePoint = point;
        }
      } else if (selectedFaces.length > 0) {
        selectedFace = selectedFaces[selectedFaces.length - 1];
        selectedBody =
          state.solidScene.bodies.find((body) =>
            body.faces.some((face) => face.id === selectedFace),
          )?.id ?? null;
        if (selectedFace === state.selectedFace) {
          selectedFacePoint = state.selectedFacePoint;
        }
      } else if (selectedEdges.length > 0) {
        const edgeId = selectedEdges[selectedEdges.length - 1];
        selectedBody =
          state.solidScene.bodies.find((body) =>
            body.edges.some((edge) => edge.id === edgeId),
          )?.id ?? null;
      } else if (selectedBodies.length > 0) {
        selectedBody = selectedBodies[selectedBodies.length - 1];
      }

      return {
        selectedBody,
        selectedBodies,
        selectedFace,
        selectedFaces,
        selectedFacePoint,
        selectedEdges,
        selectedNode: bodyBrowserNode(state.document, selectedBody),
      };
    }),

  clearSolidSelection: () =>
    set({
      selectedNode: null,
      selectedBody: null,
      selectedBodies: [],
      selectedFace: null,
      selectedFaces: [],
      selectedFacePoint: null,
      modelingPointHover: null,
      selectedEdges: [],
      hoveredFace: null,
      hoveredEdge: null,
      hoveredPlane: null,
      hoveredDatumPlane: null,
      selectedOccurrenceId: null,
      hoveredOccurrenceId: null,
    }),

  setHoveredEdge: (id) => set((state) => (state.hoveredEdge === id ? state : { hoveredEdge: id })),

  configureProfilePicker: (owner, catalog, selected, sketchName) =>
    set({
      profilePicker: {
        owner,
        catalog,
        selected,
        hovered: null,
        sketchName,
      },
    }),

  replaceProfilePicks: (owner, selected, sketchName) =>
    set((state) =>
      state.profilePicker?.owner === owner
        ? {
            profilePicker: {
              ...state.profilePicker,
              selected,
              sketchName: sketchName ?? state.profilePicker.sketchName,
            },
          }
        : state,
    ),

  toggleProfilePick: (profile) =>
    set((state) => {
      const picker = state.profilePicker;
      if (!picker) return state;
      const same = (candidate: ProfileRefDto) =>
        candidate.sketch_name === profile.sketch_name &&
        candidate.profile_index === profile.profile_index;
      let selected: ProfileRefDto[];
      if (picker.owner === 'sweep') {
        selected = [profile];
      } else if (picker.owner === 'loft') {
        selected = picker.selected.some(same)
          ? picker.selected.filter((candidate) => !same(candidate))
          : [...picker.selected, profile];
      } else {
        const sameSketch = picker.selected.filter(
          (candidate) => candidate.sketch_name === profile.sketch_name,
        );
        selected = sameSketch.some(same)
          ? sameSketch.filter((candidate) => !same(candidate))
          : [...sameSketch, profile];
      }
      return {
        profilePicker: {
          ...picker,
          selected,
          sketchName: profile.sketch_name,
        },
      };
    }),

  setHoveredProfilePick: (profile) =>
    set((state) => {
      const current = state.profilePicker?.hovered;
      if (
        !state.profilePicker ||
        (current?.sketch_name === profile?.sketch_name &&
          current?.profile_index === profile?.profile_index)
      ) {
        return state;
      }
      return { profilePicker: { ...state.profilePicker, hovered: profile } };
    }),

  clearProfilePicker: (owner) =>
    set((state) =>
      !state.profilePicker || (owner !== undefined && state.profilePicker.owner !== owner)
        ? state
        : { profilePicker: null },
    ),

  configureCurvePicker: (owner, catalog, selected, sketchName) =>
    set((state) => {
      const command = owner.split('_', 1)[0];
      const previousCommand = state.curvePicker?.owner.split('_', 1)[0];
      const selectionsByOwner = command === previousCommand
        ? state.curvePicker?.selectionsByOwner ?? {}
        : {};
      return {
        curvePicker: {
          owner,
          catalog,
          selected,
          selectionsByOwner: {
            ...selectionsByOwner,
            [owner]: selected,
          },
          hovered: null,
          sketchName,
        },
      };
    }),

  toggleCurvePick: (curve) =>
    set((state) => {
      const picker = state.curvePicker;
      if (!picker) return state;
      const sameSketch = picker.sketchName === curve.sketchName;
      const current = sameSketch ? picker.selected : [];
      const selected = current.some(
        (candidate) =>
          candidate.sketchName === curve.sketchName &&
          candidate.entityId === curve.entityId,
      )
        ? current.filter(
            (candidate) =>
              candidate.sketchName !== curve.sketchName ||
              candidate.entityId !== curve.entityId,
          )
        : [...current, curve];
      return {
        curvePicker: {
          ...picker,
          selected,
          selectionsByOwner: {
            ...picker.selectionsByOwner,
            [picker.owner]: selected,
          },
          sketchName: curve.sketchName,
        },
      };
    }),

  replaceCurvePicks: (owner, selected, sketchName) =>
    set((state) =>
      state.curvePicker?.owner !== owner
        ? state
        : {
            curvePicker: {
              ...state.curvePicker,
              selected,
              selectionsByOwner: {
                ...state.curvePicker.selectionsByOwner,
                [owner]: selected,
              },
              sketchName:
                sketchName ??
                selected[selected.length - 1]?.sketchName ??
                state.curvePicker.sketchName,
            },
          },
    ),

  setHoveredCurvePick: (curve) =>
    set((state) => {
      const current = state.curvePicker?.hovered;
      if (
        !state.curvePicker ||
        (current?.sketchName === curve?.sketchName &&
          current?.entityId === curve?.entityId)
      ) {
        return state;
      }
      return { curvePicker: { ...state.curvePicker, hovered: curve } };
    }),

  clearCurvePicker: (owner) =>
    set((state) =>
      !state.curvePicker || (owner !== undefined && state.curvePicker.owner !== owner)
        ? state
        : { curvePicker: null },
    ),

  setSolidCommandPreview: (solidCommandPreview) => set({ solidCommandPreview }),

  openExtrudeDialog: (featureId = 0) =>
    set((state) =>
      state.extrudeDialogFeature === featureId
        ? state
        : {
            extrudeDialogFeature: featureId,
            revolveDialogFeature: null,
            revolveAxisSelection: null,
            revolveAxisHover: null,
            sweepDialogFeature: null,
            loftDialogFeature: null,
            ribDialogFeature: null,
            filletDialogFeature: null,
            chamferDialogFeature: null,
            holeDialogFeature: null,
            profilePicker: null,
            curvePicker: null,
            modelingPickTarget: null,
            modelingPlaneSelection: null,
          },
    ),

  closeExtrudeDialog: () =>
    set((state) => ({
      extrudeDialogFeature: null,
      profilePicker: state.profilePicker?.owner === 'extrude' ? null : state.profilePicker,
      solidCommandPreview: null,
      modelingPickTarget: null,
      modelingPlaneSelection: null,
    })),

  openRevolveDialog: (featureId = 0) =>
    set({
      revolveDialogFeature: featureId,
      revolveAxisSelection: null,
      revolveAxisHover: null,
      extrudeDialogFeature: null,
      sweepDialogFeature: null,
      loftDialogFeature: null,
      ribDialogFeature: null,
      filletDialogFeature: null,
      chamferDialogFeature: null,
      holeDialogFeature: null,
      profilePicker: null,
      curvePicker: null,
      modelingPickTarget: null,
      modelingPlaneSelection: null,
    }),

  closeRevolveDialog: () =>
    set({
      revolveDialogFeature: null,
      revolveAxisSelection: null,
      revolveAxisHover: null,
      profilePicker: null,
      curvePicker: null,
      modelingPickTarget: null,
      modelingPlaneSelection: null,
    }),

  setRevolveAxisSelection: (selection) => set({ revolveAxisSelection: selection }),

  setRevolveAxisHover: (selection) =>
    set((state) => {
      const current = state.revolveAxisHover;
      if (
        current?.sketchName === selection?.sketchName &&
        current?.entityId === selection?.entityId
      ) {
        return state;
      }
      return { revolveAxisHover: selection };
    }),

  openSweepDialog: (featureId = 0) =>
    set({
      sweepDialogFeature: featureId,
      extrudeDialogFeature: null,
      revolveDialogFeature: null,
      revolveAxisSelection: null,
      revolveAxisHover: null,
      loftDialogFeature: null,
      ribDialogFeature: null,
      filletDialogFeature: null,
      chamferDialogFeature: null,
      holeDialogFeature: null,
      profilePicker: null,
      curvePicker: null,
      modelingPickTarget: null,
      modelingPlaneSelection: null,
    }),

  closeSweepDialog: () =>
    set((state) => ({
      sweepDialogFeature: null,
      profilePicker: state.profilePicker?.owner === 'sweep' ? null : state.profilePicker,
      curvePicker: null,
      modelingPickTarget: null,
      modelingPlaneSelection: null,
    })),

  openLoftDialog: (featureId = 0) =>
    set({
      loftDialogFeature: featureId,
      extrudeDialogFeature: null,
      revolveDialogFeature: null,
      revolveAxisSelection: null,
      revolveAxisHover: null,
      sweepDialogFeature: null,
      ribDialogFeature: null,
      filletDialogFeature: null,
      chamferDialogFeature: null,
      holeDialogFeature: null,
      profilePicker: null,
      curvePicker: null,
      modelingPickTarget: null,
      modelingPlaneSelection: null,
    }),

  closeLoftDialog: () =>
    set((state) => ({
      loftDialogFeature: null,
      profilePicker: state.profilePicker?.owner === 'loft' ? null : state.profilePicker,
      curvePicker: null,
      modelingPickTarget: null,
      modelingPlaneSelection: null,
    })),

  openRibDialog: (featureId = 0) =>
    set({
      ribDialogFeature: featureId,
      extrudeDialogFeature: null,
      revolveDialogFeature: null,
      revolveAxisSelection: null,
      revolveAxisHover: null,
      sweepDialogFeature: null,
      loftDialogFeature: null,
      filletDialogFeature: null,
      chamferDialogFeature: null,
      holeDialogFeature: null,
      profilePicker: null,
      curvePicker: null,
      modelingPickTarget: null,
      modelingPlaneSelection: null,
    }),

  closeRibDialog: () => set({
    ribDialogFeature: null,
    curvePicker: null,
    modelingPickTarget: null,
    modelingPlaneSelection: null,
  }),

  openFilletDialog: (featureId = 0) => set({
    filletDialogFeature: featureId,
    chamferDialogFeature: null,
    holeDialogFeature: null,
    extrudeDialogFeature: null,
    revolveDialogFeature: null,
    sweepDialogFeature: null,
    loftDialogFeature: null,
    ribDialogFeature: null,
    profilePicker: null,
    curvePicker: null,
    hoveredFace: null,
    hoveredEdge: null,
    modelingPickTarget: null,
    modelingPlaneSelection: null,
  }),

  closeFilletDialog: () => set({
    filletDialogFeature: null,
    hoveredEdge: null,
    modelingPickTarget: null,
    modelingPlaneSelection: null,
  }),

  openChamferDialog: (featureId = 0) => set({
    chamferDialogFeature: featureId,
    filletDialogFeature: null,
    holeDialogFeature: null,
    extrudeDialogFeature: null,
    revolveDialogFeature: null,
    sweepDialogFeature: null,
    loftDialogFeature: null,
    ribDialogFeature: null,
    profilePicker: null,
    curvePicker: null,
    hoveredFace: null,
    hoveredEdge: null,
    modelingPickTarget: null,
    modelingPlaneSelection: null,
  }),

  closeChamferDialog: () => set({
    chamferDialogFeature: null,
    hoveredEdge: null,
    modelingPickTarget: null,
    modelingPlaneSelection: null,
  }),

  openHoleDialog: (featureId = 0) => set({
    holeDialogFeature: featureId,
    holePositionSelections: [],
    holePositionHover: null,
    filletDialogFeature: null,
    chamferDialogFeature: null,
    extrudeDialogFeature: null,
    revolveDialogFeature: null,
    sweepDialogFeature: null,
    loftDialogFeature: null,
    ribDialogFeature: null,
    profilePicker: null,
    curvePicker: null,
    solidCommandPreview: null,
    modelingPickTarget: null,
    modelingPlaneSelection: null,
  }),

  closeHoleDialog: () => set({
    holeDialogFeature: null,
    holePositionSelections: [],
    holePositionHover: null,
    hoveredFace: null,
    solidCommandPreview: null,
    modelingPickTarget: null,
    modelingPlaneSelection: null,
  }),

  setHolePositionSelections: (selections) => set({ holePositionSelections: selections }),

  toggleHolePositionSelection: (selection) =>
    set((state) => {
      const same = (candidate: FinishedSketchPointPick) =>
        candidate.sketch_name === selection.sketch_name &&
        candidate.entity_id === selection.entity_id &&
        candidate.kind === selection.kind &&
        (candidate.kind === 'fit_point' ? candidate.index : null) ===
          (selection.kind === 'fit_point' ? selection.index : null);
      return {
        holePositionSelections: state.holePositionSelections.some(same)
          ? state.holePositionSelections.filter((candidate) => !same(candidate))
          : [...state.holePositionSelections, selection],
      };
    }),

  setHolePositionHover: (selection) =>
    set((state) => {
      const current = state.holePositionHover;
      if (
        current?.sketch_name === selection?.sketch_name &&
        current?.entity_id === selection?.entity_id &&
        current?.kind === selection?.kind &&
        (current?.kind === 'fit_point' ? current.index : null) ===
          (selection?.kind === 'fit_point' ? selection.index : null)
      ) {
        return state;
      }
      return { holePositionHover: selection };
    }),

  openConstructionPlaneDialog: (kind, featureId = 0) =>
    set((state) => {
      const planarFaces = state.selectedFaces.filter((faceId) =>
        state.solidScene.bodies.some((body) =>
          body.faces.some((face) => face.id === faceId && face.plane !== null),
        ),
      );
      const activeFaceIsPlanar = state.solidScene.bodies.some((body) =>
        body.faces.some(
          (face) => face.id === state.selectedFace && face.plane !== null,
        ),
      );
      const constructionPlanePickTarget: ConstructionPlanePickTarget =
        featureId > 0
          ? null
          : kind === 'midplane'
            ? planarFaces.length >= 2
              ? null
              : planarFaces.length === 1
                ? 'second_reference'
                : 'first_reference'
            : kind === 'offset'
              ? activeFaceIsPlanar
                ? null
                : 'first_reference'
              : 'first_reference';
      return {
        constructionPlaneDialog: { kind, featureId },
        constructionPlanePickTarget,
        constructionPlanePickedReference: null,
        constructionPlanePickedEdge: null,
        bodyFeatureDialog: null,
        extrudeDialogFeature: null,
        revolveDialogFeature: null,
        sweepDialogFeature: null,
        loftDialogFeature: null,
        ribDialogFeature: null,
        filletDialogFeature: null,
        chamferDialogFeature: null,
        holeDialogFeature: null,
        profilePicker: null,
        curvePicker: null,
        solidCommandPreview: null,
        modelingPickTarget: null,
        modelingPlaneSelection: null,
      };
    }),

  closeConstructionPlaneDialog: () =>
    set({
      constructionPlaneDialog: null,
      constructionPlanePickTarget: null,
      constructionPlanePickedReference: null,
      constructionPlanePickedEdge: null,
      solidCommandPreview: null,
      modelingPickTarget: null,
      modelingPlaneSelection: null,
    }),

  setConstructionPlanePickTarget: (target) =>
    set({
      constructionPlanePickTarget: target,
      constructionPlanePickedReference: null,
      constructionPlanePickedEdge: null,
    }),

  setConstructionPlanePickedReference: (reference) =>
    set({ constructionPlanePickedReference: reference }),

  setConstructionPlanePickedEdge: (edge) =>
    set({ constructionPlanePickedEdge: edge }),

  setModelingPickTarget: (target) =>
    set({ modelingPickTarget: target, modelingPointHover: null }),

  setModelingPlaneSelection: (reference) =>
    set({ modelingPlaneSelection: reference }),

  openBodyFeatureDialog: (kind, featureId = 0) =>
    set({
      bodyFeatureDialog: { kind, featureId },
      constructionPlaneDialog: null,
      constructionPlanePickTarget: null,
      constructionPlanePickedReference: null,
      constructionPlanePickedEdge: null,
      extrudeDialogFeature: null,
      revolveDialogFeature: null,
      sweepDialogFeature: null,
      loftDialogFeature: null,
      ribDialogFeature: null,
      filletDialogFeature: null,
      chamferDialogFeature: null,
      holeDialogFeature: null,
      profilePicker: null,
      curvePicker: null,
      solidCommandPreview: null,
      modelingPickTarget: null,
      modelingPlaneSelection: null,
    }),

  closeBodyFeatureDialog: () =>
    set({
      bodyFeatureDialog: null,
      solidCommandPreview: null,
      modelingPickTarget: null,
      modelingPlaneSelection: null,
    }),

  setHistoryEdit: (historyEdit) => set({ historyEdit }),

  openSketchPatternDialog: (kind) =>
    set({
      sketchPatternDialog: kind,
      activeTool: null,
      dimEditor: null,
      dynInput: {
        active: false,
        x: 0,
        y: 0,
        fields: [],
        focus: null,
        selectAll: false,
        pending: false,
      },
    }),

  closeSketchPatternDialog: () => set({ sketchPatternDialog: null }),

  setProjectBusy: (busy) => set({ projectBusy: busy }),

  setSolidBusy: (busy) => set({ solidBusy: busy }),

  setPaletteOption: (key, value) =>
    set((s) => ({ palette: { ...s.palette, [key]: value } })),
}));

/** Resolve appearance for a body id from the live store. */
export function bodyAppearanceFor(bodyId: number): BodyAppearance {
  return appearanceFor(useAppStore.getState().bodyAppearances, bodyId);
}

/**
 * Export the authoritative engine model after applying frontend-owned Browser
 * visibility. Keeping this boundary explicit prevents a rapid Save or tab
 * switch from racing an asynchronous eye-toggle IPC call.
 */
export async function exportProjectModelWithVisibility(
  providedEngine?: Engine,
  assertCurrent?: () => void | Promise<void>,
): Promise<string> {
  await assertCurrent?.();
  const engine = providedEngine ?? await getEngine();
  await assertCurrent?.();
  await synchronizeSnapshotVisibility(
    useAppStore.getState().projectVisibility,
    async () => { await assertCurrent?.(); return engine.projectVisibility(); },
    async visibility => { await assertCurrent?.(); return engine.setProjectVisibility(visibility); },
  );
  await assertCurrent?.();
  const model = await engine.exportProjectModel();
  await assertCurrent?.();
  return model;
}
