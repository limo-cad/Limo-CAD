import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import { isTauriRuntime } from '../engine';
import { trackEngineOperation } from '../engine/activity';
import type { SolidSceneDto } from '../engine/types';
import { chooseOpenFile, chooseSaveTarget, writeSaveTarget } from '../files/fileIO';
import { newProject } from '../files/projectFiles';
import { presentation } from '../operationPlayback';
import { publishNow } from '../sessionBridge';
import { useAppStore } from '../store/appStore';

export interface ScriptExample {
  id: string;
  name: string;
  summary: string;
  group: string;
  operation: string;
  operations: string[];
  preview: boolean;
  source: string;
}
export interface ScriptInfo {
  name: string;
  step_count: number;
  check_count: number;
  source: string;
  path?: string;
  chapters?: Array<{ chapter?: string; text: string }>;
}
export interface ScriptPreviewFrame { caption: string; scene: SolidSceneDto }
interface ScriptWorkspace {
  open: boolean;
  loading: boolean;
  running: boolean;
  examples: ScriptExample[];
  selectedExample: ScriptExample | null;
  info: ScriptInfo | null;
  source: string;
  path: string;
  error: string | null;
  completed: boolean;
  tab: 'overview' | 'source';
  mode: 'present' | 'fast';
  speed: number;
}
export const useScriptWorkspace = create<ScriptWorkspace>(() => ({
  open: false, loading: false, running: false, examples: [], selectedExample: null,
  info: null, source: '', path: '', error: null, completed: false,
  tab: 'overview', mode: 'present', speed: 2,
}));

let examplesRequest: Promise<ScriptExample[]> | null = null;
const previews = new Map<string, Promise<ScriptPreviewFrame[]>>();
let currentRun: { cancelled: boolean; nativeStarted: boolean } | null = null;
/** Launch preferences remain separate from controls for this document's run. */
export function ownsScriptPlayback(): boolean {
  return !!currentRun?.nativeStarted;
}
export function errorMessage(error: unknown): string {
  if (error && typeof error === 'object' && 'message' in error) return String(error.message);
  return String(error);
}
export async function ensureScriptExamples(): Promise<ScriptExample[]> {
  if (!isTauriRuntime()) return [];
  if (!examplesRequest) {
    examplesRequest = invoke<ScriptExample[]>('native_script_examples').then(examples => {
      useScriptWorkspace.setState({ examples });
      return examples;
    }).catch(error => { examplesRequest = null; throw error; });
  }
  return examplesRequest;
}
export function showScripts(): void {
  useScriptWorkspace.setState({ open: true });
  void ensureScriptExamples().catch(error => useScriptWorkspace.setState({ error: errorMessage(error) }));
}
export function closeScripts(): void { useScriptWorkspace.setState({ open: false }); }
export function editScriptSource(source: string): void {
  const state = useScriptWorkspace.getState();
  if (state.loading || state.running) return;
  useScriptWorkspace.setState({ source, completed: false, selectedExample: null });
}

function requireDesktop(): void {
  if (!isTauriRuntime()) throw new Error('Open scripts in the desktop application to run the native CAD engine.');
}
async function inspect(source: string): Promise<ScriptInfo> {
  requireDesktop();
  return invoke<ScriptInfo>('native_script_inspect', { source });
}
function acceptScript(info: ScriptInfo, example: ScriptExample | null = null, path = ''): void {
  useScriptWorkspace.setState({ info, source: info.source, selectedExample: example,
    // Loading opened the panel already. A later Close must survive this reply.
    path: info.path ?? path, error: null, completed: false, tab: 'overview' });
}
async function load(action: () => Promise<void>): Promise<void> {
  showScripts();
  const state = useScriptWorkspace.getState();
  if (state.loading || state.running) return;
  useScriptWorkspace.setState({ loading: true, error: null, open: true });
  try { await action(); }
  catch (error) { useScriptWorkspace.setState({ error: errorMessage(error) }); }
  finally { useScriptWorkspace.setState({ loading: false }); }
}
export async function showScriptExample(example: ScriptExample): Promise<void> {
  await load(async () => acceptScript(await inspect(example.source), example));
}
export async function openScriptFile(): Promise<void> {
  await load(async () => {
    requireDesktop();
    const file = await chooseOpenFile({ description: 'noBS CAD command script', extension: '.jsonc',
      alternateExtensions: ['.json'], mime: 'application/json' });
    if (!file) return;
    if (file.bytes.length > 2 * 1024 * 1024) throw new Error('Script files must be smaller than 2 MB.');
    const source = new TextDecoder('utf-8', { fatal: true }).decode(file.bytes);
    const path = file.writableTarget?.kind === 'native' ? file.writableTarget.path : file.name;
    acceptScript(await inspect(source), null, path);
  });
}
export async function loadScriptPath(): Promise<void> {
  await load(async () => {
    requireDesktop();
    const path = useScriptWorkspace.getState().path.trim();
    if (!path) throw new Error('Choose a script file or enter its path.');
    acceptScript(await invoke<ScriptInfo>('native_script_inspect', { path }), null, path);
  });
}
export async function validateScriptSource(): Promise<void> {
  await load(async () => {
    const state = useScriptWorkspace.getState();
    const info = await inspect(state.source);
    useScriptWorkspace.setState({ info, completed: false });
  });
}

