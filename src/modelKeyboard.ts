/** Model shortcuts must not consume keys in a focused script companion.
 * Apply this before capture-phase sketch input and Escape cancellation. */
export function listenForModelKeys(listener: (event: KeyboardEvent) => void, capture = false): () => void {
  const route = (event: KeyboardEvent) => {
    // A hover preview can be open while focus remains in the viewport. Its
    // document-level Escape handler gets first refusal, before CAD cancellation.
    if (event.key === 'Escape' && document.querySelector('[data-feature-script-preview]')) return;
    if (event.target instanceof Element) {
      if (event.key === 'Escape' && event.target.closest('[data-script-preview-pending]')) return;
      if (event.key === 'ArrowDown' && event.target.closest('[data-script-preview-trigger]')) return;
    }
    if (event.target instanceof Element && event.target.closest('[data-script-companion]')) return;
    listener(event);
  };
  window.addEventListener('keydown', route, capture);
  return () => window.removeEventListener('keydown', route, capture);
}
