import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { isTauriRuntime } from '../engine';
import { useTranslation } from '../i18n';
import { useAppStore } from '../store/appStore';
import { completeStartup, startupFailed, useStartupStore, workspaceIsReady, workspaceLayoutIsReady, startupApplicationPhase } from '../startup';
import { requestNativeViewportLayout, type NativeViewportMetrics } from './viewport/nativeViewportBridge';
import { applyUiScale, currentUiScale } from '../uiScale';
import { measureInitialViewportInteraction } from '../startupMeasurement';
import { initializeStartupDocument } from '../startupInitialization';
import { currentDocumentLoad, useDocumentLoadStore } from '../files/documentLoadProgress';
import { DocumentLoadStatus } from './DocumentLoadStatus';

let rendererStart: Promise<void> | null = null;
let workspaceExpansion: Promise<void> | null = null;

function expandWorkspaceWindow(): Promise<void> {
  workspaceExpansion ??= invoke<void>('native_window_prepare_workspace').then(async () => {
    await applyUiScale(useAppStore.getState().uiScale).catch((error) => {
      console.warn('Could not restore workspace UI scale', error);
      useAppStore.setState({ uiScale: currentUiScale() });
    });
    document.documentElement.dataset.startupWindow = 'workspace';
    requestNativeViewportLayout();
  });
  return workspaceExpansion;
}
function startRendererAfterFeedback(): Promise<void> {
  if (window.__rendererStartPromise) return window.__rendererStartPromise;
  if (!rendererStart) {
    rendererStart = new Promise<void>((resolve) => {
      requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
    }).then(() => window.__rendererStartPromise ?? invoke<void>('native_viewport_start'));
  }
  return rendererStart;
}

