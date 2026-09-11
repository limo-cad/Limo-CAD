/**
 * WasmEngine — engine adapter for the browser preview/dev path.
 *
 * Loads the wasm-pack bundle (`npm run build:wasm` →
 * `src/engine-wasm/pkg/`) and exposes the same `Engine` interface as the
 * Tauri host. The generated JavaScript and WASM are gitignored; only their
 * TypeScript declaration is retained so native desktop builds can type-check
 * without compiling the browser engine. The bundle is wasm-pack `--target
 * web`; Vite handles its `new URL(..., import.meta.url)` wasm asset natively,
 * so no Vite plugin is required.
 */
import init, { WasmEngine as WasmEngineInner } from '../engine-wasm/pkg/nbcad_wasm';
import { EngineError, ProjectLoadError, unwrapEnvelope, type Engine } from './index';
import { restoreLoadedDatumHistoryFrames } from './historyFrames';
import { BrowserOcctKernel } from './occtBrowser';
import { drawingInstanceScene, projectSceneForDrawing } from '../drawing/projection';

/** wasm-pack typings lag until `npm run build:wasm`; keep additive methods typed here. */
type WasmEngineMethods = WasmEngineInner & {
  body_appearances(): string;
  set_body_appearance(payload: string): string;
  project_visibility(): string;
  project_set_visibility(payload: string): string;
  drawing_document(): string;
  drawing_apply(payload: string): string;
  drawing_set_document(payload: string): string;
  assembly_document(): string;
  assembly_set_document(payload: string): string;
  assembly_solution(): string;
  assembly_create_component(payload: string): string;
  assembly_update_component(payload: string): string;
  assembly_create_occurrence(payload: string): string;
  assembly_update_occurrence(payload: string): string;
  assembly_duplicate_occurrence(payload: string): string;
  assembly_set_occurrence_grounded(payload: string): string;
  assembly_set_occurrence_pose(payload: string): string;
  assembly_preview_joint(payload: string): string;
  assembly_create_joint(payload: string): string;
  assembly_update_joint(payload: string): string;
  assembly_preview_joint_update(payload: string): string;
  assembly_delete_joint(payload: string): string;
  assembly_set_joint_enabled(payload: string): string;
  assembly_set_joint_motion(payload: string): string;
  assembly_preview_joint_motion(payload: string): string;
  assembly_set_joint_coordinates(payload: string): string;
  assembly_preview_joint_coordinates(payload: string): string;
  assembly_preview_mechanism_drag(payload: string): string;
  assembly_apply_joint_motions(payload: string): string;
  assembly_create_position(payload: string): string;
  assembly_update_position(payload: string): string;
  assembly_delete_position(payload: string): string;
  assembly_apply_position(payload: string): string;
  assembly_create_motion_study(payload: string): string;
  assembly_update_motion_study(payload: string): string;
  assembly_delete_motion_study(payload: string): string;
  assembly_sample_motion_study(payload: string): string;
  assembly_export_motion_path_csv(payload: string): string;
  assembly_create_contact_set(payload: string): string;
  assembly_update_contact_set(payload: string): string;
  assembly_delete_contact_set(payload: string): string;
  assembly_interference_check(payload: string): string;
  assembly_evaluate_motion_study(payload: string): string;
  assembly_swept_collision_check(payload: string): string;
  assembly_set_grounded_body(payload: string): string;
};
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
  ChamferRequest,
  SketchCircularPatternRequest,
  CircleRequest,
  ComponentDefinitionDto,
  ComponentOccurrenceDto,
  ConstraintPayload,
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
  FaceSketchOrigin,
  EditDimensionRequest,
  EndSketchResult,
  EvalExpressionResult,
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
  ExtendRequest,
  FilletPreviewDto,
  FilletRequest,
  LockedCircleRequest,
  SlotRequest,
  SplineRequest,
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
  PolygonRequest,
  PreviewDto,
  ProfileCatalogItemDto,
  ProjectVisibilityDto,
  RecomputePlanDto,
  RectangleRequest,
  SketchRectangularPatternRequest,
  ScaleRequest,
  SegmentRequest,
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

