import { create } from 'zustand';
import { beginRuntimeActivity } from '../runtimeActivity';
import type { CamDocumentDto, SolidSceneDto } from '../engine/types';

export interface SimulationIntent {
  serial: number;
  kind: 'cam' | 'nc';
  setupId: number;
  operationId: number | null;
}

export type CamRibbonSection = 'program' | 'simulate' | 'output';

/** Ephemeral UI work, never persisted into a CAD document or a toolpath. */
export const useCamActivity = create<{
  jobs: ReadonlyMap<number, string>;
  intent: SimulationIntent | null;
  /** Ribbon navigation is presentation only; never changes the CAM inputs. */
  ribbonSection: CamRibbonSection;
}>(() => ({ jobs: new Map(), intent: null, ribbonSection: 'program' }));

let sequence = 0;
const inputNamespace = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(36).slice(2)}`;
const inputIds = new WeakMap<object, number>();
function inputId(value: object) {
  let id = inputIds.get(value);
  if (id === undefined) { id = ++sequence; inputIds.set(value, id); }
  return id;
}

/** Selecting the active setup changes persisted UI state, not machining
 * intent. Preserve its input identity without serializing geometry in JS. */
export function inheritCamSimulationInputs(before: CamDocumentDto, after: CamDocumentDto) {
  inputIds.set(after, inputId(before));
}

export function camSimulationInputKey(cam: CamDocumentDto, scene: SolidSceneDto, setupId: number, detail: string, tolerance: number) {
  return `cam-${inputNamespace}-${inputId(cam)}-${inputId(scene)}-${setupId}-${detail}-${tolerance}`;
}
export function beginCamActivity(label: string): (() => void) & { fail(error: unknown): void; discard(): void } {
  const id = ++sequence;
  const activity = beginRuntimeActivity({kind: 'cam', target: label});
  activity.setPhase('running');
  useCamActivity.setState(({ jobs }) => ({ jobs: new Map(jobs).set(id, label) }));
  let ended = false;
  const remove = () => {
    if (ended) return;
    ended = true;
    useCamActivity.setState(({ jobs }) => {
      const next = new Map(jobs);
      next.delete(id);
      return { jobs: next };
    });
  };
  const finish = () => { activity.finish(); remove(); };
  finish.fail = (error: unknown) => { activity.fail(error); remove(); };
  finish.discard = () => { activity.discard(); remove(); };
  return finish;
}

export function requestCamSimulation(kind: 'cam' | 'nc', setupId: number, operationId: number | null) {
  useCamActivity.setState({ ribbonSection: 'simulate', intent: { serial: ++sequence, kind, setupId, operationId } });
}

/** Give visible UI a paint opportunity before a fallback WASM call can
 * monopolize it. Hidden/minimized views may suspend animation frames; paint
 * feedback must never hold document work or guarded application exit hostage. */
export function paintCamActivity(): Promise<void> {
  if (document.visibilityState !== 'visible') return Promise.resolve();
  return new Promise<void>((resolve) => {
    let frame = 0;
    let finished = false;
    const finish = () => {
      if (finished) return;
      finished = true;
      cancelAnimationFrame(frame);
      window.clearTimeout(timer);
      document.removeEventListener('visibilitychange', visibilityChanged);
      resolve();
    };
    const visibilityChanged = () => {
      if (document.visibilityState !== 'visible') finish();
    };
    const timer = window.setTimeout(finish, 100);
    document.addEventListener('visibilitychange', visibilityChanged);
    frame = requestAnimationFrame(() => {
      if (!finished) frame = requestAnimationFrame(finish);
    });
  });
}
