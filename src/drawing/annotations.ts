import type {
  DrawingAnnotationDto,
  DrawingAttachmentRefDto,
  DrawingCircularRefDto,
  DrawingDimensionPresentationDto,
  DrawingLineDimensionMode,
  DrawingLinearDimensionMode,
  DrawingLineRefDto,
  DrawingProjectionAnchorDto,
  DrawingProjectionDto,
  DrawingProjectedCircleDto,
  DrawingRadialDimensionMode,
  DrawingStandard,
  DrawingTopologyAnchorRefDto,
  DrawingViewDto,
} from '../engine/types';
import type { UnitSystem } from '../types/document';

export interface ResolvedDrawingAnchor {
  anchor: DrawingProjectionAnchorDto;
  paper: [number, number];
  resolution: 'exact' | 'edge_key';
}

export interface DrawingDimensionGeometry {
  first: [number, number];
  second: [number, number];
  dimensionStart: [number, number];
  dimensionEnd: [number, number];
  firstExtension: [[number, number], [number, number]];
  secondExtension: [[number, number], [number, number]];
  textPosition: [number, number];
  textAngle: number;
  arrowSize: number;
  value: number;
}

export interface DrawingLinearDimensionLayout {
  lineStart: [number, number];
  lineEnd: [number, number];
  firstArrowToward: [number, number];
  secondArrowToward: [number, number];
  textPosition: [number, number];
  textWidth: number;
  arrowsOutside: boolean;
  textOutside: boolean;
  maskDimensionLine: boolean;
}

export interface ResolvedDrawingCircle {
  circle: DrawingProjectedCircleDto;
  center: [number, number];
  paperRadius: number;
  resolution: 'exact' | 'edge_key';
}

export interface DrawingRadialGeometry {
  center: [number, number];
  featurePoint: [number, number];
  shoulder: [number, number];
  textPosition: [number, number];
  value: number;
}

export interface DrawingAngularGeometry {
  vertex: [number, number];
  firstRay: [number, number];
  secondRay: [number, number];
  arcStart: [number, number];
  arcEnd: [number, number];
  arcPath: string;
  textPosition: [number, number];
  value: number;
}

export type DrawingLineDimensionGeometry =
  | { kind: 'linear'; geometry: DrawingDimensionGeometry }
  | { kind: 'angular'; geometry: DrawingAngularGeometry };

export interface DrawingCenterMarkGeometry {
  center: [number, number];
  horizontal: [[number, number], [number, number]];
  vertical: [[number, number], [number, number]];
}

export interface DrawingCenterLineGeometry {
  firstCenter: [number, number];
  secondCenter: [number, number];
  start: [number, number];
  end: [number, number];
}

export interface ResolvedDrawingLine {
  reference: DrawingLineRefDto;
  start: [number, number];
  end: [number, number];
  resolution: 'exact' | 'edge_key';
}

export interface DrawingParallelEdgeCenterLineGeometry {
  firstMidpoint: [number, number];
  secondMidpoint: [number, number];
  start: [number, number];
  end: [number, number];
}

export interface DrawingBoltCircleGeometry {
  center: [number, number];
  radius: number;
  marks: DrawingCenterMarkGeometry[];
}

export interface DrawingOrdinateGeometry {
  origin: [number, number];
  target: [number, number];
  elbow: [number, number];
  textPosition: [number, number];
  xValue: number;
  yValue: number;
}

export interface DrawingArcLengthGeometry {
  center: [number, number];
  radius: number;
  start: [number, number];
  end: [number, number];
  textPosition: [number, number];
  path: string;
  value: number;
}

export interface ResolvedDrawingAttachment {
  point: [number, number];
  resolution: 'exact' | 'edge_key';
}

export function drawingAnchorRef(
  anchor: DrawingProjectionAnchorDto,
  projection?: DrawingProjectionDto,
): DrawingTopologyAnchorRefDto {
  return {
    topology_signature: projection?.topology_signatures?.[String(anchor.body_id)],
    occurrence_id: anchor.occurrence_id,
    body_id: anchor.body_id,
    edge_id: anchor.edge_id,
    edge_key: anchor.edge_key,
    endpoint: anchor.endpoint,
    fallback_point: anchor.model_point,
  };
}

export function drawingCircleCenterAnchorRef(
  circle: DrawingProjectedCircleDto,
  projection?: DrawingProjectionDto,
): DrawingTopologyAnchorRefDto {
  return {
    topology_signature: projection?.topology_signatures?.[String(circle.body_id)],
    occurrence_id: circle.occurrence_id,
    body_id: circle.body_id,
    edge_id: circle.edge_id,
    edge_key: circle.edge_key,
    // Retained for backward-compatible anchor serialization. Resolution uses
    // the analytic circle while circle_center is true.
    endpoint: 'start',
    fallback_point: circle.center_model,
    circle_center: true,
  };
}

export function drawingCircularRef(circle: DrawingProjectedCircleDto, projection?: DrawingProjectionDto): DrawingCircularRefDto {
  return {
    topology_signature: projection?.topology_signatures?.[String(circle.body_id)],
    occurrence_id: circle.occurrence_id,
    body_id: circle.body_id,
    edge_id: circle.edge_id,
    edge_key: circle.edge_key,
    fallback_center: circle.center_model,
    fallback_normal: circle.normal_model,
    fallback_radius: circle.radius,
    closed: circle.closed,
  };
}

export function drawingLineRef(
  bodyId: number,
  edgeId: number,
  edgeKey: string,
  start: [number, number, number],
  end: [number, number, number],
  occurrenceId?: number | null,
  projection?: DrawingProjectionDto,
): DrawingLineRefDto {
  return {
    topology_signature: projection?.topology_signatures?.[String(bodyId)],
    occurrence_id: occurrenceId,
    body_id: bodyId,
    edge_id: edgeId,
    edge_key: edgeKey,
    fallback_start: start,
    fallback_end: end,
  };
}

