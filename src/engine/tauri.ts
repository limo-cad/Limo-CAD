/**
 * TauriEngine — engine adapter for the native shell (production path).
 *
 * Every method invokes the same-named Tauri command, which dispatches
 * through `nbcad_sketch::host::handle` in Rust. Payloads are JSON
 * strings, exactly like the WASM host.
 */
import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import {trackEngineOperation} from './activity';
function invoke<T>(...args: Parameters<typeof tauriInvoke>): Promise<T> {
  return trackEngineOperation(tauriInvoke<T>(...args));
}
import { EngineError, ProjectLoadError, unwrapEnvelope, type Engine } from './index';
import { restoreLoadedDatumHistoryFrames } from './historyFrames';
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

export class TauriEngine implements Engine {
  readonly kind = 'tauri' as const;

  private async call<T>(command: string, payload?: unknown): Promise<T> {
    const json =
      payload === undefined
        ? await invoke<string>(command)
        : await invoke<string>(command, { payload: JSON.stringify(payload) });
    return unwrapEnvelope(json);
  }

  async getDocument(): Promise<DocumentDto> {
    return invoke<DocumentDto>('get_document');
  }

  async beginSketch(
    plane: PlaneRef,
    faceOrigin: FaceSketchOrigin = 'face_center',
  ): Promise<SketchDto> {
    return this.call('engine_begin_sketch', { plane, face_origin: faceOrigin });
  }

  async endSketch(): Promise<EndSketchResult> {
    return this.call('engine_end_sketch');
  }

  async finishedSketches(): Promise<SketchDto[]> {
    return this.call('engine_finished_sketches');
  }

  async editSketch(name: string): Promise<SketchDto> {
    return this.call('engine_edit_sketch', name);
  }

  async activeSketch(): Promise<SketchDto | null> {
    return this.call('engine_active_sketch');
  }

  async profileCatalog(): Promise<ProfileCatalogItemDto[]> {
    return this.call('engine_profile_catalog');
  }

  async solidScene(): Promise<SolidSceneDto> {
    return this.call('engine_solid_scene');
  }

  async bodyAppearances(): Promise<BodyAppearance[]> {
    return this.call('engine_body_appearances');
  }

  async projectVisibility(): Promise<ProjectVisibilityDto> {
    return this.call('engine_project_visibility');
  }

  async setProjectVisibility(visibility: ProjectVisibilityDto): Promise<ProjectVisibilityDto> {
    return this.call('engine_project_set_visibility', visibility);
  }

  async setConstructionVisibility(request: import('./types').ConstructionVisibilityRequest): Promise<ProjectVisibilityDto> {
    return this.call('engine_construction_set_visibility', request);
  }

  async drawingDocument(): Promise<DrawingDocumentDto> {
    return this.call('engine_drawing_document');
  }

  async drawingApply(command: import('./types').DrawingCommandDto): Promise<DrawingDocumentDto> {
    return this.call('engine_drawing_apply',command);
  }

  async setDrawingDocument(document: DrawingDocumentDto): Promise<DrawingDocumentDto> {
    return this.call('engine_drawing_set_document', document);
  }

  async assemblyDocument(): Promise<AssemblyDocumentDto> {
    return this.call('engine_assembly_document');
  }

  async setAssemblyDocument(document: AssemblyDocumentDto): Promise<AssemblyDocumentDto> {
    return this.call('engine_assembly_set_document', document);
  }

  async assemblySolution(): Promise<AssemblySolutionDto> {
    return this.call('engine_assembly_solution');
  }

  async createComponent(request: CreateComponentRequestDto): Promise<ComponentDefinitionDto> {
    return this.call('engine_assembly_create_component', request);
  }

  async updateComponent(request: UpdateComponentRequestDto): Promise<ComponentDefinitionDto> {
    return this.call('engine_assembly_update_component', request);
  }

  async createOccurrence(request: CreateOccurrenceRequestDto): Promise<ComponentOccurrenceDto> {
    return this.call('engine_assembly_create_occurrence', request);
  }

  async updateOccurrence(request: UpdateOccurrenceRequestDto): Promise<ComponentOccurrenceDto> {
    return this.call('engine_assembly_update_occurrence', request);
  }

