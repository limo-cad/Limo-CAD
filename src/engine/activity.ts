export type EngineOperationOwner = symbol;
const pending = new Set<EngineOperationOwner>();
export function pendingEngineOperations(owner?: EngineOperationOwner): number {
  return pending.size - (owner && pending.has(owner) ? 1 : 0);
}
export async function trackEngineOperation<T>(operation: Promise<T> | ((owner: EngineOperationOwner) => Promise<T>)): Promise<T> {
  const owner = Symbol('engine operation');
  pending.add(owner);
  try { return await (typeof operation === 'function' ? operation(owner) : operation); }
  finally { pending.delete(owner); }
}
