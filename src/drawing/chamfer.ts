import type {
  DrawingProjectionDto,
  DrawingTopologyAnchorRefDto,
  DrawingViewDto,
  SolidSceneDto,
} from '../engine/types';

type Vec2 = [number, number];
type Vec3 = [number, number, number];

interface ProjectedLinearEdge {
  occurrenceId?: number | null;
  bodyId: number;
  edgeId: number;
  edgeKey: string;
  startModel: Vec3;
  endModel: Vec3;
  startProjected: Vec2;
  endProjected: Vec2;
  startPaper: Vec2;
  endPaper: Vec2;
  length: number;
  depth: number;
  hidden: boolean;
}

export interface DrawingChamferCandidate {
  key: string;
  bodyId: number;
  edgeId: number;
  edgeKey: string;
  first: DrawingTopologyAnchorRefDto;
  second: DrawingTopologyAnchorRefDto;
  paperStart: Vec2;
  paperEnd: Vec2;
  attachment: Vec2;
  /** Setback measured along the automatically selected reference edge. */
  distance: number;
  /** Acute angle between the chamfer and its selected reference edge. */
  angleDeg: number;
  referenceEdgeId: number;
  hidden: boolean;
}

/**
 * Finds straight, true-shape projected edges that connect two non-parallel,
 * non-perpendicular carrier edges. This mirrors the conventional chamfer-note
 * workflow while keeping the persisted reference on an exact stable OCCT edge.
 */
export function drawingChamferCandidates(
  scene: SolidSceneDto,
  view: DrawingViewDto,
  projection: DrawingProjectionDto,
): DrawingChamferCandidate[] {
  const direction = normalize3(view.direction);
  const right = normalize3(cross3(view.up, direction));
  const pageUp = normalize3(cross3(direction, right));
  const center: Vec2 = [
    (projection.bounds[0] + projection.bounds[2]) / 2,
    (projection.bounds[1] + projection.bounds[3]) / 2,
  ];
  const selected = view.body_ids.length === 0
    ? scene.bodies
    : scene.bodies.filter((body) => view.body_ids.includes(body.id));
  const edges: ProjectedLinearEdge[] = [];

  for (const body of selected) {
    for (const edge of body.edges) {
      const points = edge.points.map((point) => [point.x, point.y, point.z] as Vec3);
      if (!isLinear(points)) continue;
      for (const startAnchor of projection.anchors.filter((anchor) => anchor.body_id === body.id && anchor.edge_id === edge.id && anchor.edge_key === edge.key && anchor.endpoint === 'start')) {
      const endAnchor = projection.anchors.find((anchor) => anchor.body_id === body.id && anchor.edge_id === edge.id && anchor.edge_key === edge.key && anchor.endpoint === 'end' && (anchor.occurrence_id ?? null) === (startAnchor.occurrence_id ?? null));
      if (!endAnchor) continue;
      const startModel = startAnchor.model_point;
      const endModel = endAnchor.model_point;
      const delta = subtract3(endModel, startModel);
      const length = magnitude3(delta);
      if (length <= 1e-7) continue;
      // Chamfer dimensions are only unambiguous in a true-shape view.
      if (Math.abs(dot3(scale3(delta, 1 / length), direction)) > 1e-4) continue;
      const startProjected: Vec2 = [dot3(startModel, right), dot3(startModel, pageUp)];
      const endProjected: Vec2 = [dot3(endModel, right), dot3(endModel, pageUp)];
      const startPaper = projectedToPaper(startProjected, center, view);
      const endPaper = projectedToPaper(endProjected, center, view);
      if (distance2(startPaper, endPaper) < 0.75) continue;
      const hidden = projection.visible.length > 0 && !projectedSegmentTouches(
        startProjected,
        endProjected,
        projection.visible,
        Math.max(0.03, 0.12 / Math.max(view.scale, 0.01)),
      );
      edges.push({
        occurrenceId: startAnchor.occurrence_id,
        bodyId: body.id,
        edgeId: edge.id,
        edgeKey: edge.key,
        startModel,
        endModel,
        startProjected,
        endProjected,
        startPaper,
        endPaper,
        length,
        depth: dot3(midpoint3(startModel, endModel), direction),
        hidden,
      });
      }
    }
  }

  const candidates: Array<{ candidate: DrawingChamferCandidate; depth: number }> = [];
  for (const edge of edges) {
    if (edge.hidden && !view.show_hidden_lines) continue;
    const references = edges
      .filter((other) => (other.occurrenceId ?? null) === (edge.occurrenceId ?? null) && other.bodyId === edge.bodyId && other.edgeId !== edge.edgeId)
      .map((other) => chamferReference(edge, other))
      .filter((candidate): candidate is NonNullable<typeof candidate> => candidate !== null);
    if (!references.some((candidate) => candidate.chamferEndpoint === 'start')
      || !references.some((candidate) => candidate.chamferEndpoint === 'end')) continue;

    references.sort((left, rightCandidate) =>
      Number(left.reference.hidden) - Number(rightCandidate.reference.hidden)
      || rightCandidate.reference.length - left.reference.length
      || left.reference.edgeId - rightCandidate.reference.edgeId,
    );
    const reference = references[0];
    const angleRad = reference.angleDeg * Math.PI / 180;
    const distance = edge.length * Math.cos(angleRad);
    if (!Number.isFinite(distance) || distance <= 1e-7) continue;
    candidates.push({
      depth: edge.depth,
      candidate: {
        key: `${edge.occurrenceId ?? 'definition'}:${edge.bodyId}:${edge.edgeId}`,
        bodyId: edge.bodyId,
        edgeId: edge.edgeId,
        edgeKey: edge.edgeKey,
        first: anchorRef(edge, 'start'),
        second: anchorRef(edge, 'end'),
        paperStart: edge.startPaper,
        paperEnd: edge.endPaper,
        attachment: midpoint2(edge.startPaper, edge.endPaper),
        distance,
        angleDeg: reference.angleDeg,
        referenceEdgeId: reference.reference.edgeId,
        hidden: edge.hidden,
      },
    });
  }

  // Front and back chamfer edges can project to the exact same paper segment.
  // Present one deterministic target, preferring HLR-visible and then the edge
  // closest to the view direction, so a click never selects an occluded twin.
  const unique = new Map<string, { candidate: DrawingChamferCandidate; depth: number }>();
  for (const item of candidates) {
    const key = projectedSegmentKey(item.candidate.paperStart, item.candidate.paperEnd);
    const current = unique.get(key);
    if (!current
      || Number(item.candidate.hidden) < Number(current.candidate.hidden)
      || (item.candidate.hidden === current.candidate.hidden && item.depth > current.depth)
      || (item.candidate.hidden === current.candidate.hidden && item.depth === current.depth
        && item.candidate.edgeId < current.candidate.edgeId)) {
      unique.set(key, item);
    }
  }
  return [...unique.values()].map((item) => item.candidate).sort((left, rightCandidate) =>
    left.bodyId - rightCandidate.bodyId || left.edgeId - rightCandidate.edgeId,
  );
}