export function drawingProjectedPointToPaper(
  view: DrawingViewDto,
  projection: DrawingProjectionDto,
  point: [number, number],
): [number, number] {
  const centerX = (projection.bounds[0] + projection.bounds[2]) / 2;
  const centerY = (projection.bounds[1] + projection.bounds[3]) / 2;
  return [
    view.position[0] + (point[0] - centerX) * view.scale,
    view.position[1] - (point[1] - centerY) * view.scale,
  ];
}

export function resolveDrawingAnchor(
  reference: DrawingTopologyAnchorRefDto,
  view: DrawingViewDto,
  projection: DrawingProjectionDto,
): ResolvedDrawingAnchor | null {
  if ((reference.topology_signature ?? null) !== (projection.topology_signatures?.[String(reference.body_id)] ?? null)) return null;
  if (reference.circle_center) {
    const exact = projection.circles.find((candidate) =>
      (candidate.occurrence_id ?? null) === (reference.occurrence_id ?? null) && candidate.body_id === reference.body_id && candidate.edge_id === reference.edge_id
        && candidate.edge_key === reference.edge_key,
    );
    const circle = exact ?? projection.circles.find((candidate) =>
      (candidate.occurrence_id ?? null) === (reference.occurrence_id ?? null) && candidate.body_id === reference.body_id && candidate.edge_key === reference.edge_key,
    );
    if (!circle) return null;
    return {
      anchor: {
        occurrence_id: circle.occurrence_id,
    body_id: circle.body_id,
        edge_id: circle.edge_id,
        edge_key: circle.edge_key,
        endpoint: reference.endpoint,
        model_point: circle.center_model,
        point: circle.center,
        hidden: circle.hidden,
      },
      paper: drawingProjectedPointToPaper(view, projection, circle.center),
      resolution: exact ? 'exact' : 'edge_key',
    };
  }
  const exact = projection.anchors.find((candidate) =>
    (candidate.occurrence_id ?? null) === (reference.occurrence_id ?? null) && candidate.body_id === reference.body_id
      && candidate.edge_id === reference.edge_id
      && candidate.edge_key === reference.edge_key
      && candidate.endpoint === reference.endpoint,
  );
  const anchor = exact ?? projection.anchors.find((candidate) =>
    (candidate.occurrence_id ?? null) === (reference.occurrence_id ?? null) && candidate.body_id === reference.body_id
      && candidate.edge_key === reference.edge_key
      && candidate.endpoint === reference.endpoint,
  );
  if (!anchor) return null;
  return {
    anchor,
    paper: drawingProjectedPointToPaper(view, projection, anchor.point),
    resolution: exact ? 'exact' : 'edge_key',
  };
}

export function centerMarkGeometry(
  resolved: ResolvedDrawingCircle,
  extension: number,
): DrawingCenterMarkGeometry {
  const halfSpan = resolved.paperRadius + Math.max(0, extension);
  return {
    center: resolved.center,
    horizontal: [
      [resolved.center[0] - halfSpan, resolved.center[1]],
      [resolved.center[0] + halfSpan, resolved.center[1]],
    ],
    vertical: [
      [resolved.center[0], resolved.center[1] - halfSpan],
      [resolved.center[0], resolved.center[1] + halfSpan],
    ],
  };
}

export function centerLineGeometry(
  first: ResolvedDrawingCircle,
  second: ResolvedDrawingCircle,
  extension: number,
): DrawingCenterLineGeometry | null {
  const vector = subtract(second.center, first.center);
  const length = magnitude(vector);
  if (length < 1e-7) return null;
  const direction = scale(vector, 1 / length);
  const extra = Math.max(0, extension);
  return {
    firstCenter: first.center,
    secondCenter: second.center,
    start: add(first.center, scale(direction, -(first.paperRadius + extra))),
    end: add(second.center, scale(direction, second.paperRadius + extra)),
  };
}

export function automaticSymmetryAxisGeometry(
  view: DrawingViewDto,
  projection: DrawingProjectionDto,
  axis: 'x' | 'y' | 'both',
  extension: number,
): Array<[[number, number], [number, number]]> {
  const [minX, minY, width, height] = (() => {
    const first = drawingProjectedPointToPaper(view, projection, [projection.bounds[0], projection.bounds[1]]);
    const second = drawingProjectedPointToPaper(view, projection, [projection.bounds[2], projection.bounds[3]]);
    return [Math.min(first[0], second[0]), Math.min(first[1], second[1]), Math.abs(second[0] - first[0]), Math.abs(second[1] - first[1])] as const;
  })();
  const extra = Math.max(0, extension);
  const centerX = minX + width / 2;
  const centerY = minY + height / 2;
  const result: Array<[[number, number], [number, number]]> = [];
  if (axis === 'x' || axis === 'both') result.push([[minX - extra, centerY], [minX + width + extra, centerY]]);
  if (axis === 'y' || axis === 'both') result.push([[centerX, minY - extra], [centerX, minY + height + extra]]);
  return result;
}

export function boltCircleGeometry(
  circles: ResolvedDrawingCircle[],
  extension: number,
): DrawingBoltCircleGeometry | null {
  if (circles.length < 3) return null;
  const center: [number, number] = [
    circles.reduce((sum, circle) => sum + circle.center[0], 0) / circles.length,
    circles.reduce((sum, circle) => sum + circle.center[1], 0) / circles.length,
  ];
  const distances = circles.map((circle) => magnitude(subtract(circle.center, center)));
  const radius = distances.reduce((sum, value) => sum + value, 0) / distances.length;
  const tolerance = Math.max(0.35, radius * 0.015);
  if (radius < 1e-5 || distances.some((value) => Math.abs(value - radius) > tolerance)) return null;
  return {
    center,
    radius,
    marks: circles.map((circle) => centerMarkGeometry(circle, extension)),
  };
}

