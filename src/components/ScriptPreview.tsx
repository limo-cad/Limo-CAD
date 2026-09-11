import { useEffect, useRef, useState } from 'react';
import { closeScriptPreview, errorMessage, openScriptPreview, renderScriptPreview, type ScriptPreviewFrame } from '../scripts/workspace';

const WIDTH = 300;
const HEIGHT = 176;
const FRAME_MS = 1800;
const HOME = { yaw: Math.PI / 4, pitch: Math.PI / 6 };
type Pose = typeof HOME;
type ImageResult = { url: string; previewId: string; frameIndex: number };
type RenderView = { id: string | null; ready: Promise<string>; closed: boolean; busy: boolean; revision: number;
  pending: { frame: ScriptPreviewFrame; pose: Pose } | null };

/** Bevy renders the immutable example; this component owns accessible controls
 * and one-shot sequencing, never model projection, shading or live CAD state. */
export function ScriptPreview({ frames, autoPlay = true }: { frames: ScriptPreviewFrame[]; autoPlay?: boolean }) {
  const [index, setIndex] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [pose, setPose] = useState(HOME);
  const [image, setImage] = useState<ImageResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const imageUrl = useRef<string | null>(null);
  const view = useRef<RenderView | null>(null);
  const drag = useRef<{ x: number; y: number; yaw: number; pitch: number } | null>(null);
  const reducedMotion = useRef(false);
  const current = Math.min(index, Math.max(0, frames.length - 1));
  const frame = frames[current];

  useEffect(() => {
    const owner: RenderView = { id: null, ready: openScriptPreview(), closed: false, busy: false, revision: 0, pending: null };
    void owner.ready.then(id => { owner.id = id; if (owner.closed) closeScriptPreview(id); }).catch(() => undefined);
    view.current = owner;
    setImage(null); setError(null);
    return () => {
      owner.closed = true; owner.pending = null;
      if (owner.id) closeScriptPreview(owner.id);
      if (imageUrl.current) URL.revokeObjectURL(imageUrl.current);
      imageUrl.current = null;
    };
  }, [frames]);

  useEffect(() => {
    const preference = window.matchMedia?.('(prefers-reduced-motion: reduce)');
    const update = () => { reducedMotion.current = preference?.matches ?? false; if (reducedMotion.current) setPlaying(false); };
    update();
    preference?.addEventListener('change', update);
    setIndex(0); setPose(HOME);
    setPlaying(autoPlay && frames.length > 1 && !reducedMotion.current);
    return () => preference?.removeEventListener('change', update);
  }, [frames, autoPlay]);

  useEffect(() => {
    if (!playing || image?.previewId !== frame?.previewId || image?.frameIndex !== frame?.frameIndex) return;
    const timer = window.setTimeout(() => {
      const next = Math.min(current + 1, Math.max(0, frames.length - 1));
      setIndex(next); setPlaying(next < frames.length - 1);
    }, FRAME_MS);
    return () => window.clearTimeout(timer);
  }, [playing, current, frames.length, image, frame]);

  useEffect(() => {
    const owner = view.current;
    if (!owner || !frame) return;
    owner.pending = { frame, pose };
    const pump = async () => {
      if (owner.closed || owner.busy || !owner.pending) return;
      const request = owner.pending;
      owner.pending = null; owner.busy = true;
      const revision = ++owner.revision;
      try {
        const viewId = await owner.ready;
        if (owner.closed) return;
        const ratio = Math.min(Math.max(window.devicePixelRatio || 1, 1), 2);
        const blob = await renderScriptPreview(request.frame, viewId, revision, request.pose,
          Math.round(WIDTH * ratio), Math.round(HEIGHT * ratio));
        // A turn/frame change may arrive while the GPU is working. Only the
        // latest request may publish pixels or errors into this mounted view.
        if (owner.closed || owner.pending) return;
        const url = URL.createObjectURL(blob);
        if (imageUrl.current) URL.revokeObjectURL(imageUrl.current);
        imageUrl.current = url;
        setImage({ url, previewId: request.frame.previewId, frameIndex: request.frame.frameIndex });
        setError(null);
      } catch (failure) {
        if (!owner.closed && !owner.pending) setError(errorMessage(failure));
      } finally {
        owner.busy = false;
        void pump();
      }
    };
    void pump();
  }, [frames, frame, pose]);

  const choose = (next: number) => { setPlaying(false); setIndex(Math.max(0, Math.min(frames.length - 1, next))); };
  const turn = (yaw: number, pitch: number) => { setPlaying(false); setPose({ yaw, pitch: Math.max(-1.3, Math.min(1.3, pitch)) }); };
  const fit = () => { setPlaying(false); setPose({ ...HOME }); };
  const visibleImage = image?.previewId === frame?.previewId && image?.frameIndex === frame?.frameIndex ? image : null;
  return (
    <div data-script-preview className="min-w-0 text-[11px] text-ink">
      <div role="img" tabIndex={0}
        aria-label={frame ? `Example model: ${frame.caption}` : 'No preview geometry'}
        aria-keyshortcuts="ArrowLeft ArrowRight ArrowUp ArrowDown Home"
        title="Drag or use arrow keys to turn this example. Home fits the model. The open design is unchanged."
        className="relative flex w-full touch-none items-center justify-center overflow-hidden rounded border border-edge bg-viewport cursor-grab focus-visible:outline focus-visible:outline-accent active:cursor-grabbing"
        style={{ aspectRatio: `${WIDTH} / ${HEIGHT}` }}
        onKeyDown={event => {
          if (event.key === 'Home') { event.preventDefault(); event.stopPropagation(); fit(); return; }
          const delta = { ArrowLeft: [-0.15, 0], ArrowRight: [0.15, 0], ArrowUp: [0, 0.15], ArrowDown: [0, -0.15] }[event.key];
          if (!delta) return;
          event.preventDefault(); event.stopPropagation();
          turn(pose.yaw + delta[0], pose.pitch + delta[1]);
        }}
        onPointerDown={event => {
          if (event.button !== 0) return;
          event.preventDefault(); event.currentTarget.focus(); setPlaying(false);
          drag.current = { x: event.clientX, y: event.clientY, ...pose };
          event.currentTarget.setPointerCapture(event.pointerId);
        }}
        onPointerMove={event => {
          const start = drag.current;
          if (start) turn(start.yaw + (event.clientX - start.x) * 0.012, start.pitch + (event.clientY - start.y) * 0.012);
        }}
        onPointerUp={() => { drag.current = null; }} onPointerCancel={() => { drag.current = null; }}
        onLostPointerCapture={() => { drag.current = null; }}
      >
        {visibleImage ? <img src={visibleImage.url} alt="" draggable={false} className="block h-full w-full" />
          : <span className="px-3 text-center text-mute">{error ?? 'Rendering native preview…'}</span>}
      </div>
      <p className="mt-2 min-h-8 leading-4" aria-live="polite">{error ?? frame?.caption ?? 'No preview is available.'}</p>
      <div className="mt-2 flex items-center gap-1 text-[10px]">
        <button type="button" className="rounded px-2 py-1 hover:bg-edge disabled:opacity-40" disabled={current === 0} onClick={() => choose(current - 1)} aria-label="Previous preview step">Previous</button>
        <span className="text-mute tabular-nums">{frames.length ? current + 1 : 0}/{frames.length}</span>
        <button type="button" className="rounded px-2 py-1 hover:bg-edge disabled:opacity-40" disabled={current >= frames.length - 1} onClick={() => choose(current + 1)} aria-label="Next preview step">Next</button>
        <button type="button" className="ml-auto rounded px-2 py-1 hover:bg-edge disabled:opacity-40" disabled={frames.length < 2} onClick={() => {
          setIndex(0); setPlaying(!reducedMotion.current);
        }} aria-label="Replay feature preview">Replay</button>
        <button type="button" className="rounded px-2 py-1 hover:bg-edge" onClick={fit} aria-label="Fit preview model">Fit</button>
      </div>
    </div>
  );
}
