import { WasmEngine } from './wasm';
import { BrowserOcctKernel } from './occtBrowser';
import type { StepExportRequest } from './types';

/** The contract server supplies a minimal wasm binding, while this executes
 * the production adapter, tab lifecycle and asynchronous kernel-load boundary. */
export async function checkBrowserStepExportOwnership() {
  const original = BrowserOcctKernel.create;
  const check = (value: unknown, message: string) => { if (!value) throw new Error(message); };
  try {
    for (const mode of ['unchanged', 'edit', 'tab', 'unprotected'] as const) {
      let ready!: (kernel: BrowserOcctKernel) => void;
      BrowserOcctKernel.create = () => new Promise((resolve) => { ready = resolve; });
      const engine = await WasmEngine.create();
      await engine.bindProjectSession('original');
      const expected = await engine.exportProjectModel();
      const request: StepExportRequest = {body_ids: [7], thread_metadata: [],
        ...(mode === 'unprotected' ? {} : {expected_model_json: expected})};
      const pending = engine.exportStep(request);
      if (mode === 'edit' || mode === 'unprotected') await engine.setDocumentName('Changed during kernel initialization');
      if (mode === 'tab') await engine.createProjectSession('replacement');
      let renders = 0;
      ready({exportStep: (actual: StepExportRequest) => {
        check(actual === request, 'Adapter must preserve the full selected STEP request');
        renders++; return new Uint8Array([7, 42]);
      }} as unknown as BrowserOcctKernel);
      if (mode === 'edit' || mode === 'tab') {
        let rejected: unknown;
        try { await pending; } catch (error) { rejected = error; }
        check(/document changed/i.test(String(rejected)), `${mode} must reject after async kernel load`);
        check(renders === 0, 'An invalidated request must not enter OCCT');
      } else {
        check(JSON.stringify([...await pending]) === '[7,42]' && renders === 1,
          'Unchanged and legacy requests preserve browser STEP export');
      }
    }
    return {sameDocumentEdit: true, tabChangeDuringLoad: true, unchanged: true, legacy: true};
  } finally { BrowserOcctKernel.create = original; }
}
