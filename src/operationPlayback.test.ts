import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { PresentationController, SerialPlayback, presentation } from './operationPlayback';
import { PresentationControls, PresentationReopen } from './components/PresentationControls';

function check(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}
let now = 0;
const player = new PresentationController(() => now);
player.control({ command: 'configure', mode: 'present' });
player.control({ command: 'note', text: 'Cut the centered slot', chapter: 'Picket', duration_ms: 1000, step_index: 1, step_count: 2 });
now = 250;
check(player.status().wait_ms === 750, 'Caption hold advances with real time');
player.control({ command: 'pause' });
now = 10000;
check(!player.canApply() && player.status().wait_ms === 750, 'Pause freezes dwell and leaves model work pending');
player.control({ command: 'configure', speed: 2 });
check(player.status().wait_ms === 375, 'Changing speed re-scales the remaining hold, including while paused');
player.control({ command: 'step' });
check(player.status().step_pending, 'A paused caption exposes the step permit so a script can reach its next mutation');
check(player.canApply() && player.canApply(), 'Empty inbox polls must not consume the step permit');
player.applied('Camera: front');
check(player.canApply(), 'Camera and control feedback must not consume a modeling step');
player.modelApplied();
check(!player.canApply() && !player.status().step_pending, 'A completed modeling operation consumes exactly one step');
player.control({ command: 'resume' });
now += 375;
check(player.canApply(), 'Resume completes the remaining presentation hold');
player.control({ command: 'note', duration_ms: 10000 });
player.control({ command: 'configure', mode: 'fast' });
check(player.canApply() && player.motionDuration(300) === 0, 'Maximum rate has neither caption delay nor camera animation');
player.control({ command: 'stop' });
check(player.status().stopped && !player.canApply(), 'Stop never silently continues model work');
let refused = false;
try { player.control({ command: 'resume' }); } catch { refused = true; }
check(refused, 'Stopped playback requires an explicit new run');
refused = false;
try { player.control({ command: 'finish' }); } catch { refused = true; }
check(refused && player.status().stopped, 'A late finish cannot clear the user’s stop');
player.control({ command: 'configure', mode: 'present', speed: 1, step_index: 0, step_count: 4 });
check(player.canApply() && !player.status().stopped, 'A fresh configuration can start a new run');
player.control({ command: 'note', text: 'Assembly ready', chapter: 'Review', step_index: 4 });
player.control({ command: 'finish' });
check(player.status().finished && player.status().text === 'Assembly ready' && player.canApply(), 'Finished caption remains readable and does not block subsequent work');
const completedRun = new PresentationController(() => now);
completedRun.control({ command: 'configure', mode: 'present', chapter: 'A fresh walkthrough', step_index: 0, step_count: 7 });
completedRun.control({ command: 'note', text: 'Before the drawing steps', step_index: 2 });
completedRun.applied('Camera: isometric');
// Local expressions, drawing reads and checks need not emit a new caption.
completedRun.control({ command: 'finish', step_index: 7, step_count: 7 });
const completedSnapshot = completedRun.snapshot();
completedRun.applied('File: save');
completedRun.applied('solid_extrude');
check(completedRun.snapshot() === completedSnapshot && completedSnapshot.step_index === 7
  && completedSnapshot.operation === 'Camera: isometric',
  'Authoritative completion includes uncaptained steps and later file/model work cannot rewrite the finished run');
completedRun.control({ command: 'configure', mode: 'fast', step_index: 0, step_count: 3, text: '', chapter: 'Next run' });
completedRun.applied('solid_extrude');
check(completedRun.snapshot().operation === 'extrude' && completedRun.snapshot().step_index === 0,
  'A new run accepts operation feedback again and resets its progress');
completedRun.control({ command: 'note', step_index: 1 });
completedRun.control({ command: 'stop' });
const stoppedSnapshot = completedRun.snapshot();
completedRun.applied('File: save');
check(completedRun.snapshot() === stoppedSnapshot && stoppedSnapshot.step_index === 1 && !stoppedSnapshot.finished,
  'A stopped run retains partial progress and its last operation');
player.configurePace(350);
player.modelApplied();
check(!player.canApply(), 'Explicit legacy pace remains supported');
now += 350;
player.control({ command: 'configure', mode: 'present' });
player.modelApplied();
check(player.canApply(), 'New presentation configuration does not inherit legacy per-operation delays');
for (const request of [{ speed: 0 }, { duration_ms: -1 }, { step_index: 5 }, { text: 'x'.repeat(4001) }, { chapter: 'x'.repeat(201) }, { text: '\u0001' }]) {
  const before = JSON.stringify(player.status());
  let failed = false;
  try { player.control({ command: 'note', ...request }); } catch { failed = true; }
  check(failed && JSON.stringify(player.status()) === before, 'Invalid presentation requests leave playback unchanged');
}

