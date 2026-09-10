import type {
  DrawingAnnotationDto,
  DrawingAttachmentRefDto,
  DrawingBomItemDto,
  DrawingChainDimensionLayout,
  DrawingCircularRefDto,
  DrawingDimensionPresentationDto,
  DrawingDocumentDto,
  DrawingLineDimensionMode,
  DrawingLinearDimensionMode,
  DrawingLineRefDto,
  DrawingOrdinateAxis,
  DrawingProjectionMethod,
  DrawingRadialDimensionMode,
  DrawingSheetDto,
  DrawingSheetFormat,
  DrawingSheetOrientation,
  DrawingStandard,
  DrawingTemplateDto,
  DrawingRevisionDto,
  DrawingGdtCharacteristic,
  DrawingMaterialCondition,
  DrawingSurfaceLay,
  DrawingWeldContour,
  DrawingWeldSide,
  DrawingWeldType,
  DrawingToleranceNoteDto,
  DrawingTopologyAnchorRefDto,
  DrawingViewDto,
  DrawingViewDerivationDto,
  DrawingViewKind,
  HoleDefinitionDto,
  SolidSceneDto,
} from '../engine/types';
import { getEngine } from '../engine';
import {
  commitDrawingRedoHistory,
  commitDrawingUndoHistory,
  currentHistoryProjectKey,
  peekDrawingRedoHistory,
  peekDrawingUndoHistory,
  recordDrawingHistory,
} from '../engine/applicationHistory';
import { useAppStore } from '../store/appStore';
import { defaultDrawingFormat, defaultDrawingSheetStyle, drawingSheetSize } from './sheet';

let writeQueue: Promise<void> = Promise.resolve();

export function defaultDrawingDimensionPresentation(): DrawingDimensionPresentationDto {
  return {
    tolerance: { mode: 'none', upper: 0, lower: 0 },
    basic: false,
    reference: false,
    dual_units: null,
    fit_class: '',
  };
}

export interface DrawingSheetSetup {
  standard: DrawingStandard;
  format: DrawingSheetFormat;
  orientation: DrawingSheetOrientation;
  projection_method: DrawingProjectionMethod;
  tolerance_note: DrawingToleranceNoteDto;
  title: string;
  drawing_number: string;
  revision: string;
  author: string;
}

export function defaultDrawingSheetSetup(
  standard: DrawingStandard = 'iso',
  documentName = useAppStore.getState().document?.name ?? 'Untitled',
): DrawingSheetSetup {
  return {
    standard,
    format: defaultDrawingFormat(standard),
    orientation: 'landscape',
    projection_method: standard === 'ansi' ? 'third_angle' : 'first_angle',
    tolerance_note: {
      preset: standard === 'ansi' ? 'ansi_decimal' : 'iso2768_medium',
      custom: '',
    },
    title: documentName,
    drawing_number: '',
    revision: 'A',
    author: '',
  };
}

/** Drawing is a peer workspace, not a command inside solid modeling. */
export async function enterDrawingWorkspace(): Promise<void> {
  const state = useAppStore.getState();
  if (state.mode !== 'solid') {
    throw new Error('Finish the active sketch before opening Drawings.');
  }
  state.setSelectedDrawingViewId(null);
  state.setSelectedDrawingAnnotationId(null);
  state.setDrawingTool(null);
  state.setDrawingPendingViewKind(null);
  state.setActiveTab('drawing');
  state.setDrawingSheetSetupOpen(state.drawingDocument.sheets.length === 0);
}

export function leaveDrawingWorkspace(): void {
  const state = useAppStore.getState();
  state.setSelectedDrawingViewId(null);
  state.setSelectedDrawingAnnotationId(null);
  state.setDrawingTool(null);
  state.setDrawingPendingViewKind(null);
  state.setDrawingSheetSetupOpen(false);
  state.setActiveTab('solid');
}

export function beginDrawingSheetSetup(): void {
  const state = useAppStore.getState();
  state.setSelectedDrawingViewId(null);
  state.setSelectedDrawingAnnotationId(null);
  state.setDrawingTool(null);
  state.setDrawingPendingViewKind(null);
  state.setDrawingSheetSetupOpen(true);
}

/** Creates a framed but intentionally empty sheet. */
export function createDrawingSheet(setup: DrawingSheetSetup): Promise<void> {
  return enqueueDrawingCommand((drawing) => ({
    type: 'create_sheet', arguments: {
      name: `Sheet ${drawing.sheets.length + 1}`, format: setup.format, orientation: setup.orientation,
      standard: setup.standard, projection_method: setup.projection_method, tolerance_note: setup.tolerance_note,
      title_block: { title: setup.title, drawing_number: setup.drawing_number, revision: setup.revision, author: setup.author },
    },
  })).then(() => { clearDrawingSelection(); useAppStore.getState().setDrawingSheetSetupOpen(false); });
}

export function setActiveDrawingSheet(sheetId: number): Promise<void> {
  return enqueueDrawingCommand(() => ({ type: 'select_sheet', arguments: { sheet_id: sheetId } }), false).then(clearDrawingSelection);
}
export function deleteDrawingSheet(sheetId: number): Promise<void> {
  return enqueueDrawingCommand(() => ({ type: 'delete_sheet', arguments: { sheet_id: sheetId } })).then(() => {
    clearDrawingSelection(); if (!useAppStore.getState().drawingDocument.sheets.length) useAppStore.getState().setDrawingSheetSetupOpen(true);
  });
}

/** Add the conventional front/top/right/isometric group to an empty sheet. */
export function autoLayoutDrawingViews(): Promise<void> {
  return enqueueDrawingUpdate((drawing, state) => {
    const next = cloneDrawing(drawing);
    const sheet = activeSheet(next);
    if (!sheet) throw new Error('Create a drawing sheet first.');
    if (sheet.views.length > 0) {
      throw new Error('Automatic layout is available on an empty sheet. Delete existing views or place additional views manually.');
    }
    const [width, height] = drawingSheetSize(sheet.format, sheet.orientation);
    const scale = suggestedViewScale(state.solidScene, width, height);
    const frontId = next.next_view_id++;
    const frontPosition: [number, number] = [width * 0.39, height * 0.47];
    const verticalOffset = Math.min(height * 0.28, 70);
    const horizontalOffset = Math.min(width * 0.24, 90);
    const topAbove = sheet.projection_method === 'third_angle';
    const rightOnRight = sheet.projection_method === 'third_angle';
    sheet.views.push(
      makeView(frontId, 'front', frontPosition, scale, null, 'free'),
      makeView(
        next.next_view_id++,
        'top',
        [frontPosition[0], frontPosition[1] + (topAbove ? -verticalOffset : verticalOffset)],
        scale,
        frontId,
        'vertical',
      ),
      makeView(
        next.next_view_id++,
        'right',
        [frontPosition[0] + (rightOnRight ? horizontalOffset : -horizontalOffset), frontPosition[1]],
        scale,
        frontId,
        'horizontal',
      ),
      makeView(
        next.next_view_id++,
        'isometric',
        [width * 0.74, height * 0.31],
        scale,
        frontId,
        'free',
      ),
    );
    queueMicrotask(clearDrawingSelection);
    return next;
  });
}

