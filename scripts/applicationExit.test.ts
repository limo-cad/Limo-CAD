import assert from 'node:assert/strict';
import { createExitController, ExitBarrier } from '../src/files/applicationExit';

for (const dirty of [false, true]) {
  for (const decision of ['cancel', 'discard', 'save'] as const) {
    for (const saved of [false, true]) {
      let exits = 0, prompts = 0, saves = 0;
      const controller = createExitController({
        dirty: () => dirty,
        decide: async () => { prompts++; return decision; },
        save: async () => { saves++; return saved; },
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
console.log('PASS exit: clean, save, discard, cancel, failed save, duplicate requests, acknowledgement ordering, error recovery, disposal');
