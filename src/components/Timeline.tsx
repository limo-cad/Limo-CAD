/**
 * Parametric build history. `rollback_index` is a feature count: entries
 * before the build cursor participate in recompute; entries after it are
 * rolled back. Clicking feature cards selects them without recomputing;
 * Cmd/Ctrl and Shift extend that selection. The build cursor moves only through
 * its deliberate controls. Double-clicking a supported feature opens its editor.
 */
import {
  ArrowLeft,
  ArrowLeftToLine,
  ArrowRight,
  ArrowRightToLine,
  Box,
  CircleAlert,
  History,
  Layers3,
  Link2,
  Move3d,
  MoveRight,
  PanelTop,
  Pencil,
  PenLine,
  RefreshCw,
  Blend,
  CircleDot,
  Copy,
  FileUp,
  Grid2X2,
  Scissors,
  Shell,
  Trash2,
  Triangle,
} from 'lucide-react';
import {
  Fragment,
  useCallback,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type MouseEvent,
  type PointerEvent as ReactPointerEvent,
} from 'react';
import {
  beginTimelineFeatureEdit,
  deleteTimelineFeatures,
  editSketch,
  openExtrude,
  openLoft,
  openRevolve,
  openRib,
  openSweep,
  openSolidFillet,
  openSolidChamfer,
  openHole,
  openBodyFeature,
  openConstructionPlane,
  reorderTimelineFeature,
  setTimelineRollback,
} from '../engine/controller';
import { useTranslation } from '../i18n';
import { cx } from '../lib/cx';
import { useAppStore } from '../store/appStore';
import type { FeatureDto } from '../types/document';
import type { JointDefinitionDto } from '../engine/types';
import { ContextMenu, type ContextMenuEntry } from './ContextMenu';
import { DeleteFeatureDialog } from './DeleteFeatureDialog';

function editTimelineFeature(feature: FeatureDto) {
  void beginTimelineFeatureEdit(feature.id, async (engine, assertCurrent) => {
    if (feature.kind === 'sketch') return editSketch(feature.name, assertCurrent);
    if (feature.kind === 'extrude') openExtrude(feature.id);
    if (feature.kind === 'revolve') openRevolve(feature.id);
    if (feature.kind === 'sweep') openSweep(feature.id);
    if (feature.kind === 'loft') openLoft(feature.id);
    if (feature.kind === 'rib') openRib(feature.id);
    if (feature.kind === 'fillet') openSolidFillet(feature.id);
    if (feature.kind === 'chamfer') openSolidChamfer(feature.id);
    if (feature.kind === 'hole') openHole(feature.id);
    if (feature.kind === 'external_thread') {
      openBodyFeature('external_thread', feature.id);
    }
    if (feature.kind === 'move_copy') openBodyFeature('move_copy', feature.id);
    if (feature.kind === 'construction_plane') {
      // The stage view deliberately hides this datum while it is being
      // edited, so resolve its retained definition through the engine.
      const definitions = await engine.datumPlaneDefinitions();
      await assertCurrent();
      const definition = definitions.find(
        (plane) => plane.feature_id === feature.id,
      );
      if (!definition) {
        throw new Error(`Could not find construction plane ${feature.name}`);
      }
      openConstructionPlane(definition.source.type, feature.id);
    }
    if (feature.kind === 'shell') openBodyFeature('shell', feature.id);
    if (feature.kind === 'mirror') openBodyFeature('mirror', feature.id);
    if (feature.kind === 'rectangular_pattern') {
      openBodyFeature('rectangular_pattern', feature.id);
    }
    if (feature.kind === 'circular_pattern') {
      openBodyFeature('circular_pattern', feature.id);
    }
    if (feature.kind === 'combine') openBodyFeature('combine', feature.id);
    if (feature.kind === 'split_body') openBodyFeature('split_body', feature.id);
  });
}

function canEditTimelineFeature(feature: FeatureDto): boolean {
  // Imported STEP is an immutable source feature for now. It can still be
  // rolled back, reordered, or deleted; replacing its source file will be a
  // separate command so a double-click never silently opens the wrong editor.
  return feature.kind !== 'import_step';
}

