import { invoke } from '@tauri-apps/api/core';
import { useAppStore } from './store/appStore';
import { getSessionCamera } from './components/viewport/cameraApi';
import { inspectUi, operateUi, visible, type UiAction } from './uiControl';
import { presentOperation, setPlaybackPace, waitForPlayback } from './operationPlayback';
import { operateUiFile, type UiFileRequest } from './uiFiles';
import {drivePointer, type UiGesture} from './uiPointer';
import {pendingEngineOperations} from './engine/activity';
import { applicationExitBarrier } from './files/applicationExit';

export const viewDirections: Record<string, [number, number, number]> = {
  front: [0, -1, 0], back: [0, 1, 0], left: [-1, 0, 0], right: [1, 0, 0],
  top: [0, 0, 1], bottom: [0, 0, -1],
};
let applying = false;
interface ViewRequest { id: string; session_id: string; view: string; fit: boolean; expires_ms: number; ui?: Omit<UiAction, 'action'> & UiFileRequest & { action: UiAction['action'] | 'window' | 'file' | 'viewport'; pace_ms?: number; mode?: string; canvas?: 'viewport' | 'drawing'; gesture?: UiGesture; point?: [number, number]; to?: [number, number]; world?: [number, number, number]; shift?: boolean } }

/** Called by the existing UI heartbeat loop; never changes the model revision. */
export async function applyLiveUiControl(publishChangedState: () => Promise<void>): Promise<void> {
  if (applying || useAppStore.getState().engineKind !== 'tauri') return;
  applying = true;
  const releaseExit = applicationExitBarrier.hold();
  try {
    const before = useAppStore.getState();
    const document = before.document;
    const request = await invoke<ViewRequest | null>('mcp_session_bridge_control');
    if (!request) return;
    const response: Record<string, unknown> = { request_id: request.id, session_id: request.session_id };
    try {
      if (request.ui) {
        if (request.expires_ms < Date.now()) throw new Error('UI request expired');
        if (request.ui.pace_ms !== undefined) setPlaybackPace(request.ui.pace_ms);
        if (request.ui.action === 'window') {
          response.window = await invoke('mcp_window_control', { mode: request.ui.mode ?? 'inspect' });
        } else if (request.ui.action === 'file') {
          useAppStore.getState().setProjectBusy(true);
          try { response.completed = await operateUiFile(request.ui); }
          finally { useAppStore.getState().setProjectBusy(false); }
          if (!response.completed) throw new Error('File operation did not complete; inspect the UI for details');
          await presentOperation(`File: ${request.ui.command}`);
        } else if (request.ui.action === 'viewport') {
          if ([...window.document.querySelectorAll<HTMLElement>('[aria-modal="true"]')].some(visible)) throw new Error('A modal dialog blocks the viewport');
          const api = getSessionCamera();
          const drawing = request.ui.canvas === 'drawing' ? window.document.querySelector('[data-testid="drawing-sheet"]') : null;
          if (request.ui.canvas === 'drawing' ? !drawing : !api) throw new Error('Requested canvas is unavailable');
          if (drawing && request.ui.world) throw new Error('Drawing canvas uses window pixel coordinates');
          const projected = request.ui.world ? api?.worldToScreen(request.ui.world) : null;
          const point = request.ui.point ?? (projected ? [projected.x, projected.y] as [number, number] : null);
          if (!point) throw new Error('Viewport action requires point or world coordinates');
          if (drawing) await drivePointer(drawing, request.ui.gesture ?? 'click', point, request.ui.shift, request.ui.to);
          else await api!.pointer(request.ui.gesture ?? 'click', point, request.ui.shift, request.ui.to);
          await presentOperation(`Viewport: ${request.ui.gesture ?? 'click'}`);
        } else {
          const target = operateUi(request.ui as UiAction, document);
          if (request.ui.action !== 'inspect') await presentOperation(request.ui.action, target);
        }
        while (pendingEngineOperations() || useAppStore.getState().solidBusy || useAppStore.getState().projectBusy) {
          if ([...window.document.querySelectorAll<HTMLElement>('[aria-modal="true"]')].some(visible)) {
            response.awaiting_input = true;
            break;
          }
          if (Date.now() >= request.expires_ms) throw new Error('UI operation is still busy; inspect before retrying');
          await waitForPlayback(25);
        }
        const after = useAppStore.getState();
        if (after.document !== before.document || after.activeSketch !== before.activeSketch
          || after.drawingDocument !== before.drawingDocument || after.assemblyDocument !== before.assemblyDocument) {
          await publishChangedState();
        }
        response.status = 'applied';
        response.ui = inspectUi(useAppStore.getState().document);
        const state = useAppStore.getState();
        response.state = { mode: state.mode, active_tool: state.activeTool, selected_body: state.selectedBody,
          selected_face: state.selectedFace, selected_edges: state.selectedEdges, selected_entities: state.selectedEntities,
          viewport: getSessionCamera()?.bounds() ?? null };
        response.presented = window.document.visibilityState === 'visible';
        await invoke('mcp_session_bridge_control', { response });
        return;
      }
      if (document !== useAppStore.getState().document || request.expires_ms < Date.now()) {
        throw new Error('Document changed or view request expired');
      }
      const api = getSessionCamera();
      if (!api) throw new Error('Viewport camera is not ready');
      const waitForCamera = async () => {
        while (api.isAnimating()) {
          if (Date.now() >= request.expires_ms) throw new Error('Camera animation did not complete before the request expired');
          if (document !== useAppStore.getState().document || getSessionCamera() !== api) throw new Error('Document or viewport changed');
          await waitForPlayback(25);
        }
      };
      if (request.view === 'isometric') api.home();
      else if (viewDirections[request.view]) api.snapToDirection(viewDirections[request.view]);
      else if (request.view !== 'current') throw new Error('Unknown view');
      await waitForCamera();
      if (document !== useAppStore.getState().document) throw new Error('Document changed');
      if (request.fit) {
        api.fit();
        await waitForCamera();
      }
      if (document !== useAppStore.getState().document) throw new Error('Document changed');
      response.status = 'applied';
      response.camera = api.getSnapshot();
    } catch (error) {
      response.status = 'failed';
      response.error = String(error);
      if (request.ui) response.ui = inspectUi(useAppStore.getState().document);
    }
    await invoke('mcp_session_bridge_control', { response });
  } catch (error) {
    console.debug('[sessionBridge] view request failed', error);
  } finally { applying = false; releaseExit(); }
}
