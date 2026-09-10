/** Presentation observes the same ordered modeling operations as the UI. */
export class SerialPlayback {
  private running = false;
  async tick(work: () => Promise<void>): Promise<boolean> {
    if (this.running) return false;
    this.running = true;
    try {
      await work();
      return true;
    } finally { this.running = false; }
  }
}

export interface PresentationRequest {
  command?: 'configure' | 'note' | 'pause' | 'resume' | 'step' | 'status' | 'finish' | 'stop' | 'dismiss' | 'show';
  mode?: 'fast' | 'present';
  speed?: number;
  duration_ms?: number;
  text?: string;
  chapter?: string;
  step_index?: number;
  step_count?: number;
}
export interface PresentationSnapshot {
  active: boolean;
  visible: boolean;
  mode: 'fast' | 'present';
  speed: number;
  paused: boolean;
  stopped: boolean;
  finished: boolean;
  text: string;
  chapter: string;
  operation: string;
  step_index: number;
  step_count: number;
  highlighted_body_ids: number[];
  highlighted_sketch_entity_ids: number[];
}

/** A clock-injected gate makes pause/speed/step behavior independent of timers. */
export class PresentationController {
  private state: PresentationSnapshot = { active: false, visible: false, mode: 'fast', speed: 1,
    paused: false, stopped: false, finished: false, text: '', chapter: '', operation: '',
    step_index: 0, step_count: 0, highlighted_body_ids: [], highlighted_sketch_entity_ids: [] };
  private remainingMs = 0;
  private lastClock = 0;
  private credits = 0;
  private paceMs = 0;
  private documentRevision = 0;
  private listeners = new Set<() => void>();
  constructor(private clock: () => number = Date.now) { this.lastClock = clock(); }
  snapshot = (): PresentationSnapshot => this.state;
  documentVersion = (): number => this.documentRevision;
  /** New/Open/tab hydration replaces document UI, unlike an ordinary edit or
   * Save. Async operations use this revision to recognize their document. */
  documentChanged(): void {
    this.documentRevision += 1;
    this.remainingMs = 0; this.credits = 0; this.paceMs = 0;
    this.lastClock = this.clock();
    this.emit({active: false, visible: false, mode: 'fast', speed: 1,
      paused: false, stopped: false, finished: false, text: '', chapter: '', operation: '',
      step_index: 0, step_count: 0, highlighted_body_ids: [], highlighted_sketch_entity_ids: []});
  }
  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener); return () => { this.listeners.delete(listener); };
  };
  private advance(): void {
    const now = this.clock();
    if (!this.state.paused) this.remainingMs = Math.max(0, this.remainingMs - (now - this.lastClock) * this.state.speed);
    this.lastClock = now;
  }
  private emit(patch: Partial<PresentationSnapshot>): void {
    this.state = { ...this.state, ...patch };
    for (const listener of this.listeners) listener();
  }
  status(): PresentationSnapshot & { wait_ms: number; step_pending: boolean } {
    this.advance();
    return { ...this.state, wait_ms: this.state.mode === 'fast' ? 0 : Math.ceil(this.remainingMs / this.state.speed),
      step_pending: this.credits > 0 };
  }
  configurePace(ms: number): void {
    if (!Number.isInteger(ms) || ms < 0 || ms > 2000) throw new Error('pace_ms must be an integer from 0 to 2000');
    this.control({ command: 'configure', mode: ms ? 'present' : 'fast' });
    this.paceMs = ms;
  }
  control(request: PresentationRequest): ReturnType<PresentationController['status']> {
    const command = request.command ?? 'status';
    if (!['configure', 'note', 'pause', 'resume', 'step', 'status', 'finish', 'stop', 'dismiss', 'show'].includes(command)) throw new Error('Unknown presentation command');
    if (request.mode !== undefined && !['fast', 'present'].includes(request.mode)) throw new Error('mode must be fast or present');
    if (request.speed !== undefined && (!Number.isFinite(request.speed) || request.speed < 0.1 || request.speed > 16)) throw new Error('speed must be from 0.1 to 16');
    if (request.duration_ms !== undefined && (!Number.isInteger(request.duration_ms) || request.duration_ms < 0 || request.duration_ms > 10000)) throw new Error('duration_ms must be an integer from 0 to 10000');
    for (const key of ['text', 'chapter'] as const) {
      const value = request[key];
      const limit = key === 'text' ? 4000 : 200;
      if (value !== undefined && (typeof value !== 'string' || value.length > limit || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/.test(value))) throw new Error(`${key} must contain at most ${limit} printable characters`);
    }
    for (const key of ['step_index', 'step_count'] as const) if (request[key] !== undefined && (!Number.isSafeInteger(request[key]) || request[key]! < 0)) throw new Error(`${key} must be a nonnegative integer`);
    const index = request.step_index ?? this.state.step_index;
    const count = request.step_count ?? this.state.step_count;
    if (count && index > count) throw new Error('step_index cannot exceed step_count');
    if (command === 'finish' && this.state.stopped) throw new Error('Playback was stopped; it cannot be marked complete');
    if (command === 'show' && !this.state.active) throw new Error('No playback is available to show');
    this.advance();
    if (command === 'status') return this.status();
    if (command === 'dismiss' || command === 'show') {
      // Visibility is independent of execution. In particular, hiding a
      // paused run neither resumes it nor consumes its single-step permit.
      this.emit({ visible: command === 'show' });
      return this.status();
    }
    if (command === 'configure') this.paceMs = 0;
    const patch: Partial<PresentationSnapshot> = { active: true };
    if (!this.state.active) patch.visible = true;
    if (request.mode !== undefined) patch.mode = request.mode;
    if (request.speed !== undefined) patch.speed = request.speed;
    if (request.text !== undefined) patch.text = request.text;
    if (request.chapter !== undefined) patch.chapter = request.chapter;
    if (request.step_index !== undefined) patch.step_index = index;
    if (request.step_count !== undefined) patch.step_count = count;
    if (command === 'configure' && (this.state.finished || this.state.stopped)) {
      Object.assign(patch, { visible: true, paused: false, finished: false, stopped: false, operation: '',
        highlighted_body_ids: [], highlighted_sketch_entity_ids: [] });
      this.credits = 0; this.remainingMs = 0;
    }
    if (command === 'note') this.remainingMs = request.duration_ms ?? 0;
    if (command === 'pause') { patch.paused = true; this.credits = 0; }
    if (command === 'resume') {
      if (this.state.stopped) throw new Error('Playback stopped; configure a new run before resuming');
      patch.paused = false; this.credits = 0;
    }
    if (command === 'step') {
      if (this.state.stopped) throw new Error('Playback stopped; configure a new run before stepping');
      patch.paused = true; this.credits += 1;
    }
    if (command === 'finish' || command === 'stop') {
      Object.assign(patch, { paused: command === 'stop', stopped: command === 'stop', finished: command === 'finish',
        highlighted_body_ids: [], highlighted_sketch_entity_ids: [] });
      this.credits = 0; this.remainingMs = 0;
    }
    this.emit(patch);
    return this.status();
  }
  canApply(): boolean {
    this.advance();
    if (this.state.stopped) return false;
    if (this.state.paused) return this.credits > 0;
    return this.state.mode === 'fast' || this.remainingMs <= 0;
  }
  applied(label: string): void {
    // The completed/stopped walkthrough remains a record of that run, even
    // while the user saves the result or resumes ordinary modeling afterward.
    if (!this.state.active || this.state.finished || this.state.stopped) return;
    this.emit({ operation: label.replace(/^(sketch|solid|assembly)_/, '').replace(/_/g, ' ') });
  }
  modelApplied(): void {
    this.advance();
    if (this.state.paused && this.credits) this.credits -= 1;
    if (this.state.mode === 'present') this.remainingMs = Math.max(this.remainingMs, this.paceMs);
  }
  emphasize(bodyIds: number[] = [], sketchEntityIds: number[] = []): void {
    if (!this.state.active || this.state.mode === 'fast') return;
    this.emit({ highlighted_body_ids: bodyIds, highlighted_sketch_entity_ids: sketchEntityIds });
  }
  /** Camera motion is optional presentation, never an execution-rate tax. */
  motionDuration(ms: number): number {
    if (!this.state.active || this.state.finished || this.state.stopped) return ms;
    // Keep even an authored slow camera move within the interactive control
    // request lifetime. Caption reading holds can continue for longer.
    return this.state.mode === 'fast' ? 0 : Math.min(10000, ms / this.state.speed);
  }
}

