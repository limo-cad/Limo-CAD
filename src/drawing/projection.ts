import { Quaternion, Vector3 } from '../components/viewport/cadInteraction';
import type {
  BodyDto,
  AssemblyDocumentDto,
  AssemblySolutionDto,
  DrawingPolylineDto,
  DrawingProjectionAnchorDto,
  DrawingProjectionDto,
  DrawingProjectionRequest,
  DrawingProjectedCircleDto,
  DrawingTopologyAnchorRefDto,
  DrawingViewDto,
  Point3Dto,
  SolidSceneDto,
} from '../engine/types';

type DrawingInstanceBody = BodyDto & { drawingOccurrenceId?: number };

/** Presentation-only transform of Rust-solved poses. Retained scene data is unchanged. */
export function drawingInstanceScene(
  scene: SolidSceneDto,
  solution: AssemblySolutionDto,
  assembly?: AssemblyDocumentDto,
  occurrenceIds: number[] = [],
  includeMesh = true,
): SolidSceneDto {
  if (!solution.solved) throw new Error('Resolve assembly diagnostics before projecting its occurrences.');
  const selected = new Set(occurrenceIds);
  if (assembly) {
    for (const id of occurrenceIds) {
      if (!assembly.component_structure.occurrences.some((node) => node.id === id)) throw new Error('Drawing occurrence is missing.');
    }
    let before: number;
    do {
      before = selected.size;
      for (const node of assembly.component_structure.occurrences) {
        if (node.parent_occurrence_id != null && selected.has(node.parent_occurrence_id)) selected.add(node.id);
      }
    } while (selected.size !== before);
  }
  const bodies = solution.instance_body_poses.filter((pose) => pose.visible && (selected.size === 0 || selected.has(pose.occurrence_id))).map((pose): DrawingInstanceBody => {
    const body = scene.bodies.find((value) => value.id === pose.body_id);
    if (!body) throw new Error('Drawing occurrence source body is missing.');
    const q = new Quaternion(...pose.rotation).normalize();
    const t = new Vector3(...pose.translation);
    const point = (p: Point3Dto): Point3Dto => {
      const value = new Vector3(p.x, p.y, p.z).applyQuaternion(q).add(t);
      return { x:value.x, y:value.y, z:value.z };
    };
    const positions: number[] = includeMesh ? [] : body.mesh.positions;
    for (let i=0; includeMesh && i<body.mesh.positions.length; i+=3) {
      const p = point({x:body.mesh.positions[i],y:body.mesh.positions[i+1],z:body.mesh.positions[i+2]});
      positions.push(p.x,p.y,p.z);
    }
    return { ...body, drawingOccurrenceId:pose.occurrence_id, mesh:{...body.mesh,positions}, edges:body.edges.map((edge) => ({...edge,points:edge.points.map(point)})) };
  });
  return {...scene,bodies};
}

type Vec3 = [number, number, number];
type ProjectedPoint = [number, number, number];

interface ProjectedTriangle {
  points: [ProjectedPoint, ProjectedPoint, ProjectedPoint];
  bounds: [number, number, number, number];
}

interface ProjectionBasis {
  right: Vec3;
  up: Vec3;
  towardViewer: Vec3;
}

interface SectionSlab {
  point: Vec3;
  normal: Vec3;
  depth: number | null;
}

interface DrawingViewBasis {
  direction: Vec3;
  up: Vec3;
}

/**
 * Builds one projection request from persisted view intent and the current
 * model topology. Derived views resolve their exact OCCT references here so a
 * recompute does not silently keep using diagnostic fallback coordinates.
 */
export function drawingProjectionRequestForView(
  view: DrawingViewDto,
  views: DrawingViewDto[],
  scene: SolidSceneDto,
  solution?: AssemblySolutionDto,
): DrawingProjectionRequest {
  if (view.scope === 'assembly' && view.derivation && solution?.solved) {
    scene = drawingInstanceScene(scene, solution, undefined, [], false);
  }
  const basis = currentDrawingViewBasis(view, views, scene, new Set());
  const derivation = view.derivation;
  const sectionPlane = derivation?.type === 'section' || derivation?.type === 'removed_section'
    ? {
      point: resolveModelAnchorPoint(derivation.first, scene) ?? derivation.first.fallback_point,
      normal: basis.direction,
      depth: derivation.type === 'section' ? derivation.depth : null,
    }
    : null;
  return {
    scope: view.scope ?? 'definition',
    occurrence_ids: view.occurrence_ids ?? [],
    body_ids: view.body_ids,
    direction: basis.direction,
    up: basis.up,
    include_hidden: view.show_hidden_lines,
    include_tangent_edges: view.show_tangent_edges,
    deflection: Math.max(0.01, 0.08 / view.scale),
    section_plane: sectionPlane,
  };
}

