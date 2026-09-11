/** Keep shutdown behind any live control acknowledgement (including a click
 * on Discard). The caller receives its reply before the webview disappears. */
export class ExitBarrier {
  private pending = new Set<Promise<void>>();

  hold(): () => void {
    let release!: () => void;
    const promise = new Promise<void>(resolve => { release = resolve; });
    this.pending.add(promise);
    return () => { this.pending.delete(promise); release(); };
  }

  async wait(): Promise<void> {
    while (this.pending.size) await Promise.all(this.pending);
  }
}

export const applicationExitBarrier = new ExitBarrier();

interface ExitActions {
  dirty(): boolean;
  decide(): Promise<'save' | 'discard' | 'cancel'>;
  save(): Promise<boolean>;
  exit(): Promise<void>;
  error(error: unknown): void;
}

/** One guarded route for native close, application Quit, and the interface. */
export function createExitController(actions: ExitActions, barrier = applicationExitBarrier) {
  let pending = false;
  let disposed = false;
  return {
    dispose() { disposed = true; },
    async request() {
      if (pending || disposed) return;
      pending = true;
      try {
        while (!disposed) {
          // A control already in progress may create unsaved work. Decide only
          // after it completes, rather than trusting its pre-operation state.
          await barrier.wait();
          if (disposed) return;
          let discard = false;
          if (actions.dirty()) {
            const decision = await actions.decide();
            if (decision === 'cancel' || disposed) return;
            discard = decision === 'discard';
            if (decision === 'save' && !(await actions.save())) return;
          }
          await barrier.wait();
          if (disposed) return;
          // A late control can create more work after Save succeeds. Its
          // acknowledgement releases shutdown, but does not save that work.
          // An explicit Discard remains authorization to finish quitting.
          if (!discard && actions.dirty()) continue;
          await actions.exit();
          return;
        }
      } catch (error) {
        actions.error(error);
      } finally {
        pending = false;
      }
    },
  };
}

export function requestApplicationExit(): void {
  window.dispatchEvent(new Event('nbcad:quit-request'));
}
