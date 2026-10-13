// Runs before the application module: no framework, fonts, network, or engine
// initialization is required to provide the first useful feedback.
(() => {
  const regional = {
    en: ['Limo CAD', 'Design with understanding.', 'Opening Limo CAD…', 'Limo CAD could not start.', 'Try again'],
    'zh-CN': ['砺模 CAD', '砺模，求理解之设计。', '正在启动砺模 CAD…', '砺模 CAD 未能启动。', '重试'],
    es: ['Limo CAD', 'Diseñar con comprensión.', 'Abriendo Limo CAD…', 'No se pudo iniciar Limo CAD.', 'Reintentar'],
    de: ['Limo CAD', 'Konstruieren mit Verständnis.', 'Limo CAD wird geöffnet…', 'Limo CAD konnte nicht gestartet werden.', 'Erneut versuchen'],
  };
  let copy = regional[document.documentElement.lang] || regional.en;
  const closeCopy = { en: 'Close app', 'zh-CN': '关闭应用', es: 'Cerrar aplicación', de: 'Anwendung schließen' };
  const updateClose = () => {
    const button = document.getElementById('startup-close');
    button.hidden = !window.__TAURI_INTERNALS__?.invoke;
    button.title = closeCopy[document.documentElement.lang] || closeCopy.en;
    button.ariaLabel = button.title;
  };
  updateClose();
  document.getElementById('startup-close').onclick = () => {
    window.__TAURI_INTERNALS__?.invoke('native_force_quit').catch(() => {});
  };
  // Choose once per launch. Optional local notes arrive independently of the
  // first paint and native initialization; they never affect readiness.
  window.__startupNoteIndex ??= Math.floor(Math.random() * 100);
  const updateNote = () => {
    const element = document.getElementById('startup-note');
    const screen = document.getElementById('startup-bootstrap');
    if (!element || !screen) return;
    const locale = document.documentElement.lang;
    const text = window.__startupNotes?.[locale]?.[window.__startupNoteIndex];
    element.hidden = !text || screen.dataset.failed === 'true';
    if (element.hidden) return;
    document.getElementById('startup-note-label').textContent = {
      en: 'Workshop note', 'zh-CN': '设计随想', es: 'Nota de taller', de: 'Werkstattnotiz',
    }[locale];
    document.getElementById('startup-note-text').textContent = text;
  };
  window.addEventListener('limo:startup-notes', updateNote);
  updateNote();
  document.title = copy[0];
  window.__TAURI_INTERNALS__?.invoke('native_window_set_locale', {
    locale: document.documentElement.lang,
  }).catch(() => {});
  document.getElementById('startup-name').textContent = copy[0];
  document.getElementById('startup-tagline').textContent = copy[1];
  document.getElementById('startup-status').textContent = copy[2];
  if (window.__TAURI_INTERNALS__?.invoke) {
    // Diagnostic settings are opt-in on the native side. Never hold feedback
    // or renderer work for this request; normal saved preferences stay intact.
    window.__startupMeasurementSettingsPromise = window.__TAURI_INTERNALS__
      .invoke('startup_measurement_settings').then((settings) => {
        if (!settings?.locale || !regional[settings.locale]) return settings;
        document.documentElement.lang = settings.locale;
        updateClose();
        copy = regional[settings.locale];
        document.title = copy[0];
        window.__TAURI_INTERNALS__.invoke('native_window_set_locale', { locale: settings.locale }).catch(() => {});
        const screen = document.getElementById('startup-bootstrap');
        if (screen) {
          document.getElementById('startup-name').textContent = copy[0];
          document.getElementById('startup-tagline').textContent = copy[1];
          document.getElementById('startup-status').textContent = copy[screen.dataset.failed === 'true' ? 3 : 2];
          document.getElementById('startup-retry').textContent = copy[4];
          updateNote();
        }
        return settings;
      });
    window.__startupMeasurementSettingsPromise.catch(() => {});
  }
  window.__startupTiming = { feedbackRequestedMs: performance.now() };
  requestAnimationFrame(() => requestAnimationFrame(() => {
    window.__startupTiming.firstFeedbackMs = performance.now();
    const native = window.__TAURI_INTERNALS__;
    if (native?.invoke) {
      native.invoke('startup_record', {
        phase: 'feedback', elapsedMs: window.__startupTiming.firstFeedbackMs,
      }).catch(() => {});
      window.__rendererStartPromise = native.invoke('native_viewport_start');
      // The React subscriber can attach later while its module is parsing.
      window.__rendererStartPromise.catch((error) => fail(String(error)));
    }
  }));
  const fail = (detail) => {
    const screen = document.getElementById('startup-bootstrap');
    if (!screen) return;
    screen.dataset.failed = 'true';
    updateNote();
    document.getElementById('startup-status').textContent = copy[3];
    document.getElementById('startup-detail').textContent = detail;
    document.getElementById('startup-retry').hidden = false;
    document.getElementById('startup-retry').textContent = copy[4];
  };
  window.addEventListener('error', (event) => {
    // A missing optional note file must never become a startup failure.
    if (event.target?.id === 'startup-notes-script' || event.filename?.endsWith('/startup-notes.js')) return;
    fail(event.message || copy[3]);
  }, true);
  window.addEventListener('unhandledrejection', (event) => fail(String(event.reason)));
  // A script/network failure need not emit a useful error. Keep recovery
  // reachable if the module never replaces this independent bootstrap screen.
  window.setTimeout(() => {
    if (document.getElementById('startup-bootstrap')) fail(copy[3]);
  }, 30_000);
})();