interface WasmProjectContext {
  inner: WasmEngineInner;
  kernelPromise: Promise<BrowserOcctKernel> | null;
}

export class WasmEngine implements Engine {
  readonly kind = 'wasm' as const;
  private readonly contexts = new Map<string, WasmProjectContext>();
  private activeContext: WasmProjectContext;
  private activeSessionId: string | null = null;

  private constructor(inner: WasmEngineInner) {
    this.activeContext = { inner, kernelPromise: null };
  }

  private get inner(): WasmEngineInner {
    return this.activeContext.inner;
  }

  /** Instantiate the wasm module and construct the engine. */
  static async create(): Promise<WasmEngine> {
    await init();
    return new WasmEngine(new WasmEngineInner());
  }

  async getDocument(): Promise<DocumentDto> {
    return unwrapEnvelope(this.inner.document());
  }

  async beginSketch(
    plane: PlaneRef,
    faceOrigin: FaceSketchOrigin = 'face_center',
  ): Promise<SketchDto> {
    return unwrapEnvelope(
      this.inner.begin_sketch(JSON.stringify({ plane, face_origin: faceOrigin })),
    );
  }

  async endSketch(): Promise<EndSketchResult> {
    return unwrapEnvelope(this.inner.end_sketch());
  }

  async finishedSketches(): Promise<SketchDto[]> {
    return unwrapEnvelope(this.inner.finished_sketches());
  }

  async editSketch(name: string): Promise<SketchDto> {
    return unwrapEnvelope(this.inner.edit_sketch(JSON.stringify(name)));
  }

  async activeSketch(): Promise<SketchDto | null> {
    return unwrapEnvelope(this.inner.active_sketch());
  }

  async profileCatalog(): Promise<ProfileCatalogItemDto[]> {
    return unwrapEnvelope(this.inner.profile_catalog());
  }

  async solidScene(): Promise<SolidSceneDto> {
    return unwrapEnvelope(this.inner.solid_scene());
  }

  async bodyAppearances(): Promise<BodyAppearance[]> {
    return unwrapEnvelope((this.inner as WasmEngineMethods).body_appearances());
  }

  async projectVisibility(): Promise<ProjectVisibilityDto> {
    return unwrapEnvelope((this.inner as WasmEngineMethods).project_visibility());
  }