export function resolveDrawingAttachment(
  attachment: DrawingAttachmentRefDto,
  view: DrawingViewDto,
  projection: DrawingProjectionDto,
): ResolvedDrawingAttachment | null {
  if (attachment.type === 'anchor') {
    const resolved = resolveDrawingAnchor(attachment.reference, view, projection);
    return resolved ? { point: resolved.paper, resolution: resolved.resolution } : null;
  }
  if (attachment.type === 'circle') {
    const resolved = resolveDrawingCircle(attachment.reference, view, projection);
    return resolved ? { point: resolved.center, resolution: resolved.resolution } : null;
  }
  const resolved = resolveDrawingLine(attachment.reference, view, projection);
  return resolved ? { point: midpoint(resolved.start, resolved.end), resolution: resolved.resolution } : null;
}

export function resolveDrawingLine(
  reference: DrawingLineRefDto,
  view: DrawingViewDto,
  projection: DrawingProjectionDto,
): ResolvedDrawingLine | null {
  if ((reference.topology_signature ?? null) !== (projection.topology_signatures?.[String(reference.body_id)] ?? null)) return null;
  const exactStart = projection.anchors.find((candidate) =>
    (candidate.occurrence_id ?? null) === (reference.occurrence_id ?? null) && candidate.body_id === reference.body_id
      && candidate.edge_id === reference.edge_id
      && candidate.endpoint === 'start',
  );
  const exactEnd = projection.anchors.find((candidate) =>
    (candidate.occurrence_id ?? null) === (reference.occurrence_id ?? null) && candidate.body_id === reference.body_id
      && candidate.edge_id === reference.edge_id
      && candidate.endpoint === 'end',
  );
  const start = exactStart ?? projection.anchors.find((candidate) =>
    (candidate.occurrence_id ?? null) === (reference.occurrence_id ?? null) && candidate.body_id === reference.body_id
      && candidate.edge_key === reference.edge_key
      && candidate.endpoint === 'start',
  );
  const end = exactEnd ?? projection.anchors.find((candidate) =>
    (candidate.occurrence_id ?? null) === (reference.occurrence_id ?? null) && candidate.body_id === reference.body_id
      && candidate.edge_key === reference.edge_key
      && candidate.endpoint === 'end',
  );
  if (!start || !end) return null;
  return {
    reference,
    start: drawingProjectedPointToPaper(view, projection, start.point),
    end: drawingProjectedPointToPaper(view, projection, end.point),
    resolution: exactStart && exactEnd ? 'exact' : 'edge_key',
  };
}

/**
 * Constructs the axis of symmetry between two parallel projected edges.
 * The result spans the union of both edges and extends at each end in paper
 * millimetres, matching the ISO/ASME long-short center-line convention.
 */
export function centerLineBetweenEdgesGeometry(
  first: ResolvedDrawingLine,
  second: ResolvedDrawingLine,
  extension: number,
): DrawingParallelEdgeCenterLineGeometry | null {
  const firstVector = subtract(first.end, first.start);
  const secondVector = subtract(second.end, second.start);
  const firstLength = magnitude(firstVector);
  const secondLength = magnitude(secondVector);
  if (firstLength < 1e-7 || secondLength < 1e-7) return null;
  const direction = scale(firstVector, 1 / firstLength);
  const secondDirection = scale(secondVector, 1 / secondLength);
  const parallelError = Math.abs(direction[0] * secondDirection[1] - direction[1] * secondDirection[0]);
  if (parallelError > Math.sin(0.75 * Math.PI / 180)) return null;

  const normal: [number, number] = [-direction[1], direction[0]];
  const firstMidpoint = midpoint(first.start, first.end);
  const secondMidpoint = midpoint(second.start, second.end);
  const separation = Math.abs(
    (secondMidpoint[0] - firstMidpoint[0]) * normal[0]
      + (secondMidpoint[1] - firstMidpoint[1]) * normal[1],
  );
  if (separation < 1e-4) return null;

  const along = [first.start, first.end, second.start, second.end]
    .map((point) => point[0] * direction[0] + point[1] * direction[1]);
  const firstRange = [Math.min(along[0], along[1]), Math.max(along[0], along[1])] as const;
  const secondRange = [Math.min(along[2], along[3]), Math.max(along[2], along[3])] as const;
  const overlap = Math.min(firstRange[1], secondRange[1]) - Math.max(firstRange[0], secondRange[0]);
  if (overlap < Math.min(firstLength, secondLength) * 0.05) return null;

  const normalOffset = (
    firstMidpoint[0] * normal[0] + firstMidpoint[1] * normal[1]
      + secondMidpoint[0] * normal[0] + secondMidpoint[1] * normal[1]
  ) / 2;
  const extra = Math.max(0, extension);
  const startAlong = Math.min(...along) - extra;
  const endAlong = Math.max(...along) + extra;
  const pointAt = (distance: number): [number, number] => [
    direction[0] * distance + normal[0] * normalOffset,
    direction[1] * distance + normal[1] * normalOffset,
  ];
  return {
    firstMidpoint,
    secondMidpoint,
    start: pointAt(startAlong),
    end: pointAt(endAlong),
  };
}

export function resolveDrawingCircle(
  reference: DrawingCircularRefDto,
  view: DrawingViewDto,
  projection: DrawingProjectionDto,
): ResolvedDrawingCircle | null {
  if ((reference.topology_signature ?? null) !== (projection.topology_signatures?.[String(reference.body_id)] ?? null)) return null;
  const exact = projection.circles.find((candidate) =>
    (candidate.occurrence_id ?? null) === (reference.occurrence_id ?? null) && candidate.body_id === reference.body_id && candidate.edge_id === reference.edge_id,
  );
  const circle = exact ?? projection.circles.find((candidate) =>
    (candidate.occurrence_id ?? null) === (reference.occurrence_id ?? null) && candidate.body_id === reference.body_id && candidate.edge_key === reference.edge_key,
  );
  if (!circle) return null;
  return {
    circle,
    center: drawingProjectedPointToPaper(view, projection, circle.center),
    paperRadius: circle.radius * view.scale,
    resolution: exact ? 'exact' : 'edge_key',
  };
}

