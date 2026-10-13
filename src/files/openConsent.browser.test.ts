import { openProject } from './projectFiles';
import { createNbcadArchive } from './nbcad';
import { useAppStore } from '../store/appStore';
import { useStartupStore } from '../startup';
import { useDocumentLoadStore } from './documentLoadProgress';
import { useRuntimeActivityStore } from '../runtimeActivity';
import { projectTransitions } from './projectTransitions';
import type { DocumentDto } from '../engine/types';

/** Exercise the real Open/ownership path, replacing only native file IO.
 * A file selected for A cannot silently replace newer unsaved work or tab B. */
export async function checkOpenConsentOwnership() {
  const before = useAppStore.getState();
  const startupBefore = useStartupStore.getState();
  const activityBefore = useRuntimeActivityStore.getState();
  const w = window as typeof window & {__TAURI_INTERNALS__?: {invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>}};
  const internals = w.__TAURI_INTERNALS__;
  const documentA: DocumentDto = {name: 'Original part', settings: {units: 'mm'}, features: [], rollback_index: 0, browser: []};
  const archive = Array.from(createNbcadArchive(JSON.stringify({format: 'nbcad-project', schema_version: 6, document: documentA})));
  let nativeLoads = 0;
  const check = (condition: unknown, message: string) => {if (!condition) throw new Error(message);};
  try {
    useStartupStore.setState({completed: true, phase: 'ready'});
    for (const replace of ['edit', 'tab', 'pending-save-edit', 'pending-save-cam', 'pending-save-view', 'transition'] as const) {
      useAppStore.setState({...before, document: documentA, activeProjectTabId: 'document-A', dirty: false,
        projectBusy: false, solidBusy: false, historyEdit: null, activeSketch: null});
      let beginRead!: () => void;
      let finishRead!: (bytes: number[]) => void;
      const reading = new Promise<void>(resolve => {beginRead = resolve;});
      const bytes = new Promise<number[]>(resolve => {finishRead = resolve;});
      const snapshot = replace.startsWith('pending-save-') ? projectTransitions.beginSnapshot() : null;
      w.__TAURI_INTERNALS__ = {async invoke(command) {
        if (command === 'read_binary_file') {beginRead(); return bytes;}
        if (command === 'engine_project_load') nativeLoads++;
        throw new Error('Unexpected native command: '+command);
      }};
      const opening = openProject({filePath: 'C:\\private-fixture.nbcad',
        ...(snapshot ? {discardChanges: true} : {interactive: true})});
      const result = opening.then(() => null, error => error);
      await reading;
      if (replace === 'edit') useAppStore.setState({dirty: true});
      else if (replace === 'tab') useAppStore.setState({document: {...documentA, name: 'Newer part'}, activeProjectTabId: 'document-B', dirty: true});
      else if (replace === 'pending-save-edit') useAppStore.setState({document: {...documentA, settings: {units: 'cm'}}, dirty: true});
      else if (replace === 'pending-save-cam') useAppStore.setState({camDocument: {...before.camDocument}, dirty: true});
      else if (replace === 'pending-save-view') useAppStore.setState({activeNamedView: 'Newer view', dirty: true});
      else projectTransitions.begin()(true, false);
      snapshot?.release();
      finishRead(archive);
      const error = await result;
      check(error instanceof Error && error.message.length > 0, 'Changed ownership must reject this Open.');
      check(nativeLoads === 0, 'Open must reject stale consent before replacing the native model.');
      check(replace === 'transition' || useAppStore.getState().dirty, 'Unsaved work must remain dirty.');
      check(useAppStore.getState().activeProjectTabId === (replace === 'tab' ? 'document-B' : 'document-A'), 'Open must preserve the newer document owner.');
      check(useStartupStore.getState().completed, 'Runtime rejection must not show application startup again.');
      check(useDocumentLoadStore.getState().operations.length === 0, 'Rejected Open must retire only its progress owner.');
      check(useRuntimeActivityStore.getState().active.length === 0, 'Rejected Open must retire its Activity job.');
    }
  } finally {
    w.__TAURI_INTERNALS__ = internals;
    useAppStore.setState(before);
    useStartupStore.setState(startupBefore);
    useRuntimeActivityStore.setState(activityBefore);
  }
  return {passed: 6, nativeReplacements: nativeLoads};
}