  async duplicateOccurrence(request: DuplicateOccurrenceRequestDto): Promise<ComponentOccurrenceDto> {
    return this.call('engine_assembly_duplicate_occurrence', request);
  }

  async setOccurrenceGrounded(request: SetOccurrenceGroundedRequestDto): Promise<AssemblyDocumentDto> {
    return this.call('engine_assembly_set_occurrence_grounded', request);
  }

  async setOccurrencePose(request: SetOccurrencePoseRequestDto): Promise<AssemblyDocumentDto> {
    return this.call('engine_assembly_set_occurrence_pose', request);
  }

  async previewJoint(request: CreateJointRequestDto): Promise<AssemblySolutionDto> {
    return this.call('engine_assembly_preview_joint', request);
  }

  async createJoint(request: CreateJointRequestDto): Promise<JointDefinitionDto> {
    return this.call('engine_assembly_create_joint', request);
  }

  async updateJoint(request: UpdateJointRequestDto): Promise<JointDefinitionDto> {
    return this.call('engine_assembly_update_joint', request);
  }

  async previewJointUpdate(request: UpdateJointRequestDto): Promise<AssemblySolutionDto> {
    return this.call('engine_assembly_preview_joint_update', request);
  }

  async deleteJoint(id: number): Promise<AssemblyDocumentDto> {
    return this.call('engine_assembly_delete_joint', id);
  }

  async setJointEnabled(request: SetJointEnabledRequestDto): Promise<AssemblyDocumentDto> {
    return this.call('engine_assembly_set_joint_enabled', request);
  }

  async setJointMotion(request: SetJointMotionRequestDto): Promise<AssemblyDocumentDto> {
    return this.call('engine_assembly_set_joint_motion', request);
  }

  async previewJointMotion(request: SetJointMotionRequestDto): Promise<AssemblySolutionDto> {
    return this.call('engine_assembly_preview_joint_motion', request);
  }

  async setJointCoordinates(request: SetJointCoordinatesRequestDto): Promise<AssemblyDocumentDto> {
    return this.call('engine_assembly_set_joint_coordinates', request);
  }

  async previewJointCoordinates(request: SetJointCoordinatesRequestDto): Promise<AssemblySolutionDto> {
    return this.call('engine_assembly_preview_joint_coordinates', request);
  }

  async previewMechanismDrag(request: MechanismDragRequestDto): Promise<MechanismPreviewDto> {
    return this.call('engine_assembly_preview_mechanism_drag', request);
  }

  async applyJointMotions(request: ApplyJointMotionsRequestDto): Promise<AssemblyDocumentDto> {
    return this.call('engine_assembly_apply_joint_motions', request);
  }

  async createAssemblyPosition(request: CreateAssemblyPositionRequestDto): Promise<AssemblyPositionDto> {
    return this.call('engine_assembly_create_position', request);
  }

  async updateAssemblyPosition(position: AssemblyPositionDto): Promise<AssemblyPositionDto> {
    return this.call('engine_assembly_update_position', position);
  }

  async deleteAssemblyPosition(id: number): Promise<AssemblyDocumentDto> {
    return this.call('engine_assembly_delete_position', id);
  }

  async applyAssemblyPosition(id: number): Promise<AssemblyDocumentDto> {
    return this.call('engine_assembly_apply_position', id);
  }

  async createMotionStudy(request: CreateMotionStudyRequestDto): Promise<MotionStudyDto> {
    return this.call('engine_assembly_create_motion_study', request);
  }

  async updateMotionStudy(study: MotionStudyDto): Promise<MotionStudyDto> {
    return this.call('engine_assembly_update_motion_study', study);
  }

  async deleteMotionStudy(id: number): Promise<AssemblyDocumentDto> {
    return this.call('engine_assembly_delete_motion_study', id);
  }

  async sampleMotionStudy(request: SampleMotionStudyRequestDto): Promise<MotionStudySampleDto> {
    return this.call('engine_assembly_sample_motion_study', request);
  }

