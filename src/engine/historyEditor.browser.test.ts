// Drive the rendered Timeline and Browser controller; only native IPC is simulated.
import { createElement } from 'react';
import { flushSync } from 'react-dom';
import { createRoot } from 'react-dom/client';
import { Timeline } from '../components/Timeline';
import { beginTimelineFeatureEdit, cancelTimelineFeatureEdit, editSketch } from './controller';
import { useAppStore } from '../store/appStore';
import type { DatumPlaneDefinitionDto, DocumentDto, SketchDto } from './types';

export async function checkHistoryEditorCallbacks() {
  const check = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };
  const initial = useAppStore.getState();
  const basis = {origin: [0, 0, 0], u: [1, 0, 0], v: [0, 1, 0], normal: [0, 0, 1]} as SketchDto['basis'];
  const sketch: SketchDto = {name: 'Sketch A', plane: {type: 'origin_plane', plane: 'xy'}, basis,
    entities: [], constraints: [], reference_midpoints: [], dimensions: [], dimension_style: 'aligned',
    dof: {value: 0, fully_defined: true}, can_undo: false, can_redo: false};
  const plane: DatumPlaneDefinitionDto = {feature_id: 1, name: 'Plane A', datum_id: 1, basis,
    source: {type: 'offset', reference: {type: 'origin_plane', plane: 'xy'}, distance: 5}};
  let kind: DocumentDto['features'][number]['kind'] = 'construction_plane';
  let rollback = 1;
  const documentA = (): DocumentDto => ({name: 'Editor A', settings: {units: 'mm'}, rollback_index: rollback,
    features: [{id: 1, name: kind === 'sketch' ? sketch.name : plane.name, kind,
      suppressed: false, status: {state: 'ok'}}], browser: []});
  const scene = {bodies: [], errors: []};
  const counts = new Map<string, number>();
  let gate: {command: string; occurrence: number; entered(): void; wait: Promise<void>} | undefined;
  const arm = (command: string, occurrence: number) => {
    let release!: () => void;
    let entered!: () => void;
    const ready = new Promise<void>(resolve => { entered = resolve; });
    gate = {command, occurrence, entered, wait: new Promise<void>(resolve => { release = resolve; })};
    return {ready, release};
  };
  const w = window as typeof window & {__TAURI_INTERNALS__?: {invoke(command: string, args?: Record<string, unknown>): Promise<unknown>}};
  const native = w.__TAURI_INTERNALS__;
  w.__TAURI_INTERNALS__ = {async invoke(command, args = {}) {
    const occurrence = (counts.get(command) ?? 0) + 1;
    counts.set(command, occurrence);
    if (gate?.command === command && gate.occurrence === occurrence) {
      const pending = gate; gate = undefined; pending.entered(); await pending.wait;
    }
    let value: unknown;
    if (command === 'engine_solid_set_rollback') {
      rollback = JSON.parse(args.payload as string).rollback_index;
      value = {document: documentA(), scene};
    } else if (command === 'engine_datum_plane_definitions') value = [plane];
    else if (command === 'engine_edit_sketch') value = sketch;
    else if (command === 'engine_finished_sketches' || command === 'engine_body_appearances') value = [];
    else if (command === 'engine_assembly_document') value = initial.assemblyDocument;
    else if (command === 'engine_assembly_solution') value = initial.assemblySolution;
    else throw new Error(`Unexpected editor command: ${command}`);
    return JSON.stringify({ok: true, value});
  }};
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  const frame = () => new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
  const loadA = () => {
    rollback = 1;
    useAppStore.setState({activeProjectTabId: 'same-editor-tab', projectBusy: false});
    useAppStore.getState().loadProjectState({document: documentA(), scene}, [], [plane], null);
    useAppStore.setState({solidBusy: false});
  };
  const loadB = () => {
    useAppStore.getState().loadProjectState({document: {...documentA(), name: 'Replacement B'}, scene}, [], [], null);
    useAppStore.setState({solidBusy: true, dirty: true, selectedEntity: 91, hoveredEntity: 92,
      constraintDialog: {titleKey: 'constraints.invalidTitle', message: 'Replacement notice'}});
    return useAppStore.getState();
  };
  try {
    for (const phase of ['datum', 'sketch', 'finished-sketches', 'browser-sketch'] as const) {
      kind = phase === 'datum' ? 'construction_plane' : 'sketch';
      loadA();
      flushSync(() => root.render(createElement(Timeline)));
      await frame();
      counts.clear();
      const pending = arm(phase === 'datum' ? 'engine_datum_plane_definitions'
        : phase === 'finished-sketches' ? 'engine_finished_sketches' : 'engine_edit_sketch',
      phase === 'datum' || phase === 'finished-sketches' ? 2 : 1);
      let browserEdit: Promise<void> | undefined;
      if (phase === 'browser-sketch') browserEdit = editSketch(sketch.name);
      else {
        const feature = container.querySelector<HTMLButtonElement>('[data-feature-id="1"]');
        check(feature && !feature.disabled, 'The actual Timeline feature must be editable');
        feature!.dispatchEvent(new MouseEvent('dblclick', {bubbles: true}));
      }
      let timeout: ReturnType<typeof setTimeout> | undefined;
      try {
        await Promise.race([pending.ready, new Promise<never>((_, reject) => {
          timeout = setTimeout(() => reject(new Error(`The ${phase} callback did not reach its native gate: ${JSON.stringify([...counts])}`)), 5000);
        })]);
      } finally { clearTimeout(timeout); }
      const rollbackCalls = counts.get('engine_solid_set_rollback') ?? 0;
      const replacement = loadB();
      pending.release();
      await browserEdit;
      // Let the actual fire-and-forget Timeline handler and React effects settle.
      await frame();
      check(useAppStore.getState().document === replacement.document
        && useAppStore.getState().activeSketch === replacement.activeSketch
        && useAppStore.getState().finishedSketches === replacement.finishedSketches
        && useAppStore.getState().mode === replacement.mode
        && useAppStore.getState().constructionPlaneDialog === replacement.constructionPlaneDialog
        && useAppStore.getState().constraintDialog === replacement.constraintDialog
        && useAppStore.getState().selectedEntity === 91 && useAppStore.getState().hoveredEntity === 92
        && useAppStore.getState().solidBusy && useAppStore.getState().dirty,
      `A delayed ${phase} editor callback must leave the replacement interface intact`);
      check((counts.get('engine_solid_set_rollback') ?? 0) === rollbackCalls,
        'The abandoned editor must not compensate against the replacement model');
      useAppStore.setState({solidBusy: false});
      await beginTimelineFeatureEdit(1, () => undefined);
      check(useAppStore.getState().historyEdit !== null, 'A stale callback must release its history guard');
      await cancelTimelineFeatureEdit(() => undefined);
    }
    kind = 'sketch';
    loadA();
    counts.clear();
    await editSketch(sketch.name);
    check(useAppStore.getState().activeSketch?.name === sketch.name && useAppStore.getState().mode === 'sketch',
      'A current Browser sketch edit must still publish its sketch state');
    check(counts.get('engine_edit_sketch') === 1 && counts.get('engine_finished_sketches') === 1
      && counts.size === 2, 'Editor ownership guards must not add native calls');
    return {datumReply: true, sketchReply: true, finishedSketchReply: true, browserSketch: true,
      replacementUiPreserved: true, historyReleased: true, nativeCallsUnchanged: true};
  } finally {
    root.unmount(); container.remove(); useAppStore.setState(initial);
    if (native) w.__TAURI_INTERNALS__ = native; else delete w.__TAURI_INTERNALS__;
  }
}
