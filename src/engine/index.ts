/**
 * Engine adapter layer.
 *
 * The frontend talks to a single `Engine` interface; `getEngine()` returns
 * the `TauriEngine` when running inside the native shell and the
 * `WasmEngine` otherwise (browser preview/dev). Both hosts run the same
 * Rust code via `host::handle`, so behavior is identical by construction.
 * No sketch/geometry logic lives in TypeScript.
 */
import type {
  AddConstraintResult,
  AddLineResult,
  AssemblyDocumentDto,
  AssemblySolutionDto,
  Arc3PointRequest,
  ArcCenterRequest,
  BreakRequest,
  BodyAppearance,
  BodyFeatureDefinitionDto,
  BodyFeatureRequestDto,
  FaceSketchOrigin,
  ChamferRequest,
  SketchCircularPatternRequest,
  CircleRequest,
  ConstraintPayload,
  ComponentDefinitionDto,
  ComponentDefinitionPatchDto,
  ComponentOccurrenceDto,
  ComponentOccurrencePatchDto,
  CreateComponentRequestDto,
  CreateJointRequestDto,
  CreateOccurrenceRequestDto,
  DuplicateOccurrenceRequestDto,
  UpdateJointRequestDto,
  SetJointEnabledRequestDto,
  SetJointCoordinatesRequestDto,
  ApplyJointMotionsRequestDto,
  AssemblyPositionDto,
  ContactSetDto,
  CreateAssemblyPositionRequestDto,
  CreateContactSetRequestDto,
  CreateMotionStudyRequestDto,
  EvaluateMotionStudyRequestDto,
  InterferenceCheckRequestDto,
  InterferenceReportDto,
  MechanismDragRequestDto,
  MechanismPreviewDto,
  MotionPathRequestDto,
  MotionStudyDto,
  MotionStudyEvaluationDto,
  MotionStudySampleDto,
  SampleMotionStudyRequestDto,
  SetJointMotionRequestDto,
  SetOccurrenceGroundedRequestDto,
  SetOccurrencePoseRequestDto,
  SweptCollisionReportDto,
  SweptCollisionRequestDto,
  UpdateComponentRequestDto,
  UpdateOccurrenceRequestDto,
  DeleteEntityResult,
  DimensionRequest,
  DimensionStyle,
  DatumPlaneDefinitionDto,
  DatumPlaneRequest,
  DatumPlaneUpdateDto,
  DocumentDto,
  DrawingDocumentDto,
  DrawingProjectionDto,
  DrawingProjectionRequest,
  EditDimensionRequest,
  EndSketchResult,
  ExtrudeDefinitionDto,
  ExtrudeRequest,
  LoftDefinitionDto,
  LoftRequest,
  RevolveDefinitionDto,
  RevolveRequest,
  RibDefinitionDto,
  RibRequest,
  SolidFilletDefinitionDto,
  SolidFilletRequest,
  SolidChamferDefinitionDto,
  SolidChamferRequest,
  HoleDefinitionDto,
  HoleRequest,
  JointDefinitionDto,
  EvalExpressionResult,
  ExtendRequest,
  FilletPreviewDto,
  FilletRequest,
  LockedCircleRequest,
  LockedRectangleRequest,
  LockedSegmentRequest,
  MidpointLineRequest,
  MirrorRequest,
  MoveCopyRequest,
  MoveDimensionRequest,
  SetDimensionModeRequest,
  MovePointRequest,
  MovePointResult,
  OffsetPreviewDto,
  OffsetRequest,
  PlaneRef,
  PointRequest,
  ProfileCatalogItemDto,
  ProjectVisibilityDto,
  PolygonRequest,
  PreviewDto,
  RectangleRequest,
  SketchRectangularPatternRequest,
  ScaleRequest,
  SegmentRequest,
  SlotRequest,
  SplineRequest,
  SketchDto,
  SolidSceneDto,
  SolidUpdateDto,
  StepExportRequest,
  MeshExportRequest,
  SweepDefinitionDto,
  SweepRequest,
  ToolResult,
  TrimPreviewDto,
  TrimRequest,
  UndoResult,
} from './types';

