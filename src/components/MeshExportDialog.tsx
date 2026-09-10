import { useEffect, useRef, useState } from 'react';
import type { MeshExportScope } from '../engine/types';
import { useTranslation } from '../i18n';

let pending: ((scope: MeshExportScope | null) => void) | null = null;
let previousFocus: HTMLElement | null = null;
const changeEvent = 'nbcad:mesh-export-options';

/** Both File menus use this choice before opening the native save dialog. */
export function requestMeshExportScope(): Promise<MeshExportScope | null> {
  if (pending) return Promise.resolve(null);
  previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  return new Promise((resolve) => {
    pending = resolve;
    window.dispatchEvent(new Event(changeEvent));
  });
}

function finish(scope: MeshExportScope | null) {
  const resolve = pending;
  pending = null;
  window.dispatchEvent(new Event(changeEvent));
  resolve?.(scope);
}

export function MeshExportDialog() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(() => pending !== null);
  const [scope, setScope] = useState<MeshExportScope>('assembly');
  const dialog = useRef<HTMLElement>(null);
  useEffect(() => {
    const sync = () => {
      setOpen(pending !== null);
      setScope('assembly');
    };
    window.addEventListener(changeEvent, sync);
    return () => window.removeEventListener(changeEvent, sync);
  }, []);
  useEffect(() => {
    if (!open) return undefined;
    const restoreFocus = previousFocus;
    const keydown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        event.stopPropagation();
        finish(null);
      } else if (event.key === 'Tab') {
        const controls = [...(dialog.current?.querySelectorAll<HTMLElement>('button:not(:disabled),input:not(:disabled)') ?? [])];
        if (!controls.length) return;
        const index = controls.indexOf(document.activeElement as HTMLElement);
        const next = index < 0 ? 0 : (index + (event.shiftKey ? -1 : 1) + controls.length) % controls.length;
        event.preventDefault();
        event.stopPropagation();
        controls[next].focus();
      }
    };
    window.addEventListener('keydown', keydown, true);
    return () => {
      window.removeEventListener('keydown', keydown, true);
      if (restoreFocus?.isConnected) restoreFocus.focus({ preventScroll: true });
    };
  }, [open]);
  if (!open) return null;
  return (
    <div data-native-viewport-dim="0.45" className="fixed inset-0 z-[200] flex items-center justify-center bg-black/45 p-5">
      <section ref={dialog} role="dialog" aria-modal="true" aria-labelledby="mesh-export-title" data-testid="mesh-export-options" className="feature-dialog w-[450px] max-w-full bg-panel text-ink">
        <header className="flex items-center justify-between border-b border-edge bg-header px-4 py-3">
          <h2 id="mesh-export-title" className="text-sm font-semibold">{t('meshExport.title')}</h2>
          <button type="button" aria-label={t('meshExport.close')} onClick={() => finish(null)}>×</button>
        </header>
        <div className="space-y-3 px-4 py-4 text-sm">
          <label className="block">
            <input type="radio" name="mesh-export-scope" value="assembly" checked={scope === 'assembly'} onChange={() => setScope('assembly')} autoFocus /> {t('meshExport.assembly')}
            <span className="mt-1 block text-xs text-mute">{t('meshExport.assemblyHelp')}</span>
          </label>
          <label className="block">
            <input type="radio" name="mesh-export-scope" value="definition" checked={scope === 'definition'} onChange={() => setScope('definition')} /> {t('meshExport.definition')}
            <span className="mt-1 block text-xs text-mute">{t('meshExport.definitionHelp')}</span>
          </label>
        </div>
        <footer className="flex justify-end gap-2 border-t border-edge bg-header px-4 py-3">
          <button type="button" onClick={() => finish(null)} className="h-8 rounded border border-edge px-3 text-xs">{t('file.cancel')}</button>
          <button type="button" onClick={() => finish(scope)} className="h-8 rounded bg-accent px-3 text-xs text-white">{t('meshExport.continue')}</button>
        </footer>
      </section>
    </div>
  );
}
