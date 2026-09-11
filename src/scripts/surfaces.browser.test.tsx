// Production components, Run/New and presentation controller; only native IPC
// is replaced. Playwright supplies actual pointer and keyboard interaction.
import {createRoot} from 'react-dom/client';
import {FeatureScriptPreview} from '../components/FeatureScriptPreview';
import {ScriptPanel} from '../components/ScriptPanel';
import {PresentationControls, PresentationReopen} from '../components/PresentationControls';
import {presentation} from '../operationPlayback';
import {listenForModelKeys} from '../modelKeyboard';
import {useAppStore} from '../store/appStore';
import type {DocumentDto} from '../engine/types';
import {showScripts, useScriptWorkspace, type ScriptExample} from './workspace';

export function mountScriptSurfaces(disabled = true) {
  const source = '{"version":1,"name":"Presentation controls","steps":[{"let":{"n":1}}]}';
  const info = {name: 'Presentation controls', source, step_count: 1, check_count: 0};
  const exampleData = {id: 'lesson', name: 'Fillet lesson', summary: '', group: 'solid/modify', operation: 'solid_fillet',
    kind: 'lesson' as const, focus_operations: ['solid_fillet'], operations: ['solid_fillet'], preview: true, source};
  const example: ScriptExample = exampleData;
  let resolveCatalog!: (examples: ScriptExample[]) => void;
  const catalog = new Promise<ScriptExample[]>(resolve => { resolveCatalog = resolve; });
  let finishRun!: () => void;
  let documentId = 'retained-design';
  let model: DocumentDto = {name: 'Retained design', settings: {units: 'mm'}, features: [], rollback_index: 0, browser: []};
  const ok = (value: unknown) => JSON.stringify({ok: true, value});
  const calls: string[] = [];
  Object.assign(window, {__TAURI_INTERNALS__: {async invoke(command: string, args: Record<string, unknown> = {}) {
    calls.push(command);
    if (command === 'native_script_examples') return catalog;
    if (command === 'native_script_preview') throw new Error('Preview geometry intentionally unavailable in interaction fixture');
    if (command === 'native_script_inspect') return info;
    if (command === 'engine_project_session_create') {
      documentId = args.sessionId as string; model = {...model, name: 'Untitled'};
      return ok({document: model, scene: {bodies: [], errors: []}});
    }
    if (command === 'engine_project_export_model') return ok(JSON.stringify({document: model}));
    if (command === 'engine_project_visibility') return ok(useAppStore.getState().projectVisibility);
    if (command === 'engine_active_sketch') return ok(null);
    if (command === 'mcp_session_bridge_reserve') return {session_id: `session-${documentId}`, project_session_id: documentId, generation: 1};
    if (command === 'mcp_session_bridge_write') return {skipped: false};
    if (command === 'native_script_run') {
      presentation.control({command: 'configure', mode: args.mode as 'present' | 'fast', speed: Number(args.speed), step_count: 1});
      return new Promise(resolve => { finishRun = () => {
        presentation.control({command: 'finish', step_index: 1}); resolve({steps_completed: 1, checks_completed: 0});
      }; });
    }
    throw new Error(`Unexpected native action: ${command}`);
  }}});
  useAppStore.getState().loadProjectState({document: model, scene: {bodies: [], errors: []}}, [], [], 'retained.nbcad');
  useAppStore.setState({engineKind: 'tauri', activeProjectTabId: documentId,
    projectTabs: [{id: documentId, name: model.name, fileName: 'retained.nbcad', dirty: false, workspaceTab: 'solid'}]});
  useScriptWorkspace.setState({source, info, loading: false, running: false, open: false, selectedExample: null});
  let escapedToCad = 0;
  // Use the same routing boundary as the real viewport's capture listener.
  listenForModelKeys(event => {
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); escapedToCad++; }
  }, true);
  const container = document.createElement('div'); document.body.replaceChildren(container);
  createRoot(container).render(<>
    <button aria-label="Scripts" onClick={showScripts}>Scripts</button>
    <PresentationReopen />
    <FeatureScriptPreview group="solid/modify" operation="solid_fillet" label="Fillet" disabled={disabled}>
      <button disabled={disabled}>Fillet</button>
    </FeatureScriptPreview>
    <button aria-label="Outside">Outside</button>
    <ScriptPanel /><PresentationControls />
  </>);
  return {
    resolveCatalog: () => resolveCatalog([example]),
    finishRun: () => finishRun(),
    snapshot: () => ({playback: presentation.snapshot(), scripts: useScriptWorkspace.getState(), escapedToCad,
      retained: useAppStore.getState().projectTabs.some(tab => tab.id === 'retained-design'), calls}),
  };
}