/** Place a view at a user-selected paper point and preserve projected alignment. */
export function addDrawingView(
  kind: DrawingViewKind,
  position: [number, number],
  requestedParentId: number | null = null,
  requestedScale?: number,
): Promise<void> {
  return enqueueDrawingCommand((drawing, state) => {
    const sheet = activeSheet(drawing); if (!sheet) throw new Error('Create a drawing sheet first.');
    const view = drawingViewPlacementDraft(sheet, state.solidScene, kind, position, requestedParentId, requestedScale, drawing.next_view_id);
    return { type: 'add_view', arguments: { sheet_id: sheet.id, view, rescale_group: requestedScale !== undefined } };
  }).then(() => {
    const store = useAppStore.getState(); store.setDrawingTool(null); store.setDrawingPendingViewKind(null);
    store.setSelectedDrawingAnnotationId(null); store.setSelectedDrawingViewId(store.drawingDocument.next_view_id - 1);
  });
}

export function updateDrawingView(viewId: number, update: Partial<DrawingViewDto>): Promise<void> {
  return enqueueDrawingUpdate((drawing) => {
    const next = cloneDrawing(drawing);
    const sheet = next.sheets.find((candidate) => candidate.views.some((view) => view.id === viewId));
    const view = sheet?.views.find((candidate) => candidate.id === viewId);
    if (!sheet || !view) return drawing;
    const oldPosition: [number, number] = [...view.position];
    Object.assign(view, update);

    if (update.scale !== undefined) {
      const root = drawingViewGroupRoot(sheet, view.id);
      if (root) {
        for (const member of sheet.views) {
          if (drawingViewGroupRoot(sheet, member.id)?.id === root.id) {
            member.scale = update.scale;
          }
        }
      }
    }

    const parent = view.parent_view_id == null
      ? null
      : sheet.views.find((candidate) => candidate.id === view.parent_view_id) ?? null;
    if (parent && update.position) {
      if (view.alignment === 'horizontal') view.position[1] = parent.position[1];
      if (view.alignment === 'vertical') view.position[0] = parent.position[0];
    }

    if (update.position) {
      const delta: [number, number] = [view.position[0] - oldPosition[0], view.position[1] - oldPosition[1]];
      for (const child of sheet.views.filter((candidate) => candidate.parent_view_id === viewId)) {
        if (child.alignment === 'horizontal') child.position[1] += delta[1];
        if (child.alignment === 'vertical') child.position[0] += delta[0];
      }
    }
    return next;
  });
}

export function deleteDrawingView(viewId: number): Promise<void> {
  return enqueueDrawingUpdate((drawing) => {
    const next = cloneDrawing(drawing);
    for (const sheet of next.sheets) {
      sheet.views = sheet.views.filter((view) => view.id !== viewId);
      for (const child of sheet.views) {
        if (child.parent_view_id === viewId) {
          child.parent_view_id = null;
          child.alignment = 'free';
        }
      }
      sheet.annotations = sheet.annotations.filter((annotation) =>
        !('view_id' in annotation) || annotation.view_id !== viewId,
      );
    }
    queueMicrotask(clearDrawingSelection);
    return next;
  });
}

export function addDrawingLinearDimension(
  viewId: number,
  first: DrawingTopologyAnchorRefDto,
  second: DrawingTopologyAnchorRefDto,
  mode: DrawingLinearDimensionMode = 'aligned',
  offset = 12,
): Promise<void> {
  let createdId: number;
  return enqueueDrawingCommand(drawing => {
    const sheet = drawing.sheets.find(s => s.views.some(v => v.id === viewId));
    if (!sheet) throw new Error('Drawing dimension references a missing view.');
    createdId = drawing.next_annotation_id;
    return { type: 'add_linear_dimension', arguments: {
      sheet_id: sheet.id, view_id: viewId, first, second, mode, offset,
      prefix: '', suffix: '', precision: 2, presentation: defaultDrawingDimensionPresentation(),
    } };
  }).then(() => selectCreatedAnnotation(createdId));
}

export function addDrawingLineDimension(
  viewId: number,
  first: DrawingLineRefDto,
  second: DrawingLineRefDto | null,
  mode: DrawingLineDimensionMode,
  position: [number, number],
): Promise<void> {
  if (mode === 'length' && second !== null) {
    return Promise.reject(new Error('A line-length dimension accepts one straight edge.'));
  }
  if (mode !== 'length' && second === null) {
    return Promise.reject(new Error('Line distance and angle dimensions require two straight edges.'));
  }
  return addViewAnnotation(viewId, (id) => ({
    kind: 'line_dimension', id, view_id: viewId, first, second, mode, position,
    prefix: '', suffix: '', precision: mode === 'angle' ? 1 : 2,
    presentation: defaultDrawingDimensionPresentation(),
  }));
}

export function addDrawingPointLineDimension(
  viewId: number,
  point: DrawingTopologyAnchorRefDto,
  line: DrawingLineRefDto,
  position: [number, number],
): Promise<void> {
  return addViewAnnotation(viewId, (id) => ({
    kind: 'point_line_dimension', id, view_id: viewId, point, line, position,
    prefix: '', suffix: '', precision: 2,
    presentation: defaultDrawingDimensionPresentation(),
  }));
}

export function addDrawingRadialDimension(
  viewId: number,
  feature: DrawingCircularRefDto,
  mode: DrawingRadialDimensionMode,
): Promise<void> {
  if (mode === 'diameter' && !feature.closed) {
    return Promise.reject(new Error('Diameter dimensions require a complete circle. Use Radius for an arc.'));
  }
  return addViewAnnotation(viewId, (id) => ({
    kind: 'radial_dimension', id, view_id: viewId, feature, mode,
    leader_angle_deg: -35, offset: 14, prefix: '', suffix: '', precision: 2,
    presentation: defaultDrawingDimensionPresentation(),
  }));
}

