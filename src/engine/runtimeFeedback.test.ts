import { withParentGeometryActivity, withRuntimeCommandFeedback } from './runtimeFeedback';
import { useStartupStore } from '../startup';
import { useRuntimeActivityStore } from '../runtimeActivity';

const assert = {
  equal(actual: unknown, expected: unknown, message = 'Unexpected feedback state') {
    if (actual !== expected) throw new Error(message);
  },
  async rejects(work: Promise<unknown>, predicate: (error: unknown) => boolean) {
    try { await work; } catch (error) {if (predicate(error)) return; throw error;}
    throw new Error('Expected a runtime operation failure');
  },
  throws(work: () => unknown, predicate: (error: unknown) => boolean) {
    try { work(); } catch (error) {if (predicate(error)) return; throw error;}
    throw new Error('Expected an invocation failure');
  },
};

const startupBefore = useStartupStore.getState();
const activityBefore = useRuntimeActivityStore.getState();
const hadWindow = 'window' in globalThis;
const windowBefore = (globalThis as typeof globalThis & {window?: unknown}).window;
let release!: () => void;
const waiting = new Promise<void>(resolve => { release = resolve; });
try {
  Object.defineProperty(globalThis, 'window', { configurable: true, value: {} });
  useStartupStore.setState({ completed: false, phase: 'document' });
  await withRuntimeCommandFeedback('engine_solid_extrude', async () => 'initial work');
  assert.equal(useRuntimeActivityStore.getState().active.length, 0);
  assert.equal(useRuntimeActivityStore.getState().history.length, activityBefore.history.length);

  useStartupStore.setState({ completed: true, phase: 'ready' });
  const nested = withParentGeometryActivity(() => withRuntimeCommandFeedback('engine_solid_body_feature', () => waiting));
  const separate = withRuntimeCommandFeedback('engine_solid_extrude', () => waiting);
  assert.equal(useRuntimeActivityStore.getState().active.length, 1,
    'Suppression for a parent invocation must not hide unrelated work while its promise waits.');
  release();
  await Promise.all([nested, separate]);
  assert.equal(useRuntimeActivityStore.getState().history.length, activityBefore.history.length + 1);

  const failure = new Error('Geometry could not be computed');
  await assert.rejects(withRuntimeCommandFeedback('engine_solid_fillet', async () => {throw failure;}), error => error === failure);
  assert.equal(useRuntimeActivityStore.getState().notifications[0].error, failure.message);
  assert.equal(useStartupStore.getState().completed, true, 'Runtime failure cannot restart application loading.');
  const beforeQuery = useRuntimeActivityStore.getState().history.length;
  await withRuntimeCommandFeedback('engine_solid_scene', async () => 'scene');
  assert.equal(useRuntimeActivityStore.getState().history.length, beforeQuery, 'Scene polling is not a new user job.');
  assert.throws(() => withParentGeometryActivity(() => {throw failure;}), error => error === failure);
  await withRuntimeCommandFeedback('engine_solid_recompute', async () => 'rebuilt');
  assert.equal(useRuntimeActivityStore.getState().history[0].status, 'completed', 'A throwing parent cannot leave suppression enabled.');
} finally {
  useStartupStore.setState(startupBefore);
  useRuntimeActivityStore.setState(activityBefore);
  if (hadWindow) Object.defineProperty(globalThis, 'window', {configurable: true, value: windowBefore});
  else Reflect.deleteProperty(globalThis, 'window');
}
console.log('Cold initialization, nested ownership, concurrent computation and runtime failure feedback passed.');