export function StartupScreen() {
  const { t, locale } = useTranslation();
  const startup = useStartupStore();
  const fileLoading = useDocumentLoadStore((state) => currentDocumentLoad(state.operations) !== null);
  const [copied, setCopied] = useState(false);
  const [recovering, setRecovering] = useState(false);
  const [notes, setNotes] = useState(() => window.__startupNotes);
  useEffect(() => {
    const loaded = () => setNotes(window.__startupNotes);
    window.addEventListener('limo:startup-notes', loaded);
    loaded();
    return () => window.removeEventListener('limo:startup-notes', loaded);
  }, []);

  useEffect(() => {
    if (startup.completed) return;
    if (!isTauriRuntime()) {
      startupFailed(t('startup.browser'));
      return;
    }
    let disposed = false;
    let timer = 0;
    let fontsReady = false;
    let expanded = document.documentElement.dataset.startupWindow === 'workspace';
    let requiredRevision: number | null = null;
    let expansionStartedMs: number | null = null;
    const delayedTimer = window.setTimeout(() => {
      if (!disposed && !useStartupStore.getState().completed) useStartupStore.setState({ delayed: true });
    }, 30_000);
    const fail = (error: unknown, source = useStartupStore.getState().failureSource) => {
      window.clearTimeout(delayedTimer);
      window.clearTimeout(deadlineTimer);
      if (!disposed && !useStartupStore.getState().error) startupFailed(error, source);
    };
    // Includes document/GPU preparation and the expanded-window frame. Native
    // initial-loading timeout can finish after the compact frame succeeds.
    const deadlineTimer = window.setTimeout(() => fail(new Error(t('startup.timeout'))), 60_000);
    void document.fonts.ready.then(() => { fontsReady = true; }).catch((error) => fail(error, 'renderer'));
    const poll = async () => {
      try {
        const state = useStartupStore.getState();
        if (state.completed || state.error || disposed) {
          window.clearTimeout(delayedTimer);
          window.clearTimeout(deadlineTimer);
          return;
        }
        if (state.documentReady && requiredRevision === null) {
          requiredRevision = await invoke<number>('native_viewport_sync_model');
        }
        const metrics = await invoke<NativeViewportMetrics>('native_viewport_metrics');
        if (disposed || useStartupStore.getState().completed || useStartupStore.getState().error) return;
        if (metrics.startupError) {
          fail(new Error(metrics.startupError), 'renderer');
          return;
        }
        const sessionId = useAppStore.getState().activeProjectTabId;
        if (requiredRevision !== null && workspaceIsReady(state.documentReady, fontsReady, sessionId, metrics, requiredRevision)) {
          if (!expanded) {
            useStartupStore.setState({ phase: 'frame' });
            expansionStartedMs = performance.now();
            await expandWorkspaceWindow();
            if (disposed || useStartupStore.getState().completed || useStartupStore.getState().error) return;
            expanded = true;
            // The compact frame was real, but it is not the expanded layout.
            // Read fresh native metrics after the resize and zoom have applied.
            timer = window.setTimeout(() => void poll(), 0);
            return;
          }
          const viewport = document.querySelector('.native-viewport-surface')?.getBoundingClientRect() ?? null;
          const windowSize = {
            width: document.documentElement.clientWidth,
            height: document.documentElement.clientHeight,
          };
          if (!workspaceLayoutIsReady(metrics, viewport, windowSize, currentUiScale())) {
            useStartupStore.setState({ phase: 'frame' });
            requestNativeViewportLayout();
            timer = window.setTimeout(() => void poll(), 16);
            return;
          }
          if (!completeStartup()) return;
          window.clearTimeout(delayedTimer);
          window.clearTimeout(deadlineTimer);
          document.documentElement.dataset.startupReady = 'true';
          requestAnimationFrame(() => requestAnimationFrame(() => {
            const elapsedMs = performance.now();
            if (expansionStartedMs !== null) void invoke('startup_record', {
              phase: 'windowExpansion', elapsedMs, durationMs: elapsedMs - expansionStartedMs,
            }).catch((error) => console.warn('Window transition measurement failed', error));
            if (window.__startupTiming) window.__startupTiming.usableWorkspaceMs = elapsedMs;
            void invoke('startup_record', { phase: 'workspace', elapsedMs })
              .then(measureInitialViewportInteraction)
              .catch((error) => console.error('Startup measurement failed', error));
          }));
          return;
        }
        useStartupStore.setState({ phase: startupApplicationPhase(state.appPrepared, fontsReady, metrics) });
        // The resize is short and already rendering. Observe its completed
        // frame at frame cadence instead of adding another 50 ms phase wait.
        timer = window.setTimeout(() => void poll(), expanded ? 16 : 50);
      } catch (error) {
        fail(error, 'renderer');
      }
    };
    void startRendererAfterFeedback().then(poll).catch((error) => fail(error, 'renderer'));
    return () => {
      disposed = true;
      window.clearTimeout(timer);
      window.clearTimeout(delayedTimer);
      window.clearTimeout(deadlineTimer);
    };
    // Locale changes affect copy, but must not restart native initialization.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [startup.attempt, startup.completed]);

  if (startup.completed || startup.phase === 'ready') return null;
  const failed = startup.error !== null;
  const phaseKey = startup.phase;
  const details = startup.error ?? (startup.delayed ? t('startup.slow') : '');
  const copyDetails = async () => {
    try {
      await navigator.clipboard.writeText(`${t('app.name')}\n${t(`startup.${phaseKey}`)}\n${details}`);
      setCopied(true);
    } catch (error) {
      console.warn('Could not copy startup details', error);
    }
  };
  const retryDocument = async (empty = false) => {
    setRecovering(true);
    try {
      await initializeStartupDocument(empty);
    } catch (error) {
      startupFailed(error, 'document');
    }
    setRecovering(false);
  };
  return (
    <div className="startup-screen" data-native-viewport-overlay data-testid="startup-screen" data-failed={failed} data-document-loading={fileLoading}>
      <div className="startup-card">
        <div className="startup-drag" aria-hidden="true" data-tauri-drag-region />
        {isTauriRuntime() && <button className="startup-close" aria-label={t('startup.quit')} title={t('startup.quit')}
          onClick={() => void invoke('native_force_quit').catch((error) => startupFailed(error))}>
          <span aria-hidden="true">×</span>
        </button>}
        <header className="startup-brand">
          <h1 className="startup-name">{t('app.name')}</h1>
          <p className="startup-tagline">{t('startup.tagline')}</p>
          <img className="startup-drawing" src="/startup-part.svg" alt="" width="216" height="172" />
        </header>
        <div className="startup-body">
        <p className="startup-work-label">{t('startup.scope')}</p>
        <p className="startup-status" role={failed ? 'alert' : 'status'} aria-live="polite">
          <span className="startup-dot" aria-hidden="true" />{t(`startup.${phaseKey}`)}
        </p>
        {!failed && <DocumentLoadStatus startup />}
        {!failed && !startup.delayed && !recovering && notes?.[locale]?.[window.__startupNoteIndex ?? 0] && <aside className="startup-note" aria-label={t('startup.note')}>
          <span className="startup-note-label">{t('startup.note')}</span>
          <p>{notes[locale][window.__startupNoteIndex ?? 0]}</p>
        </aside>}
        {details && <p className="startup-detail">{details}</p>}
        {(failed || startup.delayed) && startup.failureSource === 'renderer' && <p className="startup-detail">{t('startup.reopen')}</p>}
        {(failed || startup.delayed) && <div className="startup-actions">
          {startup.failureSource === 'document' && <button disabled={recovering} onClick={() => void retryDocument()}>{t('startup.retry')}</button>}
          {startup.failureSource === 'document' && isTauriRuntime() && <button disabled={recovering} onClick={() => void retryDocument(true)}>{t('startup.empty')}</button>}
          <button onClick={() => void copyDetails()}>{t(copied ? 'startup.copied' : 'startup.copy')}</button>
          {isTauriRuntime() && <button onClick={() => void invoke('native_force_quit').catch((error) => startupFailed(error))}>{t('startup.quit')}</button>}
        </div>}
        </div>
      </div>
    </div>
  );
}
