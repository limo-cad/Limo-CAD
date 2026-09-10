import type {
  DrawingLineRefDto,
  DrawingProjectionDto,
  DrawingViewDto,
  SolidSceneDto,
} from '../engine/types';
import {
  centerLineBetweenEdgesGeometry,
  drawingLineRef,
  type ResolvedDrawingLine,
} from './annotations';

type Vec2 = [number, number];
type Vec3 = [number, number, number];

export interface DrawingCenterlineEdgeCandidate {
  key: string;
  reference: DrawingLineRefDto;
  paperStart: Vec2;
  paperEnd: Vec2;
  hidden: boolean;
}

/**
 * Exposes straight OCCT topology edges as deterministic centerline targets.
 * Coincident front/back projections are collapsed, preferring the visible
 * edge closest to the viewer.
 */
export function drawingCenterlineEdgeCandidates(
  scene: SolidSceneDto,
  view: DrawingViewDto,
  projection: DrawingProjectionDto,
): DrawingCenterlineEdgeCandidate[] {
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
  const candidates: Array<{ candidate: DrawingCenterlineEdgeCandidate; depth: number }> = [];

  for (const body of selected) {
    for (const edge of body.edges) {
      const points = edge.points.map((point) => [point.x, point.y, point.z] as Vec3);
      if (!isLinear(points)) continue;
      for (const startAnchor of projection.anchors.filter((anchor) => anchor.body_id === body.id && anchor.edge_id === edge.id && anchor.edge_key === edge.key && anchor.endpoint === 'start')) {
      const endAnchor = projection.anchors.find((anchor) => anchor.body_id === body.id && anchor.edge_id === edge.id && anchor.edge_key === edge.key && anchor.endpoint === 'end' && (anchor.occurrence_id ?? null) === (startAnchor.occurrence_id ?? null));
      if (!endAnchor) continue;
      const start = startAnchor.model_point;
      const end = endAnchor.model_point;
      const projectedStart: Vec2 = [dot3(start, right), dot3(start, pageUp)];
      const projectedEnd: Vec2 = [dot3(end, right), dot3(end, pageUp)];
      const paperStart = projectedToPaper(projectedStart, center, view);
      const paperEnd = projectedToPaper(projectedEnd, center, view);
      if (distance2(paperStart, paperEnd) < 0.75) continue;
      const hidden = projection.visible.length > 0 && !projectedSegmentTouches(
        projectedStart,
        projectedEnd,
        projection.visible,
        Math.max(0.03, 0.12 / Math.max(view.scale, 0.01)),
      );
      if (hidden && !view.show_hidden_lines) continue;
      candidates.push({
        depth: dot3(midpoint3(start, end), direction),
        candidate: {
          key: `${startAnchor.occurrence_id ?? 'definition'}:${body.id}:${edge.id}`,
          reference: drawingLineRef(body.id, edge.id, edge.key, start, end, startAnchor.occurrence_id, projection),
          paperStart,
          paperEnd,
          hidden,
        },
      });
      }
    }
  }

  const unique = new Map<string, { candidate: DrawingCenterlineEdgeCandidate; depth: number }>();
  for (const item of candidates) {
    const key = projectedSegmentKey(item.candidate.paperStart, item.candidate.paperEnd);
    const current = unique.get(key);
    if (
      !current
      || Number(item.candidate.hidden) < Number(current.candidate.hidden)
      || (item.candidate.hidden === current.candidate.hidden && item.depth > current.depth)
      || (item.candidate.hidden === current.candidate.hidden
        && Math.abs(item.depth - current.depth) <= 1e-7
        && item.candidate.reference.edge_id < current.candidate.reference.edge_id)
    ) unique.set(key, item);
  }
  return [...unique.values()].map((item) => item.candidate).sort((left, rightCandidate) =>
    left.reference.body_id - rightCandidate.reference.body_id
      || left.reference.edge_id - rightCandidate.reference.edge_id,
  );
}

export function drawingCenterlineEdgesCompatible(
  first: DrawingCenterlineEdgeCandidate,
  second: DrawingCenterlineEdgeCandidate,
): boolean {
  if (sameDrawingLineRef(first.reference, second.reference)) return false;
  return centerLineBetweenEdgesGeometry(resolved(first), resolved(second), 0) !== null;
}

export function sameDrawingLineRef(left: DrawingLineRefDto, right: DrawingLineRefDto): boolean {
  return (left.occurrence_id ?? null) === (right.occurrence_id ?? null) && left.body_id === right.body_id && left.edge_id === right.edge_id;
}

