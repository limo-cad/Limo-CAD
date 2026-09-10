import { useSyncExternalStore } from 'react';
import { presentation } from '../operationPlayback';

/** The visible controls and MCP operate the same presentation controller. */
export function PresentationControls() {
  const state = useSyncExternalStore(presentation.subscribe, presentation.snapshot);
  if (!state.active) return null;
  const complete = state.finished || state.stopped;
  const buttonClass = 'rounded border border-edge bg-panel px-2 py-1 text-xs disabled:opacity-40';
  return (
    <section aria-label="Presentation" data-interface-group="document/presentation" data-native-viewport-overlay
      className="absolute bottom-4 left-4 z-30 rounded-lg border border-edge bg-panel/95 p-3 text-ink shadow-lg"
      style={{ width: 'min(440px, calc(100% - 32px))' }}>
      <div className="mb-1 flex items-baseline justify-between gap-3 text-xs">
        <strong>{state.chapter || 'Model walkthrough'}</strong>
        <span>{state.stopped ? 'Stopped' : state.finished ? 'Complete' : state.paused ? 'Paused' : state.mode === 'fast' ? 'Maximum speed' : `${state.speed}×`}</span>
      </div>
      {state.text && <p className="mb-2 whitespace-pre-line text-sm leading-relaxed" role="status" aria-live="polite">{state.text}</p>}
      <div className="flex items-baseline justify-between gap-3 text-xs text-mute">
        <span className="truncate">{state.operation || 'Ready'}</span>
        {state.step_count > 0 && <span>{state.step_index} / {state.step_count}</span>}
      </div>
      {state.step_count > 0 && <progress aria-label="Presentation progress" className="mt-2 h-1 w-full accent-emerald-500" value={state.step_index} max={state.step_count} />}
      <div className="mt-2 flex flex-wrap items-center gap-2">
        <button className={buttonClass} disabled={complete} onClick={() => presentation.control({ command: state.paused ? 'resume' : 'pause' })}>{state.paused ? 'Resume' : 'Pause'}</button>
        <button className={buttonClass} disabled={complete} title="Apply one modeling operation, then remain paused" onClick={() => presentation.control({ command: 'step' })}>Step</button>
        <label className="flex items-center gap-1 text-xs">Speed
          <select aria-label="Presentation speed" disabled={complete} className="rounded border border-edge bg-panel px-1 py-1" value={state.mode === 'fast' ? 'fast' : String(state.speed)}
            onChange={event => presentation.control({ command: 'configure', mode: event.target.value === 'fast' ? 'fast' : 'present', ...(event.target.value === 'fast' ? {} : { speed: Number(event.target.value) }) })}>
            {[0.25, 0.5, 1, 2, 4, 8, 16].map(speed => <option key={speed} value={speed}>{speed}×</option>)}
            <option value="fast">Maximum</option>
          </select>
        </label>
        <button className={buttonClass} disabled={complete} onClick={() => presentation.control({ command: 'stop' })}>Stop</button>
      </div>
    </section>
  );
}
