/* tslint:disable */
/* eslint-disable */

/**
 * Engine instance held by the frontend `WasmEngine` adapter.
 */
export class WasmEngine {
    free(): void;
    [Symbol.dispose](): void;
    active_sketch(): string;
    /**
     * `payload`: serialized `Arc3PointRequest`.
     */
    add_arc_3pt(payload: string): string;
    /**
     * `payload`: serialized `ArcCenterRequest`.
     */
    add_arc_center(payload: string): string;
    /**
     * `payload`: serialized `CircleRequest`.
     */
    add_circle(payload: string): string;
    /**
     * `payload`: serialized `LockedCircleRequest`.
     */
    add_circle_locked(payload: string): string;
    /**
     * `payload`: serialized `Constraint`.
     */
    add_constraint(payload: string): string;
    add_constraints(payload: string): string;
    /**
     * `payload`: serialized `DimensionRequest`.
     */
    add_dimension(payload: string): string;
    /**
     * `payload`: serialized `SegmentRequest`.
     */
    add_line(payload: string): string;
    /**
     * `payload`: serialized `LockedSegmentRequest`.
     */
    add_line_locked(payload: string): string;
    /**
     * `payload`: serialized `MidpointLineRequest`.
     */
    add_line_midpoint(payload: string): string;
    /**
     * `payload`: serialized `PointRequest`.
     */
    add_point(payload: string): string;
    /**
     * `payload`: serialized `RectangleRequest`.
     */
    add_rectangle(payload: string): string;
    /**
     * `payload`: serialized `LockedRectangleRequest`.
     */
    add_rectangle_locked(payload: string): string;
    /**
     * `payload`: serialized `SlotRequest`.
     */
    add_slot(payload: string): string;
    /**
     * `payload`: serialized `SplineRequest`.
     */
    add_spline(payload: string): string;
    assembly_apply_joint_motions(payload: string): string;
    assembly_apply_position(payload: string): string;
    assembly_create_component(payload: string): string;
    assembly_create_contact_set(payload: string): string;
    assembly_create_joint(payload: string): string;
    assembly_create_motion_study(payload: string): string;
    assembly_create_occurrence(payload: string): string;
    assembly_create_position(payload: string): string;
    assembly_delete_contact_set(payload: string): string;
    assembly_delete_joint(payload: string): string;
    assembly_delete_motion_study(payload: string): string;
    assembly_delete_position(payload: string): string;
    assembly_document(): string;
    assembly_duplicate_occurrence(payload: string): string;
    assembly_evaluate_motion_study(payload: string): string;
    assembly_export_motion_path_csv(payload: string): string;
    assembly_interference_check(payload: string): string;
    assembly_preview_joint(payload: string): string;
    assembly_preview_joint_coordinates(payload: string): string;
    assembly_preview_joint_motion(payload: string): string;
    assembly_preview_joint_update(payload: string): string;
    assembly_preview_mechanism_drag(payload: string): string;
    assembly_sample_motion_study(payload: string): string;
    assembly_set_document(payload: string): string;
    assembly_set_grounded_body(payload: string): string;
    assembly_set_joint_coordinates(payload: string): string;
    assembly_set_joint_enabled(payload: string): string;
    assembly_set_joint_motion(payload: string): string;
    assembly_set_occurrence_grounded(payload: string): string;
    assembly_set_occurrence_pose(payload: string): string;
    assembly_solution(): string;
    assembly_swept_collision_check(payload: string): string;
    assembly_update_component(payload: string): string;
    assembly_update_contact_set(payload: string): string;
    assembly_update_joint(payload: string): string;
    assembly_update_motion_study(payload: string): string;
    assembly_update_occurrence(payload: string): string;
    assembly_update_position(payload: string): string;
    /**
     * `payload`: serialized `PlaneRef`.
     */
    begin_sketch(payload: string): string;
    body_appearances(): string;
    body_feature_definitions(): string;
    /**
     * `payload`: serialized `BreakRequest`.
     */
    break_curve(payload: string): string;
    chamfer_definitions(): string;
    /**
     * `payload`: serialized `ChamferRequest`.
     */
    chamfer_lines(payload: string): string;
    /**
     * `payload`: serialized `CircularPatternRequest`.
     */
    circular_pattern(payload: string): string;
    datum_plane_create(payload: string): string;
    datum_plane_definitions(): string;
    datum_plane_edit(payload: string): string;
    /**
     * `payload`: serialized `DeleteConstraintRequest`.
     */
    delete_constraint(payload: string): string;
    /**
     * `payload`: serialized `DeleteDimensionRequest`.
     */
    delete_dimension(payload: string): string;
    /**
     * `payload`: serialized `DeleteEntitiesRequest`.
     */
    delete_entities(payload: string): string;
    /**
     * `payload`: serialized `DeleteEntityRequest`.
     */
    delete_entity(payload: string): string;
    /**
     * Document snapshot (name, settings, browser tree incl. sketches).
     */
    document(): string;
    document_set_name(payload: string): string;
    drawing_apply(payload: string): string;
    drawing_document(): string;
    drawing_set_document(payload: string): string;
    /**
     * `payload`: serialized `EditDimensionRequest`.
     */
    edit_dimension(payload: string): string;
    /**
     * `payload`: sketch name (JSON string) to re-enter for editing (M1d).
     */
    edit_sketch(payload: string): string;
    end_sketch(): string;
    /**
     * `payload`: serialized `EvalExpressionRequest`.
     */
    eval_expression(payload: string): string;
    /**
     * `payload`: serialized `ExtendRequest`.
     */
    extend_entity(payload: string): string;
    extrude_definitions(): string;
    fillet_definitions(): string;
    /**
     * `payload`: serialized `FilletRequest`.
     */
    fillet_lines(payload: string): string;
    /**
     * `payload`: serialized `FilletRequest`.
     */
    fillet_preview(payload: string): string;
    /**
     * Finished-sketch snapshots (M1d): muted 3D rendering + re-edit list.
     */
    finished_sketches(): string;
    hole_definitions(): string;
    loft_definitions(): string;
    /**
     * `payload`: serialized `MirrorRequest`.
     */
    mirror_entities(payload: string): string;
    /**
     * `payload`: serialized `MoveCopyRequest`.
     */
    move_copy_entities(payload: string): string;
    /**
     * `payload`: serialized `MoveDimensionRequest`.
     */
    move_dimension(payload: string): string;
    /**
     * `payload`: serialized `MovePointRequest`.
     */
    move_point(payload: string): string;
    constructor();
    /**
     * `payload`: serialized `OffsetRequest`.
     */
    offset_curve(payload: string): string;
    /**
     * `payload`: serialized `OffsetRequest`.
     */
    offset_preview(payload: string): string;
    /**
     * `payload`: serialized `PolygonRequest`.
     */
    polygon_create(payload: string): string;
    /**
     * `payload`: serialized `SegmentRequest`.
     */
    preview_segment(payload: string): string;
    /**
     * `payload`: serialized `LockedSegmentRequest`.
     */
    preview_segment_locked(payload: string): string;
    profile_catalog(): string;
    project_export_model(payload?: string | null): string;
    project_prepare_load(payload: string): string;
    project_prepare_new(): string;
    project_set_visibility(payload: string): string;
    project_visibility(): string;
    /**
     * `payload`: serialized `RectangularPatternRequest`.
     */
    rectangular_pattern(payload: string): string;
    redo(): string;
    revolve_definitions(): string;
    rib_definitions(): string;
    /**
     * `payload`: serialized `ScaleRequest`.
     */
    scale_entities(payload: string): string;
    set_body_appearance(payload: string): string;
    /**
     * `payload`: serialized `SetDimensionModeRequest`.
     */
    set_dimension_mode(payload: string): string;
    /**
     * `payload`: serialized `SetDimensionStyleRequest`.
     */
    set_dimension_style(payload: string): string;
    /**
     * `payload`: serialized `SetGridSnapRequest`.
     */
    set_grid_snap(payload: string): string;
    /**
     * `payload`: serialized `SetGridStepRequest`.
     */
    set_grid_step(payload: string): string;
    solid_commit(payload: string): string;
    solid_prepare_body_feature(payload: string): string;
    solid_prepare_chamfer(payload: string): string;
    solid_prepare_delete_feature(payload: string): string;
    solid_prepare_edit_body_feature(payload: string): string;
    solid_prepare_edit_chamfer(payload: string): string;
    solid_prepare_edit_extrude(payload: string): string;
    solid_prepare_edit_fillet(payload: string): string;
    solid_prepare_edit_hole(payload: string): string;
    solid_prepare_edit_loft(payload: string): string;
    solid_prepare_edit_revolve(payload: string): string;
    solid_prepare_edit_rib(payload: string): string;
    solid_prepare_edit_sweep(payload: string): string;
    solid_prepare_extrude(payload: string): string;
    solid_prepare_fillet(payload: string): string;
    solid_prepare_hole(payload: string): string;
    solid_prepare_loft(payload: string): string;
    solid_prepare_recompute(): string;
    solid_prepare_reorder_feature(payload: string): string;
    solid_prepare_revolve(payload: string): string;
    solid_prepare_rib(payload: string): string;
    solid_prepare_set_rollback(payload: string): string;
    solid_prepare_sweep(payload: string): string;
    solid_scene(): string;
    sweep_definitions(): string;
    /**
     * `payload`: serialized `DeleteEntityRequest`.
     */
    toggle_fix(payload: string): string;
    toggle_fix_entities(payload: string): string;
    /**
     * `payload`: serialized `TrimRequest`.
     */
    trim_entity(payload: string): string;
    /**
     * `payload`: serialized `TrimRequest`.
     */
    trim_preview(payload: string): string;
    undo(): string;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_wasmengine_free: (a: number, b: number) => void;
    readonly wasmengine_active_sketch: (a: number) => [number, number];
    readonly wasmengine_add_arc_3pt: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_add_arc_center: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_add_circle: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_add_circle_locked: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_add_constraint: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_add_constraints: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_add_dimension: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_add_line: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_add_line_locked: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_add_line_midpoint: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_add_point: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_add_rectangle: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_add_rectangle_locked: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_add_slot: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_add_spline: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_apply_joint_motions: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_apply_position: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_create_component: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_create_contact_set: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_create_joint: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_create_motion_study: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_create_occurrence: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_create_position: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_delete_contact_set: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_delete_joint: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_delete_motion_study: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_delete_position: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_document: (a: number) => [number, number];
    readonly wasmengine_assembly_duplicate_occurrence: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_evaluate_motion_study: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_export_motion_path_csv: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_interference_check: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_preview_joint: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_preview_joint_coordinates: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_preview_joint_motion: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_preview_joint_update: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_preview_mechanism_drag: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_sample_motion_study: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_set_document: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_set_grounded_body: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_set_joint_coordinates: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_set_joint_enabled: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_set_joint_motion: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_set_occurrence_grounded: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_set_occurrence_pose: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_solution: (a: number) => [number, number];
    readonly wasmengine_assembly_swept_collision_check: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_update_component: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_update_contact_set: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_update_joint: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_update_motion_study: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_update_occurrence: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_assembly_update_position: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_begin_sketch: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_body_appearances: (a: number) => [number, number];
    readonly wasmengine_body_feature_definitions: (a: number) => [number, number];
    readonly wasmengine_break_curve: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_chamfer_definitions: (a: number) => [number, number];
    readonly wasmengine_chamfer_lines: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_circular_pattern: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_datum_plane_create: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_datum_plane_definitions: (a: number) => [number, number];
    readonly wasmengine_datum_plane_edit: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_delete_constraint: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_delete_dimension: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_delete_entities: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_delete_entity: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_document: (a: number) => [number, number];
    readonly wasmengine_document_set_name: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_drawing_apply: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_drawing_document: (a: number) => [number, number];
    readonly wasmengine_drawing_set_document: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_edit_dimension: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_edit_sketch: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_end_sketch: (a: number) => [number, number];
    readonly wasmengine_eval_expression: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_extend_entity: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_extrude_definitions: (a: number) => [number, number];
    readonly wasmengine_fillet_definitions: (a: number) => [number, number];
    readonly wasmengine_fillet_lines: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_fillet_preview: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_finished_sketches: (a: number) => [number, number];
    readonly wasmengine_hole_definitions: (a: number) => [number, number];
    readonly wasmengine_loft_definitions: (a: number) => [number, number];
    readonly wasmengine_mirror_entities: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_move_copy_entities: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_move_dimension: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_move_point: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_new: () => number;
    readonly wasmengine_offset_curve: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_offset_preview: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_polygon_create: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_preview_segment: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_preview_segment_locked: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_profile_catalog: (a: number) => [number, number];
    readonly wasmengine_project_export_model: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_project_prepare_load: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_project_prepare_new: (a: number) => [number, number];
    readonly wasmengine_project_set_visibility: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_project_visibility: (a: number) => [number, number];
    readonly wasmengine_rectangular_pattern: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_redo: (a: number) => [number, number];
    readonly wasmengine_revolve_definitions: (a: number) => [number, number];
    readonly wasmengine_rib_definitions: (a: number) => [number, number];
    readonly wasmengine_scale_entities: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_set_body_appearance: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_set_dimension_mode: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_set_dimension_style: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_set_grid_snap: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_set_grid_step: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_commit: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_prepare_body_feature: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_prepare_chamfer: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_prepare_delete_feature: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_prepare_edit_body_feature: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_prepare_edit_chamfer: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_prepare_edit_extrude: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_prepare_edit_fillet: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_prepare_edit_hole: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_prepare_edit_loft: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_prepare_edit_revolve: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_prepare_edit_rib: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_prepare_edit_sweep: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_prepare_extrude: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_prepare_fillet: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_prepare_hole: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_prepare_loft: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_prepare_recompute: (a: number) => [number, number];
    readonly wasmengine_solid_prepare_reorder_feature: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_prepare_revolve: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_prepare_rib: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_prepare_set_rollback: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_prepare_sweep: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_solid_scene: (a: number) => [number, number];
    readonly wasmengine_sweep_definitions: (a: number) => [number, number];
    readonly wasmengine_toggle_fix: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_toggle_fix_entities: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_trim_entity: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_trim_preview: (a: number, b: number, c: number) => [number, number];
    readonly wasmengine_undo: (a: number) => [number, number];
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
