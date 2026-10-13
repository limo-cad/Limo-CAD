import { useAppStore } from '../store/appStore';
import { pendingEngineMutations, type EngineOperationOwner } from '../engine/activity';
import { translate } from '../i18n';
import { projectTransitions } from './projectTransitions';

/** Retain the published document and every frontend-owned part of its model.
 * A tab ID survives Open, and dirty alone does not identify a later edit. */
export interface ProjectOwnerOptions {
  /** Writers that never consume the derived assembly read-model may ignore its
   *  background refresh: `applySolidUpdate` re-reads `assemblyDocument` from
   *  the engine asynchronously, and that read landing mid-write does not change
   *  the document the writer is bound to. Save keeps the strict check because a
   *  save reads assembly content from the engine. */
  ignoreAssemblyReadModel?: boolean;
}

export function captureProjectOwner(
  allowSolidBusy = false,
  operationOwner?: EngineOperationOwner,
  options: ProjectOwnerOptions = {},
) {
  const state = useAppStore.getState();
  const revision = projectTransitions.capture();
  const assertFrontendUnchanged = (allowNameAdoption = false) => {
    const current = useAppStore.getState();
    const sameDocument = current.document === state.document || (
      allowNameAdoption && current.document !== null && state.document !== null
      && JSON.stringify({ ...current.document, name: state.document.name }) === JSON.stringify(state.document)
    );
    const assemblyReadModelChanged = !options.ignoreAssemblyReadModel
      && current.assemblyDocument !== state.assemblyDocument;
    if (current.activeProjectTabId !== state.activeProjectTabId
      || !sameDocument || current.solidScene !== state.solidScene
      || current.finishedSketches !== state.finishedSketches || current.datumPlanes !== state.datumPlanes
      || current.bodyAppearances !== state.bodyAppearances || current.drawingDocument !== state.drawingDocument
      || assemblyReadModelChanged || current.projectVisibility !== state.projectVisibility
      || current.activeSketch !== state.activeSketch || current.historyEdit !== state.historyEdit) {
      throw new Error(translate('file.errorDocumentChangedDuringSave'));
    }
  };
  const assertUnchanged = () => {
    projectTransitions.assertPublished(revision);
    assertFrontendUnchanged();
  };
  // Open replaces rather than consumes the native model. An uncertain failed
  // load must remain recoverable without weakening Save/export publication.
  const assertReplacementSource = (allowNameAdoption = false) => {
    projectTransitions.assertRevision(revision);
    assertFrontendUnchanged(allowNameAdoption);
    const current = useAppStore.getState();
    if (current.camDocument !== state.camDocument || current.activeNamedView !== state.activeNamedView
      || current.viewPartOffsets !== state.viewPartOffsets) {
      throw new Error(translate('file.errorDocumentChangedDuringOpen'));
    }
  };
  const assertSettled = () => {
    projectTransitions.assertSettled(revision);
    const current = useAppStore.getState();
    if ((!allowSolidBusy && current.solidBusy) || pendingEngineMutations(operationOwner) > 0
      || (!state.projectBusy && current.projectBusy)) throw new Error(translate('file.errorDocumentChangedDuringSave'));
    assertUnchanged();
  };
  return { state, assertSettled, assertUnchanged, assertReplacementSource, async assertCurrent() {
    await projectTransitions.assertCurrent(revision);
    assertSettled();
  } };
}
