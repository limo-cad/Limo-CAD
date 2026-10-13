import { initializeStartupDocument } from './startupInitialization';
import { useStartupStore } from './startup';
import { useAppStore } from './store/appStore';
import { getCurrentProjectTarget } from './files/projectTabs';
import type { DocumentDto } from './engine/types';

/** Run on a fresh desktop-mode browser page. Exercise the real document, tab,
 * ownership and recovery flow against a host that forbids rebinding bootstrap. */
export async function checkStartupDocumentRecovery() {
  const check = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };
  const initial = useAppStore.getState();
  const native = window as typeof window & { __TAURI_INTERNALS__?: {
    invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>;
    transformCallback: () => number;
  }};
  const previousNative = native.__TAURI_INTERNALS__;
  const previousSettings = window.__startupMeasurementSettingsPromise;
  const startupBefore = useStartupStore.getState();
  const document: DocumentDto = { name: 'Untitled', settings: { units: 'mm' }, features: [], rollback_index: 0, browser: [] };
  const scene = { bodies: [], errors: [] };
  const model = JSON.stringify({ format: 'nbcad-project', schema_version: 6, document });
  const ok = (value: unknown) => JSON.stringify({ ok: true, value });
  let boundId: string | null = null;
  const bindings: string[] = [];
  let fileReads = 0;
  let resets = 0;
  window.__startupMeasurementSettingsPromise = Promise.resolve({ locale: null, documentPath: 'missing-part.nbcad' });
  native.__TAURI_INTERNALS__ = { transformCallback: () => 1, async invoke(command, args = {}) {
    if (command === 'startup_record') return null;
    if (command === 'get_document') return document;
    if (['engine_finished_sketches', 'engine_datum_plane_definitions', 'engine_body_appearances'].includes(command)) return ok([]);
    if (command === 'engine_solid_scene') return ok(scene);
    if (command === 'engine_drawing_document') return ok(initial.drawingDocument);
    if (command === 'engine_cam_document') return ok(initial.camDocument);
    if (command === 'engine_assembly_document') return ok(initial.assemblyDocument);
    if (command === 'engine_assembly_solution') return ok(initial.assemblySolution);
    if (command === 'engine_project_visibility') return ok(initial.projectVisibility);
    if (command === 'engine_project_export_model') return ok(model);
    if (command === 'engine_project_session_bind') {
      const id = args.sessionId as string;
      if (boundId && boundId !== id) throw new Error('bootstrap project session already bound');
      boundId = id; bindings.push(id); return ok(null);
    }
    if (command === 'native_viewport_set_suspended') return null;
    if (command === 'read_binary_file') { fileReads++; throw new Error('The startup file is missing'); }
    if (command === 'engine_project_new') { resets++; return ok({ document, scene }); }
    throw new Error(`Unexpected native command: ${command}`);
  }};
  try {
    let failed = false;
    try { await initializeStartupDocument(); } catch { failed = true; }
    check(failed && fileReads === 1, 'Exercise a real rejected file open after native tab binding');
    const failedStartup = useStartupStore.getState();
    const retainedId = useAppStore.getState().activeProjectTabId;
    check(retainedId && boundId === retainedId, 'The failed startup must retain its bound native tab');
    check(failedStartup.phase === 'failed' && failedStartup.failureSource === 'document', 'Document failure must expose recovery');
    check(failedStartup.appPrepared && !failedStartup.documentReady,
      'A failed file Open must not erase the separately completed application engine/session setup');
    await initializeStartupDocument(true);
    const recovered = useStartupStore.getState();
    check(resets === 1 && fileReads === 1, 'Empty recovery must reset the engine without reopening the failed file');
    check(bindings.length === 2 && bindings.every(id => id === retainedId), 'Recovery must preserve the native session UUID');
    check(useAppStore.getState().activeProjectTabId === retainedId, 'Recovery must preserve frontend tab ownership');
    check(recovered.documentReady && !recovered.error && recovered.phase === 'renderer', 'Recovery must resume renderer readiness');
    check(recovered.failureSource === 'renderer', 'After document preparation, delayed GPU recovery must target the renderer');
    check(recovered.attempt > failedStartup.attempt, 'Recovery must restart the readiness attempt');
    check(getCurrentProjectTarget() === null, 'The recovered empty workspace must not save over the failed source');
    return { recovered: true, nativeSessionRetained: true, fileReads, resets };
  } finally {
    if (previousNative) native.__TAURI_INTERNALS__ = previousNative;
    else delete native.__TAURI_INTERNALS__;
    window.__startupMeasurementSettingsPromise = previousSettings;
    useAppStore.setState(initial);
    useStartupStore.setState(startupBefore);
  }
}
