import { create } from 'zustand';

export type RuntimeActivityKind = 'open' | 'import' | 'save' | 'export' | 'rebuild' | 'drawing' | 'cam';
export type RuntimeActivityPhase = 'queued' | 'choosing' | 'reading' | 'validating' | 'geometry' | 'adopting' | 'rendering' | 'writing' | 'running';
export interface RuntimeActivity {
  id: number;
  kind: RuntimeActivityKind;
  target?: string;
  phase: RuntimeActivityPhase;
  startedAt: number;
  phaseStartedAt: number;
}
export interface RuntimeActivityResult extends RuntimeActivity {
  status: 'completed' | 'failed';
  endedAt: number;
  error?: string;
}
export interface RuntimeActivityState {
  active: RuntimeActivity[];
  history: RuntimeActivityResult[];
  notifications: RuntimeActivityResult[];
}
export interface RuntimeActivityOptions {
  kind: RuntimeActivityKind;
  target?: string;
  notifySuccess?: boolean;
}
export const RUNTIME_ACTIVITY_GRACE_MS = 300;
export const RUNTIME_ACTIVITY_HISTORY_LIMIT = 20;
export const RUNTIME_ACTIVITY_NOTIFICATION_LIMIT = 6;

/** Independent owners describe observed work. This does not schedule CAD,
 * change edit guards, swallow failures, or claim percentages/frame readiness. */
export function createRuntimeActivityTracker(now: () => number = () => performance.now()) {
  const useStore = create<RuntimeActivityState>(() => ({ active: [], history: [], notifications: [] }));
  let nextId = 0;
  function begin(options: RuntimeActivityOptions) {
    const id = ++nextId;
    const startedAt = now();
    let settled = false;
    useStore.setState(state => ({ active: [...state.active, {
      id, kind: options.kind, target: options.target, phase: 'queued', startedAt, phaseStartedAt: startedAt,
    }] }));
    function settle(status: RuntimeActivityResult['status'], error?: string) {
      if (settled) return;
      settled = true;
      const endedAt = now();
      useStore.setState(state => {
        const task = state.active.find(item => item.id === id);
        if (!task) return state;
        const result = { ...task, status, endedAt, ...(error === undefined ? {} : { error }) };
        return {
          active: state.active.filter(item => item.id !== id),
          history: [result, ...state.history].slice(0, RUNTIME_ACTIVITY_HISTORY_LIMIT),
          notifications: status === 'failed' || options.notifySuccess
            ? [result, ...state.notifications].slice(0, RUNTIME_ACTIVITY_NOTIFICATION_LIMIT) : state.notifications,
        };
      });
    }
    return {
      id,
      setPhase(phase: RuntimeActivityPhase) {
        if (settled) return;
        const phaseStartedAt = now();
        useStore.setState(state => ({ active: state.active.map(task => task.id === id && task.phase !== phase
          ? { ...task, phase, phaseStartedAt } : task) }));
      },
      finish() { settle('completed'); },
      // The caller abandoned a picker/options flow; no work completion claim.
      discard() {
        if (settled) return;
        settled = true;
        useStore.setState(state => ({ active: state.active.filter(task => task.id !== id) }));
      },
      fail(error: unknown) {
        const detail = error instanceof Error ? error.message : String(error);
        settle('failed', detail.slice(0, 4000));
      },
    };
  }
  return {
    useStore, begin,
    dismissNotification(id: number) {
      useStore.setState(state => ({ notifications: state.notifications.filter(item => item.id !== id) }));
    },
  };
}

const runtimeActivity = createRuntimeActivityTracker();
export const useRuntimeActivityStore = runtimeActivity.useStore;
export const beginRuntimeActivity = runtimeActivity.begin;
export const dismissRuntimeNotification = runtimeActivity.dismissNotification;
export function visibleRuntimeActivities(tasks: readonly RuntimeActivity[], now: number): RuntimeActivity[] {
  return tasks.filter(task => now - task.startedAt >= RUNTIME_ACTIVITY_GRACE_MS);
}
