import {useAppStore} from '../store/appStore';
import {applyInboxNow, publishCurrentSession} from '../sessionBridge';
import {presentation} from '../operationPlayback';
import {projectTransitions} from '../files/projectTransitions';
import {applicationExitBarrier, createExitController} from '../files/applicationExit';
import type {DocumentDto} from '../engine/types';

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return {promise, resolve};
}

/** A real inbox operation owns hydration, snapshot publication and shutdown. */
export async function checkInboxCompletion() {
  const check = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };
  const initial = useAppStore.getState();
  const docA: DocumentDto = {name: 'A', settings: {units: 'mm'}, features: [], rollback_index: 0, browser: []};
  const docB = {...docA, name: 'B'};
  const scene = {bodies: [], errors: []};
  const w = window as typeof window & {__TAURI_INTERNALS__?: {invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>}};
  const previousNative = w.__TAURI_INTERNALS__;
  const ok = (value: unknown) => JSON.stringify({ok: true, value});
  const outcomes: string[] = [];
  try {
    for (const phase of ['solid', 'replacement', 'hydration-failure', 'unverified'] as const) {
      const fails = phase === 'hydration-failure' || phase === 'unverified';
      const setup = projectTransitions.begin();
      useAppStore.getState().loadProjectState({document: docA, scene}, [], [], 'A.nbcad');
      useAppStore.setState({engineKind: 'tauri', activeProjectTabId: 'same-tab',
        projectTabs: [{id: 'same-tab', name: 'A', fileName: 'A.nbcad', dirty: false, workspaceTab: 'solid'}]});
      setup(true, true);
      let nativeDocument = docA;
      let nativeSession = 'session-A';
      const entered = deferred<void>();
      const nativeReply = deferred<unknown>();
      const publishing = deferred<void>();
      const publishReply = deferred<void>();
      let applied = false;
      let polls = 0;
      let snapshots = 0;
      w.__TAURI_INTERNALS__ = {async invoke(command, args = {}) {
        if (command === 'mcp_session_bridge_reserve') return {session_id: nativeSession, project_session_id: 'same-tab', generation: 1};
        if (command === 'mcp_session_bridge_write') {
          check(JSON.parse(args.payload as string).session_id === nativeSession, 'Publish must follow the actual replacement session');
          snapshots++;
          if (applied) { publishing.resolve(); await publishReply.promise; }
          return {skipped: false};
        }
        if (command === 'mcp_session_bridge_apply_inbox') {
          check(args.sessionId === nativeSession, 'Inbox must be polled with its published identity');
          polls++; entered.resolve(); return nativeReply.promise;
        }
        if (command === 'get_document') return nativeDocument;
        if (command === 'engine_project_export_model') return ok(JSON.stringify({document: nativeDocument}));
        if (command === 'engine_project_visibility') return ok(initial.projectVisibility);
        if (command === 'engine_active_sketch') return ok(null);
        if (command === 'engine_solid_scene') return ok(scene);
        if (command === 'engine_drawing_document') return ok(initial.drawingDocument);
        if (command === 'engine_assembly_document') return ok(initial.assemblyDocument);
        if (command === 'engine_assembly_solution') return ok(initial.assemblySolution);
        if (command === 'engine_finished_sketches' && phase === 'hydration-failure') throw new Error('Replacement metadata is unavailable');
        if (['engine_finished_sketches', 'engine_datum_plane_definitions', 'engine_body_appearances'].includes(command)) return ok([]);
        throw new Error(`Unexpected inbox completion command: ${command}`);
      }};
      check(await publishCurrentSession(), 'Seed a published A owner');
      presentation.control({command: 'configure', mode: 'present', text: 'Old walkthrough'});
      const version = presentation.documentVersion();
      const applying = applyInboxNow();
      await entered.promise;
      let decisions = 0;
      let exits = 0;
      const exit = createExitController({
        dirty: () => useAppStore.getState().dirty,
        decide: async () => { decisions++; return 'cancel'; },
        save: async () => true,
        exit: async () => { exits++; },
        error: error => { throw error; },
      });
      const quitting = exit.request();
      await Promise.resolve();
      check(decisions === 0 && exits === 0, 'Quit must wait for an in-flight inbox native reply');
      nativeDocument = docB;
      if (phase !== 'solid') nativeSession = 'session-B';
      applied = true;
      nativeReply.resolve(phase === 'unverified'
        ? {applied: false, dead_lettered: true, project_replaced: true}
        : {applied: true, project_replaced: phase !== 'solid',
          name: phase !== 'solid' ? 'cad_load_project_model' : 'solid_extrude', result: {document: docB, scene}});
      if (!fails) {
        await publishing.promise;
        check(useAppStore.getState().dirty && decisions === 0 && exits === 0,
          'Quit must wait for the owning inbox snapshot without self-deadlocking its publication');
        publishReply.resolve();
      }
      await applying;
      await quitting;
      check(decisions === 1 && exits === 0 && useAppStore.getState().dirty,
        'Applied or uncertain native work must reach the unsaved decision before shutdown');
      check(polls === 1, 'One inbox operation must be consumed once');
      if (phase === 'solid') check(presentation.documentVersion() === version, 'An ordinary modeling op preserves its playback owner');
      else check(presentation.documentVersion() === version + 1 && !presentation.snapshot().active,
        'A successful or unverified replacement must retire the old walkthrough');
      if (fails) {
        check(snapshots === 1 && await publishCurrentSession() === null, 'An unverified replacement cannot publish stale frontend state');
      } else {
        check(snapshots === 2 && useAppStore.getState().document?.name === 'B', 'Hydrated B must publish through its own inbox transition');
      }
      await applicationExitBarrier.wait();
      exit.dispose();
      outcomes.push(phase);
    }
    return {ownedPublicationAndExit: outcomes};
  } finally {
    const restore = projectTransitions.begin();
    useAppStore.setState(initial); presentation.documentChanged();
    restore(true, true);
    if (previousNative) w.__TAURI_INTERNALS__ = previousNative; else delete w.__TAURI_INTERNALS__;
  }
}
