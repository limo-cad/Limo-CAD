/** Mirror the in-app File menu into the native macOS menu bar.
 *
 * The Rust side (src-tauri/src/native_menu.rs) rebuilds the default File
 * submenu with one item per in-app entry and emits `native-file-command`
 * payloads; this module routes those payloads through the exact same project
 * actions as the window's File dropdown and pushes enabled-state back so both
 * menus agree. Browser/WASM development keeps the in-app menu only. */
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  export3mf,
  exportStep,
  exportStl,
  importStep,
  newProject,
  openProject,
  renameProject,
  saveProject,
} from './files/projectFiles';
import { exportActiveDrawingDxf } from './drawing/export';
import { openScriptFile } from './scripts/workspace';
import { useAppStore } from './store/appStore';

const FILE_COMMANDS = [
  'new',
  'open',
  'open-script',
  'save',
  'save-as',
  'rename',
  'import-step',
  'export-step-all',
  'export-step-selected',
  'export-3mf-all',
  'export-3mf-selected',
  'export-stl-all',
  'export-stl-selected',
  'export-drawing-dxf',
  'export-profile-dxf',
  'settings',
] as const;
type NativeFileCommand = (typeof FILE_COMMANDS)[number];

function isMacOS(): boolean {
  return typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform);
}

export function nativeMacMenuOwnsFileCommands(): boolean {
  return isTauri() && isMacOS();
}

function menuFlags() {
  const state = useAppStore.getState();
  return {
    busy: state.projectBusy || state.solidBusy || state.historyEdit !== null,
    documentOpen: state.document !== null,
    hasBodies: state.solidScene.bodies.length > 0,
    hasSelectedBody: state.selectedBody !== null,
    drawingWorkspace: state.activeTab === 'drawing',
    drawingSheetReady:
      state.drawingDocument.active_sheet_id !== null
      && state.drawingDocument.sheets.some(
        (sheet) => sheet.id === state.drawingDocument.active_sheet_id,
      )
      && !state.drawingSheetSetupOpen,
  };
}

/** Same guard + error surface as the in-app menu's click handler. */
function run(action: () => Promise<unknown> | void): void {
  const state = useAppStore.getState();
  if (state.projectBusy || state.solidBusy || state.historyEdit) return;
  useAppStore.getState().setProjectBusy(true);
  Promise.resolve()
    .then(() => action())
    .catch((error) => {
      useAppStore.getState().setConstraintDialog({
        titleKey: 'file.errorTitle',
        message: error instanceof Error ? error.message : String(error),
      });
    })
    .finally(() => useAppStore.getState().setProjectBusy(false));
}

function dispatch(command: NativeFileCommand): void {
  switch (command) {
    case 'new':
      return run(newProject);
    case 'open':
      return run(openProject);
    case 'open-script':
      return run(openScriptFile);
    case 'save':
      return run(() => saveProject(false));
    case 'save-as':
      return run(() => saveProject(true));
    case 'rename':
      return run(renameProject);
    case 'import-step':
      return run(importStep);
    case 'export-step-all':
      return run(() => exportStep(false));
    case 'export-step-selected':
      return run(() => exportStep(true));
    case 'export-3mf-all':
      return run(() => export3mf(false));
    case 'export-3mf-selected':
      return run(() => export3mf(true));
    case 'export-stl-all':
      return run(() => exportStl(false));
    case 'export-stl-selected':
      return run(() => exportStl(true));
    case 'export-drawing-dxf':
      return run(exportActiveDrawingDxf);
    case 'export-profile-dxf':
      return run(() => {
        useAppStore.getState().setDrawingProfileExportOpen(true);
      });
    case 'settings':
      useAppStore.getState().setSettingsOpen(true);
      return;
  }
}

/** Install once at the App boundary, next to installNativeEditMenu. */
export function installNativeFileMenu(): () => void {
  if (!nativeMacMenuOwnsFileCommands()) return () => {};

  let disposed = false;
  let unlisten: UnlistenFn | null = null;
  let lastState = '';
  let syncQueue = Promise.resolve();

  const sync = () => {
    const next = menuFlags();
    const key = JSON.stringify(next);
    if (key === lastState) return;
    lastState = key;
    syncQueue = syncQueue
      .then(() => invoke<void>('native_file_menu_set_state', next))
      .catch(() => {
        // Allow a later state change to retry after startup or teardown races.
        lastState = '';
      });
  };

  const unsubscribeStore = useAppStore.subscribe(sync);
  void listen<string>('native-file-command', (event) => {
    if ((FILE_COMMANDS as readonly string[]).includes(event.payload)) {
      dispatch(event.payload as NativeFileCommand);
    }
  }).then((stop) => {
    if (disposed) stop();
    else unlisten = stop;
  }).catch(() => {});
  sync();

  return () => {
    disposed = true;
    unsubscribeStore();
    unlisten?.();
  };
}
