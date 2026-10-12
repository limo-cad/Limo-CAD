import { translate } from '../i18n';

const changedMessage = () => translate('file.errorDocumentChangedDuringExport');

export interface ProjectTransitionRelease {
  (changed?: boolean, published?: boolean): void;
  /** Wait for earlier native/UI changes and drain snapshots before replacing. */
  waitForSnapshots(): Promise<void>;
}

/** Own a native project change until its frontend state has been published.
 * A tab ID or store object cannot identify the model during that interval. */
export class ProjectTransitions {
  private pending = new Map<ProjectTransitionRelease, Promise<void>>();
  private snapshots = new Set<Promise<void>>();
  private revision = 0;
  private published = true;
  private listeners = new Set<() => void>();

  /** Background reads must yield until native changes and publication finish. */
  isSettled(): boolean { return this.published && !this.pending.size && !this.snapshots.size; }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  }

  private notify(): void { for (const listener of this.listeners) listener(); }

  begin(): ProjectTransitionRelease {
    const previous = [...this.pending.values()];
    let settled!: () => void;
    const pending = new Promise<void>(resolve => { settled = resolve; });
    let released = false;
    const release: ProjectTransitionRelease = Object.assign((changed = true, published = true) => {
      if (released) return;
      released = true;
      this.pending.delete(release);
      if (changed) {
        this.revision++;
        this.published = published;
      }
      settled();
      this.notify();
    }, {waitForSnapshots: async () => {
      // Preserve native/UI hydration order for competing Open/tab/inbox work.
      // Only predecessors are awaited, so queued writers cannot wait on one
      // another in a cycle. No transition is held across a file picker.
      await Promise.all(previous);
      while (this.snapshots.size) await Promise.all(this.snapshots);
    }});
    this.pending.set(release, pending);
    this.notify();
    return release;
  }

  /** Background polling yields to existing snapshots instead of invalidating
   * a Save/export just to discover that its native inbox is empty. Explicit
   * document replacements still use begin() and retain their ownership fence. */
  tryBegin(): ProjectTransitionRelease | null {
    if (this.pending.size || this.snapshots.size) return null;
    return this.begin();
  }

  capture(): number {
    return this.revision;
  }

  hasSnapshots(): boolean { return this.snapshots.size > 0; }

  assertSettled(revision: number): void {
    this.assertPublished(revision);
    if (this.pending.size) throw new Error(changedMessage());
  }

  assertPublished(revision: number): void {
    this.assertRevision(revision);
    if (!this.published) throw new Error(changedMessage());
  }

  /** Replacement can recover an unpublished model, but cannot borrow consent
   * from before a different native/UI transition. */
  assertRevision(revision: number): void {
    if (revision !== this.revision) throw new Error(changedMessage());
  }

  /** Publication may run inside its own applied inbox operation, after that
   * operation has hydrated the UI. It must never borrow another transition. */
  beginSnapshot(owner?: ProjectTransitionRelease): {assertCurrent(): void; assertOwned(): void; release(): void} {
    const revision = this.revision;
    const assertCurrent = () => {
      if (!this.published || revision !== this.revision
        || [...this.pending.keys()].some(transition => transition !== owner)) throw new Error(changedMessage());
    };
    assertCurrent();
    let settled!: () => void;
    const snapshot = new Promise<void>(resolve => { settled = resolve; });
    this.snapshots.add(snapshot);
    this.notify();
    return {assertCurrent, assertOwned: () => this.assertPublished(revision), release: () => {
      this.snapshots.delete(snapshot);
      settled();
      this.notify();
    }};
  }

  async assertCurrent(revision: number): Promise<void> {
    // A connected CAD polls continuously. Wait for an outstanding empty poll;
    // it must not randomly reject a valid export or invalidate its ownership.
    while (this.pending.size) await Promise.all(this.pending.values());
    this.assertSettled(revision);
  }
}

export const projectTransitions = new ProjectTransitions();
