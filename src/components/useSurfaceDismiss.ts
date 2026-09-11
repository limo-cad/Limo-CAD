import { useEffect, useRef, type KeyboardEvent } from 'react';

/** Dismiss only the focused companion surface, leaving CAD and playback alone. */
export function useSurfaceDismiss(open: boolean, close: () => void, openerSelector: string) {
  const surface = useRef<HTMLElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  const restoreFrame = useRef<number | null>(null);
  useEffect(() => () => {
    if (restoreFrame.current !== null) cancelAnimationFrame(restoreFrame.current);
  }, []);
  useEffect(() => {
    if (open && document.activeElement instanceof HTMLElement
      && !surface.current?.contains(document.activeElement)) opener.current = document.activeElement;
  }, [open]);
  const dismiss = () => {
    const restore = surface.current?.contains(document.activeElement);
    const closing = surface.current;
    close();
    if (restoreFrame.current !== null) cancelAnimationFrame(restoreFrame.current);
    if (restore) restoreFrame.current = requestAnimationFrame(() => {
      restoreFrame.current = null;
      // A reopened surface or newer focus choice wins over this dismissal.
      if (surface.current || (document.activeElement !== document.body && !closing?.contains(document.activeElement))) return;
      const target = opener.current;
      if (target?.isConnected && target !== document.body && !target.matches(':disabled') && target.getClientRects().length) target.focus();
      else document.querySelector<HTMLElement>(openerSelector)?.focus();
    });
  };
  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key !== 'Escape' || event.defaultPrevented) return;
    event.preventDefault();
    event.stopPropagation();
    dismiss();
  };
  return { surface, dismiss, onKeyDown };
}
