import { cloneElement, isValidElement, useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { ensureScriptExamples, errorMessage, previewExample, showScriptExample, type ScriptExample, type ScriptPreviewFrame } from '../scripts/workspace';
import { ScriptPreview } from './ScriptPreview';

/** The ribbon supplies its existing catalog operation; examples declare the
 * operations their Rust scripts actually execute. There is no second tool list. */
export function FeatureScriptPreview({ children, group, operation, label, disabled = false }: {
  children: ReactNode; group: string; operation: string; label: string; disabled?: boolean;
}) {
  const [position, setPosition] = useState<{ left: number; top: number } | null>(null);
  const [example, setExample] = useState<ScriptExample | null>(null);
  const [frames, setFrames] = useState<ScriptPreviewFrame[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const anchor = useRef<HTMLSpanElement>(null);
  const card = useRef<HTMLDivElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const available = useRef<ScriptExample | null>(null);
  const id = useId();
  const clearTimer = () => { if (timer.current !== null) clearTimeout(timer.current); timer.current = null; };
  const close = () => { clearTimer(); setPosition(null); };
  const contains = (node: EventTarget | null) => node instanceof Node && (anchor.current?.contains(node) || card.current?.contains(node));
  const scheduleOpen = () => {
    clearTimer();
    if (position) return;
    timer.current = setTimeout(() => {
      if (!available.current) return;
      const rect = anchor.current?.getBoundingClientRect();
      if (!rect) return;
      setPosition({ left: Math.max(8, Math.min(rect.left, window.innerWidth - 336)), top: Math.max(8, Math.min(rect.bottom + 6, window.innerHeight - 380)) });
    }, 600);
  };
  const scheduleClose = (relatedTarget: EventTarget | null) => {
    if (contains(relatedTarget)) return;
    clearTimer();
    // A short bridge across the visual gap lets the pointer reach the controls.
    timer.current = setTimeout(() => setPosition(null), 140);
  };

  useEffect(() => () => clearTimer(), []);
  useEffect(() => {
    let cancelled = false;
    available.current = null;
    setExample(null);
    setPosition(null);
    // Load the catalog once, but compute geometry only when a supported preview
    // is actually opened. Unsupported operations keep their ordinary button.
    void ensureScriptExamples().then(examples => {
      if (cancelled) return;
      const found = examples.find(item => item.preview && item.operations.includes(operation)) ?? null;
      available.current = found;
      setExample(found);
    }).catch(() => { /* Catalog unavailable: leave the modeling button intact. */ });
    return () => { cancelled = true; available.current = null; };
  }, [operation]);
  useEffect(() => {
    if (!position || !example) return;
    let cancelled = false;
    setFrames(null);
    setError(null);
    void previewExample(example).then(preview => {
      if (!cancelled) setFrames(preview);
    }).catch(failure => { if (!cancelled) setError(errorMessage(failure)); });
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      event.stopPropagation();
      if (card.current?.contains(document.activeElement)) anchor.current?.querySelector('button')?.focus();
      close();
    };
    const onPointerDown = (event: PointerEvent) => { if (!contains(event.target)) close(); };
    const onResize = () => close();
    document.addEventListener('keydown', onKeyDown, true);
    document.addEventListener('pointerdown', onPointerDown);
    window.addEventListener('resize', onResize);
    return () => {
      cancelled = true;
      document.removeEventListener('keydown', onKeyDown, true);
      document.removeEventListener('pointerdown', onPointerDown);
      window.removeEventListener('resize', onResize);
    };
  }, [position, example]);

  return (
    <>
      <span
        ref={anchor}
        className="inline-flex shrink-0"
        tabIndex={disabled && example ? 0 : undefined}
        aria-label={disabled && example ? `${label} example` : undefined}
        aria-describedby={position ? id : undefined}
        onMouseEnter={scheduleOpen}
        onMouseLeave={event => { if (!contains(document.activeElement)) scheduleClose(event.relatedTarget); }}
        onFocus={scheduleOpen}
        onBlur={event => scheduleClose(event.relatedTarget)}
        onClickCapture={close}
        onKeyDown={event => {
          if (event.key === 'ArrowDown' && position) {
            event.preventDefault();
            event.stopPropagation();
            card.current?.querySelector('button')?.focus();
          }
        }}
      >{example && isValidElement<{ 'aria-describedby'?: string; 'aria-haspopup'?: 'dialog'; 'aria-expanded'?: boolean }>(children)
          ? cloneElement(children, { 'aria-describedby': position ? id : undefined, 'aria-haspopup': 'dialog', 'aria-expanded': !!position })
          : children}</span>
      {position && example && createPortal(
        <div
          ref={card}
          id={id}
          role="dialog"
          aria-label={`${label} example`}
          data-feature-script-preview
          data-native-viewport-overlay
          data-interface-group={group}
          data-interface-operation={operation}
          className="fixed z-[110] w-[320px] max-w-[calc(100vw-16px)] overflow-auto rounded-lg border border-edge bg-panel p-3 text-[11px] text-ink shadow-xl shadow-black/30"
          style={{ left: position.left, top: position.top, maxHeight: 'calc(100vh - 16px)' }}
          onMouseEnter={clearTimer}
          onMouseLeave={event => { if (!contains(document.activeElement)) scheduleClose(event.relatedTarget); }}
          onFocus={clearTimer}
          onBlur={event => scheduleClose(event.relatedTarget)}
        >
          <div className="mb-2 flex items-start gap-2">
            <div className="min-w-0 flex-1">
              <p className="font-semibold">{example?.name ?? `${label} example`}</p>
              <p className="mt-0.5 text-[10px] text-mute">Separate example · your model stays unchanged</p>
            </div>
            <button type="button" aria-label="Close feature preview" title="Close (Escape)" className="rounded px-1.5 py-0.5 text-mute hover:bg-edge hover:text-ink" onClick={close}>×</button>
          </div>
          {error ? <p role="status" className="py-4 leading-4 text-mute">{error}</p> : frames ? <ScriptPreview frames={frames} /> : <p role="status" className="flex h-44 items-center justify-center text-mute">Building the small example…</p>}
          {example && <button type="button" className="mt-3 w-full rounded border border-edge px-2 py-1.5 text-left hover:bg-edge" onClick={() => {
            void showScriptExample(example).then(close).catch(failure => setError(errorMessage(failure)));
          }}>Open this script →</button>}
        </div>,
        document.body,
      )}
    </>
  );
}