let visibilityClock = 0;
const visibility = new PresentationController(() => visibilityClock);
visibility.control({ command: 'dismiss' });
check(!visibility.status().active && !visibility.status().visible, 'Dismissing absent playback does not create a run');
visibility.control({ command: 'configure', mode: 'present' });
visibility.control({ command: 'note', text: 'An authored hold', duration_ms: 1000 });
visibility.control({ command: 'dismiss' });
visibilityClock += 1000;
check(!visibility.status().visible && visibility.canApply(), 'Closing the bar leaves running playback and its hold advancing');
visibility.control({ command: 'pause' });
visibility.control({ command: 'step' });
visibility.control({ command: 'show' });
visibility.control({ command: 'dismiss' });
check(!visibility.status().visible && visibility.status().paused && visibility.status().step_pending,
  'Dismissing paused playback preserves its pending step instead of silently resuming or canceling it');
visibility.control({ command: 'note', text: 'The next caption', duration_ms: 500 });
check(!visibility.status().visible, 'Later captions respect the user’s decision to hide the controls');
visibility.control({ command: 'show' });
check(visibility.status().visible && visibility.status().paused && visibility.status().step_pending,
  'Reopening restores controls without changing the paused execution state');
visibility.modelApplied();
visibility.control({ command: 'dismiss' });
visibility.control({ command: 'finish' });
check(!visibility.status().visible && visibility.status().finished && visibility.canApply(),
  'Completion stays dismissed and does not block normal modeling');
visibility.control({ command: 'show' });
visibility.control({ command: 'dismiss' });
check(!visibility.status().visible && visibility.canApply(), 'Completed playback remains closable');
visibility.control({ command: 'configure', mode: 'present' });
check(visibility.status().visible && !visibility.status().finished, 'A newly started run reveals its controls again');

const renderControls = () => renderToStaticMarkup(createElement(PresentationControls));
const renderReopen = () => renderToStaticMarkup(createElement(PresentationReopen));
check(renderControls() === '' && renderReopen() === '', 'No playback chrome appears before a run exists');
presentation.control({ command: 'configure', mode: 'present', speed: 0.1 });
check(renderControls().includes('aria-label="Close playback controls"'), 'The actual playback surface exposes an accessible close control');
check(renderControls().includes('value="0.1" selected'), 'A configured non-preset speed remains visible in the actual selector');
presentation.control({ command: 'pause' });
presentation.control({ command: 'dismiss' });
check(renderControls() === '' && renderReopen().includes('Show playback controls') && renderReopen().includes('Paused'),
  'Hidden paused playback leaves a discoverable reopen control with its status');
presentation.control({ command: 'show' });
check(renderReopen() === '' && renderControls().includes('Resume'), 'Showing playback restores the real resume control without duplicate surfaces');
presentation.control({ command: 'finish', step_index: 7, step_count: 7 });
check(renderControls().includes('7 / 7') && renderControls().includes('value="7" max="7"'),
  'The actual completed surface renders the full interpreter count and a full progress bar');
const closeControl = renderControls().match(/<button[^>]*aria-label="Close playback controls"[^>]*>/)?.[0];
check(Boolean(closeControl) && !/\sdisabled(?:\s|=|>)/.test(closeControl!), 'The close control remains enabled after completion');
presentation.control({ command: 'dismiss' });
check(renderControls() === '' && renderReopen().includes('Complete'), 'A completed bar can be removed and reopened');

const documentReset = new PresentationController(() => now);
const initialPresentation = JSON.stringify(documentReset.snapshot());
documentReset.configurePace(700);
documentReset.control({command: 'note', text: 'Previous document', chapter: 'Assembly', step_index: 2, step_count: 3, duration_ms: 1000});
documentReset.emphasize([4], [5]);
documentReset.control({command: 'step'});
check(documentReset.documentVersion() === 0 && documentReset.status().step_pending, 'Ordinary presentation controls retain the document revision');
let observedRevision = -1;
documentReset.subscribe(() => { observedRevision = documentReset.documentVersion(); });
documentReset.documentChanged();
check(documentReset.documentVersion() === 1 && observedRevision === 1, 'Replacement advances the revision before notifying observers');
check(JSON.stringify(documentReset.snapshot()) === initialPresentation && documentReset.status().wait_ms === 0
  && !documentReset.status().step_pending && documentReset.canApply(), 'Replacement clears prior controls, holds, step permits and highlights');
documentReset.control({command: 'configure', mode: 'present'});
documentReset.modelApplied();
check(documentReset.documentVersion() === 1 && documentReset.canApply(), 'An ordinary model edit retains its owner without inheriting the old pace');
documentReset.documentChanged();
check(documentReset.documentVersion() === 2, 'Repeated replacement advances the same document revision');

const lane = new SerialPlayback();
let release!: () => void;
const barrier = new Promise<void>(resolve => { release = resolve; });
let runs = 0;
const first = lane.tick(async () => { runs += 1; if (runs === 1) await barrier; });
await lane.tick(async () => { throw new Error('Parallel lane entered'); });
release();
await first;
check(runs === 1, 'The serial lane never repeats or overlaps a completed operation');
check(await lane.tick(async () => {}), 'The lane accepts subsequent work after completion');
console.log('Presentation timing, pause, step, stop, maximum rate, dismiss/reopen, rendered controls, and serial ordering passed');
