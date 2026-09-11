/**
 * Window-level project tabs.
 *
 * Each open tab keeps a retained modeling context so normal tab switches do
 * not replay a feature tree through OCCT. The serialized model remains the
 * recovery/eviction boundary: long-idle tabs and tabs released under system
 * memory pressure are reconstructed from it only when selected again.
 */
import { getEngine, isTauriRuntime } from '../engine';
import type {
  AssemblyDocumentDto,
  AssemblySolutionDto,
  BodyAppearance,
  DatumPlaneDefinitionDto,
  DrawingDocumentDto,
  ProjectVisibilityDto,
  SketchDto,
  SolidUpdateDto,
} from '../engine/types';
import { translate } from '../i18n';
import { dropApplicationHistory } from '../engine/applicationHistory';
import { enterDrawingWorkspace } from '../drawing/document';
import {
  exportProjectModelWithVisibility,
  useAppStore,
  type ProjectTabSummary,
} from '../store/appStore';
import type { SaveTarget } from './fileIO';
import { projectTransitions } from './projectTransitions';
import { captureProjectOwner } from './projectOwnership';

interface ProjectTabRuntime {
  modelJson: string;
  /** Native paths/file handles never enter the inspectable Zustand store. */
  saveTarget: SaveTarget | null;
  /** False after the native/WASM OCCT context has been evicted. */
  resident: boolean;
  /** Last time this tab stopped being active; used for conservative LRU. */
  lastUsedAt: number;
  /** Workspace stage (solid/drawing) the project was last viewed in. */
  workspaceTab: string;
  /** Frontend mirror retained by reference to avoid large mesh JSON on switch. */
  viewState: ProjectTabViewState | null;
}

interface ProjectTabViewState {
  update: SolidUpdateDto;
  finishedSketches: SketchDto[];
  datumPlanes: DatumPlaneDefinitionDto[];
  bodyAppearances: BodyAppearance[];
  drawingDocument: DrawingDocumentDto;
  assemblyDocument: AssemblyDocumentDto;
  assemblySolution: AssemblySolutionDto;
  projectVisibility: ProjectVisibilityDto;
}

export interface RecoverableProjectTab {
  id: string;
  name: string;
  fileName: string | null;
  modelJson: string;
}

const runtimes = new Map<string, ProjectTabRuntime>();
let currentProjectTarget: SaveTarget | null = null;
let nextTabId = 1;
const LONG_IDLE_EVICTION_MS = 60 * 60 * 1_000;
const RETENTION_CHECK_MS = 30_000;

interface SystemMemoryStatus {
  totalBytes: number;
  availableBytes: number;
  pressure: 'normal' | 'constrained' | 'critical';
}

function emptyDrawingDocument(): DrawingDocumentDto {
  return {
    sheets: [],
    active_sheet_id: null,
    next_sheet_id: 1,
    next_view_id: 1,
    next_annotation_id: 1,
    next_revision_id: 1,
    next_bom_item_id: 1,
    templates: [],
    next_template_id: 1,
  };
}

function emptyAssemblyDocument(): AssemblyDocumentDto {
  return {
    joints: [],
    next_joint_id: 1,
    grounded_body_id: null,
    component_structure: {
      definitions: [],
      occurrences: [],
      next_component_id: 1,
      next_occurrence_id: 1,
    },
    positions: [],
    next_position_id: 1,
    motion_studies: [],
    next_motion_study_id: 1,
    contact_sets: [],
    next_contact_set_id: 1,
  };
}

function emptyAssemblySolution(): AssemblySolutionDto {
  return {
    body_poses: [],
    occurrence_poses: [],
    instance_body_poses: [],
    diagnostics: [],
    solved: true,
  };
}

function createTabId(): string {
  const randomId = globalThis.crypto?.randomUUID?.();
  const id = randomId ?? `project-${Date.now()}-${nextTabId}`;
  nextTabId += 1;
  return id;
}

/** Workspace stage of the active document; sketch is a mode inside modeling,
 * never a restorable workspace. */
function activeWorkspaceTab(): string {
  const tab = useAppStore.getState().activeTab;
  return tab === 'sketch' ? 'solid' : tab;
}

function summaryFromActiveState(id: string): ProjectTabSummary {
  const state = useAppStore.getState();
  return {
    id,
    name: state.document?.name ?? translate('app.untitledDocument'),
    fileName: state.projectFileName,
    dirty: state.dirty,
    workspaceTab: activeWorkspaceTab(),
  };
}

