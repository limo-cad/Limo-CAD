/**
 * noBS CAD application shell: integrated ribbon header, project tabs,
 * browser tree, viewport with overlays, sketch palette, comments strip,
 * and timeline. Layout is defined by noBS CAD's workspace model.
 *
 * Also owns global sketch-tool shortcuts, Delete/Backspace selection
 * deletion, Esc plane-pick cancellation, and engine undo/redo.
 */
import { useEffect, useRef } from 'react';
import { installOperationFeedback } from './operationPlayback';
import { useTranslation } from './i18n';
import { useAppStore } from './store/appStore';
import {
  cancelPlanePick,
  cancelTimelineFeatureEdit,
  deleteDimension,
  deleteConstraint,
  deleteEntities,
  openExtrude,
  openHole,
  redoApplicationHistory,
  undoApplicationHistory,
} from './engine/controller';
import { Ribbon } from './components/Ribbon';
import { BrowserTree } from './components/BrowserTree';
import { DrawingBrowser } from './components/drawing/DrawingBrowser';
import { DrawingWorkspace } from './components/drawing/DrawingWorkspace';
import { AssemblyBrowser } from './components/assembly/AssemblyBrowser';
import { JointDialog } from './components/assembly/JointDialog';
import { ProjectTabBar } from './components/TopBar';
import { AppearanceDialog } from './components/AppearanceDialog';
import { SketchPalette } from './components/SketchPalette';
import { CommentsPanel } from './components/CommentsPanel';
import { Timeline } from './components/Timeline';
import { Viewport } from './components/viewport/Viewport';
import { ConstraintDialogHost } from './components/ConstraintDialogHost';
import { ExtrudeDialog } from './components/ExtrudeDialog';
import { RevolveDialog } from './components/RevolveDialog';
import { SweepDialog } from './components/SweepDialog';
import { LoftDialog } from './components/LoftDialog';
import { RibDialog } from './components/RibDialog';
import { SolidChamferDialog, SolidFilletDialog } from './components/SolidEdgeDialogs';
import { HoleDialog } from './components/HoleDialog';
import { SketchPlaneOriginDialog } from './components/SketchPlaneOriginDialog';
import { ConstructionPlaneDialog } from './components/ConstructionPlaneDialog';
import { BodyFeatureDialog } from './components/BodyFeatureDialog';
import { BodyAppearancePanel } from './components/BodyAppearancePanel';
import { SketchPatternDialog } from './components/SketchPatternDialog';
import {
  installProjectRecovery,
  newProject,
  openProject,
  saveAllUnsavedProjects,
  saveProject,
} from './files/projectFiles';
import {
  hasUnsavedProjects,
  initializeProjectTabs,
  installProjectTabRetention,
} from './files/projectTabs';
import { SYSTEM_DARK_QUERY } from './theme';
import { deleteDrawingAnnotation } from './drawing/document';
import {
  installNativeEditMenu,
  nativeMacMenuOwnsUndoRedo,
} from './nativeEditMenu';
import { installNativeFileMenu } from './nativeFileMenu';
import { isTauriRuntime } from './engine';
import { requestUnsavedDecision } from './files/unsavedChanges';
import { createExitController } from './files/applicationExit';
import { UnsavedChangesDialog } from './components/UnsavedChangesDialog';

