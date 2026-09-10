import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Activity,
  Anchor,
  Box,
  Boxes,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Copy,
  Download,
  Eye,
  EyeOff,
  FolderTree,
  Gauge,
  Link2,
  Move3d,
  Pencil,
  Pause,
  Play,
  Plus,
  RotateCcw,
  Save,
  ShieldAlert,
  Square,
  TimerReset,
  Trash2,
  TriangleAlert,
} from 'lucide-react';
import type {
  AssemblyTransformDto,
  ComponentDefinitionDto,
  ComponentOccurrenceDto,
  JointDefinitionDto,
  JointLimitsDto,
  JointMotionStateDto,
  ContactSetDto,
  InterferenceReportDto,
  MotionCoordinateDto,
  MotionDriverDto,
  MotionKeyframeDto,
  MotionStudyDto,
  SweptCollisionReportDto,
} from '../../engine/types';
import { getEngine } from '../../engine';
import { chooseSaveTarget, writeSaveTarget } from '../../files/fileIO';
import { useAppStore } from '../../store/appStore';
import { openBodyFeature } from '../../engine/controller';

interface MotionValues {
  angle: number;
  linear: number;
  secondaryAngle: number;
  tertiaryAngle: number;
  secondaryLinear: number;
}

export function AssemblyBrowser() {
  const assembly = useAppStore((state) => state.assemblyDocument);
  const solution = useAppStore((state) => state.assemblySolution);
  const motionPreview = useAppStore((state) => state.jointMotionPreview);
  const mechanismPreview = useAppStore((state) => state.mechanismPreview);
  const motionStudyPreview = useAppStore((state) => state.motionStudyPreview);
  const selectedJointId = useAppStore((state) => state.selectedJointId);
  const selectedOccurrenceId = useAppStore((state) => state.selectedOccurrenceId);
  const selectedBodies = useAppStore((state) => state.selectedBodies);
  const bodies = useAppStore((state) => state.solidScene.bodies);
  const setSelectedJointId = useAppStore((state) => state.setSelectedJointId);
  const setJointDialogOpen = useAppStore((state) => state.setJointDialogOpen);
  const openJointEditor = useAppStore((state) => state.openJointEditor);
  const setJointEnabled = useAppStore((state) => state.setJointEnabled);
  const setSolidSidebarMode = useAppStore((state) => state.setSolidSidebarMode);
  const createComponent = useAppStore((state) => state.createComponent);
  const updateComponent = useAppStore((state) => state.updateComponent);
  const createOccurrence = useAppStore((state) => state.createOccurrence);
  const updateOccurrence = useAppStore((state) => state.updateOccurrence);
  const duplicateOccurrence = useAppStore((state) => state.duplicateOccurrence);
  const setOccurrenceGrounded = useAppStore((state) => state.setOccurrenceGrounded);
  const setOccurrencePose = useAppStore((state) => state.setOccurrencePose);
  const setSelectedOccurrenceId = useAppStore((state) => state.setSelectedOccurrenceId);
  const replaceSelectedBodies = useAppStore((state) => state.replaceSelectedBodies);
  const previewJointCoordinates = useAppStore((state) => state.previewJointCoordinates);
  const clearJointMotionPreview = useAppStore((state) => state.clearJointMotionPreview);
  const captureJointPosition = useAppStore((state) => state.captureJointPosition);
  const clearMechanismPreview = useAppStore((state) => state.clearMechanismPreview);
  const captureMechanismPosition = useAppStore((state) => state.captureMechanismPosition);
  const liveBodyIds = useMemo(
    () => new Set(bodies.map((body) => body.id)),
    [bodies],
  );
  const activeJoints = useMemo(
    () => assembly.joints.filter((joint) =>
      liveBodyIds.has(joint.connector_a.body_id)
      && liveBodyIds.has(joint.connector_b.body_id)),
    [assembly.joints, liveBodyIds],
  );
  const selectedJoint = activeJoints.find((joint) => joint.id === selectedJointId) ?? null;
  const [motionValues, setMotionValues] = useState<MotionValues>(() => valuesForJoint(selectedJoint));
  const [demoRunning, setDemoRunning] = useState(false);
  const [panel, setPanel] = useState<'assembly' | 'motion' | 'inspect'>('assembly');
  const [componentsExpanded, setComponentsExpanded] = useState(true);
  const [jointsExpanded, setJointsExpanded] = useState(true);
  const [expandedOccurrences, setExpandedOccurrences] = useState<Set<number>>(
    () => new Set(assembly.component_structure.occurrences.map((occurrence) => occurrence.id)),
  );
  const [instanceComponentId, setInstanceComponentId] = useState<number | null>(null);
  const motionTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const demoFrame = useRef<number | null>(null);
  const knownJointIds = useRef(new Set(assembly.joints.map((joint) => joint.id)));
  const selectedMotionPreview = motionPreview?.jointId === selectedJoint?.id
    ? motionPreview
    : null;
  const visibleSolution = mechanismPreview?.solution
    ?? motionPreview?.solution
    ?? motionStudyPreview?.sample.solution
    ?? solution;
  const visibleDiagnostics = visibleSolution.diagnostics.filter(
    (diagnostic) => !(
      mechanismPreview
      && diagnostic.kind === 'kinematic_unreachable'
    ),
  );
  const structure = assembly.component_structure;
  const definitionsById = useMemo(
    () => new Map(structure.definitions.map((definition) => [definition.id, definition])),
    [structure.definitions],
  );
  const selectedOccurrence = structure.occurrences.find(
    (occurrence) => occurrence.id === selectedOccurrenceId,
  ) ?? null;
  const selectedComponent = selectedOccurrence
    ? definitionsById.get(selectedOccurrence.component_id) ?? null
    : null;

  useEffect(() => {
    if (
      instanceComponentId === null
      || !structure.definitions.some((definition) => definition.id === instanceComponentId)
    ) {
      setInstanceComponentId(selectedComponent?.id ?? structure.definitions[0]?.id ?? null);
    }
  }, [instanceComponentId, selectedComponent?.id, structure.definitions]);

  useEffect(() => {
    if (!selectedJoint) {
      setMotionValues(valuesForJoint(null));
      return;
    }
    setMotionValues(selectedMotionPreview
      ? valuesForMotion(selectedMotionPreview.motion)
      : valuesForJoint(selectedJoint));
  }, [selectedJoint, selectedMotionPreview]);

  useEffect(() => {
    if (selectedJointId !== null && !activeJoints.some((joint) => joint.id === selectedJointId)) {
      setSelectedJointId(null);
    }
  }, [activeJoints, selectedJointId, setSelectedJointId]);

  useEffect(() => () => {
    if (motionTimer.current) clearTimeout(motionTimer.current);
    if (demoFrame.current !== null) cancelAnimationFrame(demoFrame.current);
  }, []);

  const selectJoint = (jointId: number) => {
    const state = useAppStore.getState();
    const joint = state.assemblyDocument.joints.find((candidate) => candidate.id === jointId);
    if (!joint) return;
    state.clearSolidSelection();
    for (const connector of [joint.connector_a, joint.connector_b]) {
      if (connector.kind === 'circular_edge' && connector.edge_id) {
        state.selectSolidFeature('edge', connector.body_id, connector.edge_id, null, true);
      } else {
        state.selectSolidFeature('face', connector.body_id, connector.face_id, null, true);
      }
    }
    setSelectedJointId(jointId);
  };

  const queueMotion = (joint: JointDefinitionDto, values: MotionValues) => {
    if (demoFrame.current !== null) {
      cancelAnimationFrame(demoFrame.current);
      demoFrame.current = null;
      setDemoRunning(false);
    }
    setMotionValues(values);
    if (motionTimer.current) clearTimeout(motionTimer.current);
    motionTimer.current = setTimeout(() => {
      void previewJointCoordinates(motionForValues(joint, values)).catch(showAssemblyError);
    }, 32);
  };

  const playMotionDemo = useCallback((joint: JointDefinitionDto) => {
    if (joint.kind === 'rigid' || !joint.enabled) return;
    if (motionTimer.current) clearTimeout(motionTimer.current);
    if (demoFrame.current !== null) cancelAnimationFrame(demoFrame.current);
    setDemoRunning(true);
    clearJointMotionPreview();
    const base = valuesForJoint(joint);
    const started = performance.now();
    let lastRequest = 0;
    const frame = (now: number) => {
      const progress = Math.min(1, (now - started) / 1_800);
      if (now - lastRequest >= 33 || progress === 1) {
        lastRequest = now;
        const phase = Math.sin(progress * Math.PI * 2);
        const values = demoValues(joint, base, phase);
        setMotionValues(values);
        void previewJointCoordinates(motionForValues(joint, values)).catch(showAssemblyError);
      }
      if (progress < 1) {
        demoFrame.current = requestAnimationFrame(frame);
      } else {
        demoFrame.current = null;
        setDemoRunning(false);
        clearJointMotionPreview();
        setMotionValues(base);
      }
    };
    demoFrame.current = requestAnimationFrame(frame);
  }, [clearJointMotionPreview, previewJointCoordinates]);

  useEffect(() => {
    const added = assembly.joints.find((joint) => !knownJointIds.current.has(joint.id));
    knownJointIds.current = new Set(assembly.joints.map((joint) => joint.id));
    if (added && added.kind !== 'rigid') playMotionDemo(added);
  }, [assembly.joints, playMotionDemo]);

  const selectOccurrence = (occurrence: ComponentOccurrenceDto) => {
    const definition = definitionsById.get(occurrence.component_id);
    setSelectedJointId(null);
    setSelectedOccurrenceId(occurrence.id);
    replaceSelectedBodies(definition?.body_ids ?? []);
  };

  const createSelectedComponent = async () => {
    if (selectedBodies.length === 0) return;
    await createComponent(
      uniqueComponentName(structure.definitions, 'Component'),
      selectedBodies,
    );
  };

  const createSubassembly = async () => {
    await createComponent(
      uniqueComponentName(structure.definitions, 'Subassembly'),
      [],
    );
  };

  const addOccurrence = async (asChild: boolean) => {
    if (instanceComponentId === null) return;
    await createOccurrence(
      instanceComponentId,
      asChild ? selectedOccurrence?.id ?? null : null,
    );
  };

  return (
    <aside data-testid="assembly-browser" className="flex h-full min-h-0 w-[286px] shrink-0 flex-col overflow-hidden border-r border-edge bg-panel">
      <header className="flex h-8 items-center justify-between border-b border-edge px-2.5 text-[10px] font-semibold tracking-[0.16em] text-mute">
        <button
          type="button"
          title="Back to model browser"
          onClick={() => setSolidSidebarMode('model')}
          className="flex items-center gap-1 rounded py-1 pr-1 hover:bg-edge hover:text-ink"
        >
          <ChevronLeft size={13} /> MODEL
        </button>
        <span className="ml-auto mr-2">ASSEMBLY</span>
        <button
          type="button"
          title="Create joint"
          onClick={() => setJointDialogOpen(true)}
          className="rounded p-1 text-mute hover:bg-edge hover:text-ink"
        >
          <Plus size={15} />
        </button>
      </header>

      <nav className="grid h-8 shrink-0 grid-cols-3 border-b border-edge bg-header/60 p-0.5 text-[9px]">
        {([
          ['assembly', 'Structure'],
          ['motion', 'Motion'],
          ['inspect', 'Inspect'],
        ] as const).map(([id, label]) => (
          <button
            key={id}
            type="button"
            onClick={() => setPanel(id)}
            className={`rounded ${panel === id ? 'bg-accent/20 font-semibold text-accent' : 'text-mute hover:bg-edge hover:text-ink'}`}
          >
            {label}
          </button>
        ))}
      </nav>

      <div
        data-testid="assembly-structure-scroll"
        className={panel === 'assembly' ? 'min-h-0 flex-1 overflow-y-auto' : 'hidden'}
      >

      <section className="shrink-0 border-b border-edge">
        <button
          type="button"
          onClick={() => setComponentsExpanded((expanded) => !expanded)}
          className="flex h-8 w-full items-center gap-1.5 px-2.5 text-left text-[10px] font-semibold uppercase tracking-[0.12em] text-mute hover:bg-edge/40 hover:text-ink"
        >
          {componentsExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
          <Boxes size={13} className="text-accent" /> Components
          <span className="ml-auto rounded bg-header px-1.5 py-0.5 text-[8px] font-normal tracking-normal">
            {structure.occurrences.length}
          </span>
        </button>
        {componentsExpanded && (
          <div data-testid="assembly-component-structure" className="border-t border-edge/70">
            <div className="grid grid-cols-2 gap-1.5 p-2">
              <button
                type="button"
                disabled={selectedBodies.length === 0}
                onClick={() => void createSelectedComponent().catch(showAssemblyError)}
                title={selectedBodies.length > 0
                  ? `Group ${selectedBodies.length} selected bod${selectedBodies.length === 1 ? 'y' : 'ies'} into one reusable component`
                  : 'Select one or more model bodies first'}
                className="flex h-7 items-center justify-center gap-1 rounded border border-edge bg-header px-1.5 text-[9px] text-ink hover:border-accent disabled:cursor-not-allowed disabled:opacity-35"
              >
                <Box size={11} /> Make component
              </button>
              <button
                type="button"
                onClick={() => void createSubassembly().catch(showAssemblyError)}
                className="flex h-7 items-center justify-center gap-1 rounded border border-edge bg-header px-1.5 text-[9px] text-ink hover:border-accent"
                title="Create an empty component that can own nested occurrences"
              >
                <FolderTree size={11} /> Subassembly
              </button>
            </div>

            <div className="max-h-52 overflow-y-auto border-y border-edge/70 py-1">
              {structure.occurrences.length === 0 ? (
                <p className="px-3 py-5 text-center text-[10px] leading-4 text-mute">
                  Bodies become one-body components automatically. Select multiple bodies to make a rigid multi-body component.
                </p>
              ) : (
                <OccurrenceTree
                  parentId={null}
                  occurrences={structure.occurrences}
                  definitionsById={definitionsById}
                  selectedOccurrenceId={selectedOccurrenceId}
                  expanded={expandedOccurrences}
                  onSelect={selectOccurrence}
                  onToggleExpanded={(id) => setExpandedOccurrences((current) => {
                    const next = new Set(current);
                    if (next.has(id)) next.delete(id);
                    else next.add(id);
                    return next;
                  })}
                  onToggleVisibility={(occurrence) => void updateOccurrence({
                    ...occurrence,
                    visible: !occurrence.visible,
                  }).catch(showAssemblyError)}
                  onToggleGround={(occurrence) => void setOccurrenceGrounded(
                    occurrence.id,
                    !occurrence.grounded,
                  ).catch(showAssemblyError)}
                  onDuplicate={(occurrence) => void duplicateOccurrence(
                    occurrence.id,
                    occurrence.parent_occurrence_id,
                  ).catch(showAssemblyError)}
                  onMoveCopy={(occurrence) => {
                    setSelectedOccurrenceId(occurrence.id);
                    openBodyFeature('move_copy');
                  }}
                />
              )}
            </div>

            {structure.definitions.length > 0 && (
              <div className="grid grid-cols-[1fr_auto_auto] gap-1.5 p-2">
                <select
                  aria-label="Reusable component definition"
                  value={instanceComponentId ?? ''}
                  onChange={(event) => setInstanceComponentId(Number(event.target.value))}
                  className="h-7 min-w-0 rounded border border-edge bg-header px-1.5 text-[9px] text-ink outline-none focus:border-accent"
                >
                  {structure.definitions.map((definition) => (
                    <option key={definition.id} value={definition.id}>
                      {definition.name} · {definition.body_ids.length} bod{definition.body_ids.length === 1 ? 'y' : 'ies'}
                    </option>
                  ))}
                </select>
                <button
                  type="button"
                  onClick={() => void addOccurrence(false).catch(showAssemblyError)}
                  className="h-7 rounded border border-edge bg-header px-2 text-[9px] text-ink hover:border-accent"
                  title="Add a reusable root occurrence"
                >
                  + Root
                </button>
                <button
                  type="button"
                  disabled={!selectedOccurrence}
                  onClick={() => void addOccurrence(true).catch(showAssemblyError)}
                  className="h-7 rounded border border-edge bg-header px-2 text-[9px] text-ink hover:border-accent disabled:opacity-35"
                  title="Add this component inside the selected subassembly occurrence"
                >
                  + Child
                </button>
              </div>
            )}
          </div>
        )}
      </section>

      {!selectedJoint && selectedOccurrence && selectedComponent && (
        <OccurrenceInspector
          occurrence={selectedOccurrence}
          component={selectedComponent}
          occurrences={structure.occurrences}
          bodies={bodies}
          onUpdateOccurrence={(next) => void updateOccurrence(next).catch(showAssemblyError)}
          onSetPose={(pose) => void setOccurrencePose(selectedOccurrence.id, pose).catch(showAssemblyError)}
          onUpdateComponent={(next) => void updateComponent(next).catch(showAssemblyError)}
        />
      )}

      {mechanismPreview && (
        <section data-testid="mechanism-position-capture" className="shrink-0 border-b border-accent/40 bg-accent/10 p-2">
          <p className="text-[10px] font-semibold text-accent">Mechanism position preview</p>
          <p className="mt-0.5 text-[9px] leading-3 text-mute">
            {mechanismPreview.converged
              ? `${mechanismPreview.joint_motions.length} joint(s) solved in ${mechanismPreview.iterations} iteration(s).`
              : `Closest constrained position · ${mechanismPreview.position_error_mm.toFixed(2)} mm residual.`}
          </p>
          <div className="mt-2 flex gap-1.5">
            <button
              type="button"
              onClick={clearMechanismPreview}
              className="flex h-7 flex-1 items-center justify-center gap-1 rounded border border-edge bg-header text-[9px] text-ink hover:border-accent"
            >
              <RotateCcw size={11} /> Revert
            </button>
            <button
              type="button"
              disabled={mechanismPreview.joint_motions.length === 0}
              title={mechanismPreview.converged
                ? 'Save this solved mechanism position'
                : 'The cursor target is unreachable; save the closest valid constrained position'}
              onClick={() => void captureMechanismPosition().catch(showAssemblyError)}
              className="flex h-7 flex-1 items-center justify-center gap-1 rounded bg-accent text-[9px] font-semibold text-white hover:brightness-110 disabled:opacity-40"
            >
              <Save size={11} /> {mechanismPreview.converged ? 'Save position' : 'Save closest'}
            </button>
          </div>
        </section>
      )}

      <section className="flex flex-col">
        <button
          type="button"
          onClick={() => setJointsExpanded((expanded) => !expanded)}
          className="flex h-8 shrink-0 items-center gap-1.5 border-b border-edge px-2.5 text-left text-[10px] font-semibold uppercase tracking-[0.12em] text-mute hover:bg-edge/40 hover:text-ink"
        >
          {jointsExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
          <Link2 size={13} className="text-accent" /> Joints
          <span className="ml-auto rounded bg-header px-1.5 py-0.5 text-[8px] font-normal tracking-normal">
            {activeJoints.length}
          </span>
        </button>
        {jointsExpanded && (
          <div className="py-1">
          {activeJoints.length === 0 && (
          <div className="px-4 py-8 text-center">
            <Link2 className="mx-auto mb-2 text-mute/50" size={28} />
            <p className="text-[11px] font-medium text-ink">No joints</p>
            <p className="mt-1 text-[10px] leading-relaxed text-mute">
              Connect exact faces, cylindrical axes, or circular openings on different components.
            </p>
            <button
              type="button"
              onClick={() => setJointDialogOpen(true)}
              className="mt-3 rounded bg-accent px-3 py-1.5 text-[10px] font-semibold text-white hover:brightness-110"
            >
              Create joint
            </button>
          </div>
        )}
          {activeJoints.map((joint) => {
          const broken = jointHasBrokenReference(joint, bodies);
          return (
            <div
              key={joint.id}
              data-testid={`assembly-joint-${joint.id}`}
              className={`group flex min-h-8 items-center gap-1 px-2 ${
                selectedJointId === joint.id
                  ? 'bg-accent/20 text-ink'
                  : 'text-mute hover:bg-edge/50 hover:text-ink'
              }`}
            >
              <button
                type="button"
                onClick={() => selectJoint(joint.id)}
                onDoubleClick={() => openJointEditor(joint.id)}
                title={broken ? 'Broken topology reference — edit to repair' : joint.enabled ? undefined : 'Joint is suppressed'}
                className="flex min-w-0 flex-1 items-center gap-2 py-1 text-left"
              >
                <Link2
                  size={14}
                  className={broken ? 'text-warn' : joint.enabled ? 'text-accent' : 'opacity-35'}
                />
                <span className={`truncate text-[11px] ${joint.enabled ? '' : 'line-through opacity-60'}`}>
                  {joint.name}
                </span>
                <span className={`ml-auto text-[8px] uppercase ${broken ? 'text-warn' : 'opacity-55'}`}>
                  {broken ? 'repair' : kindShortLabel(joint)}
                </span>
              </button>
              <button
                type="button"
                title={joint.enabled ? `Suppress ${joint.name}` : `Unsuppress ${joint.name}`}
                onClick={() => void setJointEnabled(joint.id, !joint.enabled).catch(showAssemblyError)}
                className="invisible rounded p-1 text-mute hover:bg-edge hover:text-ink group-hover:visible"
              >
                {joint.enabled ? <Eye size={12} /> : <EyeOff size={12} />}
              </button>
              <button
                type="button"
                title={`Edit ${joint.name}`}
                onClick={() => openJointEditor(joint.id)}
                className="invisible rounded p-1 text-mute hover:bg-edge hover:text-ink group-hover:visible"
              >
                <Pencil size={12} />
              </button>
              <button
                type="button"
                title={`Delete ${joint.name}`}
                onClick={() => void useAppStore.getState().deleteJoint(joint.id).catch(showAssemblyError)}
                className="invisible rounded p-1 text-mute hover:bg-warn/15 hover:text-warn group-hover:visible"
              >
                <Trash2 size={12} />
              </button>
            </div>
          );
          })}
          </div>
        )}
      </section>

      {selectedJoint && (
        <section data-testid="joint-motion-panel" className="border-t border-edge p-2.5">
          <div className="flex items-center gap-2 text-[10px] font-semibold uppercase tracking-wide text-mute">
            <Gauge size={13} className="text-accent" /> Motion
          </div>
          <div className="mt-1 flex items-center gap-2">
            <p className="min-w-0 flex-1 truncate text-[11px] font-medium text-ink">{selectedJoint.name}</p>
            <button
              type="button"
              onClick={() => openJointEditor(selectedJoint.id)}
              className="rounded p-1 text-mute hover:bg-edge hover:text-ink"
              title="Edit joint definition"
            >
              <Pencil size={11} />
            </button>
          </div>
          {!selectedJoint.enabled ? (
            <p className="mt-2 text-[10px] leading-4 text-mute">This joint is suppressed and does not constrain the mechanism.</p>
          ) : selectedJoint.kind === 'rigid' ? (
            <p className="mt-2 text-[10px] leading-4 text-mute">Rigid joints have no free motion.</p>
          ) : (
            <>
              <div className="mt-2 flex items-center justify-between gap-2">
                <button
                  type="button"
                  data-testid="joint-motion-demo"
                  onClick={() => playMotionDemo(selectedJoint)}
                  className="flex items-center gap-1 rounded border border-edge bg-header px-2 py-1 text-[9px] text-ink hover:border-accent"
                >
                  <Play size={10} /> Demo motion
                </button>
                <span className="text-[9px] text-mute">Preview only</span>
              </div>
              {motionControls(selectedJoint).map((control) => (
                <MotionControl
                  key={control.key}
                  testId={`joint-motion-${control.key}`}
                  label={control.label}
                  unit={control.unit}
                  value={motionValues[control.key]}
                  minimum={control.limits?.min ?? control.fallback[0]}
                  maximum={control.limits?.max ?? control.fallback[1]}
                  step={control.unit === '°' ? 1 : 0.5}
                  onChange={(value) => queueMotion(selectedJoint, {
                    ...motionValues,
                    [control.key]: value,
                  })}
                />
              ))}
              <p className="mt-1 text-[9px] leading-3 text-mute">
                With this joint selected, drag its component to drive only this joint. Clear the joint selection and drag any movable component to solve the whole mechanism from the exact point under the cursor.
              </p>
              {selectedMotionPreview && !demoRunning && (
                <div data-testid="joint-position-capture" className="mt-2 flex gap-1.5 border-t border-edge pt-2">
                  <button
                    type="button"
                    onClick={() => {
                      clearJointMotionPreview();
                      setMotionValues(valuesForJoint(selectedJoint));
                    }}
                    className="flex h-7 flex-1 items-center justify-center gap-1 rounded border border-edge bg-header text-[9px] text-ink hover:border-accent"
                  >
                    <RotateCcw size={11} /> Revert
                  </button>
                  <button
                    type="button"
                    onClick={() => void captureJointPosition().catch(showAssemblyError)}
                    className="flex h-7 flex-1 items-center justify-center gap-1 rounded bg-accent text-[9px] font-semibold text-white hover:brightness-110"
                  >
                    <Save size={11} /> Save position
                  </button>
                </div>
              )}
            </>
          )}
        </section>
      )}

      {visibleDiagnostics.length > 0 && (
        <section data-testid="assembly-diagnostics" className="border-t border-edge p-2">
          {visibleDiagnostics.map((diagnostic, index) => (
            <div key={`${diagnostic.kind}-${diagnostic.joint_id ?? diagnostic.body_id ?? index}`} className="mb-1 flex gap-1.5 text-[9px] leading-3 text-mute last:mb-0">
              <TriangleAlert size={11} className={diagnostic.kind === 'free_component' ? 'text-mute' : 'text-warn'} />
              <span>{diagnostic.message}</span>
            </div>
          ))}
        </section>
      )}
      </div>

      {panel === 'motion' && <MotionStudioPanel />}
      {panel === 'inspect' && <InterferencePanel />}
    </aside>
  );
}

async function refreshAssemblyState(dirty = false) {
  const engine = await getEngine();
  const [assemblyDocument, assemblySolution] = await Promise.all([
    engine.assemblyDocument(),
    engine.assemblySolution(),
  ]);
  useAppStore.setState({
    assemblyDocument,
    assemblySolution,
    ...(dirty ? { dirty: true } : {}),
  });
}

function MotionStudioPanel() {
  const assembly = useAppStore((state) => state.assemblyDocument);
  const preview = useAppStore((state) => state.motionStudyPreview);
  const [selectedStudyId, setSelectedStudyId] = useState<number | null>(
    assembly.motion_studies[0]?.id ?? null,
  );
  const [time, setTime] = useState(0);
  const [playing, setPlaying] = useState(false);
  const timeRef = useRef(0);
  const frameRef = useRef<number | null>(null);
  const startedRef = useRef({ wall: 0, timeline: 0 });
  const previousTimeRef = useRef<number | null>(null);
  const requestInFlight = useRef(false);
  const pendingEvaluationRef = useRef<{ study: MotionStudyDto; time: number } | null>(null);
  const selectedStudy = assembly.motion_studies.find((study) => study.id === selectedStudyId)
    ?? assembly.motion_studies[0]
    ?? null;

  useEffect(() => {
    if (!selectedStudy && assembly.motion_studies.length > 0) {
      setSelectedStudyId(assembly.motion_studies[0].id);
    } else if (selectedStudy && selectedStudy.id !== selectedStudyId) {
      setSelectedStudyId(selectedStudy.id);
    }
  }, [assembly.motion_studies, selectedStudy, selectedStudyId]);

  useEffect(() => () => {
    if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
    useAppStore.setState({ motionStudyPreview: null });
  }, []);

  const evaluate = useCallback(async (study: MotionStudyDto, nextTime: number) => {
    if (requestInFlight.current) {
      pendingEvaluationRef.current = { study, time: nextTime };
      return;
    }
    requestInFlight.current = true;
    let failed = false;
    try {
      const engine = await getEngine();
      const evaluation = await engine.evaluateMotionStudy({
        study_id: study.id,
        time_seconds: nextTime,
        previous_time_seconds: previousTimeRef.current,
        enforce_contacts: true,
      });
      previousTimeRef.current = evaluation.sample.time_seconds;
      timeRef.current = evaluation.sample.time_seconds;
      setTime(evaluation.sample.time_seconds);
      useAppStore.setState({
        motionStudyPreview: evaluation,
        jointMotionPreview: null,
        mechanismPreview: null,
      });
      if (evaluation.stopped_by_contact !== null) setPlaying(false);
    } catch (error) {
      failed = true;
      pendingEvaluationRef.current = null;
      setPlaying(false);
      showAssemblyError(error);
    } finally {
      requestInFlight.current = false;
      const pending = pendingEvaluationRef.current;
      pendingEvaluationRef.current = null;
      if (!failed && pending) void evaluate(pending.study, pending.time);
    }
  }, []);

  useEffect(() => {
    if (!playing || !selectedStudy) return undefined;
    startedRef.current = { wall: performance.now(), timeline: timeRef.current };
    let lastRequest = 0;
    const frame = (now: number) => {
      const elapsed = ((now - startedRef.current.wall) / 1000) * selectedStudy.playback_speed;
      let next = startedRef.current.timeline + elapsed;
      if (selectedStudy.looped && next > selectedStudy.duration_seconds) {
        next %= selectedStudy.duration_seconds;
        previousTimeRef.current = 0;
        startedRef.current = { wall: now, timeline: next };
      } else if (next >= selectedStudy.duration_seconds) {
        next = selectedStudy.duration_seconds;
        setPlaying(false);
      }
      if (now - lastRequest >= 32 || next === selectedStudy.duration_seconds) {
        lastRequest = now;
        void evaluate(selectedStudy, next);
      }
      if (next < selectedStudy.duration_seconds || selectedStudy.looped) {
        frameRef.current = requestAnimationFrame(frame);
      }
    };
    frameRef.current = requestAnimationFrame(frame);
    return () => {
      if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
      frameRef.current = null;
    };
  }, [evaluate, playing, selectedStudy]);

  const createPosition = async () => {
    const engine = await getEngine();
    await engine.createAssemblyPosition({
      name: `Position ${assembly.next_position_id}`,
      motions: preview?.sample.joint_motions ?? assembly.joints.map(motionStateForJoint),
    });
    await refreshAssemblyState(true);
  };

  const applyPosition = async (id: number) => {
    const engine = await getEngine();
    await engine.applyAssemblyPosition(id);
    useAppStore.setState({ motionStudyPreview: null });
    await refreshAssemblyState(true);
  };

  const updateStudy = async (study: MotionStudyDto) => {
    const engine = await getEngine();
    await engine.updateMotionStudy(study);
    await refreshAssemblyState(true);
  };

  const addDriver = async () => {
    if (!selectedStudy) return;
    const joint = assembly.joints.find((candidate) => candidate.enabled && candidate.kind !== 'rigid');
    if (!joint) throw new Error('Create an enabled motion joint before adding a driver.');
    const coordinate = coordinateOptions(joint)[0];
    const initial = motionCoordinateValue(joint, coordinate);
    const driver: MotionDriverDto = {
      id: selectedStudy.next_driver_id,
      name: `Driver ${selectedStudy.next_driver_id}`,
      joint_id: joint.id,
      coordinate,
      enabled: true,
      law: {
        kind: 'keyframes',
        keyframes: [
          { time_seconds: 0, value: initial, interpolation: 'smooth' },
          { time_seconds: selectedStudy.duration_seconds, value: initial, interpolation: 'smooth' },
        ],
      },
    };
    await updateStudy({
      ...selectedStudy,
      drivers: [...selectedStudy.drivers, driver],
      next_driver_id: selectedStudy.next_driver_id + 1,
    });
  };

  const exportPath = async () => {
    if (!selectedStudy) return;
    const target = await chooseSaveTarget(`${selectedStudy.name}-motion-path`, {
      description: 'Motion path CSV',
      extension: '.csv',
      mime: 'text/csv',
    });
    if (!target) return;
    const engine = await getEngine();
    const csv = await engine.exportMotionPathCsv({
      study_id: selectedStudy.id,
      sample_rate_hz: 60,
      occurrence_ids: [],
    });
    await writeSaveTarget(target, new TextEncoder().encode(csv));
  };

  return (
    <div data-testid="motion-studio" className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <section className="border-b border-edge p-2.5">
        <div className="flex items-center justify-between">
          <span className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wide text-mute">
            <Save size={12} className="text-accent" /> Named positions
          </span>
          <button type="button" onClick={() => void createPosition().catch(showAssemblyError)} className="rounded bg-accent px-2 py-1 text-[9px] font-semibold text-white">
            Capture
          </button>
        </div>
        <div className="mt-2 space-y-1">
          {assembly.positions.length === 0 && <p className="py-2 text-[9px] leading-4 text-mute">Capture named positions without overwriting the active joint design state.</p>}
          {assembly.positions.map((position) => (
            <div key={position.id} className="flex items-center gap-1 rounded border border-edge bg-header p-1">
              <input
                aria-label={`Position ${position.id} name`}
                defaultValue={position.name}
                onBlur={(event) => {
                  const name = event.currentTarget.value.trim();
                  if (name && name !== position.name) {
                    void getEngine().then((engine) => engine.updateAssemblyPosition({ ...position, name }))
                      .then(() => refreshAssemblyState(true)).catch(showAssemblyError);
                  }
                }}
                className="h-6 min-w-0 flex-1 bg-transparent px-1 text-[10px] text-ink outline-none focus:ring-1 focus:ring-accent"
              />
              <button type="button" onClick={() => void applyPosition(position.id).catch(showAssemblyError)} className="rounded border border-edge px-1.5 py-1 text-[8px] text-ink hover:border-accent">Apply</button>
              <button type="button" title="Delete position" onClick={() => void getEngine().then((engine) => engine.deleteAssemblyPosition(position.id)).then(() => refreshAssemblyState(true)).catch(showAssemblyError)} className="rounded p-1 text-mute hover:text-warn"><Trash2 size={10} /></button>
            </div>
          ))}
        </div>
      </section>

      <section className="border-b border-edge p-2.5">
        <div className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wide text-mute">
          <Activity size={12} className="text-accent" /> Motion study
          <button
            type="button"
            title="New motion study"
            onClick={() => void getEngine().then((engine) => engine.createMotionStudy({ name: `Study ${assembly.next_motion_study_id}`, duration_seconds: 5 })).then(async (study) => { setSelectedStudyId(study.id); await refreshAssemblyState(true); }).catch(showAssemblyError)}
            className="ml-auto rounded p-1 hover:bg-edge hover:text-ink"
          ><Plus size={12} /></button>
        </div>
        {assembly.motion_studies.length === 0 ? (
          <button type="button" onClick={() => void getEngine().then((engine) => engine.createMotionStudy({ name: 'Study 1', duration_seconds: 5 })).then(async (study) => { setSelectedStudyId(study.id); await refreshAssemblyState(true); }).catch(showAssemblyError)} className="mt-3 w-full rounded border border-dashed border-edge py-3 text-[10px] text-mute hover:border-accent hover:text-ink">Create a motion study</button>
        ) : selectedStudy && (
          <div className="mt-2 space-y-2">
            <div className="flex gap-1">
              <select value={selectedStudy.id} onChange={(event) => { setSelectedStudyId(Number(event.target.value)); timeRef.current = 0; setTime(0); previousTimeRef.current = null; useAppStore.setState({ motionStudyPreview: null }); }} className="h-7 min-w-0 flex-1 rounded border border-edge bg-header px-2 text-[10px] text-ink">
                {assembly.motion_studies.map((study) => <option key={study.id} value={study.id}>{study.name}</option>)}
              </select>
              <button type="button" title="Delete motion study" onClick={() => void getEngine().then((engine) => engine.deleteMotionStudy(selectedStudy.id)).then(async () => { setPlaying(false); timeRef.current = 0; setTime(0); previousTimeRef.current = null; useAppStore.setState({ motionStudyPreview: null }); await refreshAssemblyState(true); }).catch(showAssemblyError)} className="h-7 w-7 rounded border border-edge text-mute hover:border-warn hover:text-warn"><Trash2 size={11} className="mx-auto" /></button>
            </div>
            <input
              aria-label="Motion study name"
              key={`${selectedStudy.id}-name-${selectedStudy.name}`}
              defaultValue={selectedStudy.name}
              onBlur={(event) => {
                const name = event.currentTarget.value.trim();
                if (name && name !== selectedStudy.name) void updateStudy({ ...selectedStudy, name }).catch(showAssemblyError);
                else event.currentTarget.value = selectedStudy.name;
              }}
              className="h-7 w-full rounded border border-edge bg-header px-2 text-[10px] text-ink"
            />
            <div className="grid grid-cols-2 gap-1.5">
              <label className="text-[8px] uppercase text-mute">Duration (s)<input key={`${selectedStudy.id}-duration-${selectedStudy.duration_seconds}`} type="number" min={0.01} step={0.1} defaultValue={selectedStudy.duration_seconds} onBlur={(event) => { const duration = Number(event.currentTarget.value); if (Number.isFinite(duration) && duration > 0 && duration !== selectedStudy.duration_seconds) void updateStudy(resizeMotionStudy(selectedStudy, duration)).catch(showAssemblyError); else event.currentTarget.value = String(selectedStudy.duration_seconds); }} className="mt-0.5 h-7 w-full rounded border border-edge bg-header px-2 text-[10px] text-ink" /></label>
              <label className="text-[8px] uppercase text-mute">Playback speed<input key={`${selectedStudy.id}-speed-${selectedStudy.playback_speed}`} type="number" min={0.05} step={0.25} defaultValue={selectedStudy.playback_speed} onBlur={(event) => { const speed = Number(event.currentTarget.value); if (Number.isFinite(speed) && speed > 0 && speed !== selectedStudy.playback_speed) void updateStudy({ ...selectedStudy, playback_speed: speed }).catch(showAssemblyError); else event.currentTarget.value = String(selectedStudy.playback_speed); }} className="mt-0.5 h-7 w-full rounded border border-edge bg-header px-2 text-[10px] text-ink" /></label>
            </div>
            <label className="flex items-center gap-1.5 text-[9px] text-mute"><input type="checkbox" checked={selectedStudy.looped} onChange={(event) => void updateStudy({ ...selectedStudy, looped: event.target.checked }).catch(showAssemblyError)} /> Loop playback</label>
            <div className="rounded border border-edge bg-header p-2">
              <div className="flex items-center gap-1.5">
                <button type="button" onClick={() => { if (timeRef.current >= selectedStudy.duration_seconds) { timeRef.current = 0; setTime(0); previousTimeRef.current = null; } setPlaying((value) => !value); }} className="flex h-7 w-7 items-center justify-center rounded bg-accent text-white">{playing ? <Pause size={12} /> : <Play size={12} />}</button>
                <button type="button" onClick={() => { setPlaying(false); timeRef.current = 0; setTime(0); previousTimeRef.current = null; useAppStore.setState({ motionStudyPreview: null }); }} className="flex h-7 w-7 items-center justify-center rounded border border-edge text-mute hover:text-ink"><Square size={10} /></button>
                <input type="range" min={0} max={selectedStudy.duration_seconds} step={0.001} value={time} onChange={(event) => { const next = Number(event.target.value); setPlaying(false); timeRef.current = next; setTime(next); void evaluate(selectedStudy, next); }} className="min-w-0 flex-1 accent-[var(--accent)]" />
                <span className="w-12 text-right font-mono text-[9px] text-ink">{time.toFixed(2)}s</span>
              </div>
              {preview?.stopped_by_contact != null && <p className="mt-1 text-[8px] text-warn">Stopped by contact set {preview.stopped_by_contact} at {preview.stop_time_seconds?.toFixed(4)} s</p>}
            </div>
            <div className="flex gap-1.5">
              <button type="button" onClick={() => void addDriver().catch(showAssemblyError)} className="flex h-7 flex-1 items-center justify-center gap-1 rounded border border-edge bg-header text-[9px] text-ink hover:border-accent"><Plus size={10} /> Driver</button>
              <button type="button" onClick={() => void exportPath().catch(showAssemblyError)} className="flex h-7 flex-1 items-center justify-center gap-1 rounded border border-edge bg-header text-[9px] text-ink hover:border-accent"><Download size={10} /> Path CSV</button>
            </div>
          </div>
        )}
      </section>

      {selectedStudy && (
        <section className="space-y-2 p-2.5">
          {selectedStudy.drivers.map((driver) => (
            <MotionDriverEditor
              key={driver.id}
              driver={driver}
              study={selectedStudy}
              joints={assembly.joints}
              onChange={(next) => void updateStudy({ ...selectedStudy, drivers: selectedStudy.drivers.map((candidate) => candidate.id === next.id ? next : candidate) }).catch(showAssemblyError)}
              onDelete={() => void updateStudy({ ...selectedStudy, drivers: selectedStudy.drivers.filter((candidate) => candidate.id !== driver.id) }).catch(showAssemblyError)}
            />
          ))}
        </section>
      )}
    </div>
  );
}

function MotionDriverEditor({ driver, study, joints, onChange, onDelete }: {
  driver: MotionDriverDto;
  study: MotionStudyDto;
  joints: JointDefinitionDto[];
  onChange: (driver: MotionDriverDto) => void;
  onDelete: () => void;
}) {
  const joint = joints.find((candidate) => candidate.id === driver.joint_id) ?? null;
  const options = joint ? coordinateOptions(joint) : [];
  return (
    <div className="rounded border border-edge bg-header p-2">
      <div className="flex items-center gap-1">
        <input key={`${driver.id}-name-${driver.name}`} defaultValue={driver.name} onBlur={(event) => { const name = event.currentTarget.value.trim(); if (name && name !== driver.name) onChange({ ...driver, name }); else event.currentTarget.value = driver.name; }} className="h-6 min-w-0 flex-1 bg-transparent text-[10px] font-medium text-ink outline-none" />
        <label className="text-[8px] text-mute"><input type="checkbox" checked={driver.enabled} onChange={(event) => onChange({ ...driver, enabled: event.target.checked })} /> on</label>
        <button type="button" onClick={onDelete} className="p-1 text-mute hover:text-warn"><Trash2 size={10} /></button>
      </div>
      <div className="mt-1 grid grid-cols-2 gap-1">
        <select value={driver.joint_id} onChange={(event) => {
          const nextJoint = joints.find((candidate) => candidate.id === Number(event.target.value));
          if (nextJoint) onChange({ ...driver, joint_id: nextJoint.id, coordinate: coordinateOptions(nextJoint)[0] });
        }} className="h-7 rounded border border-edge bg-panel px-1 text-[9px] text-ink">
          {joints.filter((candidate) => candidate.kind !== 'rigid').map((candidate) => <option key={candidate.id} value={candidate.id}>{candidate.name}</option>)}
        </select>
        <select value={driver.coordinate} onChange={(event) => onChange({ ...driver, coordinate: event.target.value as MotionCoordinateDto })} className="h-7 rounded border border-edge bg-panel px-1 text-[9px] text-ink">
          {options.map((coordinate) => <option key={coordinate} value={coordinate}>{coordinateLabel(coordinate)}</option>)}
        </select>
      </div>
      <div className="mt-1 flex rounded border border-edge p-0.5 text-[8px]">
        <button type="button" onClick={() => onChange({ ...driver, law: { kind: 'keyframes', keyframes: [{ time_seconds: 0, value: 0, interpolation: 'smooth' }, { time_seconds: study.duration_seconds, value: 0, interpolation: 'smooth' }] } })} className={`flex-1 rounded py-1 ${driver.law.kind === 'keyframes' ? 'bg-accent/20 text-accent' : 'text-mute'}`}>Keyframes</button>
        <button type="button" onClick={() => onChange({ ...driver, law: { kind: 'motor', initial_value: 0, velocity_per_second: 1, acceleration_per_second2: 0 } })} className={`flex-1 rounded py-1 ${driver.law.kind === 'motor' ? 'bg-accent/20 text-accent' : 'text-mute'}`}>Motor</button>
      </div>
      {driver.law.kind === 'motor' ? (
        <div className="mt-1 grid grid-cols-3 gap-1">
          {([
            ['Start', 'initial_value'],
            ['Speed/s', 'velocity_per_second'],
            ['Accel/s²', 'acceleration_per_second2'],
          ] as const).map(([label, key]) => <label key={key} className="text-[7px] uppercase text-mute">{label}<input key={`${driver.id}-${key}-${driver.law.kind === 'motor' ? driver.law[key] : 0}`} type="number" defaultValue={driver.law.kind === 'motor' ? driver.law[key] : 0} onBlur={(event) => { const value = Number(event.currentTarget.value); if (driver.law.kind === 'motor' && Number.isFinite(value) && value !== driver.law[key]) onChange({ ...driver, law: { ...driver.law, [key]: value } }); else if (driver.law.kind === 'motor') event.currentTarget.value = String(driver.law[key]); }} className="mt-0.5 h-6 w-full rounded border border-edge bg-panel px-1 text-[8px] text-ink" /></label>)}
        </div>
      ) : (
        <div className="mt-1 space-y-1">
          {driver.law.keyframes.map((keyframe, index) => (
            <div key={`${driver.id}-${index}-${keyframe.time_seconds}-${keyframe.value}-${keyframe.interpolation}`} className="grid grid-cols-[0.8fr_0.8fr_1fr_auto] gap-1">
              <input aria-label="Keyframe time" type="number" min={0} max={study.duration_seconds} step={0.1} defaultValue={keyframe.time_seconds} onBlur={(event) => { const value = Number(event.currentTarget.value); if (driver.law.kind === 'keyframes' && Number.isFinite(value) && value >= 0 && value <= study.duration_seconds && value !== keyframe.time_seconds) onChange({ ...driver, law: { ...driver.law, keyframes: driver.law.keyframes.map((candidate, candidateIndex) => candidateIndex === index ? { ...candidate, time_seconds: value } : candidate).sort((a, b) => a.time_seconds - b.time_seconds) } }); else event.currentTarget.value = String(keyframe.time_seconds); }} className="h-6 rounded border border-edge bg-panel px-1 text-[8px] text-ink" />
              <input aria-label="Keyframe value" type="number" step={0.5} defaultValue={keyframe.value} onBlur={(event) => { const value = Number(event.currentTarget.value); if (driver.law.kind === 'keyframes' && Number.isFinite(value) && value !== keyframe.value) onChange({ ...driver, law: { ...driver.law, keyframes: driver.law.keyframes.map((candidate, candidateIndex) => candidateIndex === index ? { ...candidate, value } : candidate) } }); else event.currentTarget.value = String(keyframe.value); }} className="h-6 rounded border border-edge bg-panel px-1 text-[8px] text-ink" />
              <select aria-label="Keyframe interpolation" value={keyframe.interpolation} onChange={(event) => driver.law.kind === 'keyframes' && onChange({ ...driver, law: { ...driver.law, keyframes: driver.law.keyframes.map((candidate, candidateIndex) => candidateIndex === index ? { ...candidate, interpolation: event.target.value as 'step' | 'linear' | 'smooth' } : candidate) } })} className="h-6 rounded border border-edge bg-panel px-1 text-[8px] text-ink"><option value="step">Step</option><option value="linear">Linear</option><option value="smooth">Smooth</option></select>
              <button type="button" disabled={driver.law.kind !== 'keyframes' || driver.law.keyframes.length <= 1} onClick={() => driver.law.kind === 'keyframes' && onChange({ ...driver, law: { ...driver.law, keyframes: driver.law.keyframes.filter((_, candidateIndex) => candidateIndex !== index) } })} className="p-1 text-mute hover:text-warn disabled:opacity-25"><Trash2 size={9} /></button>
            </div>
          ))}
          <button type="button" onClick={() => driver.law.kind === 'keyframes' && onChange({ ...driver, law: { ...driver.law, keyframes: addMotionKeyframe(driver.law.keyframes, study.duration_seconds) } })} className="w-full rounded border border-dashed border-edge py-1 text-[8px] text-mute hover:border-accent">+ Keyframe</button>
        </div>
      )}
    </div>
  );
}

function InterferencePanel() {
  const assembly = useAppStore((state) => state.assemblyDocument);
  const solution = useAppStore((state) => state.motionStudyPreview?.sample.solution ?? state.assemblySolution);
  const [threshold, setThreshold] = useState(0);
  const [sampleRate, setSampleRate] = useState(120);
  const [stopAtFirst, setStopAtFirst] = useState(false);
  const [sweptStudyId, setSweptStudyId] = useState<number | null>(
    assembly.motion_studies[0]?.id ?? null,
  );
  const [report, setReport] = useState<InterferenceReportDto | null>(null);
  const [swept, setSwept] = useState<SweptCollisionReportDto | null>(null);
  const placed = solution.instance_body_poses.filter((pose) => pose.visible);
  const [firstKey, setFirstKey] = useState('');
  const [secondKey, setSecondKey] = useState('');
  const placedKey = (occurrence: number, body: number) => `${occurrence}:${body}`;

  useEffect(() => {
    const keys = placed.map((pose) => placedKey(pose.occurrence_id, pose.body_id));
    if (!keys.includes(firstKey)) setFirstKey(keys[0] ?? '');
    if (!keys.includes(secondKey) || secondKey === (keys[0] ?? '')) setSecondKey(keys[1] ?? '');
  }, [firstKey, placed, secondKey]);

  useEffect(() => {
    if (!assembly.motion_studies.some((study) => study.id === sweptStudyId)) {
      setSweptStudyId(assembly.motion_studies[0]?.id ?? null);
    }
  }, [assembly.motion_studies, sweptStudyId]);

  const check = async () => {
    const engine = await getEngine();
    setReport(await engine.interferenceCheck({ occurrence_ids: [], clearance_threshold_mm: threshold }));
  };
  const runSwept = async () => {
    const study = assembly.motion_studies.find((candidate) => candidate.id === sweptStudyId);
    if (!study) throw new Error('Create a motion study before running swept collision.');
    const engine = await getEngine();
    setSwept(await engine.sweptCollisionCheck({
      study_id: study.id,
      sample_rate_hz: sampleRate,
      clearance_threshold_mm: threshold,
      stop_at_first: stopAtFirst,
    }));
  };
  const createContact = async () => {
    const [occurrenceA, bodyA] = firstKey.split(':').map(Number);
    const [occurrenceB, bodyB] = secondKey.split(':').map(Number);
    if (!occurrenceA || !bodyA || !occurrenceB || !bodyB) throw new Error('Choose two placed bodies.');
    const engine = await getEngine();
    await engine.createContactSet({
      name: `Contact ${assembly.next_contact_set_id}`,
      occurrence_a: occurrenceA,
      body_a: bodyA,
      occurrence_b: occurrenceB,
      body_b: bodyB,
      clearance_mm: threshold,
      stop_motion: true,
    });
    await refreshAssemblyState(true);
  };
  const updateContact = async (contact: ContactSetDto) => {
    const engine = await getEngine();
    await engine.updateContactSet(contact);
    await refreshAssemblyState(true);
  };

  return (
    <div data-testid="interference-panel" data-interface-group="assembly/inspect" className="min-h-0 flex-1 overflow-y-auto p-2.5">
      <div className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wide text-mute"><ShieldAlert size={12} className="text-accent" /> Interference & clearance</div>
      <div className="mt-2 grid grid-cols-2 gap-1.5">
        <label className="block text-[8px] uppercase text-mute">Clearance (mm)<input type="number" min={0} step={0.1} value={threshold} onChange={(event) => setThreshold(Number(event.target.value))} className="mt-0.5 h-7 w-full rounded border border-edge bg-header px-2 text-[10px] text-ink" /></label>
        <label className="block text-[8px] uppercase text-mute">Samples / second<input type="number" min={1} max={240} step={1} value={sampleRate} onChange={(event) => setSampleRate(Math.max(1, Math.min(240, Number(event.target.value))))} className="mt-0.5 h-7 w-full rounded border border-edge bg-header px-2 text-[10px] text-ink" /></label>
      </div>
      {assembly.motion_studies.length > 0 && <div className="mt-1.5 flex items-center gap-1.5"><select aria-label="Swept collision motion study" value={sweptStudyId ?? ''} onChange={(event) => setSweptStudyId(Number(event.target.value))} className="h-7 min-w-0 flex-1 rounded border border-edge bg-header px-2 text-[9px] text-ink">{assembly.motion_studies.map((study) => <option key={study.id} value={study.id}>{study.name}</option>)}</select><label className="whitespace-nowrap text-[8px] text-mute"><input type="checkbox" checked={stopAtFirst} onChange={(event) => setStopAtFirst(event.target.checked)} /> stop at first</label></div>}
      <div className="mt-2 grid grid-cols-2 gap-1.5">
        <button type="button" onClick={() => void check().catch(showAssemblyError)} className="h-7 rounded bg-accent text-[9px] font-semibold text-white">Check current</button>
        <button type="button" onClick={() => void runSwept().catch(showAssemblyError)} className="h-7 rounded border border-edge bg-header text-[9px] text-ink hover:border-accent">Swept study</button>
      </div>
      {report && (
        <div className="mt-2 rounded border border-edge bg-header p-2 text-[9px]">
          <div className="flex justify-between"><span className="font-semibold text-ink">Static result</span><span className={report.exact ? 'text-accent' : 'text-warn'}>{report.exact ? 'Exact OCCT' : 'Mesh fallback'}</span></div>
          {report.pairs.filter((pair) => pair.interfering || pair.below_clearance).length === 0 ? <p className="mt-1 text-mute">No interference or clearance violations.</p> : report.pairs.filter((pair) => pair.interfering || pair.below_clearance).slice(0, 20).map((pair) => <p key={`${pair.occurrence_a}-${pair.body_a}-${pair.occurrence_b}-${pair.body_b}`} className="mt-1 leading-3 text-warn">O{pair.occurrence_a}/B{pair.body_a} ↔ O{pair.occurrence_b}/B{pair.body_b}: {pair.interfering ? `${pair.overlap_volume_mm3.toFixed(3)} mm³ overlap` : `${pair.minimum_clearance_mm.toFixed(3)} mm clearance`}</p>)}
        </div>
      )}
      {swept && <div className="mt-2 rounded border border-edge bg-header p-2 text-[9px]"><div className="flex justify-between"><span className="font-semibold text-ink">Swept result</span><span className={swept.exact ? 'text-accent' : 'text-warn'}>{swept.sample_count} {swept.exact ? 'exact B-rep' : 'mesh fallback'} samples</span></div><p className="mt-1 text-mute">{swept.events.length === 0 ? 'No swept collisions.' : `${swept.events.length} pair(s) collide; first at ${Math.min(...swept.events.map((event) => event.first_time_seconds)).toFixed(4)} s.`}</p>{swept.events.slice(0, 8).map((event) => <p key={`${event.occurrence_a}-${event.body_a}-${event.occurrence_b}-${event.body_b}`} className="mt-1 text-[8px] text-warn">O{event.occurrence_a}/B{event.body_a} ↔ O{event.occurrence_b}/B{event.body_b}: {event.first_time_seconds.toFixed(4)}–{event.last_time_seconds.toFixed(4)} s</p>)}</div>}

      <div className="mt-4 flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wide text-mute"><TimerReset size={12} className="text-accent" /> Contact stops</div>
      <div className="mt-2 space-y-1">
        <select value={firstKey} onChange={(event) => setFirstKey(event.target.value)} className="h-7 w-full rounded border border-edge bg-header px-1 text-[9px] text-ink">{placed.map((pose) => <option key={placedKey(pose.occurrence_id, pose.body_id)} value={placedKey(pose.occurrence_id, pose.body_id)}>Occurrence {pose.occurrence_id} · Body {pose.body_id}</option>)}</select>
        <select value={secondKey} onChange={(event) => setSecondKey(event.target.value)} className="h-7 w-full rounded border border-edge bg-header px-1 text-[9px] text-ink">{placed.map((pose) => <option key={placedKey(pose.occurrence_id, pose.body_id)} value={placedKey(pose.occurrence_id, pose.body_id)}>Occurrence {pose.occurrence_id} · Body {pose.body_id}</option>)}</select>
        <button type="button" disabled={!firstKey || !secondKey || firstKey === secondKey} onClick={() => void createContact().catch(showAssemblyError)} className="h-7 w-full rounded border border-edge bg-header text-[9px] text-ink hover:border-accent disabled:opacity-35">Create physical stop</button>
      </div>
      <div className="mt-2 space-y-1.5">
        {assembly.contact_sets.map((contact) => (
          <div key={contact.id} className="rounded border border-edge bg-header p-2 text-[9px]">
            <div className="flex items-center gap-1"><input key={`${contact.id}-name-${contact.name}`} defaultValue={contact.name} onBlur={(event) => { const name = event.currentTarget.value.trim(); if (name && name !== contact.name) void updateContact({ ...contact, name }).catch(showAssemblyError); else event.currentTarget.value = contact.name; }} className="min-w-0 flex-1 bg-transparent font-medium text-ink outline-none" /><button type="button" onClick={() => void getEngine().then((engine) => engine.deleteContactSet(contact.id)).then(() => refreshAssemblyState(true)).catch(showAssemblyError)} className="text-mute hover:text-warn"><Trash2 size={10} /></button></div>
            <p className="mt-1 text-[8px] text-mute">O{contact.occurrence_a}/B{contact.body_a} ↔ O{contact.occurrence_b}/B{contact.body_b}</p>
            <div className="mt-1 flex items-center gap-2"><label className="text-[8px] text-mute"><input type="checkbox" checked={contact.enabled} onChange={(event) => void updateContact({ ...contact, enabled: event.target.checked }).catch(showAssemblyError)} /> enabled</label><label className="text-[8px] text-mute"><input type="checkbox" checked={contact.stop_motion} onChange={(event) => void updateContact({ ...contact, stop_motion: event.target.checked }).catch(showAssemblyError)} /> stop</label><input aria-label="Contact clearance" key={`${contact.id}-clearance-${contact.clearance_mm}`} type="number" min={0} step={0.1} defaultValue={contact.clearance_mm} onBlur={(event) => { const clearance = Number(event.currentTarget.value); if (Number.isFinite(clearance) && clearance >= 0 && clearance !== contact.clearance_mm) void updateContact({ ...contact, clearance_mm: clearance }).catch(showAssemblyError); else event.currentTarget.value = String(contact.clearance_mm); }} className="ml-auto h-6 w-16 rounded border border-edge bg-panel px-1 text-[8px] text-ink" /></div>
          </div>
        ))}
      </div>
    </div>
  );
}

function coordinateOptions(joint: JointDefinitionDto): MotionCoordinateDto[] {
  switch (joint.kind) {
    case 'revolute': case 'screw': return ['primary_angle'];
    case 'slider': return ['primary_linear'];
    case 'cylindrical': case 'pin_slot': return ['primary_angle', 'primary_linear'];
    case 'planar': return ['primary_angle', 'primary_linear', 'secondary_linear'];
    case 'ball': return ['primary_angle', 'secondary_angle', 'tertiary_angle'];
    case 'universal': return ['primary_angle', 'secondary_angle'];
    default: return [];
  }
}

function resizeMotionStudy(study: MotionStudyDto, durationSeconds: number): MotionStudyDto {
  const scale = durationSeconds / study.duration_seconds;
  return {
    ...study,
    duration_seconds: durationSeconds,
    drivers: study.drivers.map((driver) => driver.law.kind === 'keyframes' ? {
      ...driver,
      law: {
        ...driver.law,
        keyframes: driver.law.keyframes.map((keyframe) => ({
          ...keyframe,
          time_seconds: Math.min(durationSeconds, keyframe.time_seconds * scale),
        })),
      },
    } : driver),
  };
}

function addMotionKeyframe(
  keyframes: MotionKeyframeDto[],
  durationSeconds: number,
): MotionKeyframeDto[] {
  const sorted = [...keyframes].sort((left, right) => left.time_seconds - right.time_seconds);
  if (sorted.length === 0) {
    return [{ time_seconds: 0, value: 0, interpolation: 'smooth' }];
  }
  const boundaries = [
    { time_seconds: 0, value: sorted[0].value },
    ...sorted,
    { time_seconds: durationSeconds, value: sorted[sorted.length - 1]?.value ?? 0 },
  ];
  let bestIndex = 0;
  let bestGap = -1;
  for (let index = 0; index < boundaries.length - 1; index += 1) {
    const gap = boundaries[index + 1].time_seconds - boundaries[index].time_seconds;
    if (gap > bestGap) {
      bestGap = gap;
      bestIndex = index;
    }
  }
  if (bestGap <= 1e-9) return sorted;
  const left = boundaries[bestIndex];
  const right = boundaries[bestIndex + 1];
  return [...sorted, {
    time_seconds: (left.time_seconds + right.time_seconds) / 2,
    value: (left.value + right.value) / 2,
    interpolation: 'smooth' as const,
  }].sort((a, b) => a.time_seconds - b.time_seconds);
}

function coordinateLabel(coordinate: MotionCoordinateDto): string {
  return ({
    primary_angle: 'Primary angle', secondary_angle: 'Secondary angle', tertiary_angle: 'Tertiary angle',
    primary_linear: 'Primary distance', secondary_linear: 'Secondary distance',
  } as const)[coordinate];
}

function motionCoordinateValue(joint: JointDefinitionDto, coordinate: MotionCoordinateDto): number {
  switch (coordinate) {
    case 'primary_angle': return joint.angle_offset_deg;
    case 'secondary_angle': return joint.advanced.secondary_angle_offset_deg;
    case 'tertiary_angle': return joint.advanced.tertiary_angle_offset_deg;
    case 'primary_linear': return joint.linear_offset_mm;
    case 'secondary_linear': return joint.advanced.secondary_linear_offset_mm;
  }
}

function motionStateForJoint(joint: JointDefinitionDto): JointMotionStateDto {
  return {
    joint_id: joint.id,
    angle_offset_deg: joint.angle_offset_deg,
    linear_offset_mm: joint.linear_offset_mm,
    secondary_angle_offset_deg: joint.advanced.secondary_angle_offset_deg,
    tertiary_angle_offset_deg: joint.advanced.tertiary_angle_offset_deg,
    secondary_linear_offset_mm: joint.advanced.secondary_linear_offset_mm,
  };
}

function OccurrenceTree({
  parentId,
  occurrences,
  definitionsById,
  selectedOccurrenceId,
  expanded,
  depth = 0,
  onSelect,
  onToggleExpanded,
  onToggleVisibility,
  onToggleGround,
  onDuplicate,
  onMoveCopy,
}: {
  parentId: number | null;
  occurrences: ComponentOccurrenceDto[];
  definitionsById: Map<number, ComponentDefinitionDto>;
  selectedOccurrenceId: number | null;
  expanded: Set<number>;
  depth?: number;
  onSelect: (occurrence: ComponentOccurrenceDto) => void;
  onToggleExpanded: (occurrenceId: number) => void;
  onToggleVisibility: (occurrence: ComponentOccurrenceDto) => void;
  onToggleGround: (occurrence: ComponentOccurrenceDto) => void;
  onDuplicate: (occurrence: ComponentOccurrenceDto) => void;
  onMoveCopy: (occurrence: ComponentOccurrenceDto) => void;
}) {
  const siblings = occurrences
    .filter((occurrence) => occurrence.parent_occurrence_id === parentId)
    .sort((a, b) => a.id - b.id);
  return (
    <>
      {siblings.map((occurrence) => {
        const children = occurrences.filter(
          (candidate) => candidate.parent_occurrence_id === occurrence.id,
        );
        const definition = definitionsById.get(occurrence.component_id);
        const isExpanded = expanded.has(occurrence.id);
        const isSelected = selectedOccurrenceId === occurrence.id;
        return (
          <div key={occurrence.id}>
            <div
              data-testid={`component-occurrence-${occurrence.id}`}
              className={`group flex h-8 items-center pr-1 ${
                isSelected
                  ? 'bg-accent/20 text-ink'
                  : 'text-mute hover:bg-edge/50 hover:text-ink'
              }`}
              style={{ paddingLeft: `${4 + depth * 13}px` }}
            >
              <button
                type="button"
                title="Move or create a linked copy of this component occurrence"
                onClick={() => onMoveCopy(occurrence)}
                className="invisible rounded p-1 text-mute hover:bg-edge hover:text-accent group-hover:visible"
              >
                <Move3d size={11} />
              </button>
              <button
                type="button"
                aria-label={isExpanded ? 'Collapse occurrence' : 'Expand occurrence'}
                disabled={children.length === 0}
                onClick={() => onToggleExpanded(occurrence.id)}
                className="flex h-6 w-5 shrink-0 items-center justify-center rounded hover:bg-edge disabled:opacity-25"
              >
                {isExpanded ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
              </button>
              <button
                type="button"
                onClick={() => onSelect(occurrence)}
                className="flex min-w-0 flex-1 items-center gap-1.5 py-1 text-left"
                title={`${definition?.name ?? 'Missing definition'} · occurrence ${occurrence.id}`}
              >
                {children.length > 0 || (definition?.body_ids.length ?? 0) === 0
                  ? <FolderTree size={13} className="shrink-0 text-accent" />
                  : <Box size={13} className="shrink-0 text-accent" />}
                <span className="truncate text-[10px]">{occurrence.name}</span>
                {occurrence.grounded && <Anchor size={10} className="ml-auto shrink-0 text-accent" />}
                <span className="shrink-0 text-[8px] opacity-50">
                  {definition?.body_ids.length ?? 0}B
                </span>
              </button>
              <button
                type="button"
                title={occurrence.grounded ? 'Release this occurrence' : 'Ground this occurrence'}
                onClick={() => onToggleGround(occurrence)}
                className="invisible rounded p-1 text-mute hover:bg-edge hover:text-accent group-hover:visible"
              >
                <Anchor size={11} />
              </button>
              <button
                type="button"
                title="Duplicate this occurrence and its nested subtree"
                onClick={() => onDuplicate(occurrence)}
                className="invisible rounded p-1 text-mute hover:bg-edge hover:text-ink group-hover:visible"
              >
                <Copy size={11} />
              </button>
              <button
                type="button"
                title={occurrence.visible ? 'Hide occurrence' : 'Show occurrence'}
                onClick={() => onToggleVisibility(occurrence)}
                className="rounded p-1 text-mute hover:bg-edge hover:text-ink"
              >
                {occurrence.visible ? <Eye size={11} /> : <EyeOff size={11} />}
              </button>
            </div>
            {isExpanded && children.length > 0 && (
              <OccurrenceTree
                parentId={occurrence.id}
                occurrences={occurrences}
                definitionsById={definitionsById}
                selectedOccurrenceId={selectedOccurrenceId}
                expanded={expanded}
                depth={depth + 1}
                onSelect={onSelect}
                onToggleExpanded={onToggleExpanded}
                onToggleVisibility={onToggleVisibility}
                onToggleGround={onToggleGround}
                onDuplicate={onDuplicate}
                onMoveCopy={onMoveCopy}
              />
            )}
          </div>
        );
      })}
    </>
  );
}

interface TransformDraft {
  translation: [number, number, number];
  rotationDeg: [number, number, number];
}

function OccurrenceInspector({
  occurrence,
  component,
  occurrences,
  bodies,
  onUpdateOccurrence,
  onSetPose,
  onUpdateComponent,
}: {
  occurrence: ComponentOccurrenceDto;
  component: ComponentDefinitionDto;
  occurrences: ComponentOccurrenceDto[];
  bodies: ReturnType<typeof useAppStore.getState>['solidScene']['bodies'];
  onUpdateOccurrence: (occurrence: ComponentOccurrenceDto) => void;
  onSetPose: (pose: AssemblyTransformDto) => void;
  onUpdateComponent: (component: ComponentDefinitionDto) => void;
}) {
  const [expanded, setExpanded] = useState(true);
  const [name, setName] = useState(occurrence.name);
  const [componentName, setComponentName] = useState(component.name);
  const [pose, setPose] = useState<TransformDraft>(() => transformDraft(occurrence.local_pose));
  const [coordinateSystem, setCoordinateSystem] = useState<TransformDraft>(
    () => transformDraft(component.local_coordinate_system),
  );

  useEffect(() => {
    setName(occurrence.name);
    setPose(transformDraft(occurrence.local_pose));
  }, [occurrence]);

  useEffect(() => {
    setComponentName(component.name);
    setCoordinateSystem(transformDraft(component.local_coordinate_system));
  }, [component]);

  const excludedParents = descendantOccurrenceIds(occurrences, occurrence.id);
  excludedParents.add(occurrence.id);
  const bodyNames = component.body_ids.map((bodyId) => (
    bodies.find((body) => body.id === bodyId)?.name ?? `Body ${bodyId}`
  ));

  return (
    <section data-testid="component-occurrence-inspector" className="shrink-0 border-b border-edge bg-header/35">
      <button
        type="button"
        onClick={() => setExpanded((value) => !value)}
        className="flex h-8 w-full items-center gap-1.5 px-2.5 text-left text-[10px] font-semibold uppercase tracking-[0.12em] text-mute hover:bg-edge/40 hover:text-ink"
      >
        {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <Box size={13} className="text-accent" /> Selected occurrence
      </button>
      {expanded && (
        <div className="max-h-[42vh] overflow-y-auto border-t border-edge/70 p-2">
          <label className="block text-[8px] font-semibold uppercase tracking-wide text-mute">
            Occurrence name
            <span className="mt-1 flex gap-1">
              <input
                value={name}
                onChange={(event) => setName(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter' && name.trim()) {
                    onUpdateOccurrence({ ...occurrence, name: name.trim() });
                  }
                }}
                className="h-7 min-w-0 flex-1 rounded border border-edge bg-panel px-1.5 text-[10px] font-normal normal-case tracking-normal text-ink outline-none focus:border-accent"
              />
              <button
                type="button"
                disabled={!name.trim() || name.trim() === occurrence.name}
                onClick={() => onUpdateOccurrence({ ...occurrence, name: name.trim() })}
                className="rounded border border-edge bg-panel px-2 text-[9px] font-normal normal-case tracking-normal text-ink hover:border-accent disabled:opacity-30"
              >
                Rename
              </button>
            </span>
          </label>

          <label className="mt-2 block text-[8px] font-semibold uppercase tracking-wide text-mute">
            Parent coordinate system
            <select
              value={occurrence.parent_occurrence_id ?? ''}
              onChange={(event) => onUpdateOccurrence({
                ...occurrence,
                parent_occurrence_id: event.target.value === '' ? null : Number(event.target.value),
              })}
              className="mt-1 h-7 w-full rounded border border-edge bg-panel px-1.5 text-[10px] font-normal normal-case tracking-normal text-ink outline-none focus:border-accent"
            >
              <option value="">Document root</option>
              {occurrences
                .filter((candidate) => !excludedParents.has(candidate.id))
                .map((candidate) => (
                  <option key={candidate.id} value={candidate.id}>{candidate.name}</option>
                ))}
            </select>
          </label>

          <TransformEditor
            title="Occurrence placement"
            value={pose}
            onChange={setPose}
            onApply={() => onSetPose(transformFromDraft(pose))}
            applyLabel="Apply placement"
          />
          <p className="mt-1 text-[8px] leading-3 text-mute">
            This transform is parent-local assembly placement. It never edits the part feature history.
          </p>

          <div className="my-2 border-t border-edge" />
          <label className="block text-[8px] font-semibold uppercase tracking-wide text-mute">
            Reusable definition
            <span className="mt-1 flex gap-1">
              <input
                value={componentName}
                onChange={(event) => setComponentName(event.target.value)}
                className="h-7 min-w-0 flex-1 rounded border border-edge bg-panel px-1.5 text-[10px] font-normal normal-case tracking-normal text-ink outline-none focus:border-accent"
              />
              <button
                type="button"
                disabled={!componentName.trim() || componentName.trim() === component.name}
                onClick={() => onUpdateComponent({ ...component, name: componentName.trim() })}
                className="rounded border border-edge bg-panel px-2 text-[9px] font-normal normal-case tracking-normal text-ink hover:border-accent disabled:opacity-30"
              >
                Rename
              </button>
            </span>
          </label>
          <p className="mt-1 truncate text-[9px] text-mute" title={bodyNames.join(', ')}>
            {bodyNames.length > 0 ? bodyNames.join(', ') : 'Subassembly container · no direct bodies'}
          </p>
          <TransformEditor
            title="Component local coordinate system"
            value={coordinateSystem}
            onChange={setCoordinateSystem}
            onApply={() => onUpdateComponent({
              ...component,
              local_coordinate_system: transformFromDraft(coordinateSystem),
            })}
            applyLabel="Apply component origin"
          />
          <p className="mt-1 text-[8px] leading-3 text-mute">
            All occurrences share this definition origin; each occurrence keeps its own placement.
          </p>
        </div>
      )}
    </section>
  );
}

function TransformEditor({
  title,
  value,
  onChange,
  onApply,
  applyLabel,
}: {
  title: string;
  value: TransformDraft;
  onChange: (value: TransformDraft) => void;
  onApply: () => void;
  applyLabel: string;
}) {
  const setTranslation = (axis: number, next: number) => {
    const translation = [...value.translation] as [number, number, number];
    translation[axis] = next;
    onChange({ ...value, translation });
  };
  const setRotation = (axis: number, next: number) => {
    const rotationDeg = [...value.rotationDeg] as [number, number, number];
    rotationDeg[axis] = next;
    onChange({ ...value, rotationDeg });
  };
  return (
    <div className="mt-2">
      <p className="text-[8px] font-semibold uppercase tracking-wide text-mute">{title}</p>
      <div className="mt-1 grid grid-cols-[18px_repeat(3,1fr)] items-center gap-1">
        <span className="text-[8px] text-mute">mm</span>
        {value.translation.map((entry, axis) => (
          <input
            key={`translation-${axis}`}
            aria-label={`${title} ${'XYZ'[axis]} translation`}
            type="number"
            step="any"
            value={roundedInput(entry)}
            onChange={(event) => setTranslation(axis, Number(event.target.value))}
            className="h-7 min-w-0 rounded border border-edge bg-panel px-1 text-right text-[9px] text-ink outline-none focus:border-accent"
          />
        ))}
        <span className="text-[8px] text-mute">deg</span>
        {value.rotationDeg.map((entry, axis) => (
          <input
            key={`rotation-${axis}`}
            aria-label={`${title} ${'XYZ'[axis]} rotation`}
            type="number"
            step="any"
            value={roundedInput(entry)}
            onChange={(event) => setRotation(axis, Number(event.target.value))}
            className="h-7 min-w-0 rounded border border-edge bg-panel px-1 text-right text-[9px] text-ink outline-none focus:border-accent"
          />
        ))}
      </div>
      <div className="mt-1 flex items-center justify-between">
        <span className="pl-[22px] text-[7px] uppercase tracking-wide text-mute">X · Y · Z</span>
        <button
          type="button"
          onClick={onApply}
          className="rounded border border-edge bg-panel px-2 py-1 text-[8px] text-ink hover:border-accent"
        >
          {applyLabel}
        </button>
      </div>
    </div>
  );
}

function uniqueComponentName(definitions: ComponentDefinitionDto[], base: string): string {
  const names = new Set(definitions.map((definition) => definition.name));
  for (let suffix = 1; suffix < Number.MAX_SAFE_INTEGER; suffix += 1) {
    const candidate = `${base}${suffix}`;
    if (!names.has(candidate)) return candidate;
  }
  return `${base}-${Date.now()}`;
}

function descendantOccurrenceIds(
  occurrences: ComponentOccurrenceDto[],
  rootId: number,
): Set<number> {
  const descendants = new Set<number>();
  const queue = [rootId];
  while (queue.length > 0) {
    const parent = queue.shift()!;
    for (const occurrence of occurrences) {
      if (occurrence.parent_occurrence_id === parent && !descendants.has(occurrence.id)) {
        descendants.add(occurrence.id);
        queue.push(occurrence.id);
      }
    }
  }
  return descendants;
}

function transformDraft(transform: AssemblyTransformDto): TransformDraft {
  return {
    translation: [...transform.translation] as [number, number, number],
    rotationDeg: quaternionToEulerDegrees(transform.rotation),
  };
}

function transformFromDraft(draft: TransformDraft): AssemblyTransformDto {
  return {
    translation: draft.translation.map((value) => Number.isFinite(value) ? value : 0) as [number, number, number],
    rotation: eulerDegreesToQuaternion(draft.rotationDeg),
  };
}

function quaternionToEulerDegrees(
  rotation: [number, number, number, number],
): [number, number, number] {
  const [x, y, z, w] = rotation;
  const roll = Math.atan2(2 * (w * x + y * z), 1 - 2 * (x * x + y * y));
  const pitch = Math.asin(Math.max(-1, Math.min(1, 2 * (w * y - z * x))));
  const yaw = Math.atan2(2 * (w * z + x * y), 1 - 2 * (y * y + z * z));
  const degrees = 180 / Math.PI;
  return [roll * degrees, pitch * degrees, yaw * degrees];
}

function eulerDegreesToQuaternion(
  rotation: [number, number, number],
): [number, number, number, number] {
  const radians = Math.PI / 360;
  const [sx, sy, sz] = rotation.map((value) => Math.sin(value * radians));
  const [cx, cy, cz] = rotation.map((value) => Math.cos(value * radians));
  const quaternion: [number, number, number, number] = [
    sx * cy * cz + cx * sy * sz,
    cx * sy * cz - sx * cy * sz,
    cx * cy * sz + sx * sy * cz,
    cx * cy * cz - sx * sy * sz,
  ];
  const length = Math.hypot(...quaternion) || 1;
  return quaternion.map((value) => value / length) as [number, number, number, number];
}

function roundedInput(value: number): number {
  return Number((Number.isFinite(value) ? value : 0).toFixed(4));
}

function showAssemblyError(error: unknown): void {
  useAppStore.getState().setConstraintDialog({
    titleKey: 'file.errorTitle',
    message: error instanceof Error ? error.message : String(error),
  });
}

function valuesForJoint(joint: JointDefinitionDto | null): MotionValues {
  return joint ? {
    angle: joint.angle_offset_deg,
    linear: joint.linear_offset_mm,
    secondaryAngle: joint.advanced.secondary_angle_offset_deg,
    tertiaryAngle: joint.advanced.tertiary_angle_offset_deg,
    secondaryLinear: joint.advanced.secondary_linear_offset_mm,
  } : {
    angle: 0,
    linear: 0,
    secondaryAngle: 0,
    tertiaryAngle: 0,
    secondaryLinear: 0,
  };
}

function valuesForMotion(motion: JointMotionStateDto): MotionValues {
  return {
    angle: motion.angle_offset_deg,
    linear: motion.linear_offset_mm,
    secondaryAngle: motion.secondary_angle_offset_deg,
    tertiaryAngle: motion.tertiary_angle_offset_deg,
    secondaryLinear: motion.secondary_linear_offset_mm,
  };
}

function motionForValues(joint: JointDefinitionDto, values: MotionValues): JointMotionStateDto {
  return {
    joint_id: joint.id,
    angle_offset_deg: values.angle,
    linear_offset_mm: values.linear,
    secondary_angle_offset_deg: values.secondaryAngle,
    tertiary_angle_offset_deg: values.tertiaryAngle,
    secondary_linear_offset_mm: values.secondaryLinear,
  };
}

function motionControls(joint: JointDefinitionDto): Array<{
  key: keyof MotionValues;
  label: string;
  unit: '°' | 'mm';
  limits: JointLimitsDto | null;
  fallback: [number, number];
}> {
  const controls: ReturnType<typeof motionControls> = [];
  const angleLimits = joint.angle_limits
    ?? (joint.kind === 'revolute' || joint.kind === 'screw' ? joint.limits : null);
  const linearLimits = joint.linear_limits ?? (joint.kind === 'slider' ? joint.limits : null);
  if (['revolute', 'cylindrical', 'planar', 'ball', 'pin_slot', 'screw', 'universal'].includes(joint.kind)) {
    controls.push({ key: 'angle', label: joint.kind === 'screw' ? 'Rotation / travel' : 'Primary rotation', unit: '°', limits: angleLimits, fallback: [-180, 180] });
  }
  if (['slider', 'cylindrical', 'planar', 'pin_slot'].includes(joint.kind)) {
    controls.push({ key: 'linear', label: joint.kind === 'planar' || joint.kind === 'pin_slot' ? 'X slide' : 'Slide', unit: 'mm', limits: linearLimits, fallback: [-100, 100] });
  }
  if (joint.kind === 'ball' || joint.kind === 'universal') {
    controls.push({ key: 'secondaryAngle', label: 'Secondary rotation', unit: '°', limits: joint.advanced.secondary_angle_limits, fallback: [-180, 180] });
  }
  if (joint.kind === 'ball') {
    controls.push({ key: 'tertiaryAngle', label: 'Tertiary rotation', unit: '°', limits: joint.advanced.tertiary_angle_limits, fallback: [-180, 180] });
  }
  if (joint.kind === 'planar') {
    controls.push({ key: 'secondaryLinear', label: 'Y slide', unit: 'mm', limits: joint.advanced.secondary_linear_limits, fallback: [-100, 100] });
  }
  return controls;
}

function demoValues(joint: JointDefinitionDto, base: MotionValues, phase: number): MotionValues {
  const next = { ...base };
  for (const control of motionControls(joint)) {
    const fallback = control.unit === '°' ? 35 : 8;
    const amplitude = availableDemoAmplitude(base[control.key], control.limits, fallback);
    next[control.key] = clampToLimits(base[control.key] + phase * amplitude, control.limits);
  }
  return next;
}

function jointHasBrokenReference(
  joint: JointDefinitionDto,
  bodies: ReturnType<typeof useAppStore.getState>['solidScene']['bodies'],
): boolean {
  return [joint.connector_a, joint.connector_b].some((connector) => {
    const body = bodies.find((candidate) => candidate.id === connector.body_id);
    if (connector.kind === 'circular_edge') {
      const edge = body?.edges.find((candidate) => candidate.id === connector.edge_id);
      return !edge?.circle?.closed || edge.key !== connector.edge_key;
    }
    const face = body?.faces.find((candidate) => candidate.id === connector.face_id);
    return (!face?.plane && !face?.cylinder) || face.key !== connector.face_key;
  });
}

function kindShortLabel(joint: JointDefinitionDto): string {
  if (joint.kind === 'pin_slot') return 'pin-slot';
  return joint.kind;
}

function MotionControl({
  testId,
  label,
  unit,
  value,
  minimum,
  maximum,
  step,
  onChange,
}: {
  testId: string;
  label: string;
  unit: string;
  value: number;
  minimum: number;
  maximum: number;
  step: number;
  onChange: (value: number) => void;
}) {
  return (
    <label className="mt-2 block">
      <span className="flex items-center justify-between text-[9px] text-mute">
        <span>{label}</span>
        <span>{unit}</span>
      </span>
      <span className="mt-0.5 flex items-center gap-2">
        <input
          data-testid={`${testId}-slider`}
          type="range"
          min={minimum}
          max={maximum}
          step={step}
          value={value}
          onChange={(event) => onChange(Number(event.target.value))}
          className="min-w-0 flex-1 accent-accent"
        />
        <input
          data-testid={`${testId}-value`}
          type="number"
          step="any"
          value={value}
          onChange={(event) => {
            const next = Number(event.target.value);
            if (Number.isFinite(next)) onChange(next);
          }}
          className="h-7 w-[72px] rounded border border-edge bg-header px-1.5 text-right text-[10px] text-ink outline-none focus:border-accent"
        />
      </span>
    </label>
  );
}

function clampToLimits(value: number, limits: JointLimitsDto | null): number {
  return limits ? Math.max(limits.min, Math.min(limits.max, value)) : value;
}

function availableDemoAmplitude(
  base: number,
  limits: JointLimitsDto | null,
  fallback: number,
): number {
  if (!limits) return fallback;
  return Math.min(
    fallback,
    Math.max(Math.abs(limits.max - base), Math.abs(base - limits.min)),
  );
}