function activeViewState(): ProjectTabViewState | null {
  const state = useAppStore.getState();
  if (!state.document) return null;
  return {
    update: { document: state.document, scene: state.solidScene },
    finishedSketches: state.finishedSketches,
    datumPlanes: state.datumPlanes,
    bodyAppearances: state.bodyAppearances,
    drawingDocument: state.drawingDocument,
    assemblyDocument: state.assemblyDocument,
    assemblySolution: state.assemblySolution,
    projectVisibility: state.projectVisibility,
  };
}

function sameViewState(
  left: ProjectTabViewState | null,
  right: ProjectTabViewState | null,
): boolean {
  return (
    left !== null &&
    right !== null &&
    left.update.document === right.update.document &&
    left.update.scene === right.update.scene &&
    left.finishedSketches === right.finishedSketches &&
    left.datumPlanes === right.datumPlanes &&
    left.bodyAppearances === right.bodyAppearances &&
    left.drawingDocument === right.drawingDocument &&
    left.assemblyDocument === right.assemblyDocument &&
    left.assemblySolution === right.assemblySolution &&
    left.projectVisibility === right.projectVisibility
  );
}

function syncActiveSummary(): void {
  const state = useAppStore.getState();
  const id = state.activeProjectTabId;
  if (!id) return;
  const summary = summaryFromActiveState(id);
  useAppStore.setState({
    projectTabs: state.projectTabs.map((tab) =>
      tab.id === id ? summary : tab,
    ),
  });
}

async function ensureActiveProjectTab(
  knownModelJson?: string,
): Promise<string> {
  const state = useAppStore.getState();
  if (state.activeProjectTabId) return state.activeProjectTabId;

  const id = createTabId();
  const modelJson =
    knownModelJson ?? (await exportProjectModelWithVisibility());
  runtimes.set(id, {
    modelJson,
    saveTarget: currentProjectTarget,
    resident: false,
    lastUsedAt: Date.now(),
    workspaceTab: activeWorkspaceTab(),
    viewState: activeViewState(),
  });
  useAppStore.setState({
    activeProjectTabId: id,
    projectTabs: [summaryFromActiveState(id)],
  });
  return id;
}

async function snapshotActiveProjectTab(): Promise<string> {
  // The tab wrapper itself owns solidBusy, before replacing the active model.
  const owner = captureProjectOwner(true);
  const { state } = owner;
  await owner.assertCurrent();
  if (state.historyEdit) {
    throw new Error(translate('file.finishBeforeFeatureEdit'));
  }
  if (state.activeSketch) {
    throw new Error(translate('file.finishBeforeTabSwitch'));
  }
  const existingRuntime = state.activeProjectTabId
    ? runtimes.get(state.activeProjectTabId)
    : undefined;
  const viewState = activeViewState();
  const modelJson = sameViewState(existingRuntime?.viewState ?? null, viewState)
    ? existingRuntime!.modelJson
    : await exportProjectModelWithVisibility(undefined, owner.assertCurrent);
  await owner.assertCurrent();
  const id = await ensureActiveProjectTab(modelJson);
  // Registration may assign the first tab ID; every existing tab still has
  // to be the owner captured above before its recovery snapshot is replaced.
  if (state.activeProjectTabId) owner.assertSettled();
  const runtime = runtimes.get(id);
  runtimes.set(id, {
    modelJson,
    saveTarget: currentProjectTarget,
    resident: runtime?.resident ?? true,
    lastUsedAt: Date.now(),
    workspaceTab: activeWorkspaceTab(),
    viewState,
  });
  syncActiveSummary();
  return id;
}

async function loadModelState(
  modelJson: string,
): Promise<ProjectTabViewState> {
  const engine = await getEngine();
  const update = await engine.loadProjectModel(modelJson);
  const [finishedSketches, datumPlanes, bodyAppearances, drawingDocument, assemblyDocument, assemblySolution, projectVisibility] = await Promise.all([
    engine.finishedSketches(),
    engine.datumPlaneDefinitions(),
    engine.bodyAppearances(),
    engine.drawingDocument(),
    engine.assemblyDocument(),
    engine.assemblySolution(),
    engine.projectVisibility(),
  ]);
  return { update, finishedSketches, datumPlanes, bodyAppearances, drawingDocument, assemblyDocument, assemblySolution, projectVisibility };
}