function currentDrawingViewBasis(
  view: DrawingViewDto,
  views: DrawingViewDto[],
  scene: SolidSceneDto,
  visited: Set<number>,
): DrawingViewBasis {
  if (visited.has(view.id)) return { direction: view.direction, up: view.up };
  visited.add(view.id);
  const derivation = view.derivation;
  if (!derivation) return { direction: view.direction, up: view.up };
  const parent = views.find((candidate) => candidate.id === derivation.parent_view_id);
  if (!parent) return { direction: view.direction, up: view.up };
  const parentBasis = currentDrawingViewBasis(parent, views, scene, visited);
  if (derivation.type === 'detail' || derivation.type === 'broken') return parentBasis;

  const line = derivation.type === 'auxiliary'
    ? resolveModelLine(
      derivation.reference.body_id,
      derivation.reference.edge_id,
      derivation.reference.edge_key,
      scene,
      derivation.reference.occurrence_id,
      derivation.reference.topology_signature,
    ) ?? [derivation.reference.fallback_start, derivation.reference.fallback_end]
    : [
      resolveModelAnchorPoint(derivation.first, scene) ?? derivation.first.fallback_point,
      resolveModelAnchorPoint(derivation.second, scene) ?? derivation.second.fallback_point,
    ];
  const edge = normalizeOrNull(subtractTuple(line[1], line[0]));
  const parentDirection = normalizeOrNull(parentBasis.direction);
  if (!edge || !parentDirection) return { direction: view.direction, up: view.up };
  let direction = normalizeOrNull(cross(edge, parentDirection));
  if (!direction) return { direction: view.direction, up: view.up };

  if (derivation.type === 'auxiliary') {
    if (derivation.flipped) direction = scale3(direction, -1);
  } else if (dot(direction, view.direction) < 0) {
    // Section arrow direction is persisted in the child view. Reversing the
    // model edge's internal orientation must not turn the section around.
    direction = scale3(direction, -1);
  }
  let up = normalizeOrNull(cross(direction, edge)) ?? view.up;
  if (derivation.type !== 'auxiliary' && dot(up, view.up) < 0) up = scale3(up, -1);
  return { direction, up };
}

function resolveModelAnchorPoint(
  reference: DrawingTopologyAnchorRefDto,
  scene: SolidSceneDto,
): Vec3 | null {
  const line = resolveModelLine(reference.body_id, reference.edge_id, reference.edge_key, scene, reference.occurrence_id, reference.topology_signature);
  if (!line) return null;
  if (!reference.circle_center) {
    return reference.endpoint === 'start' ? line[0] : line[1];
  }
  const body = scene.bodies.find((candidate) => candidate.id === reference.body_id && ((candidate as DrawingInstanceBody).drawingOccurrenceId ?? null) === (reference.occurrence_id ?? null));
  const edge = body?.edges.find((candidate) => candidate.id === reference.edge_id)
    ?? body?.edges.find((candidate) => candidate.key === reference.edge_key);
  return edge?.circle ? pointTuple(edge.circle.center) : edge ? fitCircleCenter(edge.points.map(pointTuple)) : null;
}

function resolveModelLine(
  bodyId: number,
  edgeId: number,
  edgeKey: string,
  scene: SolidSceneDto,
  occurrenceId?: number | null,
  topologySignature?: string | null,
): [Vec3, Vec3] | null {
  const body = scene.bodies.find((candidate) => candidate.id === bodyId && ((candidate as DrawingInstanceBody).drawingOccurrenceId ?? null) === (occurrenceId ?? null));
  const currentSignature = body?.topology_signature ? `feature:${body.feature_id}:${body.topology_signature}` : null;
  if ((topologySignature ?? null) !== currentSignature) {
    throw new Error('Drawing reference is unverified or its topology changed; explicitly reassociate the derived view.');
  }
  const edge = body?.edges.find((candidate) => candidate.id === edgeId)
    ?? body?.edges.find((candidate) => candidate.key === edgeKey);
  const first = edge?.points[0];
  const last = edge?.points[edge.points.length - 1];
  return first && last ? [pointTuple(first), pointTuple(last)] : null;
}

