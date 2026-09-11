import {useAppStore} from '../store/appStore';
import {applyInboxNow, publishCurrentSession} from '../sessionBridge';
import {presentation} from '../operationPlayback';
import {projectTransitions} from '../files/projectTransitions';
import type {BodyAppearance, BodyDto, DocumentDto, ProjectVisibilityDto} from '../engine/types';

/** Drive the real inbox dispatch and canonical store hydration through history. */
export async function checkInboxHistoryMetadata() {
  const check = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };
  const initial = useAppStore.getState();
  const appearances: BodyAppearance[] = [1, 2].map(body_id => ({body_id,
    color: {r: 200, g: 40, b: 40, a: 255}, material_name: `Material ${body_id}`,
    filament_type: 'PETG', brand: 'Generic', color_name: 'Red', filament_id: null,
    preset_id: null, density_g_cm3: null, diameter_mm: 1.75}));
  const bodies: BodyDto[] = [1, 2].map(id => ({id, name: `Body${id}`, feature_id: id,
    mesh: {positions: [], normals: [], indices: []}, faces: [], edges: []}));
  let rollback = 2;
  let nextRollback = 0;
  let generation = 0;
  let opName = 'solid_set_rollback';
  let visibility: ProjectVisibilityDto = {hidden_body_ids: [2], hidden_datum_plane_ids: [], hidden_sketch_names: []};
  const document = (): DocumentDto => ({name: 'Inbox history', settings: {units: 'mm'}, rollback_index: rollback,
    features: [1, 2].map(id => ({id, name: `Extrude${id}`, kind: 'extrude', suppressed: false, status: {state: 'ok'}})),
    browser: bodies.slice(0, rollback).map(body => ({id: generation * 100 + body.id, kind: 'body',
      name: body.name, reference_id: body.id, visible: true, children: []}))});
  const scene = () => ({bodies: bodies.slice(0, rollback), errors: []});
  const ok = (value: unknown) => JSON.stringify({ok: true, value});
  let metadataReads = 0;
  let gate: {entered(): void; wait: Promise<void>} | undefined;
  const w = window as typeof window & {__TAURI_INTERNALS__?: {invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>}};
  const previousNative = w.__TAURI_INTERNALS__;
  w.__TAURI_INTERNALS__ = {async invoke(command, args = {}) {
    if (command === 'mcp_session_bridge_reserve') return {session_id: 'history-session', project_session_id: 'history-tab', generation};
    if (command === 'mcp_session_bridge_write') return {skipped: false};
    if (command === 'mcp_session_bridge_apply_inbox') {
      rollback = nextRollback; generation++;
      return {applied: true, name: opName, result: {document: document(), scene: scene()}};
    }
    if (command === 'get_document') return document();
    if (command === 'engine_project_export_model') return ok(JSON.stringify({document: document(), body_appearances: appearances, visibility}));
    if (command === 'engine_project_visibility') return ok(visibility);
    if (command === 'engine_project_set_visibility') { visibility = JSON.parse(args.payload as string); return ok(visibility); }
    if (command === 'engine_active_sketch') return ok(null);
    if (command === 'engine_solid_scene') return ok(scene());
    if (command === 'engine_drawing_document') return ok(initial.drawingDocument);
    if (command === 'engine_assembly_document') return ok(initial.assemblyDocument);
    if (command === 'engine_assembly_solution') return ok(initial.assemblySolution);
    if (command === 'engine_body_appearances') {
      metadataReads++;
      if (gate) { const pending = gate; gate = undefined; pending.entered(); await pending.wait; }
      return ok(appearances);
    }
    if (command === 'engine_finished_sketches' || command === 'engine_datum_plane_definitions') return ok([]);
    throw new Error(`Unexpected inbox history command: ${command}`);
  }};
  try {
    const setup = projectTransitions.begin();
    useAppStore.getState().loadProjectState({document: document(), scene: scene()}, [], [], 'history.nbcad',
      appearances, initial.drawingDocument, initial.assemblyDocument, visibility, initial.assemblySolution);
    useAppStore.setState({engineKind: 'tauri', activeProjectTabId: 'history-tab', solidBusy: false, projectBusy: false});
    setup(true, true);
    presentation.control({command: 'configure', mode: 'fast'});
    check(await publishCurrentSession(), 'Publish the history fixture before native polling');
    const version = presentation.documentVersion();
    await applyInboxNow();
    check(useAppStore.getState().solidScene.bodies.length === 0, 'Inbox rollback must publish its empty scene');
    nextRollback = 2;
    await applyInboxNow();
    check(useAppStore.getState().bodyAppearances.find(entry => entry.body_id === 2)?.material_name === 'Material 2',
      'Inbox forward replay must restore native materials after an empty history stage');
    check(useAppStore.getState().hidden[generation * 100 + 2],
      'Inbox forward replay must restore hidden choices on recreated Browser nodes');

    let entered!: () => void;
    let release!: () => void;
    const ready = new Promise<void>(resolve => { entered = resolve; });
    gate = {entered, wait: new Promise<void>(resolve => { release = resolve; })};
    nextRollback = 0;
    const applying = applyInboxNow();
    await ready;
    const currentBodyNode = useAppStore.getState().document!.browser.find(node => node.reference_id === 1)!;
    useAppStore.getState().toggleHidden(currentBodyNode.id);
    release();
    await applying;
    check(useAppStore.getState().projectVisibility.hidden_body_ids.join(',') === '1,2'
      && visibility.hidden_body_ids.join(',') === '1,2',
      'An eye change during history metadata hydration must survive through the owning snapshot');
    nextRollback = 2;
    await applyInboxNow();
    check(useAppStore.getState().hidden[generation * 100 + 1] && useAppStore.getState().hidden[generation * 100 + 2],
      'Both choices must reach the restored live Browser rows');
    const reads = metadataReads;
    opName = 'solid_extrude';
    await applyInboxNow();
    check(metadataReads === reads, 'Ordinary solid inbox operations must retain their fast path');
    check(presentation.documentVersion() === version, 'History navigation preserves the current document owner');
    return {rollbackForwardMaterials: true, recreatedHiddenNodes: true, eyeChangeDuringHydration: true, ordinaryFastPath: true};
  } finally {
    const restore = projectTransitions.begin();
    useAppStore.setState(initial); presentation.documentChanged(); restore(true, true);
    if (previousNative) w.__TAURI_INTERNALS__ = previousNative; else delete w.__TAURI_INTERNALS__;
  }
}
