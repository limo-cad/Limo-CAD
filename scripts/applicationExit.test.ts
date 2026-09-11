import assert from 'node:assert/strict';
import { createExitController, ExitBarrier } from '../src/files/applicationExit';

for (const dirty of [false, true]) {
  for (const decision of ['cancel', 'discard', 'save'] as const) {
    for (const saved of [false, true]) {
      let exits = 0, prompts = 0, saves = 0;
      let currentDirty = dirty;
      const controller = createExitController({
        dirty: () => currentDirty,
        decide: async () => { prompts++; return decision; },
        save: async () => { saves++; if (saved) currentDirty = false; return saved; },
        exit: async () => { exits++; },
        error: error => { throw error; },
      });
      await Promise.all([controller.request(), controller.request()]);
      assert.equal(exits, Number(!dirty || decision === 'discard' || (decision === 'save' && saved)));
      assert.equal(prompts, Number(dirty));
      assert.equal(saves, Number(dirty && decision === 'save'));
    }
  }
}

const barrier = new ExitBarrier();
const release = barrier.hold();
const events: string[] = [];
const controller = createExitController({
  dirty: () => false, decide: async () => 'cancel', save: async () => false,
  exit: async () => { events.push('exit'); }, error: error => { throw error; },
}, barrier);
const quitting = controller.request();
await Promise.resolve();
assert.deepEqual(events, []);
events.push('acknowledged');
release();
await quitting;
assert.deepEqual(events, ['acknowledged', 'exit']);

const mutationBarrier = new ExitBarrier();
const finishMutation = mutationBarrier.hold();
let changed = false, latePrompts = 0, lateExits = 0;
const duringMutation = createExitController({
  dirty: () => changed,
  decide: async () => { latePrompts++; return 'cancel'; },
  save: async () => false,
  exit: async () => { lateExits++; },
  error: error => { throw error; },
}, mutationBarrier);
const requestedDuringMutation = duringMutation.request();
changed = true;
finishMutation();
await requestedDuringMutation;
assert.equal(latePrompts, 1, 'Work created by the pending control must be guarded');
assert.equal(lateExits, 0);

for (const nextDecision of ['cancel', 'discard', 'save'] as const) {
  const afterSaveBarrier = new ExitBarrier();
  let unsaved = true, prompts = 0, saves = 0, exits = 0;
  const order: string[] = [];
  const afterSave = createExitController({
    dirty: () => unsaved,
    decide: async () => { prompts++; return prompts === 1 ? 'save' : nextDecision; },
    save: async () => {
      saves++; unsaved = false;
      if (saves === 1) {
        // Save finished, then another live control created work while its
        // acknowledgement was still holding the final shutdown barrier.
        const acknowledge = afterSaveBarrier.hold();
        queueMicrotask(() => { unsaved = true; order.push('late edit acknowledged'); acknowledge(); });
      }
      return true;
    },
    exit: async () => { exits++; order.push('exit'); },
    error: error => { throw error; },
  }, afterSaveBarrier);
  await afterSave.request();
  assert.equal(prompts, 2, 'New work after Save needs its own unsaved-work decision');
  assert.equal(exits, Number(nextDecision !== 'cancel'));
  assert.equal(saves, nextDecision === 'save' ? 2 : 1);
  assert.equal(unsaved, nextDecision !== 'save', 'Only successful Save may clear newly created work');
  assert.deepEqual(order, nextDecision === 'cancel' ? ['late edit acknowledged'] : ['late edit acknowledged', 'exit']);
}

const discardBarrier = new ExitBarrier();
const discardOrder: string[] = [];
const discard = createExitController({
  dirty: () => true,
  decide: async () => {
    const acknowledge = discardBarrier.hold();
    queueMicrotask(() => { discardOrder.push('discard acknowledged'); acknowledge(); });
    return 'discard';
  },
  save: async () => { throw new Error('Discard must not save'); },
  exit: async () => { discardOrder.push('exit'); },
  error: error => { throw error; },
}, discardBarrier);
await discard.request();
assert.deepEqual(discardOrder, ['discard acknowledged', 'exit'], 'Explicit Discard must finish after its own control acknowledgement');

let attempts = 0, errors = 0;
const retry = createExitController({
  dirty: () => false, decide: async () => 'cancel', save: async () => false,
  exit: async () => { if (++attempts === 1) throw new Error('IPC failed'); },
  error: () => { errors++; },
});
await retry.request();
await retry.request();
assert.equal(attempts, 2);
assert.equal(errors, 1);
retry.dispose();
await retry.request();
assert.equal(attempts, 2);
console.log('PASS exit: clean, save, discard, cancel, failed save, duplicate requests, acknowledgement ordering, late edits after Save, error recovery, disposal');