function fitCircleCenter(points: Vec3[]): Vec3 | null {
  if (points.length < 3) return null;
  const a = points[0];
  const b = points[Math.floor(points.length / 3)];
  const c = points[Math.floor(points.length * 2 / 3)];
  const ab = subtractTuple(b, a);
  const ac = subtractTuple(c, a);
  const normal = cross(ab, ac);
  const normalSquared = dot(normal, normal);
  if (normalSquared < 1e-16) return null;
  const firstTerm = scale3(cross(ac, normal), dot(ab, ab));
  const secondTerm = scale3(cross(normal, ab), dot(ac, ac));
  return add3(a, scale3(add3(firstTerm, secondTerm), 1 / (2 * normalSquared)));
}

function pointTuple(point: Point3Dto): Vec3 {
  return [point.x, point.y, point.z];
}

function subtractTuple(left: Vec3, right: Vec3): Vec3 {
  return [left[0] - right[0], left[1] - right[1], left[2] - right[2]];
}

function normalizeOrNull(value: Vec3): Vec3 | null {
  const length = Math.hypot(...value);
  return Number.isFinite(length) && length >= 1e-9 ? scale3(value, 1 / length) : null;
}

/**
 * Browser development fallback for the native OCCT HLR path.
 *
 * It projects exact tessellated topology edges, derives smooth silhouettes
 * from triangle adjacency, and depth-tests short curve spans against the mesh.
 * Desktop production uses OCCT's B-rep HLR, but both paths share coordinates
 * and response shapes so the DOM/SVG drawing UI can be developed in a browser.
 */
export function projectSceneForDrawing(
  scene: SolidSceneDto,
  request: DrawingProjectionRequest,
): DrawingProjectionDto {
  const basis = projectionBasis(request.direction, request.up);
  const selected = request.body_ids.length === 0
    ? scene.bodies
    : scene.bodies.filter((body) => request.body_ids.includes(body.id));
  const sectionSlab = request.section_plane
    ? normalizeSectionSlab(
      request.section_plane.point,
      request.section_plane.normal,
      request.section_plane.depth ?? null,
    )
    : null;
  const triangles = selected.flatMap((body) => projectedTriangles(body, basis, sectionSlab));
  const candidates: Point3Dto[][] = [];

  for (const body of selected) {
    for (const edge of body.edges) {
      if (edge.points.length >= 2 && (request.include_tangent_edges || edge.refinable)) {
        candidates.push(...clipWorldPolyline(edge.points, sectionSlab));
      }
    }
    candidates.push(
      ...silhouetteSegments(body, basis)
        .flatMap((segment) => clipWorldPolyline(segment, sectionSlab)),
    );
  }

  const visible: DrawingPolylineDto[] = [];
  const hidden: DrawingPolylineDto[] = [];
  const seen = new Set<string>();
  for (const candidate of candidates) {
    const projected = candidate.map((point) => projectPoint(point, basis));
    for (const segment of classifyPolyline(projected, triangles)) {
      const points = segment.points.map(([x, y]) => [x, y] as [number, number]);
      const key = polylineKey(points);
      if (seen.has(key)) continue;
      seen.add(key);
      if (segment.hidden) {
        if (request.include_hidden) hidden.push({ points });
      } else {
        visible.push({ points });
      }
    }
  }

  const section = sectionSlab
    ? tessellatedSection(selected, sectionSlab.point, sectionSlab.normal, basis)
    : [];
  return {
    visible,
    hidden,
    topology_signatures: Object.fromEntries(selected.filter((body) => body.topology_signature)
      .map((body) => [String(body.id), `feature:${body.feature_id}:${body.topology_signature}`])),
    anchors: projectedAnchors(selected, basis, visible, request.deflection),
    circles: projectedCircles(selected, basis, visible, request.deflection),
    section,
    bounds: projectionBounds([...visible, ...hidden, ...section]),
  };
}

function normalizeSectionSlab(
  point: Vec3,
  normalValue: Vec3,
  depth: number | null,
): SectionSlab {
  if (depth !== null && (!Number.isFinite(depth) || depth <= 0)) {
    throw new Error('Drawing section depth must be a positive finite model distance.');
  }
  return {
    point,
    normal: normalize(normalValue),
    depth,
  };
}

/**
 * Clips a model-space polyline to the material retained by a section view.
 * The cutting-plane normal points toward the removed material, so a full
 * section retains d <= 0 and a depth section retains -depth <= d <= 0.
 */
