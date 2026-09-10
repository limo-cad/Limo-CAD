import { runMeshExport } from './meshExportFlow';
import type { MeshExportScope } from '../engine/types';
import { ProjectTransitions } from './projectTransitions';

function same(actual: unknown, expected: unknown, message = 'Unexpected result') {
  const equal = Array.isArray(actual) && Array.isArray(expected)
    ? JSON.stringify(actual) === JSON.stringify(expected) : actual === expected;
  if (!equal) throw new Error(message);
}
async function rejects(promise: Promise<unknown>, expected: RegExp) {
  try { await promise; } catch (error) {
    if (expected.test(String(error))) return;
    throw error;
  }
  throw new Error('Expected export rejection');
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return {promise, resolve};
}

function fixture() {
  let model = 'model A';
  let owner = {};
  const capturedOwner = owner;
  const transitions = new ProjectTransitions();
  const transition = transitions.capture();
  const calls: string[] = [];
  const written: string[] = [];
  const choose = deferred<MeshExportScope | null>();
  const choosing = deferred<void>();
  const flow = {
    async assertSelectionOwner() {
      await transitions.assertCurrent(transition);
      same(owner, capturedOwner, 'Selection document changed');
    },
    async captureModel() { return model; },
    async chooseScope() { calls.push('scope'); choosing.resolve(); return choose.promise; },
    async render(_scope: MeshExportScope, expected: string) {
      calls.push('render');
      same(model, expected, 'Native snapshot precondition failed');
      return new TextEncoder().encode(model);
    },
    async chooseTarget() { calls.push('target'); return 'A.3mf'; },
    async write(target: string, bytes: Uint8Array) { written.push(`${target}:${new TextDecoder().decode(bytes)}`); },
  };
  return {flow, transitions, choose, choosing, calls, written,
    // Models the live MCP open path replacing the model in the same tab.
    openFromMcp(publish = true) { model = 'model B'; if (publish) owner = {}; },
  };
}

async function main() {
  const pending = fixture();
  const exportPromise = runMeshExport(pending.flow);
  await pending.choosing.promise;
  pending.openFromMcp();
  pending.choose.resolve('definition');
  await rejects(exportPromise, /Selection document changed/);
  same(pending.calls, ['scope']);
  same(pending.written, []);

  const capturing = fixture();
  const capture = deferred<string>();
  capturing.flow.captureModel = () => capture.promise;
  const capturePromise = runMeshExport(capturing.flow);
  capturing.openFromMcp();
  capture.resolve('model B');
  await rejects(capturePromise, /Selection document changed/);
  same(capturing.calls, [], 'A new snapshot cannot borrow the old UI name/selection');

  const unpublished = fixture();
  const captureStarted = deferred<void>();
  const unpublishedSnapshot = deferred<string>();
  unpublished.flow.captureModel = async () => {
    captureStarted.resolve();
    return unpublishedSnapshot.promise;
  };
  const unpublishedExport = runMeshExport(unpublished.flow);
  await captureStarted.promise;
  const finishOpen = unpublished.transitions.begin();
  unpublished.openFromMcp(false); // Native B, but A's store objects are still visible.
  unpublishedSnapshot.resolve('model B');
  finishOpen(); // Even a same-object UI publication cannot restore the old owner.
  await rejects(unpublishedExport, /document changed/);
  same(unpublished.calls, []);
  same(unpublished.written, []);

  const polling = fixture();
  const waitingOnPoll = deferred<void>();
  const originalOwnerCheck = polling.flow.assertSelectionOwner;
  let ownershipChecks = 0;
  polling.flow.assertSelectionOwner = async () => {
    if (++ownershipChecks === 3) waitingOnPoll.resolve();
    await originalOwnerCheck();
  };
  const polledExport = runMeshExport(polling.flow);
  await polling.choosing.promise;
  const finishEmptyPoll = polling.transitions.begin();
  polling.choose.resolve('definition');
  await waitingOnPoll.promise;
  same(polling.calls, ['scope'], 'Rendering waits for the outstanding poll');
  finishEmptyPoll(false);
  same(await polledExport, true, 'An empty MCP poll must not invalidate export');
  same(polling.written, ['A.3mf:model A']);

  const failedPublication = new ProjectTransitions();
  failedPublication.begin()(true, false);
  await rejects(failedPublication.assertCurrent(failedPublication.capture()), /document changed/);
  failedPublication.begin()(true, true);
  await failedPublication.assertCurrent(failedPublication.capture());

  const queued = fixture();
  const originalRender = queued.flow.render;
  queued.flow.render = async (scope, expected) => {
    // Queued after the last frontend check, before native lock acquisition.
    await Promise.resolve();
    queued.openFromMcp(false);
    return originalRender(scope, expected);
  };
  queued.choose.resolve('assembly');
  await rejects(runMeshExport(queued.flow), /Native snapshot precondition failed/);
  same(queued.calls, ['scope', 'render']);
  same(queued.written, []);

  const saving = fixture();
  saving.flow.chooseTarget = async () => { saving.openFromMcp(); return 'A.3mf'; };
  saving.choose.resolve('definition');
  same(await runMeshExport(saving.flow), true);
  same(saving.written, ['A.3mf:model A'], 'Save must write already captured bytes');

  const cancelled = fixture();
  cancelled.choose.resolve(null);
  same(await runMeshExport(cancelled.flow), false);
  same(cancelled.calls, ['scope']);
  same(cancelled.written, []);
  console.log('Mesh export ownership, same-tab replacement, queued edit and captured-save checks passed.');
}
await main();