  async evaluateMotionStudy(request: EvaluateMotionStudyRequestDto): Promise<MotionStudyEvaluationDto> {
    return this.call('engine_assembly_evaluate_motion_study', request);
  }

  async exportMotionPathCsv(request: MotionPathRequestDto): Promise<string> {
    return this.call('engine_assembly_export_motion_path_csv', request);
  }

  async createContactSet(request: CreateContactSetRequestDto): Promise<ContactSetDto> {
    return this.call('engine_assembly_create_contact_set', request);
  }

  async updateContactSet(contact: ContactSetDto): Promise<ContactSetDto> {
    return this.call('engine_assembly_update_contact_set', contact);
  }

  async deleteContactSet(id: number): Promise<AssemblyDocumentDto> {
    return this.call('engine_assembly_delete_contact_set', id);
  }

  async interferenceCheck(request: InterferenceCheckRequestDto): Promise<InterferenceReportDto> {
    return this.call('engine_assembly_interference_check', request);
  }

  async sweptCollisionCheck(request: SweptCollisionRequestDto): Promise<SweptCollisionReportDto> {
    return this.call('engine_assembly_swept_collision_check', request);
  }

  async setGroundedBody(bodyId: number | null): Promise<AssemblyDocumentDto> {
    return this.call('engine_assembly_set_grounded_body', bodyId);
  }

  async drawingProjection(request: DrawingProjectionRequest): Promise<DrawingProjectionDto> {
    return this.call('engine_drawing_projection', request);
  }

  async setBodyAppearance(appearance: BodyAppearance): Promise<BodyAppearance[]> {
    return this.call('engine_set_body_appearance', appearance);
  }

  async extrudeDefinitions(): Promise<ExtrudeDefinitionDto[]> {
    return this.call('engine_extrude_definitions');
  }

  async revolveDefinitions(): Promise<RevolveDefinitionDto[]> {
    return this.call('engine_revolve_definitions');
  }

  async sweepDefinitions(): Promise<SweepDefinitionDto[]> {
    return this.call('engine_sweep_definitions');
  }

  async loftDefinitions(): Promise<LoftDefinitionDto[]> {
    return this.call('engine_loft_definitions');
  }

  async ribDefinitions(): Promise<RibDefinitionDto[]> {
    return this.call('engine_rib_definitions');
  }

  async filletDefinitions(): Promise<SolidFilletDefinitionDto[]> {
    return this.call('engine_fillet_definitions');
  }

  async chamferDefinitions(): Promise<SolidChamferDefinitionDto[]> {
    return this.call('engine_chamfer_definitions');
  }

  async holeDefinitions(): Promise<HoleDefinitionDto[]> {
    return this.call('engine_hole_definitions');
  }

  async datumPlaneDefinitions(): Promise<DatumPlaneDefinitionDto[]> {
    return this.call('engine_datum_plane_definitions');
  }

  async bodyFeatureDefinitions(): Promise<BodyFeatureDefinitionDto[]> {
    return this.call('engine_body_feature_definitions');
  }

  async createDatumPlane(request: DatumPlaneRequest): Promise<DatumPlaneUpdateDto> {
    return this.call('engine_datum_plane_create', request);
  }

  async editDatumPlane(
    featureId: number,
    request: DatumPlaneRequest,
  ): Promise<DatumPlaneUpdateDto> {
    return this.call('engine_datum_plane_edit', {
      feature_id: featureId,
      plane: request,
    });
  }

  async bodyFeature(request: BodyFeatureRequestDto): Promise<SolidUpdateDto> {
    return this.call('engine_solid_body_feature', request);
  }

  async editBodyFeature(
    featureId: number,
    request: BodyFeatureRequestDto,
  ): Promise<SolidUpdateDto> {
    return this.call('engine_solid_edit_body_feature', {
      feature_id: featureId,
      feature: request,
    });
  }

  async extrude(request: ExtrudeRequest): Promise<SolidUpdateDto> {
    return this.call('engine_solid_extrude', request);
  }

  async editExtrude(featureId: number, request: ExtrudeRequest): Promise<SolidUpdateDto> {
    return this.call('engine_solid_edit_extrude', { feature_id: featureId, extrude: request });
  }