export function addDrawingAngularDimension(
  viewId: number,
  vertex: DrawingTopologyAnchorRefDto,
  first: DrawingTopologyAnchorRefDto,
  second: DrawingTopologyAnchorRefDto,
): Promise<void> {
  return addViewAnnotation(viewId, (id) => ({
    kind: 'angular_dimension', id, view_id: viewId, vertex, first, second,
    radius: 12, prefix: '', suffix: '', precision: 1,
    presentation: defaultDrawingDimensionPresentation(),
  }));
}

export async function addDrawingHoleNote(
  viewId: number,
  feature: DrawingCircularRefDto,
  position: [number, number],
): Promise<void> {
  if (!feature.closed) throw new Error('Hole notes require a complete circular edge.');
  const definitions = await (await getEngine()).holeDefinitions().catch(() => []);
  const definition = bestHoleDefinitionForCircle(definitions, feature);
  const positions = definition
    ? (definition.positions.length > 0 ? definition.positions.map((entry) => entry.position) : [definition.position])
    : [];
  const quantity = Math.max(1, positions.length);
  const depth = definition?.extent.type === 'distance' ? definition.extent.depth : null;
  const thread = definition?.thread
    ? `${definition.thread.designation}${definition.thread.class ? ` - ${definition.thread.class}` : ''}${definition.thread.hand === 'left' ? ' LH' : ''}`
    : '';
  return addViewAnnotation(viewId, (id) => ({
    kind: 'hole_note', id, view_id: viewId, feature, position,
    quantity,
    diameter: definition?.diameter ?? feature.fallback_radius * 2,
    depth,
    thread,
    note: definition?.extent.type === 'through_all' ? 'THRU' : '',
    source_feature_id: definition?.feature_id ?? null,
    feature_name: definition?.name ?? '',
    hole_style: definition?.style ?? 'simple',
    counterbore_diameter: definition?.style === 'counterbore' ? definition.counterbore_diameter : null,
    counterbore_depth: definition?.style === 'counterbore' ? definition.counterbore_depth : null,
    countersink_diameter: definition?.style === 'countersink' ? definition.countersink_diameter : null,
    countersink_angle_deg: definition?.style === 'countersink' ? definition.countersink_angle_deg : null,
    thread_depth: definition?.thread?.depth ?? null,
    pattern_note: quantity > 1 ? `${quantity} HOLES` : '',
  }));
}

export function addDrawingCenterMark(
  viewId: number,
  feature: DrawingCircularRefDto,
  extension = 2.5,
): Promise<void> {
  if (!feature.closed) return Promise.reject(new Error('Center marks require a complete circular edge.'));
  return addViewAnnotation(viewId, (id) => ({
    kind: 'center_mark', id, view_id: viewId, feature, extension,
  }));
}

export function addDrawingCenterLine(
  viewId: number,
  first: DrawingCircularRefDto,
  second: DrawingCircularRefDto,
  extension = 2.5,
): Promise<void> {
  if (!first.closed || !second.closed) {
    return Promise.reject(new Error('Centerlines require two complete circular edges.'));
  }
  if (
    (first.body_id === second.body_id && first.edge_id === second.edge_id)
    || distance3(first.fallback_center, second.fallback_center) < 1e-7
  ) {
    return Promise.reject(new Error('Select two distinct circular centers for a centerline.'));
  }
  return addViewAnnotation(viewId, (id) => ({
    kind: 'center_line', id, view_id: viewId, first, second, extension,
  }));
}

export function addDrawingCenterLineBetweenEdges(
  viewId: number,
  first: DrawingLineRefDto,
  second: DrawingLineRefDto,
  extension = 2.5,
): Promise<void> {
  if (first.body_id === second.body_id && first.edge_id === second.edge_id) {
    return Promise.reject(new Error('Select two distinct parallel edges for a centerline.'));
  }
  return addViewAnnotation(viewId, (id) => ({
    kind: 'center_line_between_edges', id, view_id: viewId, first, second, extension,
  }));
}

export function addDrawingAutomaticSymmetryAxis(
  viewId: number,
  axis: DrawingOrdinateAxis = 'both',
  extension = 2.5,
): Promise<void> {
  return addViewAnnotation(viewId, (id) => ({
    kind: 'automatic_symmetry_axis', id, view_id: viewId, axis, extension,
  }));
}

export function addDrawingBoltCircleCenterLine(
  viewId: number,
  features: DrawingCircularRefDto[],
  extension = 2.5,
): Promise<void> {
  if (features.length < 3) return Promise.reject(new Error('A bolt circle needs at least three circular centers.'));
  return addViewAnnotation(viewId, (id) => ({
    kind: 'bolt_circle_center_line', id, view_id: viewId,
    features: structuredClone(features), extension,
  }));
}

export function addDrawingChainDimension(
  viewId: number,
  anchors: DrawingTopologyAnchorRefDto[],
  layout: DrawingChainDimensionLayout = 'chain',
  mode: DrawingLinearDimensionMode = 'aligned',
): Promise<void> {
  if (anchors.length < 2) return Promise.reject(new Error('Select at least two points for a dimension series.'));
  return addViewAnnotation(viewId, (id) => ({
    kind: 'chain_dimension', id, view_id: viewId, anchors: structuredClone(anchors),
    mode, layout, offset: 12, spacing: 7, prefix: '', suffix: '', precision: 2,
    presentation: defaultDrawingDimensionPresentation(),
  }));
}

export function addDrawingOrdinateDimension(
  viewId: number,
  origin: DrawingTopologyAnchorRefDto,
  target: DrawingTopologyAnchorRefDto,
  axis: DrawingOrdinateAxis = 'both',
): Promise<void> {
  return addViewAnnotation(viewId, (id) => ({
    kind: 'ordinate_dimension', id, view_id: viewId, origin, target,
    axis, offset: 10, precision: 2, presentation: defaultDrawingDimensionPresentation(),
  }));
}

export function addDrawingArcLengthDimension(
  viewId: number,
  feature: DrawingCircularRefDto,
  first: DrawingTopologyAnchorRefDto,
  second: DrawingTopologyAnchorRefDto,
): Promise<void> {
  return addViewAnnotation(viewId, (id) => ({
    kind: 'arc_length_dimension', id, view_id: viewId, feature, first, second,
    offset: 7, precision: 2, presentation: defaultDrawingDimensionPresentation(),
  }));
}

