import { beginRuntimeActivity } from '../runtimeActivity';

interface ExportFlow<Options, Target> {
  activityTarget?: string;
  assertSelectionOwner(): void | Promise<void>;
  captureModel(): Promise<string>;
  chooseOptions(): Promise<Options | null>;
  render(options: Options, expectedModelJson: string): Promise<Uint8Array>;
  chooseTarget(): Promise<Target | null>;
  write(target: Target, bytes: Uint8Array): Promise<void>;
}

/** Keep the UI selection, native geometry and eventual saved bytes together.
 * The renderer enforces its snapshot precondition atomically in the engine;
 * no frontend identity check can close the IPC scheduling gap by itself. */
export async function runExport<Options, Target>(flow: ExportFlow<Options, Target>): Promise<boolean> {
  const activity = beginRuntimeActivity({ kind: 'export', target: flow.activityTarget, notifySuccess: true });
  try {
    activity.setPhase('reading');
    await flow.assertSelectionOwner();
    const expectedModelJson = await flow.captureModel();
    await flow.assertSelectionOwner();
    activity.setPhase('choosing');
    const options = await flow.chooseOptions();
    if (options === null) { activity.discard(); return false; }
    await flow.assertSelectionOwner();
    activity.setPhase('geometry');
    const bytes = await flow.render(options, expectedModelJson);
    // Once captured, these bytes remain the requested model even if another
    // document opens while the operating-system save picker is displayed.
    activity.setPhase('choosing');
    const target = await flow.chooseTarget();
    if (target === null) { activity.discard(); return false; }
    activity.setPhase('writing');
    await flow.write(target, bytes);
    activity.finish();
    return true;
  } catch (error) { activity.fail(error); throw error; }
}
