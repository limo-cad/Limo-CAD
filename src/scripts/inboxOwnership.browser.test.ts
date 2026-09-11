import {useAppStore} from '../store/appStore';
import {presentation} from '../operationPlayback';
import {applyInboxNow, publishCurrentSession} from '../sessionBridge';
import {projectTransitions} from '../files/projectTransitions';
import type {DocumentDto, SolidUpdateDto} from '../engine/types';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((done, failed) => { resolve = done; reject = failed; });
  return {promise, resolve, reject};
}

/** Delay real inbox-handler IPC/hydration across a successfully published Open. */
export async function checkInboxDocumentOwnership() {
  const check = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };
  const initial = useAppStore.getState();
  const document: DocumentDto = {name: 'A', settings: {units: 'mm'}, features: [], rollback_index: 0, browser: []};
  const update: SolidUpdateDto = {document, scene: {bodies: [], errors: []}};
  const w = window as typeof window & {__TAURI_INTERNALS__?: {invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>}};
  const native = w.__TAURI_INTERNALS__;
  const ok = (value: unknown) => JSON.stringify({ok: true, value});
  const outcomes: string[] = [];
  try {
    for (const phase of ['solid-reply', 'failed-reply', 'drawing-refresh', 'assembly-refresh', 'general-refresh']) {
      useAppStore.getState().loadProjectState(update, [], [], 'A.nbcad');
      useAppStore.setState({engineKind: 'tauri'});
      presentation.control({command: 'configure', mode: 'present'});
      const inbox = deferred<unknown>();
      const queried = deferred<void>();
      const hydration = deferred<unknown>();
      const calls: string[] = [];
      w.__TAURI_INTERNALS__ = {async invoke(command, args = {}) {
        calls.push(command);
        if (command === 'mcp_session_bridge_reserve') return {session_id: 'session-A', project_session_id: 'document-A', generation: 1};
        if (command === 'mcp_session_bridge_write') return {skipped: false};
        if (command === 'engine_project_export_model') return ok(JSON.stringify(update));
        if (command === 'mcp_session_bridge_apply_inbox') {
          check(args.documentId === 'document-A' && args.sessionId === 'session-A', 'Inbox calls must carry their published owner');
          return inbox.promise;
        }
        if (command === 'engine_drawing_document' && phase === 'drawing-refresh') {
          queried.resolve(); return hydration.promise;
        }
        if (command === 'get_document') {
          if (phase === 'assembly-refresh' || phase === 'general-refresh') { queried.resolve(); return hydration.promise; }
          return document;
        }
        if (command === 'engine_drawing_document') return ok(initial.drawingDocument);
        if (command === 'engine_assembly_document') return ok(initial.assemblyDocument);
        if (command === 'engine_assembly_solution') return ok(initial.assemblySolution);
        if (command === 'engine_solid_scene') return ok(update.scene);
        if (command === 'engine_project_visibility') return ok(initial.projectVisibility);
        if (command === 'engine_active_sketch') return ok(null);
        if (['engine_finished_sketches', 'engine_datum_plane_definitions', 'engine_body_appearances'].includes(command)) return ok([]);
        throw new Error(`Retired operation attempted publication or mutation: ${command}`);
      }};
      check(await publishCurrentSession(), 'Publish A before polling');
      calls.length = 0;
      const applying = applyInboxNow();
      if (phase.endsWith('refresh')) {
        inbox.resolve({applied: true, name: phase === 'drawing-refresh' ? 'drawing_add_view'
          : phase === 'assembly-refresh' ? 'assembly_create_joint' : 'sketch_end'});
        await Promise.race([queried.promise, applying.then(() => {
          throw new Error(`The expected ${phase} query never arrived: ${calls.join(', ')}`);
        })]);
      }
      const releaseReplacement = projectTransitions.begin();
      useAppStore.getState().loadProjectState({...update, document: {...document, name: 'B'}}, [], [], 'B.nbcad');
      releaseReplacement(true, true);
      const revisionB = projectTransitions.capture();
      const stateB = JSON.stringify(useAppStore.getState());
      const playbackB = JSON.stringify(presentation.snapshot());
      if (phase === 'failed-reply') inbox.reject(new Error('Old native reply lost'));
      else if (phase === 'solid-reply') inbox.resolve({applied: true, name: 'solid_extrude', result: update});
      else hydration.resolve(phase === 'drawing-refresh' ? ok(initial.drawingDocument) : document);
      await applying;
      check(JSON.stringify(useAppStore.getState()) === stateB, `Late ${phase} must not alter the replacement document or dirty state`);
      check(JSON.stringify(presentation.snapshot()) === playbackB, `Late ${phase} must not update replacement playback`);
      check(!calls.some(call => call === 'mcp_session_bridge_reserve' || call === 'mcp_session_bridge_write'),
        `Late ${phase} must not publish a snapshot for the retired operation`);
      await projectTransitions.assertCurrent(revisionB);
      outcomes.push(phase);
    }
    for (const stopped of [false, true]) {
      useAppStore.getState().loadProjectState(update, [], [], 'A.nbcad');
      let nativeSessionId = 'session-A';
      let pendingB = 1;
      const delayedPoll = deferred<void>();
      let pollCount = 0;
      w.__TAURI_INTERNALS__ = {async invoke(command, args = {}) {
        if (command === 'mcp_session_bridge_reserve') return {session_id: nativeSessionId, project_session_id: 'same-tab', generation: 1};
        if (command === 'mcp_session_bridge_write') return {skipped: false};
        if (command === 'engine_project_export_model') return ok(JSON.stringify(update));
        if (command === 'engine_project_visibility') return ok(initial.projectVisibility);
        if (command === 'engine_active_sketch') return ok(null);
        if (command === 'mcp_session_bridge_apply_inbox') {
          pollCount++;
          if (args.sessionId === 'session-A') {
            check(args.documentId === 'same-tab' && Boolean(args.rejectReason) === stopped,
              'Normal and stopped polls must carry both identities');
            await delayedPoll.promise;
          }
          // Model the native publisher-lock check before it reads the queue.
          if (args.documentId !== 'same-tab' || args.sessionId !== nativeSessionId) return null;
          pendingB--;
          return {applied: false};
        }
        throw new Error(`Unexpected native command: ${command}`);
      }};
      await applyInboxNow();
      check(pollCount === 0, 'A new document must publish before it can poll');
      check(await publishCurrentSession(), 'Publish A before its delayed poll');
      if (stopped) presentation.control({command: 'stop'});
      const applying = applyInboxNow();
      nativeSessionId = 'session-B';
      useAppStore.getState().loadProjectState({...update, document: {...document, name: 'B'}}, [], [], 'B.nbcad');
      check(await publishCurrentSession(), 'Publish the replacement in the same tab');
      delayedPoll.resolve();
      await applying;
      check(pendingB === 1, 'A delayed old poll/rejection must leave the replacement queue untouched');
      await applyInboxNow();
      check(pendingB === 0 && pollCount === 2, 'The replacement must immediately consume its own inbox');
      outcomes.push(stopped ? 'stopped-poll-start' : 'poll-start');
    }
    useAppStore.getState().loadProjectState(update, [], [], 'A.nbcad');
    let publishingSession = 'session-A';
    const lateWrite = deferred<{skipped: boolean}>();
    const writing = deferred<void>();
    const polledSessions: unknown[] = [];
    w.__TAURI_INTERNALS__ = {async invoke(command, args = {}) {
      if (command === 'mcp_session_bridge_reserve') return {session_id: publishingSession, project_session_id: 'same-tab', generation: 1};
      if (command === 'mcp_session_bridge_write') {
        if (JSON.parse(args.payload as string).session_id === 'session-A') {
          writing.resolve(); return lateWrite.promise;
        }
        return {skipped: false};
      }
      if (command === 'engine_project_export_model') return ok(JSON.stringify(update));
      if (command === 'engine_project_visibility') return ok(initial.projectVisibility);
      if (command === 'engine_active_sketch') return ok(null);
      if (command === 'mcp_session_bridge_apply_inbox') { polledSessions.push(args.sessionId); return null; }
      throw new Error(`Unexpected native command: ${command}`);
    }};
    const publishingA = publishCurrentSession();
    await writing.promise;
    publishingSession = 'session-B';
    useAppStore.getState().loadProjectState({...update, document: {...document, name: 'B'}}, [], [], 'B.nbcad');
    check(await publishCurrentSession(), 'B publication must finish before A replies');
    lateWrite.resolve({skipped: false});
    check(await publishingA === null, 'A late successful write cannot authorize a run or poll on B');
    await applyInboxNow();
    check(polledSessions.length === 1 && polledSessions[0] === 'session-B', 'A late write must not replace the current inbox owner');
    outcomes.push('late-publication');
    return {unchangedReplacement: outcomes, preservedPublicationGuard: true};
  } finally {
    useAppStore.setState(initial); presentation.documentChanged();
    if (native) w.__TAURI_INTERNALS__ = native; else delete w.__TAURI_INTERNALS__;
  }
}