  async revolve(request: RevolveRequest): Promise<SolidUpdateDto> {
    return this.call('engine_solid_revolve', request);
  }

  async editRevolve(featureId: number, request: RevolveRequest): Promise<SolidUpdateDto> {
    return this.call('engine_solid_edit_revolve', { feature_id: featureId, revolve: request });
  }

  async sweep(request: SweepRequest): Promise<SolidUpdateDto> {
    return this.call('engine_solid_sweep', request);
  }

  async editSweep(featureId: number, request: SweepRequest): Promise<SolidUpdateDto> {
    return this.call('engine_solid_edit_sweep', { feature_id: featureId, sweep: request });
  }

  async loft(request: LoftRequest): Promise<SolidUpdateDto> {
    return this.call('engine_solid_loft', request);
  }

  async editLoft(featureId: number, request: LoftRequest): Promise<SolidUpdateDto> {
    return this.call('engine_solid_edit_loft', { feature_id: featureId, loft: request });
  }

  async rib(request: RibRequest): Promise<SolidUpdateDto> {
    return this.call('engine_solid_rib', request);
  }

  async editRib(featureId: number, request: RibRequest): Promise<SolidUpdateDto> {
    return this.call('engine_solid_edit_rib', { feature_id: featureId, rib: request });
  }

  async solidFillet(request: SolidFilletRequest): Promise<SolidUpdateDto> {
    return this.call('engine_solid_fillet', request);
  }

  async editSolidFillet(featureId: number, request: SolidFilletRequest): Promise<SolidUpdateDto> {
    return this.call('engine_solid_edit_fillet', { feature_id: featureId, fillet: request });
  }

  async solidChamfer(request: SolidChamferRequest): Promise<SolidUpdateDto> {
    return this.call('engine_solid_chamfer', request);
  }

  async editSolidChamfer(featureId: number, request: SolidChamferRequest): Promise<SolidUpdateDto> {
    return this.call('engine_solid_edit_chamfer', { feature_id: featureId, chamfer: request });
  }

  async hole(request: HoleRequest): Promise<SolidUpdateDto> {
    return this.call('engine_solid_hole', request);
  }

  async editHole(featureId: number, request: HoleRequest): Promise<SolidUpdateDto> {
    return this.call('engine_solid_edit_hole', { feature_id: featureId, hole: request });
  }

  async recomputeSolids(): Promise<SolidUpdateDto> {
    return this.call('engine_solid_recompute');
  }

  async setRollback(rollbackIndex: number): Promise<SolidUpdateDto> {
    return this.call('engine_solid_set_rollback', { rollback_index: rollbackIndex });
  }

  async deleteFeature(featureId: number): Promise<SolidUpdateDto> {
    return this.call('engine_solid_delete_feature', { feature_id: featureId });
  }

  async reorderFeature(featureId: number, targetIndex: number): Promise<SolidUpdateDto> {
    return this.call('engine_solid_reorder_feature', {
      feature_id: featureId,
      target_index: targetIndex,
    });
  }

  async setDocumentName(name: string, expectedModelJson?: string): Promise<DocumentDto> {
    return this.call('engine_document_set_name', expectedModelJson === undefined ? name : { name, expected_model_json: expectedModelJson });
  }

  async exportProjectModel(options?: { expected_model_json: string; save_name?: string }): Promise<string> {
    return this.call('engine_project_export_model', options);
  }

  private async projectSessionCall<T>(
    command: string,
    sessionId: string,
  ): Promise<T> {
    const json = await invoke<string>(command, { sessionId });
    return unwrapEnvelope(json);
  }

  async bindProjectSession(sessionId: string): Promise<void> {
    return this.projectSessionCall('engine_project_session_bind', sessionId);
  }

  async createProjectSession(sessionId: string): Promise<SolidUpdateDto> {
    return this.projectSessionCall('engine_project_session_create', sessionId);
  }

  async activateProjectSession(sessionId: string): Promise<boolean> {
    return this.projectSessionCall('engine_project_session_activate', sessionId);
  }

  async dropProjectSession(sessionId: string): Promise<void> {
    await this.projectSessionCall('engine_project_session_drop', sessionId);
  }