function clipWorldPolyline(points: Point3Dto[], slab: SectionSlab | null): Point3Dto[][] {
  if (!slab) return points.length >= 2 ? [points] : [];
  const output: Point3Dto[][] = [];
  let active: Point3Dto[] = [];
  for (let index = 0; index + 1 < points.length; index += 1) {
    const clipped = clipWorldSegment(points[index], points[index + 1], slab);
    if (!clipped) {
      if (active.length >= 2) output.push(active);
      active = [];
      continue;
    }
    const [start, end] = clipped;
    if (active.length === 0) {
      active = [start, end];
    } else if (sameWorldPoint(active[active.length - 1], start)) {
      if (!sameWorldPoint(active[active.length - 1], end)) active.push(end);
    } else {
      if (active.length >= 2) output.push(active);
      active = [start, end];
    }
  }
  if (active.length >= 2) output.push(active);
  return output;
}

function clipWorldSegment(
  start: Point3Dto,
  end: Point3Dto,
  slab: SectionSlab,
): [Point3Dto, Point3Dto] | null {
  const startDistance = sectionDistance(start, slab);
  const endDistance = sectionDistance(end, slab);
  let minimum = 0;
  let maximum = 1;
  const delta = endDistance - startDistance;
  const clipUpper = (limit: number): boolean => {
    // startDistance + t * delta <= limit
    if (Math.abs(delta) < 1e-12) return startDistance <= limit + 1e-9;
    const intersection = (limit - startDistance) / delta;
    if (delta > 0) maximum = Math.min(maximum, intersection);
    else minimum = Math.max(minimum, intersection);
    return minimum <= maximum + 1e-12;
  };
  const clipLower = (limit: number): boolean => {
    // startDistance + t * delta >= limit
    if (Math.abs(delta) < 1e-12) return startDistance >= limit - 1e-9;
    const intersection = (limit - startDistance) / delta;
    if (delta > 0) minimum = Math.max(minimum, intersection);
    else maximum = Math.min(maximum, intersection);
    return minimum <= maximum + 1e-12;
  };
  if (!clipUpper(0)) return null;
  if (slab.depth !== null && !clipLower(-slab.depth)) return null;
  if (minimum > 1 + 1e-12 || maximum < -1e-12) return null;
  minimum = Math.max(0, Math.min(1, minimum));
  maximum = Math.max(0, Math.min(1, maximum));
  if (maximum - minimum < 1e-10) return null;
  return [interpolateWorld(start, end, minimum), interpolateWorld(start, end, maximum)];
}

function sectionDistance(point: Point3Dto, slab: SectionSlab): number {
  return dot([point.x - slab.point[0], point.y - slab.point[1], point.z - slab.point[2]], slab.normal);
}

function interpolateWorld(start: Point3Dto, end: Point3Dto, amount: number): Point3Dto {
  return {
    x: start.x + (end.x - start.x) * amount,
    y: start.y + (end.y - start.y) * amount,
    z: start.z + (end.z - start.z) * amount,
  };
}

function sameWorldPoint(left: Point3Dto, right: Point3Dto): boolean {
  return Math.abs(left.x - right.x) < 1e-9
    && Math.abs(left.y - right.y) < 1e-9
    && Math.abs(left.z - right.z) < 1e-9;
}

function tessellatedSection(
  bodies: BodyDto[],
  planePoint: Vec3,
  planeNormal: Vec3,
  basis: ProjectionBasis,
): DrawingPolylineDto[] {
  const normalLength = Math.hypot(...planeNormal);
  if (normalLength < 1e-9) return [];
  const normal = scale3(planeNormal, 1 / normalLength);
  const result: DrawingPolylineDto[] = [];
  const seen = new Set<string>();
  for (const body of bodies) {
    const { positions, indices } = body.mesh;
    const vertex = (index: number): Vec3 => [positions[index * 3], positions[index * 3 + 1], positions[index * 3 + 2]];
    for (let cursor = 0; cursor + 2 < indices.length; cursor += 3) {
      const triangle = [vertex(indices[cursor]), vertex(indices[cursor + 1]), vertex(indices[cursor + 2])] as const;
      const distances = triangle.map((point) => dot(subtract3(point, planePoint), normal));
      const intersections: Vec3[] = [];
      for (const [firstIndex, secondIndex] of [[0, 1], [1, 2], [2, 0]] as const) {
        const firstDistance = distances[firstIndex];
        const secondDistance = distances[secondIndex];
        const first = triangle[firstIndex];
        const second = triangle[secondIndex];
        if (Math.abs(firstDistance) < 1e-7) intersections.push(first);
        if (firstDistance * secondDistance < -1e-14) {
          const ratio = firstDistance / (firstDistance - secondDistance);
          intersections.push(add3(first, scale3(subtract3(second, first), ratio)));
        }
      }
      const unique = intersections.filter((point, index) => intersections.findIndex((candidate) => distance3(point, candidate) < 1e-7) === index);
      if (unique.length < 2) continue;
      const firstProjected = projectPoint({ x: unique[0][0], y: unique[0][1], z: unique[0][2] }, basis);
      const secondProjected = projectPoint({ x: unique[1][0], y: unique[1][1], z: unique[1][2] }, basis);
      const points: Array<[number, number]> = [[firstProjected[0], firstProjected[1]], [secondProjected[0], secondProjected[1]]];
      const key = polylineKey(points);
      if (!seen.has(key)) {
        seen.add(key);
        result.push({ points });
      }
    }
  }
  return result;
}