export function addDrawingJoggedRadiusDimension(
  viewId: number,
  feature: DrawingCircularRefDto,
  position: [number, number],
): Promise<void> {
  return addViewAnnotation(viewId, (id) => ({
    kind: 'jogged_radius_dimension', id, view_id: viewId, feature,
    jog: [position[0] - 10, position[1]],
    position, precision: 2, presentation: defaultDrawingDimensionPresentation(),
  }));
}

export function addDrawingDatumFeature(
  viewId: number,
  attachment: DrawingAttachmentRefDto,
  position: [number, number],
  label = 'A',
): Promise<void> {
  return addViewAnnotation(viewId, (id) => ({
    kind: 'datum_feature', id, view_id: viewId, attachment, label, position, target_index: null,
  }));
}

export function addDrawingGdtFrame(
  viewId: number,
  attachment: DrawingAttachmentRefDto,
  position: [number, number],
): Promise<void> {
  return addViewAnnotation(viewId, (id) => ({
    kind: 'gdt_frame', id, view_id: viewId, attachment, position,
    characteristic: 'position', tolerance: 0.1, diameter_zone: true,
    material_condition: 'none', datums: [], projected_zone: null, free_state: false,
  }));
}

export function addDrawingSurfaceTexture(
  viewId: number,
  attachment: DrawingAttachmentRefDto,
  position: [number, number],
): Promise<void> {
  return addViewAnnotation(viewId, (id) => ({
    kind: 'surface_texture', id, view_id: viewId, attachment, position,
    roughness_ra: 3.2, process: '', lay: 'none', machining_allowance: null,
  }));
}

export function addDrawingEdgeRequirement(
  viewId: number,
  attachment: DrawingLineRefDto,
  position: [number, number],
): Promise<void> {
  return addViewAnnotation(viewId, (id) => ({
    kind: 'edge_requirement', id, view_id: viewId, attachment, position,
    upper_deviation: 0, lower_deviation: -0.2, note: '',
  }));
}

export function addDrawingWeldSymbol(
  viewId: number,
  attachment: DrawingLineRefDto,
  position: [number, number],
): Promise<void> {
  return addViewAnnotation(viewId, (id) => ({
    kind: 'weld_symbol', id, view_id: viewId, attachment, position,
    weld_type: 'fillet', side: 'arrow', size: 3, length: null, pitch: null,
    contour: 'none', finish: '', all_around: false, field_weld: false, tail: '',
  }));
}

export function addDrawingItemBalloon(
  viewId: number,
  attachment: DrawingAttachmentRefDto,
  position: [number, number],
  bomItemId: number,
): Promise<void> {
  return addViewAnnotation(viewId, (id) => ({
    kind: 'item_balloon', id, view_id: viewId, attachment, position, bom_item_id: bomItemId,
  }));
}

export function addDrawingRevisionCloud(
  points: Array<[number, number]>,
  revision: string,
): Promise<void> {
  if (points.length < 3) return Promise.reject(new Error('A revision cloud needs at least three points.'));
  return enqueueDrawingUpdate((drawing) => {
    const next = cloneDrawing(drawing);
    const sheet = activeSheet(next);
    if (!sheet) throw new Error('Create a drawing sheet first.');
    const id = next.next_annotation_id++;
    sheet.annotations.push({ kind: 'revision_cloud', id, revision, points: structuredClone(points) });
    queueMicrotask(() => selectCreatedAnnotation(id));
    return next;
  });
}

export function addDrawingDerivedView(
  kind: Extract<DrawingViewKind, 'section' | 'detail' | 'auxiliary' | 'broken' | 'removed_section'>,
  parentViewId: number,
  position: [number, number],
  derivation: DrawingViewDerivationDto,
): Promise<void> {
  return enqueueDrawingUpdate((drawing) => {
    const next = cloneDrawing(drawing);
    const sheet = activeSheet(next);
    const parent = sheet?.views.find((view) => view.id === parentViewId);
    if (!sheet || !parent) throw new Error('Select an existing parent view first.');
    const id = next.next_view_id++;
    const basis = derivedViewBasis(parent, derivation);
    sheet.views.push({
      ...structuredClone(parent), id, name: viewLabel(kind), kind, position,
      parent_view_id: parent.id, alignment: 'free', derivation: structuredClone(derivation),
      direction: basis.direction,
      up: basis.up,
    });
    queueMicrotask(() => {
      const store = useAppStore.getState();
      store.setSelectedDrawingViewId(id);
      store.setSelectedDrawingAnnotationId(null);
      store.setDrawingTool(null);
    });
    return next;
  });
}

function derivedViewBasis(
  parent: DrawingViewDto,
  derivation: DrawingViewDerivationDto,
): Pick<DrawingViewDto, 'direction' | 'up'> {
  if (derivation.type !== 'section' && derivation.type !== 'removed_section' && derivation.type !== 'auxiliary') {
    return { direction: [...parent.direction], up: [...parent.up] };
  }
  const reference = derivation.type === 'auxiliary'
    ? subtract3(derivation.reference.fallback_end, derivation.reference.fallback_start)
    : subtract3(derivation.second.fallback_point, derivation.first.fallback_point);
  const edge = normalize3(reference);
  const parentDirection = normalize3(parent.direction);
  if (!edge || !parentDirection) return { direction: [...parent.direction], up: [...parent.up] };
  let direction = normalize3(cross3(edge, parentDirection));
  if (!direction) return { direction: [...parent.direction], up: [...parent.up] };
  if (derivation.type === 'auxiliary' && derivation.flipped) direction = scale3(direction, -1);
  const up = normalize3(cross3(direction, edge)) ?? normalize3(parent.up) ?? [0, 0, 1];
  return { direction, up };
}

export function addDrawingChamferNote(
  viewId: number,
  first: DrawingTopologyAnchorRefDto,
  second: DrawingTopologyAnchorRefDto,
  position: [number, number],
  length = distance3(first.fallback_point, second.fallback_point),
  angleDeg = 45,
): Promise<void> {
  return addViewAnnotation(viewId, (id) => ({
    kind: 'chamfer_note', id, view_id: viewId, first, second, position,
    length, angle_deg: angleDeg, prefix: '',
  }));
}

export function addDrawingNote(position: [number, number], text = 'NOTE'): Promise<void> {
  return enqueueDrawingCommand(drawing => {
    const sheet = activeSheet(drawing); if (!sheet) throw new Error('Create a drawing sheet first.');
    return { type: 'add_note', arguments: { sheet_id: sheet.id, text, position } };
  }).then(() => selectCreatedAnnotation(useAppStore.getState().drawingDocument.next_annotation_id - 1));
}

