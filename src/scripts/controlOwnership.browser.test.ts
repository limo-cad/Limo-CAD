import {useAppStore} from '../store/appStore';
import {presentation} from '../operationPlayback';
import {applyLiveUiControl} from '../liveUiBridge';
import {inspectUi} from '../uiControl';
import {getSessionCamera, registerSessionCamera, unregisterSessionCamera, type ViewportCameraApi} from '../components/viewport/cameraApi';
import type {DocumentDto} from '../engine/types';

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return {promise, resolve};
}

/** Native may deliver A's control just before Open and its IPC reply may reach
 * JavaScript after B is hydrated. Exercise the real control dispatcher. */
export async function checkControlDocumentOwnership() {
  const check = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };
  const original = useAppStore.getState();
  const camera = getSessionCamera();
  const sharedDocument: DocumentDto = {name: 'Retained document', settings: {units: 'mm'},
    features: [], rollback_index: 0, browser: []};
  const w = window as typeof window & {__TAURI_INTERNALS__?: {invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>}};
  const native = w.__TAURI_INTERNALS__;
  const button = document.createElement('button');
  button.textContent = 'Current document action'; document.body.append(button);
  let effects = 0;
  button.onclick = () => { effects++; };
  const testCamera = {
    pointer: async () => { effects++; }, focus: () => { effects++; },
    home: () => { effects++; }, snapToDirection: () => { effects++; },
    isAnimating: () => false, bounds: () => ({x: 0, y: 0, width: 100, height: 100}),
    getSnapshot: () => ({position: [0, 0, 10], target: [0, 0, 0], up: [0, 1, 0]}),
  } as unknown as ViewportCameraApi;
  registerSessionCamera(testCamera);
  const replace = (file: string) => {
    // A reload can hydrate the same document object into the same native tab;
    // ownership must not depend only on a document reference or tab id.
    useAppStore.getState().loadProjectState({document: sharedDocument, scene: {bodies: [], errors: []}}, [], [], file);
    useAppStore.setState({engineKind: 'tauri', activeProjectTabId: 'same-tab', projectBusy: false, solidBusy: false});
  };
  const outcomes: string[] = [];
  try {
    for (const kind of ['configure', 'stop', 'pace', 'button', 'viewport', 'camera', 'window', 'file'] as const) {
      replace('A.nbcad'); effects = 0;
      const controlA = inspectUi(sharedDocument).surfaces.flatMap(surface => surface.controls)
        .find(control => control.label === button.textContent)!;
      const ui = kind === 'configure' ? {action: 'presentation', command: 'configure', mode: 'present'}
        : kind === 'stop' ? {action: 'presentation', command: 'stop'}
        : kind === 'pace' ? {action: 'inspect', pace_ms: 1000}
        : kind === 'button' ? {action: 'click', target: controlA.id}
        : kind === 'viewport' ? {action: 'viewport', gesture: 'click', point: [10, 10]}
        : kind === 'window' ? {action: 'window', mode: 'maximize'}
        : kind === 'file' ? {action: 'file', command: 'rename', name: 'Old command'} : undefined;
      const request = {id: `A-${kind}`, session_id: 'session-A', expires_ms: Date.now() + 10_000,
        ...(ui ? {ui} : {view: 'top', fit: true, duration_ms: 0})};
      const delivered = deferred<void>();
      const lateReply = deferred<unknown>();
      const responses: Record<string, unknown>[] = [];
      let nextRequest: unknown = null;
      let polls = 0;
      let publications = 0;
      w.__TAURI_INTERNALS__ = {async invoke(command, args = {}) {
        if (command === 'mcp_session_bridge_control') {
          if (args.response) { responses.push(args.response as Record<string, unknown>); return null; }
          if (++polls === 1) { delivered.resolve(); return lateReply.promise; }
          const pending = nextRequest; nextRequest = null; return pending;
        }
        effects++;
        throw new Error(`A retired control reached native command ${command}`);
      }};
      const applying = applyLiveUiControl(async () => { publications++; });
      await delivered.promise;
      replace('B.nbcad');
      const playbackB = JSON.stringify(presentation.snapshot());
      const stateB = JSON.stringify(useAppStore.getState());
      const controlB = inspectUi(sharedDocument).surfaces.flatMap(surface => surface.controls)
        .find(control => control.label === button.textContent)!;
      lateReply.resolve(request);
      await applying;
      check(effects === 0 && publications === 0, `Late ${kind} must not affect the replacement document`);
      check(JSON.stringify(presentation.snapshot()) === playbackB, `Late ${kind} must leave replacement playback unchanged`);
      check(JSON.stringify(useAppStore.getState()) === stateB, `Late ${kind} must not change replacement UI or busy state`);
      check(responses.length === 1 && responses[0].status === 'failed'
        && /document changed/i.test(String(responses[0].error)) && responses[0].session_id === 'session-A',
      `Late ${kind} needs a failure receipt belonging to A`);
      // Rejecting A must neither refresh the global UI snapshot (invalidating
      // B's controls) nor leave the control lane occupied.
      nextRequest = {id: `B-${kind}`, session_id: 'session-B', expires_ms: Date.now() + 10_000,
        ui: {action: 'click', target: controlB.id}};
      await applyLiveUiControl(async () => { publications++; });
      check(effects === 1 && responses.length === 2 && responses[1].status === 'applied'
        && responses[1].session_id === 'session-B', `B must retain its inspected controls after rejecting ${kind}`);
      outcomes.push(kind);
    }
    return {lateControlsRejected: outcomes, replacementControlsPreserved: true};
  } finally {
    button.remove(); unregisterSessionCamera(testCamera);
    if (camera) registerSessionCamera(camera);
    useAppStore.setState(original); presentation.documentChanged();
    if (native) w.__TAURI_INTERNALS__ = native; else delete w.__TAURI_INTERNALS__;
  }
}