function projectedCircles(
  bodies: BodyDto[],
  basis: ProjectionBasis,
  visible: DrawingPolylineDto[],
  deflection: number,
): DrawingProjectedCircleDto[] {
  const candidates: Array<{ circle: DrawingProjectedCircleDto; depth: number }> = [];
  for (const body of bodies) {
    for (const edge of body.edges) {
      const points = edge.points.map((point) => [point.x, point.y, point.z] as Vec3);
      const fitted = fitCircle(points);
      if (!fitted || Math.abs(dot(fitted.normal, basis.towardViewer)) < 0.995) continue;
      const projectedCenter = projectPoint(
        { x: fitted.center[0], y: fitted.center[1], z: fitted.center[2] },
        basis,
      );
      candidates.push({
        depth: projectedCenter[2],
        circle: {
        occurrence_id: (body as DrawingInstanceBody).drawingOccurrenceId,
        body_id: body.id,
        edge_id: edge.id,
        edge_key: edge.key,
        center_model: fitted.center,
        normal_model: fitted.normal,
        center: [projectedCenter[0], projectedCenter[1]],
        radius: fitted.radius,
        closed: fitted.closed,
        hidden: !points.some((point) => {
          const projected = projectPoint({ x: point[0], y: point[1], z: point[2] }, basis);
          return pointTouchesPolylines(
            [projected[0], projected[1]],
            visible,
            Math.max(1e-4, deflection) * 2.5,
          );
        }),
        },
      });
    }
  }
  // Tessellation depth tests can classify an exact boundary as coplanar and
  // therefore miss both coincident end circles of a cylinder. When a projected
  // circle stack has no visible member, expose only its front-most member.
  // This preserves hidden back rims without making annotation picking depend
  // on a sub-pixel polyline coincidence.
  for (const candidate of candidates) {
    if (!candidate.circle.hidden) continue;
    const stack = candidates.filter((other) => sameProjectedCircle(candidate.circle, other.circle));
    if (stack.length < 2 || stack.some((other) => !other.circle.hidden)) continue;
    const frontDepth = Math.max(...stack.map((other) => other.depth));
    if (candidate.depth >= frontDepth - 1e-7) candidate.circle.hidden = false;
  }
  return candidates.map(({ circle }) => circle)
    .sort((left, right) => left.body_id - right.body_id || left.edge_id - right.edge_id);
}

function sameProjectedCircle(left: DrawingProjectedCircleDto, right: DrawingProjectedCircleDto): boolean {
  const scale = Math.max(1, left.radius, right.radius);
  return Math.hypot(left.center[0] - right.center[0], left.center[1] - right.center[1]) <= scale * 1e-5
    && Math.abs(left.radius - right.radius) <= scale * 1e-5;
}

