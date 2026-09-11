import { useEffect, useState, useSyncExternalStore } from 'react';
import { BookOpen, FileCode2, FolderOpen, Play, X } from 'lucide-react';
import {
  closeScripts, editScriptSource, errorMessage, loadScriptPath, openScriptFile, previewExample, runLoadedScript, saveScriptSource,
  showScriptExample, stopScript, useScriptWorkspace, validateScriptSource, ownsScriptPlayback,
  type ScriptPreviewFrame,
} from '../scripts/workspace';
import { useAppStore } from '../store/appStore';
import { ScriptPreview } from './ScriptPreview';
import { presentation } from '../operationPlayback';
import { useSurfaceDismiss } from './useSurfaceDismiss';

const button = 'rounded border border-edge px-2 py-1.5 text-xs hover:bg-edge disabled:opacity-40';

/** A docked document companion: source and lessons never cover the model. */
export function ScriptPanel() {
  const state = useScriptWorkspace();
  const playback = useSyncExternalStore(presentation.subscribe, presentation.snapshot, presentation.snapshot);
  const { surface, dismiss, onKeyDown } = useSurfaceDismiss(state.open, closeScripts, 'button[aria-label="Scripts"]');
  const editing = useAppStore(s => !!s.activeSketch || !!s.historyEdit || s.projectBusy || s.solidBusy);
  const [frames, setFrames] = useState<ScriptPreviewFrame[] | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  useEffect(() => {
    let current = true;
    setFrames(null); setPreviewError(null);
    if (state.open && state.selectedExample?.preview) {
      void previewExample(state.selectedExample).then(result => { if (current) setFrames(result); })
        .catch(error => { if (current) setPreviewError(errorMessage(error)); });
    }
    return () => { current = false; };
  }, [state.open, state.selectedExample]);
  if (!state.open) return null;
  const busy = state.loading || state.running;
  const live = state.running && ownsScriptPlayback() && playback.active;
  const mode = live ? playback.mode : state.mode;
  const speed = live ? playback.speed : state.speed;
  const speeds = [...new Set([0.25, 0.5, 1, 2, 4, 8, 16, speed])].sort((a, b) => a - b);
  const shortError = state.error?.split(': {')[0].slice(0, 300);
  return (
    <aside ref={surface} onKeyDown={onKeyDown} aria-label="Scripts" data-script-companion data-interface-group="document/scripts"
      className="flex w-[380px] min-w-0 shrink-0 flex-col border-l border-edge bg-panel text-ink">
      <header className="flex shrink-0 items-center justify-between border-b border-edge px-4 py-3">
        <div><h2 className="flex items-center gap-2 text-sm font-semibold"><BookOpen size={16} /> Scripts</h2>
          <p className="mt-1 text-xs text-mute">Learn a feature or replay a complete design.</p></div>
        <button className={button} aria-label="Close scripts" onClick={dismiss}><X size={14} /></button>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
        <button className={`${button} flex w-full items-center justify-center gap-2`} disabled={busy}
          onClick={() => void openScriptFile()}><FolderOpen size={14} /> Open script…</button>
        <details className="mt-2 text-xs">
          <summary className="cursor-pointer text-mute">Load from a file path</summary>
          <div className="mt-2 flex gap-2"><input aria-label="Script path" value={state.path} disabled={busy}
            onChange={event => useScriptWorkspace.setState({ path: event.target.value })}
            onKeyDown={event => { if (event.key === 'Enter') void loadScriptPath(); }}
            className="min-w-0 flex-1 rounded border border-edge bg-header px-2 py-1" />
            <button className={button} disabled={busy || !state.path.trim()} onClick={() => void loadScriptPath()}>Load script</button></div>
        </details>
        <h3 className="mb-2 mt-5 text-xs font-semibold text-mute">Examples</h3>
        <div className="space-y-2">{state.examples.map(example => (
          <button key={example.id} aria-label={example.name} className={`w-full rounded border p-3 text-left ${state.selectedExample?.id === example.id ? 'border-accent bg-accent/10' : 'border-edge hover:bg-header'}`}
            disabled={busy} onClick={() => void showScriptExample(example)}>
            <span className="block text-xs font-semibold">{example.name}</span>
            <span className="mt-1 block text-xs leading-relaxed text-mute">{example.summary}</span>
            <span className="mt-2 block text-[10px] text-mute">{example.preview ? 'Short feature lesson' : 'Complete design walkthrough'}</span>
          </button>
        ))}</div>
        {state.loading && <p role="status" className="mt-3 text-xs text-mute">Loading script…</p>}
        {state.info && <section className="mt-5 border-t border-edge pt-4">
          <h3 className="text-sm font-semibold">{state.info.name}</h3>
          <p className="mt-1 text-xs text-mute">{state.info.step_count} steps · {state.info.check_count} final checks</p>
          <div className="my-3 flex gap-2" role="tablist" aria-label="Script details">
            <button role="tab" aria-selected={state.tab === 'overview'} className={button}
              onClick={() => useScriptWorkspace.setState({ tab: 'overview' })}>Overview</button>
            <button role="tab" aria-selected={state.tab === 'source'} className={`${button} flex items-center gap-1`}
              onClick={() => useScriptWorkspace.setState({ tab: 'source' })}><FileCode2 size={12} /> Source</button>
          </div>
          {state.tab === 'source' ? <>
            <textarea aria-label="Script source" spellCheck={false} value={state.source} readOnly={busy}
              onChange={event => editScriptSource(event.target.value)}
              className="h-80 w-full resize-y rounded border border-edge bg-header p-2 font-mono text-[11px] leading-relaxed" />
            <button className={`${button} mt-2`} disabled={busy} onClick={() => void validateScriptSource()}>Validate changes</button>
            <button className={`${button} ml-2 mt-2`} disabled={busy} onClick={() => void saveScriptSource()}>Save script as…</button>
          </> : <>
            {frames && <ScriptPreview frames={frames} autoPlay={false} />}
            {state.selectedExample?.preview && !frames && !previewError && <p className="text-xs text-mute">Preparing isolated preview…</p>}
            {previewError && <p className="text-xs text-mute">Preview unavailable: {previewError.slice(0, 180)}</p>}
            <ol className="mt-3 space-y-3">{state.info.chapters?.map((chapter, index) => <li key={index}>
              {chapter.chapter && <h4 className="text-xs font-semibold">{chapter.chapter}</h4>}
              <p className="mt-1 text-xs leading-relaxed text-mute">{chapter.text}</p>
            </li>)}</ol>
          </>}
        </section>}
        {state.error && <div role="alert" className="mt-4 rounded border border-red-400/30 p-3 text-xs">
          <p>{shortError}</p>{state.error !== shortError && <details className="mt-2"><summary>Details</summary><pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap break-words">{state.error}</pre></details>}
        </div>}
        {state.completed && <p role="status" className="mt-4 text-xs text-emerald-500">Complete. The editable result is in its own design tab.</p>}
      </div>
      {state.info && <footer className="shrink-0 space-y-3 border-t border-edge p-4">
        <div className="flex items-center gap-2 text-xs">
          <label className="flex flex-1 items-center gap-2">Mode<select aria-label="Script run mode" className="min-w-0 flex-1 rounded border border-edge bg-header p-1" value={mode} disabled={busy}
            onChange={event => useScriptWorkspace.setState({ mode: event.target.value as 'present' | 'fast' })}>
            <option value="present">Presentation</option><option value="fast">Maximum speed</option></select></label>
          {mode === 'present' && <label className="flex items-center gap-1">Speed<select aria-label="Script speed" className="rounded border border-edge bg-header p-1" value={speed} disabled={busy}
            onChange={event => useScriptWorkspace.setState({ speed: Number(event.target.value) })}>
            {speeds.map(speed => <option key={speed} value={speed}>{speed}×</option>)}</select></label>}
        </div>
        <button className="flex w-full items-center justify-center gap-2 rounded bg-accent px-3 py-2 text-sm text-white disabled:opacity-40"
          disabled={!state.running && (state.loading || editing)} onClick={() => state.running ? stopScript() : void runLoadedScript()}>
          <Play size={14} /> {state.running ? 'Stop script' : 'Run in new design'}</button>
        <p className="text-[11px] text-mute">{editing && !state.running ? 'Finish the current edit to run this script.' : 'Your current design stays in its own tab. Loading a script does not run it.'}</p>
      </footer>}
    </aside>
  );
}
