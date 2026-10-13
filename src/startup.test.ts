import { completeStartup, startupFailed, useStartupStore, usesNativeStartup, workspaceIsReady, workspaceLayoutIsReady, startupApplicationPhase, type RenderReadiness } from './startup';
import { beginDocumentLoad, currentDocumentLoad, useDocumentLoadStore } from './files/documentLoadProgress';
import { initializeStartupDocument } from './startupInitialization';
import { projectModelDifference } from './engine/projectModelComparison';
import en from './i18n/en.json';
import zhCN from './i18n/zh-CN.json';
import es from './i18n/es.json';
import de from './i18n/de.json';

function assert(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

assert(!usesNativeStartup(false, 'development'), 'The WASM browser must retain working interaction without native renderer readiness.');
assert(!usesNativeStartup(false, 'production'), 'A web production build must not display native-renderer failure.');
const fullMetrics = { logicalWidth: 1200, logicalHeight: 720 };
assert(!workspaceLayoutIsReady(fullMetrics, { width: 1200, height: 720 }, { width: 520, height: 440 }, 1),
  'A rendered compact startup window must not count as the expanded workspace.');
assert(!workspaceLayoutIsReady({ logicalWidth: 520, logicalHeight: 440 }, { width: 1200, height: 720 }, { width: 1440, height: 900 }, 1),
  'A stale compact native extent must not release the expanded workspace.');
assert(workspaceLayoutIsReady(fullMetrics, { width: 1200 / 1.75, height: 720 / 1.75 }, { width: 1440 / 1.75, height: 900 / 1.75 }, 1.75),
  'Saved CAD zoom must match the final layout in native logical pixels.');
assert(!workspaceLayoutIsReady(fullMetrics, null, { width: 1440, height: 900 }, 1),
  'A missing or collapsed viewport must not release startup.');
assert(usesNativeStartup(false, 'desktop'), 'A desktop preview must explain that its native host is required.');
assert(usesNativeStartup(true, 'development'), 'Tauri development must use native startup readiness.');
assert(startupApplicationPhase(false, false, { ready: false, startupPhase: 'gpu' }) === 'gpu',
  'Real application GPU initialization must remain visible while engine/session setup is incomplete.');
assert(startupApplicationPhase(true, true, { ready: false, startupPhase: 'pipelines' }) === 'pipelines',
  'A prepared engine/session must not claim application rendering has finished.');
assert(startupApplicationPhase(false, true, { ready: true, startupPhase: 'ready' }) === 'document',
  'Rendering cannot claim that required initial engine/session setup has completed.');
assert(startupApplicationPhase(true, true, { ready: true, startupPhase: 'ready' }) === 'initialized',
  'A genuinely initialized application may still be opening a separate file.');
const firstOpen = beginDocumentLoad('first.nbcad');
const secondOpen = beginDocumentLoad('second.nbcad');
firstOpen.setPhase('geometry');
secondOpen.setPhase('waiting');
assert(currentDocumentLoad(useDocumentLoadStore.getState().operations)?.name === 'first.nbcad',
  'A queued Open must not replace the active document operation status.');
firstOpen.finish();
firstOpen.setPhase('adopting');
assert(currentDocumentLoad(useDocumentLoadStore.getState().operations)?.name === 'second.nbcad'
  && currentDocumentLoad(useDocumentLoadStore.getState().operations)?.phase === 'waiting',
  'A finished or stale operation must neither clear nor overwrite another Open.');
secondOpen.finish();
assert(currentDocumentLoad(useDocumentLoadStore.getState().operations) === null,
  'Completion must remove document progress without affecting application readiness.');
assert(projectModelDifference('{"geometry":{"x":1,"y":2},"features":[1,2]}', '{"features":[1,2],"geometry":{"y":2,"x":1}}') === null,
  'Restoration must accept JSON object key order while preserving all values.');
assert(Boolean(projectModelDifference('{"features":[1,2]}', '{"features":[2,1]}')),
  'Feature order must remain exact.');
assert(projectModelDifference('{"geometry":{"x":1}}', '{"geometry":{"x":1.000000001}}')?.includes('["x"]') === true,
  'Geometric drift must fail with a bounded differing leaf.');
assert(Boolean(projectModelDifference('{"metadata":{"id":1}}', '{"metadata":{}}')),
  'Restoration must never silently discard metadata differences.');

const rendered: RenderReadiness = {
  available: true, ready: true, renderedFrames: 1, startupError: null,
  modelSessionId: 'initial-document', geometryRevision: 4,
};
const ready = (metrics = rendered, documentReady = true, fontsReady = true) =>
  workspaceIsReady(documentReady, fontsReady, 'initial-document', metrics, 4);

assert(ready(), 'A successful current-document frame must reveal the workspace immediately.');
assert(!ready(rendered, false), 'Rendering must not bypass required document initialization.');
assert(!ready(rendered, true, false), 'Text assets must settle before revealing the workspace.');
assert(!ready({ ...rendered, ready: false }), 'Renderer installation alone is insufficient.');
assert(!ready({ ...rendered, renderedFrames: 0 }), 'Pipeline preparation alone is insufficient.');
assert(!ready({ ...rendered, geometryRevision: 3 }), 'A prior rendered model must not satisfy the current document.');
assert(!ready({ ...rendered, modelSessionId: 'another-document' }), 'A successful frame for a different document must not reveal the workspace.');
assert(!ready({ ...rendered, startupError: 'pipeline compilation failed' }), 'A rendering failure must never be hidden by an earlier successful frame.');
assert(!workspaceIsReady(true, true, null, rendered, 4), 'An unbound bootstrap document must not be considered usable.');

const initialStartup = useStartupStore.getState();
assert(!completeStartup(), 'Startup cannot latch before its required document is prepared.');
startupFailed(new Error('initial document failed'), 'document');
assert(!completeStartup(), 'An initial failure must retain recovery instead of latching ready.');
useStartupStore.setState({ documentReady: true, error: null });
assert(completeStartup() && useStartupStore.getState().completed, 'Verified startup completion must latch for this session.');
startupFailed(new Error('later runtime failure'), 'renderer');
assert(useStartupStore.getState().phase === 'ready' && useStartupStore.getState().error === null,
  'A later operation failure must never replace the ready workspace with the startup screen.');
const completedStartup = useStartupStore.getState();
void initializeStartupDocument(true);
assert(useStartupStore.getState() === completedStartup,
  'A remount or stale empty-recovery action after completion must not reset the document or startup state.');
useStartupStore.setState(initialStartup);

const dictionaries = { en, 'zh-CN': zhCN, es, de };
assert(Object.values(en.startup).every((copy) => !/\p{Script=Han}/u.test(copy)),
  'English loading phases and recovery actions must not mix in Chinese copy.');
const startupKeys = Object.keys(en.startup);
for (const [locale, dict] of Object.entries(dictionaries)) {
  assert(dict.app.name === (locale === 'zh-CN' ? '砺模 CAD' : 'Limo CAD'), `${locale} must use its published regional brand.`);
  for (const key of startupKeys) {
    assert(Boolean((dict.startup as Record<string, string>)[key]?.trim()), `${locale} is missing startup copy for ${key}.`);
  }
  for (const key of Object.keys(en.documentLoad)) {
    assert(Boolean((dict.documentLoad as Record<string, string>)[key]?.trim()), `${locale} is missing file preparation copy for ${key}.`);
  }
}
assert(Object.values(en.documentLoad).every((copy) => !/\p{Script=Han}/u.test(copy)),
  'English file preparation messages must remain entirely English.');
console.log('Startup readiness and four-locale recovery copy passed.');