export function defaultChamferNotePosition(candidate: DrawingChamferCandidate): Vec2 {
  const delta: Vec2 = [
    candidate.paperEnd[0] - candidate.paperStart[0],
    candidate.paperEnd[1] - candidate.paperStart[1],
  ];
  const length = Math.hypot(delta[0], delta[1]);
  let normal: Vec2 = length > 1e-7 ? [-delta[1] / length, delta[0] / length] : [0, -1];
  if (normal[1] > 0) normal = [-normal[0], -normal[1]];
  return [
    candidate.attachment[0] + normal[0] * 14 + 5,
    candidate.attachment[1] + normal[1] * 14,
  ];
}

function chamferReference(
  chamfer: ProjectedLinearEdge,
  reference: ProjectedLinearEdge,
): {
  reference: ProjectedLinearEdge;
  chamferEndpoint: 'start' | 'end';
  angleDeg: number;
} | null {
  const tolerance = Math.max(1e-6, Math.min(chamfer.length, reference.length) * 1e-5);
  const shared = sharedEndpoint(chamfer, reference, tolerance);
  if (!shared) return null;
  const chamferOther = shared.chamferEndpoint === 'start' ? chamfer.endModel : chamfer.startModel;
  const referenceOther = shared.referenceEndpoint === 'start' ? reference.endModel : reference.startModel;
  const chamferDirection = normalize3(subtract3(chamferOther, shared.point));
  const referenceDirection = normalize3(subtract3(referenceOther, shared.point));
  const raw = Math.acos(clamp(dot3(chamferDirection, referenceDirection), -1, 1)) * 180 / Math.PI;
  const acute = Math.min(raw, 180 - raw);
  // Parallel lines do not define a chamfer angle; perpendicular lines are the
  // unmodified corner carriers, not the cut edge itself.
  if (acute < 2 || acute > 88) return null;
  return { reference, chamferEndpoint: shared.chamferEndpoint, angleDeg: acute };
}

function sharedEndpoint(
  left: ProjectedLinearEdge,
  right: ProjectedLinearEdge,
  tolerance: number,
): {
  point: Vec3;
  chamferEndpoint: 'start' | 'end';
  referenceEndpoint: 'start' | 'end';
} | null {
  for (const [chamferEndpoint, leftPoint] of [['start', left.startModel], ['end', left.endModel]] as const) {
    for (const [referenceEndpoint, rightPoint] of [['start', right.startModel], ['end', right.endModel]] as const) {
      if (distance3(leftPoint, rightPoint) <= tolerance) {
        return { point: leftPoint, chamferEndpoint, referenceEndpoint };
      }
    }
  }
  return null;
}