async function currentModelState(): Promise<ProjectTabViewState> {
  const engine = await getEngine();
  const [document, scene, finishedSketches, datumPlanes, bodyAppearances, drawingDocument, assemblyDocument, assemblySolution, projectVisibility] =
    await Promise.all([
      engine.getDocument(),
      engine.solidScene(),
      engine.finishedSketches(),
      engine.datumPlaneDefinitions(),
      engine.bodyAppearances(),
      engine.drawingDocument(),
      engine.assemblyDocument(),
      engine.assemblySolution(),
      engine.projectVisibility(),
    ]);
  return {
    update: { document, scene },
    finishedSketches,
    datumPlanes,
    bodyAppearances,
    drawingDocument,
    assemblyDocument,
    assemblySolution,
    projectVisibility,
  };
}

async function hydrateProjectTab(tabId: string): Promise<void> {
  const state = useAppStore.getState();
  const tab = state.projectTabs.find((candidate) => candidate.id === tabId);
  const runtime = runtimes.get(tabId);
  if (!tab || !runtime) {
    throw new Error(translate('file.tabUnavailable'));
  }

  const previousTabId = state.activeProjectTabId;
  const engine = await getEngine();
  let createdColdContext = false;
  try {
    const retained = await engine.activateProjectSession(tabId);
    let projectState = retained ? runtime.viewState : null;
    if (!retained) {
      await engine.createProjectSession(tabId);
      createdColdContext = true;
      projectState = await loadModelState(runtime.modelJson);
    } else if (!projectState) {
      // Recovery normally leaves only its active tab resident. This fallback
      // keeps the engine API robust if a host restores contexts independently.
      projectState = await currentModelState();
    }
    runtimes.set(tabId, {
      ...runtime,
      resident: true,
      lastUsedAt: Date.now(),
      viewState: projectState,
    });
    currentProjectTarget = runtime.saveTarget;
    useAppStore
      .getState()
      .loadProjectState(
        projectState.update,
        projectState.finishedSketches,
        projectState.datumPlanes,
        tab.fileName,
        projectState.bodyAppearances,
        projectState.drawingDocument,
        projectState.assemblyDocument,
        projectState.projectVisibility,
        projectState.assemblySolution,
      );
    useAppStore.setState({
      activeProjectTabId: tabId,
      dirty: tab.dirty,
    });
    // The workspace stage is per-project state nested under the project tab:
    // re-enter the stage this project was last viewed in. If an entry guard
    // rejects the transition, fall back to plain modeling.
    try {
      if (runtime.workspaceTab === 'drawing') await enterDrawingWorkspace();
    } catch {
      useAppStore.getState().setActiveTab('solid');
    }
  } catch (error) {
    // A cold reload is transactional from the user's perspective: keep the
    // outgoing tab active and dispose the partial replacement context.
    if (previousTabId && previousTabId !== tabId) {
      try {
        await engine.activateProjectSession(previousTabId);
        if (createdColdContext) {
          await engine.dropProjectSession(tabId);
        }
      } catch {
        // Preserve the original load error, which is the actionable failure.
      }
    }
    if (createdColdContext) {
      runtimes.set(tabId, { ...runtime, resident: false });
    }
    throw error;
  }
}

async function withProjectTransition(
  operation: (replacingActiveModel: () => void) => Promise<boolean>,
): Promise<boolean> {
  const state = useAppStore.getState();
  if (state.solidBusy || state.historyEdit) return false;
  let releaseTransition: ((changed?: boolean, published?: boolean) => void) | undefined;
  let changed = false;
  let published = false;
  state.setSolidBusy(true);
  try {
    const result = await operation(() => {
      releaseTransition ??= projectTransitions.begin();
      changed = true;
    });
    // Closing an inactive tab or selecting the active tab can succeed without
    // publishing the active native model. Those operations must not clear an
    // earlier unverified load, nor invalidate an otherwise valid export.
    published = changed && result;
    return result;
  } finally {
    useAppStore.getState().setSolidBusy(false);
    releaseTransition?.(changed, published);
  }
}

/** Register the engine document loaded during application startup. */
export async function initializeProjectTabs(): Promise<void> {
  const id = await ensureActiveProjectTab();
  await (await getEngine()).bindProjectSession(id);
  const runtime = runtimes.get(id);
  if (runtime) {
    runtimes.set(id, { ...runtime, resident: true, lastUsedAt: Date.now() });
  }
}