export function radialDimensionGeometry(
  resolved: ResolvedDrawingCircle,
  mode: DrawingRadialDimensionMode,
  leaderAngleDeg: number,
  offset: number,
): DrawingRadialGeometry {
  const angle = leaderAngleDeg * Math.PI / 180;
  const direction: [number, number] = [Math.cos(angle), Math.sin(angle)];
  const featurePoint = add(resolved.center, scale(direction, resolved.paperRadius));
  const shoulder = add(resolved.center, scale(direction, resolved.paperRadius + offset));
  return {
    center: resolved.center,
    featurePoint,
    shoulder,
    textPosition: add(shoulder, [direction[0] >= 0 ? 2 : -2, -1]),
    value: resolved.circle.radius * (mode === 'diameter' ? 2 : 1),
  };
}

export function angularDimensionGeometry(
  vertex: ResolvedDrawingAnchor,
  first: ResolvedDrawingAnchor,
  second: ResolvedDrawingAnchor,
  radius: number,
): DrawingAngularGeometry | null {
  const firstVector = subtract(first.paper, vertex.paper);
  const secondVector = subtract(second.paper, vertex.paper);
  const firstLength = magnitude(firstVector);
  const secondLength = magnitude(secondVector);
  if (firstLength < 1e-7 || secondLength < 1e-7) return null;
  const firstDirection = scale(firstVector, 1 / firstLength);
  const secondDirection = scale(secondVector, 1 / secondLength);
  const dot = Math.max(-1, Math.min(1, firstDirection[0] * secondDirection[0] + firstDirection[1] * secondDirection[1]));
  const angle = Math.acos(dot);
  if (angle < 1e-7) return null;
  const cross = firstDirection[0] * secondDirection[1] - firstDirection[1] * secondDirection[0];
  const sweep = cross >= 0 ? 1 : 0;
  const arcStart = add(vertex.paper, scale(firstDirection, radius));
  const arcEnd = add(vertex.paper, scale(secondDirection, radius));
  const bisector = add(firstDirection, secondDirection);
  const bisectorLength = magnitude(bisector);
  const textDirection = bisectorLength < 1e-7
    ? [-firstDirection[1], firstDirection[0]] as [number, number]
    : scale(bisector, 1 / bisectorLength);
  return {
    vertex: vertex.paper,
    firstRay: add(vertex.paper, scale(firstDirection, radius + 3)),
    secondRay: add(vertex.paper, scale(secondDirection, radius + 3)),
    arcStart,
    arcEnd,
    arcPath: `M${pointText(arcStart)} A${round(radius)},${round(radius)} 0 0 ${sweep} ${pointText(arcEnd)}`,
    textPosition: add(vertex.paper, scale(textDirection, radius + 4)),
    value: angle * 180 / Math.PI,
  };
}

export function linearDimensionGeometry(
  first: ResolvedDrawingAnchor,
  second: ResolvedDrawingAnchor,
  mode: DrawingLinearDimensionMode,
  offset: number,
  viewScale: number,
): DrawingDimensionGeometry | null {
  const a = first.paper;
  const b = second.paper;
  let dimensionStart: [number, number];
  let dimensionEnd: [number, number];
  let value: number;

  if (mode === 'horizontal') {
    if (Math.abs(b[0] - a[0]) < 1e-7) return null;
    dimensionStart = [a[0], a[1] + offset];
    dimensionEnd = [b[0], a[1] + offset];
    value = Math.abs(b[0] - a[0]) / viewScale;
  } else if (mode === 'vertical') {
    if (Math.abs(b[1] - a[1]) < 1e-7) return null;
    dimensionStart = [a[0] + offset, a[1]];
    dimensionEnd = [a[0] + offset, b[1]];
    value = Math.abs(b[1] - a[1]) / viewScale;
  } else {
    const delta = subtract(b, a);
    const length = magnitude(delta);
    if (length < 1e-7) return null;
    const normal: [number, number] = [-delta[1] / length, delta[0] / length];
    dimensionStart = add(a, scale(normal, offset));
    dimensionEnd = add(b, scale(normal, offset));
    // A drawing dimension measures the orthographic projection seen on the
    // sheet. Using the spatial distance between topology references turns a
    // 30 mm front-view span into a depth diagonal (for example 33.11 mm).
    value = length / viewScale;
  }

  const dimensionVector = subtract(dimensionEnd, dimensionStart);
  const dimensionLength = magnitude(dimensionVector);
  if (dimensionLength < 1e-7 || !Number.isFinite(value)) return null;
  const direction = scale(dimensionVector, 1 / dimensionLength);
  let textAngle = Math.atan2(direction[1], direction[0]) * 180 / Math.PI;
  if (textAngle > 90 || textAngle < -90) textAngle += 180;

  return {
    first: a,
    second: b,
    dimensionStart,
    dimensionEnd,
    firstExtension: extensionLine(a, dimensionStart),
    secondExtension: extensionLine(b, dimensionEnd),
    textPosition: midpoint(dimensionStart, dimensionEnd),
    textAngle,
    arrowSize: Math.min(2.5, Math.max(1.4, dimensionLength * 0.12)),
    value,
  };
}

