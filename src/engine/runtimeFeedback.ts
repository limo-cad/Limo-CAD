import { beginRuntimeActivity } from '../runtimeActivity';
import { useStartupStore } from '../startup';

let parentGeometryActivity = 0;
/** Associate this synchronous native invocation with its caller's named job.
 * Never hold suppression across awaits, where unrelated jobs can interleave. */
export function withParentGeometryActivity<T>(work: () => T): T {
  parentGeometryActivity++;
  try { return work(); } finally { parentGeometryActivity--; }
}

/** Geometry mutations are observable work; polling/getters are not new jobs. */
export function runtimeCommandIsGeometry(command: string): boolean {
  return command.startsWith('engine_solid_') && command !== 'engine_solid_scene';
}

export async function withRuntimeCommandFeedback<T>(command: string, work: () => Promise<T>): Promise<T> {
  if (parentGeometryActivity || !runtimeCommandIsGeometry(command) || typeof window === 'undefined'
    || !useStartupStore.getState().completed) return work();
  const activity = beginRuntimeActivity({ kind: 'rebuild' });
  activity.setPhase('geometry');
  try {
    const value = await work();
    // This acknowledges geometry computation, not a presented viewport frame.
    // File Open separately waits for its current-session rendered view.
    activity.finish();
    return value;
  } catch (error) {
    activity.fail(error);
    throw error;
  }
}
