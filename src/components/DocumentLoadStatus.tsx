import { useTranslation } from '../i18n';
import { currentDocumentLoad, useDocumentLoadStore } from '../files/documentLoadProgress';

/** A small document-local notice. It has no input shield and does not change
 * project ownership, edit guards, renderer state, or the startup overlay. */
export function DocumentLoadStatus({ startup = false }: { startup?: boolean }) {
  const { t } = useTranslation();
  const loading = useDocumentLoadStore((state) => currentDocumentLoad(state.operations));
  if (!loading) return null;
  return <div
    className={startup ? 'startup-file-status' : 'absolute left-3 top-3 z-20 max-w-[calc(100%-24px)] rounded-md border border-edge bg-panel px-3 py-2 text-ink shadow-sm pointer-events-none'}
    data-native-viewport-overlay={startup ? undefined : true}
    data-testid="document-load-status"
    role="status" aria-live="polite"
  >
    <div className="truncate text-xs text-mute" title={loading.name}>{loading.name}</div>
    <p className="m-0 mt-1 text-xs">{t(`documentLoad.${loading.phase}`)}</p>
  </div>;
}