interface TimelineContextTarget {
  feature: FeatureDto;
  index: number;
  x: number;
  y: number;
}

interface JointContextTarget {
  joint: JointDefinitionDto;
  x: number;
  y: number;
}

interface FeatureDrag {
  pointerId: number;
  featureId: number;
  sourceIndex: number;
  targetIndex: number;
  startX: number;
  moved: boolean;
}

export function Timeline() {
  const { t } = useTranslation();
  const document = useAppStore((s) => s.document);
  const busy = useAppStore((s) => s.solidBusy);
  const projectBusy = useAppStore((s) => s.projectBusy);
  const historyEditing = useAppStore((s) => s.historyEdit !== null);
  const mode = useAppStore((s) => s.mode);
  const joints = useAppStore((s) => s.assemblyDocument.joints);
  const selectedJointId = useAppStore((s) => s.selectedJointId);
  const setSelectedJointId = useAppStore((s) => s.setSelectedJointId);
  const setSolidSidebarMode = useAppStore((s) => s.setSolidSidebarMode);
  const openJointEditor = useAppStore((s) => s.openJointEditor);
  const deleteJoint = useAppStore((s) => s.deleteJoint);
  const setJointEnabled = useAppStore((s) => s.setJointEnabled);
  const features = document?.features ?? [];
  const rollback = document?.rollback_index ?? 0;
  const [contextTarget, setContextTarget] = useState<TimelineContextTarget | null>(null);
  const [jointContextTarget, setJointContextTarget] = useState<JointContextTarget | null>(null);
  const [deleteTargets, setDeleteTargets] = useState<FeatureDto[]>([]);
  const [selectedFeatureIds, setSelectedFeatureIds] = useState<Set<number>>(
    () => new Set(),
  );
  const [requestedRollback, setRequestedRollback] = useState<number | null>(null);
  const [drag, setDrag] = useState<{ pointerId: number; index: number } | null>(null);
  const [featureDrag, setFeatureDrag] = useState<FeatureDrag | null>(null);
  const timelineRef = useRef<HTMLDivElement>(null);
  const dragIndexRef = useRef(rollback);
  const featureDragRef = useRef<FeatureDrag | null>(null);
  const suppressFeatureClickRef = useRef<{
    featureId: number;
    expiresAt: number;
  } | null>(null);
  const selectionAnchorRef = useRef<number | null>(null);
  const displayedRollback = drag?.index ?? requestedRollback ?? rollback;
  const interactionLocked = busy || projectBusy || historyEditing;
  const canReorder = mode === 'solid' && !interactionLocked && rollback === features.length;

  useEffect(() => {
    const validIds = new Set(features.map((feature) => feature.id));
    setSelectedFeatureIds((current) => {
      const retained = new Set(
        Array.from(current).filter((featureId) => validIds.has(featureId)),
      );
      return retained.size === current.size ? current : retained;
    });
    if (
      selectionAnchorRef.current !== null
      && !validIds.has(selectionAnchorRef.current)
    ) {
      selectionAnchorRef.current = null;
    }
  }, [document?.features]);

  const selectFeature = useCallback(
    (event: MouseEvent<HTMLButtonElement>, featureId: number, index: number) => {
      const isMacControlClick =
        event.ctrlKey
        && !event.metaKey
        && typeof navigator !== 'undefined'
        && /Mac|iPhone|iPad/.test(navigator.platform);
      if (isMacControlClick) return;

      const additive = event.metaKey || event.ctrlKey;
      const rangeAnchorId = selectionAnchorRef.current;
      setSelectedFeatureIds((current) => {
        const next = additive ? new Set(current) : new Set<number>();
        if (event.shiftKey) {
          const anchorIndex =
            rangeAnchorId === null
              ? -1
              : features.findIndex((feature) => feature.id === rangeAnchorId);
          const start = anchorIndex < 0 ? index : Math.min(anchorIndex, index);
          const end = anchorIndex < 0 ? index : Math.max(anchorIndex, index);
          for (let candidate = start; candidate <= end; candidate += 1) {
            const feature = features[candidate];
            if (feature) next.add(feature.id);
          }
        } else if (additive) {
          if (next.has(featureId)) next.delete(featureId);
          else next.add(featureId);
        } else {
          next.add(featureId);
        }
        return next;
      });
      if (!event.shiftKey || rangeAnchorId === null) {
        selectionAnchorRef.current = featureId;
      }
      setSelectedJointId(null);
    },
    [features, setSelectedJointId],
  );

  const move = useCallback(
    (next: number) => {
      const clamped = Math.max(0, Math.min(features.length, next));
      if (interactionLocked || clamped === rollback) return;
      setRequestedRollback(clamped);
      void setTimelineRollback(clamped).finally(() => {
        setRequestedRollback((current) => (current === clamped ? null : current));
      });
    },
    [features.length, interactionLocked, rollback],
  );

  useEffect(() => {
    if (requestedRollback !== null && requestedRollback === rollback) {
      setRequestedRollback(null);
    }
  }, [requestedRollback, rollback]);

  useEffect(() => {
    if (!drag) return;
    const pointerId = drag.pointerId;
    const nearestIndex = (clientX: number) => {
      const timeline = timelineRef.current;
      if (!timeline) return dragIndexRef.current;
      const bounds = timeline.getBoundingClientRect();
      const edgeZone = 32;
      if (clientX < bounds.left + edgeZone) {
        timeline.scrollLeft -= Math.max(5, Math.ceil((bounds.left + edgeZone - clientX) / 3));
      } else if (clientX > bounds.right - edgeZone) {
        timeline.scrollLeft += Math.max(5, Math.ceil((clientX - bounds.right + edgeZone) / 3));
      }
      const cards = Array.from(
        timeline.querySelectorAll<HTMLElement>('[data-feature-id]'),
      );
      const index = cards.findIndex((card) => {
        const rect = card.getBoundingClientRect();
        return clientX < rect.left + rect.width / 2;
      });
      return index < 0 ? cards.length : index;
    };
    const update = (event: globalThis.PointerEvent) => {
      if (event.pointerId !== pointerId) return;
      event.preventDefault();
      const index = nearestIndex(event.clientX);
      dragIndexRef.current = index;
      setDrag((current) =>
        current && current.pointerId === pointerId && current.index !== index
          ? { ...current, index }
          : current,
      );
    };
    const finish = (event: globalThis.PointerEvent) => {
      if (event.pointerId !== pointerId) return;
      const index = nearestIndex(event.clientX);
      dragIndexRef.current = index;
      setDrag(null);
      move(index);
    };
    const cancel = (event: globalThis.PointerEvent) => {
      if (event.pointerId === pointerId) setDrag(null);
    };
    const cancelOnEscape = (event: globalThis.KeyboardEvent) => {
      if (event.key === 'Escape') setDrag(null);
    };
    window.addEventListener('pointermove', update, { passive: false });
    window.addEventListener('pointerup', finish);
    window.addEventListener('pointercancel', cancel);
    window.addEventListener('keydown', cancelOnEscape);
    return () => {
      window.removeEventListener('pointermove', update);
      window.removeEventListener('pointerup', finish);
      window.removeEventListener('pointercancel', cancel);
      window.removeEventListener('keydown', cancelOnEscape);
    };
  }, [drag?.pointerId, move]);

  const beginCursorDrag = (event: ReactPointerEvent<HTMLButtonElement>) => {
    if (interactionLocked || event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    dragIndexRef.current = displayedRollback;
    setDrag({ pointerId: event.pointerId, index: displayedRollback });
  };

  const beginFeatureDrag = (
    event: ReactPointerEvent<HTMLButtonElement>,
    featureId: number,
    sourceIndex: number,
  ) => {
    if (!canReorder || event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey) {
      return;
    }
    const next: FeatureDrag = {
      pointerId: event.pointerId,
      featureId,
      sourceIndex,
      targetIndex: sourceIndex,
      startX: event.clientX,
      moved: false,
    };
    featureDragRef.current = next;
    setFeatureDrag(next);
  };

  useEffect(() => {
    if (!featureDrag) return;
    const pointerId = featureDrag.pointerId;
    const nearestSlot = (clientX: number) => {
      const timeline = timelineRef.current;
      if (!timeline) return featureDragRef.current?.targetIndex ?? 0;
      const bounds = timeline.getBoundingClientRect();
      const edgeZone = 32;
      if (clientX < bounds.left + edgeZone) {
        timeline.scrollLeft -= Math.max(5, Math.ceil((bounds.left + edgeZone - clientX) / 3));
      } else if (clientX > bounds.right - edgeZone) {
        timeline.scrollLeft += Math.max(5, Math.ceil((clientX - bounds.right + edgeZone) / 3));
      }
      const cards = Array.from(
        timeline.querySelectorAll<HTMLElement>('[data-feature-id]'),
      );
      const index = cards.findIndex((card) => {
        const rect = card.getBoundingClientRect();
        return clientX < rect.left + rect.width / 2;
      });
      return index < 0 ? cards.length : index;
    };
    const update = (event: globalThis.PointerEvent) => {
      const current = featureDragRef.current;
      if (!current || event.pointerId !== pointerId) return;
      const moved = current.moved || Math.abs(event.clientX - current.startX) >= 5;
      if (!moved) return;
      event.preventDefault();
      const targetIndex = nearestSlot(event.clientX);
      const next = { ...current, moved, targetIndex };
      featureDragRef.current = next;
      setFeatureDrag(next);
    };
    const finish = (event: globalThis.PointerEvent) => {
      const current = featureDragRef.current;
      if (!current || event.pointerId !== pointerId) return;
      const targetIndex = current.moved ? nearestSlot(event.clientX) : current.targetIndex;
      featureDragRef.current = null;
      setFeatureDrag(null);
      if (!current.moved) return;
      suppressFeatureClickRef.current = {
        featureId: current.featureId,
        expiresAt: performance.now() + 250,
      };
      const insertionIndex =
        current.sourceIndex < targetIndex ? targetIndex - 1 : targetIndex;
      if (insertionIndex !== current.sourceIndex) {
        void reorderTimelineFeature(current.featureId, targetIndex);
      }
    };
    const cancel = (event: globalThis.PointerEvent) => {
      if (event.pointerId !== pointerId) return;
      featureDragRef.current = null;
      setFeatureDrag(null);
    };
    const cancelOnEscape = (event: globalThis.KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      featureDragRef.current = null;
      setFeatureDrag(null);
    };
    window.addEventListener('pointermove', update, { passive: false });
    window.addEventListener('pointerup', finish);
    window.addEventListener('pointercancel', cancel);
    window.addEventListener('keydown', cancelOnEscape);
    return () => {
      window.removeEventListener('pointermove', update);
      window.removeEventListener('pointerup', finish);
      window.removeEventListener('pointercancel', cancel);
      window.removeEventListener('keydown', cancelOnEscape);
    };
  }, [featureDrag?.pointerId]);

  const consumeSuppressedFeatureClick = (featureId: number) => {
    const suppressed = suppressFeatureClickRef.current;
    if (!suppressed) return false;
    if (performance.now() > suppressed.expiresAt) {
      suppressFeatureClickRef.current = null;
      return false;
    }
    if (suppressed.featureId !== featureId) return false;
    suppressFeatureClickRef.current = null;
    return true;
  };

  const controls = [
    {
      id: 'goToStart',
      icon: <ArrowLeftToLine size={14} />,
      label: t('timeline.goToStart'),
      next: 0,
    },
    {
      id: 'stepBack',
      icon: <ArrowLeft size={14} />,
      label: t('timeline.stepBack'),
      next: rollback - 1,
    },
    {
      id: 'stepForward',
      icon: <ArrowRight size={14} />,
      label: t('timeline.stepForward'),
      next: rollback + 1,
    },
    {
      id: 'goToEnd',
      icon: <ArrowRightToLine size={14} />,
      label: t('timeline.goToEnd'),
      next: features.length,
    },
  ];

  const contextEntries = (): ContextMenuEntry[] => {
    if (!contextTarget) return [];
    const { feature, index } = contextTarget;
    return [
      {
        type: 'item',
        id: feature.kind === 'sketch' ? 'edit-sketch' : 'edit-feature',
        label:
          feature.kind === 'sketch'
            ? t('timeline.editSketch')
            : t('timeline.editFeature'),
        icon: <Pencil size={14} />,
        disabled: mode !== 'solid' || interactionLocked || !canEditTimelineFeature(feature),
        onSelect: () => editTimelineFeature(feature),
      },
      { type: 'separator', id: 'history-separator' },
      {
        type: 'item',
        id: 'rollback-before',
        label: t('timeline.rollbackBefore'),
        icon: <ArrowLeftToLine size={14} />,
        disabled: interactionLocked || rollback === index,
        onSelect: () => move(index),
      },
      {
        type: 'item',
        id: 'rollback-after',
        label: t('timeline.rollbackAfter'),
        icon: <ArrowRightToLine size={14} />,
        disabled: interactionLocked || rollback === index + 1,
        onSelect: () => move(index + 1),
      },
      {
        type: 'item',
        id: 'rollback-end',
        label: t('timeline.rollbackEnd'),
        icon: <ArrowRightToLine size={14} />,
        disabled: interactionLocked || rollback === features.length,
        onSelect: () => move(features.length),
      },
      { type: 'separator', id: 'delete-separator' },
      {
        type: 'item',
        id: 'delete-feature',
        label: t('timeline.deleteFeature'),
        icon: <Trash2 size={14} />,
        danger: true,
        disabled: mode !== 'solid' || interactionLocked,
        onSelect: () => {
          const targets = selectedFeatureIds.has(feature.id)
            ? features.filter((candidate) => selectedFeatureIds.has(candidate.id))
            : [feature];
          setDeleteTargets(targets);
        },
      },
    ];
  };

  const jointContextEntries = (): ContextMenuEntry[] => {
    if (!jointContextTarget) return [];
    const { joint } = jointContextTarget;
    return [
      {
        type: 'item',
        id: 'edit-joint',
        label: t('timeline.editJoint'),
        icon: <Pencil size={14} />,
        disabled: mode !== 'solid' || interactionLocked,
        onSelect: () => openJointEditor(joint.id),
      },
      {
        type: 'item',
        id: 'toggle-joint',
        label: joint.enabled ? t('timeline.suppressJoint') : t('timeline.unsuppressJoint'),
        icon: <Link2 size={14} />,
        disabled: mode !== 'solid' || interactionLocked,
        onSelect: () => void setJointEnabled(joint.id, !joint.enabled),
      },
      { type: 'separator', id: 'delete-joint-separator' },
      {
        type: 'item',
        id: 'delete-joint',
        label: t('timeline.deleteJoint'),
        icon: <Trash2 size={14} />,
        danger: true,
        disabled: mode !== 'solid' || interactionLocked,
        onSelect: () => void deleteJoint(joint.id),
      },
    ];
  };

  return (
    <footer className="flex h-12 shrink-0 items-center gap-2 border-t border-edge bg-panel px-3">
      <div className="flex h-8 shrink-0 items-center gap-2 rounded-lg border border-edge/70 bg-header/55 px-2.5">
        <History size={13} className="text-accent" />
        <span className="text-[9px] font-semibold uppercase tracking-[0.14em] text-mute">
          {t('timeline.historyTitle')}
        </span>
        <span
          className="rounded-md bg-panel px-1.5 py-0.5 font-mono text-[9px] tabular-nums text-ink"
          aria-label={`${displayedRollback} / ${features.length}`}
        >
          {displayedRollback}/{features.length}
        </span>
        {joints.length > 0 && (
          <span
            className="rounded-md bg-accent/10 px-1.5 py-0.5 font-mono text-[9px] tabular-nums text-accent"
            aria-label={`${joints.length} ${t('timeline.joints')}`}
          >
            {joints.length}J
          </span>
        )}
      </div>

      <div
        className="flex shrink-0 items-center gap-0.5 rounded-lg border border-edge/60 bg-header/30 p-0.5"
        aria-label={t('timeline.historyTitle')}
      >
        {controls.map((control) => {
          const disabled =
            interactionLocked ||
            features.length === 0 ||
            Math.max(0, Math.min(features.length, control.next)) === rollback;
          return (
            <button
              key={control.id}
              type="button"
              title={control.label}
              aria-label={control.label}
              disabled={disabled}
              onClick={() => move(control.next)}
              className="flex h-6 w-7 items-center justify-center rounded-md text-mute transition-colors hover:bg-edge hover:text-ink disabled:cursor-default disabled:opacity-25"
            >
              {control.icon}
            </button>
          );
        })}
      </div>

      <div
        ref={timelineRef}
        data-testid="feature-timeline"
        className="relative flex h-9 min-w-0 flex-1 items-center gap-1.5 overflow-x-auto rounded-lg border border-edge/60 bg-header/25 px-2"
      >
        <div
          aria-hidden="true"
          className="pointer-events-none absolute left-2 right-2 top-1/2 h-px -translate-y-1/2 bg-edge/70"
        />
        {features.map((feature, index) => (
          <Fragment key={feature.id}>
            {featureDrag?.moved && featureDrag.targetIndex === index && (
              <FeatureDropIndicator />
            )}
            {index === displayedRollback && (
              <HistoryCursor
                label={t('timeline.rollbackMarker')}
                hint={t('timeline.dragMarker')}
                value={displayedRollback}
                max={features.length}
                busy={interactionLocked}
                dragging={drag !== null}
                onPointerDown={beginCursorDrag}
                onMove={move}
              />
            )}
            <TimelineFeature
              feature={feature}
              index={index}
              active={index < displayedRollback}
              selected={selectedFeatureIds.has(feature.id)}
              busy={interactionLocked}
              reorderEnabled={canReorder}
              reordering={featureDrag?.featureId === feature.id && featureDrag.moved}
              onSelect={(event) => selectFeature(event, feature.id, index)}
              onBeginReorder={(event) => beginFeatureDrag(event, feature.id, index)}
              shouldSuppressClick={() => consumeSuppressedFeatureClick(feature.id)}
              onOpenContext={(x, y) => setContextTarget({ feature, index, x, y })}
            />
          </Fragment>
        ))}
        {featureDrag?.moved && featureDrag.targetIndex === features.length && (
          <FeatureDropIndicator />
        )}
        {features.length > 0 && displayedRollback === features.length && (
          <HistoryCursor
            label={t('timeline.rollbackMarker')}
            hint={t('timeline.dragMarker')}
            value={displayedRollback}
            max={features.length}
            busy={interactionLocked}
            dragging={drag !== null}
            onPointerDown={beginCursorDrag}
            onMove={move}
          />
        )}
        {joints.length > 0 && (
          <div className="relative z-10 mx-0.5 h-5 w-px shrink-0 bg-edge" title={t('timeline.assemblyHistory')} />
        )}
        {joints.map((joint) => (
          <TimelineJoint
            key={joint.id}
            joint={joint}
            selected={joint.id === selectedJointId}
            busy={interactionLocked}
            onSelect={() => {
              setSolidSidebarMode('assembly');
              setSelectedJointId(joint.id);
            }}
            onEdit={() => openJointEditor(joint.id)}
            onOpenContext={(x, y) => {
              setContextTarget(null);
              setJointContextTarget({ joint, x, y });
            }}
          />
        ))}
        {features.length === 0 && joints.length === 0 && (
          <div className="relative z-10 h-1 flex-1 rounded-full bg-edge" />
        )}
      </div>
      {contextTarget && (
        <ContextMenu
          point={contextTarget}
          entries={contextEntries()}
          ariaLabel={`${contextTarget.feature.name} — ${t('timeline.contextMenu')}`}
          onClose={() => setContextTarget(null)}
        />
      )}
      {jointContextTarget && (
        <ContextMenu
          point={jointContextTarget}
          entries={jointContextEntries()}
          ariaLabel={`${jointContextTarget.joint.name} — ${t('timeline.jointContextMenu')}`}
          onClose={() => setJointContextTarget(null)}
        />
      )}
      {deleteTargets.length > 0 && (
        <DeleteFeatureDialog
          features={deleteTargets}
          busy={interactionLocked}
          onCancel={() => setDeleteTargets([])}
          onConfirm={() => {
            const featureIds = deleteTargets.map((feature) => feature.id);
            setDeleteTargets([]);
            void deleteTimelineFeatures(featureIds).then((deleted) => {
              if (deleted) {
                setSelectedFeatureIds(new Set());
                selectionAnchorRef.current = null;
              }
            });
          }}
        />
      )}
    </footer>
  );
}

function TimelineJoint({
  joint,
  selected,
  busy,
  onSelect,
  onEdit,
  onOpenContext,
}: {
  joint: JointDefinitionDto;
  selected: boolean;
  busy: boolean;
  onSelect: () => void;
  onEdit: () => void;
  onOpenContext: (x: number, y: number) => void;
}) {
  const { t } = useTranslation();
  const openPointerContext = (event: MouseEvent<HTMLButtonElement>) => {
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.focus({ preventScroll: true });
    onOpenContext(event.clientX, event.clientY);
  };
  const onJointKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key !== 'ContextMenu' && !(event.shiftKey && event.key === 'F10')) return;
    event.preventDefault();
    const rect = event.currentTarget.getBoundingClientRect();
    onOpenContext(rect.left + Math.min(36, rect.width / 2), rect.bottom);
  };
  return (
    <button
      type="button"
      disabled={busy}
      data-joint-id={joint.id}
      aria-haspopup="menu"
      title={`${joint.name} · ${joint.kind} — ${t('timeline.editJoint')}`}
      onClick={onSelect}
      onDoubleClick={onEdit}
      onContextMenu={openPointerContext}
      onKeyDown={onJointKeyDown}
      className={cx(
        'relative z-10 flex h-7 min-w-9 shrink-0 items-center justify-center rounded-md border px-2 text-[10px] shadow-sm shadow-black/15 transition-colors',
        selected
          ? 'border-accent bg-accent/15 text-accent'
          : 'border-edge bg-panel text-ink hover:border-accent/70 hover:bg-header',
        !joint.enabled && 'line-through opacity-50',
      )}
    >
      <Link2 size={13} />
      <span className="ml-1 max-w-20 truncate">{joint.name}</span>
    </button>
  );
}

