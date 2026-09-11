/**
 * Read-only MCP snapshot bridge publisher (Jack §3 model 1).
 *
 * Writes `<NBCAD_SESSION_DIR>/<uuid>/{model.json,focus.json,heartbeat.json}`
 * via Tauri. MCP `cad_submit` writes `inbox/<seq>.json`; this module polls
 * `mcp_session_bridge_apply_inbox` so the live engine applies the op, then
 * the existing publisher emits a new snapshot.
 *
 * Authoritative `engine_revision` advances in native code under the publisher
 * lock (`run_ui_mutation` / inbox apply) — not via a later JS note — so inbox
 * OCC cannot race the UI→JS gap. Reserve captures that revision; write rejects
 * if a mutation landed during export. The bridge is bound to the native
 * project-session identity on tab transitions. Conflicting/malformed heads
 * are dead-lettered. Not in-process shared memory. MCP never writebacks model.json.
 */
import { invoke } from '@tauri-apps/api/core';
import {currentHistoryProjectKey,recordDrawingHistory} from './engine/applicationHistory';
import { listen } from '@tauri-apps/api/event';
import { getEngine } from './engine';
import { applyLiveUiControl } from './liveUiBridge';
import { SerialPlayback, presentOperation, presentation, wakePlayback } from './operationPlayback';
import { getSessionCamera } from './components/viewport/cameraApi';
import { captureSessionSnapshot, synchronizeSnapshotVisibility } from './sessionSnapshot';
import type { SolidUpdateDto } from './engine/types';
import { projectTransitions } from './files/projectTransitions';
import {
  useAppStore,
  type AppMode,
  type SketchTool,
} from './store/appStore';

export type McpFocusPack =
  | 'document'
  | 'sketch'
  | 'solid'
  | 'modify'
  | 'body_ops'
  | 'datums'
  | 'history'
  | 'inspect'
  | 'print';

/** Keep in sync with mcp-server/src/disclosure.rs focus packs. */
export function focusFromUi(
  mode: AppMode,
  activeTool: SketchTool,
  solidDialog: string | null,
): McpFocusPack {
  if (solidDialog) {
    switch (solidDialog) {
      case 'fillet':
      case 'chamfer':
      case 'hole':
        return 'modify';
      case 'shell':
      case 'mirror':
      case 'rectangular_pattern':
      case 'circular_pattern':
      case 'combine':
      case 'split_body':
        return 'body_ops';
      case 'construction_plane':
        return 'datums';
      case 'extrude':
      case 'revolve':
      case 'sweep':
      case 'loft':
      case 'rib':
        return 'solid';
      default:
        return 'solid';
    }
  }
  if (mode === 'sketch') return 'sketch';
  if (mode === 'pickPlane') return 'datums';
  if (mode === 'solid') return 'solid';
  if (activeTool) return 'sketch';
  return 'document';
}

function activeSolidDialog(state: ReturnType<typeof useAppStore.getState>): string | null {
  if (state.filletDialogFeature !== null) return 'fillet';
  if (state.chamferDialogFeature !== null) return 'chamfer';
  if (state.holeDialogFeature !== null) return 'hole';
  if (state.extrudeDialogFeature !== null) return 'extrude';
  if (state.revolveDialogFeature !== null) return 'revolve';
  if (state.sweepDialogFeature !== null) return 'sweep';
  if (state.loftDialogFeature !== null) return 'loft';
  if (state.ribDialogFeature !== null) return 'rib';
  if (state.constructionPlaneDialog) return 'construction_plane';
  if (state.bodyFeatureDialog) return state.bodyFeatureDialog.kind;
  return null;
}

let publishTimer: ReturnType<typeof setTimeout> | null = null;
let heartbeatTimer: ReturnType<typeof setInterval> | null = null;
let inboxTimer: ReturnType<typeof setInterval> | null = null;
let inboxApplying = false;
let started = false;

interface InboxApplyResult {
  applied: boolean;
  dead_lettered?: boolean;
  reason?: string;
  seq?: number;
  name?: string;
  result?: SolidUpdateDto;
  pending?: number;
  error?: string;
}

interface PublishReservation {
  session_id: string;
  generation: number;
  engine_revision?: number;
  project_session_id?: string | null;
}

interface PublishWriteResult {
  skipped: boolean;
  reason?: string;
  session_id?: string;
  generation?: number;
  engine_revision?: number;
}