/** Add a fresh document while preserving the current one as an inactive tab. */
export function createProjectTab(): Promise<boolean> {
  return withProjectTransition(async (replacingActiveModel) => {
    await snapshotActiveProjectTab();
    const engine = await getEngine();
    const id = createTabId();
    replacingActiveModel();
    const update = await engine.createProjectSession(id);
    const modelJson = await engine.exportProjectModel();
    currentProjectTarget = null;
    useAppStore.getState().loadProjectState(update, [], [], null);

    runtimes.set(id, {
      modelJson,
      saveTarget: null,
      resident: true,
      lastUsedAt: Date.now(),
      workspaceTab: 'solid',
      viewState: {
        update,
        finishedSketches: [],
        datumPlanes: [],
        bodyAppearances: [],
        drawingDocument: emptyDrawingDocument(),
        assemblyDocument: emptyAssemblyDocument(),
        assemblySolution: emptyAssemblySolution(),
        projectVisibility: { hidden_body_ids: [], hidden_datum_plane_ids: [], hidden_sketch_names: [] },
      },
    });
    const state = useAppStore.getState();
    useAppStore.setState({
      activeProjectTabId: id,
      projectTabs: [...state.projectTabs, summaryFromActiveState(id)],
    });
    return true;
  });
}

/** Hydrate an existing tab into the one native modeling/rendering engine. */
export function switchProjectTab(tabId: string): Promise<boolean> {
  return withProjectTransition(async (replacingActiveModel) => {
    const state = useAppStore.getState();
    if (tabId === state.activeProjectTabId) return true;
    if (!state.projectTabs.some((tab) => tab.id === tabId)) return false;
    await snapshotActiveProjectTab();
    replacingActiveModel();
    await hydrateProjectTab(tabId);
    return true;
  });
}

/** Close one tab after its caller has resolved any unsaved-work decision.
 * A dirty tab remains protected unless `discardUnsaved` is explicit. */
export function closeProjectTab(
  tabId?: string,
  discardUnsaved = false,
): Promise<boolean> {
  return withProjectTransition(async (replacingActiveModel) => {
    const state = useAppStore.getState();
    const id = tabId ?? state.activeProjectTabId;
    if (!id) return false;
    const index = state.projectTabs.findIndex((tab) => tab.id === id);
    if (index < 0) return false;
    const tab = state.projectTabs[index];
    const dirty = id === state.activeProjectTabId ? state.dirty : tab.dirty;
    if (dirty && !discardUnsaved) return false;

    if (id !== state.activeProjectTabId) {
      const runtime = runtimes.get(id);
      if (runtime?.resident) {
        await (await getEngine()).dropProjectSession(id);
      }
      runtimes.delete(id);
      dropApplicationHistory(id);
      useAppStore.setState({
        projectTabs: state.projectTabs.filter((candidate) => candidate.id !== id),
      });
      return true;
    }

    if (state.projectTabs.length > 1) {
      const adjacent =
        state.projectTabs[index + 1] ?? state.projectTabs[index - 1];
      replacingActiveModel();
      await hydrateProjectTab(adjacent.id);
      const runtime = runtimes.get(id);
      if (runtime?.resident) {
        await (await getEngine()).dropProjectSession(id);
      }
      runtimes.delete(id);
      dropApplicationHistory(id);
      useAppStore.setState((current) => ({
        projectTabs: current.projectTabs.filter(
          (candidate) => candidate.id !== id,
        ),
      }));
      return true;
    }

    const engine = await getEngine();
    replacingActiveModel();
    const update = await engine.newProject();
    const modelJson = await engine.exportProjectModel();
    dropApplicationHistory(id);
    currentProjectTarget = null;
    runtimes.set(id, {
      modelJson,
      saveTarget: null,
      resident: true,
      lastUsedAt: Date.now(),
      workspaceTab: 'solid',
      viewState: {
        update,
        finishedSketches: [],
        datumPlanes: [],
        bodyAppearances: [],
        drawingDocument: emptyDrawingDocument(),
        assemblyDocument: emptyAssemblyDocument(),
        assemblySolution: emptyAssemblySolution(),
        projectVisibility: { hidden_body_ids: [], hidden_datum_plane_ids: [], hidden_sketch_names: [] },
      },
    });
    useAppStore.getState().loadProjectState(update, [], [], null);
    useAppStore.setState({
      activeProjectTabId: id,
      projectTabs: [summaryFromActiveState(id)],
    });
    return true;
  });
}

export function getCurrentProjectTarget(): SaveTarget | null {
  return currentProjectTarget;
}

