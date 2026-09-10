import { PresentationController, SerialPlayback } from './operationPlayback';

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
console.log('Presentation timing, pause, step, stop, maximum rate, and serial ordering passed');