  async newProject(): Promise<SolidUpdateDto> {
    return this.call('engine_project_new');
  }

  async loadProjectModel(modelJson: string): Promise<SolidUpdateDto> {
    let update: SolidUpdateDto;
    try {
      update = await this.call<SolidUpdateDto>('engine_project_load', modelJson);
    } catch (error) {
      const data = error instanceof EngineError ? error.data : null;
      const unchanged = data !== null && typeof data === 'object'
        && 'project_load_state' in data && data.project_load_state === 'unchanged';
      throw new ProjectLoadError(unchanged ? 'unchanged' : 'unverified', error);
    }
    try {
      return await restoreLoadedDatumHistoryFrames(this, update);
    } catch (error) {
      // The new document is already committed. Repair may itself have changed
      // the rollback stage; never classify this as an atomic load rejection.
      throw new ProjectLoadError('unverified', error);
    }
  }

  async exportStep(request: StepExportRequest): Promise<Uint8Array> {
    const bytes = await invoke<number[]>('engine_export_step', {
      payload: JSON.stringify(request),
    });
    return Uint8Array.from(bytes);
  }

  async exportStl(request: MeshExportRequest): Promise<Uint8Array> {
    const bytes = await invoke<number[]>('engine_export_stl', {
      payload: JSON.stringify(request),
    });
    return Uint8Array.from(bytes);
  }

  async export3mf(request: MeshExportRequest): Promise<Uint8Array> {
    const bytes = await invoke<number[]>('engine_export_3mf', {
      payload: JSON.stringify(request),
    });
    return Uint8Array.from(bytes);
  }

  async previewSegment(request: SegmentRequest): Promise<PreviewDto> {
    return this.call('engine_preview_segment', request);
  }

  async addLine(request: SegmentRequest): Promise<AddLineResult> {
    return this.call('engine_add_line', request);
  }

  async previewSegmentLocked(request: LockedSegmentRequest): Promise<PreviewDto> {
    return this.call('engine_preview_segment_locked', request);
  }

  async addLineLocked(request: LockedSegmentRequest): Promise<AddLineResult> {
    return this.call('engine_add_line_locked', request);
  }

  async addPoint(request: PointRequest): Promise<ToolResult> {
    return this.call('engine_add_point', request);
  }

  async addLineMidpoint(request: MidpointLineRequest): Promise<ToolResult> {
    return this.call('engine_add_line_midpoint', request);
  }

  async addRectangle(request: RectangleRequest): Promise<ToolResult> {
    return this.call('engine_add_rectangle', request);
  }

  async addRectangleLocked(request: LockedRectangleRequest): Promise<ToolResult> {
    return this.call('engine_add_rectangle_locked', request);
  }

  async addCircle(request: CircleRequest): Promise<ToolResult> {
    return this.call('engine_add_circle', request);
  }

  async addCircleLocked(request: LockedCircleRequest): Promise<ToolResult> {
    return this.call('engine_add_circle_locked', request);
  }

  async addSlot(request: SlotRequest): Promise<ToolResult> {
    return this.call('engine_add_slot', request);
  }

  async addSpline(request: SplineRequest): Promise<ToolResult> {
    return this.call('engine_add_spline', request);
  }

  async addArc3pt(request: Arc3PointRequest): Promise<ToolResult> {
    return this.call('engine_add_arc_3pt', request);
  }

  async addArcCenter(request: ArcCenterRequest): Promise<ToolResult> {
    return this.call('engine_add_arc_center', request);
  }

  async addConstraint(constraint: ConstraintPayload): Promise<AddConstraintResult> {
    return this.call('engine_add_constraint', constraint);
  }

  async addConstraints(constraints: ConstraintPayload[]): Promise<ToolResult> {
    return this.call('engine_add_constraints', { constraints });
  }

  async addDimension(request: DimensionRequest): Promise<ToolResult> {
    return this.call('engine_add_dimension', request);
  }

  async editDimension(request: EditDimensionRequest): Promise<AddConstraintResult> {
    return this.call('engine_edit_dimension', request);
  }

