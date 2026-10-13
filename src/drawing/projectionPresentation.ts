import {getEngine} from '../engine';
import type {DrawingProjectionDto, DrawingProjectionRequest} from '../engine/types';
import {projectTransitions} from '../files/projectTransitions';
import {presentation} from '../operationPlayback';
import {useAppStore} from '../store/appStore';
import {beginRuntimeActivity} from '../runtimeActivity';

/** UI linework always comes from the normal native projection path. A running
 * script supplies its completed explicit projections in the existing control
 * reply, so background HLR cannot seize the engine between edit and publish. */
export function captureDrawingProjectionScope() {
  const state = useAppStore.getState();
  return {version: presentation.documentVersion(), tab: state.activeProjectTabId,
    scene: state.solidScene, solution: state.assemblySolution};
}
type Scope = ReturnType<typeof captureDrawingProjectionScope>;
const sameScope = (a: Scope, b: Scope) => a.version === b.version && a.tab === b.tab
  && a.scene === b.scene && a.solution === b.solution;
export const isDrawingProjectionScopeCurrent = (scope: Scope) => sameScope(scope, captureDrawingProjectionScope());
const current = isDrawingProjectionScopeCurrent;
let scope: Scope | null = null;
let owner: {sessionId: string; documentId: string | null; revision: number | undefined} | null = null;
const cache = new Map<string, {projection: DrawingProjectionDto; bytes: number}>();
const listeners = new Set<() => void>();
let bytes = 0;
let automaticRunning = false;
let automaticHolds = 0;
const notify = () => { for (const listener of listeners) listener(); };

/** A UI control may finish playback before its native acknowledgment settles.
 * Keep new background HLR out of that interval without blocking file transitions
 * or delivery of already completed native projections. */
export function holdAutomaticDrawingProjections(): () => void {
  automaticHolds++;
  let released = false;
  return () => {
    if (released) return;
    released = true;
    automaticHolds--;
    notify();
  };
}

function refreshScope() {
  const next = captureDrawingProjectionScope();
  if (!scope || !sameScope(scope, next)) { scope = next; owner = null; cache.clear(); bytes = 0; }
}

// Normalize only serialized defaults; the native host resolves assembly poses.
// Preserve order and exact numeric values, including the requested deflection.
function key(request: DrawingProjectionRequest): string {
  const section = request.section_plane;
  return JSON.stringify([request.scope ?? 'definition', request.occurrence_ids ?? [], request.body_ids ?? [],
    request.direction, request.up, request.include_hidden ?? false, request.include_tangent_edges ?? false,
    request.deflection ?? 0.05, section ? [section.point, section.normal, section.depth ?? null] : null]);
}

function remember(requestKey: string, projection: DrawingProjectionDto) {
  const size = JSON.stringify(projection).length * 2;
  const old = cache.get(requestKey);
  if (old) { bytes -= old.bytes; cache.delete(requestKey); }
  cache.set(requestKey, {projection, bytes: size}); bytes += size;
  // Deliver a large result to its mounted view, without retaining it afterward.
  while (cache.size > 12 || (bytes > 16 * 1024 * 1024 && cache.size > 1)) {
    const first = cache.keys().next().value!;
    bytes -= cache.get(first)!.bytes; cache.delete(first);
  }
  notify();
  if (size > 16 * 1024 * 1024) { cache.delete(requestKey); bytes -= size; }
}

/** Called only after the exact reserved snapshot has been written. */
export function bindDrawingProjectionOwner(sessionId: string, documentId: string | null, revision?: number) {
  refreshScope();
  if (owner && (owner.sessionId !== sessionId || owner.documentId !== documentId)) { cache.clear(); bytes = 0; }
  owner = {sessionId, documentId, revision};
}

export interface CompletedDrawingProjection {
  session_id: string;
  document_id: string;
  engine_revision: number;
  drawing_projections: {request: DrawingProjectionRequest; projection: DrawingProjectionDto}[];
}

/** Native stamped this reply under the publisher/engine lock. Recheck the
 * frontend owner captured before polling; even a same-tab Open retires it. */
export function acceptDrawingProjection(reply: CompletedDrawingProjection, before: Scope): boolean {
  refreshScope();
  if (!current(before) || !projectTransitions.isSettled() || !owner
    || owner.documentId !== reply.document_id || scope?.tab !== reply.document_id
    || owner.sessionId !== reply.session_id || owner.revision !== reply.engine_revision) return false;
  for (const {request, projection} of reply.drawing_projections) remember(key(request), projection);
  return true;
}

function automaticAllowed() {
  const playback = presentation.snapshot();
  return automaticHolds === 0 && projectTransitions.isSettled()
    && !(playback.active && !playback.finished && !playback.stopped);
}

/** Cancellable observer, with one background native read at a time. Pause
 * retains script ownership: starting cold HLR there could block Resume's edit. */
export function observeDrawingProjection(request: DrawingProjectionRequest,
  receive: (projection: DrawingProjectionDto) => void, failed: (error: unknown) => void): () => void {
  const captured = captureDrawingProjectionScope();
  const requestKey = key(request);
  let cancelled = false, delivered: DrawingProjectionDto | null = null, failedRead = false;
  const pump = () => {
    if (cancelled || !current(captured)) return;
    refreshScope();
    const cached = cache.get(requestKey)?.projection;
    if (cached) { if (cached !== delivered) { delivered = cached; receive(cached); } return; }
    if (delivered || failedRead || automaticRunning || !automaticAllowed()) return;
    automaticRunning = true;
    let activity: ReturnType<typeof beginRuntimeActivity> | null = null;
    void getEngine().then(async engine => {
      // A script/transition can start while the adapter is being loaded.
      if (cancelled || !current(captured) || !automaticAllowed()) return;
      activity = beginRuntimeActivity({kind: 'drawing', target: useAppStore.getState().document?.name});
      activity.setPhase('geometry');
      const result = await engine.drawingProjection(request);
      if (!cancelled && current(captured)) { remember(requestKey, result); activity.finish(); }
      else activity.discard();
    }).catch(error => {
      if (!cancelled && current(captured)) { activity?.fail(error); failedRead = true; failed(error); }
      else activity?.discard();
    }).finally(() => { activity?.discard(); automaticRunning = false; notify(); });
  };
  listeners.add(pump);
  const unsubscribeTransitions = projectTransitions.subscribe(pump);
  const unsubscribePlayback = presentation.subscribe(pump);
  pump();
  return () => { cancelled = true; listeners.delete(pump); unsubscribeTransitions(); unsubscribePlayback(); };
}