/** Keep active-tab metadata and its reusable Save target in sync after Save. */
export async function recordActiveProjectSave(
  modelJson: string,
  saveTarget: SaveTarget | null,
  assertCurrent?: () => void,
): Promise<void> {
  const id = await ensureActiveProjectTab(modelJson);
  assertCurrent?.();
  currentProjectTarget = saveTarget;
  const runtime = runtimes.get(id);
  runtimes.set(id, {
    modelJson,
    saveTarget,
    resident: runtime?.resident ?? true,
    lastUsedAt: runtime?.lastUsedAt ?? Date.now(),
    workspaceTab: activeWorkspaceTab(),
    viewState: activeViewState(),
  });
  syncActiveSummary();
}

/** Replace only the active tab after Open; sibling tabs remain untouched. */
export async function recordActiveProjectOpen(
  modelJson: string,
  saveTarget: SaveTarget | null,
): Promise<void> {
  const id = await ensureActiveProjectTab(modelJson);
  // Open replaces this tab's entire project, so neither Solid nor Drawing
  // history from the discarded document may operate on the loaded model.
  dropApplicationHistory(id);
  currentProjectTarget = saveTarget;
  const runtime = runtimes.get(id);
  runtimes.set(id, {
    modelJson,
    saveTarget,
    resident: runtime?.resident ?? true,
    lastUsedAt: runtime?.lastUsedAt ?? Date.now(),
    workspaceTab: activeWorkspaceTab(),
    viewState: activeViewState(),
  });
  syncActiveSummary();
}

export function recordActiveProjectMetadata(): void {
  syncActiveSummary();
}

export function hasUnsavedProjects(): boolean {
  const state = useAppStore.getState();
  // Include the active summary as well as the live flag. During a tab
  // hydration these update in adjacent Zustand commits; a brief false
  // positive is safe, while a false negative could erase crash recovery.
  return state.dirty || state.projectTabs.some((tab) => tab.dirty);
}

/** Capture every dirty tab for crash recovery without exposing save paths. */
export async function collectRecoverableProjectTabs(): Promise<{
  activeTabId: string | null;
  tabs: RecoverableProjectTab[];
}> {
  const owner = captureProjectOwner();
  const { state } = owner;
  const capturedRuntimes = new Map(runtimes);
  let activeModelJson: string | null = null;
  if (state.dirty && !state.activeSketch && !state.historyEdit) {
    try {
      activeModelJson = await exportProjectModelWithVisibility(undefined, owner.assertCurrent);
      owner.assertSettled();
      if (state.activeProjectTabId) {
        runtimes.set(state.activeProjectTabId, {
          modelJson: activeModelJson,
          saveTarget: currentProjectTarget,
          resident: runtimes.get(state.activeProjectTabId)?.resident ?? true,
          lastUsedAt:
            runtimes.get(state.activeProjectTabId)?.lastUsedAt ?? Date.now(),
          workspaceTab: activeWorkspaceTab(),
          viewState: activeViewState(),
        });
      }
    } catch {
      activeModelJson = null;
    }
  }

  const tabs = state.projectTabs.flatMap((tab): RecoverableProjectTab[] => {
    const dirty =
      tab.id === state.activeProjectTabId ? state.dirty : tab.dirty;
    if (!dirty) return [];
    const modelJson =
      tab.id === state.activeProjectTabId
        ? activeModelJson ?? capturedRuntimes.get(tab.id)?.modelJson
        : capturedRuntimes.get(tab.id)?.modelJson;
    if (!modelJson) return [];
    return [{
      id: tab.id,
      name:
        tab.id === state.activeProjectTabId
          ? state.document?.name ?? tab.name
          : tab.name,
      fileName:
        tab.id === state.activeProjectTabId
          ? state.projectFileName
          : tab.fileName,
      modelJson,
    }];
  });
  return { activeTabId: state.activeProjectTabId, tabs };
}

