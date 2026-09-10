import { EngineError, ProjectLoadError } from './index';
import { TauriEngine } from './tauri';
import { pendingEngineOperations } from './activity';

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}
type Reply = {command: string; value?: unknown; failure?: unknown};
const originalWindow = globalThis.window;
let replies: Reply[] = [];
globalThis.window = {__TAURI_INTERNALS__: {
  async invoke(command: string) {
    const reply = replies.shift();
    assert(reply?.command === command, `Unexpected command ${command}; expected ${reply?.command}`);
    if (reply.failure !== undefined) throw reply.failure;
    return typeof reply.value === 'string' ? reply.value : JSON.stringify(reply.value);
  },
}} as unknown as Window & typeof globalThis;

async function rejected(sequence: Reply[], expected: 'unchanged' | 'unverified', message: string) {
  replies = [...sequence];
  try {
    await new TauriEngine().loadProjectModel('requested model');
    throw new Error('Expected Open to reject');
  } catch (error) {
    assert(error instanceof ProjectLoadError && error instanceof EngineError, 'Preserve typed engine errors');
    assert(error.engineState === expected, `Expected ${expected}, got ${error.engineState}`);
    assert(error.message === message, 'Preserve the original failure message');
    assert(replies.length === 0, 'A failed load must not issue extra repair commands');
    assert(pendingEngineOperations() === 0, 'Release engine activity after every outcome');
    return error;
  }
}

try {
  const data = {project_load_state: 'unchanged', detail: 'schema 999'};
  const unchanged = await rejected([{command: 'engine_project_load', value: {
    ok: false, error: 'Unsupported schema', data,
  }}], 'unchanged', 'Unsupported schema');
  assert(JSON.stringify(unchanged.data) === JSON.stringify(data), 'Retain native diagnostic data');
  assert(unchanged.cause instanceof EngineError, 'Retain the original engine error as cause');

  for (const unknownData of [undefined, {}, {project_load_state: 'unknown'}]) {
    await rejected([{command: 'engine_project_load', value: {
      ok: false, error: 'Kernel conversion failed', data: unknownData,
    }}], 'unverified', 'Kernel conversion failed');
  }
  const transport = new Error('Connection lost after native work');
  const unknown = await rejected([{command: 'engine_project_load', failure: transport}],
    'unverified', transport.message);
  assert(unknown.cause === transport, 'Transport failures retain their cause without assuming rollback');

  const update = {document: {rollback_index: 2, features: [
    {id: 1, kind: 'construction_plane', suppressed: false},
    {id: 2, kind: 'extrude', suppressed: false},
  ]}, scene: {bodies: [], errors: []}};
  // Even a diagnostic marker on a later query cannot retroactively declare
  // the successful native replacement unchanged.
  await rejected([
    {command: 'engine_project_load', value: {ok: true, value: update}},
    {command: 'engine_datum_plane_definitions', value: {ok: false, error: 'Repair read failed', data}},
  ], 'unverified', 'Repair read failed');
  await rejected([
    {command: 'engine_project_load', value: {ok: true, value: update}},
    {command: 'engine_datum_plane_definitions', value: {ok: true, value: [
      {feature_id: 1, source: {type: 'offset', reference: {type: 'planar_face'}}},
    ]}},
    {command: 'engine_solid_set_rollback', value: {ok: false, error: 'Repair replay failed'}},
  ], 'unverified', 'Repair replay failed');

  replies = [
    {command: 'engine_project_load', value: {ok: true, value: update}},
    {command: 'engine_datum_plane_definitions', value: {ok: true, value: []}},
  ];
  assert(JSON.stringify(await new TauriEngine().loadProjectModel('valid model')) === JSON.stringify(update),
    'Successful load returns the original repaired update');
  assert(replies.length === 0 && pendingEngineOperations() === 0, 'Successful load finishes cleanly');
  console.log('Project load outcomes preserve atomic rejection, uncertain replacement, repair failures and diagnostics.');
} finally {
  globalThis.window = originalWindow;
}
