import { createDocumentFrameWaiter, type DocumentFrameDependencies } from './documentFrameReadiness';

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

async function checkDocumentFrames() {
  const owner = { activeProjectTabId: 'current-tab', document: {} };
  let current = { ...owner, activeTab: 'solid' };
  let clock = 0;
  let reads = 0;
  let syncs = 0;
  let paints = 0;
  const ready = {
    available: true, ready: true, startupError: null, modelSessionId: 'current-tab',
    geometryRevision: 7, renderedFrames: 10, logicalWidth: 900, logicalHeight: 700,
  };
  const snapshots = [
    { ...ready, renderedFrames: 9 },
    { ...ready, modelSessionId: 'old-tab' },
    { ...ready, geometryRevision: 6 },
    { ...ready, logicalWidth: 560, logicalHeight: 380 },
    { ...ready, renderedFrames: 9 },
    ready,
  ];
  const deps: DocumentFrameDependencies = {
    native: () => true,
    current: () => current,
    metrics: async () => snapshots[reads++] ?? ready,
    syncModel: async () => { syncs++; return 7; },
    viewport: () => ({ width: 900 / 1.75, height: 700 / 1.75 }),
    scale: () => 1.75,
    requestLayout: () => {},
    afterPaint: async () => { paints++; },
    delay: async (ms) => { clock += ms; },
    now: () => clock,
  };
  assert(await createDocumentFrameWaiter(deps)(owner), 'Only the current successful newly presented matching-extent frame may complete.');
  assert(reads === 6 && syncs === 1, 'Stale session, revision, extent and frame count must each retain progress.');

  reads = 0; syncs = 0;
  assert(await createDocumentFrameWaiter({ ...deps, native: () => false })(owner), 'Browser completion acknowledges its next paint.');
  assert(reads === 0 && syncs === 0 && paints === 2, 'The browser path must not issue native commands.');
  current = { ...owner, activeTab: 'drawing' };
  assert(!await createDocumentFrameWaiter(deps)(owner), 'Drawing must not pretend to present an unavailable 3D viewport.');
  assert(reads === 0 && syncs === 0, 'Drawing must not issue 3D readiness commands.');

  current = { ...owner, activeTab: 'solid' };
  assert(!await createDocumentFrameWaiter({ ...deps, metrics: async () => {
    current = { ...current, document: {} };
    return ready;
  } })(owner), 'A different published document in the same tab must supersede the wait.');
  assert(syncs === 0, 'Superseded work must not sync another document into its viewport.');

  current = { ...owner, activeTab: 'solid' };
  let nativeError = '';
  try {
    await createDocumentFrameWaiter({ ...deps, metrics: async () => ({ ...ready, startupError: 'shader preparation failed' }) })(owner);
  } catch (error) { nativeError = String(error); }
  assert(nativeError.includes('shader preparation failed'), 'The actual renderer failure must reach the operation caller.');

  let timedOut = false;
  try {
    await createDocumentFrameWaiter({ ...deps, metrics: () => new Promise(() => {}) })(owner, {
      timeoutMs: 5, timeoutMessage: 'Document rendering timed out.',
    });
  } catch (error) { timedOut = error instanceof Error && error.message === 'Document rendering timed out.'; }
  assert(timedOut, 'A hung native readiness command must remain bounded, not leave runtime activity stuck.');
}

void checkDocumentFrames().then(() => console.log('Document presented-frame ownership, extent, failure and browser checks passed.'));