export type DrawingAnnotationUpdate = Partial<{
  first: DrawingTopologyAnchorRefDto | DrawingCircularRefDto | DrawingLineRefDto;
  second: DrawingTopologyAnchorRefDto | DrawingCircularRefDto | DrawingLineRefDto;
  point: DrawingTopologyAnchorRefDto;
  line: DrawingLineRefDto;
  vertex: DrawingTopologyAnchorRefDto;
  origin: DrawingTopologyAnchorRefDto;
  target: DrawingTopologyAnchorRefDto;
  feature: DrawingCircularRefDto;
  anchors: DrawingTopologyAnchorRefDto[];
  features: DrawingCircularRefDto[];
  attachment: DrawingAttachmentRefDto | DrawingLineRefDto;
  text: string;
  position: [number, number];
  mode: DrawingLinearDimensionMode | DrawingLineDimensionMode | DrawingRadialDimensionMode;
  offset: number;
  leader_angle_deg: number;
  radius: number;
  prefix: string;
  suffix: string;
  precision: number;
  quantity: number;
  diameter: number;
  depth: number | null;
  thread: string;
  note: string;
  source_feature_id: number | null;
  feature_name: string;
  hole_style: 'simple' | 'counterbore' | 'countersink';
  counterbore_diameter: number | null;
  counterbore_depth: number | null;
  countersink_diameter: number | null;
  countersink_angle_deg: number | null;
  thread_depth: number | null;
  pattern_note: string;
  length: number | null;
  angle_deg: number;
  extension: number;
  presentation: DrawingDimensionPresentationDto;
  layout: DrawingChainDimensionLayout;
  spacing: number;
  axis: DrawingOrdinateAxis;
  jog: [number, number];
  label: string;
  target_index: number | null;
  characteristic: DrawingGdtCharacteristic;
  tolerance: number;
  diameter_zone: boolean;
  material_condition: DrawingMaterialCondition;
  datums: Array<{ label: string; material_condition: DrawingMaterialCondition }>;
  projected_zone: number | null;
  free_state: boolean;
  roughness_ra: number;
  process: string;
  lay: DrawingSurfaceLay;
  machining_allowance: number | null;
  upper_deviation: number;
  lower_deviation: number;
  weld_type: DrawingWeldType;
  side: DrawingWeldSide;
  size: number;
  pitch: number | null;
  contour: DrawingWeldContour;
  finish: string;
  all_around: boolean;
  field_weld: boolean;
  tail: string;
  bom_item_id: number;
  revision: string;
  points: Array<[number, number]>;
}>;

export function updateDrawingAnnotation(annotationId: number, update: DrawingAnnotationUpdate): Promise<void> {
  return enqueueDrawingUpdate((drawing) => {
    const next = cloneDrawing(drawing);
    const annotation = next.sheets.flatMap((sheet) => sheet.annotations)
      .find((candidate) => candidate.id === annotationId);
    if (!annotation) return drawing;
    applyAnnotationUpdate(annotation, update);
    return next;
  });
}

export function deleteDrawingAnnotation(annotationId: number): Promise<void> {
  return enqueueDrawingUpdate((drawing) => {
    const next = cloneDrawing(drawing);
    for (const sheet of next.sheets) {
      sheet.annotations = sheet.annotations.filter((annotation) => annotation.id !== annotationId);
    }
    queueMicrotask(() => useAppStore.getState().setSelectedDrawingAnnotationId(null));
    return next;
  });
}

export function updateActiveDrawingSheet(update: Partial<DrawingSheetDto>): Promise<void> {
  return enqueueDrawingUpdate((drawing) => {
    const next = cloneDrawing(drawing);
    const sheet = activeSheet(next);
    if (!sheet) return drawing;
    Object.assign(sheet, update);
    return next;
  });
}

/** Save the active sheet's standards, title defaults, and complete style as a
 * project-local company template. Existing sheets remain self-contained. */
export function saveActiveDrawingTemplate(name: string): Promise<void> {
  return enqueueDrawingUpdate((drawing) => {
    const normalized = name.trim();
    if (!normalized) throw new Error('Enter a template name first.');
    const next = cloneDrawing(drawing);
    const sheet = activeSheet(next);
    if (!sheet) throw new Error('Create a drawing sheet first.');
    const existing = next.templates.find((template) => template.name.toLocaleLowerCase() === normalized.toLocaleLowerCase());
    const values: Omit<DrawingTemplateDto, 'id'> = {
      name: normalized,
      standard: sheet.standard,
      projection_method: sheet.projection_method,
      tolerance_note: structuredClone(sheet.tolerance_note),
      title_defaults: structuredClone(sheet.title_block),
      style: structuredClone(sheet.style),
    };
    if (existing) Object.assign(existing, values);
    else next.templates.push({ id: next.next_template_id++, ...values });
    sheet.template_name = normalized;
    return next;
  });
}

export function applyDrawingTemplate(templateId: number): Promise<void> {
  return enqueueDrawingUpdate((drawing) => {
    const next = cloneDrawing(drawing);
    const sheet = activeSheet(next);
    const template = next.templates.find((candidate) => candidate.id === templateId);
    if (!sheet || !template) throw new Error('The selected drawing template no longer exists.');
    sheet.standard = template.standard;
    sheet.projection_method = template.projection_method;
    sheet.tolerance_note = structuredClone(template.tolerance_note);
    sheet.style = structuredClone(template.style);
    sheet.template_name = template.name;
    // Keep sheet-identity fields unless the template supplies an intentional
    // company default. This avoids changing a drawing number or revision when
    // an existing drawing is restyled.
    const identity = {
      title: sheet.title_block.title,
      drawing_number: sheet.title_block.drawing_number,
      revision: sheet.title_block.revision,
    };
    sheet.title_block = { ...structuredClone(template.title_defaults), ...identity };
    return next;
  });
}

export function deleteDrawingTemplate(templateId: number): Promise<void> {
  return enqueueDrawingUpdate((drawing) => {
    const next = cloneDrawing(drawing);
    next.templates = next.templates.filter((template) => template.id !== templateId);
    return next;
  });
}

export function addDrawingRevision(
  revision: Omit<DrawingRevisionDto, 'id'>,
): Promise<void> {
  return enqueueDrawingUpdate((drawing) => {
    const next = cloneDrawing(drawing);
    const sheet = activeSheet(next);
    if (!sheet) throw new Error('Create a drawing sheet first.');
    const entry: DrawingRevisionDto = { ...structuredClone(revision), id: next.next_revision_id++ };
    sheet.revisions.push(entry);
    sheet.title_block.revision = entry.revision;
    if (entry.status === 'released') {
      sheet.release = {
        status: 'released',
        released_revision: entry.revision,
        released_at: entry.date,
      };
    }
    return next;
  }, true, revision.status === 'released');
}

