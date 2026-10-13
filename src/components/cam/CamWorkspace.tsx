import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from '../../i18n';
import {
  AlertTriangle,
  Clock3,
  Pause,
  Play,
  RefreshCw,
  RotateCcw,
  Route,
  Settings2,
  SkipBack,
  SkipForward,
  X,
} from 'lucide-react';
import { activeCamSetup, findCamOperation, setCamUnits } from '../../cam/document';
import { modeledStockBodyId } from '../../cam/geometry';
import { beginCamActivity, camSimulationInputKey, useCamActivity } from '../../cam/simulationUi';
import { adjacentCamMove, camPlaybackRange } from '../../cam/playback';
import { StockPlaybackBuffer } from '../../cam/stockPlaybackBuffer';
import { commitLength, displayLength, lengthUnitLabel } from '../../cam/units';
import { getEngine } from '../../engine';
import type {
  CamDocumentDto,
  CamProgramDto,
  CamSetupDto,
  CamSimulationResultDto,
  CamSimulationTargetDto,
  CamStockMeshDto,
  CamUnits,
  SolidSceneDto,
} from '../../engine/types';
import { camPickCandidateKey, cancelCamPointPick } from '../../cam/pointPick';
import {
  useAppStore,
  type CamDialogState,
  type CamSimulationPlaybackState,
} from '../../store/appStore';
import {
  simulationMeshPresentationWarnings,
  simulationPlaybackPose,
} from '../../cam/overlay';
import { Viewport } from '../viewport/Viewport';
import { runCamAction } from './CamBrowser';
import { CamOperationDialog } from './CamOperationDialog';
import { CamAdaptiveDialog } from './CamAdaptiveDialog';
import { CamFlatDialog } from './CamFlatDialog';
import {
  CamGcodeSimulationDialog,
  type CamGcodeSimulationInput,
} from './CamGcodeSimulationDialog';
import { CamPostDialog } from './CamPostDialog';
import { CamSetupDialog } from './CamSetupDialog';
import { CamToolDialog } from './CamToolDialog';

type SimulationInput = { kind: 'cam' } | CamGcodeSimulationInput;
type SimulationDetail = 'auto' | 'fine' | 'balanced' | 'fast';
const StableViewport = memo(Viewport);