/** Classifies the semantic result of selecting a second straight edge. */
export function drawingLineDimensionMode(
  first: Pick<ResolvedDrawingLine, 'start' | 'end'>,
  second: Pick<ResolvedDrawingLine, 'start' | 'end'>,
): Exclude<DrawingLineDimensionMode, 'length'> {
  const firstVector = subtract(first.end, first.start);
  const secondVector = subtract(second.end, second.start);
  const firstLength = magnitude(firstVector);
  const secondLength = magnitude(secondVector);
  if (firstLength < 1e-7 || secondLength < 1e-7) return 'angle';
  const firstDirection = scale(firstVector, 1 / firstLength);
  const secondDirection = scale(secondVector, 1 / secondLength);
  const cross = Math.abs(
    firstDirection[0] * secondDirection[1] - firstDirection[1] * secondDirection[0],
  );
  // Projected OCCT edges can carry small tessellation/projection noise. A
  // one-degree tolerance keeps genuinely parallel design edges deterministic.
  return cross <= Math.sin(Math.PI / 180) ? 'distance' : 'angle';
}

/** Associative perpendicular distance from a point feature to a straight edge. */
export function pointLineDimensionGeometry(
  point: ResolvedDrawingAnchor,
  line: ResolvedDrawingLine,
  position: [number, number],
  viewScale: number,
): DrawingDimensionGeometry | null {
  if (!Number.isFinite(viewScale) || viewScale <= 0) return null;
  const vector = subtract(line.end, line.start);
  const length = magnitude(vector);
  if (length < 1e-7) return null;
  const direction = scale(vector, 1 / length);
  const foot = add(line.start, scale(direction, dot2(subtract(point.paper, line.start), direction)));
  const measured = subtract(foot, point.paper);
  const distance = magnitude(measured);
  if (distance < 1e-7) return null;

  // The measured span is always normal to the edge. Pointer movement only
  // chooses a station along the edge, matching conventional CAD placement.
  const station = dot2(subtract(position, point.paper), direction);
  const dimensionStart = add(point.paper, scale(direction, station));
  const dimensionEnd = add(foot, scale(direction, station));
  return dimensionGeometry(
    point.paper,
    foot,
    dimensionStart,
    dimensionEnd,
    distance / viewScale,
  );
}

/**
 * Geometry for the context-aware straight-edge dimension command.
 *
 * `position` is deliberately treated as a placement intent rather than raw
 * unconstrained geometry: length dimensions only consume its component normal
 * to the selected edge, parallel-edge distance only consumes its component
 * along the edges, and angular dimensions use it to choose the sector/radius.
 */
export function lineDimensionGeometry(
  first: ResolvedDrawingLine,
  second: ResolvedDrawingLine | null,
  mode: DrawingLineDimensionMode,
  position: [number, number],
  viewScale: number,
): DrawingLineDimensionGeometry | null {
  if (!Number.isFinite(viewScale) || viewScale <= 0) return null;
  const firstVector = subtract(first.end, first.start);
  const firstLength = magnitude(firstVector);
  if (firstLength < 1e-7) return null;
  const firstDirection = scale(firstVector, 1 / firstLength);
  const firstMidpoint = midpoint(first.start, first.end);

  if (mode === 'length') {
    const normal: [number, number] = [-firstDirection[1], firstDirection[0]];
    const offset = dot2(subtract(position, firstMidpoint), normal);
    const dimensionStart = add(first.start, scale(normal, offset));
    const dimensionEnd = add(first.end, scale(normal, offset));
    return {
      kind: 'linear',
      geometry: dimensionGeometry(
        first.start,
        first.end,
        dimensionStart,
        dimensionEnd,
        firstLength / viewScale,
      ),
    };
  }

  if (!second) return null;
  const secondVector = subtract(second.end, second.start);
  const secondLength = magnitude(secondVector);
  if (secondLength < 1e-7) return null;
  const secondDirection = scale(secondVector, 1 / secondLength);

  if (mode === 'distance') {
    const cross = Math.abs(cross2(firstDirection, secondDirection));
    if (cross > Math.sin(Math.PI / 180)) return null;
    const normal: [number, number] = [-firstDirection[1], firstDirection[0]];
    const secondMidpoint = midpoint(second.start, second.end);
    const separation = dot2(subtract(secondMidpoint, firstMidpoint), normal);
    if (Math.abs(separation) < 1e-7) return null;
    // Sliding the pointer parallel to the source edges changes only the
    // station of the perpendicular dimension line.
    const station = dot2(subtract(position, firstMidpoint), firstDirection);
    const dimensionStart = add(firstMidpoint, scale(firstDirection, station));
    const dimensionEnd = add(dimensionStart, scale(normal, separation));
    // A parallel-line dimension is placed on the infinite extensions of the
    // selected edges, but its witness lines still need to begin on the finite
    // selected features. Using the dimension endpoints as both source and
    // destination collapses those witness lines to zero length whenever the
    // value is placed beyond the feature ends.
    const firstSource = closestPointOnSegment(dimensionStart, first.start, first.end);
    const secondSource = closestPointOnSegment(dimensionEnd, second.start, second.end);
    return {
      kind: 'linear',
      geometry: dimensionGeometry(
        firstSource,
        secondSource,
        dimensionStart,
        dimensionEnd,
        Math.abs(separation) / viewScale,
      ),
    };
  }

  const denominator = cross2(firstDirection, secondDirection);
  if (Math.abs(denominator) <= Math.sin(Math.PI / 180)) return null;
  const intersectionOffset = subtract(second.start, first.start);
  const firstParameter = cross2(intersectionOffset, secondDirection) / denominator;
  const vertex = add(first.start, scale(firstDirection, firstParameter));
  const towardPointer = subtract(position, vertex);
  let firstRay = dot2(towardPointer, firstDirection) < 0
    ? scale(firstDirection, -1)
    : firstDirection;
  let secondRay = dot2(towardPointer, secondDirection) < 0
    ? scale(secondDirection, -1)
    : secondDirection;
  // Prefer the included angle nearest the cursor. If the two independently
  // selected ray signs oppose the cursor sector, flip the weaker one.
  if (dot2(firstRay, secondRay) < -0.999999) return null;
  let angle = Math.acos(clamp(dot2(firstRay, secondRay), -1, 1));
  if (angle > Math.PI - 1e-7) {
    secondRay = scale(secondRay, -1);
    angle = Math.acos(clamp(dot2(firstRay, secondRay), -1, 1));
  }
  if (angle < 1e-7) return null;
  const radius = Math.max(4, magnitude(towardPointer));
  const cross = cross2(firstRay, secondRay);
  const arcStart = add(vertex, scale(firstRay, radius));
  const arcEnd = add(vertex, scale(secondRay, radius));
  const bisector = add(firstRay, secondRay);
  const bisectorLength = magnitude(bisector);
  const textDirection = bisectorLength < 1e-7
    ? [-firstRay[1], firstRay[0]] as [number, number]
    : scale(bisector, 1 / bisectorLength);
  return {
    kind: 'angular',
    geometry: {
      vertex,
      firstRay: add(vertex, scale(firstRay, radius + 3)),
      secondRay: add(vertex, scale(secondRay, radius + 3)),
      arcStart,
      arcEnd,
      arcPath: `M${pointText(arcStart)} A${round(radius)},${round(radius)} 0 0 ${cross >= 0 ? 1 : 0} ${pointText(arcEnd)}`,
      textPosition: add(vertex, scale(textDirection, radius + 4)),
      value: angle * 180 / Math.PI,
    },
  };
}

