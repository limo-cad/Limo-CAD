/** Preserve editing in fields, without mistaking focused toggle/buttons for text. */
export function isTextEditingTarget(target: EventTarget | null): target is HTMLElement {
  if (target instanceof HTMLInputElement) {
    return !['checkbox', 'radio', 'button', 'submit', 'reset', 'image'].includes(target.type);
  }
  return target instanceof HTMLTextAreaElement
    || (target instanceof HTMLElement && target.isContentEditable);
}

/** Resolve File shortcuts while preserving focused field editing. */
export function applicationFileShortcut(event: KeyboardEvent): 'save' | 'open' | 'new' | null {
  if (isTextEditingTarget(event.target) || !(event.metaKey || event.ctrlKey)) return null;
  switch (event.key.toLowerCase()) {
    case 's': return 'save';
    case 'o': return 'open';
    case 'n': return 'new';
    default: return null;
  }
}

/** Model shortcuts must not consume keys in a focused script companion.
 * Apply this before capture-phase sketch input and Escape cancellation. */
export function listenForModelKeys(listener: (event: KeyboardEvent) => void, capture = false): () => void {
  const route = (event: KeyboardEvent) => {
    // Settings owns focus and Escape while open, including before its capture
    // listener runs. A modal dismissal must never cancel the design underneath.
    if (document.querySelector('[data-settings-dialog]')) return;
    // A hover preview can be open while focus remains in the viewport. Its
    // document-level Escape handler gets first refusal, before CAD cancellation.
    if (event.key === 'Escape' && document.querySelector('[data-feature-script-preview]')) return;
    if (event.target instanceof Element) {
      if (event.key === 'Escape' && event.target.closest('[data-script-preview-pending]')) return;
      if (event.key === 'ArrowDown' && event.target.closest('[data-script-preview-trigger]')) return;
    }
    if (event.target instanceof Element && event.target.closest('[data-script-companion], [data-runtime-activity-panel]')) return;
    listener(event);
  };
  window.addEventListener('keydown', route, capture);
  return () => window.removeEventListener('keydown', route, capture);
}
