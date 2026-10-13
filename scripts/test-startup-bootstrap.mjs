import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { runInNewContext } from 'node:vm';

const html = readFileSync(new URL('../index.html', import.meta.url), 'utf8');
const bootstrap = readFileSync(new URL('../public/startup.js', import.meta.url), 'utf8');
const preferenceScript = html.match(/<script>([\s\S]*?)<\/script>/)[1];

function launch({ storedLocale, language = 'en-US', denyStorage = false, native = false } = {}) {
  const elements = new Map();
  const frames = [];
  const timers = [];
  const events = new Map();
  const invocations = [];
  const context = {
    Math: { floor: Math.floor, random: () => 0 },
    Event,
    document: {
      title: '',
      documentElement: { lang: 'en', dataset: {}, style: {} },
      getElementById(id) {
        if (!elements.has(id)) elements.set(id, { textContent: '', dataset: {}, hidden: id === 'startup-retry' });
        return elements.get(id);
      },
    },
    localStorage: { getItem(key) { if (denyStorage) throw new Error('Denied'); return key === 'nbcad.locale' ? storedLocale : null; } },
    navigator: { language },
    matchMedia: () => ({ matches: false }),
    requestAnimationFrame: (callback) => frames.push(callback),
    performance: { now: () => 12 },
    addEventListener: (name, callback) => events.set(name, callback),
    dispatchEvent: (event) => events.get(event.type)?.(event),
    setTimeout: (callback, duration) => timers.push({ callback, duration }),
  };
  context.window = context;
  if (native) context.__TAURI_INTERNALS__ = {
    invoke(command, args) { invocations.push({ command, args }); return Promise.resolve(); },
  };
  runInNewContext(preferenceScript, context);
  runInNewContext(bootstrap, context);
  return { context, elements, frames, events, timers, invocations };
}

test('early feedback follows saved language and starts native work only after feedback can paint', () => {
  const app = launch({ storedLocale: 'zh-CN', language: 'de-DE', native: true });
  assert.equal(app.elements.get('startup-name').textContent, '砺模 CAD');
  assert.equal(app.elements.get('startup-status').textContent, '正在启动砺模 CAD…');
  assert.deepEqual(app.invocations.map((call) => call.command), ['native_window_set_locale', 'startup_measurement_settings']);
  app.frames.shift()();
  assert.equal(app.invocations.length, 2);
  app.frames.shift()();
  assert.deepEqual(app.invocations.map((call) => call.command), ['native_window_set_locale', 'startup_measurement_settings', 'startup_record', 'native_viewport_start']);
  assert.equal(app.invocations[0].args.locale, 'zh-CN');
  assert.equal(app.invocations[2].args.phase, 'feedback');
  assert.ok(app.context.__rendererStartPromise);
});

test('denied or invalid preferences preserve browser-language and English fallbacks', () => {
  assert.equal(launch({ denyStorage: true, language: 'es-MX' }).context.document.documentElement.lang, 'es');
  assert.equal(launch({ storedLocale: 'unknown', language: 'de-AT' }).elements.get('startup-name').textContent, 'Limo CAD');
  assert.equal(launch({ language: 'fr-FR' }).context.document.documentElement.lang, 'en');
});

test('frameless close is localized and dragging uses the declarative Tauri region', () => {
  const app = launch({ storedLocale: 'de', native: true });
  assert.equal(app.elements.get('startup-close').title, 'Anwendung schließen');
  assert.equal(app.elements.get('startup-close').hidden, false);
  const count = app.invocations.length;
  assert.match(html, /<div\b[^>]*id="startup-drag"[^>]*\bdata-tauri-drag-region(?:\s|>)/);
  assert.doesNotMatch(bootstrap, /onmousedown|native_window_start_dragging/);
  assert.equal(app.invocations.length, count, 'Declaring the drag region must not invoke native work.');
  app.elements.get('startup-close').onclick();
  assert.equal(app.invocations.at(-1).command, 'native_force_quit');
  assert.equal(launch().elements.get('startup-close').hidden, true);
});