export function updateDrawingRevision(
  revisionId: number,
  update: Partial<Omit<DrawingRevisionDto, 'id'>>,
): Promise<void> {
  return enqueueDrawingUpdate((drawing) => {
    const next = cloneDrawing(drawing);
    const sheet = activeSheet(next);
    const revision = sheet?.revisions.find((candidate) => candidate.id === revisionId);
    if (!sheet || !revision) return drawing;
    if (revision.status === 'released') {
      throw new Error(`Revision ${revision.revision} is released and immutable. Add the next revision to make changes.`);
    }
    Object.assign(revision, structuredClone(update));
    sheet.title_block.revision = revision.revision;
    if (update.status === 'released') {
      sheet.release = {
        status: 'released',
        released_revision: revision.revision,
        released_at: revision.date,
      };
    }
    return next;
  }, true, update.status === 'released');
}

export function deleteDrawingRevision(revisionId: number): Promise<void> {
  return enqueueDrawingUpdate((drawing) => {
    const next = cloneDrawing(drawing);
    const sheet = activeSheet(next);
    if (!sheet) return drawing;
    const revision = sheet.revisions.find((candidate) => candidate.id === revisionId);
    if (revision?.status === 'released') {
      throw new Error(`Released revision ${revision.revision} cannot be deleted.`);
    }
    sheet.revisions = sheet.revisions.filter((revision) => revision.id !== revisionId);
    return next;
  });
}

export function addDrawingBomItem(
  item: Partial<Omit<DrawingBomItemDto, 'id'>> = {},
): Promise<void> {
  return enqueueDrawingUpdate((drawing, state) => {
    const next = cloneDrawing(drawing);
    const sheet = activeSheet(next);
    if (!sheet) throw new Error('Create a drawing sheet first.');
    const body = item.body_id != null
      ? state.solidScene.bodies.find((candidate) => candidate.id === item.body_id)
      : state.solidScene.bodies.find((candidate) => !sheet.bom.some((entry) => entry.body_id === candidate.id));
    const id = next.next_bom_item_id++;
    sheet.bom.push({
      id,
      item_number: item.item_number ?? String(sheet.bom.length + 1),
      body_id: item.body_id ?? body?.id ?? null,
      part_number: item.part_number ?? '',
      description: item.description ?? body?.name ?? `Item ${sheet.bom.length + 1}`,
      quantity: item.quantity ?? 1,
      material: item.material ?? '',
      finish: item.finish ?? '',
    });
    return next;
  });
}

export function updateDrawingBomItem(
  itemId: number,
  update: Partial<Omit<DrawingBomItemDto, 'id'>>,
): Promise<void> {
  return enqueueDrawingUpdate((drawing) => {
    const next = cloneDrawing(drawing);
    const item = activeSheet(next)?.bom.find((candidate) => candidate.id === itemId);
    if (!item) return drawing;
    Object.assign(item, structuredClone(update));
    return next;
  });
}

export function deleteDrawingBomItem(itemId: number): Promise<void> {
  return enqueueDrawingUpdate((drawing) => {
    const next = cloneDrawing(drawing);
    const sheet = activeSheet(next);
    if (!sheet) return drawing;
    sheet.bom = sheet.bom.filter((item) => item.id !== itemId);
    sheet.annotations = sheet.annotations.filter(
      (annotation) => annotation.kind !== 'item_balloon' || annotation.bom_item_id !== itemId,
    );
    return next;
  });
}

export function activeDrawingSheet(drawing: DrawingDocumentDto): DrawingSheetDto | null {
  return activeSheet(drawing);
}

/** Drawing history is independent from solid feature history. Auto Layout is
 * one snapshot mutation, so one Undo removes all related projected views. */
export function undoDrawingDocument(): Promise<boolean> {
  return enqueueDrawingHistoryRestore('undo');
}

export function redoDrawingDocument(): Promise<boolean> {
  return enqueueDrawingHistoryRestore('redo');
}

function addViewAnnotation(
  viewId: number,
  make: (id: number) => DrawingAnnotationDto,
): Promise<void> {
  return enqueueDrawingUpdate((drawing) => {
    const next = cloneDrawing(drawing);
    const sheet = activeSheet(next);
    if (!sheet?.views.some((view) => view.id === viewId)) {
      throw new Error('The projected view for this annotation no longer exists.');
    }
    const id = next.next_annotation_id++;
    sheet.annotations.push(make(id));
    queueMicrotask(() => selectCreatedAnnotation(id));
    return next;
  });
}

function selectCreatedAnnotation(id: number): void {
  const store = useAppStore.getState();
  store.setSelectedDrawingViewId(null);
  store.setSelectedDrawingAnnotationId(id);
  store.setDrawingTool(null);
}

function enqueueDrawingCommand(make: (drawing: DrawingDocumentDto, state: ReturnType<typeof useAppStore.getState>) => import('../engine/types').DrawingCommandDto, recordHistory = true): Promise<void> {
  const operation = writeQueue.then(async () => {
    const state = useAppStore.getState(); const projectKey = currentHistoryProjectKey(); const engine = await getEngine();
    const before = await engine.drawingDocument();
    const next = await engine.drawingApply(make(before, state));
    useAppStore.setState({ drawingDocument: next, dirty: true });
    if (recordHistory) recordDrawingHistory(projectKey, before, next);
  });
  writeQueue = operation.catch(() => undefined); return operation;
}

function enqueueDrawingUpdate(
  mutate: (drawing: DrawingDocumentDto, state: ReturnType<typeof useAppStore.getState>) => DrawingDocumentDto,
  recordHistory = true,
  preserveRelease = false,
): Promise<void> {
  const operation = writeQueue.then(async () => {
    const state = useAppStore.getState();
    const projectKey = currentHistoryProjectKey();
    const before = state.drawingDocument;
    const next = mutate(before, state);
    if (next === before) return;
    if (!preserveRelease) returnReleasedSheetsToDraft(before, next);
    await state.setDrawingDocument(next);
    if (recordHistory) {
      recordDrawingHistory(
        projectKey,
        before,
        useAppStore.getState().drawingDocument,
      );
    }
  });
  writeQueue = operation.catch(() => undefined);
  return operation;
}

