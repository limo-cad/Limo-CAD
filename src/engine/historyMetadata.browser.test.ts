// Exercise the production history controller and Browser eye handler; only IPC is simulated.
import { beginTimelineFeatureEdit, setTimelineRollback, submitConstructionPlane, submitSolidFillet } from './controller';
import { useAppStore } from '../store/appStore';
import { projectTransitions } from '../files/projectTransitions';
import type { BodyAppearance, BodyDto, DocumentDto, ProjectVisibilityDto } from './types';

export async function checkHistoryMetadata() {
  const check = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };
  const initial = useAppStore.getState();
  const body = (id: number): BodyDto => ({id, name: `Body${id}`, feature_id: id,
    mesh: {positions: [], normals: [], indices: []}, faces: [], edges: []});
  const appearance = (body_id: number): BodyAppearance => ({body_id, color: {r: 200, g: 40, b: 40, a: 255},
    material_name: `Material ${body_id}`, filament_type: 'PETG', brand: 'Generic', color_name: 'Red',
    filament_id: null, preset_id: null, density_g_cm3: null, diameter_mm: 1.75});
  let generation = 0;
  let documentName = 'History metadata';
  const retained = [1, 2];
  let rollback = 2;
  let appearances = retained.map(appearance);
  let visibility: ProjectVisibilityDto = {hidden_body_ids: [2], hidden_datum_plane_ids: [], hidden_sketch_names: []};
  const document = (): DocumentDto => ({name: documentName, settings: {units: 'mm'}, rollback_index: rollback,
    features: retained.map(id => ({id, name: `Extrude${id}`, kind: 'extrude', suppressed: false, status: {state: 'ok'}})),
    browser: retained.slice(0, rollback).map(id => ({id: generation * 100 + id, kind: 'body', name: `Body${id}`,
      reference_id: id, visible: true, children: []}))});
  const scene = () => ({bodies: retained.slice(0, rollback).map(body), errors: []});
  const update = () => ({document: document(), scene: scene()});
  const ok = (value: unknown) => JSON.stringify({ok: true, value});
  const calls: string[] = [];
  let metadataGate: {entered(): void; wait: Promise<void>} | undefined;
  const w = window as typeof window & {__TAURI_INTERNALS__?: {invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>}};
  w.__TAURI_INTERNALS__ = {async invoke(command, args = {}) {
    calls.push(command);
    if (command === 'engine_solid_set_rollback') {
      rollback = JSON.parse(args.payload as string).rollback_index; generation++;
      return ok(update());
    }
    if (command === 'engine_datum_plane_edit') return ok({document: document(), planes: []});
    if (command === 'engine_solid_edit_fillet') return ok(update());
    if (command === 'get_document') return document();
    if (command === 'engine_solid_scene') return ok(scene());
    if (command === 'engine_active_sketch') return ok(null);
    if (command === 'engine_drawing_document') return ok(initial.drawingDocument);
    if (command === 'engine_body_appearances') {
      const captured = appearances;
      if (metadataGate) { const gate = metadataGate; metadataGate = undefined; gate.entered(); await gate.wait; }
      return ok(captured);
    }
    if (command === 'engine_project_visibility') return ok(visibility);
    if (command === 'engine_project_set_visibility') {
      const desired = JSON.parse(args.payload as string) as ProjectVisibilityDto;
      visibility = {...desired, hidden_body_ids: desired.hidden_body_ids.filter(id => retained.includes(id))};
      return ok(visibility);
    }
    if (command === 'engine_finished_sketches' || command === 'engine_datum_plane_definitions') return ok([]);
    if (command === 'engine_assembly_document') return ok(initial.assemblyDocument);
    if (command === 'engine_assembly_solution') return ok(initial.assemblySolution);
    throw new Error(`Unexpected history metadata command: ${command}`);
  }};
  useAppStore.setState({activeProjectTabId: 'history-metadata', mode: 'solid', solidBusy: false, projectBusy: false, historyEdit: null});
  useAppStore.getState().loadProjectState(update(), [], [], null, appearances, initial.drawingDocument,
    initial.assemblyDocument, visibility, initial.assemblySolution);
  try {
    await setTimelineRollback(1);
    await setTimelineRollback(2);
    check(useAppStore.getState().bodyAppearances.find(entry => entry.body_id === 2)?.material_name === 'Material 2',
      'Returning to a later history stage must restore the native body material');
    check(useAppStore.getState().hidden[generation * 100 + 2],
      'Returning body visibility must follow its stable body ID after Browser node IDs change');

    await setTimelineRollback(1);
    useAppStore.getState().toggleHidden(generation * 100 + 1);
    check(useAppStore.getState().projectVisibility.hidden_body_ids.join(',') === '1,2',
      'Toggling an eye at an earlier stage must preserve the later hidden body');
    await setTimelineRollback(2);
    check(useAppStore.getState().projectVisibility.hidden_body_ids.join(',') === '1,2',
      'History hydration must preserve an eye change that has not reached native publication yet');
    check(useAppStore.getState().hidden[generation * 100 + 1] && useAppStore.getState().hidden[generation * 100 + 2],
      'Both retained choices must apply to the current Browser nodes');
    const readsBeforeOrdinaryUpdate = calls.filter(command => command === 'engine_body_appearances').length;
    useAppStore.getState().applySolidUpdate(update());
    await new Promise<void>(resolve => setTimeout(resolve, 0));
    check(calls.filter(command => command === 'engine_body_appearances').length === readsBeforeOrdinaryUpdate,
      'Ordinary solid updates must not add metadata IPC reads');

    const gateRead = () => {
      let release!: () => void;
      let entered!: () => void;
      const ready = new Promise<void>(resolve => { entered = resolve; });
      metadataGate = {entered, wait: new Promise<void>(resolve => { release = resolve; })};
      return {ready, release};
    };
    const pollGate = gateRead();
    const duringPoll = setTimelineRollback(1);
    await pollGate.ready;
    const finishPoll = projectTransitions.begin();
    pollGate.release();
    await new Promise<void>(resolve => setTimeout(resolve, 0));
    finishPoll(false);
    await duringPoll;
    check(!useAppStore.getState().solidBusy && useAppStore.getState().document?.rollback_index === 1,
      'An unchanged inbox poll must not strand the original document busy or discard its history update');

    const materialGate = gateRead();
    const pendingStage = setTimelineRollback(1);
    await materialGate.ready;
    appearances = appearances.map(entry => ({...entry, material_name: 'Newer material'}));
    useAppStore.getState().setBodyAppearances(appearances);
    materialGate.release();
    await pendingStage;
    check(useAppStore.getState().bodyAppearances.every(entry => entry.material_name === 'Newer material'),
      'A delayed metadata read must not overwrite a newer material edit in the same document');

    await setTimelineRollback(2);
    const inboxGate = gateRead();
    let staleEditorOpened = false;
    const beforeInbox = beginTimelineFeatureEdit(2, () => { staleEditorOpened = true; });
    await inboxGate.ready;
    documentName = 'Edited by the current inbox';
    appearances = appearances.map(entry => ({...entry, material_name: 'Inbox material'}));
    await useAppStore.getState().refreshAfterInboxApply('set_body_appearance');
    const inboxState = useAppStore.getState();
    inboxGate.release();
    await beforeInbox;
    check(useAppStore.getState().document === inboxState.document
      && useAppStore.getState().bodyAppearances === inboxState.bodyAppearances && !staleEditorOpened,
      'A later mutation of the same document must supersede stale stage metadata');
    check(!useAppStore.getState().solidBusy && useAppStore.getState().historyEdit === null,
      'A same-document inbox mutation must still release the original stage busy state and editor guard');

    await setTimelineRollback(2);
    const replacementGate = gateRead();
    let editorOpened = false;
    const pendingEditor = beginTimelineFeatureEdit(2, () => { editorOpened = true; });
    await replacementGate.ready;
    const rollbackCalls = calls.filter(command => command === 'engine_solid_set_rollback').length;
    const replacement = {...update(), document: {...document(), name: 'Replacement'}};
    const replacementAppearances = retained.map(id => ({...appearance(id), material_name: 'Replacement material'}));
    const replacementVisibility = {...visibility, hidden_body_ids: [1]};
    useAppStore.getState().loadProjectState(replacement, [], [], null, replacementAppearances,
      initial.drawingDocument, initial.assemblyDocument, replacementVisibility, initial.assemblySolution);
    useAppStore.setState({solidBusy: true, dirty: true});
    const replacementState = useAppStore.getState();
    replacementGate.release();
    await pendingEditor;
    check(calls.filter(command => command === 'engine_solid_set_rollback').length === rollbackCalls,
      'A stale editor hydration must not issue a compensating rollback against the replacement');
    check(!editorOpened && useAppStore.getState().document === replacementState.document,
      'A stale editor hydration must not open or overwrite the replacement document');
    check(useAppStore.getState().bodyAppearances === replacementState.bodyAppearances
      && useAppStore.getState().projectVisibility === replacementState.projectVisibility
      && useAppStore.getState().historyEdit === null && useAppStore.getState().solidBusy
      && useAppStore.getState().dirty && useAppStore.getState().constraintDialog === replacementState.constraintDialog,
      'Stale history completion must leave replacement metadata, dirty state, busy state and dialogs intact');

    for (const kind of ['construction', 'fillet'] as const) {
      rollback = 2;
      useAppStore.getState().loadProjectState(update(), [], [], null, appearances,
        initial.drawingDocument, initial.assemblyDocument, visibility, initial.assemblySolution);
      useAppStore.setState({solidBusy: false});
      await beginTimelineFeatureEdit(2, () => undefined);
      check(useAppStore.getState().historyEdit?.featureId === 2, 'A fresh editor must start after abandoned history work');
      const resumeGate = gateRead();
      const submission = kind === 'construction'
        ? submitConstructionPlane({source: {type: 'offset', reference: {type: 'origin_plane', plane: 'xy'}, distance: 10}}, 2)
        : submitSolidFillet({body_id: 1, edge_ids: [1], radius: 1, tangent_chain: false}, 2);
      await resumeGate.ready;
      useAppStore.getState().loadProjectState(replacement, [], [], null, replacementAppearances,
        initial.drawingDocument, initial.assemblyDocument, replacementVisibility, initial.assemblySolution);
      useAppStore.setState({solidBusy: true, dirty: true});
      const beforeCompletion = useAppStore.getState();
      resumeGate.release();
      await submission;
      check(useAppStore.getState().document === beforeCompletion.document
        && useAppStore.getState().bodyAppearances === beforeCompletion.bodyAppearances
        && useAppStore.getState().projectVisibility === beforeCompletion.projectVisibility
        && useAppStore.getState().constraintDialog === beforeCompletion.constraintDialog
        && useAppStore.getState().solidBusy && useAppStore.getState().dirty,
        `A delayed ${kind} history restore must not change the replacement through outer submission cleanup`);
    }
    return {rollbackMaterial: true, recreatedNodeVisibility: true, offStageEyeChoice: true,
      unpublishedEyeChoice: true, ordinaryUpdatesUnchanged: true, unchangedPoll: true,
      newerMaterialPreserved: true, ordinaryInboxCleanup: true,
      replacementHydrationGuard: true, submissionCleanupGuard: true};
  } finally { useAppStore.setState(initial); delete w.__TAURI_INTERNALS__; }
}
