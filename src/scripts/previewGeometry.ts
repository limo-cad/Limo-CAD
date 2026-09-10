import type { SolidSceneDto } from '../engine/types';

export type PreviewPose = { yaw: number; pitch: number };
export const PREVIEW_HOME: PreviewPose = { yaw: Math.PI / 4, pitch: Math.PI / 6 };
type Point = [number, number, number];
export type PreviewFit = { center: Point; scale: number };
export type PreviewTriangle = { points: [Point, Point, Point]; color: string; depth: number };

const MAX_TRIANGLES = 100_000;

function project(point: Point, pose: PreviewPose): Point {
  const horizontal = Math.sin(pose.yaw) * point[0] - Math.cos(pose.yaw) * point[1];
  return [
    Math.cos(pose.yaw) * point[0] + Math.sin(pose.yaw) * point[1],
    -Math.sin(pose.pitch) * horizontal + Math.cos(pose.pitch) * point[2],
    Math.cos(pose.pitch) * horizontal + Math.sin(pose.pitch) * point[2],
  ];
}

/** Fit every frame together so a feature change never causes an apparent zoom. */
export function fitPreview(scenes: readonly SolidSceneDto[], pose: PreviewPose, width: number, height: number): PreviewFit {
  const low: Point = [Infinity, Infinity, Infinity];
  const high: Point = [-Infinity, -Infinity, -Infinity];
  for (const scene of scenes) for (const body of scene.bodies) {
    const { positions } = body.mesh;
    for (let index = 0; index + 2 < positions.length; index += 3) {
      const point = project([positions[index], positions[index + 1], positions[index + 2]], pose);
      if (!point.every(Number.isFinite)) continue;
      for (let axis = 0; axis < 3; axis++) {
        low[axis] = Math.min(low[axis], point[axis]);
        high[axis] = Math.max(high[axis], point[axis]);
      }
    }
  }
  if (!Number.isFinite(low[0])) return { center: [0, 0, 0], scale: 1 };
  return {
    center: low.map((value, axis) => (value + high[axis]) / 2) as Point,
    scale: Math.min((width - 36) / Math.max(high[0] - low[0], 1), (height - 36) / Math.max(high[1] - low[1], 1)),
  };
}

/** Immutable, bounded projection for tiny teaching models; no live CAD engine. */
export function previewTriangles(scene: SolidSceneDto, pose: PreviewPose, fit: PreviewFit, width: number, height: number): PreviewTriangle[] {
  if (scene.bodies.reduce((count, body) => count + body.mesh.indices.length / 3, 0) > MAX_TRIANGLES) {
    throw new Error('This model is too large for a feature preview. Open its script to inspect it.');
  }
  const triangles: PreviewTriangle[] = [];
  for (const body of scene.bodies) {
    const { positions, indices } = body.mesh;
    for (let index = 0; index + 2 < indices.length; index += 3) {
      if (indices.slice(index, index + 3).some(vertex => !Number.isInteger(vertex) || vertex < 0 || vertex * 3 + 2 >= positions.length)) continue;
      const vertices = indices.slice(index, index + 3).map(vertex => {
        const offset = vertex * 3;
        return positions.slice(offset, offset + 3) as Point;
      });
      if (vertices.some(vertex => vertex.length !== 3 || !vertex.every(Number.isFinite))) continue;
      const [a, b, c] = vertices;
      const ab = b.map((value, axis) => value - a[axis]);
      const ac = c.map((value, axis) => value - a[axis]);
      const normal = [ab[1] * ac[2] - ab[2] * ac[1], ab[2] * ac[0] - ab[0] * ac[2], ab[0] * ac[1] - ab[1] * ac[0]];
      const length = Math.hypot(...normal);
      if (length < 1e-12) continue;
      const light = Math.max(0, (normal[0] * 0.3 - normal[1] * 0.5 + normal[2] * 0.81) / length);
      const shade = 0.45 + 0.55 * light;
      const points = vertices.map(vertex => {
        const projected = project(vertex, pose);
        return [
          width / 2 + (projected[0] - fit.center[0]) * fit.scale,
          height / 2 - (projected[1] - fit.center[1]) * fit.scale,
          projected[2],
        ] as Point;
      }) as [Point, Point, Point];
      triangles.push({ points, color: `rgb(${[139, 190, 213].map(channel => Math.round(channel * shade)).join(',')})`, depth: points.reduce((sum, point) => sum + point[2], 0) / 3 });
    }
  }
  // These previews are deliberately small solid feature examples. Painter order
  // avoids a GPU context and leaves the main viewport's renderer untouched.
  return triangles.sort((a, b) => a.depth - b.depth);
}

export function previewStep(index: number, count: number): { index: number; playing: boolean } {
  return { index: Math.min(index + 1, Math.max(0, count - 1)), playing: index + 1 < count - 1 };
}