function returnReleasedSheetsToDraft(before: DrawingDocumentDto, next: DrawingDocumentDto): void {
  for (const prior of before.sheets) {
    if (prior.release.status !== 'released') continue;
    const current = next.sheets.find((sheet) => sheet.id === prior.id);
    if (!current || current.release.status !== 'released') continue;
    const priorComparable = { ...prior, release: null };
    const currentComparable = { ...current, release: null };
    if (JSON.stringify(priorComparable) !== JSON.stringify(currentComparable)) {
      current.release = {
        ...current.release,
        status: 'draft',
      };
    }
  }
}

function enqueueDrawingHistoryRestore(
  direction: 'undo' | 'redo',
): Promise<boolean> {
  const operation = writeQueue.then(async () => {
    const projectKey = currentHistoryProjectKey();
    const entry = direction === 'undo'
      ? peekDrawingUndoHistory(projectKey)
      : peekDrawingRedoHistory(projectKey);
    if (!entry) return false;
    const target = structuredClone(
      direction === 'undo' ? entry.before : entry.after,
    );
    await useAppStore.getState().setDrawingDocument(target);
    const committed = direction === 'undo'
      ? commitDrawingUndoHistory(projectKey, entry)
      : commitDrawingRedoHistory(projectKey, entry);
    if (!committed) {
      throw new Error('Drawing history changed while restoring a command.');
    }
    resetDrawingUiAfterHistory(target);
    return true;
  });
  writeQueue = operation.then(() => undefined, () => undefined);
  return operation;
}

function resetDrawingUiAfterHistory(drawing: DrawingDocumentDto): void {
  const store = useAppStore.getState();
  store.setSelectedDrawingViewId(null);
  store.setSelectedDrawingAnnotationId(null);
  store.setDrawingTool(null);
  store.setDrawingPendingViewKind(null);
  store.setDrawingSheetSetupOpen(
    store.activeTab === 'drawing'
    && (
      drawing.active_sheet_id === null
      || !drawing.sheets.some((sheet) => sheet.id === drawing.active_sheet_id)
    ),
  );
}

function makeView(
  id: number,
  kind: DrawingViewKind,
  position: [number, number],
  scale: number,
  parentViewId: number | null,
  alignment: DrawingViewDto['alignment'],
): DrawingViewDto {
  const basis = standardViewBasis(kind);
  return {
    id,
    name: viewLabel(kind),
    kind,
    direction: basis.direction,
    up: basis.up,
    position,
    scale,
    body_ids: [],
    show_hidden_lines: false,
    show_tangent_edges: false,
    parent_view_id: parentViewId,
    alignment,
    derivation: null,
  };
}

/**
 * Resolve any selected projected view back to the first/base view in its
 * placement group. New projected views always reference this root directly,
 * so selecting a child can never create a drifting parent chain.
 */
export function drawingViewGroupRoot(
  sheet: DrawingSheetDto,
  viewId: number,
): DrawingViewDto | null {
  let current = sheet.views.find((view) => view.id === viewId) ?? null;
  const visited = new Set<number>();
  while (current?.parent_view_id != null && !visited.has(current.id)) {
    visited.add(current.id);
    const parent = sheet.views.find((view) => view.id === current?.parent_view_id) ?? null;
    if (!parent) break;
    current = parent;
  }
  return current;
}

/** The selected group's root, or the sheet's first placed view. */
export function drawingViewPlacementRoot(
  sheet: DrawingSheetDto,
  requestedParentId: number | null,
): DrawingViewDto | null {
  if (requestedParentId != null) {
    const selectedRoot = drawingViewGroupRoot(sheet, requestedParentId);
    if (selectedRoot) return selectedRoot;
  }
  const first = sheet.views[0];
  return first ? drawingViewGroupRoot(sheet, first.id) : null;
}

/**
 * Build the exact transient/committed view placement. Keeping this pure lets
 * the React UI preview the same alignment and scale that will be committed.
 */
export function drawingViewPlacementDraft(
  sheet: DrawingSheetDto,
  scene: SolidSceneDto,
  kind: DrawingViewKind,
  position: [number, number],
  requestedParentId: number | null = null,
  requestedScale?: number,
  id = -1,
): DrawingViewDto {
  const root = drawingViewPlacementRoot(sheet, requestedParentId);
  const relationship = relatedViewAlignment(kind, root);
  const alignedPosition: [number, number] = relationship.alignment === 'vertical' && root
    ? [root.position[0], position[1]]
    : relationship.alignment === 'horizontal' && root
      ? [position[0], root.position[1]]
      : position;
  const scale = requestedScale
    ?? root?.scale
    ?? suggestedViewScale(scene, ...drawingSheetSize(sheet.format, sheet.orientation));
  const view = makeView(
    id,
    kind,
    alignedPosition,
    scale,
    relationship.parentId,
    relationship.alignment,
  );
  if (root) {
    view.scope = root.scope ?? 'definition';
    view.occurrence_ids = [...(root.occurrence_ids ?? [])];
    view.body_ids = [...root.body_ids];
    view.show_hidden_lines = root.show_hidden_lines;
    view.show_tangent_edges = root.show_tangent_edges;
  }
  return view;
}

/** A useful first preview location before the pointer enters the sheet. */
export function defaultDrawingViewPlacementPosition(
  sheet: DrawingSheetDto,
  kind: DrawingViewKind,
  requestedParentId: number | null = null,
): [number, number] {
  const [width, height] = drawingSheetSize(sheet.format, sheet.orientation);
  const root = drawingViewPlacementRoot(sheet, requestedParentId);
  if (!root) return [width * 0.45, height * 0.43];

  const verticalOffset = Math.min(height * 0.28, 70);
  const horizontalOffset = Math.min(width * 0.24, 90);
  const topAbove = sheet.projection_method === 'third_angle';
  const rightOnRight = sheet.projection_method === 'third_angle';
  let position: [number, number];
  if (kind === 'top' || kind === 'bottom') {
    const topDirection = topAbove ? -1 : 1;
    const direction = kind === 'top' ? topDirection : -topDirection;
    position = [root.position[0], root.position[1] + direction * verticalOffset];
  } else if (kind === 'left' || kind === 'right') {
    const rightDirection = rightOnRight ? 1 : -1;
    const direction = kind === 'right' ? rightDirection : -rightDirection;
    position = [root.position[0] + direction * horizontalOffset, root.position[1]];
  } else {
    position = [root.position[0] + horizontalOffset, root.position[1] - verticalOffset * 0.7];
  }
  return [
    Math.max(10, Math.min(width - 10, position[0])),
    Math.max(10, Math.min(height - 10, position[1])),
  ];
}