function anchorRef(
  edge: ProjectedLinearEdge,
  endpoint: 'start' | 'end',
): DrawingTopologyAnchorRefDto {
  return {
    occurrence_id: edge.occurrenceId,
    body_id: edge.bodyId,
    edge_id: edge.edgeId,
    edge_key: edge.edgeKey,
    endpoint,
    fallback_point: endpoint === 'start' ? edge.startModel : edge.endModel,
  };
}

function isLinear(points: Vec3[]): boolean {
  if (points.length < 2) return false;
  const start = points[0];
  const end = points[points.length - 1];
  const delta = subtract3(end, start);
  const length = magnitude3(delta);
  if (length <= 1e-7) return false;
  const direction = scale3(delta, 1 / length);
  const tolerance = Math.max(1e-5, length * 1e-5);
  return points.every((point) => {
    const offset = subtract3(point, start);
    const along = dot3(offset, direction);
    const nearest = add3(start, scale3(direction, along));
    return distance3(point, nearest) <= tolerance;
  });
}

function projectedSegmentTouches(
  start: Vec2,
  end: Vec2,
  polylines: DrawingProjectionDto['visible'],
  tolerance: number,
): boolean {
  for (const ratio of [0.12, 0.5, 0.88]) {
    const sample: Vec2 = [
      start[0] + (end[0] - start[0]) * ratio,
      start[1] + (end[1] - start[1]) * ratio,
    ];
    if (polylines.some((polyline) => polyline.points.some((point, index) =>
      index > 0 && pointSegmentDistance(sample, polyline.points[index - 1], point) <= tolerance,
    ))) return true;
  }
  return false;
}

function projectedToPaper(point: Vec2, center: Vec2, view: DrawingViewDto): Vec2 {
  return [
    view.position[0] + (point[0] - center[0]) * view.scale,
    view.position[1] - (point[1] - center[1]) * view.scale,
  ];
}

function projectedSegmentKey(start: Vec2, end: Vec2): string {
  const encode = (point: Vec2) => `${Math.round(point[0] * 100)},${Math.round(point[1] * 100)}`;
  const first = encode(start);
  const second = encode(end);
  return first < second ? `${first}|${second}` : `${second}|${first}`;
}

function pointSegmentDistance(point: Vec2, start: Vec2, end: Vec2): number {
  const delta: Vec2 = [end[0] - start[0], end[1] - start[1]];
  const lengthSq = delta[0] ** 2 + delta[1] ** 2;
  if (lengthSq <= 1e-18) return distance2(point, start);
  const ratio = clamp(
    ((point[0] - start[0]) * delta[0] + (point[1] - start[1]) * delta[1]) / lengthSq,
    0,
    1,
  );
  return distance2(point, [start[0] + delta[0] * ratio, start[1] + delta[1] * ratio]);
}

function add3(left: Vec3, right: Vec3): Vec3 { return [left[0] + right[0], left[1] + right[1], left[2] + right[2]]; }
function subtract3(left: Vec3, right: Vec3): Vec3 { return [left[0] - right[0], left[1] - right[1], left[2] - right[2]]; }
function scale3(value: Vec3, factor: number): Vec3 { return [value[0] * factor, value[1] * factor, value[2] * factor]; }
function dot3(left: Vec3, right: Vec3): number { return left[0] * right[0] + left[1] * right[1] + left[2] * right[2]; }
function cross3(left: Vec3, right: Vec3): Vec3 { return [left[1] * right[2] - left[2] * right[1], left[2] * right[0] - left[0] * right[2], left[0] * right[1] - left[1] * right[0]]; }
function magnitude3(value: Vec3): number { return Math.sqrt(dot3(value, value)); }
function normalize3(value: Vec3): Vec3 { const length = magnitude3(value); return length <= 1e-12 ? [0, 0, 0] : scale3(value, 1 / length); }
function distance3(left: Vec3, right: Vec3): number { return magnitude3(subtract3(left, right)); }
function distance2(left: Vec2, right: Vec2): number { return Math.hypot(left[0] - right[0], left[1] - right[1]); }
function midpoint2(left: Vec2, right: Vec2): Vec2 { return [(left[0] + right[0]) / 2, (left[1] + right[1]) / 2]; }
function midpoint3(left: Vec3, right: Vec3): Vec3 { return [(left[0] + right[0]) / 2, (left[1] + right[1]) / 2, (left[2] + right[2]) / 2]; }
function clamp(value: number, min: number, max: number): number { return Math.max(min, Math.min(max, value)); }
