import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

export const flagshipTests = Object.freeze({
  turbine: 'turbine_replays_edits_restores_prints_and_drives_native_geometry',
  vise: 'vise::d_screw_vise_builds_editable_native_geometry',
});
const cargoTest = ['test', '--locked', '--manifest-path', 'mcp-server/Cargo.toml'];

export function testArguments(shard) {
  if (shard === 'core') {
    // Run every target, including future tests and doctests. --exact also
    // applies to --skip, so similarly named regressions are not filtered out.
    return [...cargoTest, '--', '--test-threads=1', '--exact',
      ...Object.values(flagshipTests).flatMap(name => ['--skip', name])];
  }
  assert(Object.hasOwn(flagshipTests, shard), `Unknown MCP shard: ${shard}`);
  // Retain existing slow-RPC timings on success so geometry cost is visible.
  return [...cargoTest, '--test', 'recipes', flagshipTests[shard], '--', '--exact', '--test-threads=1', '--nocapture'];
}

export function verifyInventory(output) {
  const tests = output.split(/\r?\n/).filter(line => line.endsWith(': test')).map(line => line.slice(0, -6));
  for (const name of Object.values(flagshipTests)) {
    assert.equal(tests.filter(test => test === name).length, 1,
      `Expected exactly one compiled recipe test named ${name}; update CI sharding after a rename`);
  }
}

function succeeded(result) {
  if (result.error) throw result.error;
  assert.equal(result.status, 0, `Cargo failed (exit ${result.status}, signal ${result.signal ?? 'none'})`);
}

export function runShard(shard, run = spawnSync) {
  const args = testArguments(shard);
  // libtest succeeds when a filter matches zero tests. Fail before running a
  // shard if a renamed/missing flagship would silently remove coverage.
  const inventory = run('cargo', [...cargoTest, '--test', 'recipes', '--', '--list'], {
    encoding: 'utf8', stdio: ['inherit', 'pipe', 'inherit'],
  });
  succeeded(inventory);
  verifyInventory(inventory.stdout);
  succeeded(run('cargo', args, { stdio: 'inherit' }));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runShard(process.argv[2]);
}