function fitCircle(points: Vec3[]): { center: Vec3; normal: Vec3; radius: number; closed: boolean } | null {
  if (points.length < 5) return null;
  const first = points[0];
  const last = points[points.length - 1];
  const largestStep = points.slice(1).reduce(
    (largest, point, index) => Math.max(largest, distance3(point, points[index])),
    0,
  );
  const closedGuess = distance3(first, last) <= largestStep * 1.5;
  const [a, b, c] = closedGuess
    ? [first, points[Math.floor(points.length / 3)], points[Math.floor(points.length * 2 / 3)]]
    : [first, points[Math.floor(points.length / 2)], last];
  const ab = subtract3(b, a);
  const ac = subtract3(c, a);
  const normalRaw = cross(ab, ac);
  const normalSq = dot(normalRaw, normalRaw);
  if (normalSq < 1e-16) return null;
  const termA = scale3(cross(ac, normalRaw), dot(ab, ab));
  const termB = scale3(cross(normalRaw, ab), dot(ac, ac));
  const center = add3(a, scale3(add3(termA, termB), 1 / (2 * normalSq)));
  const radius = distance3(center, a);
  if (!Number.isFinite(radius) || radius <= 1e-7) return null;
  const normal = scale3(normalRaw, 1 / Math.sqrt(normalSq));
  const radialTolerance = radius * 2e-3 + 2e-5;
  const planarTolerance = radius * 1e-3 + 2e-5;
  if (points.some((point) =>
    Math.abs(distance3(point, center) - radius) > radialTolerance
      || Math.abs(dot(subtract3(point, center), normal)) > planarTolerance,
  )) return null;
  return {
    center,
    normal,
    radius,
    closed: distance3(first, last) <= radialTolerance * 2,
  };
}

function add3(left: Vec3, right: Vec3): Vec3 {
  return [left[0] + right[0], left[1] + right[1], left[2] + right[2]];
}

function subtract3(left: Vec3, right: Vec3): Vec3 {
  return [left[0] - right[0], left[1] - right[1], left[2] - right[2]];
}

function scale3(value: Vec3, factor: number): Vec3 {
  return [value[0] * factor, value[1] * factor, value[2] * factor];
}

function distance3(left: Vec3, right: Vec3): number {
  const delta = subtract3(left, right);
  return Math.sqrt(dot(delta, delta));
}

function projectedAnchors(
  bodies: BodyDto[],
  basis: ProjectionBasis,
  visible: DrawingPolylineDto[],
  deflection: number,
): DrawingProjectionAnchorDto[] {
  const anchors: DrawingProjectionAnchorDto[] = [];
  for (const body of bodies) {
    for (const edge of body.edges) {
      const endpoints = [
        ['start', edge.points[0]],
        ['end', edge.points[edge.points.length - 1]],
      ] as const;
      for (const [endpoint, model] of endpoints) {
        if (!model) continue;
        const projected = projectPoint(model, basis);
        const point: [number, number] = [projected[0], projected[1]];
        anchors.push({
          occurrence_id: (body as DrawingInstanceBody).drawingOccurrenceId,
        body_id: body.id,
          edge_id: edge.id,
          edge_key: edge.key,
          endpoint,
          model_point: [model.x, model.y, model.z],
          point,
          hidden: !pointTouchesPolylines(point, visible, Math.max(1e-4, deflection) * 2.5),
        });
      }
    }
  }
  anchors.sort((left, right) =>
    left.body_id - right.body_id ||
    left.edge_id - right.edge_id ||
    endpointOrder(left.endpoint) - endpointOrder(right.endpoint),
  );
  // Preserve every stable edge endpoint just like the native OCCT path. The
  // Drawing UI visually collapses coincident generic point markers, while
  // associative edge-specific annotations retain their exact topology refs.
  return anchors;
}

function endpointOrder(endpoint: DrawingProjectionAnchorDto['endpoint']): number {
  return endpoint === 'start' ? 0 : 1;
}

function pointTouchesPolylines(
  point: [number, number],
  polylines: DrawingPolylineDto[],
  tolerance: number,
): boolean {
  return polylines.some((polyline) =>
    polyline.points.some((end, index) =>
      index > 0 && pointSegmentDistance(point, polyline.points[index - 1], end) <= tolerance,
    ),
  );
}

function pointSegmentDistance(
  point: [number, number],
  start: [number, number],
  end: [number, number],
): number {
  const dx = end[0] - start[0];
  const dy = end[1] - start[1];
  const lengthSq = dx * dx + dy * dy;
  if (lengthSq <= 1e-18) return Math.hypot(point[0] - start[0], point[1] - start[1]);
  const t = Math.max(0, Math.min(1, ((point[0] - start[0]) * dx + (point[1] - start[1]) * dy) / lengthSq));
  return Math.hypot(point[0] - (start[0] + dx * t), point[1] - (start[1] + dy * t));
}

function projectionBasis(direction: Vec3, desiredUp: Vec3): ProjectionBasis {
  const towardViewer = normalize(direction);
  const right = normalize(cross(desiredUp, towardViewer));
  const up = normalize(cross(towardViewer, right));
  return { right, up, towardViewer };
}