function TimelineFeature({
  feature,
  index,
  active,
  selected,
  busy,
  reorderEnabled,
  reordering,
  onSelect,
  onBeginReorder,
  shouldSuppressClick,
  onOpenContext,
}: {
  feature: FeatureDto;
  index: number;
  active: boolean;
  selected: boolean;
  busy: boolean;
  reorderEnabled: boolean;
  reordering: boolean;
  onSelect: (event: MouseEvent<HTMLButtonElement>) => void;
  onBeginReorder: (event: ReactPointerEvent<HTMLButtonElement>) => void;
  shouldSuppressClick: () => boolean;
  onOpenContext: (x: number, y: number) => void;
}) {
  const { t } = useTranslation();
  const error = feature.status.state === 'error' ? feature.status.message : null;
  const isError = error !== null;
  const Icon = feature.kind === 'sketch'
    ? PenLine
    : feature.kind === 'revolve'
      ? RefreshCw
      : feature.kind === 'sweep'
        ? MoveRight
        : feature.kind === 'loft'
          ? Layers3
          : feature.kind === 'rib'
            ? PanelTop
            : feature.kind === 'fillet'
              ? Blend
              : feature.kind === 'chamfer'
                ? Triangle
                : feature.kind === 'hole'
                  ? CircleDot
                  : feature.kind === 'external_thread'
                    ? RefreshCw
                  : feature.kind === 'construction_plane'
                    ? Layers3
                    : feature.kind === 'move_copy'
                      ? Move3d
                    : feature.kind === 'shell'
                      ? Shell
                      : feature.kind === 'mirror'
                        ? Copy
                        : feature.kind === 'rectangular_pattern' ||
                            feature.kind === 'circular_pattern'
                          ? Grid2X2
                          : feature.kind === 'split_body'
                            ? Scissors
                            : feature.kind === 'import_step'
                              ? FileUp
                              : Box;
  const editable = canEditTimelineFeature(feature);
  const editLabel =
    feature.kind === 'sketch' ? t('timeline.editSketch') : t('timeline.editFeature');
  const openPointerContext = (event: MouseEvent<HTMLButtonElement>) => {
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.focus({ preventScroll: true });
    onOpenContext(event.clientX, event.clientY);
  };

  const onFeatureKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key !== 'ContextMenu' && !(event.shiftKey && event.key === 'F10')) return;
    event.preventDefault();
    event.stopPropagation();
    const rect = event.currentTarget.getBoundingClientRect();
    onOpenContext(rect.left + Math.min(36, rect.width / 2), rect.bottom);
  };

  return (
    <button
      type="button"
      disabled={busy}
      data-feature-id={feature.id}
      data-timeline-selected={selected ? 'true' : 'false'}
      aria-pressed={selected}
      aria-haspopup="menu"
      title={
        error
          ? `${t('timeline.featureError')}: ${error}`
          : `${feature.name}${editable ? ` — ${editLabel}` : ''}${
              reorderEnabled ? ` — ${t('timeline.dragFeature')}` : ''
            }`
      }
      aria-grabbed={reordering}
      onPointerDown={onBeginReorder}
      onClick={(event) => {
        if (shouldSuppressClick()) return;
        onSelect(event);
      }}
      onDoubleClick={() => {
        if (editable) editTimelineFeature(feature);
      }}
      onContextMenu={openPointerContext}
      onKeyDown={onFeatureKeyDown}
      className={cx(
        'relative z-10 flex h-7 min-w-9 items-center justify-center rounded-md border px-2 text-[10px] shadow-sm shadow-black/15 transition-colors',
        selected
          ? 'border-accent bg-accent/15 text-accent ring-1 ring-accent/50'
          : active
            ? 'border-edge bg-panel text-ink hover:border-accent/70 hover:bg-header'
            : 'border-edge/40 bg-panel/80 text-mute/45',
        feature.suppressed && 'line-through opacity-50',
        isError && 'border-red-500/70 bg-red-500/10 text-red-300',
        reorderEnabled && 'touch-none cursor-grab active:cursor-grabbing',
        reordering && 'border-accent bg-accent/15 opacity-70',
      )}
    >
      {isError ? <CircleAlert size={13} /> : <Icon size={13} />}
      <span className="ml-1 max-w-20 truncate">{feature.name}</span>
      <span className="sr-only">{index + 1}</span>
    </button>
  );
}