export const presentation = new PresentationController();
const wakeups = new Set<() => void>();
export function wakePlayback(): void { for (const wake of [...wakeups]) wake(); }
export function waitForPlayback(ms: number): Promise<void> {
  return new Promise(resolve => {
    const deadline = Date.now() + ms;
    const finish = () => { clearTimeout(timer); wakeups.delete(wake); resolve(); };
    const wake = () => { if (Date.now() >= deadline) finish(); };
    const timer = setTimeout(finish, ms);
    wakeups.add(wake);
  });
}
export function setPlaybackPace(ms: number): void { presentation.configurePace(ms); }

/** Pointer feedback uses the same calm status line, without flashing geometry. */
export function installOperationFeedback(): () => void {
  const touch = (event: PointerEvent) => {
    if (!presentation.snapshot().active || !event.isTrusted || !(event.target instanceof Element)
      || event.target.closest('[data-interface-group="document/presentation"]')) return;
    const target = event.target.closest<HTMLElement>('button,[role="button"],[role="menuitem"]');
    if (target && !target.matches(':disabled,[aria-disabled="true"]')) {
      presentation.applied(target.getAttribute('aria-label') || target.title || target.textContent || 'Select');
    }
  };
  document.addEventListener('pointerdown', touch, true);
  return () => document.removeEventListener('pointerdown', touch, true);
}

export async function presentOperation(label: string, _target?: HTMLElement | null): Promise<void> {
  // One persistent card is updated in place. No viewport border flash, pulse,
  // brightness filter, opacity loop, or label racing across the model.
  presentation.applied(label);
}