test('module failure and a missing module both expose localized early recovery', () => {
  const rejected = launch({ storedLocale: 'de' });
  rejected.events.get('unhandledrejection')({ reason: 'Module unavailable' });
  assert.equal(rejected.elements.get('startup-status').textContent, 'Limo CAD konnte nicht gestartet werden.');
  assert.equal(rejected.elements.get('startup-detail').textContent, 'Module unavailable');
  assert.equal(rejected.elements.get('startup-retry').hidden, false);
  assert.equal(rejected.elements.get('startup-retry').textContent, 'Erneut versuchen');
  const missing = launch({ storedLocale: 'es' });
  assert.equal(missing.timers[0].duration, 30_000);
  missing.timers[0].callback();
  assert.equal(missing.elements.get('startup-retry').textContent, 'Reintentar');
});

test('one local note per launch stays secondary, localized, and independent of readiness', () => {
  const app = launch({ storedLocale: 'en', native: true });
  assert.equal(app.elements.get('startup-note').hidden, true, 'Missing optional notes must not delay early feedback.');
  const framesBefore = app.frames.length;
  const callsBefore = app.invocations.length;
  const notesScript = readFileSync(new URL('../public/startup-notes.js', import.meta.url), 'utf8');
  runInNewContext(notesScript, app.context);
  assert.equal(app.context.__startupNoteIndex, 0);
  assert.equal(app.elements.get('startup-note-text').textContent, 'Tolerance is a virtue. Also a number.');
  assert.equal(app.elements.get('startup-note-label').textContent, 'Workshop note');
  assert.equal(app.elements.get('startup-status').textContent, 'Opening Limo CAD…');
  assert.equal(app.frames.length, framesBefore, 'Notes must not schedule an animation or branding delay.');
  assert.equal(app.invocations.length, callsBefore, 'Notes must not invoke native work.');
  assert.equal(app.timers.length, 1, 'Notes must not rotate or add timers.');
  for (const pool of Object.values(app.context.__startupNotes)) {
    assert.equal(pool.length, 100);
    assert.equal(new Set(pool).size, 100);
    assert.ok(pool.every((note) => typeof note === 'string' && note.trim() && note.length < 180));
  }
  assert.ok(app.context.__startupNotes.en.every((note) => !/\p{Script=Han}/u.test(note)),
    'English launches must keep every workshop note entirely English.');
  app.context.document.documentElement.lang = 'zh-CN';
  app.context.dispatchEvent(new Event('limo:startup-notes'));
  assert.equal(app.context.__startupNoteIndex, 0, 'Locale changes must preserve the launch selection.');
  assert.equal(app.elements.get('startup-note-text').textContent, app.context.__startupNotes['zh-CN'][0]);
  assert.equal(app.elements.get('startup-note-label').textContent, '设计随想');
  app.events.get('unhandledrejection')({ reason: 'Required document failed' });
  assert.equal(app.elements.get('startup-note').hidden, true, 'Recovery must give the failure and actions full attention.');
  app.context.dispatchEvent(new Event('limo:startup-notes'));
  assert.equal(app.elements.get('startup-note').hidden, true);
  const late = launch({ storedLocale: 'en', native: true });
  late.frames.shift()();
  late.frames.shift()();
  assert.equal(late.invocations.at(-1).command, 'native_viewport_start');
  const callsAfterFeedback = late.invocations.length;
  runInNewContext(notesScript, late.context);
  assert.equal(late.invocations.length, callsAfterFeedback, 'Late notes must leave recorded feedback and native startup untouched.');
  assert.equal(late.context.__startupTiming.firstFeedbackMs, 12);
  assert.equal(late.elements.get('startup-note').hidden, false);
});

test('optional note resource failure cannot fail application startup', () => {
  const app = launch({ storedLocale: 'en' });
  app.events.get('error')({ target: { id: 'startup-notes-script' } });
  assert.equal(app.elements.get('startup-bootstrap').dataset.failed, undefined);
  assert.equal(app.elements.get('startup-status').textContent, 'Opening Limo CAD…');
});