interface PublishedSession {
  sessionId: string;
  documentId: string | null;
}
let inboxOwner: (PublishedSession & {documentRevision: number}) | null = null;

export async function publishNow(): Promise<boolean> {
  return (await publishCurrentSession()) !== null;
}

/** The identity belongs to the exact reservation whose snapshot write passed,
 * not a later active-tab query that could silently retarget script startup. */
export async function publishCurrentSession(): Promise<PublishedSession | null> {
  const state = useAppStore.getState();
  if (state.engineKind !== 'tauri') return null;
  const documentRevision = presentation.documentVersion();
  const focus = focusFromUi(state.mode, state.activeTool, activeSolidDialog(state));
  try {
    // Reserve captures engine_revision and project/session identity before
    // export. Write carries that identity so a tab switch cannot publish
    // this snapshot into another session. If a UI mutation lands before
    // write, native rejects the stale snapshot and we retry.
    for (let attempt = 0; attempt < 4; attempt += 1) {
      const engine = await getEngine();
      const { reservation, activeSketch, modelJson } = await captureSessionSnapshot({
        synchronizeVisibility: () => synchronizeSnapshotVisibility(
          useAppStore.getState().projectVisibility,
          () => engine.projectVisibility(),
          visibility => engine.setProjectVisibility(visibility),
        ),
        reserve: () => invoke<PublishReservation>('mcp_session_bridge_reserve'),
        activeSketch: () => engine.activeSketch(),
        exportModel: () => engine.exportProjectModel(),
      });
      const written = await invoke<PublishWriteResult>('mcp_session_bridge_write', {
        payload: JSON.stringify({
          focus,
          model_json: modelJson,
          active_sketch_json: activeSketch === null ? null : JSON.stringify(activeSketch),
          generation: reservation.generation,
          session_id: reservation.session_id,
          project_session_id: reservation.project_session_id ?? null,
        }),
      });
      if (
        written?.skipped &&
        (written.reason === 'engine_revision_changed' ||
          written.reason === 'session_identity_mismatch')
      ) {
        continue;
      }
      if (written?.skipped || documentRevision !== presentation.documentVersion()) return null;
      const owner = {
        sessionId: reservation.session_id, documentId: reservation.project_session_id ?? null,
      };
      inboxOwner = {...owner, documentRevision};
      return owner;
    }
  } catch (error) {
    console.debug('[sessionBridge] publish failed', error);
  }
  return null;
}

/** Apply one MCP inbox op on the live engine, then let the publisher run. */
export async function applyInboxNow(): Promise<void> {
  const state = useAppStore.getState();
  if (state.engineKind !== 'tauri' || inboxApplying) return;
  const ownerRevision = presentation.documentVersion();
  const ownsDocument = () => ownerRevision === presentation.documentVersion();
  // Reuse the exact successfully published identity. Asking native for the
  // current session during a delayed poll could instead return a replacement.
  if (inboxOwner?.documentRevision !== ownerRevision || !inboxOwner.documentId) return;
  const owner = {documentId: inboxOwner.documentId, sessionId: inboxOwner.sessionId};
  if (presentation.snapshot().stopped) {
    inboxApplying = true;
    try {
      await invoke('mcp_session_bridge_apply_inbox', {...owner, rejectReason: 'Playback stopped'});
    } catch (error) {
      console.debug('[sessionBridge] inbox reject failed', error);
    } finally {
      inboxApplying = false;
    }
    return;
  }
  // Control requests stay responsive in their own lane; no paused promise
  // occupies the serial lane and prevents Resume or Stop from reaching it.
  if (!presentation.canApply()) return;
  // inboxApplying also suppresses a second engine_revision bump if any store
  // subscription still notes mutations: native apply already advanced it.
  inboxApplying = true;
  const releaseTransition = projectTransitions.begin();
  let changed = true; // An uncertain native failure must invalidate captures.
  let published = false;
  try {
    const drawingBefore=useAppStore.getState().drawingDocument;
    const drawingProject=currentHistoryProjectKey();
    const result = await invoke<InboxApplyResult>('mcp_session_bridge_apply_inbox', owner);
    if (!ownsDocument()) return;
    changed = Boolean(result?.applied);
    if (result?.dead_lettered) {
      console.warn('[sessionBridge] inbox op dead-lettered; queue unblocked', result);
      // Keep polling so the next sequence can apply on a subsequent tick.
      return;
    }
    if (!result?.applied) return;
    presentation.modelApplied();
    if (publishTimer) { clearTimeout(publishTimer); publishTimer = null; }
    try {
      if (result.result?.scene && result.result.document && !result.name?.startsWith('sketch_')) {
        useAppStore.getState().applySolidUpdate(result.result);
      } else {
        // Targeted / live refresh with dirty:true — never loadDocument (clears dirty).
        await useAppStore.getState().refreshAfterInboxApply(result.name, ownerRevision);
      }
      if (!ownsDocument()) return;
      published = true;
      if(result.name?.startsWith('drawing_')&&result.name!=='drawing_select_sheet') {
        recordDrawingHistory(drawingProject,drawingBefore,useAppStore.getState().drawingDocument);
      }
      if (result.name === 'sketch_begin') presentation.emphasize();
      if (result.name?.startsWith('solid_')) {
        const beforeIds = new Set(state.solidScene.bodies.map(body => body.id));
        const created = useAppStore.getState().solidScene.bodies.filter(body => !beforeIds.has(body.id));
        if (created.length) presentation.emphasize(created.map(body => body.id));
      }
    } finally {
      // Native already archived the seq and bumped engine_revision. Publish
      // even if leftover store refresh throws so cad_refresh sees the live
      // engine. Next applyInboxNow is a no-op on the archived seq.
      if (ownsDocument()) await presentOperation(result.name ?? 'Model operation');
      // Sequential scripts need this result before their next operation.
      // Publishing now removes the old 300 ms debounce from every command.
      if (ownsDocument() && !await publishNow() && ownsDocument()) scheduleSessionBridgePublish();
    }
  } catch (error) {
    console.debug('[sessionBridge] inbox apply failed', error);
  } finally {
    // The replacement has its own transition/publication. A late result from
    // its predecessor must neither invalidate nor bless the current model.
    releaseTransition(ownsDocument() && changed, published);
    inboxApplying = false;
  }
}