export async function saveScriptSource(): Promise<void> {
  await load(async () => {
    const state = useScriptWorkspace.getState();
    const info = await inspect(state.source);
    const name = state.path.split(/[\\/]/).pop() || `${info.name.replace(/[<>:"/\\|?*]/g, '-')}.nbcad.jsonc`;
    const target = await chooseSaveTarget(name, {
      description: 'noBS CAD command script', extension: '.jsonc', mime: 'application/json',
    });
    if (!target) return;
    await writeSaveTarget(target, new TextEncoder().encode(state.source));
    useScriptWorkspace.setState({ info, path: target.kind === 'native' ? target.path : target.name });
  });
}

/** File loading never runs commands. Run explicitly creates a retained new tab. */
export async function runLoadedScript(): Promise<void> {
  const state = useScriptWorkspace.getState();
  if (state.loading || state.running) return;
  const app = useAppStore.getState();
  if (app.activeSketch || app.historyEdit || app.projectBusy || app.solidBusy) {
    useScriptWorkspace.setState({ error: 'Finish the current editing operation before starting a script.' });
    return;
  }
  useScriptWorkspace.setState({ running: true, completed: false, error: null });
  const attempt = { cancelled: false, nativeStarted: false };
  currentRun = attempt;
  const checkCancelled = () => {
    if (attempt.cancelled) throw new Error('Script stopped.');
  };
  // Stop can arrive while the native worker is starting. Preserve that intent
  // when its initial configure message arrives, rather than clearing the stop.
  const unsubscribe = presentation.subscribe(() => {
    const playback = presentation.snapshot();
    if (attempt.cancelled && attempt.nativeStarted && !playback.stopped && !playback.finished) {
      presentation.control({ command: 'stop' });
    }
  });
  try {
    // The shared UI receipt waits for document handoff, then acknowledges the
    // new session. Playback itself stays asynchronous so controls remain usable.
    await trackEngineOperation((async () => {
      // Validate before creating a document or changing the existing one.
      const info = await inspect(state.source);
      checkCancelled();
      useScriptWorkspace.setState({ info });
      await Promise.allSettled([...previews.values()]);
      checkCancelled();
      if (!await newProject()) throw new Error('A new design could not be created.');
      checkCancelled();
      if (!await publishNow()) throw new Error('The new design is not ready. Please try Run again.');
      checkCancelled();
    })());
    attempt.nativeStarted = true;
    await invoke('native_script_run', { source: state.source, mode: state.mode, speed: state.speed });
    checkCancelled();
    useScriptWorkspace.setState({ completed: true });
  } catch (error) {
    useScriptWorkspace.setState({ error: errorMessage(error) });
  } finally {
    unsubscribe();
    currentRun = null;
    useScriptWorkspace.setState({ running: false });
  }
}
export function stopScript(): void {
  if (currentRun) currentRun.cancelled = true;
  if (currentRun?.nativeStarted) presentation.control({ command: 'stop' });
}

/** Cached immutable geometry, computed in an unattached native engine. */
export function previewExample(example: ScriptExample): Promise<ScriptPreviewFrame[]> {
  if (!example.preview) return Promise.reject(new Error('This example has no short preview.'));
  requireDesktop();
  const key = `${example.id}\n${example.source}`;
  if (!previews.has(key)) {
    if (useScriptWorkspace.getState().running) return Promise.reject(new Error('A script is running. Preview it after playback finishes.'));
    // Bound retained previews; failures can be retried after an active run ends.
    if (previews.size >= 8) previews.delete(previews.keys().next().value!);
    previews.set(key, invoke<{ exports: { preview_frames: ScriptPreviewFrame[] } }>(
      'native_script_preview', { source: example.source },
    ).then(report => {
      const frames = report.exports.preview_frames;
      if (!Array.isArray(frames) || !frames.length) throw new Error('The script did not provide preview frames.');
      return frames;
    }).catch(error => { previews.delete(key); throw error; }));
  }
  return previews.get(key)!;
}