/** Restore all recoverable tabs, hydrating only one into OCCT/Bevy. */
export async function restoreProjectTabs(
  recovered: RecoverableProjectTab[],
  requestedActiveId: string | null,
): Promise<boolean> {
  if (recovered.length === 0) return false;
  const active =
    recovered.find((tab) => tab.id === requestedActiveId) ?? recovered[0];
  const { update, finishedSketches, datumPlanes, bodyAppearances, drawingDocument, assemblyDocument, assemblySolution, projectVisibility } =
    await loadModelState(active.modelJson);

  runtimes.clear();
  for (const tab of recovered) {
    runtimes.set(tab.id, {
      modelJson: tab.modelJson,
      saveTarget: null,
      resident: false,
      lastUsedAt: Date.now(),
      workspaceTab: 'solid',
      viewState:
        tab.id === active.id
          ? { update, finishedSketches, datumPlanes, bodyAppearances, drawingDocument, assemblyDocument, assemblySolution, projectVisibility }
          : null,
    });
  }
  currentProjectTarget = null;
  useAppStore
    .getState()
    .loadProjectState(
      update,
      finishedSketches,
      datumPlanes,
      active.fileName,
      bodyAppearances,
      drawingDocument,
      assemblyDocument,
      projectVisibility,
      assemblySolution,
    );
  useAppStore.setState({
    activeProjectTabId: active.id,
    dirty: true,
    projectTabs: recovered.map((tab) => ({
      id: tab.id,
      name: tab.id === active.id ? update.document.name : tab.name,
      fileName: tab.fileName,
      dirty: true,
      workspaceTab: 'solid',
    })),
  });
  return true;
}

async function systemMemoryStatus(): Promise<SystemMemoryStatus | null> {
  if (!isTauriRuntime()) return null;
  try {
    const { invoke } = await import('@tauri-apps/api/core');
    return await invoke<SystemMemoryStatus>('system_memory_status');
  } catch {
    // Idle eviction remains available if an older shell lacks this command.
    return null;
  }
}

async function evictProjectRuntimes(tabIds: string[]): Promise<void> {
  const state = useAppStore.getState();
  if (state.projectBusy || state.solidBusy || tabIds.length === 0) return;
  const activeId = state.activeProjectTabId;
  const candidates = tabIds.filter((id) => {
    const runtime = runtimes.get(id);
    return id !== activeId && runtime?.resident === true;
  });
  if (candidates.length === 0) return;

  // Coordinate with modeling and tab transitions without exposing a second
  // lock to UI code. Dropping an inactive context is normally near-instant.
  state.setSolidBusy(true);
  try {
    const engine = await getEngine();
    for (const id of candidates) {
      if (useAppStore.getState().activeProjectTabId === id) continue;
      const runtime = runtimes.get(id);
      if (!runtime?.resident) continue;
      await engine.dropProjectSession(id);
      runtimes.set(id, { ...runtime, resident: false, viewState: null });
    }
  } finally {
    useAppStore.getState().setSolidBusy(false);
  }
}

async function enforceProjectTabRetention(): Promise<void> {
  const state = useAppStore.getState();
  if (
    state.projectTabs.length < 2 ||
    state.projectBusy ||
    state.solidBusy
  ) {
    return;
  }
  const now = Date.now();
  const inactiveResident = state.projectTabs
    .filter((tab) => tab.id !== state.activeProjectTabId)
    .map((tab) => ({ id: tab.id, runtime: runtimes.get(tab.id) }))
    .filter(
      (entry): entry is { id: string; runtime: ProjectTabRuntime } =>
        entry.runtime?.resident === true,
    )
    .sort((left, right) => left.runtime.lastUsedAt - right.runtime.lastUsedAt);

  const staleIds = inactiveResident
    .filter((entry) => now - entry.runtime.lastUsedAt >= LONG_IDLE_EVICTION_MS)
    .map((entry) => entry.id);
  await evictProjectRuntimes(staleIds);

  const memory = await systemMemoryStatus();
  if (!memory || memory.pressure === 'normal') return;
  const remaining = inactiveResident.filter(
    (entry) => runtimes.get(entry.id)?.resident === true,
  );
  await evictProjectRuntimes(
    memory.pressure === 'critical'
      ? remaining.map((entry) => entry.id)
      : remaining.slice(0, 1).map((entry) => entry.id),
  );
}

/**
 * Keep professional documents warm by default, with a portable macOS/Windows
 * safety valve for very old tabs and low physical memory. The active tab is
 * never eligible for eviction.
 */
export function installProjectTabRetention(): () => void {
  let running = false;
  const check = () => {
    if (running) return;
    running = true;
    void enforceProjectTabRetention()
      .catch(() => undefined)
      .finally(() => {
        running = false;
      });
  };
  const timer = window.setInterval(check, RETENTION_CHECK_MS);
  const onVisibilityChange = () => {
    if (document.visibilityState === 'visible') check();
  };
  document.addEventListener('visibilitychange', onVisibilityChange);
  return () => {
    window.clearInterval(timer);
    document.removeEventListener('visibilitychange', onVisibilityChange);
  };
}
