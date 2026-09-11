import {useAppStore} from '../store/appStore';
import {presentation} from '../operationPlayback';
import {trackEngineOperation} from '../engine/activity';
import {closeScripts, runLoadedScript, stopScript, useScriptWorkspace, type ScriptInfo} from './workspace';
import type {DocumentDto, SolidUpdateDto} from '../engine/types';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((done, failed) => { resolve = done; reject = failed; });
  return {promise, resolve, reject};
}

/** Production Run, tab creation, publication and lifecycle; only IPC is mocked.
 * Native tests separately prove the retired UUID rejects old queued commands. */
export async function checkScriptDocumentOwnership() {
  const check = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };
  const initialApp = useAppStore.getState();
  const initialScripts = useScriptWorkspace.getState();
  const document: DocumentDto = {name: 'Original', settings: {units: 'mm'}, features: [], rollback_index: 0, browser: []};
  let nativeUpdate: SolidUpdateDto = {document, scene: {bodies: [], errors: []}};
  let nativeDocumentId = 'existing';
  let nativeSessionId = 'existing-session';
  let pendingInspect: ReturnType<typeof deferred<ScriptInfo>> | null = deferred<ScriptInfo>();
  let started = deferred<void>();
  let completed = deferred<unknown>();
  const source = '{"version":1,"name":"test","steps":[{"let":{"n":1}}]}';
  const info: ScriptInfo = {name: 'test', source, step_count: 1, check_count: 0};
  const calls: string[] = [];
  const ok = (value: unknown) => JSON.stringify({ok: true, value});
  const w = window as typeof window & {__TAURI_INTERNALS__?: {invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>}};
  const previousNative = w.__TAURI_INTERNALS__;
  w.__TAURI_INTERNALS__ = {async invoke(command, args = {}) {
    calls.push(command);
    if (command === 'native_script_inspect') return pendingInspect ? pendingInspect.promise : info;
    if (command === 'engine_project_session_create') {
      nativeDocumentId = args.sessionId as string;
      nativeSessionId = `published-${nativeDocumentId}`;
      nativeUpdate = {...nativeUpdate, document: {...document, name: 'Untitled'}};
      return ok(nativeUpdate);
    }
    if (command === 'engine_project_export_model') return ok(JSON.stringify(nativeUpdate));
    if (command === 'engine_project_visibility') return ok({hidden_body_ids: [], hidden_datum_plane_ids: [], hidden_sketch_names: []});
    if (command === 'engine_active_sketch') return ok(null);
    if (command === 'mcp_session_bridge_reserve') return {session_id: nativeSessionId, project_session_id: nativeDocumentId, generation: 1};
    if (command === 'mcp_session_bridge_write') {
      const payload = JSON.parse(args.payload as string);
      check(payload.session_id === nativeSessionId && payload.project_session_id === nativeDocumentId, 'Publish must retain the reservation identity');
      return {skipped: false};
    }
    if (command === 'native_script_run') {
      check(args.documentId === nativeDocumentId && args.sessionId === nativeSessionId,
        'Run must pass the document AND session identity from its successful snapshot write');
      presentation.control({command: 'configure', mode: 'present', step_index: 0, step_count: 1});
      started.resolve();
      return completed.promise;
    }
    throw new Error(`Unexpected native command: ${command}`);
  }};
  const replace = (name: string) => useAppStore.getState().loadProjectState({
    document: {...document, name}, scene: {bodies: [], errors: []},
  }, [], [], `${name}.nbcad`);
  try {
    replace('Original');
    useAppStore.setState({engineKind: 'tauri', activeProjectTabId: 'existing',
      projectTabs: [{id: 'existing', name: 'Original', fileName: null, dirty: false, workspaceTab: 'solid'}]});
    useScriptWorkspace.setState({source, info, loading: false, running: false, completed: false});
    const inspecting = runLoadedScript();
    replace('Opened during inspection');
    pendingInspect.resolve(info); pendingInspect = null;
    await inspecting;
    check(!calls.includes('engine_project_session_create') && !calls.includes('native_script_run'),
      'A replaced document during inspection must cancel before creating a tab or starting native commands');

    const unrelatedNativeReply = deferred<void>();
    const unrelatedOperation = trackEngineOperation(unrelatedNativeReply.promise);
    await runLoadedScript();
    check(!calls.includes('engine_project_session_create') && !calls.includes('native_script_run'),
      'The script handoff token must not exempt an unrelated pending engine operation');
    unrelatedNativeReply.resolve();
    await unrelatedOperation;

    let replaceAfterNew = true;
    const stopWatchingNew = presentation.subscribe(() => {
      if (!replaceAfterNew) return;
      replaceAfterNew = false;
      queueMicrotask(() => replace('Opened before New returned'));
    });
    try { await runLoadedScript(); } finally { stopWatchingNew(); }
    check(!calls.includes('native_script_run') && useScriptWorkspace.getState().error?.includes('document changed'),
      'A replacement before New resolves must not become the script target');

    const successful = runLoadedScript();
    await started.promise;
    closeScripts();
    presentation.control({command: 'finish', step_index: 1, step_count: 1});
    completed.resolve({steps_completed: 1, checks_completed: 0});
    await successful;
    check(useScriptWorkspace.getState().completed && presentation.snapshot().finished, 'The owning document receives completion');
    check(!useScriptWorkspace.getState().open, 'Closing the panel does not cancel playback or reopen it on completion');
    const revision = presentation.documentVersion();
    useAppStore.setState({document: {...useAppStore.getState().document!, name: 'Saved name'}, dirty: false});
    check(presentation.documentVersion() === revision && presentation.snapshot().finished,
      'An ordinary model update/Save rename must not discard the completed walkthrough');
    replace('Replacement in the same tab');
    check(!useScriptWorkspace.getState().completed && !presentation.snapshot().active && presentation.canApply(),
      'Open in the same tab clears old completion and leaves the replacement inbox usable');

    started = deferred<void>(); completed = deferred<unknown>();
    const oldRun = runLoadedScript();
    await started.promise;
    presentation.control({command: 'stop'});
    replace('Another file');
    stopScript();
    check(!presentation.snapshot().active && presentation.canApply(), 'Stop for the retired run cannot stop the replacement document');
    completed.resolve({steps_completed: 1, checks_completed: 0});
    await oldRun;
    check(!useScriptWorkspace.getState().completed && !useScriptWorkspace.getState().running
      && useScriptWorkspace.getState().error?.includes('document changed'),
      'A late native success from the replaced run cannot label another file complete');
    started = deferred<void>(); completed = deferred<unknown>();
    const failedRun = runLoadedScript();
    await started.promise;
    completed.reject(new Error('Native receipt was lost'));
    await failedRun;
    check(presentation.snapshot().stopped && !useScriptWorkspace.getState().running
      && useScriptWorkspace.getState().error === 'Native receipt was lost',
      'A native failure without a completion control stops the owning playback and releases the workspace');
    return {checks: ['inspection-owner', 'unrelated-engine-operation', 'new-handoff-owner', 'published-session-identity', 'completion-owner', 'save-retains-record',
      'close-during-run', 'same-tab-open-reset', 'stopped-run-isolation', 'late-native-completion', 'native-failure'],
      nativeRuns: calls.filter(call => call === 'native_script_run').length};
  } finally {
    useScriptWorkspace.setState(initialScripts); useAppStore.setState(initialApp);
    presentation.documentChanged();
    if (previousNative) w.__TAURI_INTERNALS__ = previousNative; else delete w.__TAURI_INTERNALS__;
  }
}