function FeatureDropIndicator() {
  return (
    <div
      data-testid="timeline-reorder-indicator"
      aria-hidden="true"
      className="relative z-30 h-8 w-1 shrink-0"
    >
      <span className="absolute inset-y-0 left-1/2 w-0.5 -translate-x-1/2 rounded-full bg-accent shadow-[0_0_8px_rgb(var(--accent-rgb)/0.7)]" />
    </div>
  );
}

function HistoryCursor({
  label,
  hint,
  value,
  max,
  busy,
  dragging,
  onPointerDown,
  onMove,
}: {
  label: string;
  hint: string;
  value: number;
  max: number;
  busy: boolean;
  dragging: boolean;
  onPointerDown: (event: ReactPointerEvent<HTMLButtonElement>) => void;
  onMove: (index: number) => void;
}) {
  return (
    <button
      type="button"
      role="slider"
      data-testid="timeline-history-cursor"
      title={`${label} — ${hint}`}
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={max}
      aria-valuenow={value}
      aria-valuetext={`${value} / ${max}`}
      disabled={busy}
      onPointerDown={onPointerDown}
      onKeyDown={(event) => {
        let next: number | null = null;
        if (event.key === 'ArrowLeft') next = value - 1;
        if (event.key === 'ArrowRight') next = value + 1;
        if (event.key === 'Home') next = 0;
        if (event.key === 'End') next = max;
        if (next === null) return;
        event.preventDefault();
        onMove(next);
      }}
      className={cx(
        'relative z-20 flex h-8 w-3 shrink-0 touch-none cursor-col-resize items-center justify-center rounded outline-none focus-visible:ring-2 focus-visible:ring-accent/70 disabled:cursor-default',
        dragging && 'bg-accent/15',
      )}
    >
      <span className="h-full w-px rounded-full bg-accent shadow-[0_0_7px_rgb(var(--accent-rgb)/0.5)]" />
      <span className="absolute top-0 h-1.5 w-1.5 -translate-y-0.5 rotate-45 rounded-[1px] bg-accent" />
    </button>
  );
}
