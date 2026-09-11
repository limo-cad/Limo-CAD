import {useAppStore} from '../store/appStore';
import {publishCurrentSession, applyInboxNow} from '../sessionBridge';
import {openProject} from '../files/projectFiles';
import {createNbcadArchive} from '../files/nbcad';
import {projectTransitions} from '../files/projectTransitions';
import {presentation} from '../operationPlayback';
import type {DocumentDto, ProjectVisibilityDto} from '../engine/types';

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return {promise, resolve};
}

/** Real publisher/Open/inbox handlers; only native IPC delivery is controlled. */
export async function checkPublicationOwnership() {
  const check = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };
  const initial = useAppStore.getState();
  const docA: DocumentDto = {name: 'A', settings: {units: 'mm'}, features: [], rollback_index: 0, browser: []};
  const docB = {...docA, name: 'B'};
  const scene = {bodies: [], errors: []};
  const visibilityA: ProjectVisibilityDto = {hidden_body_ids: [1], hidden_datum_plane_ids: [], hidden_sketch_names: []};
  const visibilityB: ProjectVisibilityDto = {...visibilityA, hidden_body_ids: [2]};
  const modelB = JSON.stringify({format: 'nbcad-project', schema_version: 6, document: docB});
  const w = window as typeof window & {__TAURI_INTERNALS__?: {invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>}};
  const previousNative = w.__TAURI_INTERNALS__;
  const ok = (value: unknown) => JSON.stringify({ok: true, value});
  const checks: string[] = [];
  try {
    for (const phase of ['visibility-read', 'visibility-write', 'reserve', 'active-sketch', 'export', 'write']) {
      useAppStore.getState().loadProjectState({document: docA, scene}, [], [], 'A.nbcad', [],
        initial.drawingDocument, initial.assemblyDocument, visibilityA);
      useAppStore.setState({engineKind: 'tauri', activeProjectTabId: 'same-tab',
        projectTabs: [{id: 'same-tab', name: 'A', fileName: 'A.nbcad', dirty: false, workspaceTab: 'solid'}]});
      let nativeDocument = docA;
      let nativeVisibility = phase === 'visibility-write' ? visibilityB : visibilityA;
      let nativeSession = 'session-A';
      const interrupted = deferred<void>();
      const continueSnapshot = deferred<void>();
      const loading = deferred<void>();
      const fileRead = deferred<void>();
      const hydrate = deferred<void>();
      let paused = false;
      const calls: string[] = [];
      const writes: string[] = [];
      const hold = async (point: string) => {
        if (phase === point && !paused) { paused = true; interrupted.resolve(); await continueSnapshot.promise; }
      };
      w.__TAURI_INTERNALS__ = {async invoke(command, args = {}) {
        calls.push(command);
        if (command === 'read_binary_file') { fileRead.resolve(); return Array.from(createNbcadArchive(modelB)); }
        if (command === 'engine_project_load') {
          nativeDocument = docB; nativeVisibility = visibilityB; nativeSession = 'session-B';
          loading.resolve(); return ok({document: docB, scene});
        }
        if (command === 'engine_finished_sketches') { await hydrate.promise; return ok([]); }
        if (command === 'engine_datum_plane_definitions' || command === 'engine_body_appearances') return ok([]);
        if (command === 'engine_drawing_document') return ok(initial.drawingDocument);
        if (command === 'engine_assembly_document') return ok(initial.assemblyDocument);
        if (command === 'engine_assembly_solution') return ok(initial.assemblySolution);
        if (command === 'engine_project_visibility') { await hold('visibility-read'); return ok(nativeVisibility); }
        if (command === 'engine_project_set_visibility') {
          await hold('visibility-write');
          nativeVisibility = JSON.parse(args.payload as string);
          check(nativeDocument === docA, 'An already issued A visibility write must finish before native Open');
          return ok(nativeVisibility);
        }
        if (command === 'engine_active_sketch') { await hold('active-sketch'); return ok(null); }
        if (command === 'engine_project_export_model') {
          await hold('export'); return ok(JSON.stringify({format: 'nbcad-project', schema_version: 6, document: nativeDocument}));
        }
        if (command === 'mcp_session_bridge_reserve') {
          await hold('reserve'); return {session_id: nativeSession, project_session_id: 'same-tab', generation: 1};
        }
        if (command === 'mcp_session_bridge_write') {
          await hold('write');
          writes.push(JSON.parse(args.payload as string).session_id);
          return {skipped: false};
        }
        if (command === 'mcp_session_bridge_apply_inbox') {
          check(args.sessionId === nativeSession, 'A cached publisher owner must never poll the replacement');
          return {applied: false};
        }
        throw new Error(`Unexpected native command: ${command}`);
      }};
      const publishing = publishCurrentSession();
      await interrupted.promise;
      const opening = openProject({filePath: 'B.nbcad', discardChanges: true});
      await fileRead.promise;
      let replacementPending = false;
      for (let turn = 0; turn < 30 && !replacementPending; turn++) {
        await Promise.resolve();
        try { projectTransitions.assertSettled(projectTransitions.capture()); } catch { replacementPending = true; }
      }
      check(replacementPending, 'Open must claim its transition while the publication is delayed');
      check(nativeDocument === docA, 'Open must drain an existing snapshot before replacing native state');
      continueSnapshot.resolve();
      check(await publishing === null, 'A publisher interrupted by a pending Open cannot authorize its owner');
      await loading.promise;
      check(useAppStore.getState().document?.name === 'A', 'Hold the real native-to-frontend hydration gap');
      const before = calls.length;
      check(await publishCurrentSession() === null, 'Publication must reject while B native/A frontend ownership is mixed');
      await applyInboxNow();
      check(calls.length === before, 'Mixed ownership must not read, synchronize visibility, reserve, export, write or poll');
      check(nativeVisibility === visibilityB, 'B must retain its loaded visibility during metadata hydration');
      hydrate.resolve();
      check(await opening, 'The actual Open must finish after hydration');
      const owner = await publishCurrentSession();
      check(owner?.sessionId === 'session-B' && useAppStore.getState().document?.name === 'B', 'Only fully hydrated B can publish a current owner');
      check(writes.every(session => session === 'session-A' || session === 'session-B'), 'Every write must retain its reserved identity');
      await applyInboxNow();
      checks.push(phase);
    }
    return {blockedAcrossOpen: checks, restoredPublication: true};
  } finally {
    useAppStore.setState(initial); presentation.documentChanged();
    if (previousNative) w.__TAURI_INTERNALS__ = previousNative; else delete w.__TAURI_INTERNALS__;
  }
}
