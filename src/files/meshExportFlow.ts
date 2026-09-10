import type { MeshExportScope } from '../engine/types';

interface MeshExportFlow<Target> {
  assertSelectionOwner(): void | Promise<void>;
  captureModel(): Promise<string>;
  chooseScope(): Promise<MeshExportScope | null>;
  render(scope: MeshExportScope, expectedModelJson: string): Promise<Uint8Array>;
  chooseTarget(): Promise<Target | null>;
  write(target: Target, bytes: Uint8Array): Promise<void>;
}

/** Keep the UI selection, native geometry and eventual saved bytes together.
 * The renderer enforces its snapshot precondition atomically in the engine;
 * no frontend identity check can close the IPC scheduling gap by itself. */
export async function runMeshExport<Target>(flow: MeshExportFlow<Target>): Promise<boolean> {
  await flow.assertSelectionOwner();
  const expectedModelJson = await flow.captureModel();
  await flow.assertSelectionOwner();
  const scope = await flow.chooseScope();
  if (scope === null) return false;
  await flow.assertSelectionOwner();
  const bytes = await flow.render(scope, expectedModelJson);
  // Once captured, these bytes remain the requested model even if another
  // document opens while the operating-system save picker is displayed.
  const target = await flow.chooseTarget();
  if (target === null) return false;
  await flow.write(target, bytes);
  return true;
}
