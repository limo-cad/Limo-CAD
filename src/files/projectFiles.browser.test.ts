// Production frontend recovery test. Only the native IPC boundary is replaced;
// archive parsing, engine adapter, project ownership, export flow and dialog run
// unchanged in the isolated browser used by the existing MCP contract suite.
import { createElement } from 'react';
import { createRoot } from 'react-dom/client';
import { MeshExportDialog } from '../components/MeshExportDialog';
import { useAppStore } from '../store/appStore';
import { openProject, export3mf, exportStl } from './projectFiles';
import { createNbcadArchive } from './nbcad';
import { I18nProvider } from '../i18n';
import type { DocumentDto, SolidSceneDto } from '../engine/types';

type Failure = 'unchanged' | 'repair' | 'publication' | 'transport' | 'success';

export async function checkProjectLoadRecovery() {
  const check = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };
  const documentA: DocumentDto = {name: 'Existing part', settings: {units: 'mm'}, features: [], rollback_index: 0, browser: []};
  const scene: SolidSceneDto = {bodies: [{id: 7, name: 'Existing body', feature_id: 1,
    mesh: {positions: [], normals: [], indices: []}, faces: [], edges: []}], errors: []};
  const nativeA = JSON.stringify({format: 'nbcad-project', schema_version: 6, document: documentA});
  const nativeB = JSON.stringify({format: 'nbcad-project', schema_version: 6, document: {...documentA, name: 'Replacement'}});
  const unsupported = JSON.stringify({format: 'nbcad-project', schema_version: 9999, document: documentA});
  const oldState = useAppStore.getState();
  useAppStore.setState({document: documentA, solidScene: scene, dirty: false,
    activeProjectTabId: 'retained-document', selectedBody: null, bodyAppearances: []});
  let behavior: Failure = 'unchanged';
  let nativeModel = nativeA;
  let captures = 0;
  const rendered: {command: string; model: string; scope: string}[] = [];
  const saved: {path: string; bytes: number[]}[] = [];
  const ok = (value: unknown) => JSON.stringify({ok: true, value});
  const w = window as typeof window & {__TAURI_INTERNALS__?: {invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>}};
  w.__TAURI_INTERNALS__ = {
    async invoke(command, args = {}) {
      if (command === 'read_binary_file') return Array.from(createNbcadArchive(behavior === 'unchanged' ? unsupported : nativeB));
      if (command === 'engine_project_load') {
        if (behavior === 'unchanged') return JSON.stringify({ok: false, error: 'Unsupported project schema 9999', data: {project_load_state: 'unchanged'}});
        if (behavior === 'transport') throw new Error('Native load reply was lost');
        nativeModel = nativeB;
        return ok({document: {...documentA, name: 'Replacement', rollback_index: 1,
          features: [{id: 1, kind: 'sketch', name: 'Profile', suppressed: false, status: {state: 'ok'}}]}, scene});
      }
      if (command === 'engine_datum_plane_definitions') {
        if (behavior === 'repair') throw new Error('Loaded datum restoration failed');
        return ok([]);
      }
      if (command === 'engine_finished_sketches') {
        if (behavior === 'publication') throw new Error('Frontend hydration read failed');
        return ok([]);
      }
      if (command === 'engine_body_appearances') return ok([]);
      if (command === 'engine_drawing_document') return ok(oldState.drawingDocument);
      if (command === 'engine_assembly_document') return ok(oldState.assemblyDocument);
      if (command === 'engine_assembly_solution') return ok(oldState.assemblySolution);
      if (command === 'engine_project_visibility') return ok(oldState.projectVisibility);
      if (command === 'engine_project_session_bind') return ok(null);
      if (command === 'engine_project_export_model') { captures++; return ok(nativeModel); }
      if (command === 'engine_export_3mf' || command === 'engine_export_stl') {
        const request = JSON.parse(args.payload as string);
        check(request.expected_model_json === nativeModel, 'Export must retain the actual native snapshot precondition');
        check(JSON.stringify(request.body_ids) === '[7]', 'Export must retain the original selected body definitions');
        rendered.push({command, model: nativeModel, scope: request.scope});
        return [71, 101, 111, rendered.length];
      }
      if (command === 'plugin:dialog|save') return (args.options as {defaultPath: string}).defaultPath;
      if (command === 'native_viewport_set_suspended') return null;
      if (command === 'write_binary_file_atomic') {
        saved.push({path: args.path as string, bytes: args.bytes as number[]});
        return null;
      }
      throw new Error(`Unexpected native command in project recovery test: ${command}`);
    },
  };

  const rootElement = document.createElement('div');
  document.body.append(rootElement);
  const root = createRoot(rootElement);
  root.render(createElement(I18nProvider, {locale: 'en', children: createElement(MeshExportDialog)}));
  async function exportMesh(format: '3mf' | 'stl'): Promise<string | null> {
    // Observe the real dialog and press its real Continue button. There is no
    // replacement of the ownership/export functions or the scope picker.
    const accept = () => {
      const button = [...rootElement.querySelectorAll<HTMLButtonElement>('button')]
        .find(candidate => candidate.textContent === 'Continue');
      button?.click();
    };
    const observer = new MutationObserver(accept);
    observer.observe(rootElement, {childList: true, subtree: true});
    try {
      const exported = await (format === '3mf' ? export3mf(false) : exportStl(false));
      check(exported, 'The real mesh export must complete');
      return null;
    } catch (error) { return String(error); }
    finally { observer.disconnect(); }
  }
  async function rejectOpen(expected: RegExp) {
    let failure: unknown;
    try { await openProject({filePath: 'candidate.nbcad', discardChanges: true}); }
    catch (error) { failure = error; }
    check(expected.test(String(failure)), `Expected rejected open ${expected}, got ${String(failure)}`);
  }
  async function assertBlocked() {
    const before = {captures, rendered: rendered.length, saved: saved.length};
    for (const format of ['3mf', 'stl'] as const) {
      check(/document changed/i.test(await exportMesh(format) ?? ''), 'An unpublished/unknown replacement must block each fresh export');
    }
    check(JSON.stringify(before) === JSON.stringify({captures, rendered: rendered.length, saved: saved.length}),
      'Blocked exports must not capture a model, render native geometry or write files');
  }
  try {
    // Regression from PR108: two independent exports after one provably atomic
    // failed load must use the still-open original document and its save name.
    await rejectOpen(/Unsupported project schema/);
    check(useAppStore.getState().document === documentA && nativeModel === nativeA, 'Rejected schema must leave both owners on A');
    for (const format of ['3mf', '3mf', 'stl'] as const) {
      check(await exportMesh(format) === null, `Fresh ${format} export was poisoned by rejected open`);
    }
    check(rendered.length === 3 && saved.length === 3, 'Every successful export must render and write exactly once');
    check(rendered.every(item => item.model === nativeA && item.scope === 'assembly'), 'Exports must retain A and the chosen scope');
    check(saved.every((item, index) => item.path === `Existing part.${index === 2 ? 'stl' : '3mf'}`
      && JSON.stringify(item.bytes) === JSON.stringify([71, 101, 111, index + 1])), 'Saved bytes and filenames must belong to A');

    for (const failure of ['repair', 'publication', 'transport'] as const) {
      const priorDocument = useAppStore.getState().document;
      behavior = failure;
      await rejectOpen(/restoration failed|hydration read failed|reply was lost/);
      check(useAppStore.getState().document === priorDocument, 'Failed publication keeps the old frontend owner');
      await assertBlocked();
      // A later unchanged rejection cannot bless an already unverified native
      // replacement. Only a successful native load + UI publication recovers it.
      behavior = 'unchanged';
      await rejectOpen(/Unsupported project schema/);
      await assertBlocked();
      behavior = 'success';
      check(await openProject({filePath: 'recovered.nbcad', discardChanges: true}), 'Successful reload must restore ownership');
      check(await exportMesh('3mf') === null, 'Successful native load and frontend publication must recover export');
    }
    return {atomicRejectedOpen: true, repeated3mf: true, stl: true,
      repairFailureGuard: true, publicationFailureGuard: true, unknownTransportGuard: true, successfulReloadRecovery: true};
  } finally {
    root.unmount();
    rootElement.remove();
    delete w.__TAURI_INTERNALS__;
  }
}
