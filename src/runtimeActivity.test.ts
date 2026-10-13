import {
  createRuntimeActivityTracker, visibleRuntimeActivities,
  RUNTIME_ACTIVITY_HISTORY_LIMIT, RUNTIME_ACTIVITY_NOTIFICATION_LIMIT,
} from './runtimeActivity';
import en from './i18n/en.json';
import zh from './i18n/zh-CN.json';
import es from './i18n/es.json';
import de from './i18n/de.json';

function assert(condition: boolean, message: string): void { if (!condition) throw new Error(message); }
let clock = 0;
const tracker = createRuntimeActivityTracker(() => clock);
const state = () => tracker.useStore.getState();
const open = tracker.begin({ kind: 'open', target: 'assembly.nbcad', notifySuccess: true });
clock = 80;
const cam = tracker.begin({ kind: 'cam', target: 'Roughing' });
open.setPhase('reading');
cam.setPhase('running');
assert(visibleRuntimeActivities(state().active, 299).length === 0, 'Fast work must not flash a detailed active task.');
assert(visibleRuntimeActivities(state().active, 300).map(task => task.id).join() === String(open.id), 'Each owner has its own 300ms grace.');
clock = 600;
open.finish();
open.setPhase('rendering');
open.fail(new Error('Late callback'));
assert(state().active.length === 1 && state().active[0].id === cam.id && state().active[0].phase === 'running', 'Settling one owner must preserve another task and ignore stale callbacks.');
assert(state().notifications.length === 1 && state().notifications[0].status === 'completed', 'Only explicit success opt-in produces a completed notification.');
assert(state().history[0].endedAt - state().history[0].startedAt === 600, 'Elapsed time must use real start and completion boundaries.');
tracker.dismissNotification(open.id);
assert(state().notifications.length === 0 && state().history.length === 1, 'Dismissal must preserve recent history.');
clock = 900;
cam.fail(new Error('Toolpath computation failed: unavailable face'));
cam.finish();
assert(state().active.length === 0 && state().notifications.length === 1 && state().notifications[0].error?.includes('unavailable face') === true, 'A real failure must retain actionable details without a completion claim.');
clock = 90_000;
assert(state().notifications[0].status === 'failed', 'Errors must not disappear on an elapsed timer.');
const picker = tracker.begin({ kind: 'save', notifySuccess: true });
picker.discard();
picker.finish();
assert(state().history.length === 2 && state().notifications.length === 1, 'An abandoned picker must publish no result or notification.');
tracker.begin({ kind: 'rebuild' }).finish();
assert(state().notifications.length === 1, 'Ordinary geometry work must not emit success notifications.');
for (let i = 0; i < 30; i++) tracker.begin({ kind: 'export', notifySuccess: true }).finish();
assert(state().history.length === RUNTIME_ACTIVITY_HISTORY_LIMIT && state().notifications.length === RUNTIME_ACTIVITY_NOTIFICATION_LIMIT, 'Recent results and notifications must stay bounded.');
const before = state().notifications.map(item => item.id);
tracker.dismissNotification(before[2]);
assert(state().notifications.map(item => item.id).join() === before.filter(id => id !== before[2]).join(), 'Dismissing one result must not clear another notification.');
const kinds = ['open', 'import', 'save', 'export', 'rebuild', 'drawing', 'cam'] as const;
const phases = ['queued', 'reading', 'validating', 'geometry', 'adopting', 'rendering', 'writing', 'running'] as const;
for (const dictionary of [en, zh, es, de]) {
  for (const kind of kinds) assert(Boolean(dictionary.runtimeActivity.kind[kind]), `Missing localized activity ${kind}.`);
  for (const phase of phases) assert(Boolean(dictionary.runtimeActivity.phase[phase]), `Missing localized phase ${phase}.`);
}
assert(!/[\u3400-\u9fff]/.test(JSON.stringify(en.runtimeActivity)), 'English activity feedback must contain only its regional copy.');
console.log('Runtime activity ownership, grace, abandonment, durable failures, notifications, history bounds, and four locales passed.');