/** The host-neutral engine API (JSON shapes mirror the Rust DTOs). */
export interface Engine {
  readonly kind: 'tauri' | 'wasm';
  getDocument(): Promise<DocumentDto>;
  beginSketch(plane: PlaneRef, faceOrigin?: FaceSketchOrigin): Promise<SketchDto>;
  endSketch(): Promise<EndSketchResult>;
  /** Snapshots of all finished sketches (M1d): muted 3D render + re-edit. */
  finishedSketches(): Promise<SketchDto[]>;
  /** Re-enter a finished sketch for editing, by browser name (M1d). */
  editSketch(name: string): Promise<SketchDto>;
  activeSketch(): Promise<SketchDto | null>;
  profileCatalog(): Promise<ProfileCatalogItemDto[]>;
  solidScene(): Promise<SolidSceneDto>;
  bodyAppearances(): Promise<BodyAppearance[]>;
  projectVisibility(): Promise<ProjectVisibilityDto>;
  setProjectVisibility(visibility: ProjectVisibilityDto): Promise<ProjectVisibilityDto>;
  drawingDocument(): Promise<DrawingDocumentDto>;
  drawingApply(command: import('./types').DrawingCommandDto): Promise<DrawingDocumentDto>;
  setDrawingDocument(document: DrawingDocumentDto): Promise<DrawingDocumentDto>;
  assemblyDocument(): Promise<AssemblyDocumentDto>;
  setAssemblyDocument(document: AssemblyDocumentDto): Promise<AssemblyDocumentDto>;
  assemblySolution(): Promise<AssemblySolutionDto>;
  createComponent(request: CreateComponentRequestDto): Promise<ComponentDefinitionDto>;
  updateComponent(request: UpdateComponentRequestDto): Promise<ComponentDefinitionDto>;
  createOccurrence(request: CreateOccurrenceRequestDto): Promise<ComponentOccurrenceDto>;
  updateOccurrence(request: UpdateOccurrenceRequestDto): Promise<ComponentOccurrenceDto>;
  duplicateOccurrence(request: DuplicateOccurrenceRequestDto): Promise<ComponentOccurrenceDto>;
  setOccurrenceGrounded(request: SetOccurrenceGroundedRequestDto): Promise<AssemblyDocumentDto>;
  setOccurrencePose(request: SetOccurrencePoseRequestDto): Promise<AssemblyDocumentDto>;
  previewJoint(request: CreateJointRequestDto): Promise<AssemblySolutionDto>;
  createJoint(request: CreateJointRequestDto): Promise<JointDefinitionDto>;
  updateJoint(request: UpdateJointRequestDto): Promise<JointDefinitionDto>;
  previewJointUpdate(request: UpdateJointRequestDto): Promise<AssemblySolutionDto>;
  deleteJoint(id: number): Promise<AssemblyDocumentDto>;
  setJointEnabled(request: SetJointEnabledRequestDto): Promise<AssemblyDocumentDto>;
  setJointMotion(request: SetJointMotionRequestDto): Promise<AssemblyDocumentDto>;
  previewJointMotion(request: SetJointMotionRequestDto): Promise<AssemblySolutionDto>;
  setJointCoordinates(request: SetJointCoordinatesRequestDto): Promise<AssemblyDocumentDto>;
  previewJointCoordinates(request: SetJointCoordinatesRequestDto): Promise<AssemblySolutionDto>;
  previewMechanismDrag(request: MechanismDragRequestDto): Promise<MechanismPreviewDto>;
  applyJointMotions(request: ApplyJointMotionsRequestDto): Promise<AssemblyDocumentDto>;
  createAssemblyPosition(request: CreateAssemblyPositionRequestDto): Promise<AssemblyPositionDto>;
  updateAssemblyPosition(position: AssemblyPositionDto): Promise<AssemblyPositionDto>;
  deleteAssemblyPosition(id: number): Promise<AssemblyDocumentDto>;
  applyAssemblyPosition(id: number): Promise<AssemblyDocumentDto>;
  createMotionStudy(request: CreateMotionStudyRequestDto): Promise<MotionStudyDto>;
  updateMotionStudy(study: MotionStudyDto): Promise<MotionStudyDto>;
  deleteMotionStudy(id: number): Promise<AssemblyDocumentDto>;
  sampleMotionStudy(request: SampleMotionStudyRequestDto): Promise<MotionStudySampleDto>;
  evaluateMotionStudy(request: EvaluateMotionStudyRequestDto): Promise<MotionStudyEvaluationDto>;
  exportMotionPathCsv(request: MotionPathRequestDto): Promise<string>;
  createContactSet(request: CreateContactSetRequestDto): Promise<ContactSetDto>;
  updateContactSet(contact: ContactSetDto): Promise<ContactSetDto>;
  deleteContactSet(id: number): Promise<AssemblyDocumentDto>;
  interferenceCheck(request: InterferenceCheckRequestDto): Promise<InterferenceReportDto>;
  sweptCollisionCheck(request: SweptCollisionRequestDto): Promise<SweptCollisionReportDto>;
  setGroundedBody(bodyId: number | null): Promise<AssemblyDocumentDto>;
  drawingProjection(request: DrawingProjectionRequest): Promise<DrawingProjectionDto>;
  setBodyAppearance(appearance: BodyAppearance): Promise<BodyAppearance[]>;
  extrudeDefinitions(): Promise<ExtrudeDefinitionDto[]>;
  revolveDefinitions(): Promise<RevolveDefinitionDto[]>;
  sweepDefinitions(): Promise<SweepDefinitionDto[]>;
  loftDefinitions(): Promise<LoftDefinitionDto[]>;
  ribDefinitions(): Promise<RibDefinitionDto[]>;
  filletDefinitions(): Promise<SolidFilletDefinitionDto[]>;
  chamferDefinitions(): Promise<SolidChamferDefinitionDto[]>;
  holeDefinitions(): Promise<HoleDefinitionDto[]>;
  datumPlaneDefinitions(): Promise<DatumPlaneDefinitionDto[]>;
  bodyFeatureDefinitions(): Promise<BodyFeatureDefinitionDto[]>;
  createDatumPlane(request: DatumPlaneRequest): Promise<DatumPlaneUpdateDto>;
  editDatumPlane(featureId: number, request: DatumPlaneRequest): Promise<DatumPlaneUpdateDto>;
  bodyFeature(request: BodyFeatureRequestDto): Promise<SolidUpdateDto>;
  editBodyFeature(featureId: number, request: BodyFeatureRequestDto): Promise<SolidUpdateDto>;
  extrude(request: ExtrudeRequest): Promise<SolidUpdateDto>;
  editExtrude(featureId: number, request: ExtrudeRequest): Promise<SolidUpdateDto>;
  revolve(request: RevolveRequest): Promise<SolidUpdateDto>;
  editRevolve(featureId: number, request: RevolveRequest): Promise<SolidUpdateDto>;
  sweep(request: SweepRequest): Promise<SolidUpdateDto>;
  editSweep(featureId: number, request: SweepRequest): Promise<SolidUpdateDto>;
  loft(request: LoftRequest): Promise<SolidUpdateDto>;
  editLoft(featureId: number, request: LoftRequest): Promise<SolidUpdateDto>;
  rib(request: RibRequest): Promise<SolidUpdateDto>;
  editRib(featureId: number, request: RibRequest): Promise<SolidUpdateDto>;
  solidFillet(request: SolidFilletRequest): Promise<SolidUpdateDto>;
  editSolidFillet(featureId: number, request: SolidFilletRequest): Promise<SolidUpdateDto>;
  solidChamfer(request: SolidChamferRequest): Promise<SolidUpdateDto>;
  editSolidChamfer(featureId: number, request: SolidChamferRequest): Promise<SolidUpdateDto>;
  hole(request: HoleRequest): Promise<SolidUpdateDto>;
  editHole(featureId: number, request: HoleRequest): Promise<SolidUpdateDto>;
  recomputeSolids(): Promise<SolidUpdateDto>;
  setRollback(rollbackIndex: number): Promise<SolidUpdateDto>;
  deleteFeature(featureId: number): Promise<SolidUpdateDto>;
  reorderFeature(featureId: number, targetIndex: number): Promise<SolidUpdateDto>;
  setDocumentName(name: string): Promise<DocumentDto>;
  exportProjectModel(): Promise<string>;
  /** Bind the bootstrap engine context to the first frontend tab. */
  bindProjectSession(sessionId: string): Promise<void>;
  /** Create and activate a blank retained modeling context for a new tab. */
  createProjectSession(sessionId: string): Promise<SolidUpdateDto>;
  /** Activate a retained context; false means it was evicted. */
  activateProjectSession(sessionId: string): Promise<boolean>;
  /** Release an inactive tab's native/WASM modeling context. */
  dropProjectSession(sessionId: string): Promise<void>;
  newProject(): Promise<SolidUpdateDto>;
  loadProjectModel(modelJson: string): Promise<SolidUpdateDto>;
  exportStep(request: StepExportRequest): Promise<Uint8Array>;
  exportStl(request: MeshExportRequest): Promise<Uint8Array>;
  export3mf(request: MeshExportRequest): Promise<Uint8Array>;
  previewSegment(request: SegmentRequest): Promise<PreviewDto>;
  addLine(request: SegmentRequest): Promise<AddLineResult>;
  previewSegmentLocked(request: LockedSegmentRequest): Promise<PreviewDto>;
  addLineLocked(request: LockedSegmentRequest): Promise<AddLineResult>;
  addPoint(request: PointRequest): Promise<ToolResult>;
  addLineMidpoint(request: MidpointLineRequest): Promise<ToolResult>;
  addRectangle(request: RectangleRequest): Promise<ToolResult>;
  addRectangleLocked(request: LockedRectangleRequest): Promise<ToolResult>;
  addCircle(request: CircleRequest): Promise<ToolResult>;
  addCircleLocked(request: LockedCircleRequest): Promise<ToolResult>;
  addSlot(request: SlotRequest): Promise<ToolResult>;
  addSpline(request: SplineRequest): Promise<ToolResult>;
  addArc3pt(request: Arc3PointRequest): Promise<ToolResult>;
  addArcCenter(request: ArcCenterRequest): Promise<ToolResult>;
  addConstraint(constraint: ConstraintPayload): Promise<AddConstraintResult>;
  addConstraints(constraints: ConstraintPayload[]): Promise<ToolResult>;
  addDimension(request: DimensionRequest): Promise<ToolResult>;
  editDimension(request: EditDimensionRequest): Promise<AddConstraintResult>;
  setDimensionMode(request: SetDimensionModeRequest): Promise<AddConstraintResult>;
  moveDimension(request: MoveDimensionRequest): Promise<AddConstraintResult>;
  deleteDimension(constraintId: number): Promise<AddConstraintResult>;
  deleteConstraint(constraintId: number): Promise<AddConstraintResult>;
  setDimensionStyle(style: DimensionStyle): Promise<SketchDto>;
  evalExpression(text: string): Promise<EvalExpressionResult>;
  filletPreview(request: FilletRequest): Promise<FilletPreviewDto>;
  filletLines(request: FilletRequest): Promise<ToolResult>;
  chamferLines(request: ChamferRequest): Promise<ToolResult>;
  offsetPreview(request: OffsetRequest): Promise<OffsetPreviewDto>;
  offsetCurve(request: OffsetRequest): Promise<ToolResult>;
  trimPreview(request: TrimRequest): Promise<TrimPreviewDto>;
  trimEntity(request: TrimRequest): Promise<ToolResult>;
  extendEntity(request: ExtendRequest): Promise<ToolResult>;
  breakCurve(request: BreakRequest): Promise<ToolResult>;
  mirrorEntities(request: MirrorRequest): Promise<ToolResult>;
  rectangularPattern(request: SketchRectangularPatternRequest): Promise<ToolResult>;
  circularPattern(request: SketchCircularPatternRequest): Promise<ToolResult>;
  moveCopyEntities(request: MoveCopyRequest): Promise<ToolResult>;
  scaleEntities(request: ScaleRequest): Promise<ToolResult>;
  polygonCreate(request: PolygonRequest): Promise<ToolResult>;
  toggleFix(entityId: number): Promise<AddConstraintResult>;
  toggleFixEntities(entityIds: number[]): Promise<ToolResult>;
  movePoint(request: MovePointRequest): Promise<MovePointResult>;
  deleteEntity(entityId: number): Promise<DeleteEntityResult>;
  deleteEntities(entityIds: number[]): Promise<DeleteEntityResult>;
  undo(): Promise<UndoResult>;
  redo(): Promise<UndoResult>;
  setGridSnap(enabled: boolean): Promise<SketchDto>;
  setGridStep(stepMm: number): Promise<void>;
}