function relatedViewAlignment(
  kind: DrawingViewKind,
  parent: DrawingViewDto | null,
): { parentId: number | null; alignment: DrawingViewDto['alignment'] } {
  if (!parent) {
    return { parentId: null, alignment: 'free' };
  }
  if (kind === 'top' || kind === 'bottom') {
    return { parentId: parent.id, alignment: 'vertical' };
  }
  if (kind === 'left' || kind === 'right') {
    return { parentId: parent.id, alignment: 'horizontal' };
  }
  return { parentId: parent.id, alignment: 'free' };
}

function standardViewBasis(kind: DrawingViewKind): {
  direction: [number, number, number];
  up: [number, number, number];
} {
  switch (kind) {
    case 'front': return { direction: [0, -1, 0], up: [0, 0, 1] };
    case 'rear': return { direction: [0, 1, 0], up: [0, 0, 1] };
    case 'left': return { direction: [-1, 0, 0], up: [0, 0, 1] };
    case 'right': return { direction: [1, 0, 0], up: [0, 0, 1] };
    case 'top': return { direction: [0, 0, 1], up: [0, 1, 0] };
    case 'bottom': return { direction: [0, 0, -1], up: [0, 1, 0] };
    case 'isometric': return { direction: [1, -1, 1], up: [0, 0, 1] };
    case 'custom': return { direction: [1, -1, 1], up: [0, 0, 1] };
    case 'section':
    case 'detail':
    case 'auxiliary':
    case 'broken':
    case 'removed_section': return { direction: [1, -1, 1], up: [0, 0, 1] };
  }
}

function viewLabel(kind: DrawingViewKind): string {
  return kind === 'isometric' ? 'Isometric' : `${kind.charAt(0).toUpperCase()}${kind.slice(1)}`;
}

function suggestedViewScale(scene: SolidSceneDto, sheetWidth = 297, sheetHeight = 210): number {
  const min = [Number.POSITIVE_INFINITY, Number.POSITIVE_INFINITY, Number.POSITIVE_INFINITY];
  const max = [Number.NEGATIVE_INFINITY, Number.NEGATIVE_INFINITY, Number.NEGATIVE_INFINITY];
  for (const body of scene.bodies) {
    for (let index = 0; index + 2 < body.mesh.positions.length; index += 3) {
      for (let axis = 0; axis < 3; axis += 1) {
        const value = body.mesh.positions[index + axis];
        min[axis] = Math.min(min[axis], value);
        max[axis] = Math.max(max[axis], value);
      }
    }
  }
  const largest = Math.max(...max.map((value, index) => value - min[index]));
  if (!Number.isFinite(largest) || largest <= 0) return 1;
  const target = Math.min(sheetWidth * 0.2, sheetHeight * 0.23) / largest;
  const standard = [10, 5, 2, 1, 0.5, 0.2, 0.1, 0.05, 0.02, 0.01];
  return standard.find((candidate) => candidate <= target) ?? 0.01;
}

function activeSheet(drawing: DrawingDocumentDto): DrawingSheetDto | null {
  return drawing.sheets.find((sheet) => sheet.id === drawing.active_sheet_id) ?? null;
}

function cloneDrawing(drawing: DrawingDocumentDto): DrawingDocumentDto {
  return structuredClone(drawing);
}

function applyAnnotationUpdate(annotation: DrawingAnnotationDto, update: DrawingAnnotationUpdate): void {
  const target = annotation as unknown as Record<string, unknown>;
  for (const [key, value] of Object.entries(update)) {
    if (value !== undefined && key in target) target[key] = structuredClone(value);
  }
  if ('extension' in target && typeof target.extension === 'number') {
    target.extension = Math.max(0, target.extension);
  }
}

function clearDrawingSelection(): void {
  const store = useAppStore.getState();
  store.setSelectedDrawingViewId(null);
  store.setSelectedDrawingAnnotationId(null);
}

function distance3(left: [number, number, number], right: [number, number, number]): number {
  return Math.hypot(left[0] - right[0], left[1] - right[1], left[2] - right[2]);
}
function subtract3(left: [number, number, number], right: [number, number, number]): [number, number, number] { return [left[0] - right[0], left[1] - right[1], left[2] - right[2]]; }
function cross3(left: [number, number, number], right: [number, number, number]): [number, number, number] { return [left[1] * right[2] - left[2] * right[1], left[2] * right[0] - left[0] * right[2], left[0] * right[1] - left[1] * right[0]]; }
function scale3(vector: [number, number, number], scalar: number): [number, number, number] { return [vector[0] * scalar, vector[1] * scalar, vector[2] * scalar]; }
function normalize3(vector: [number, number, number]): [number, number, number] | null { const length = Math.hypot(...vector); return length < 1e-9 ? null : scale3(vector, 1 / length); }

function bestHoleDefinitionForCircle(
  definitions: HoleDefinitionDto[],
  feature: DrawingCircularRefDto,
): HoleDefinitionDto | null {
  let best: { definition: HoleDefinitionDto; score: number } | null = null;
  for (const definition of definitions) {
    if (definition.body_id !== feature.body_id || !definition.face_basis) continue;
    const radiusError = Math.abs(definition.diameter / 2 - feature.fallback_radius);
    const radiusTolerance = Math.max(0.03, definition.diameter * 0.015);
    if (radiusError > radiusTolerance) continue;
    const basis = definition.face_basis;
    const normal = normalize3(feature.fallback_normal);
    if (normal && Math.abs(normal[0] * basis.normal[0] + normal[1] * basis.normal[1] + normal[2] * basis.normal[2]) < 0.985) continue;
    const delta = subtract3(feature.fallback_center, basis.origin);
    const projected: [number, number] = [
      delta[0] * basis.u[0] + delta[1] * basis.u[1] + delta[2] * basis.u[2],
      delta[0] * basis.v[0] + delta[1] * basis.v[1] + delta[2] * basis.v[2],
    ];
    const positions = definition.positions.length > 0
      ? definition.positions.map((entry) => entry.position)
      : [definition.position];
    const centerError = Math.min(...positions.map((position) => Math.hypot(projected[0] - position.x, projected[1] - position.y)));
    const centerTolerance = Math.max(0.08, definition.diameter * 0.025);
    if (centerError > centerTolerance) continue;
    const score = centerError + radiusError * 4;
    if (!best || score < best.score || (score === best.score && definition.feature_id < best.definition.feature_id)) {
      best = { definition, score };
    }
  }
  return best?.definition ?? null;
}
