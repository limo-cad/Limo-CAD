interface ExportFlow<Options, Target> {
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
  await flow.assertSelectionOwner();
  const expectedModelJson = await flow.captureModel();
  await flow.assertSelectionOwner();
  const options = await flow.chooseOptions();
  if (options === null) return false;
  await flow.assertSelectionOwner();
  const bytes = await flow.render(options, expectedModelJson);
  // Once captured, these bytes remain the requested model even if another
  // document opens while the operating-system save picker is displayed.
  const target = await flow.chooseTarget();
  if (target === null) return false;
  await flow.write(target, bytes);
  return true;
}
