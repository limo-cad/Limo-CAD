const changedMessage = 'The document changed while choosing mesh export options. Start the export again.';

/** Own a native project change until its frontend state has been published.
 * A tab ID or store object cannot identify the model during that interval. */
export class ProjectTransitions {
  private pending = new Set<Promise<void>>();
  private revision = 0;
  private published = true;

  begin(): (changed?: boolean, published?: boolean) => void {
    let settled!: () => void;
    const pending = new Promise<void>(resolve => { settled = resolve; });
    this.pending.add(pending);
    let released = false;
    return (changed = true, published = true) => {
      if (released) return;
      released = true;
      this.pending.delete(pending);
      if (changed) {
        this.revision++;
        this.published = published;
      }
      settled();
    };
  }

  capture(): number {
    return this.revision;
  }

  async assertCurrent(revision: number): Promise<void> {
    // A connected CAD polls continuously. Wait for an outstanding empty poll;
    // it must not randomly reject a valid export or invalidate its ownership.
    while (this.pending.size) await Promise.all(this.pending);
    if (!this.published || revision !== this.revision) throw new Error(changedMessage);
  }
}

export const projectTransitions = new ProjectTransitions();