function projectPoint(point: Point3Dto, basis: ProjectionBasis): ProjectedPoint {
  const vector: Vec3 = [point.x, point.y, point.z];
  return [dot(vector, basis.right), dot(vector, basis.up), dot(vector, basis.towardViewer)];
}

function projectedTriangles(
  body: BodyDto,
  basis: ProjectionBasis,
  slab: SectionSlab | null,
): ProjectedTriangle[] {
  const { positions, indices } = body.mesh;
  const result: ProjectedTriangle[] = [];
  for (let cursor = 0; cursor + 2 < indices.length; cursor += 3) {
    const polygon = [indices[cursor], indices[cursor + 1], indices[cursor + 2]].map((index) => ({
      x: positions[index * 3],
      y: positions[index * 3 + 1],
      z: positions[index * 3 + 2],
    }));
    const clipped = clipTriangleToSectionSlab(polygon, slab);
    for (let fan = 1; fan + 1 < clipped.length; fan += 1) {
      const points = [clipped[0], clipped[fan], clipped[fan + 1]].map((point) =>
        projectPoint(point, basis),
      ) as [ProjectedPoint, ProjectedPoint, ProjectedPoint];
      result.push({
        points,
        bounds: [
          Math.min(...points.map((point) => point[0])),
          Math.min(...points.map((point) => point[1])),
          Math.max(...points.map((point) => point[0])),
          Math.max(...points.map((point) => point[1])),
        ],
      });
    }
  }
  return result;
}

function clipTriangleToSectionSlab(
  triangle: Point3Dto[],
  slab: SectionSlab | null,
): Point3Dto[] {
  if (!slab) return triangle;
  let polygon = clipPolygonBySectionDistance(triangle, slab, 0, true);
  if (slab.depth !== null) {
    polygon = clipPolygonBySectionDistance(polygon, slab, -slab.depth, false);
  }
  return polygon;
}

function clipPolygonBySectionDistance(
  polygon: Point3Dto[],
  slab: SectionSlab,
  limit: number,
  keepBelow: boolean,
): Point3Dto[] {
  if (polygon.length === 0) return [];
  const output: Point3Dto[] = [];
  const inside = (distance: number) => keepBelow
    ? distance <= limit + 1e-9
    : distance >= limit - 1e-9;
  for (let index = 0; index < polygon.length; index += 1) {
    const current = polygon[index];
    const previous = polygon[(index + polygon.length - 1) % polygon.length];
    const currentDistance = sectionDistance(current, slab);
    const previousDistance = sectionDistance(previous, slab);
    const currentInside = inside(currentDistance);
    const previousInside = inside(previousDistance);
    if (currentInside !== previousInside) {
      const amount = (limit - previousDistance) / (currentDistance - previousDistance);
      output.push(interpolateWorld(previous, current, amount));
    }
    if (currentInside) output.push(current);
  }
  return output;
}

function silhouetteSegments(body: BodyDto, basis: ProjectionBasis): Point3Dto[][] {
  const positions = body.mesh.positions;
  const indices = body.mesh.indices;
  const edges = new Map<string, { endpoints: [Point3Dto, Point3Dto]; signs: number[] }>();
  for (let cursor = 0; cursor + 2 < indices.length; cursor += 3) {
    const vertices = [indices[cursor], indices[cursor + 1], indices[cursor + 2]].map((index) => ({
      x: positions[index * 3],
      y: positions[index * 3 + 1],
      z: positions[index * 3 + 2],
    })) as [Point3Dto, Point3Dto, Point3Dto];
    const normal = cross(
      subtract(vertices[1], vertices[0]),
      subtract(vertices[2], vertices[0]),
    );
    const sign = Math.sign(dot(normal, basis.towardViewer));
    for (const [a, b] of [[0, 1], [1, 2], [2, 0]] as const) {
      const key = worldEdgeKey(vertices[a], vertices[b]);
      const entry = edges.get(key);
      if (entry) entry.signs.push(sign);
      else edges.set(key, { endpoints: [vertices[a], vertices[b]], signs: [sign] });
    }
  }
  return [...edges.values()]
    .filter(({ signs }) => {
      const front = signs.some((sign) => sign > 0);
      const back = signs.some((sign) => sign < 0);
      return (front && back) || (signs.length === 1 && front);
    })
    .map(({ endpoints }) => endpoints);
}

