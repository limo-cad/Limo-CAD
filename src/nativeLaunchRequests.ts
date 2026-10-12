/** Ordinary OS launches remain in Rust until handled after application readiness.
 * Files use the same Open/unsaved decision path; recipes open source only. */
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { isTauriRuntime } from './engine';
import { pendingEngineOperations, subscribeEngineOperations } from './engine/activity';
import { useAppStore } from './store/appStore';
import { openProject } from './files/projectFiles';
import { currentUnsavedPrompt } from './files/unsavedChanges';
import { projectTransitions } from './files/projectTransitions';
import { queueRecipeOpen } from './scripts/recipeLinks';
import { useScriptWorkspace } from './scripts/workspace';

type LaunchRequest = { id: number } & (
  | { kind: 'file'; path: string }
  | { kind: 'recipe'; recipe: string }
);
// An IPC acknowledgement can fail after Open succeeded. Retain handled IDs
// through effect remounts so a retry acknowledges rather than reopening.
const handled = new Set<number>();
let draining = false;
const afterDrain = new Set<() => void>();

export function installNativeLaunchRequests(): () => void {
  if (!isTauriRuntime()) return () => undefined;
  let disposed = false;
  let waiting = false;
  let requested = false;
  let stopEvents: (() => void) | undefined;
  const blocked = () => {
    const state = useAppStore.getState();
    const script = useScriptWorkspace.getState();
    return state.projectBusy || state.solidBusy || state.historyEdit !== null || state.activeSketch !== null
      || pendingEngineOperations() > 0 || !projectTransitions.isSettled() || currentUnsavedPrompt() !== null
      || script.loading || script.running || state.constraintDialog !== null;
  };
  const receive = async () => {
    requested = true;
    if (disposed) return;
    if (draining) { afterDrain.add(receive); return; }
    draining = true;
    try {
      do {
        requested = false;
        const requests = await invoke<LaunchRequest[]>('native_launch_pending');
        if (disposed) return;
        waiting = requests.length > 0;
        for (const request of requests) {
          if (disposed) return;
          if (!handled.has(request.id)) {
            if (blocked()) return;
            try {
              if (request.kind === 'file') {
                useAppStore.getState().setProjectBusy(true);
                try { await openProject({ filePath: request.path, interactive: true }); }
                finally { useAppStore.getState().setProjectBusy(false); }
              } else {
                await queueRecipeOpen(request.recipe);
              }
            } catch (error) {
              useAppStore.getState().setConstraintDialog({
                titleKey: 'file.errorTitle', message: error instanceof Error ? error.message : String(error),
              });
            }
            handled.add(request.id);
          }
          await invoke('native_launch_ack', { id: request.id });
          handled.delete(request.id);
        }
        waiting = false;
      } while (requested && !disposed);
    } catch (error) {
      if (!disposed) useAppStore.getState().setConstraintDialog({
        titleKey: 'file.errorTitle', message: error instanceof Error ? error.message : String(error),
      });
    } finally {
      draining = false;
      for (const retry of afterDrain) queueMicrotask(retry);
      afterDrain.clear();
      if (requested && !disposed) queueMicrotask(() => { void receive(); });
    }
  };
  const retry = () => { if (waiting && !disposed) void receive(); };
  const stopStore = useAppStore.subscribe(retry);
  const stopScripts = useScriptWorkspace.subscribe(retry);
  const stopOperations = subscribeEngineOperations(retry);
  const stopTransitions = projectTransitions.subscribe(retry);
  window.addEventListener('nbcad:unsaved-prompt-change', retry);
  void listen('native-launch-pending', () => { void receive(); }).then(stop => {
    if (disposed) { stop(); return; }
    stopEvents = stop;
    void receive();
  }).catch(error => console.error('Could not receive native launch requests', error));
  return () => {
    disposed = true;
    afterDrain.delete(receive);
    stopEvents?.(); stopStore(); stopScripts(); stopOperations(); stopTransitions();
    window.removeEventListener('nbcad:unsaved-prompt-change', retry);
  };
}