/**
 * Resolves overlapping SVG edge hit strokes by geometric proximity instead of
 * DOM paint order. Closely spaced projected edges intentionally have generous
 * hit regions; without this arbitration the later-painted edge can win even
 * while the pointer is visibly on its neighbour.
 */
export function nearestDrawingCenterlineEdgeCandidate(
  point: Vec2,
  candidates: DrawingCenterlineEdgeCandidate[],
  preferredKey?: string,
): DrawingCenterlineEdgeCandidate | null {
  let nearest: DrawingCenterlineEdgeCandidate | null = null;
  let nearestDistance = Number.POSITIVE_INFINITY;
  for (const candidate of candidates) {
    const distance = pointSegmentDistance(point, candidate.paperStart, candidate.paperEnd);
    if (
      distance < nearestDistance - 1e-7
      || (Math.abs(distance - nearestDistance) <= 1e-7 && candidate.key === preferredKey)
    ) {
      nearest = candidate;
      nearestDistance = distance;
    }
  }
  return nearest;
}

function resolved(candidate: DrawingCenterlineEdgeCandidate): ResolvedDrawingLine {
  return {
    reference: candidate.reference,
    start: candidate.paperStart,
    end: candidate.paperEnd,
    resolution: 'exact',
  };
}

function projectedToPaper(point: Vec2, center: Vec2, view: DrawingViewDto): Vec2 {
  return [
    view.position[0] + (point[0] - center[0]) * view.scale,
    view.position[1] - (point[1] - center[1]) * view.scale,
  ];
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
    if (!polylines.some((polyline) => polylineTouchesPoint(polyline.points, sample, tolerance))) {
      return false;
    }
  }
  return true;
}

function polylineTouchesPoint(points: Vec2[], point: Vec2, tolerance: number): boolean {
  for (let index = 1; index < points.length; index += 1) {
    if (pointSegmentDistance(point, points[index - 1], points[index]) <= tolerance) return true;
  }
  return false;
}

function pointSegmentDistance(point: Vec2, start: Vec2, end: Vec2): number {
  const delta: Vec2 = [end[0] - start[0], end[1] - start[1]];
  const lengthSquared = delta[0] * delta[0] + delta[1] * delta[1];
  if (lengthSquared <= 1e-16) return distance2(point, start);
  const t = Math.max(0, Math.min(1,
    ((point[0] - start[0]) * delta[0] + (point[1] - start[1]) * delta[1]) / lengthSquared,
  ));
  return distance2(point, [start[0] + delta[0] * t, start[1] + delta[1] * t]);
}

function projectedSegmentKey(start: Vec2, end: Vec2): string {
  const left = roundedPoint(start);
  const right = roundedPoint(end);
  return left < right ? `${left}|${right}` : `${right}|${left}`;
}

function roundedPoint(point: Vec2): string {
  return `${Math.round(point[0] * 1e5)},${Math.round(point[1] * 1e5)}`;
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
    return distance3(point, add3(start, scale3(direction, along))) <= tolerance;
  });
}

function normalize3(vector: Vec3): Vec3 {
  const length = magnitude3(vector);
  return length <= 1e-12 ? [0, 0, 0] : scale3(vector, 1 / length);
}

function midpoint3(left: Vec3, right: Vec3): Vec3 {
  return [(left[0] + right[0]) / 2, (left[1] + right[1]) / 2, (left[2] + right[2]) / 2];
}

function add3(left: Vec3, right: Vec3): Vec3 {
  return [left[0] + right[0], left[1] + right[1], left[2] + right[2]];
}

function subtract3(left: Vec3, right: Vec3): Vec3 {
  return [left[0] - right[0], left[1] - right[1], left[2] - right[2]];
}

function scale3(vector: Vec3, factor: number): Vec3 {
  return [vector[0] * factor, vector[1] * factor, vector[2] * factor];
}

function cross3(left: Vec3, right: Vec3): Vec3 {
  return [
    left[1] * right[2] - left[2] * right[1],
    left[2] * right[0] - left[0] * right[2],
    left[0] * right[1] - left[1] * right[0],
  ];
}

function dot3(left: Vec3, right: Vec3): number {
  return left[0] * right[0] + left[1] * right[1] + left[2] * right[2];
}

function magnitude3(vector: Vec3): number {
  return Math.hypot(vector[0], vector[1], vector[2]);
}

function distance3(left: Vec3, right: Vec3): number {
  return magnitude3(subtract3(left, right));
}

function distance2(left: Vec2, right: Vec2): number {
  return Math.hypot(left[0] - right[0], left[1] - right[1]);
}