function dimensionGeometry(
  first: [number, number],
  second: [number, number],
  dimensionStart: [number, number],
  dimensionEnd: [number, number],
  value: number,
): DrawingDimensionGeometry {
  const dimensionVector = subtract(dimensionEnd, dimensionStart);
  const dimensionLength = magnitude(dimensionVector);
  const direction = dimensionLength < 1e-7 ? [1, 0] as [number, number] : scale(dimensionVector, 1 / dimensionLength);
  let textAngle = Math.atan2(direction[1], direction[0]) * 180 / Math.PI;
  if (textAngle > 90 || textAngle < -90) textAngle += 180;
  return {
    first,
    second,
    dimensionStart,
    dimensionEnd,
    firstExtension: extensionLine(first, dimensionStart),
    secondExtension: extensionLine(second, dimensionEnd),
    textPosition: midpoint(dimensionStart, dimensionEnd),
    textAngle,
    arrowSize: Math.min(2.5, Math.max(1.4, dimensionLength * 0.12)),
    value,
  };
}

export function ordinateDimensionGeometry(
  origin: ResolvedDrawingAnchor,
  target: ResolvedDrawingAnchor,
  offset: number,
  viewScale: number,
): DrawingOrdinateGeometry | null {
  if (!Number.isFinite(offset) || viewScale <= 0) return null;
  const delta = subtract(target.paper, origin.paper);
  if (magnitude(delta) < 1e-7) return null;
  const horizontal = Math.abs(delta[0]) >= Math.abs(delta[1]);
  const elbow: [number, number] = horizontal
    ? [target.paper[0], target.paper[1] + offset]
    : [target.paper[0] + offset, target.paper[1]];
  const textPosition: [number, number] = horizontal
    ? [elbow[0], elbow[1] + Math.sign(offset || 1) * 2]
    : [elbow[0] + Math.sign(offset || 1) * 2, elbow[1]];
  return {
    origin: origin.paper,
    target: target.paper,
    elbow,
    textPosition,
    xValue: delta[0] / viewScale,
    yValue: -delta[1] / viewScale,
  };
}

export function arcLengthDimensionGeometry(
  circle: ResolvedDrawingCircle,
  first: ResolvedDrawingAnchor,
  second: ResolvedDrawingAnchor,
  offset: number,
): DrawingArcLengthGeometry | null {
  const firstVector = subtract(first.paper, circle.center);
  const secondVector = subtract(second.paper, circle.center);
  if (magnitude(firstVector) < 1e-7 || magnitude(secondVector) < 1e-7) return null;
  let startAngle = Math.atan2(firstVector[1], firstVector[0]);
  let endAngle = Math.atan2(secondVector[1], secondVector[0]);
  let sweep = endAngle - startAngle;
  while (sweep <= -Math.PI) sweep += Math.PI * 2;
  while (sweep > Math.PI) sweep -= Math.PI * 2;
  if (Math.abs(sweep) < 1e-7) return null;
  const radius = circle.paperRadius + Math.max(1, offset);
  const start: [number, number] = [circle.center[0] + Math.cos(startAngle) * radius, circle.center[1] + Math.sin(startAngle) * radius];
  const end: [number, number] = [circle.center[0] + Math.cos(endAngle) * radius, circle.center[1] + Math.sin(endAngle) * radius];
  const midAngle = startAngle + sweep / 2;
  const textPosition: [number, number] = [circle.center[0] + Math.cos(midAngle) * (radius + 3), circle.center[1] + Math.sin(midAngle) * (radius + 3)];
  const sweepFlag = sweep > 0 ? 1 : 0;
  const path = `M${pointText(start)} A${round(radius)},${round(radius)} 0 0 ${sweepFlag} ${pointText(end)}`;
  return {
    center: circle.center,
    radius,
    start,
    end,
    textPosition,
    path,
    value: circle.circle.radius * Math.abs(sweep),
  };
}

