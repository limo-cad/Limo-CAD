import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Activity, Check, CircleAlert, Loader2, X } from 'lucide-react';
import { useTranslation } from '../i18n';
import {
  dismissRuntimeNotification, RUNTIME_ACTIVITY_GRACE_MS, useRuntimeActivityStore,
  visibleRuntimeActivities, type RuntimeActivityResult,
} from '../runtimeActivity';
import { requestNativeViewportLayout } from './viewport/nativeViewportBridge';
import { useSurfaceDismiss } from './useSurfaceDismiss';

/** Activity and notifications live in window chrome. The optional panel only
 * occludes its own bounds; it never shields the canvas or changes CAD guards. */
export function ActivityIndicator() {
  const { t } = useTranslation();
  const { active, history, notifications } = useRuntimeActivityStore();
  const [open, setOpen] = useState(false);
  const [now, setNow] = useState(() => performance.now());
  const [copied, setCopied] = useState<number | null>(null);
  const anchor = useRef<HTMLButtonElement>(null);
  const [position, setPosition] = useState({ top: 30, right: 8 });
  const close = useCallback(() => setOpen(false), []);
  const { surface, dismiss, onKeyDown } = useSurfaceDismiss(open, close, '[data-testid="activity-button"]');
  const visible = visibleRuntimeActivities(active, now);
  const latest = notifications[0];
  const hasFailure = notifications.some(result => result.status === 'failed');

  useEffect(() => {
    if (open) surface.current?.focus();
  }, [open, surface]);

  useEffect(() => {
    setNow(performance.now());
    if (!active.length) return;
    let timer = 0;
    const tick = () => {
      const current = performance.now();
      setNow(current);
      const remaining = active.map(task => RUNTIME_ACTIVITY_GRACE_MS - (current - task.startedAt)).filter(ms => ms > 0);
      timer = window.setTimeout(tick, remaining.length ? Math.ceil(Math.min(...remaining)) : 1000);
    };
    tick();
    return () => window.clearTimeout(timer);
  }, [active]);

  const updatePosition = useCallback(() => {
    const rect = anchor.current?.getBoundingClientRect();
    if (!rect) return;
    setPosition({ top: Math.min(rect.bottom + 6, window.innerHeight - 48), right: Math.max(8, window.innerWidth - rect.right) });
    requestNativeViewportLayout();
  }, []);
  useLayoutEffect(() => {
    if (!open) return;
    updatePosition();
    const observer = new ResizeObserver(() => { requestNativeViewportLayout(); });
    if (surface.current) observer.observe(surface.current);
    window.addEventListener('resize', updatePosition);
    const outside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!surface.current?.contains(target) && !anchor.current?.contains(target)) close();
    };
    document.addEventListener('pointerdown', outside);
    return () => {
      observer.disconnect();
      window.removeEventListener('resize', updatePosition);
      document.removeEventListener('pointerdown', outside);
      requestNativeViewportLayout();
    };
  }, [open, close, surface, updatePosition]);
  useLayoutEffect(() => { requestNativeViewportLayout(); }, [open, active, notifications, history]);

  const elapsed = (started: number, ended = now) => t('runtimeActivity.elapsed').replace('{seconds}', String(Math.max(0, Math.floor((ended - started) / 1000))));
  const taskName = (task: { kind: string; target?: string }) => `${t(`runtimeActivity.kind.${task.kind}`)}${task.target ? ` · ${task.target}` : ''}`;
  const copy = async (result: RuntimeActivityResult) => {
    try {
      await navigator.clipboard.writeText(`${taskName(result)}\n${t(`runtimeActivity.phase.${result.phase}`)}\n${result.error ?? ''}`);
      setCopied(result.id);
    } catch { setCopied(null); }
  };
  const title = active.length ? `${t('runtimeActivity.title')} (${active.length})`
    : latest ? `${t(`runtimeActivity.${latest.status}`)} · ${taskName(latest)}` : t('runtimeActivity.title');
  return <>
    <button ref={anchor} type="button" data-testid="activity-button" aria-haspopup="dialog" aria-expanded={open}
      title={title} onClick={() => setOpen(value => !value)}
      className="flex shrink-0 items-center gap-1.5 border-l border-edge px-2 text-xs text-mute hover:bg-edge hover:text-ink">
      {hasFailure ? <CircleAlert size={12} className="text-warn" />
        : active.length ? <Loader2 size={12} className="animate-spin motion-reduce:animate-none" />
        : latest ? <Check size={12} className="text-emerald-500" /> : <Activity size={12} />}
      <span className="max-w-40 truncate">{visible.length ? t(`runtimeActivity.phase.${visible[0].phase}`)
        : latest && !active.length ? t(`runtimeActivity.${latest.status}`) : t('runtimeActivity.title')}</span>
      {(active.length > 0 || notifications.length > 0) && <span className="text-[10px]">{active.length || notifications.length}</span>}
    </button>
    <span className="sr-only" role="status" aria-live="polite">{latest ? `${t(`runtimeActivity.${latest.status}`)} · ${taskName(latest)}` : ''}</span>
    {open && createPortal(<section ref={surface} tabIndex={-1} onKeyDown={onKeyDown} role="dialog" aria-label={t('runtimeActivity.title')}
      data-testid="activity-panel" data-runtime-activity-panel data-native-viewport-overlay
      className="fixed z-[110] w-80 max-w-[calc(100vw-16px)] overflow-auto rounded-lg border border-edge bg-panel text-ink shadow-xl"
      style={{ ...position, maxHeight: Math.max(48, window.innerHeight - position.top - 8) }}>
      <header className="flex items-center justify-between border-b border-edge px-3 py-2 text-xs font-medium">
        {t('runtimeActivity.title')}<button type="button" onClick={dismiss} aria-label={t('runtimeActivity.close')} className="rounded p-1 hover:bg-edge"><X size={12} /></button>
      </header>
      {!active.length && !history.length && <p className="p-3 text-xs text-mute">{t('runtimeActivity.empty')}</p>}
      {active.length > 0 && <div className="border-b border-edge p-3">
        <h2 className="mb-2 text-[10px] text-mute">{t('runtimeActivity.current')}</h2>
        {visible.length === 0 && <p className="text-xs text-mute">{t('runtimeActivity.starting')}</p>}
        {visible.map(task => <div key={task.id} className="mb-2 last:mb-0" role="status">
          <p className="truncate text-xs" title={taskName(task)}>{taskName(task)}</p>
          <p className="mt-1 flex justify-between gap-2 text-[11px] text-mute"><span>{t(`runtimeActivity.phase.${task.phase}`)}</span><span className="shrink-0">{elapsed(task.startedAt)}</span></p>
        </div>)}
      </div>}
      {notifications.map(result => <div key={result.id} role={result.status === 'failed' ? 'alert' : 'status'} className="border-b border-edge p-3">
        <div className="flex items-start justify-between gap-2"><p className="min-w-0 break-words text-xs">{t(`runtimeActivity.${result.status}`)} · {taskName(result)}</p>
          <button type="button" className="shrink-0 rounded p-1 hover:bg-edge" aria-label={t('runtimeActivity.dismiss')} onClick={() => dismissRuntimeNotification(result.id)}><X size={12} /></button></div>
        {result.error && <details className="mt-2 text-xs"><summary className="cursor-pointer text-mute">{t('runtimeActivity.details')}</summary>
          <p className="my-2 max-h-28 overflow-auto whitespace-pre-wrap break-words select-text">{result.error}</p>
          <button type="button" onClick={() => void copy(result)} className="rounded border border-edge px-2 py-1 hover:bg-edge">{t(copied === result.id ? 'runtimeActivity.copied' : 'runtimeActivity.copy')}</button>
        </details>}
      </div>)}
      {history.length > 0 && <div className="p-3"><h2 className="mb-2 text-[10px] text-mute">{t('runtimeActivity.recent')}</h2>
        {history.map(result => <div key={result.id} className="mb-2 text-[11px] last:mb-0">
          <p className="truncate" title={taskName(result)}>{taskName(result)}</p>
          <p className="mt-0.5 flex justify-between gap-2 text-mute"><span>{t(`runtimeActivity.${result.status}`)}</span><span>{elapsed(result.startedAt, result.endedAt)}</span></p>
        </div>)}
      </div>}
    </section>, document.body)}
  </>;
}
