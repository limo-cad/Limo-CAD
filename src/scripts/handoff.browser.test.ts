import {useAppStore} from '../store/appStore';
import {pendingEngineOperations, trackEngineOperation} from '../engine/activity';
import {runLoadedScript, useScriptWorkspace, type ScriptInfo} from './workspace';
import type {DocumentDto} from '../engine/types';

/** Real Run/New/snapshot path, with only native IPC replaced. This belongs
 * with Save's ownership guard and does not require later playback revisions. */
export async function checkScriptHandoffOwnership() {
  const check = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };
  const initialApp = useAppStore.getState();
  const initialScripts = useScriptWorkspace.getState();
  const document: DocumentDto = {name: 'Retained design', settings: {units: 'mm'}, features: [], rollback_index: 0, browser: []};
  let nativeDocument = document;
  let nativeDocumentId = 'retained-design';
  let created = 0;
  let runs = 0;
  const source = '{"version":1,"name":"Handoff","steps":[{"let":{"n":1}}]}';
  const info: ScriptInfo = {name: 'Handoff', source, step_count: 1, check_count: 0};
  const ok = (value: unknown) => JSON.stringify({ok: true, value});
  const w = window as typeof window & {__TAURI_INTERNALS__?: {invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>}};
  const previousNative = w.__TAURI_INTERNALS__;
  w.__TAURI_INTERNALS__ = {async invoke(command, args = {}) {
    if (command === 'native_script_inspect') return info;
    if (command === 'engine_project_session_create') {
      created++;
      nativeDocumentId = args.sessionId as string;
      nativeDocument = {...document, name: 'Untitled'};
      return ok({document: nativeDocument, scene: {bodies: [], errors: []}});
    }
    if (command === 'engine_project_export_model') return ok(JSON.stringify({document: nativeDocument}));
    if (command === 'engine_project_visibility') return ok(initialApp.projectVisibility);
    if (command === 'engine_active_sketch') return ok(null);
    if (command === 'mcp_session_bridge_reserve') return {session_id: `session-${nativeDocumentId}`, project_session_id: nativeDocumentId, generation: 1};
    if (command === 'mcp_session_bridge_write') return {skipped: false};
    if (command === 'native_script_run') { runs++; return {steps_completed: 1, checks_completed: 0}; }
    throw new Error(`Unexpected script handoff command: ${command}`);
  }};
  let releaseOther!: () => void;
  let unrelated: Promise<void> | undefined;
  try {
    useAppStore.getState().loadProjectState({document, scene: {bodies: [], errors: []}}, [], [], 'retained.nbcad');
    useAppStore.setState({engineKind: 'tauri', activeProjectTabId: nativeDocumentId,
      projectTabs: [{id: nativeDocumentId, name: document.name, fileName: 'retained.nbcad', dirty: false, workspaceTab: 'solid'}]});
    useScriptWorkspace.setState({source, info, loading: false, running: false, completed: false});
    unrelated = trackEngineOperation(new Promise<void>(resolve => { releaseOther = resolve; }));
    await runLoadedScript();
    check(created === 0 && runs === 0 && /document changed/i.test(useScriptWorkspace.getState().error ?? ''),
      'The script handoff may exclude only itself; another pending native edit must block New and Run');
    releaseOther(); await unrelated; unrelated = undefined;
    await runLoadedScript();
    check(created === 1 && runs === 1 && useScriptWorkspace.getState().completed,
      'Run must retain the prior design, create its own tab, publish and start native playback while its own receipt is pending');
    check(useAppStore.getState().projectTabs.some(tab => tab.id === 'retained-design' && tab.name === document.name),
      'The original design must remain retained in its own tab');
    check(pendingEngineOperations() === 0 && !useScriptWorkspace.getState().running,
      'Both rejected and successful handoffs must release their operation receipts');
    return {ownHandoff: true, unrelatedEditGuard: true, retainedDesign: true};
  } finally {
    if (unrelated) { releaseOther(); await unrelated; }
    useAppStore.setState(initialApp); useScriptWorkspace.setState(initialScripts);
    if (previousNative) w.__TAURI_INTERNALS__ = previousNative; else delete w.__TAURI_INTERNALS__;
  }
}
