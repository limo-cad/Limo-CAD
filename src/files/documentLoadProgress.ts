import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { beginRuntimeActivity } from '../runtimeActivity';
import { useStartupStore } from '../startup';

export type DocumentLoadPhase = 'reading' | 'validating' | 'waiting' | 'geometry' | 'adopting' | 'rendering';
export interface DocumentLoadProgress {
  id: number;
  name: string;
  phase: DocumentLoadPhase;
}
export const useDocumentLoadStore = create<{ operations: DocumentLoadProgress[] }>(() => ({ operations: [] }));
let nextId = 0;
const measuredPhases: Partial<Record<DocumentLoadPhase, string>> = {
  reading: 'fileRead', validating: 'fileValidate', geometry: 'fileGeometry', adopting: 'fileAdoption',
};

/** Progress describes observed operations, not percentages or rendered-frame
 * readiness. Each Open owns its entry, including while queued behind another
 * project transition. Finishing one Open must not clear another Open's status. */
export function beginDocumentLoad(name: string) {
  const runtime = typeof window !== 'undefined'
    && (!('__TAURI_INTERNALS__' in window) || useStartupStore.getState().completed)
    ? beginRuntimeActivity({ kind: /\.(step|stp)$/i.test(name) ? 'import' : 'open', target: name, notifySuccess: true }) : null;
  runtime?.setPhase('reading');
  const id = ++nextId;
  let phase: DocumentLoadPhase = 'reading';
  let started = performance.now();
  let finished = false;
  useDocumentLoadStore.setState((state) => ({ operations: [...state.operations, { id, name, phase }] }));
  const record = () => {
    const measured = measuredPhases[phase];
    const diagnostics = typeof window !== 'undefined' ? window.__startupMeasurementSettingsPromise : undefined;
    if (measured && diagnostics) {
      const elapsedMs = performance.now();
      const durationMs = elapsedMs - started;
      // Resolve only the already-requested optional diagnostic setting. A
      // normal file Open sends no telemetry IPC and never awaits diagnostics.
      void diagnostics.then((settings) => settings
        ? invoke('startup_record', { phase: measured, elapsedMs, durationMs }) : undefined)
        .catch((error) => console.warn('File preparation measurement failed', error));
    }
  };
  return {
    setPhase(next: DocumentLoadPhase) {
      if (finished || next === phase) return;
      record();
      phase = next;
      runtime?.setPhase(next === 'waiting' ? 'queued' : next);
      started = performance.now();
      useDocumentLoadStore.setState((state) => ({
        operations: state.operations.map((operation) => operation.id === id ? { ...operation, phase } : operation),
      }));
    },
    finish() {
      if (finished) return;
      finished = true;
      record();
      runtime?.finish();
      useDocumentLoadStore.setState((state) => ({ operations: state.operations.filter((operation) => operation.id !== id) }));
    },
    fail(error: unknown) {
      if (finished) return;
      finished = true;
      record();
      runtime?.fail(error);
      useDocumentLoadStore.setState((state) => ({ operations: state.operations.filter((operation) => operation.id !== id) }));
    },
    discard() {
      if (finished) return;
      finished = true;
      record();
      runtime?.discard();
      useDocumentLoadStore.setState((state) => ({ operations: state.operations.filter((operation) => operation.id !== id) }));
    },
  };
}

export function currentDocumentLoad(operations: readonly DocumentLoadProgress[]): DocumentLoadProgress | null {
  return operations[0] ?? null;
}
