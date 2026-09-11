// Real preview component/adapter; only native image IPC and the clock are faked.
import { createElement } from 'react';
import { createRoot } from 'react-dom/client';
import { ScriptPreview } from '../components/ScriptPreview';
import { useAppStore } from '../store/appStore';
import { presentation } from '../operationPlayback';
import { previewExample, renderScriptPreview, type ScriptPreviewFrame } from './workspace';

export async function checkNativeScriptPreview() {
  const check = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };
  const initial = useAppStore.getState();
  const playback = presentation.snapshot();
  const w = window as typeof window & { __TAURI_INTERNALS__?: { invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown> } };
  const native = w.__TAURI_INTERNALS__;
  const originalMedia = window.matchMedia;
  const originalTimeout = window.setTimeout;
  const originalClearTimeout = window.clearTimeout;
  const originalCreateUrl = URL.createObjectURL;
  const originalRevokeUrl = URL.revokeObjectURL;
  const created: string[] = [];
  const revoked: string[] = [];
  const closed: string[] = [];
  const timers = new Map<number, () => void>();
  let timerId = 1_000_000;
  let reduced = false;
  let nextView = 0;
  let automaticReplies = false;
  let deferOpen = false;
  let resolveOpen: ((view: string) => void) | null = null;
  let previewBuilds = 0;
  let expired = false;
  const renders: Array<{ request: { viewId: string; previewId: string; frameIndex: number; revision: number; yaw: number; pitch: number };
    resolve: (bytes: ArrayBuffer) => void; reject: (error: Error) => void }> = [];
  const pixel = Uint8Array.from(atob('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg=='), value => value.charCodeAt(0)).buffer;
  w.__TAURI_INTERNALS__ = { invoke(command, args) {
    if (command === 'native_script_preview_open') return deferOpen ? new Promise(resolve => { resolveOpen = resolve; }) : Promise.resolve(`view-${++nextView}`);
    if (command === 'native_script_preview_close') { closed.push(String(args?.viewId)); return Promise.resolve(); }
    if (command === 'native_script_preview') return Promise.resolve({ preview_id: `cached-${++previewBuilds}`, captions: ['Stock', 'Changed'] });
    if (command === 'native_script_preview_release') return Promise.resolve();
    if (command === 'native_script_preview_render') {
      const request = args!.request as typeof renders[number]['request'];
      if (expired && request.previewId === 'cached-1') return Promise.reject(new Error('Feature preview expired; reopen the example'));
      return new Promise((resolve, reject) => {
        renders.push({ request, resolve, reject });
        if (automaticReplies) resolve(pixel);
      });
    }
    throw new Error(`Preview unexpectedly used the live model/session: ${command}`);
  } };
  window.matchMedia = () => ({ matches: reduced, addEventListener() {}, removeEventListener() {} } as unknown as MediaQueryList);
  window.setTimeout = ((handler: TimerHandler, delay?: number, ...args: unknown[]) => {
    if (delay === 1800) { const id = ++timerId; timers.set(id, handler as () => void); return id; }
    return originalTimeout(handler, delay, ...args);
  }) as typeof window.setTimeout;
  window.clearTimeout = ((id?: number) => { if (id !== undefined && timers.delete(id)) return; originalClearTimeout(id); }) as typeof window.clearTimeout;
  URL.createObjectURL = blob => { const url = originalCreateUrl(blob); created.push(url); return url; };
  URL.revokeObjectURL = url => { revoked.push(url); originalRevokeUrl(url); };
  const container = document.createElement('div'); document.body.append(container);
  let root = createRoot(container);
  const settle = async () => { for (let i = 0; i < 3; i++) await new Promise<void>(resolve => requestAnimationFrame(() => resolve())); };
  const click = (label: string) => (container.querySelector(`[aria-label="${label}"]`) as HTMLButtonElement).click();
  const frames: ScriptPreviewFrame[] = ['Stock', 'Roundover', 'Final'].map((caption, frameIndex) => ({ caption, frameIndex, previewId: 'immutable' }));
  try {
    root.render(createElement(ScriptPreview, { frames, autoPlay: false }));
    await settle();
    check(renders.length === 1 && renders[0].request.frameIndex === 0, 'Mount requests one native frame');
    click('Next preview step'); await settle();
    check(renders.length === 1, 'Frame changes coalesce behind the in-flight native request');
    renders[0].resolve(pixel); await settle();
    check(created.length === 0 && renders.length === 2 && renders[1].request.frameIndex === 1,
      'Superseded pixels must not appear beneath the next frame caption');
    renders[1].resolve(pixel); await settle();
    check(!!container.querySelector('img') && created.length === 1, 'The current native image reaches the actual component');
    const surface = container.querySelector('[role="img"]') as HTMLElement;
    check(surface.tabIndex === 0, 'Inspection surface is keyboard focusable');
    surface.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true })); await settle();
    check(renders.length === 3 && renders[2].request.yaw > renders[1].request.yaw, 'Keyboard orbit reaches the native camera request');
    surface.dispatchEvent(new KeyboardEvent('keydown', { key: 'Home', bubbles: true })); await settle();
    renders[2].reject(new Error('Late orbit failure')); await settle();
    check(!container.textContent?.includes('Late orbit failure') && renders.length === 4, 'A superseded error cannot poison the fitted view');
    check(renders[3].request.yaw === renders[1].request.yaw, 'Home fits using the original orbit');
    renders[3].resolve(pixel); await settle();
    check(revoked.includes(created[0]), 'Replacing a displayed image releases its object URL');
    surface.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true })); await settle();
    const pending = renders[renders.length - 1];
    root.unmount();
    const count = created.length;
    pending.resolve(pixel); await settle();
    check(closed.includes(pending.request.viewId) && created.length === count && created.every(url => revoked.includes(url)),
      'Close cancels the native lease and rejects late pixels while freeing image URLs');

    // A native lease can arrive after Close too; release it without rendering.
    deferOpen = true; root = createRoot(container);
    root.render(createElement(ScriptPreview, { frames, autoPlay: false })); await settle();
    root.unmount(); const rendersBeforeLateOpen = renders.length;
    (resolveOpen as unknown as (id: string) => void)('late-view'); await settle();
    check(closed.includes('late-view') && renders.length === rendersBeforeLateOpen, 'Late native Open is closed without submitting geometry');
    deferOpen = false;

    const beforeAutoplay = renders.length;
    root = createRoot(container); root.render(createElement(ScriptPreview, { frames })); await settle();
    check(timers.size === 0 && renders.length === beforeAutoplay + 1 && renders[beforeAutoplay].request.frameIndex === 0,
      'A slow first native frame must remain at step zero without starting its hold or skipping geometry');
    automaticReplies = true;
    renders[beforeAutoplay].resolve(pixel); await settle();
    for (let step = 0; step < 2; step++) {
      check(timers.size === 1, 'One-shot playback owns one timer');
      const [id, tick] = [...timers][0]; timers.delete(id); tick(); await settle();
    }
    check(timers.size === 0 && container.textContent?.includes('3/3'), 'Autoplay stops on the final immutable frame');
    root.unmount(); reduced = true;
    root = createRoot(container); root.render(createElement(ScriptPreview, { frames })); await settle();
    check(timers.size === 0, 'Reduced motion prevents autoplay');
    click('Next preview step'); await settle();
    check(container.textContent?.includes('2/3'), 'Reduced motion retains manual inspection');

    root.unmount(); automaticReplies = false;
    root = createRoot(container); root.render(createElement(ScriptPreview, { frames, autoPlay: false })); await settle();
    const failed = renders[renders.length - 1];
    failed.reject(new Error('Transient GPU failure')); await settle();
    const beforeFit = renders.length;
    click('Fit preview model'); await settle();
    check(renders.length === beforeFit + 1, 'Fit retries a transient render failure even at the initial home pose');
    renders[beforeFit].resolve(pixel); await settle();
    check(!container.textContent?.includes('Transient GPU failure') && !!container.querySelector('img'), 'Retry replaces the error with native pixels');
    automaticReplies = true;

    const example = { id: 'eviction-test', name: 'Eviction', summary: '', group: 'document', operation: 'cad_document',
      kind: 'lesson' as const, focus_operations: [], operations: [], preview: true, source: 'immutable source' };
    const cached = await previewExample(example);
    expired = true;
    await renderScriptPreview(cached[0], 'cache-view', 1, { yaw: 0, pitch: 0 }, 300, 176);
    await renderScriptPreview(cached[1], 'cache-view', 2, { yaw: 0, pitch: 0 }, 300, 176);
    check(previewBuilds === 2 && renders[renders.length - 1].request.previewId === 'cached-2',
      'Native byte eviction refreshes the immutable source once and subsequent frames reuse the new handle');
    check(useAppStore.getState() === initial && presentation.snapshot() === playback,
      'Preview controls must not mutate document, selection, session or live playback');
    return { checks: ['native-images', 'coalesced-requests', 'stale-frame', 'stale-error', 'close-during-render',
      'close-during-open', 'keyboard-orbit-fit', 'first-frame-hold', 'one-shot', 'reduced-motion', 'fit-retry', 'eviction-recovery', 'live-state-isolation'] };
  } finally {
    root.unmount(); container.remove();
    if (native) w.__TAURI_INTERNALS__ = native; else delete w.__TAURI_INTERNALS__;
    window.matchMedia = originalMedia; window.setTimeout = originalTimeout; window.clearTimeout = originalClearTimeout;
    URL.createObjectURL = originalCreateUrl; URL.revokeObjectURL = originalRevokeUrl;
  }
}
