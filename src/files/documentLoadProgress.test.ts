import { chooseOpenFile } from './fileIO';

function equal(actual: unknown, expected: unknown, message = 'Unexpected file IO progress'): void {
  if (actual !== expected) throw new Error(`${message}: ${String(actual)} !== ${String(expected)}`);
}

const type = { description: 'Project', extension: '.nbcad', mime: 'application/octet-stream' };
const originalWindow = Object.getOwnPropertyDescriptor(globalThis, 'window');
const host = (value: unknown) => Object.defineProperty(globalThis, 'window', { value, configurable: true });
try {
  const events: string[] = [];
  let complete!: (bytes: number[]) => void;
  host({ __TAURI_INTERNALS__: { invoke(command: string) {
    equal(command, 'read_binary_file');
    events.push('native-read');
    return new Promise<number[]>((resolve) => { complete = resolve; });
  } } });
  const opening = chooseOpenFile(type, 'C:\\parts\\bracket.nbcad', (name) => events.push(`reading:${name}`));
  // The dialog module is loaded asynchronously even with a supplied path.
  while (!complete) await new Promise((resolve) => setTimeout(resolve, 1));
  equal(events.join('|'), 'reading:bracket.nbcad|native-read');
  complete([1, 2, 3]);
  equal((await opening)?.name, 'bracket.nbcad');

  let callbacks = 0;
  host({ showOpenFilePicker: async () => { throw new DOMException('Cancelled', 'AbortError'); } });
  equal(await chooseOpenFile(type, undefined, () => callbacks++), null);
  equal(callbacks, 0, 'A cancelled picker must never claim a file is being read.');

  const browserEvents: string[] = [];
  host({ showOpenFilePicker: async () => [{ name: 'shaft.nbcad', getFile: async () => {
    browserEvents.push('get-file');
    return { name: 'shaft.nbcad', size: 1, arrayBuffer: async () => {
      browserEvents.push('read-bytes'); return new Uint8Array([7]).buffer;
    } };
  } }] });
  await chooseOpenFile(type, undefined, (name) => browserEvents.push(`reading:${name}`));
  equal(browserEvents.join('|'), 'reading:shaft.nbcad|get-file|read-bytes');
  console.log('File reading progress follows selection and actual native/browser IO; cancellation stays silent.');
} finally {
  if (originalWindow) Object.defineProperty(globalThis, 'window', originalWindow);
  else Reflect.deleteProperty(globalThis, 'window');
}
