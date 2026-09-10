import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import type { SolidSceneDto } from '../engine/types';
import { ScriptPreview } from '../components/ScriptPreview';
import { fitPreview, PREVIEW_HOME, previewStep, previewTriangles } from './previewGeometry';

function check(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}
function freeze<T>(value: T): T {
  if (value && typeof value === 'object') {
    for (const child of Object.values(value)) freeze(child);
    Object.freeze(value);
  }
  return value;
}
const scene: SolidSceneDto = {
  bodies: [{ id: 1, name: 'Stock', feature_id: 2, faces: [], edges: [], mesh: {
    positions: [0, 0, 0, 60, 0, 0, 0, 30, 0, 0, 0, 12], normals: [],
    indices: [0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3],
  } }], errors: [],
};
freeze(scene);
const snapshot = JSON.stringify(scene);
const fit = fitPreview([scene], PREVIEW_HOME, 300, 176);
const triangles = previewTriangles(scene, PREVIEW_HOME, fit, 300, 176);
check(triangles.length === 4, 'Every valid kernel triangle is represented');
check(triangles.every(triangle => triangle.points.every(point => point[0] >= 17.9 && point[0] <= 282.1 && point[1] >= 17.9 && point[1] <= 158.1)), 'The complete geometry fits with a readable margin');
check(triangles.every((triangle, index) => index === 0 || triangle.depth >= triangles[index - 1].depth), 'Painter order is stable from back to front');
check(JSON.stringify(scene) === snapshot, 'Preview fitting and rendering preserve the immutable kernel snapshot');
check(JSON.stringify(triangles) === JSON.stringify(previewTriangles(scene, PREVIEW_HOME, fit, 300, 176)), 'Repeated snapshots produce identical geometry');
const moved: SolidSceneDto = JSON.parse(snapshot);
moved.bodies[0].mesh.positions = moved.bodies[0].mesh.positions.map((value, index) => value + (index % 3 === 0 ? 80 : 0));
const commonFit = fitPreview([scene, moved], PREVIEW_HOME, 300, 176);
check(commonFit.scale < fit.scale, 'A shared camera fits the entire sequence instead of jumping between feature frames');
for (const sample of [scene, moved]) {
  check(previewTriangles(sample, PREVIEW_HOME, commonFit, 300, 176).every(triangle => triangle.points.every(point => point[0] >= 17.9 && point[0] <= 282.1)), 'All frames fit the same camera');
}
const malformed: SolidSceneDto = JSON.parse(snapshot);
malformed.bodies[0].mesh.indices = [-1, 2, 3, 0, 0, 0, 0, 1, 99];
check(previewTriangles(malformed, PREVIEW_HOME, fit, 300, 176).length === 0, 'Malformed and degenerate triangles cannot create bogus geometry');
const empty: SolidSceneDto = { bodies: [], errors: [] };
check(fitPreview([empty], PREVIEW_HOME, 300, 176).scale === 1, 'Empty geometry has a finite fit');
check(previewStep(0, 3).index === 1 && previewStep(0, 3).playing, 'Playback advances through its feature frames');
check(previewStep(1, 3).index === 2 && !previewStep(1, 3).playing, 'Playback stops at the final frame instead of looping');
check(previewStep(2, 3).index === 2 && !previewStep(2, 3).playing, 'The completed preview remains on its last result');
const markup = renderToStaticMarkup(createElement(ScriptPreview, { frames: [{ caption: 'Extrude 12 mm stock', scene }, { caption: 'Round the top edges', scene }], autoPlay: false }));
check(markup.includes('role="img"') && markup.includes('Example model: Extrude 12 mm stock'), 'The canvas exposes its model and current caption to assistive tools');
check(markup.includes('Replay feature preview') && markup.includes('Next preview step') && markup.includes('Fit preview model'), 'Playback and camera controls render independently of the main app');
check(!markup.includes('data-native-viewport-overlay'), 'The reusable preview is normal layout content; only its temporary ribbon flyout needs native occlusion');
console.log('Script preview geometry and rendering checks passed');