export class EngineError extends Error {
  constructor(
    message: string,
    readonly data?: unknown,
  ) {
    super(message);
  }
}

/** A rejected Open is harmless only when the engine proves rejection happened
 * before replacing either the document or its native geometry. */
export class ProjectLoadError extends EngineError {
  constructor(
    readonly engineState: 'unchanged' | 'unverified',
    readonly cause: unknown,
  ) {
    super(cause instanceof Error ? cause.message : String(cause),
      cause instanceof EngineError ? cause.data : undefined);
    this.name = 'ProjectLoadError';
  }
}

/** Parse a host envelope string, throwing `EngineError` on failure. */
export function unwrapEnvelope<T>(json: string): T {
  const env = JSON.parse(json) as { ok: boolean; value?: T; error?: string; data?: unknown };
  if (env.ok !== true) {
    throw new EngineError(env.error ?? 'unknown engine error', env.data);
  }
  return env.value as T;
}

export function isTauriRuntime(): boolean {
  // `__TAURI_INTERNALS__` is always injected by Tauri 2. `__TAURI__` only
  // exists when the optional global API compatibility flag is enabled.
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

let enginePromise: Promise<Engine> | null = null;

/** Lazily create the singleton engine for this runtime. */
export function getEngine(): Promise<Engine> {
  if (!enginePromise) {
    // Vite replaces MODE at build time. Keeping the desktop branch explicit
    // lets Rollup remove the browser OCCT/Rust WASM graph from Tauri packages,
    // while ordinary browser builds retain the convenient development host.
    if (import.meta.env.MODE === 'desktop') {
      enginePromise = import('./tauri').then((m) => new m.TauriEngine());
    } else {
      enginePromise = isTauriRuntime()
        ? import('./tauri').then((m) => new m.TauriEngine())
        : import('./wasm').then((m) => m.WasmEngine.create());
    }
  }
  return enginePromise;
}
