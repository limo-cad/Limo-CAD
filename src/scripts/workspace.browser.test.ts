// The production workspace and panel run unchanged; only native IPC is faked.
import {createElement} from 'react';
import {createRoot} from 'react-dom/client';
import {ScriptPanel} from '../components/ScriptPanel';
import {inspectUi, operateUi} from '../uiControl';
import {closeScripts, editScriptSource, runLoadedScript, showScriptExample,
  useScriptWorkspace, validateScriptSource, type ScriptExample, type ScriptInfo} from './workspace';

export async function checkScriptSourceOwnership() {
  const check = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };
  const initial = useScriptWorkspace.getState();
  const info = (source: string): ScriptInfo => ({name: source, source, step_count: 1, check_count: 0});
  // The lifecycle fixture also runs before the collection metadata migration.
  const exampleData = {id: 'replacement', name: 'Replacement', summary: '', group: 'document', operation: 'cad_document',
    kind: 'lesson', focus_operations: [], operations: [], preview: false, source: 'replacement source'};
  const example: ScriptExample = exampleData;
  let resolveInspect!: (value: ScriptInfo) => void;
  let rejectInspect!: (reason: Error) => void;
  const calls: string[] = [];
  const w = window as typeof window & {__TAURI_INTERNALS__?: {invoke: (command: string) => Promise<unknown>}};
  const native = w.__TAURI_INTERNALS__;
  w.__TAURI_INTERNALS__ = {invoke(command) {
    calls.push(command);
    if (command === 'native_script_examples') return Promise.resolve([]);
    if (command === 'native_script_inspect') return new Promise<ScriptInfo>((resolve, reject) => {
      resolveInspect = resolve; rejectInspect = reject;
    });
    throw new Error(`Unexpected model/file action during source loading: ${command}`);
  }};
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  const render = () => new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
  try {
    useScriptWorkspace.setState({open: true, info: info('existing source'), source: 'existing source',
      tab: 'source', completed: true, loading: false, running: false});
    root.render(createElement(ScriptPanel));
    await render();
    const loading = showScriptExample(example);
    await render();
    const editor = container.querySelector('textarea')!;
    check(editor.readOnly, 'The actual source field must lock while inspection is pending');
    const sourceControl = inspectUi().surfaces.flatMap(surface => surface.controls)
      .find(control => control.label === 'Script source')!;
    let readOnlyRejected = false;
    try { operateUi({action: 'set_value', target: sourceControl.id, value: 'lost edit'}); }
    catch (error) { readOnlyRejected = String(error).includes('read-only'); }
    check(readOnlyRejected, 'The shared UI/MCP edit path must respect the pending inspection');
    editScriptSource('lost edit');
    await runLoadedScript();
    check(useScriptWorkspace.getState().source === 'existing source', 'Pending load cannot race a source edit or run');
    closeScripts();
    resolveInspect(info(example.source));
    await loading;
    check(!useScriptWorkspace.getState().open, 'A completed load must not reopen a panel the user closed');
    check(useScriptWorkspace.getState().source === example.source, 'The selected source still loads without executing');
    check(!useScriptWorkspace.getState().loading && !useScriptWorkspace.getState().completed, 'Load releases its gate and prior completion');
    editScriptSource('revised source');
    const validation = validateScriptSource();
    closeScripts();
    rejectInspect(new Error('Invalid edited source'));
    await validation;
    check(useScriptWorkspace.getState().source === 'revised source' && !useScriptWorkspace.getState().loading
      && !useScriptWorkspace.getState().open && useScriptWorkspace.getState().error === 'Invalid edited source',
      'Failed inspection preserves editable source and Close while releasing the load gate');
    const retry = showScriptExample(example);
    resolveInspect(info(example.source));
    await retry;
    check(useScriptWorkspace.getState().error === null && useScriptWorkspace.getState().open, 'A subsequent explicit load recovers');
    return {checks: ['busy-source-lock', 'MCP-readonly', 'load-run-exclusion', 'close-during-load', 'inspection-failure-recovery'], calls};
  } finally {
    root.unmount(); container.remove();
    useScriptWorkspace.setState(initial);
    if (native) w.__TAURI_INTERNALS__ = native; else delete w.__TAURI_INTERNALS__;
  }
}