export function CamWorkspace() {
  const { t } = useTranslation();
  const cam = useAppStore((state) => state.camDocument);
  const scene = useAppStore((state) => state.solidScene);
  const selectedOperationId = useAppStore((state) => state.selectedCamOperationId);
  // The planned program and volumetric simulation live in the store so the
  // shared viewport's overlay collector can read them between React renders.
  const program = useAppStore((state) => state.camProgram);
  const simulation = useAppStore((state) => state.camSimulation);
  const simulationTimeline = useAppStore((state) => state.camSimulationTimeline);
  const simulationPlayback = useAppStore((state) => state.camSimulationPlayback);
  const workpieceView = useAppStore((state) => state.camWorkpieceView);
  const toolpathsVisible = useAppStore((state) => state.camToolpathsVisible);
  const resolvedTheme = useAppStore((state) => state.resolvedTheme);
  const pick = useAppStore((state) => state.camPointPick);
  const setup = activeCamSetup(cam);
  const operation = findCamOperation(cam, selectedOperationId);
  const verificationScopeName = simulation?.through_operation_id == null
    ? null
    : findCamOperation(cam, simulation.through_operation_id)?.name
      ?? t('cam.workspace.operationFallback').replace('{id}', String(simulation.through_operation_id));
  const units = cam.units;
  const [planError, setPlanError] = useState<string | null>(null);
  const [generation, setGeneration] = useState(0);
  // Share the complete plan across row clicks. Only a failed full setup needs
  // a scoped fallback; healthy selections must not re-run the planner.
  const planRequest = useRef<{
    document: CamDocumentDto; setupId: number; generation: number;
    complete: Promise<CamProgramDto>;
  } | null>(null);
  const [busy, setBusy] = useState(false);
  const [simulationError, setSimulationError] = useState<string | null>(null);
  const [simulationGeneration, setSimulationGeneration] = useState(0);
  const [simulationBusy, setSimulationBusy] = useState(false);
  const [simulationFrameBusy, setSimulationFrameBusy] = useState(false);
  const [simulationInput, setSimulationInput] = useState<SimulationInput>({ kind: 'cam' });
  const [simulationDetail, setSimulationDetail] = useState<SimulationDetail>('auto');
  const [comparisonToleranceMm, setComparisonToleranceMm] = useState(0.1);
  const [gcodeDialogOpen, setGcodeDialogOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsPosition, setSettingsPosition] = useState({ left: 0, top: 0 });
  const settingsButtonRef = useRef<HTMLButtonElement>(null);
  const settingsPopoverRef = useRef<HTMLDivElement>(null);
  const ribbonSection = useCamActivity((state) => state.ribbonSection);
  const [ribbonHost, setRibbonHost] = useState<HTMLElement | null>(null);
  useLayoutEffect(() => {
    setRibbonHost(ribbonSection === 'simulate' ? document.getElementById('cam-simulation-toolbar') : null);
    setSettingsOpen(false);
  }, [ribbonSection]);
  useEffect(() => {
    if (!settingsOpen) return;
    const close = () => setSettingsOpen(false);
    const onPointer = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!settingsButtonRef.current?.contains(target) && !settingsPopoverRef.current?.contains(target)) close();
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      close();
      settingsButtonRef.current?.focus();
    };
    document.addEventListener('pointerdown', onPointer);
    document.addEventListener('keydown', onKey);
    window.addEventListener('resize', close);
    return () => {
      document.removeEventListener('pointerdown', onPointer);
      document.removeEventListener('keydown', onKey);
      window.removeEventListener('resize', close);
    };
  }, [settingsOpen]);
  const [playbackSession, setPlaybackSession] = useState<{ setupId: number; operationId: number | null; serial: number } | null>(null);
  const intent = useCamActivity((state) => state.intent);
  const playbackOpen = simulationInput.kind === 'gcode' || (playbackSession !== null
    && playbackSession.setupId === setup?.id && playbackSession.operationId === selectedOperationId);
  const simulationFrameRequest = useRef(0);
  const stockPlaybackBuffer = useRef<StockPlaybackBuffer | null>(null);
  const preparedTargetKey = useRef<string | null>(null);
  const stockMesh = useMemo(
    () => setup ? simulationStockMesh(setup, cam, scene) : null,
    [cam, scene, setup],
  );
  const targetCacheKey = useMemo(
    () => setup
      ? camSimulationInputKey(cam, scene, setup.id, simulationDetail, comparisonToleranceMm)
      : null,
    [cam, comparisonToleranceMm, scene, setup, simulationDetail],
  );
  const simulationTargetInput = useMemo(
    () => setup && targetCacheKey
      ? simulationTarget(setup, cam, scene, comparisonToleranceMm, targetCacheKey, true)
      : null,
    [cam, comparisonToleranceMm, scene, setup, targetCacheKey],
  );
  const playbackTargetInput = useMemo(
    () => simulationTargetInput
      ? { ...simulationTargetInput, meshes: [] }
      : null,
    [simulationTargetInput],
  );
  const simulationScope = simulationInput.kind === 'cam' ? selectedOperationId : null;
  const playbackRange = useMemo(() => camPlaybackRange(program, simulationTimeline,
    simulationInput.kind === 'cam' ? simulationScope : null), [program, simulationTimeline, simulationInput.kind, simulationScope]);
  useEffect(() => {
    if (!intent || intent.setupId !== setup?.id) return;
    useCamActivity.setState({ intent: null });
    if (intent.kind === 'nc') { setGcodeDialogOpen(true); return; }
    setSimulationInput({ kind: 'cam' });
    setPlaybackSession(intent);
  }, [intent, setup?.id]);
  useEffect(() => {
    setPlaybackSession((current) => current && (current.setupId !== setup?.id || current.operationId !== selectedOperationId) ? null : current);
  }, [setup?.id, selectedOperationId]);
  const programWarnings = program?.warnings ?? [];
  // Simulation starts with the program warnings and appends its own accuracy
  // and collision-model limitations. Show those additions exactly once; the
  // old panel rendered program warnings only and silently hid simulator ones.
  const presentationWarnings = simulation
    ? simulationMeshPresentationWarnings(simulation)
    : [];
  const simulationWarnings = [...new Set([
    ...(simulationTimeline?.warnings ?? simulation?.warnings ?? []),
    ...presentationWarnings,
  ])].filter((warning) => !programWarnings.includes(warning));
  const simulationResolution = simulation
    ? formatSimulationResolution(simulation, units)
    : null;

  useEffect(() => {
    const { setCamProgram } = useAppStore.getState();
    if (!setup || !setup.operations.some((operation) => operation.enabled)) {
      setCamProgram(null);
      setPlanError(null);
      setBusy(false);
      return;
    }
    let cancelled = false;
    setBusy(true);
    const finish = beginCamActivity(t('cam.workspace.preparingCamMoves'));
    if (planRequest.current?.document !== cam || planRequest.current.setupId !== setup.id
      || planRequest.current.generation !== generation) {
      planRequest.current = { document: cam, setupId: setup.id, generation,
        complete: getEngine().then((engine) => engine.camPlan(setup.id)) };
    }
    void planRequest.current.complete
      .catch((error) => {
        if (cancelled || selectedOperationId == null) throw error;
        // Preserve every required predecessor, but don't let a later error
        // erase this operation's already-valid paths and tool pose.
        return getEngine().then((engine) => engine.camPlan(setup.id, selectedOperationId));
      })
      .then((next) => {
        if (cancelled) return;
        setCamProgram(next);
        setPlanError(null);
      })
      .catch((error) => {
        if (cancelled) return;
        finish.fail(error);
        setCamProgram(null);
        setPlanError(error instanceof Error ? error.message : String(error));
      })
      .finally(() => {
        finish();
        if (!cancelled) setBusy(false);
      });
    return () => {
      cancelled = true;
      finish.discard();
    };
  }, [cam, setup?.id, generation, selectedOperationId]);

  useEffect(() => {
    simulationFrameRequest.current += 1;
    const {
      setCamSimulation,
      setCamSimulationTimeline,
      setCamSimulationPlayback,
    } = useAppStore.getState();
    // A stale result must never paint over a freshly selected setup.
    setCamSimulation(null);
    setCamSimulationTimeline(null);
    setCamSimulationPlayback(null);
    setSimulationFrameBusy(false);
    setSimulationError(null);
    if (!setup) {
      setSimulationBusy(false);
      return;
    }
    let cancelled = false;
    setSimulationBusy(true);
    const finish = beginCamActivity(t(playbackOpen ? 'cam.workspace.preparingSimulation' : 'cam.workspace.loadingRemainingStock'));
    const voxelSize = simulationVoxelSize(simulationDetail, setup);
    const maxVoxels = simulationVoxelBudget(simulationDetail);
    void getEngine()
      .then(async (engine) => {
        const run = (target: typeof simulationTargetInput) => simulationInput.kind === 'cam'
          ? engine.camSimulate({
              setup_id: setup.id,
              voxel_size: voxelSize,
              max_voxels: maxVoxels,
              stock_mesh: stockMesh,
              target,
              // The selected operation's review cannot include later removal.
              through_operation_id: simulationScope,
              // Selecting a setup is an incoming-stock view, not an implicit
              // complete simulation followed by a second zero-step request.
              completed_steps: !playbackOpen && simulationScope === null ? 0 : null,
            })
          : engine.camSimulateGcode({
              setup_id: setup.id,
              source: simulationInput.source,
              file_name: simulationInput.fileName,
              dialect: simulationInput.dialect,
              voxel_size: voxelSize,
              max_voxels: maxVoxels,
              stock_mesh: stockMesh,
              target,
            });
        const canReuseTarget = targetCacheKey !== null
          && preparedTargetKey.current === targetCacheKey;
        try {
          return await run(canReuseTarget ? playbackTargetInput : simulationTargetInput);
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error);
          if (!canReuseTarget || !message.includes('target cache is not prepared')) throw error;
          // Both Rust caches are deliberately bounded. If this setup was
          // evicted, resend the exact target once and prepare it again.
          preparedTargetKey.current = null;
          return run(simulationTargetInput);
        }
      })
      .then((next) => {
        if (cancelled) return;
        preparedTargetKey.current = targetCacheKey;
        // Ordinary selection has no animation clock. An explicit simulation
        // owns a complete physical timeline and starts from its scope's
        // incoming stock (the preceding operations for a single toolpath).
        const range = camPlaybackRange(useAppStore.getState().camProgram, next, simulationScope);
        useAppStore.setState({
          camSimulation: playbackOpen ? null : next,
          camSimulationTimeline: next,
          camSimulationPlayback: playbackOpen ? { playing: false, time_seconds: range.start, speed: 1 } : null,
        });
        setSimulationError(null);
      })
      .catch((error) => {
        if (cancelled) return;
        finish.fail(error);
        setCamSimulation(null);
        setSimulationError(error instanceof Error ? error.message : String(error));
      })
      .finally(() => {
        finish();
        if (!cancelled) setSimulationBusy(false);
      });
    return () => {
      cancelled = true;
      finish.discard();
    };
  }, [
    cam,
    setup?.id,
    simulationGeneration,
    scene,
    simulationInput,
    simulationScope,
    simulationDetail,
    comparisonToleranceMm,
    simulationTargetInput,
    playbackTargetInput,
    stockMesh,
    targetCacheKey,
    playbackOpen,
    playbackSession?.serial,
  ]);

  // The presentation clock is independent from the renderer. Bevy continues
  // receiving pointer/orbit input while React advances only this timestamp.
  useEffect(() => {
    if (!simulationTimeline || !simulationPlayback?.playing) return;
    let animationFrame = 0;
    let previous = performance.now();
    const tick = (now: number) => {
      const state = useAppStore.getState();
      const playback = state.camSimulationPlayback;
      if (!playback?.playing || state.camSimulationTimeline !== simulationTimeline) return;
      const elapsed = Math.min(0.1, Math.max(0, (now - previous) / 1000));
      previous = now;
      const wanted = Math.min(
        simulationTimeline.estimated_seconds,
        playback.time_seconds + elapsed * playback.speed,
      );
      // Never reveal removal ahead of the cutter or run beyond prepared stock.
      // Usually several snapshots are ready; underruns wait at the buffer edge
      // without blocking pointer/orbit events or showing a busy cursor.
      const buffer = stockPlaybackBuffer.current;
      const time = buffer ? Math.max(playback.time_seconds, Math.min(wanted, buffer.readyUntil)) : wanted;
      state.setCamSimulationPlayback({
        ...playback,
        playing: time < simulationTimeline.estimated_seconds - 1e-9,
        time_seconds: time,
      });
      if (time < simulationTimeline.estimated_seconds - 1e-9) {
        animationFrame = requestAnimationFrame(tick);
      }
    };
    animationFrame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(animationFrame);
  }, [simulationTimeline, simulationPlayback?.playing]);

  // A single-flight, throttled stock worker, NOT an effect debounced by each
  // animation tick (which used to starve during continuous playback). The
  // physical cutter clock stays independent, so orbit remains responsive.
  useEffect(() => {
    if (!playbackOpen || !setup || !simulationTimeline) return;
    if (useAppStore.getState().camSimulationTimeline !== simulationTimeline) return;
    if (simulationTimeline.setup_id !== setup.id) return;
    if (
      (simulationInput.kind === 'cam' && simulationTimeline.source !== 'cam_toolpath')
      || (simulationInput.kind === 'gcode' && simulationTimeline.source !== 'g_code')
    ) return;

    if (simulationInput.kind === 'cam') {
      let cancelled = false;
      let buffer: StockPlaybackBuffer | null = null;
      void getEngine().then((engine) => {
        if (cancelled) return;
        const range = camPlaybackRange(useAppStore.getState().camProgram, simulationTimeline, simulationScope);
        buffer = new StockPlaybackBuffer(engine, {
          setup_id: setup.id,
          voxel_size: simulationVoxelSize(simulationDetail, setup),
          max_voxels: simulationVoxelBudget(simulationDetail),
          stock_mesh: stockMesh,
          // Open only once: sending the full target also handles an evicted
          // verification cache. Samples thereafter contain only session/time.
          target: simulationTargetInput,
          through_operation_id: simulationTimeline.through_operation_id,
        }, range.start, range.end,
        () => useAppStore.getState().camSimulationPlayback,
        (next) => {
          if (cancelled || useAppStore.getState().camSimulationTimeline !== simulationTimeline) return;
          useAppStore.getState().setCamSimulation(next);
          setSimulationError(null);
        },
        (buffering) => { if (!cancelled) setSimulationFrameBusy(buffering); },
        (error) => {
          if (cancelled) return;
          setSimulationError(error instanceof Error ? error.message : String(error));
          const playback = useAppStore.getState().camSimulationPlayback;
          if (playback) useAppStore.getState().setCamSimulationPlayback({ ...playback, playing: false });
        });
        stockPlaybackBuffer.current = buffer;
        void buffer.start();
      });
      return () => {
        cancelled = true;
        buffer?.close();
        if (stockPlaybackBuffer.current === buffer) stockPlaybackBuffer.current = null;
      };
    }

    const requestId = ++simulationFrameRequest.current;
    let inFlight = false;
    let lastTime = -1;
    let finishActivity: (() => void) | null = null;
    const update = () => {
      const playback = useAppStore.getState().camSimulationPlayback;
      if (!playback || inFlight || lastTime === playback.time_seconds) return;
      const time = playback.time_seconds;
      inFlight = true;
      setSimulationFrameBusy(true);
      const voxelSize = simulationVoxelSize(simulationDetail, setup);
      const maxVoxels = simulationVoxelBudget(simulationDetail);
      void getEngine()
        .then(async (engine) => {
          const run = (target: CamSimulationTargetDto | null) => engine.camSimulateGcode({
              setup_id: setup.id,
              source: simulationInput.source,
              file_name: simulationInput.fileName,
              dialect: simulationInput.dialect,
              voxel_size: voxelSize,
              max_voxels: maxVoxels,
              stock_mesh: stockMesh,
              target,
              completed_steps: completedStepsAtTime(simulationTimeline, time),
            });
          try { return await run(playbackTargetInput); }
          catch (error) {
            const message = error instanceof Error ? error.message : String(error);
            if (!message.includes('target cache is not prepared')) throw error;
            return run(simulationTargetInput);
          }
        })
        .then((next) => {
          if (simulationFrameRequest.current !== requestId) return;
          const state = useAppStore.getState();
          if (state.camSimulationTimeline !== simulationTimeline) return;
          state.setCamSimulation(next);
          lastTime = time;
          setSimulationError(null);
        })
        .catch((error) => {
          if (simulationFrameRequest.current !== requestId) return;
          // Do not retry a failed paused frame every timer tick. A new seek,
          // play action or input change can make a deliberate new request.
          lastTime = time;
          setSimulationError(error instanceof Error ? error.message : String(error));
          const playback = useAppStore.getState().camSimulationPlayback;
          if (playback) useAppStore.getState().setCamSimulationPlayback({ ...playback, playing: false });
        })
        .finally(() => {
          finishActivity?.();
          finishActivity = null;
          inFlight = false;
          if (simulationFrameRequest.current === requestId) setSimulationFrameBusy(false);
        });
    };
    update();
    const timer = window.setInterval(update, 125);
    return () => {
      window.clearInterval(timer);
      simulationFrameRequest.current += 1;
      finishActivity?.();
    };
  }, [
    playbackOpen,
    scene,
    setup,
    simulationInput,
    simulationTimeline,
    simulationDetail,
    comparisonToleranceMm,
    playbackTargetInput,
    simulationTargetInput,
    stockMesh,
  ]);

  // Escape cancels an active viewport point-pick session.
  useEffect(() => {
    if (!pick) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        cancelCamPointPick();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [pick]);

  // The manufacturing tab opens straight onto the modeled parts, even before
  // the first setup exists; setups are created from the ribbon or the empty
  // sidebar panel, never implicitly.

  const runGcodeSimulation = (input: CamGcodeSimulationInput) => {
    setGcodeDialogOpen(false);
    setSimulationInput(input);
    setSimulationGeneration((value) => value + 1);
  };

  const togglePlayback = () => {
    if (!simulationTimeline || !simulationPlayback) return;
    const atEnd = simulationPlayback.time_seconds >= simulationTimeline.estimated_seconds - 1e-9;
    useAppStore.getState().setCamSimulationPlayback({
      ...simulationPlayback,
      playing: !simulationPlayback.playing,
      time_seconds: !simulationPlayback.playing && atEnd ? playbackRange.start : simulationPlayback.time_seconds,
    });
  };

  const seekPlayback = (time: number) => {
    if (!simulationPlayback || !simulationTimeline) return;
    useAppStore.getState().setCamSimulationPlayback({
      ...simulationPlayback,
      playing: false,
      time_seconds: Math.max(playbackRange.start, Math.min(playbackRange.end, time)),
    });
  };
  const closeSimulation = () => {
    setPlaybackSession(null);
    setSimulationInput({ kind: 'cam' });
  };

  useEffect(() => {
    if (!playbackOpen || !simulationTimeline) return;
    const onKey = (event: KeyboardEvent) => {
      const element = event.target as HTMLElement;
      if (element.closest('input, select, textarea, button, [contenteditable="true"], [role="dialog"]')) return;
      const state = useAppStore.getState();
      if (state.camDialog || state.camPointPick) return;
      const playback = state.camSimulationPlayback;
      if (!playback) return;
      if (event.code === 'Space') {
        event.preventDefault();
        state.setCamSimulationPlayback({ ...playback, playing: !playback.playing,
          time_seconds: playback.time_seconds >= playbackRange.end ? playbackRange.start : playback.time_seconds });
      } else if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
        event.preventDefault();
        state.setCamSimulationPlayback({ ...playback, playing: false,
          time_seconds: adjacentCamMove(simulationTimeline, playback.time_seconds, event.key === 'ArrowLeft' ? -1 : 1, playbackRange.start, playbackRange.end) });
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [playbackOpen, simulationTimeline, playbackRange.start, playbackRange.end]);

  const inSimulationRibbon = ribbonSection === 'simulate' && ribbonHost !== null;
  const playbackControls = playbackOpen && simulationTimeline && simulationPlayback ? (
    <SimulationPlaybackControls
      inRibbon={inSimulationRibbon}
      timeline={simulationTimeline}
      playback={simulationPlayback}
      buffering={simulationFrameBusy}
      onToggle={togglePlayback}
      startTime={playbackRange.start}
      scopeName={operation?.name ?? setup?.name ?? t('cam.workspace.setupFallback')}
      onReset={() => seekPlayback(playbackRange.start)}
      onStep={(direction) => seekPlayback(adjacentCamMove(simulationTimeline, simulationPlayback.time_seconds, direction, playbackRange.start, playbackRange.end))}
      onClose={closeSimulation}
      onSeek={seekPlayback}
      onSpeed={(speed) => useAppStore.getState().setCamSimulationPlayback({ ...simulationPlayback, speed })}
    />
  ) : null;
  const preparationStatus = playbackOpen && !simulationTimeline ? (
    <div data-testid="cam-simulation-preparing" data-native-viewport-overlay
      className={inSimulationRibbon ? 'flex h-[62px] items-center justify-between gap-4 text-[11px] text-mute'
        : 'absolute bottom-3 left-1/2 z-30 flex -translate-x-1/2 items-center gap-4 rounded-lg border border-edge bg-header/95 px-3 py-2 text-[11px] text-mute shadow-xl'}>
      <span>{simulationError ? t('cam.workspace.simulationCouldNotPrepare') : t('cam.workspace.preparingSimulation')}</span>
      <button type="button" aria-label={t('cam.workspace.closeSimulation')} title={t('cam.workspace.returnToRemainingStock')} onClick={closeSimulation} className="drawing-mini-button"><X size={14} /></button>
    </div>
  ) : null;

  return (
    <div className="flex h-full min-h-0 bg-viewport" data-testid="cam-workspace">
      <section className="flex min-w-0 flex-1 flex-col">
        <div className="flex h-10 shrink-0 items-center justify-between border-b border-edge bg-header px-3">
          <div className="flex min-w-0 items-center gap-2 text-[11px] text-mute">
            {setup ? (
              <>
                <span className="truncate font-semibold text-ink">{setup.name}</span>
                <span>·</span>
                <span className="uppercase">
                  {setup.work_offset}
                  {setup.work_offset_count > 1 && ` → ${WORK_OFFSETS[WORK_OFFSETS.indexOf(setup.work_offset) + setup.work_offset_count - 1]}`}
                </span>
                <span>·</span>
                <span className="truncate" title={t('cam.workspace.predictionTitle')}>
                  {setup.machine?.profile.name ?? t('cam.workspace.generic3Axis')}
                </span>
                {program && <ProgramStats program={program} units={units} />}
              </>
            ) : (
              <span className="text-mute">
                {t('cam.workspace.noSetupHint')}
              </span>
            )}
          </div>
          <div className="relative flex shrink-0 items-center gap-1.5">
            <div role="group" aria-label={t('cam.workspace.workpieceView')} className="flex h-7 overflow-hidden rounded border border-edge bg-panel" data-testid="cam-workpiece-view">
              {([
                ['model', t('cam.workspace.viewModel'), t('cam.workspace.viewModelTitle')],
                ['stock', t('cam.workspace.viewStock'), t('cam.workspace.viewStockTitle')],
                ['compare', t('cam.workspace.viewCompare'), t('cam.workspace.viewCompareTitle')],
              ] as const).map(([mode, label, title]) => (
                <button key={mode} type="button" aria-pressed={workpieceView === mode} title={title}
                  data-testid={`cam-view-${mode}`} onClick={() => useAppStore.getState().setCamWorkpieceView(mode)}
                  className={`px-2 text-[10px] font-semibold focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent ${workpieceView === mode ? 'bg-accent/15 text-accent' : 'text-mute hover:bg-edge hover:text-ink'}`}>
                  {label}
                </button>
              ))}
            </div>
            <button type="button" aria-pressed={toolpathsVisible} aria-label={t('cam.workspace.showToolpaths')}
              title={t('cam.workspace.showToolpathsTitle')}
              data-testid="cam-toolpaths-toggle" onClick={() => useAppStore.getState().setCamToolpathsVisible(!toolpathsVisible)}
              className={`flex h-7 items-center gap-1 rounded border border-edge px-2 text-[10px] ${toolpathsVisible ? 'bg-accent/10 text-accent' : 'text-mute hover:text-ink'}`}>
              <Route size={12} /> {t('cam.workspace.paths')}
            </button>
            <button
              type="button"
              title={t('cam.workspace.unitsToggleTitle')}
              onClick={() => runCamAction(() => setCamUnits(units === 'millimeters' ? 'inches' : 'millimeters'))}
              className="flex h-7 items-center rounded border border-edge bg-panel px-2.5 text-[10px] font-semibold text-mute hover:border-accent/40 hover:text-accent"
              data-testid="cam-units-toggle"
            >
              {units === 'millimeters' ? 'mm' : t('cam.workspace.unitInch')}
            </button>
            {simulation && simulationResolution && (
              <span
                data-testid="cam-simulation-resolution"
                title={t('cam.workspace.gridTitle').replace('{dims}', simulation.dimensions.join(' × ')).replace('{resolution}', simulationResolution)}
                className="rounded border border-edge bg-panel px-2 py-1 font-mono text-[9px] text-mute"
              >
                {t('cam.workspace.gridLabel').replace('{resolution}', simulationResolution)}
              </span>
            )}
            <button
              type="button"
              disabled={!setup}
              title={t('cam.workspace.refreshPreview')}
              onClick={() => setGeneration((value) => value + 1)}
              className="drawing-mini-button disabled:cursor-not-allowed disabled:opacity-40"
            >
              <RefreshCw size={14} className={busy ? 'animate-spin' : ''} />
            </button>
          </div>
        </div>
        <div className="relative min-h-0 flex-1 overflow-hidden">
          {/* The manufacturing tab shares the modeling viewport outright:
              same navigation, same grid, same model. CAM adds its overlays
              (stock, WCS, toolpaths, simulated stock) through the viewport's
              transient preview channel. */}
          <StableViewport key={resolvedTheme} />
          {!inSimulationRibbon && preparationStatus}
          {pick && (
            <div className="pointer-events-none absolute left-1/2 top-3 z-10 max-w-[80%] -translate-x-1/2 rounded border border-accent/50 bg-header/90 px-3 py-1.5 text-center text-[11px] text-accent shadow-xl backdrop-blur-sm">
              {pick.prompt} · {t('cam.workspace.pickHint')}
              {pick.hoverKey && <div className="mt-1 text-ink">{pick.candidates.find(c => camPickCandidateKey(c) === pick.hoverKey)?.label}</div>}
            </div>
          )}
          {!inSimulationRibbon && playbackControls}
          {simulation?.comparison && (
            <SimulationVerificationPanel
              simulation={simulation}
              units={units}
              scopeName={verificationScopeName}
              displaySimplified={presentationWarnings.length > 0}
            />
          )}
          {/* Machining-time readout: the selected operation's own totals when
              one is selected, otherwise the whole setup's program. */}
          {program && !playbackOpen && (
            <div
              data-testid="cam-machining-time"
              className="pointer-events-none absolute bottom-3 right-3 z-10 rounded border border-edge bg-header/85 px-2.5 py-1 font-mono text-[10px] text-mute shadow backdrop-blur-sm"
            >
              {operation
                ? t('cam.workspace.machiningTime').replace('{name}', operation.name).replace('{time}', formatMachiningTime(program.per_operation.find((entry) => entry.operation_id === operation.id)?.estimated_seconds))
                : t('cam.workspace.totalMachiningTime').replace('{name}', program.name).replace('{time}', formatMachiningTime(program.stats.estimated_seconds))}
            </div>
          )}
          {(planError
            || simulationError
            || programWarnings.length
            || simulationWarnings.length
            || (simulationTimeline?.collisions.length ?? simulation?.collisions.length)) && (
            <div className="absolute bottom-3 left-3 max-h-48 max-w-3xl overflow-y-auto rounded border border-[#d69b45]/45 bg-[#2a2117]/95 p-2.5 text-[10px] text-[#e8c589] shadow-lg">
              <div className="flex items-start gap-2">
                <AlertTriangle size={14} className="mt-0.5 shrink-0" />
                <div>
                  {planError && <div data-testid="cam-planning-error" className="font-semibold text-[#ffbd66]">{planError}</div>}
                  {simulationError && simulationError !== planError && <div data-testid="cam-simulation-error" className="font-semibold text-[#ffbd66]">{simulationError}</div>}
                  {(simulationTimeline?.collisions ?? simulation?.collisions ?? []).map((collision) => {
                    const step = simulationTimeline?.steps.find(
                      (candidate) => candidate.command_index === collision.command_index,
                    );
                    const location = step?.source_line != null
                      ? t('cam.workspace.block').replace('{number}', String(step.source_line))
                      : t('cam.workspace.move').replace('{number}', String(collision.command_index + 1));
                    return (
                      <button
                        type="button"
                        key={`${collision.kind}-${collision.command_index}-${collision.message}`}
                        disabled={!step}
                        onClick={() => step && seekPlayback(step.cumulative_seconds)}
                        title={step ? t('cam.workspace.jumpTo').replace('{location}', location) : undefined}
                        className={`block text-left font-semibold hover:underline disabled:no-underline ${
                          collision.kind === 'target_gouge' ? 'text-[#ff75a8]' : 'text-[#e8c589]'
                        }`}
                      >
                        {collision.message} · {location}
                      </button>
                    );
                  })}
                  {programWarnings.map((warning) => <div key={warning}>{warning}</div>)}
                  {simulationWarnings.map((warning) => <div key={warning}>{warning}</div>)}
                </div>
              </div>
            </div>
          )}
        </div>
      </section>
      {inSimulationRibbon && ribbonHost && createPortal(<>
        <div data-ribbon-panel="playback" className="flex min-w-[420px] max-w-3xl flex-1 flex-col border-r border-edge px-3">
          {playbackControls ?? preparationStatus ?? <div className="flex h-[62px] items-center text-[11px] text-mute">{t('ribbon.cam.simulationIdle')}</div>}
          <span className="flex h-5 shrink-0 items-center justify-center text-[10px] uppercase tracking-wider text-mute/60">{t('ribbon.cam.playback')}</span>
        </div>
        <div data-ribbon-panel="display" className="flex shrink-0 flex-col border-r border-edge px-3">
          <div className="flex h-[62px] flex-col items-center justify-center gap-1">
            <select aria-label={t('cam.workspace.simulationDetailAria')} disabled={!setup || simulationBusy}
              value={simulationDetail} onChange={(event) => setSimulationDetail(event.target.value as SimulationDetail)}
              title={t('cam.workspace.simulationDetailTitle')}
              className="h-6 w-full rounded border border-edge bg-panel px-2 text-[9px] font-semibold text-mute outline-none disabled:opacity-40">
              <option value="auto">{t('cam.workspace.detailAuto')}</option><option value="fine">{t('cam.workspace.detailFine')}</option>
              <option value="balanced">{t('cam.workspace.detailBalanced')}</option><option value="fast">{t('cam.workspace.detailFast')}</option>
            </select>
            <div className="flex items-center gap-1">
              <button ref={settingsButtonRef} type="button" title={t('cam.workspace.simulationSettings')} aria-label={t('cam.workspace.simulationSettings')}
                aria-expanded={settingsOpen} aria-controls={settingsOpen ? 'cam-simulation-settings' : undefined}
                onClick={() => {
                  const bounds = settingsButtonRef.current?.getBoundingClientRect();
                  if (bounds) setSettingsPosition({ left: Math.max(8, Math.min(bounds.right - 288, window.innerWidth - 296)), top: bounds.bottom + 8 });
                  setSettingsOpen(!settingsOpen);
                }} className="drawing-mini-button"><Settings2 size={14} /></button>
            </div>
          </div>
          <span className="flex h-5 items-center justify-center text-[10px] uppercase tracking-wider text-mute/60">{t('ribbon.cam.display')}</span>
        </div>
      </>, ribbonHost)}
      {settingsOpen && createPortal(
        <div id="cam-simulation-settings" ref={settingsPopoverRef} role="dialog" aria-label={t('cam.workspace.simulationSettings')} data-native-viewport-overlay
          style={settingsPosition} className="fixed z-[100] w-72 rounded-lg border border-edge bg-panel p-3 shadow-xl">
          <div className="mb-2 flex items-center justify-between text-[11px] font-semibold text-ink">{t('cam.workspace.simulationSettings')}
            <button type="button" aria-label={t('cam.workspace.closeSimulationSettings')} onClick={() => setSettingsOpen(false)}><X size={13} /></button>
          </div>
          <label className="flex h-7 items-center gap-1 rounded border border-edge bg-panel px-2 text-[9px] text-mute"
            title={t('cam.workspace.toleranceTitle')}>
            {t('cam.workspace.compare')}
            <input aria-label={t('cam.workspace.partComparisonTolerance')} type="number" min={0} step={units === 'millimeters' ? 0.01 : 0.001}
              value={Number(displayLength(comparisonToleranceMm, units).toFixed(units === 'millimeters' ? 3 : 4))}
              onChange={(event) => {
                const value = Number(event.target.value);
                if (Number.isFinite(value) && value >= 0) setComparisonToleranceMm(commitLength(value, units));
              }} className="w-14 bg-transparent text-right font-mono text-ink outline-none" />
            {lengthUnitLabel(units)}
          </label>
          <p className="mt-2 text-[10px] leading-relaxed text-mute">{t('cam.workspace.detailHelp')}</p>
        </div>, document.body)}
      {/* No right sidebar in the manufacturing workspace: setup and
          operation configuration live in double-click dialogs. */}
      <CamDialogHost />
      {gcodeDialogOpen && (
        <CamGcodeSimulationDialog
          initial={simulationInput.kind === 'gcode' ? simulationInput : null}
          onClose={() => setGcodeDialogOpen(false)}
          onRun={runGcodeSimulation}
        />
      )}
    </div>
  );
}

/** Renders the active manufacturing editor dialog, if any. When the tool
 *  library is stacked on top as a picker, the suspended dialog underneath
 *  stays mounted so its drafts survive the round trip. Creation and editing
 *  share one dialog each — editing just seeds the same drafts from the stored
 *  setup/operation (the key re-seeds when the edit target changes). */
function CamDialogHost() {
  const dialog = useAppStore((state) => state.camDialog);
  const below = useAppStore((state) => state.camDialogBelow);
  const cam = useAppStore((state) => state.camDocument);
  if (!dialog) return null;
  const render = (state: CamDialogState) => {
    if (state.type === 'setup') {
      const setupEdit =
        state.editId != null
          ? cam.setups.find((candidate) => candidate.id === state.editId) ?? null
          : null;
      if (state.editId != null && !setupEdit) return null;
      return <CamSetupDialog key={state.editId ?? 'new'} editing={setupEdit ?? undefined} />;
    }
    if (state.type === 'operation') {
      const operationEdit = state.editId != null ? findCamOperation(cam, state.editId) : null;
      if (state.editId != null && !operationEdit) return null;
      if (state.kind === 'adaptive3d') {
        return <CamAdaptiveDialog key={`adaptive3d-${state.editId ?? 'new'}`}
          insertion={state.insertion}
          editing={operationEdit?.kind === 'adaptive3d' ? operationEdit : undefined} />;
      }
      if (state.kind === 'flat3d') {
        return <CamFlatDialog key={`flat3d-${state.editId ?? 'new'}`}
          insertion={state.insertion}
          editing={operationEdit?.kind === 'flat3d' ? operationEdit : undefined} />;
      }
      return (
        <CamOperationDialog
          key={`${state.kind}-${state.editId ?? 'new'}`}
          kind={state.kind}
          insertion={state.insertion}
          editing={operationEdit ?? undefined}
        />
      );
    }
    if (state.type === 'post') return <CamPostDialog />;
    return <CamToolDialog toolId={state.toolId} pickFor={state.pickFor ?? null} />;
  };
  return (
    <>
      {below ? render(below) : null}
      {render(dialog)}
    </>
  );
}

function ProgramStats({ program, units }: { program: CamProgramDto; units: CamUnits }) {
  return (
    <div className="ml-2 flex items-center gap-2">
      <span className="flex items-center gap-1 rounded bg-edge/50 px-1.5 py-0.5 font-mono text-[9px]">
        <Route size={10} /> {displayLength(program.stats.cutting_distance, units).toFixed(1)} {lengthUnitLabel(units)}
      </span>
      <span className="flex items-center gap-1 rounded bg-edge/50 px-1.5 py-0.5 font-mono text-[9px]">
        <Clock3 size={10} /> {formatDuration(program.stats.estimated_seconds)}
      </span>
    </div>
  );
}

function SimulationVerificationPanel({
  simulation,
  units,
  scopeName,
  displaySimplified,
}: {
  simulation: CamSimulationResultDto;
  units: CamUnits;
  scopeName: string | null;
  displaySimplified: boolean;
}) {
  const { t } = useTranslation();
  const comparison = simulation.comparison;
  if (!comparison) return null;
  const inProgress = simulation.completed_steps !== null;
  const hasTargetLoss = comparison.gouged_voxels > 0 || comparison.initial_shortfall_voxels > 0;
  const hasExcess = comparison.excess_voxels > 0;
  const status = inProgress
    ? t('cam.workspace.statusInProgress')
    : hasTargetLoss
      ? t('cam.workspace.statusReviewOvercut')
      : hasExcess
        ? scopeName
          ? t('cam.workspace.statusStockRemainsAtStage')
          : t('cam.workspace.statusStockRemains')
        : t('cam.workspace.statusWithinTolerance');
  const statusClass = inProgress
    ? 'border-accent/45 bg-accent/10 text-accent'
    : hasTargetLoss
      ? 'border-[#ef6a62]/60 bg-[#ef6a62]/15 text-[#ff9a91]'
      : hasExcess
        ? 'border-[#e39a38]/55 bg-[#e39a38]/12 text-[#f2bb68]'
        : 'border-[#4fd17b]/55 bg-[#4fd17b]/12 text-[#7fe3a0]';
  return (
    <div
      data-testid="cam-simulation-verification"
      className="pointer-events-none absolute left-3 top-3 z-10 min-w-64 max-w-72 rounded border border-edge bg-header/90 p-2.5 text-[10px] text-mute shadow-lg backdrop-blur-sm"
    >
      <div className="mb-2 flex items-center justify-between gap-3">
        <span className="font-semibold uppercase tracking-wide text-ink">{t('cam.workspace.partVerification')}</span>
        <span className={`rounded border px-1.5 py-0.5 text-[9px] font-semibold ${statusClass}`}>
          {status}
        </span>
      </div>
      {scopeName && (
        <div className="mb-2 rounded border border-accent/25 bg-accent/8 px-1.5 py-1 text-[9px] text-ink">
          {t('cam.workspace.scopeThrough').replace('{name}', scopeName)}
        </div>
      )}
      {displaySimplified && (
        <div className="mb-2 text-[9px] font-semibold text-[#f2bb68]">
          {t('cam.workspace.simplified')}
        </div>
      )}
      <div className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 font-mono text-[9px]">
        <span>{t('cam.workspace.removedStock')}</span>
        <span data-testid="cam-removed-volume" className="text-right text-ink">{formatSimulationVolume(simulation.removed_volume_mm3, units)}</span>
        <span className="text-[#f2bb68]">{t('cam.workspace.extraStock')}</span>
        <span className="text-right text-ink">{formatSimulationVolume(comparison.excess_volume_mm3, units)}</span>
        <span className="text-[#ff75a8]">{t('cam.workspace.overcut')}</span>
        <span className="text-right text-ink">{formatSimulationVolume(comparison.gouged_volume_mm3, units)}</span>
        <span>{t('cam.workspace.effectiveBand')}</span>
        <span className="text-right text-ink">
          ±{displayLength(comparison.effective_tolerance_mm, units).toFixed(units === 'millimeters' ? 3 : 4)} {lengthUnitLabel(units)}
        </span>
      </div>
    </div>
  );
}

function SimulationPlaybackControls({
  inRibbon = false,
  timeline,
  playback,
  buffering,
  onToggle,
  onReset,
  onSeek,
  onSpeed,
  startTime,
  scopeName,
  onStep,
  onClose,
}: {
  inRibbon?: boolean;
  timeline: CamSimulationResultDto;
  playback: CamSimulationPlaybackState;
  buffering: boolean;
  onToggle: () => void;
  onReset: () => void;
  onSeek: (time: number) => void;
  onSpeed: (speed: number) => void;
  startTime: number;
  scopeName: string;
  onStep: (direction: -1 | 1) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const pose = simulationPlaybackPose(timeline, playback.time_seconds);
  const total = Math.max(0, timeline.estimated_seconds);
  return (
    <div
      data-testid="cam-simulation-playback"
      data-native-viewport-overlay={inRibbon ? undefined : true}
      data-placement={inRibbon ? 'ribbon' : 'viewport'}
      aria-label={t('cam.workspace.playbackAria').replace('{name}', scopeName)}
      className={inRibbon ? 'flex h-[62px] min-w-0 flex-col justify-center gap-1'
        : 'absolute bottom-3 left-1/2 z-30 flex w-[min(720px,calc(100%-32px))] -translate-x-1/2 flex-col gap-1 rounded-lg border border-edge bg-header/95 px-3 py-2 shadow-xl backdrop-blur-sm'}
    >
      <div className="flex h-6 min-w-0 items-center gap-2">
      <span className="shrink-0 rounded bg-accent/15 px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wide text-accent">
        {timeline.source === 'g_code' ? t('cam.workspace.ncProgram') : t('cam.workspace.camPrediction')}
      </span>
      <span title={scopeName} className="min-w-0 flex-1 truncate text-[10px] text-mute">{scopeName}</span>
      {pose?.sourceLine !== null && pose?.sourceLine !== undefined && (
        <span className="shrink-0 rounded border border-edge px-1.5 py-0.5 font-mono text-[9px] text-mute">{t('cam.workspace.block').replace('{number}', String(pose.sourceLine))}</span>
      )}
      <span title={buffering ? t('cam.workspace.buffering') : t('cam.workspace.snapshotsReady')}
        className={`h-2 w-2 shrink-0 rounded-full ${buffering ? 'animate-pulse bg-warn' : 'bg-[#4fd17b]'}`} />
      <button type="button" title={t('cam.workspace.closeSimulationReturn')} aria-label={t('cam.workspace.closeSimulation')} onClick={onClose} className="drawing-mini-button"><X size={14} /></button>
      </div>
      <div className="flex min-w-0 items-center gap-1.5">
      <button
        type="button"
        onClick={onReset}
        title={t('cam.workspace.returnToBeginning')}
        className="drawing-mini-button"
      >
        <RotateCcw size={13} />
      </button>
      <button type="button" title={t('cam.workspace.previousMove')} aria-label={t('cam.workspace.previousMove')} onClick={() => onStep(-1)} className="drawing-mini-button"><SkipBack size={13} /></button>
      <button
        type="button"
        onClick={onToggle}
        title={playback.playing ? t('cam.workspace.pauseSimulation') : t('cam.workspace.playSimulation')}
        className="flex h-7 w-7 shrink-0 items-center justify-center rounded border border-accent/50 bg-accent/15 text-accent hover:bg-accent/25"
      >
        {playback.playing ? <Pause size={13} fill="currentColor" /> : <Play size={13} fill="currentColor" />}
      </button>
      <button type="button" title={t('cam.workspace.nextMove')} aria-label={t('cam.workspace.nextMove')} onClick={() => onStep(1)} className="drawing-mini-button"><SkipForward size={13} /></button>
      <input
        aria-label={t('cam.workspace.simulationTime')}
        type="range"
        min={startTime}
        max={Math.max(total, startTime + 0.001)}
        step={Math.max(total / 2000, 0.001)}
        value={Math.min(playback.time_seconds, Math.max(total, 0.001))}
        onChange={(event) => onSeek(Number(event.target.value))}
        className="min-w-0 flex-1 accent-[var(--accent)]"
      />
      <span className="w-[92px] shrink-0 text-right font-mono text-[9px] text-mute">
        {formatPlaybackTime(playback.time_seconds - startTime)} / {formatPlaybackTime(total - startTime)}
      </span>
      <select
        aria-label={t('cam.workspace.playbackSpeed')}
        value={playback.speed}
        onChange={(event) => onSpeed(Number(event.target.value))}
        className="h-7 shrink-0 rounded border border-edge bg-panel px-1.5 text-[9px] font-semibold text-mute outline-none"
      >
        <option value={0.25}>¼×</option>
        <option value={0.5}>½×</option>
        <option value={1}>1×</option>
        <option value={2}>2×</option>
        <option value={5}>5×</option>
        <option value={10}>10×</option>
      </select>
      </div>
    </div>
  );
}

function completedStepsAtTime(timeline: CamSimulationResultDto, timeSeconds: number): number {
  const time = Math.max(0, Math.min(timeline.estimated_seconds, timeSeconds));
  let low = 0;
  let high = timeline.steps.length;
  while (low < high) {
    const middle = Math.floor((low + high) / 2);
    if (timeline.steps[middle].cumulative_seconds <= time + 1e-9) low = middle + 1;
    else high = middle;
  }
  return low;
}

function simulationStockMesh(
  setup: CamSetupDto,
  cam: CamDocumentDto,
  scene: SolidSceneDto,
): CamStockMeshDto | null {
  const bodyId = modeledStockBodyId(setup, cam);
  if (bodyId === null) return null;
  const body = scene.bodies.find((candidate) => candidate.id === bodyId);
  return body ? { positions: body.mesh.positions, indices: body.mesh.indices } : null;
}

function simulationTarget(
  setup: CamSetupDto,
  cam: CamDocumentDto,
  scene: SolidSceneDto,
  toleranceMm: number,
  cacheKey: string,
  includeMeshes: boolean,
): CamSimulationTargetDto | null {
  const bodyIds = new Set(setup.body_ids);
  const stockBodyId = modeledStockBodyId(setup, cam);
  const targetBodies = scene.bodies
    // A dedicated modeled-stock body is raw material, never the intended
    // finished part even if a legacy/default setup selection included it.
    .filter((body) => bodyIds.has(body.id) && body.id !== stockBodyId);
  if (targetBodies.length === 0) return null;
  return {
    cache_key: cacheKey,
    meshes: includeMeshes
      ? targetBodies.map((body) => ({ positions: body.mesh.positions, indices: body.mesh.indices }))
      : [],
    tolerance_mm: toleranceMm,
  };
}

function simulationVoxelSize(detail: SimulationDetail, setup: CamSetupDto): number | null {
  // Every level is dimensionless: it describes samples across the stock's
  // longest side rather than a hard-coded millimetre size. Auto is a little
  // over 10x the former volumetric density (352^3 / 160^3 = 10.65).
  const extent = [
    setup.stock.max.x - setup.stock.min.x,
    setup.stock.max.y - setup.stock.min.y,
    setup.stock.max.z - setup.stock.min.z,
  ];
  const longestSide = Math.max(...extent);
  const cellsAcross = detail === 'fine'
    ? 512
    : detail === 'balanced'
      ? 256
      : detail === 'fast'
        ? 128
        : 352;
  return longestSide / cellsAcross;
}

function simulationVoxelBudget(detail: SimulationDetail): number | null {
  if (detail === 'fine' || detail === 'auto') return 8_000_000;
  if (detail === 'balanced') return 4_000_000;
  return 1_000_000;
}

function formatPlaybackTime(seconds: number): string {
  if (!Number.isFinite(seconds)) return '—';
  const value = Math.max(0, seconds);
  const hours = Math.floor(value / 3600);
  const minutes = Math.floor((value % 3600) / 60);
  const rest = value % 60;
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, '0')}:${rest.toFixed(1).padStart(4, '0')}`
    : `${minutes}:${rest.toFixed(1).padStart(4, '0')}`;
}

function formatSimulationResolution(simulation: CamSimulationResultDto, units: CamUnits): string {
  const edge = Math.max(...simulation.cell_size);
  const precision = units === 'millimeters' ? 3 : 4;
  return `${displayLength(edge, units).toFixed(precision)} ${lengthUnitLabel(units)}`;
}

function formatSimulationVolume(volumeMm3: number, units: CamUnits): string {
  if (!Number.isFinite(volumeMm3)) return '—';
  if (units === 'inches') return `${(volumeMm3 / (25.4 ** 3)).toFixed(4)} in³`;
  return `${volumeMm3.toFixed(volumeMm3 < 10 ? 2 : 1)} mm³`;
}

const WORK_OFFSETS = ['g54', 'g55', 'g56', 'g57', 'g58', 'g59'] as const;

function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds)) return '—';
  if (seconds < 60) return `${Math.ceil(seconds)}s`;
  const minutes = Math.floor(seconds / 60);
  const remainder = Math.round(seconds % 60);
  return `${minutes}m ${remainder}s`;
}

/** Machinist-style h:mm:ss readout for the manufacturing status line. */
function formatMachiningTime(seconds: number | undefined): string {
  if (seconds === undefined || !Number.isFinite(seconds)) return '—';
  const total = Math.max(0, Math.round(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const rest = total % 60;
  return `${hours}:${String(minutes).padStart(2, '0')}:${String(rest).padStart(2, '0')}`;
}