  async setProjectVisibility(visibility: ProjectVisibilityDto): Promise<ProjectVisibilityDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).project_set_visibility(JSON.stringify(visibility)),
    );
  }

  async drawingDocument(): Promise<DrawingDocumentDto> {
    return unwrapEnvelope((this.inner as WasmEngineMethods).drawing_document());
  }

  async drawingApply(command: import('./types').DrawingCommandDto): Promise<DrawingDocumentDto> {
    return unwrapEnvelope((this.inner as WasmEngineMethods).drawing_apply(JSON.stringify(command)));
  }

  async setDrawingDocument(document: DrawingDocumentDto): Promise<DrawingDocumentDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).drawing_set_document(JSON.stringify(document)),
    );
  }

  async assemblyDocument(): Promise<AssemblyDocumentDto> {
    return unwrapEnvelope((this.inner as WasmEngineMethods).assembly_document());
  }

  async setAssemblyDocument(document: AssemblyDocumentDto): Promise<AssemblyDocumentDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_set_document(JSON.stringify(document)),
    );
  }

  async assemblySolution(): Promise<AssemblySolutionDto> {
    return unwrapEnvelope((this.inner as WasmEngineMethods).assembly_solution());
  }

  async createComponent(request: CreateComponentRequestDto): Promise<ComponentDefinitionDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_create_component(JSON.stringify(request)),
    );
  }

  async updateComponent(request: UpdateComponentRequestDto): Promise<ComponentDefinitionDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_update_component(JSON.stringify(request)),
    );
  }

  async createOccurrence(request: CreateOccurrenceRequestDto): Promise<ComponentOccurrenceDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_create_occurrence(JSON.stringify(request)),
    );
  }

  async updateOccurrence(request: UpdateOccurrenceRequestDto): Promise<ComponentOccurrenceDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_update_occurrence(JSON.stringify(request)),
    );
  }

  async duplicateOccurrence(request: DuplicateOccurrenceRequestDto): Promise<ComponentOccurrenceDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_duplicate_occurrence(JSON.stringify(request)),
    );
  }

  async setOccurrenceGrounded(request: SetOccurrenceGroundedRequestDto): Promise<AssemblyDocumentDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_set_occurrence_grounded(JSON.stringify(request)),
    );
  }

  async setOccurrencePose(request: SetOccurrencePoseRequestDto): Promise<AssemblyDocumentDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_set_occurrence_pose(JSON.stringify(request)),
    );
  }

  async previewJoint(request: CreateJointRequestDto): Promise<AssemblySolutionDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_preview_joint(JSON.stringify(request)),
    );
  }

  async createJoint(request: CreateJointRequestDto): Promise<JointDefinitionDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_create_joint(JSON.stringify(request)),
    );
  }

  async updateJoint(request: UpdateJointRequestDto): Promise<JointDefinitionDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_update_joint(JSON.stringify(request)),
    );
  }

  async previewJointUpdate(request: UpdateJointRequestDto): Promise<AssemblySolutionDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_preview_joint_update(JSON.stringify(request)),
    );
  }

  async deleteJoint(id: number): Promise<AssemblyDocumentDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_delete_joint(JSON.stringify(id)),
    );
  }

  async setJointEnabled(request: SetJointEnabledRequestDto): Promise<AssemblyDocumentDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_set_joint_enabled(JSON.stringify(request)),
    );
  }

  async setJointMotion(request: SetJointMotionRequestDto): Promise<AssemblyDocumentDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_set_joint_motion(JSON.stringify(request)),
    );
  }

  async previewJointMotion(request: SetJointMotionRequestDto): Promise<AssemblySolutionDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_preview_joint_motion(JSON.stringify(request)),
    );
  }

  async setJointCoordinates(request: SetJointCoordinatesRequestDto): Promise<AssemblyDocumentDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_set_joint_coordinates(JSON.stringify(request)),
    );
  }

  async previewJointCoordinates(request: SetJointCoordinatesRequestDto): Promise<AssemblySolutionDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_preview_joint_coordinates(JSON.stringify(request)),
    );
  }

  async previewMechanismDrag(request: MechanismDragRequestDto): Promise<MechanismPreviewDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_preview_mechanism_drag(JSON.stringify(request)),
    );
  }

  async applyJointMotions(request: ApplyJointMotionsRequestDto): Promise<AssemblyDocumentDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_apply_joint_motions(JSON.stringify(request)),
    );
  }

  async createAssemblyPosition(request: CreateAssemblyPositionRequestDto): Promise<AssemblyPositionDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_create_position(JSON.stringify(request)),
    );
  }

  async updateAssemblyPosition(position: AssemblyPositionDto): Promise<AssemblyPositionDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_update_position(JSON.stringify(position)),
    );
  }

  async deleteAssemblyPosition(id: number): Promise<AssemblyDocumentDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_delete_position(JSON.stringify(id)),
    );
  }

  async applyAssemblyPosition(id: number): Promise<AssemblyDocumentDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_apply_position(JSON.stringify(id)),
    );
  }

  async createMotionStudy(request: CreateMotionStudyRequestDto): Promise<MotionStudyDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_create_motion_study(JSON.stringify(request)),
    );
  }

  async updateMotionStudy(study: MotionStudyDto): Promise<MotionStudyDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_update_motion_study(JSON.stringify(study)),
    );
  }

  async deleteMotionStudy(id: number): Promise<AssemblyDocumentDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_delete_motion_study(JSON.stringify(id)),
    );
  }

  async sampleMotionStudy(request: SampleMotionStudyRequestDto): Promise<MotionStudySampleDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_sample_motion_study(JSON.stringify(request)),
    );
  }

  async evaluateMotionStudy(request: EvaluateMotionStudyRequestDto): Promise<MotionStudyEvaluationDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_evaluate_motion_study(JSON.stringify(request)),
    );
  }

  async exportMotionPathCsv(request: MotionPathRequestDto): Promise<string> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_export_motion_path_csv(JSON.stringify(request)),
    );
  }

  async createContactSet(request: CreateContactSetRequestDto): Promise<ContactSetDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_create_contact_set(JSON.stringify(request)),
    );
  }

  async updateContactSet(contact: ContactSetDto): Promise<ContactSetDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_update_contact_set(JSON.stringify(contact)),
    );
  }

  async deleteContactSet(id: number): Promise<AssemblyDocumentDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_delete_contact_set(JSON.stringify(id)),
    );
  }

  async interferenceCheck(request: InterferenceCheckRequestDto): Promise<InterferenceReportDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_interference_check(JSON.stringify(request)),
    );
  }

  async sweptCollisionCheck(request: SweptCollisionRequestDto): Promise<SweptCollisionReportDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_swept_collision_check(JSON.stringify(request)),
    );
  }

  async setGroundedBody(bodyId: number | null): Promise<AssemblyDocumentDto> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).assembly_set_grounded_body(JSON.stringify(bodyId)),
    );
  }

  async drawingProjection(request: DrawingProjectionRequest): Promise<DrawingProjectionDto> {
    let scene = await this.solidScene();
    if (request.scope === 'assembly') {
      scene = drawingInstanceScene(scene, await this.assemblySolution(), await this.assemblyDocument(), request.occurrence_ids ?? []);
      if (scene.bodies.length === 0) throw new Error('The selected assembly drawing contains no visible body occurrences.');
    }
    return projectSceneForDrawing(scene, request);
  }

  async setBodyAppearance(appearance: BodyAppearance): Promise<BodyAppearance[]> {
    return unwrapEnvelope(
      (this.inner as WasmEngineMethods).set_body_appearance(JSON.stringify(appearance)),
    );
  }

  async extrudeDefinitions(): Promise<ExtrudeDefinitionDto[]> {
    return unwrapEnvelope(this.inner.extrude_definitions());
  }

  async revolveDefinitions(): Promise<RevolveDefinitionDto[]> {
    return unwrapEnvelope(this.inner.revolve_definitions());
  }

  async sweepDefinitions(): Promise<SweepDefinitionDto[]> {
    return unwrapEnvelope(this.inner.sweep_definitions());
  }

  async loftDefinitions(): Promise<LoftDefinitionDto[]> {
    return unwrapEnvelope(this.inner.loft_definitions());
  }

  async ribDefinitions(): Promise<RibDefinitionDto[]> {
    return unwrapEnvelope(this.inner.rib_definitions());
  }

  async filletDefinitions(): Promise<SolidFilletDefinitionDto[]> {
    return unwrapEnvelope(this.inner.fillet_definitions());
  }

  async chamferDefinitions(): Promise<SolidChamferDefinitionDto[]> {
    return unwrapEnvelope(this.inner.chamfer_definitions());
  }

  async holeDefinitions(): Promise<HoleDefinitionDto[]> {
    return unwrapEnvelope(this.inner.hole_definitions());
  }

  async datumPlaneDefinitions(): Promise<DatumPlaneDefinitionDto[]> {
    return unwrapEnvelope(this.inner.datum_plane_definitions());
  }

  async bodyFeatureDefinitions(): Promise<BodyFeatureDefinitionDto[]> {
    return unwrapEnvelope(this.inner.body_feature_definitions());
  }

  async createDatumPlane(request: DatumPlaneRequest): Promise<DatumPlaneUpdateDto> {
    return unwrapEnvelope(this.inner.datum_plane_create(JSON.stringify(request)));
  }

  async editDatumPlane(
    featureId: number,
    request: DatumPlaneRequest,
  ): Promise<DatumPlaneUpdateDto> {
    return unwrapEnvelope(
      this.inner.datum_plane_edit(
        JSON.stringify({ feature_id: featureId, plane: request }),
      ),
    );
  }

  async bodyFeature(request: BodyFeatureRequestDto): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(
      this.inner.solid_prepare_body_feature(JSON.stringify(request)),
    );
    return this.executeSolidPlan(plan);
  }

  async editBodyFeature(
    featureId: number,
    request: BodyFeatureRequestDto,
  ): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(
      this.inner.solid_prepare_edit_body_feature(
        JSON.stringify({ feature_id: featureId, feature: request }),
      ),
    );
    return this.executeSolidPlan(plan);
  }

  async extrude(request: ExtrudeRequest): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(
      this.inner.solid_prepare_extrude(JSON.stringify(request)),
    );
    return this.executeSolidPlan(plan);
  }

  async editExtrude(featureId: number, request: ExtrudeRequest): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(
      this.inner.solid_prepare_edit_extrude(
        JSON.stringify({ feature_id: featureId, extrude: request }),
      ),
    );
    return this.executeSolidPlan(plan);
  }

  async revolve(request: RevolveRequest): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(
      this.inner.solid_prepare_revolve(JSON.stringify(request)),
    );
    return this.executeSolidPlan(plan);
  }

  async editRevolve(featureId: number, request: RevolveRequest): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(
      this.inner.solid_prepare_edit_revolve(
        JSON.stringify({ feature_id: featureId, revolve: request }),
      ),
    );
    return this.executeSolidPlan(plan);
  }

  async sweep(request: SweepRequest): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(
      this.inner.solid_prepare_sweep(JSON.stringify(request)),
    );
    return this.executeSolidPlan(plan);
  }

  async editSweep(featureId: number, request: SweepRequest): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(
      this.inner.solid_prepare_edit_sweep(
        JSON.stringify({ feature_id: featureId, sweep: request }),
      ),
    );
    return this.executeSolidPlan(plan);
  }

  async loft(request: LoftRequest): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(
      this.inner.solid_prepare_loft(JSON.stringify(request)),
    );
    return this.executeSolidPlan(plan);
  }

  async editLoft(featureId: number, request: LoftRequest): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(
      this.inner.solid_prepare_edit_loft(
        JSON.stringify({ feature_id: featureId, loft: request }),
      ),
    );
    return this.executeSolidPlan(plan);
  }

  async rib(request: RibRequest): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(
      this.inner.solid_prepare_rib(JSON.stringify(request)),
    );
    return this.executeSolidPlan(plan);
  }

  async editRib(featureId: number, request: RibRequest): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(
      this.inner.solid_prepare_edit_rib(
        JSON.stringify({ feature_id: featureId, rib: request }),
      ),
    );
    return this.executeSolidPlan(plan);
  }

  async solidFillet(request: SolidFilletRequest): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(
      this.inner.solid_prepare_fillet(JSON.stringify(request)),
    );
    return this.executeSolidPlan(plan);
  }

  async editSolidFillet(featureId: number, request: SolidFilletRequest): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(
      this.inner.solid_prepare_edit_fillet(
        JSON.stringify({ feature_id: featureId, fillet: request }),
      ),
    );
    return this.executeSolidPlan(plan);
  }

  async solidChamfer(request: SolidChamferRequest): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(
      this.inner.solid_prepare_chamfer(JSON.stringify(request)),
    );
    return this.executeSolidPlan(plan);
  }

  async editSolidChamfer(featureId: number, request: SolidChamferRequest): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(
      this.inner.solid_prepare_edit_chamfer(
        JSON.stringify({ feature_id: featureId, chamfer: request }),
      ),
    );
    return this.executeSolidPlan(plan);
  }

  async hole(request: HoleRequest): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(
      this.inner.solid_prepare_hole(JSON.stringify(request)),
    );
    return this.executeSolidPlan(plan);
  }

  async editHole(featureId: number, request: HoleRequest): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(
      this.inner.solid_prepare_edit_hole(
        JSON.stringify({ feature_id: featureId, hole: request }),
      ),
    );
    return this.executeSolidPlan(plan);
  }

  async recomputeSolids(): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(this.inner.solid_prepare_recompute());
    return this.executeSolidPlan(plan);
  }

  async setRollback(rollbackIndex: number): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(
      this.inner.solid_prepare_set_rollback(JSON.stringify({ rollback_index: rollbackIndex })),
    );
    return this.executeSolidPlan(plan);
  }

  async deleteFeature(featureId: number): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(
      this.inner.solid_prepare_delete_feature(JSON.stringify({ feature_id: featureId })),
    );
    return this.executeSolidPlan(plan);
  }

  async reorderFeature(featureId: number, targetIndex: number): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(
      this.inner.solid_prepare_reorder_feature(
        JSON.stringify({ feature_id: featureId, target_index: targetIndex }),
      ),
    );
    return this.executeSolidPlan(plan);
  }

  async setDocumentName(name: string, expectedModelJson?: string): Promise<DocumentDto> {
    return unwrapEnvelope(this.inner.document_set_name(JSON.stringify(expectedModelJson === undefined ? name : { name, expected_model_json: expectedModelJson })));
  }

  async exportProjectModel(options?: { expected_model_json: string; save_name?: string }): Promise<string> {
    return unwrapEnvelope(this.inner.project_export_model(options === undefined ? undefined : JSON.stringify(options)));
  }

  async bindProjectSession(sessionId: string): Promise<void> {
    this.validateSessionId(sessionId);
    if (this.activeSessionId === sessionId) return;
    const retained = this.contexts.get(sessionId);
    if (retained) {
      this.activeContext = retained;
      this.activeSessionId = sessionId;
      return;
    }
    if (this.activeSessionId !== null || this.contexts.size !== 0) {
      throw new Error('the bootstrap project session is already bound');
    }
    this.contexts.set(sessionId, this.activeContext);
    this.activeSessionId = sessionId;
  }

  async createProjectSession(sessionId: string): Promise<SolidUpdateDto> {
    this.validateSessionId(sessionId);
    if (this.contexts.has(sessionId)) {
      throw new Error('project session already exists');
    }
    if (this.contexts.size >= 128) {
      throw new Error('too many resident project sessions');
    }
    const context: WasmProjectContext = {
      inner: new WasmEngineInner(),
      kernelPromise: null,
    };
    this.contexts.set(sessionId, context);
    this.activeContext = context;
    this.activeSessionId = sessionId;
    return this.currentUpdate();
  }

  async activateProjectSession(sessionId: string): Promise<boolean> {
    this.validateSessionId(sessionId);
    const context = this.contexts.get(sessionId);
    if (!context) return false;
    this.activeContext = context;
    this.activeSessionId = sessionId;
    return true;
  }

  async dropProjectSession(sessionId: string): Promise<void> {
    this.validateSessionId(sessionId);
    if (this.activeSessionId === sessionId) {
      throw new Error('cannot drop the active project session');
    }
    const context = this.contexts.get(sessionId);
    if (!context) return;
    this.contexts.delete(sessionId);
    context.inner.free();
    if (context.kernelPromise) {
      void context.kernelPromise
        .then((kernel) => kernel.dispose())
        .catch(() => undefined);
    }
  }

  async newProject(): Promise<SolidUpdateDto> {
    const plan = unwrapEnvelope<RecomputePlanDto>(this.inner.project_prepare_new());
    return this.executeSolidPlan(plan);
  }

  async loadProjectModel(modelJson: string): Promise<SolidUpdateDto> {
    let plan: RecomputePlanDto;
    try {
      plan = unwrapEnvelope<RecomputePlanDto>(
        this.inner.project_prepare_load(JSON.stringify(modelJson)),
      );
    } catch (error) {
      // A Rust prepare-error envelope precedes all kernel work. A JS/WASM
      // trap or unreadable response has no such guarantee.
      throw new ProjectLoadError(error instanceof EngineError ? 'unchanged' : 'unverified', error);
    }
    try {
      const update = await this.executeSolidPlan(plan);
      return await restoreLoadedDatumHistoryFrames(this, update);
    } catch (error) {
      throw new ProjectLoadError('unverified', error);
    }
  }

  async exportStep(request: StepExportRequest): Promise<Uint8Array> {
    const owner = this.activeContext;
    const kernel = await this.browserKernel();
    // Lazy OCCT initialization yields to tab changes and model edits. After
    // checking ownership, no await may separate the snapshot check and export.
    if (this.activeContext !== owner || (request.expected_model_json !== undefined
      && unwrapEnvelope<string>(owner.inner.project_export_model()) !== request.expected_model_json)) {
      throw new Error('The document changed while preparing export. Start the export again.');
    }
    return kernel.exportStep(request);
  }

  async exportStl(request: MeshExportRequest): Promise<Uint8Array> {
    return (await this.browserKernel()).exportStl(request);
  }

  async export3mf(request: MeshExportRequest): Promise<Uint8Array> {
    return (await this.browserKernel()).export3mf(request);
  }

  private async executeSolidPlan(plan: RecomputePlanDto): Promise<SolidUpdateDto> {
    let scene;
    if (plan.jobs.length === 0) {
      if (this.activeContext.kernelPromise) {
        (await this.activeContext.kernelPromise).clear();
      }
      scene = { bodies: [], errors: plan.errors ?? [] };
    } else {
      scene = (await this.browserKernel()).recompute(plan);
    }
    return unwrapEnvelope(
      this.inner.solid_commit(JSON.stringify({ transaction_id: plan.transaction_id, scene })),
    );
  }

  private browserKernel(): Promise<BrowserOcctKernel> {
    this.activeContext.kernelPromise ??= BrowserOcctKernel.create();
    return this.activeContext.kernelPromise;
  }

  private currentUpdate(): SolidUpdateDto {
    return {
      document: unwrapEnvelope(this.inner.document()),
      scene: unwrapEnvelope(this.inner.solid_scene()),
    };
  }

  private validateSessionId(sessionId: string): void {
    if (sessionId.length === 0 || sessionId.length > 128 || sessionId === '__bootstrap__') {
      throw new Error('invalid project session id');
    }
  }

  async previewSegment(request: SegmentRequest): Promise<PreviewDto> {
    return unwrapEnvelope(this.inner.preview_segment(JSON.stringify(request)));
  }

  async addLine(request: SegmentRequest): Promise<AddLineResult> {
    return unwrapEnvelope(this.inner.add_line(JSON.stringify(request)));
  }

  async previewSegmentLocked(request: LockedSegmentRequest): Promise<PreviewDto> {
    return unwrapEnvelope(this.inner.preview_segment_locked(JSON.stringify(request)));
  }

  async addLineLocked(request: LockedSegmentRequest): Promise<AddLineResult> {
    return unwrapEnvelope(this.inner.add_line_locked(JSON.stringify(request)));
  }

  async addPoint(request: PointRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.add_point(JSON.stringify(request)));
  }

  async addLineMidpoint(request: MidpointLineRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.add_line_midpoint(JSON.stringify(request)));
  }

  async addRectangle(request: RectangleRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.add_rectangle(JSON.stringify(request)));
  }

  async addRectangleLocked(request: LockedRectangleRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.add_rectangle_locked(JSON.stringify(request)));
  }

  async addCircle(request: CircleRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.add_circle(JSON.stringify(request)));
  }

  async addCircleLocked(request: LockedCircleRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.add_circle_locked(JSON.stringify(request)));
  }

  async addSlot(request: SlotRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.add_slot(JSON.stringify(request)));
  }

  async addSpline(request: SplineRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.add_spline(JSON.stringify(request)));
  }

  async addArc3pt(request: Arc3PointRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.add_arc_3pt(JSON.stringify(request)));
  }

  async addArcCenter(request: ArcCenterRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.add_arc_center(JSON.stringify(request)));
  }

  async addConstraint(constraint: ConstraintPayload): Promise<AddConstraintResult> {
    return unwrapEnvelope(this.inner.add_constraint(JSON.stringify(constraint)));
  }

  async addConstraints(constraints: ConstraintPayload[]): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.add_constraints(JSON.stringify({ constraints })));
  }

  async addDimension(request: DimensionRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.add_dimension(JSON.stringify(request)));
  }

  async editDimension(request: EditDimensionRequest): Promise<AddConstraintResult> {
    return unwrapEnvelope(this.inner.edit_dimension(JSON.stringify(request)));
  }

  async setDimensionMode(request: SetDimensionModeRequest): Promise<AddConstraintResult> {
    return unwrapEnvelope(this.inner.set_dimension_mode(JSON.stringify(request)));
  }

  async moveDimension(request: MoveDimensionRequest): Promise<AddConstraintResult> {
    return unwrapEnvelope(this.inner.move_dimension(JSON.stringify(request)));
  }

  async deleteDimension(constraintId: number): Promise<AddConstraintResult> {
    return unwrapEnvelope(this.inner.delete_dimension(JSON.stringify({ constraint_id: constraintId })));
  }

  async deleteConstraint(constraintId: number): Promise<AddConstraintResult> {
    // `wasm-pack` generated bindings may lag behind interface changes during
    // development; treat the export as dynamic to keep the TS build green.
    const inner = this.inner as any;
    return unwrapEnvelope(
      inner.delete_constraint(JSON.stringify({ constraint_id: constraintId })),
    );
  }

  async setDimensionStyle(style: DimensionStyle): Promise<SketchDto> {
    return unwrapEnvelope(this.inner.set_dimension_style(JSON.stringify({ style })));
  }

  async evalExpression(text: string): Promise<EvalExpressionResult> {
    return unwrapEnvelope(this.inner.eval_expression(JSON.stringify({ text })));
  }


  async filletPreview(request: FilletRequest): Promise<FilletPreviewDto> {
    return unwrapEnvelope(this.inner.fillet_preview(JSON.stringify(request)));
  }
  async filletLines(request: FilletRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.fillet_lines(JSON.stringify(request)));
  }
  async chamferLines(request: ChamferRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.chamfer_lines(JSON.stringify(request)));
  }
  async offsetPreview(request: OffsetRequest): Promise<OffsetPreviewDto> {
    return unwrapEnvelope(this.inner.offset_preview(JSON.stringify(request)));
  }
  async offsetCurve(request: OffsetRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.offset_curve(JSON.stringify(request)));
  }
  async trimPreview(request: TrimRequest): Promise<TrimPreviewDto> {
    return unwrapEnvelope(this.inner.trim_preview(JSON.stringify(request)));
  }
  async trimEntity(request: TrimRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.trim_entity(JSON.stringify(request)));
  }
  async extendEntity(request: ExtendRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.extend_entity(JSON.stringify(request)));
  }
  async breakCurve(request: BreakRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.break_curve(JSON.stringify(request)));
  }
  async mirrorEntities(request: MirrorRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.mirror_entities(JSON.stringify(request)));
  }
  async rectangularPattern(request: SketchRectangularPatternRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.rectangular_pattern(JSON.stringify(request)));
  }
  async circularPattern(request: SketchCircularPatternRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.circular_pattern(JSON.stringify(request)));
  }
  async moveCopyEntities(request: MoveCopyRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.move_copy_entities(JSON.stringify(request)));
  }
  async scaleEntities(request: ScaleRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.scale_entities(JSON.stringify(request)));
  }
  async polygonCreate(request: PolygonRequest): Promise<ToolResult> {
    return unwrapEnvelope(this.inner.polygon_create(JSON.stringify(request)));
  }

  async toggleFix(entityId: number): Promise<AddConstraintResult> {
    return unwrapEnvelope(this.inner.toggle_fix(JSON.stringify({ entity_id: entityId })));
  }

  async toggleFixEntities(entityIds: number[]): Promise<ToolResult> {
    return unwrapEnvelope(
      this.inner.toggle_fix_entities(JSON.stringify({ entity_ids: entityIds })),
    );
  }

  async deleteEntities(entityIds: number[]): Promise<DeleteEntityResult> {
    return unwrapEnvelope(this.inner.delete_entities(JSON.stringify({ entity_ids: entityIds })));
  }

  async movePoint(request: MovePointRequest): Promise<MovePointResult> {
    return unwrapEnvelope(this.inner.move_point(JSON.stringify(request)));
  }

  async deleteEntity(entityId: number): Promise<DeleteEntityResult> {
    return unwrapEnvelope(this.inner.delete_entity(JSON.stringify({ entity_id: entityId })));
  }

  async undo(): Promise<UndoResult> {
    return unwrapEnvelope(this.inner.undo());
  }

  async redo(): Promise<UndoResult> {
    return unwrapEnvelope(this.inner.redo());
  }

  async setGridSnap(enabled: boolean): Promise<SketchDto> {
    return unwrapEnvelope(this.inner.set_grid_snap(JSON.stringify({ enabled })));
  }

  async setGridStep(stepMm: number): Promise<void> {
    unwrapEnvelope(this.inner.set_grid_step(JSON.stringify({ step_mm: stepMm })));
  }
}