export function drawingDimensionText(
  value: number,
  precision: number,
  prefix: string,
  suffix: string,
  units: UnitSystem = 'mm',
  presentation?: DrawingDimensionPresentationDto,
  _standard: DrawingStandard = 'iso',
): string {
  const converted = units === 'cm' ? value / 10 : units === 'in' ? value / 25.4 : value;
  const rounded = Math.abs(converted) < 0.5 * 10 ** -precision ? 0 : converted;
  const unitLabel = units;
  let nominal = `${rounded.toFixed(precision)} ${unitLabel}`;
  if (presentation) {
    const convertDelta = (delta: number) => units === 'cm' ? delta / 10 : units === 'in' ? delta / 25.4 : delta;
    const upper = convertDelta(presentation.tolerance.upper);
    const lower = convertDelta(presentation.tolerance.lower);
    switch (presentation.tolerance.mode) {
      case 'symmetric': nominal += ` ±${Math.abs(upper).toFixed(precision)}`; break;
      case 'deviation': nominal += ` +${Math.abs(upper).toFixed(precision)}/-${Math.abs(lower).toFixed(precision)}`; break;
      case 'limits': {
        const high = converted + upper;
        const low = converted + lower;
        nominal = `${high.toFixed(precision)} / ${low.toFixed(precision)} ${unitLabel}`;
        break;
      }
      case 'none': break;
    }
    if (presentation.fit_class.trim()) nominal += ` ${presentation.fit_class.trim()}`;
    if (presentation.dual_units) {
      const dual = presentation.dual_units;
      const dualValue = dual.unit === 'millimetre' ? value : dual.unit === 'centimetre' ? value / 10 : value / 25.4;
      const dualUnit = dual.unit === 'millimetre' ? 'mm' : dual.unit === 'centimetre' ? 'cm' : 'in';
      const formatted = `${dualValue.toFixed(dual.precision)} ${dualUnit}`;
      nominal += dual.placement === 'stacked' ? ` / ${formatted}` : ` [${formatted}]`;
    }
    if (presentation.basic) nominal = `[${nominal}]`;
    if (presentation.reference) nominal = `(${nominal})`;
  }
  return `${prefix}${nominal}${suffix}`;
}

/**
 * Approximate the advance of drawing text in em units. Keeping this estimate
 * shared between the live SVG and exported SVG prevents their masks and
 * narrow-dimension decisions from drifting apart.
 */