function classifyPolyline(
  points: ProjectedPoint[],
  triangles: ProjectedTriangle[],
): Array<{ hidden: boolean; points: ProjectedPoint[] }> {
  const output: Array<{ hidden: boolean; points: ProjectedPoint[] }> = [];
  for (let index = 0; index + 1 < points.length; index += 1) {
    const start = points[index];
    const end = points[index + 1];
    // Splitting long edges lets a line transition between visible and hidden
    // portions instead of classifying the whole curve by one midpoint.
    const length = Math.hypot(end[0] - start[0], end[1] - start[1]);
    const divisions = Math.max(1, Math.min(12, Math.ceil(length / 3)));
    for (let division = 0; division < divisions; division += 1) {
      const a = interpolate(start, end, division / divisions);
      const b = interpolate(start, end, (division + 1) / divisions);
      const midpoint = interpolate(a, b, 0.5);
      const hidden = triangles.some((triangle) => occludes(midpoint, triangle));
      const previous = output[output.length - 1];
      if (previous?.hidden === hidden && samePoint(previous.points[previous.points.length - 1], a)) {
        previous.points.push(b);
      } else {
        output.push({ hidden, points: [a, b] });
      }
    }
  }
  return output;
}

function occludes(point: ProjectedPoint, triangle: ProjectedTriangle): boolean {
  const [x, y, depth] = point;
  const [minX, minY, maxX, maxY] = triangle.bounds;
  if (x < minX - 1e-7 || x > maxX + 1e-7 || y < minY - 1e-7 || y > maxY + 1e-7) {
    return false;
  }
  const [a, b, c] = triangle.points;
  const denominator = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
  if (Math.abs(denominator) < 1e-12) return false;
  const u = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / denominator;
  const v = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / denominator;
  const w = 1 - u - v;
  if (u < -1e-7 || v < -1e-7 || w < -1e-7) return false;
  const triangleDepth = u * a[2] + v * b[2] + w * c[2];
  return triangleDepth > depth + 1e-5;
}

function projectionBounds(polylines: DrawingPolylineDto[]): [number, number, number, number] {
  if (polylines.length === 0) return [0, 0, 0, 0];
  let minX = Number.POSITIVE_INFINITY;
  let minY = Number.POSITIVE_INFINITY;
  let maxX = Number.NEGATIVE_INFINITY;
  let maxY = Number.NEGATIVE_INFINITY;
  for (const { points } of polylines) {
    for (const [x, y] of points) {
      minX = Math.min(minX, x);
      minY = Math.min(minY, y);
      maxX = Math.max(maxX, x);
      maxY = Math.max(maxY, y);
    }
  }
  return [minX, minY, maxX, maxY];
}

function polylineKey(points: Array<[number, number]>): string {
  const quantized = points.map(([x, y]) => `${Math.round(x * 1e6)},${Math.round(y * 1e6)}`);
  const forward = quantized.join(';');
  const reverse = [...quantized].reverse().join(';');
  return forward < reverse ? forward : reverse;
}

function worldEdgeKey(a: Point3Dto, b: Point3Dto): string {
  const pointKey = (point: Point3Dto) =>
    `${Math.round(point.x * 1e6)},${Math.round(point.y * 1e6)},${Math.round(point.z * 1e6)}`;
  const left = pointKey(a);
  const right = pointKey(b);
  return left < right ? `${left};${right}` : `${right};${left}`;
}

function interpolate(a: ProjectedPoint, b: ProjectedPoint, t: number): ProjectedPoint {
  return [
    a[0] + (b[0] - a[0]) * t,
    a[1] + (b[1] - a[1]) * t,
    a[2] + (b[2] - a[2]) * t,
  ];
}

function samePoint(a: ProjectedPoint, b: ProjectedPoint): boolean {
  return Math.abs(a[0] - b[0]) < 1e-9 && Math.abs(a[1] - b[1]) < 1e-9;
}

function subtract(a: Point3Dto, b: Point3Dto): Vec3 {
  return [a.x - b.x, a.y - b.y, a.z - b.z];
}

function cross(a: Vec3, b: Vec3): Vec3 {
  return [
    a[1] * b[2] - a[2] * b[1],
    a[2] * b[0] - a[0] * b[2],
    a[0] * b[1] - a[1] * b[0],
  ];
}

function dot(a: Vec3, b: Vec3): number {
  return a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
}

function normalize(value: Vec3): Vec3 {
  const length = Math.hypot(...value);
  if (!Number.isFinite(length) || length < 1e-9) {
    throw new Error('Drawing view direction and up vectors must be non-zero and non-parallel.');
  }
  return [value[0] / length, value[1] / length, value[2] / length];
}