export default function App() {
  useEffect(installOperationFeedback, []);
  const { t } = useTranslation();
  const mode = useAppStore((s) => s.mode);
  const activeTab = useAppStore((s) => s.activeTab);
  const drawingWorkspace = activeTab === 'drawing';
  const solidSidebarMode = useAppStore((s) => s.solidSidebarMode);
  const resolvedTheme = useAppStore((s) => s.resolvedTheme);
  const themePreference = useAppStore((s) => s.themePreference);
  const syncResolvedTheme = useAppStore((s) => s.syncResolvedTheme);
  const loadDocument = useAppStore((s) => s.loadDocument);
  const initialized = useRef(false);

  useEffect(() => {
    if (initialized.current) return;
    initialized.current = true;
    void loadDocument()
      .then(async () => {
        // A new application launch always begins with one clean Untitled
        // document. Previous emergency snapshots are retained as a fallback,
        // but are never reopened as the user's active workspace.
        await initializeProjectTabs();
      })
      .catch((error) => {
        useAppStore.getState().setConstraintDialog({
          titleKey: 'file.errorTitle',
          message: error instanceof Error ? error.message : String(error),
        });
      });
  }, [loadDocument]);

  useEffect(() => installProjectRecovery(), []);

  useEffect(() => installProjectTabRetention(), []);

  useEffect(() => installNativeEditMenu(), []);

  useEffect(() => installNativeFileMenu(), []);

  useEffect(() => {
    const media = window.matchMedia(SYSTEM_DARK_QUERY);
    syncResolvedTheme();
    const sync = () => syncResolvedTheme();
    media.addEventListener('change', sync);
    return () => media.removeEventListener('change', sync);
  }, [syncResolvedTheme, themePreference]);

  useEffect(() => {
    const beforeUnload = (event: BeforeUnloadEvent) => {
      if (!hasUnsavedProjects()) return;
      event.preventDefault();
      event.returnValue = '';
    };
    window.addEventListener('beforeunload', beforeUnload);
    return () => window.removeEventListener('beforeunload', beforeUnload);
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let exitController: ReturnType<typeof createExitController> | null = null;
    const requestQuit = () => { void exitController?.request(); };
    let unlistenClose: (() => void) | null = null;
    let unlistenQuit: (() => void) | null = null;
    void Promise.all([
      import('@tauri-apps/api/core'),
      import('@tauri-apps/api/event'),
      import('@tauri-apps/api/window'),
    ]).then(async ([{ invoke }, { listen }, { getCurrentWindow }]) => {
      if (disposed) return;
      const appWindow = getCurrentWindow();
      exitController = createExitController({
        dirty: hasUnsavedProjects,
        decide: () => requestUnsavedDecision('quit'),
        save: saveAllUnsavedProjects,
        exit: () => invoke('native_force_quit'),
        error: (error) => {
          useAppStore.getState().setConstraintDialog({
            titleKey: 'file.errorTitle',
            message: error instanceof Error ? error.message : String(error),
          });
        },
      });
      window.addEventListener('nbcad:quit-request', requestQuit);
      unlistenClose = await appWindow.onCloseRequested((event) => {
        // Always own close: Tauri's default JS handler calls window.destroy,
        // which is not granted by core:default and bypasses our quit route.
        event.preventDefault();
        void requestQuit();
      });
      unlistenQuit = await listen('native-quit-request', () => {
        void requestQuit();
      });
      if (disposed) {
        unlistenClose();
        unlistenQuit();
        unlistenClose = null;
        unlistenQuit = null;
      }
    }).catch(() => undefined);
    return () => {
      disposed = true;
      exitController?.dispose();
      window.removeEventListener('nbcad:quit-request', requestQuit);
      unlistenClose?.();
      unlistenQuit?.();
    };
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let unsubscribe: (() => void) | null = null;
    void import('@tauri-apps/api/core').then(({ invoke }) => {
      if (disposed) return;
      const sync = () => {
        void invoke('native_unsaved_set', { unsaved: hasUnsavedProjects() });
      };
      sync();
      unsubscribe = useAppStore.subscribe(sync);
    }).catch(() => undefined);
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, []);

  useEffect(() => {
    window.document.title = t('app.name');
  }, [t]);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      // Never steal keys from text inputs.
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) {
        // The custom native menu delegates text Undo/Redo back to WebKit. If
        // WKWebView also exposes the key event, suppress its second edit.
        if (
          nativeMacMenuOwnsUndoRedo() &&
          e.metaKey &&
          e.key.toLowerCase() === 'z'
        ) {
          e.preventDefault();
        }
        return;
      }
      const s = useAppStore.getState();
      const runProjectAction = (action: () => Promise<unknown>) => {
        if (s.projectBusy || s.solidBusy || s.historyEdit) return;
        s.setProjectBusy(true);
        void action()
          .catch((error) => {
            useAppStore.getState().setConstraintDialog({
              titleKey: 'file.errorTitle',
              message: error instanceof Error ? error.message : String(error),
            });
          })
          .finally(() => useAppStore.getState().setProjectBusy(false));
      };

      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 's') {
        e.preventDefault();
        if (s.document === null) return;
        runProjectAction(() => saveProject(e.shiftKey));
        return;
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'o') {
        e.preventDefault();
        runProjectAction(openProject);
        return;
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'n') {
        e.preventDefault();
        runProjectAction(newProject);
        return;
      }

      // Sketch editing uses its command stack. At the latest solid-history
      // position, Undo permanently removes the newest feature and recomputes
      // the surviving graph. Shift+Undo still moves forward when the user
      // has explicitly moved the build cursor backward in the timeline.
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'z') {
        // On macOS/Tauri the native menu accelerator owns Cmd-Z; letting it
        // emit one command avoids a duplicate webview keydown action.
        if (nativeMacMenuOwnsUndoRedo()) return;
        e.preventDefault();
        if (e.shiftKey) void redoApplicationHistory();
        else void undoApplicationHistory();
        return;
      }

      if (e.key === 'Escape') {
        // Escape cancels CAD state only; do not let the WebView/AppKit default
        // simultaneously leave native macOS full-screen mode.
        e.preventDefault();
        // Keep navigation cancellation at the application boundary as well
        // as the viewport boundary. This remains reliable while the document
        // is loading or the native child view is being reparented.
        if (s.navTool !== 'select') s.setNavTool('select');
        if (s.drawingTool !== null) s.setDrawingTool(null);
        if (s.drawingPendingViewKind !== null) s.setDrawingPendingViewKind(null);
        // Sketch-mode Esc (end chain / deselect) is handled by the Viewport.
        if (s.mode === 'pickPlane') cancelPlanePick();
        const hasFeatureDialog =
          s.extrudeDialogFeature !== null ||
          s.revolveDialogFeature !== null ||
          s.sweepDialogFeature !== null ||
          s.loftDialogFeature !== null ||
          s.ribDialogFeature !== null ||
          s.filletDialogFeature !== null ||
          s.chamferDialogFeature !== null ||
          s.holeDialogFeature !== null ||
          s.constructionPlaneDialog !== null ||
          s.bodyFeatureDialog !== null;
        const closeFeatureDialogs = () => {
          if (s.extrudeDialogFeature !== null) s.closeExtrudeDialog();
          if (s.revolveDialogFeature !== null) s.closeRevolveDialog();
          if (s.sweepDialogFeature !== null) s.closeSweepDialog();
          if (s.loftDialogFeature !== null) s.closeLoftDialog();
          if (s.ribDialogFeature !== null) s.closeRibDialog();
          if (s.filletDialogFeature !== null) s.closeFilletDialog();
          if (s.chamferDialogFeature !== null) s.closeChamferDialog();
          if (s.holeDialogFeature !== null) s.closeHoleDialog();
          if (s.constructionPlaneDialog !== null) s.closeConstructionPlaneDialog();
          if (s.bodyFeatureDialog !== null) s.closeBodyFeatureDialog();
        };
        if (hasFeatureDialog && s.historyEdit) {
          void cancelTimelineFeatureEdit(closeFeatureDialogs);
        } else {
          closeFeatureDialogs();
        }
        if (s.sketchPatternDialog !== null) s.closeSketchPatternDialog();
        if (s.jointDialogOpen) s.setJointDialogOpen(false);
        return;
      }

      if (s.document === null) return;

      if (
        s.activeTab === 'drawing'
        && (e.key === 'Delete' || e.key === 'Backspace')
        && s.selectedDrawingAnnotationId !== null
      ) {
        e.preventDefault();
        void deleteDrawingAnnotation(s.selectedDrawingAnnotationId);
        return;
      }

      if (
        s.solidSidebarMode === 'assembly'
        && (e.key === 'Delete' || e.key === 'Backspace')
        && s.selectedJointId !== null
      ) {
        e.preventDefault();
        void s.deleteJoint(s.selectedJointId).catch((error) => {
          useAppStore.getState().setConstraintDialog({
            titleKey: 'file.errorTitle',
            message: error instanceof Error ? error.message : String(error),
          });
        });
        return;
      }

      if (s.activeTab === 'solid' && s.mode === 'solid' && e.key.toLowerCase() === 'e' && !e.metaKey && !e.ctrlKey && !e.altKey) {
        e.preventDefault();
        openExtrude();
        return;
      }
      if (s.activeTab === 'solid' && s.mode === 'solid' && e.key.toLowerCase() === 'h' && !e.metaKey && !e.ctrlKey && !e.altKey) {
        e.preventDefault();
        openHole();
        return;
      }

      if (s.mode !== 'sketch') return;

      // The dynamic-input cluster owns typing while a tool runs.
      if (s.dynInput.active) return;

      const shortcutTools = {
        l: 'line',
        d: 'dimension',
        f: 'fillet',
        o: 'offset',
        t: 'trim',
        m: 'moveCopy',
      } as const;
      const shortcut = !e.metaKey && !e.ctrlKey && !e.altKey
        ? shortcutTools[e.key.toLowerCase() as keyof typeof shortcutTools]
        : undefined;
      if (shortcut) {
        e.preventDefault();
        s.setActiveTool(shortcut);
        return;
      }

      if (
        (e.key === 'Delete' || e.key === 'Backspace') &&
        (s.selectedEntity !== null || s.selectedDimension !== null || s.selectedConstraint !== null)
      ) {
        e.preventDefault();
        // Dimensions take precedence when selected (D9).
        if (s.selectedDimension !== null) {
          void deleteDimension(s.selectedDimension);
          return;
        }
        if (s.selectedConstraint !== null) {
          void deleteConstraint(s.selectedConstraint);
          return;
        }
        const ids = new Set(s.selectedEntities);
        if (s.selectedEntity !== null) ids.add(s.selectedEntity);
        void deleteEntities([...ids]);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-panel text-ink">
      {/* Project tabs are the window-level row: each project owns the
          workspace stage (modeling/drawing) rendered below it. */}
      <div className="contents" data-mcp-surface="file-and-project-tabs"><ProjectTabBar /></div>
      <div className="contents" data-mcp-surface="ribbon"><Ribbon /></div>
      <div className="flex min-h-0 flex-1">
        <div className="contents" data-mcp-surface="browser">{drawingWorkspace ? (
          <DrawingBrowser />
        ) : solidSidebarMode === 'assembly' ? (
          <AssemblyBrowser />
        ) : (
          <BrowserTree />
        )}</div>
        <div className="flex min-w-0 flex-1 flex-col">
          <main className="relative min-h-0 min-w-0 flex-1">
            {drawingWorkspace ? (
              <div className="contents" data-mcp-surface="drawing"><DrawingWorkspace /></div>
            ) : (
              <>
                <div className="contents" data-mcp-surface="viewport"><Viewport key={resolvedTheme} /></div>
                <div className="contents" data-mcp-surface="appearance"><BodyAppearancePanel /></div>
                {mode === 'sketch' && <div className="contents" data-mcp-surface="sketch-palette"><SketchPalette /></div>}
                <div className="contents" data-mcp-surface="comments"><CommentsPanel /></div>
              </>
            )}
          </main>
        </div>
      </div>
      {!drawingWorkspace && <div className="contents" data-mcp-surface="feature-history"><Timeline /></div>}
      <ConstraintDialogHost />
      <SketchPlaneOriginDialog />
      <ExtrudeDialog />
      <RevolveDialog />
      <SweepDialog />
      <LoftDialog />
      <RibDialog />
      <SolidFilletDialog />
      <SolidChamferDialog />
      <HoleDialog />
      <ConstructionPlaneDialog />
      <BodyFeatureDialog />
      <SketchPatternDialog />
      <AppearanceDialog />
      <JointDialog />
      <UnsavedChangesDialog />
    </div>
  );
}
