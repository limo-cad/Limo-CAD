import assert from 'node:assert/strict';
import test from 'node:test';
import { flagshipTests, runShard, testArguments, verifyInventory } from './run-mcp-tests.mjs';

const inventory = `${Object.values(flagshipTests).map(name => `${name}: test`).join('\n')}\nother_regression: test\n`;

test('core keeps all targets and excludes only the two exact flagship names', () => {
  const args = testArguments('core');
  assert.deepEqual(args, ['test', '--locked', '--manifest-path', 'mcp-server/Cargo.toml', '--',
    '--test-threads=1', '--exact', '--skip', flagshipTests.turbine, '--skip', flagshipTests.vise]);
});

test('each heavy shard runs its one exact recipe serially with unchanged assertions', () => {
  for (const [shard, name] of Object.entries(flagshipTests)) {
    assert.deepEqual(testArguments(shard), ['test', '--locked', '--manifest-path', 'mcp-server/Cargo.toml',
      '--test', 'recipes', name, '--', '--exact', '--test-threads=1', '--nocapture']);
  }
  assert.throws(() => testArguments('typo'), /Unknown MCP shard/);
  assert.throws(() => testArguments('toString'), /Unknown MCP shard/);
});

test('inventory rejects missing, duplicate and substring-only flagship matches', () => {
  verifyInventory(inventory);
  verifyInventory(inventory.replaceAll('\n', '\r\n'));
  assert.throws(() => verifyInventory(''), /Expected exactly one/);
  assert.throws(() => verifyInventory(inventory + `${flagshipTests.vise}: test\n`), /Expected exactly one/);
  assert.throws(() => verifyInventory(inventory.replace(flagshipTests.turbine, `${flagshipTests.turbine}_renamed`)), /Expected exactly one/);
});

test('driver validates the compiled inventory before executing any shard', () => {
  for (const shard of ['core', 'turbine', 'vise']) {
    const calls = [];
    runShard(shard, (command, args) => {
      assert.equal(command, 'cargo');
      calls.push(args);
      return { status: 0, stdout: inventory };
    });
    assert.deepEqual(calls[0].slice(-4), ['--test', 'recipes', '--', '--list']);
    assert.deepEqual(calls[1], testArguments(shard));
  }
});

test('compile, inventory, process and test failures stop the shard', () => {
  let calls = 0;
  assert.throws(() => runShard('core', () => { calls++; return { status: 1 }; }), /Cargo failed/);
  assert.equal(calls, 1);
  assert.throws(() => runShard('core', () => ({ status: 0, stdout: '' })), /Expected exactly one/);
  assert.throws(() => runShard('core', () => ({ error: new Error('spawn failed') })), /spawn failed/);
  calls = 0;
  assert.throws(() => runShard('vise', () => ++calls === 1
    ? { status: 0, stdout: inventory } : { status: null, signal: 'SIGTERM' }), /Cargo failed/);
  assert.equal(calls, 2);
});
