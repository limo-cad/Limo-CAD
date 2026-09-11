// Exercise the actual Save/Open/recovery paths; only desktop IPC is simulated.
import { useAppStore } from '../store/appStore';
import { newProject, renameProject, saveProject } from './projectFiles';
import { operateUiFile } from '../uiFiles';
import { collectRecoverableProjectTabs, getCurrentProjectTarget, recordActiveProjectOpen } from './projectTabs';
import { createNbcadArchive, readNbcadArchive } from './nbcad';
import type { DocumentDto } from '../engine/types';

export async function checkProjectSaveOwnership() {
  const check = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };
  const initial = useAppStore.getState();
  const doc = (name: string): DocumentDto => ({name, settings: {units: 'mm'}, features: [], rollback_index: 0, browser: []});
  const model = (name: string) => JSON.stringify({format: 'nbcad-project', schema_version: 6, document: {name}, part: name});
  const original = model('Original');
  const replacement = model('Replacement');
  let nativeModel = original;
  let nextOpenModel = replacement;
  let failHydration = false;
  let rejectOpen = false;
  let failWrite = false;
  let afterWrite: (() => Promise<void>) | undefined;
  let beforeCapture: (() => Promise<void>) | undefined;
  let beforeGuardedCapture: (() => void) | undefined;
  let beforeRename: (() => void) | undefined;
  let captures = 0;
  let renames = 0;
  const writes: {path: string; bytes: number[]}[] = [];
  let pickerReady!: () => void;
  const pickerOpened = new Promise<void>(resolve => { pickerReady = resolve; });
  let finishPicker!: (path: string) => void;
  const pickedPath = new Promise<string>(resolve => { finishPicker = resolve; });
  const ok = (value: unknown) => JSON.stringify({ok: true, value});
  const requireModel = (expected: string) => {
    if (JSON.stringify(JSON.parse(expected)) !== JSON.stringify(JSON.parse(nativeModel))) {
      throw new Error('The document changed while saving. Start Save again.');
    }
  };
  const w = window as typeof window & {__TAURI_INTERNALS__?: {invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>}};
  w.__TAURI_INTERNALS__ = {async invoke(command, args = {}) {
    if (command === 'read_binary_file') return Array.from(createNbcadArchive(nextOpenModel));
    if (command === 'engine_project_load') {
      if (rejectOpen) return JSON.stringify({ok: false, error: 'Unsupported project schema', data: {project_load_state: 'unchanged'}});
      nativeModel = nextOpenModel;
      return ok({document: doc(JSON.parse(nativeModel).document.name), scene: initial.solidScene});
    }
    if (command === 'engine_document_set_name') {
      beforeRename?.(); beforeRename = undefined;
      const input = JSON.parse(args.payload as string);
      if (typeof input !== 'string') requireModel(input.expected_model_json);
      const name = typeof input === 'string' ? input : input.name;
      renames++;
      const parsed = JSON.parse(nativeModel); parsed.document.name = name; nativeModel = JSON.stringify(parsed);
      return ok(doc(name));
    }
    if (command === 'engine_project_export_model') {
      captures++;
      if (!args.payload) {
        const hook = beforeCapture; beforeCapture = undefined; await hook?.();
        return ok(nativeModel);
      }
      beforeGuardedCapture?.(); beforeGuardedCapture = undefined;
      const input = JSON.parse(args.payload as string);
      requireModel(input.expected_model_json);
      const saved = JSON.parse(nativeModel);
      if (input.save_name) saved.document.name = input.save_name;
      return ok(JSON.stringify(saved));
    }
    if (command === 'engine_finished_sketches' && failHydration) throw new Error('Frontend hydration failed');
    if (['engine_finished_sketches', 'engine_datum_plane_definitions', 'engine_body_appearances'].includes(command)) return ok([]);
    if (command === 'engine_drawing_document') return ok(initial.drawingDocument);
    if (command === 'engine_assembly_document') return ok(initial.assemblyDocument);
    if (command === 'engine_assembly_solution') return ok(initial.assemblySolution);
    if (command === 'engine_project_visibility') return ok(initial.projectVisibility);
    if (command === 'plugin:dialog|save') { pickerReady(); return pickedPath; }
    if (command === 'native_viewport_set_suspended') return null;
    if (command === 'write_binary_file_atomic') {
      if (failWrite) throw new Error('Disk is full');
      writes.push({path: args.path as string, bytes: args.bytes as number[]});
      const hook = afterWrite; afterWrite = undefined; await hook?.();
      return null;
    }
    throw new Error(`Unexpected Save test command: ${command}`);
  }};
  const target = {kind: 'native' as const, path: 'C:/original-copy.nbcad', name: 'original-copy.nbcad'};
  const settle = async (operation: Promise<unknown>) => {
    try { await operation; return ''; } catch (error) { return String(error); }
  };
  const open = async (modelJson = replacement) => {
    nextOpenModel = modelJson;
    return operateUiFile({command: 'open', path: modelJson === original ? 'C:/original.nbcad' : 'C:/replacement.nbcad', discard_changes: true});
  };
  const restore = async () => {
    failHydration = false;
    await open(original);
    useAppStore.setState({dirty: true});
  };
  useAppStore.setState({document: doc('Original'), dirty: true, activeProjectTabId: 'save-tab',
    projectTabs: [{id: 'save-tab', name: 'Original', fileName: 'original.nbcad', dirty: true, workspaceTab: 'solid'}]});
  await recordActiveProjectOpen(original, {kind: 'native', path: 'C:/original.nbcad', name: 'original.nbcad'});
  try {
    useAppStore.setState({solidBusy: true});
    check(/document changed/i.test(await settle(saveProject(true, target))), 'Save must refuse a native edit still publishing its UI');
    check(/document changed/i.test(await settle(renameProject('Busy edit'))), 'Rename must refuse a native edit still publishing its UI');
    check(captures === 0 && renames === 0, 'Busy UI must not seed a snapshot or rename');
    useAppStore.setState({solidBusy: false});
    const pendingSave = settle(saveProject(true));
    await pickerOpened;
    await open();
    finishPicker(target.path);
    check(/document changed/i.test(await pendingSave), 'Save must reject a same-tab MCP Open while its picker is pending');
    check(writes.length === 0, 'Changed-owner Save must not write replacement bytes under the old target');
    check(nativeModel === replacement && useAppStore.getState().document?.name === 'Replacement', 'Save must not rename the replacement owner');

    await restore();
    failHydration = true;
    check(/hydration failed/.test(await settle(open())), 'Test must enter a genuinely unverified native Open');
    const capturesBeforeFailure = captures;
    check(/document changed/i.test(await settle(saveProject())), 'Save after unverified Open must reject before capture');
    check(/document changed/i.test(await settle(renameProject('Wrong name'))), 'Rename after unverified Open must reject');
    check(/document changed/i.test(await settle(newProject())), 'Creating a tab must not snapshot an unverified replacement over the outgoing tab');
    const recovered = await collectRecoverableProjectTabs();
    check(recovered.tabs[0]?.modelJson.trim() === original && recovered.tabs[0]?.name === 'Original', 'Recovery must preserve the last good model and its matching name: ' + JSON.stringify(recovered));
    check(captures === capturesBeforeFailure && renames === 0 && writes.length === 0, 'Unverified ownership must not read or mutate the replacement');

    await restore();
    beforeCapture = async () => { await open(); };
    const racedRecovery = await collectRecoverableProjectTabs();
    check(racedRecovery.tabs[0]?.modelJson.trim() === original && racedRecovery.tabs[0]?.name === 'Original', 'Recovery racing same-tab Open must not pair the new runtime with old metadata');

    await restore();
    beforeCapture = async () => { await open(); };
    check(/document changed/i.test(await settle(saveProject(true, target))), 'Save must reject replacement during initial snapshot capture');
    check(writes.length === 0 && renames === 0, 'A raced snapshot must not reach the write');

    await restore();
    failWrite = true;
    check(/Disk is full/.test(await settle(saveProject(true, target))), 'Write failure must remain actionable');
    failWrite = false;
    check(nativeModel === original && useAppStore.getState().dirty && renames === 0, 'Write failure must not mutate or compensate the document name');
    check(getCurrentProjectTarget()?.name === 'original.nbcad', 'Failed Save As must preserve the previous target');

    afterWrite = async () => { await open(); useAppStore.setState({dirty: true}); };
    check(/document changed/i.test(await settle(saveProject(true, target))), 'A saved snapshot must not authorize closing or cleaning a different current document');
    const saved = JSON.parse(readNbcadArchive(Uint8Array.from(writes[0].bytes)).modelJson);
    check(saved.part === 'Original' && saved.document.name === 'original-copy', 'Write must contain the captured original with its requested saved name');
    check(nativeModel === replacement && useAppStore.getState().dirty && renames === 0, 'Delayed write completion must not rename or clean the replacement');
    check(getCurrentProjectTarget()?.name === 'replacement.nbcad', 'Delayed Save must preserve replacement target');

    await restore();
    beforeGuardedCapture = () => { nativeModel = replacement; };
    check(/document changed/i.test(await settle(saveProject(true, target))), 'Native snapshot comparison must reject changes inside the IPC scheduling gap');
    check(writes.length === 1 && renames === 0, 'Native capture guard must fail before IO');

    await restore();
    beforeRename = () => { nativeModel = replacement; };
    check(/document changed/i.test(await settle(saveProject(true, target))), 'Post-write native name adoption must compare the captured model');
    check(nativeModel === replacement && useAppStore.getState().dirty && renames === 0, 'Failed adoption must leave the replacement and dirty flag untouched');

    await restore();
    check(await saveProject(true, target), 'A stable Save As must succeed');
    check(JSON.parse(nativeModel).document.name === 'original-copy' && !useAppStore.getState().dirty, 'Successful Save As must adopt its name and clean only its owner');
    check(getCurrentProjectTarget()?.name === target.name, 'Successful Save As must retain the chosen target');
    await renameProject('Renamed design');
    check(await saveProject(), 'Ordinary Save after Rename must succeed');
    check(JSON.parse(nativeModel).document.name === 'Renamed design' && getCurrentProjectTarget()?.name === target.name, 'Ordinary Save must preserve an explicit model rename and the existing path');
    rejectOpen = true;
    check(/Unsupported project schema/.test(await settle(open())), 'Test must reach a proven unchanged rejected Open');
    check(await saveProject(), 'A proven unchanged rejected Open must leave Save available');
    return {savePickerOpenGuard: true, unverifiedSaveAndRenameGuard: true, unverifiedTabSnapshotGuard: true,
      retainedRecovery: true, captureRace: true, failedWrite: true, delayedWrite: true, nativeSnapshotGuard: true,
      postWriteAdoptionGuard: true, successfulSaveAs: true, renameThenSave: true, rejectedOpenKeepsSave: true, busyNativeEditGuard: true};
  } finally { delete w.__TAURI_INTERNALS__; }
}