/** Lightweight keep-alive — does not re-export the model or bump generation. */
async function heartbeatNow(): Promise<void> {
  const state = useAppStore.getState();
  if (state.engineKind !== 'tauri') return;
  try {
    await invoke('mcp_session_bridge_heartbeat');
  } catch (error) {
    console.debug('[sessionBridge] heartbeat failed', error);
  }
}

export function scheduleSessionBridgePublish(): void {
  if (publishTimer) clearTimeout(publishTimer);
  publishTimer = setTimeout(() => {
    publishTimer = null;
    void publishNow();
  }, 300);
}

export function startSessionBridge(): void {
  if (started) return;
  started = true;
  useAppStore.subscribe((state, prev) => {
    if (
      state.document !== prev.document ||
      state.solidScene !== prev.solidScene ||
      state.activeSketch !== prev.activeSketch ||
      state.mode !== prev.mode ||
      state.activeTool !== prev.activeTool ||
      state.projectVisibility !== prev.projectVisibility ||
      activeSolidDialog(state) !== activeSolidDialog(prev)
    ) {
      // Native engine commands bump engine_revision under the publisher lock
      // (run_ui_mutation). Do not fire-and-forget a JS note here — that
      // reopens the UI→JS race and would double-count after native apply.
      // inboxApplying still guards applyInboxNow re-entry; refreshAfterInboxApply
      // keeps dirty:true (never loadDocument).
      if (!inboxApplying) scheduleSessionBridgePublish();
    }
  });
  scheduleSessionBridgePublish();
  if (heartbeatTimer) clearInterval(heartbeatTimer);
  heartbeatTimer = setInterval(() => {
    void heartbeatNow();
  }, 10_000);
  if (inboxTimer) clearInterval(inboxTimer);
  const playback = new SerialPlayback();
  let tickRequested = false;
  const tick = () => {
    tickRequested = true;
    return playback.tick(async () => {
      do {
        tickRequested = false;
        await applyLiveUiControl(async () => {
          if (publishTimer) { clearTimeout(publishTimer); publishTimer = null; }
          if (!await publishNow()) throw new Error('UI changed, but its snapshot could not be published; inspect before retrying');
        });
        await applyInboxNow();
      } while (tickRequested);
    });
  };
  presentation.subscribe(() => { void tick(); });
  void listen('mcp-work', () => {
    getSessionCamera()?.advanceAnimation();
    wakePlayback();
    void tick();
  });
  void listen('mcp-keepalive', () => { void heartbeatNow(); });
  inboxTimer = setInterval(() => { void tick(); }, 250);
  void tick();
}
