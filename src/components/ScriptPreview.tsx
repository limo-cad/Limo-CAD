import { useEffect, useMemo, useRef, useState } from 'react';
import type { ScriptPreviewFrame } from '../scripts/workspace';
import { fitPreview, PREVIEW_HOME, previewStep, previewTriangles } from '../scripts/previewGeometry';

const WIDTH = 300;
const HEIGHT = 176;
const FRAME_MS = 1800;

/** Read-only snapshots from an isolated Rust script run, never the live scene. */
export function ScriptPreview({ frames, autoPlay = true }: { frames: ScriptPreviewFrame[]; autoPlay?: boolean }) {
  const [index, setIndex] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [pose, setPose] = useState(PREVIEW_HOME);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const drag = useRef<{ x: number; y: number; yaw: number; pitch: number } | null>(null);
  const reducedMotion = useRef(false);
  const current = Math.min(index, Math.max(0, frames.length - 1));
  const frame = frames[current];
  const geometry = useMemo(() => {
    try {
      const fit = fitPreview(frames.map(item => item.scene), pose, WIDTH, HEIGHT);
      return { triangles: frame ? previewTriangles(frame.scene, pose, fit, WIDTH, HEIGHT) : [], error: null };
    } catch (error) {
      return { triangles: [], error: error instanceof Error ? error.message : String(error) };
    }
  }, [frames, frame, pose]);

  useEffect(() => {
    const preference = window.matchMedia?.('(prefers-reduced-motion: reduce)');
    const update = () => { reducedMotion.current = preference?.matches ?? false; if (reducedMotion.current) setPlaying(false); };
    update();
    preference?.addEventListener('change', update);
    setIndex(0);
    setPlaying(autoPlay && frames.length > 1 && !reducedMotion.current);
    return () => preference?.removeEventListener('change', update);
  }, [frames, autoPlay]);

  useEffect(() => {
    if (!playing) return;
    const timer = window.setTimeout(() => {
      const next = previewStep(current, frames.length);
      setIndex(next.index);
      setPlaying(next.playing);
    }, FRAME_MS);
    return () => window.clearTimeout(timer);
  }, [playing, current, frames.length]);

  useEffect(() => {
    const canvas = canvasRef.current;
    const context = canvas?.getContext('2d');
    if (!canvas || !context) return;
    const ratio = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = WIDTH * ratio;
    canvas.height = HEIGHT * ratio;
    context.scale(ratio, ratio);
    context.clearRect(0, 0, WIDTH, HEIGHT);
    for (const triangle of geometry.triangles) {
      context.beginPath();
      triangle.points.forEach((point, vertex) => vertex === 0 ? context.moveTo(point[0], point[1]) : context.lineTo(point[0], point[1]));
      context.closePath();
      context.fillStyle = triangle.color;
      context.strokeStyle = triangle.color;
      context.lineWidth = 0.35;
      context.fill();
      context.stroke();
    }
  }, [geometry]);

  const choose = (next: number) => { setPlaying(false); setIndex(Math.max(0, Math.min(frames.length - 1, next))); };
  return (
    <div data-script-preview className="min-w-0 text-[11px] text-ink">
      <canvas
        ref={canvasRef}
        role="img"
        aria-label={frame ? `Example model: ${frame.caption}` : 'No preview geometry'}
        title="Drag to turn this example. The open model is unchanged."
        width={WIDTH}
        height={HEIGHT}
        className="block w-full touch-none rounded border border-edge bg-viewport cursor-grab active:cursor-grabbing"
        style={{ aspectRatio: `${WIDTH} / ${HEIGHT}` }}
        onPointerDown={event => {
          if (event.button !== 0) return;
          setPlaying(false);
          drag.current = { x: event.clientX, y: event.clientY, ...pose };
          event.currentTarget.setPointerCapture(event.pointerId);
        }}
        onPointerMove={event => {
          const start = drag.current;
          if (!start) return;
          setPose({ yaw: start.yaw + (event.clientX - start.x) * 0.012, pitch: Math.max(-1.3, Math.min(1.3, start.pitch + (event.clientY - start.y) * 0.012)) });
        }}
        onPointerUp={() => { drag.current = null; }}
        onPointerCancel={() => { drag.current = null; }}
        onLostPointerCapture={() => { drag.current = null; }}
      />
      <p className="mt-2 min-h-8 leading-4" aria-live="polite">{geometry.error ?? frame?.caption ?? 'No preview is available.'}</p>
      <div className="mt-2 flex items-center gap-1 text-[10px]">
        <button type="button" className="rounded px-2 py-1 hover:bg-edge disabled:opacity-40" disabled={current === 0} onClick={() => choose(current - 1)} aria-label="Previous preview step">Previous</button>
        <span className="text-mute tabular-nums">{frames.length ? current + 1 : 0}/{frames.length}</span>
        <button type="button" className="rounded px-2 py-1 hover:bg-edge disabled:opacity-40" disabled={current >= frames.length - 1} onClick={() => choose(current + 1)} aria-label="Next preview step">Next</button>
        <button type="button" className="ml-auto rounded px-2 py-1 hover:bg-edge disabled:opacity-40" disabled={frames.length < 2} onClick={() => {
          setIndex(0);
          setPlaying(!reducedMotion.current);
        }} aria-label="Replay feature preview">Replay</button>
        <button type="button" className="rounded px-2 py-1 hover:bg-edge" onClick={() => setPose(PREVIEW_HOME)} aria-label="Fit preview model">Fit</button>
      </div>
    </div>
  );
}