export function drawingTextAdvance(text: string): number {
  return Array.from(text).reduce((total, character) => {
    if (character === ' ') return total + 0.34;
    if (/[1ilI.,:;'|]/.test(character)) return total + 0.32;
    if (/[MW@%]/.test(character)) return total + 0.86;
    return total + 0.58;
  }, 0);
}

export function drawingDimensionTextWidth(text: string, textHeight: number): number {
  return Math.max(textHeight * 1.8, drawingTextAdvance(text) * textHeight + 2.2);
}

/**
 * Standards-aware presentation for an ordinary linear dimension.
 *
 * ISO 129-1 keeps the value above a continuous dimension line. ASME permits
 * the familiar centered value with the line interrupted around it. Both move
 * arrowheads outside the extension lines when the value and arrowheads do not
 * fit between them; the value then follows outside one terminator rather than
 * covering the arrows.
 */
export function drawingLinearDimensionLayout(
  geometry: DrawingDimensionGeometry,
  text: string,
  textHeight: number,
  arrowSize: number,
  standard: DrawingStandard,
): DrawingLinearDimensionLayout {
  const spanVector = subtract(geometry.dimensionEnd, geometry.dimensionStart);
  const span = magnitude(spanVector);
  const direction = span < 1e-7
    ? [1, 0] as [number, number]
    : scale(spanVector, 1 / span);
  const textWidth = drawingDimensionTextWidth(text, textHeight);
  const clearance = Math.max(0.8, arrowSize * 0.4);
  const arrowsOutside = span < textWidth + arrowSize * 2 + clearance * 2;
  const textOutside = span < textWidth + clearance * 2;

  if (!arrowsOutside) {
    return {
      lineStart: geometry.dimensionStart,
      lineEnd: geometry.dimensionEnd,
      firstArrowToward: geometry.dimensionEnd,
      secondArrowToward: geometry.dimensionStart,
      textPosition: geometry.textPosition,
      textWidth,
      arrowsOutside: false,
      textOutside: false,
      maskDimensionLine: standard === 'ansi',
    };
  }

  const arrowExtension = arrowSize + clearance;
  if (!textOutside) {
    return {
      lineStart: add(geometry.dimensionStart, scale(direction, -arrowExtension)),
      lineEnd: add(geometry.dimensionEnd, scale(direction, arrowExtension)),
      firstArrowToward: add(geometry.dimensionStart, scale(direction, -arrowSize)),
      secondArrowToward: add(geometry.dimensionEnd, scale(direction, arrowSize)),
      textPosition: geometry.textPosition,
      textWidth,
      arrowsOutside: true,
      textOutside: false,
      maskDimensionLine: standard === 'ansi',
    };
  }

  const valueGap = Math.max(1, arrowSize * 0.55);
  const textOffset = arrowSize + valueGap + textWidth / 2;
  const lineTail = textOffset + textWidth / 2 + clearance;
  return {
    lineStart: add(geometry.dimensionStart, scale(direction, -arrowExtension)),
    lineEnd: add(geometry.dimensionEnd, scale(direction, lineTail)),
    firstArrowToward: add(geometry.dimensionStart, scale(direction, -arrowSize)),
    secondArrowToward: add(geometry.dimensionEnd, scale(direction, arrowSize)),
    textPosition: add(geometry.dimensionEnd, scale(direction, textOffset)),
    textWidth,
    arrowsOutside: true,
    textOutside: true,
    // With the value outside, both standards can keep the line intact.
    maskDimensionLine: false,
  };
}

export function drawingAngularDimensionText(
  value: number,
  precision: number,
  prefix: string,
  suffix: string,
  presentation: DrawingDimensionPresentationDto,
  _standard: DrawingStandard,
): string {
  let nominal = `${value.toFixed(precision)}°`;
  const tolerance = presentation.tolerance;
  if (tolerance.mode === 'symmetric') nominal += ` ±${Math.abs(tolerance.upper).toFixed(precision)}°`;
  if (tolerance.mode === 'deviation') nominal += ` +${Math.abs(tolerance.upper).toFixed(precision)}°/-${Math.abs(tolerance.lower).toFixed(precision)}°`;
  if (tolerance.mode === 'limits') nominal = `${(value + tolerance.upper).toFixed(precision)}° / ${(value + tolerance.lower).toFixed(precision)}°`;
  if (presentation.fit_class.trim()) nominal += ` ${presentation.fit_class.trim()}`;
  if (presentation.basic) nominal = `[${nominal}]`;
  if (presentation.reference) nominal = `(${nominal})`;
  return `${prefix}${nominal}${suffix}`;
}

/** Explicit distance/angle form accepted by both ISO and ASME practice. */
export function drawingChamferText(
  lengthMm: number,
  angleDeg: number,
  prefix: string,
  standard: DrawingStandard,
  units: UnitSystem = 'mm',
): string {
  const converted = units === 'cm' ? lengthMm / 10 : units === 'in' ? lengthMm / 25.4 : lengthMm;
  const precision = units === 'in' ? 3 : 2;
  let distance = trimFixed(converted, precision);
  if (standard === 'ansi' && units === 'in') {
    distance = distance.replace(/^(-?)0\./, '$1.');
  }
  const angle = trimFixed(angleDeg, 2);
  return `${prefix}${distance}${standard === 'iso' ? ' × ' : ' X '}${angle}°`;
}

export function drawingHoleCalloutText(
  annotation: Extract<DrawingAnnotationDto, { kind: 'hole_note' }>,
  standard: DrawingStandard,
  units: UnitSystem = 'mm',
): string {
  const convert = (value: number) => units === 'cm' ? value / 10 : units === 'in' ? value / 25.4 : value;
  const precision = units === 'in' ? 3 : 2;
  const length = (value: number) => {
    let text = trimFixed(convert(value), precision);
    if (standard === 'ansi' && units === 'in') text = text.replace(/^(-?)0\./, '$1.');
    return text;
  };
  const multiplier = standard === 'iso' ? '×' : 'X';
  const lines: string[] = [];
  const quantity = annotation.quantity > 1 ? `${annotation.quantity}${multiplier} ` : '';
  const primary = annotation.thread.trim()
    ? `${quantity}${annotation.thread.trim()}${annotation.thread_depth !== null ? ` ↧${length(annotation.thread_depth)}` : annotation.depth === null ? ' THRU' : ''}`
    : `${quantity}⌀${length(annotation.diameter)}${annotation.depth !== null ? ` ↧${length(annotation.depth)}` : ' THRU'}`;
  lines.push(primary);
  if (annotation.hole_style === 'counterbore' && annotation.counterbore_diameter !== null) {
    lines.push(`⌴ ⌀${length(annotation.counterbore_diameter)}${annotation.counterbore_depth !== null ? ` ↧${length(annotation.counterbore_depth)}` : ''}`);
  }
  if (annotation.hole_style === 'countersink' && annotation.countersink_diameter !== null) {
    lines.push(`⌵ ⌀${length(annotation.countersink_diameter)}${annotation.countersink_angle_deg !== null ? ` ${multiplier} ${trimFixed(annotation.countersink_angle_deg, 2)}°` : ''}`);
  }
  if (annotation.pattern_note.trim() && annotation.pattern_note.trim() !== `${annotation.quantity} HOLES`) lines.push(annotation.pattern_note.trim());
  if (annotation.note.trim() && annotation.note.trim().toUpperCase() !== 'THRU') lines.push(annotation.note.trim());
  return lines.join('\n');
}

export function arrowPolygon(
  tip: [number, number],
  toward: [number, number],
  size: number,
): string {
  const vector = subtract(toward, tip);
  const length = magnitude(vector);
  if (length < 1e-7) return '';
  const direction = scale(vector, 1 / length);
  const normal: [number, number] = [-direction[1], direction[0]];
  const base = add(tip, scale(direction, size));
  const halfWidth = size * 0.38;
  const left = add(base, scale(normal, halfWidth));
  const right = add(base, scale(normal, -halfWidth));
  return `${pointText(tip)} ${pointText(left)} ${pointText(right)}`;
}

function extensionLine(
  anchor: [number, number],
  dimensionPoint: [number, number],
): [[number, number], [number, number]] {
  const vector = subtract(dimensionPoint, anchor);
  const length = magnitude(vector);
  if (length < 1e-7) return [anchor, dimensionPoint];
  const direction = scale(vector, 1 / length);
  return [
    add(anchor, scale(direction, Math.min(1, length * 0.2))),
    add(dimensionPoint, scale(direction, 1.2)),
  ];
}

function closestPointOnSegment(
  point: [number, number],
  start: [number, number],
  end: [number, number],
): [number, number] {
  const segment = subtract(end, start);
  const lengthSquared = dot2(segment, segment);
  if (lengthSquared < 1e-12) return start;
  const parameter = clamp(dot2(subtract(point, start), segment) / lengthSquared, 0, 1);
  return add(start, scale(segment, parameter));
}

function pointText(point: [number, number]): string {
  return `${round(point[0])},${round(point[1])}`;
}

function round(value: number): number {
  return Number(value.toFixed(5));
}

function trimFixed(value: number, precision: number): string {
  return value.toFixed(precision).replace(/\.0+$/, '').replace(/(\.\d*?)0+$/, '$1');
}

function add(left: [number, number], right: [number, number]): [number, number] {
  return [left[0] + right[0], left[1] + right[1]];
}

function subtract(left: [number, number], right: [number, number]): [number, number] {
  return [left[0] - right[0], left[1] - right[1]];
}

function scale(vector: [number, number], factor: number): [number, number] {
  return [vector[0] * factor, vector[1] * factor];
}

function magnitude(vector: [number, number]): number {
  return Math.hypot(vector[0], vector[1]);
}

function dot2(left: [number, number], right: [number, number]): number {
  return left[0] * right[0] + left[1] * right[1];
}

function cross2(left: [number, number], right: [number, number]): number {
  return left[0] * right[1] - left[1] * right[0];
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value));
}

function midpoint(left: [number, number], right: [number, number]): [number, number] {
  return [(left[0] + right[0]) / 2, (left[1] + right[1]) / 2];
}