  async setDimensionMode(request: SetDimensionModeRequest): Promise<AddConstraintResult> {
    return this.call('engine_set_dimension_mode', request);
  }

  async moveDimension(request: MoveDimensionRequest): Promise<AddConstraintResult> {
    return this.call('engine_move_dimension', request);
  }

  async deleteDimension(constraintId: number): Promise<AddConstraintResult> {
    return this.call('engine_delete_dimension', { constraint_id: constraintId });
  }

  async deleteConstraint(constraintId: number): Promise<AddConstraintResult> {
    return this.call('engine_delete_constraint', { constraint_id: constraintId });
  }

  async setDimensionStyle(style: DimensionStyle): Promise<SketchDto> {
    return this.call('engine_set_dimension_style', { style });
  }

  async evalExpression(text: string): Promise<EvalExpressionResult> {
    return this.call('engine_eval_expression', { text });
  }


  async filletPreview(request: FilletRequest): Promise<FilletPreviewDto> {
    return this.call('engine_fillet_preview', request);
  }
  async filletLines(request: FilletRequest): Promise<ToolResult> {
    return this.call('engine_fillet_lines', request);
  }
  async chamferLines(request: ChamferRequest): Promise<ToolResult> {
    return this.call('engine_chamfer_lines', request);
  }
  async offsetPreview(request: OffsetRequest): Promise<OffsetPreviewDto> {
    return this.call('engine_offset_preview', request);
  }
  async offsetCurve(request: OffsetRequest): Promise<ToolResult> {
    return this.call('engine_offset_curve', request);
  }
  async trimPreview(request: TrimRequest): Promise<TrimPreviewDto> {
    return this.call('engine_trim_preview', request);
  }
  async trimEntity(request: TrimRequest): Promise<ToolResult> {
    return this.call('engine_trim_entity', request);
  }
  async extendEntity(request: ExtendRequest): Promise<ToolResult> {
    return this.call('engine_extend_entity', request);
  }
  async breakCurve(request: BreakRequest): Promise<ToolResult> {
    return this.call('engine_break_curve', request);
  }
  async mirrorEntities(request: MirrorRequest): Promise<ToolResult> {
    return this.call('engine_mirror_entities', request);
  }
  async rectangularPattern(request: SketchRectangularPatternRequest): Promise<ToolResult> {
    return this.call('engine_rectangular_pattern', request);
  }
  async circularPattern(request: SketchCircularPatternRequest): Promise<ToolResult> {
    return this.call('engine_circular_pattern', request);
  }
  async moveCopyEntities(request: MoveCopyRequest): Promise<ToolResult> {
    return this.call('engine_move_copy_entities', request);
  }
  async scaleEntities(request: ScaleRequest): Promise<ToolResult> {
    return this.call('engine_scale_entities', request);
  }
  async polygonCreate(request: PolygonRequest): Promise<ToolResult> {
    return this.call('engine_polygon_create', request);
  }

  async toggleFix(entityId: number): Promise<AddConstraintResult> {
    return this.call('engine_toggle_fix', { entity_id: entityId });
  }

  async toggleFixEntities(entityIds: number[]): Promise<ToolResult> {
    return this.call('engine_toggle_fix_entities', { entity_ids: entityIds });
  }

  async movePoint(request: MovePointRequest): Promise<MovePointResult> {
    return this.call('engine_move_point', request);
  }

  async deleteEntity(entityId: number): Promise<DeleteEntityResult> {
    return this.call('engine_delete_entity', { entity_id: entityId });
  }

  async deleteEntities(entityIds: number[]): Promise<DeleteEntityResult> {
    return this.call('engine_delete_entities', { entity_ids: entityIds });
  }

  async undo(): Promise<UndoResult> {
    return this.call('engine_undo');
  }

  async redo(): Promise<UndoResult> {
    return this.call('engine_redo');
  }

  async setGridSnap(enabled: boolean): Promise<SketchDto> {
    return this.call('engine_set_grid_snap', { enabled });
  }

  async setGridStep(stepMm: number): Promise<void> {
    return this.call('engine_set_grid_step', { step_mm: stepMm });
  }
}

// `EngineError` re-exported for consumers that only import the adapter.
export { EngineError };
