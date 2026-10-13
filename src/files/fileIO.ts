import { invoke } from '@tauri-apps/api/core';
import { isTauriRuntime } from '../engine';
import { translate } from '../i18n';

const MAX_FILE_BYTES = 256 * 1024 * 1024;

export interface SaveType {
  description: string;
  /** Dotted i18n key; when present the picker/dialog label follows the locale. */
  descriptionKey?: string;
  extension: string;
  alternateExtensions?: string[];
  mime: string;
}

type NativeTarget = { kind: 'native'; path: string; name: string };
type BrowserTarget = { kind: 'browser'; handle: FileSystemFileHandle; name: string };
type DownloadTarget = { kind: 'download'; name: string };
export type SaveTarget = NativeTarget | BrowserTarget | DownloadTarget;

export interface OpenedFile {
  name: string;
  bytes: Uint8Array;
  writableTarget: SaveTarget | null;
}

interface PickerWindow extends Window {
  showOpenFilePicker?: (options: {
    multiple?: boolean;
    types?: Array<{
      description: string;
      accept: Record<string, string[]>;
    }>;
  }) => Promise<FileSystemFileHandle[]>;
  showSaveFilePicker?: (options: {
    suggestedName?: string;
    types?: Array<{
      description: string;
      accept: Record<string, string[]>;
    }>;
  }) => Promise<FileSystemFileHandle>;
}

function withExtension(name: string, extension: string): string {
  return name.toLowerCase().endsWith(extension.toLowerCase()) ? name : `${name}${extension}`;
}

function pathName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

function saveTypeLabel(type: SaveType): string {
  return type.descriptionKey ? translate(type.descriptionKey) : type.description;
}

function pickerType(type: SaveType) {
  return {
    description: saveTypeLabel(type),
    accept: {
      [type.mime]: [type.extension, ...(type.alternateExtensions ?? [])],
    },
  };
}

async function withNativeViewportSuspended<T>(action: () => Promise<T>): Promise<T> {
  if (!isTauriRuntime()) return action();
  await invoke('native_viewport_set_suspended', { suspended: true }).catch(() => undefined);
  try {
    return await action();
  } finally {
    await invoke('native_viewport_set_suspended', { suspended: false }).catch(() => undefined);
  }
}

export async function chooseSaveTarget(
  suggestedName: string,
  type: SaveType,
): Promise<SaveTarget | null> {
  const fileName = withExtension(suggestedName, type.extension);
  if (isTauriRuntime()) {
    const { save } = await import('@tauri-apps/plugin-dialog');
    const selected = await withNativeViewportSuspended(() => save({
        defaultPath: fileName,
        filters: [
          {
            name: saveTypeLabel(type),
            extensions: [
              type.extension.slice(1),
              ...(type.alternateExtensions ?? []).map((extension) =>
                extension.slice(1),
              ),
            ],
          },
        ],
      }));
    if (!selected) return null;
    const path = withExtension(selected, type.extension);
    return { kind: 'native', path, name: pathName(path) };
  }

  const picker = window as PickerWindow;
  if (picker.showSaveFilePicker) {
    try {
      const handle = await picker.showSaveFilePicker({
        suggestedName: fileName,
        types: [pickerType(type)],
      });
      return { kind: 'browser', handle, name: handle.name };
    } catch (error) {
      if (error instanceof DOMException && error.name === 'AbortError') return null;
      throw error;
    }
  }
  return { kind: 'download', name: fileName };
}

export async function writeSaveTarget(target: SaveTarget, bytes: Uint8Array): Promise<void> {
  if (bytes.byteLength > MAX_FILE_BYTES) {
    throw new Error(translate('file.errorFileTooLarge'));
  }
  if (target.kind === 'native') {
    await invoke('write_binary_file_atomic', {
      path: target.path,
      bytes: Array.from(bytes),
    });
    return;
  }
  if (target.kind === 'browser') {
    const writable = await target.handle.createWritable();
    try {
      await writable.write(bytes);
      await writable.close();
    } catch (error) {
      await writable.abort().catch(() => undefined);
      throw error;
    }
    return;
  }
  const blob = new Blob([bytes], { type: 'application/octet-stream' });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = target.name;
  anchor.click();
  setTimeout(() => URL.revokeObjectURL(url), 1_000);
}

export async function chooseOpenFile(type: SaveType, pathOverride?: string, onReading?: (name: string) => void): Promise<OpenedFile | null> {
  if (isTauriRuntime()) {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const selected = pathOverride ?? await withNativeViewportSuspended(() => open({
        multiple: false,
        directory: false,
        filters: [
          {
            name: saveTypeLabel(type),
            extensions: [
              type.extension.slice(1),
              ...(type.alternateExtensions ?? []).map((extension) =>
                extension.slice(1),
              ),
            ],
          },
        ],
      }));
    if (!selected || Array.isArray(selected)) return null;
    onReading?.(pathName(selected));
    const raw = await invoke<number[]>('read_binary_file', { path: selected });
    return {
      name: pathName(selected),
      bytes: Uint8Array.from(raw),
      writableTarget: { kind: 'native', path: selected, name: pathName(selected) },
    };
  }

  const picker = window as PickerWindow;
  if (picker.showOpenFilePicker) {
    try {
      const [handle] = await picker.showOpenFilePicker({
        multiple: false,
        types: [pickerType(type)],
      });
      if (!handle) return null;
      onReading?.(handle.name);
      const file = await handle.getFile();
      if (file.size > MAX_FILE_BYTES) {
        throw new Error(translate('file.errorFileTooLarge'));
      }
      return {
        name: file.name,
        bytes: new Uint8Array(await file.arrayBuffer()),
        writableTarget: { kind: 'browser', handle, name: handle.name },
      };
    } catch (error) {
      if (error instanceof DOMException && error.name === 'AbortError') return null;
      throw error;
    }
  }

  return new Promise((resolve, reject) => {
    const input = document.createElement('input');
    let settled = false;
    let cancelTimer: number | null = null;
    const finish = (value: OpenedFile | null) => {
      if (settled) return;
      settled = true;
      if (cancelTimer !== null) window.clearTimeout(cancelTimer);
      window.removeEventListener('focus', onWindowFocus);
      input.remove();
      resolve(value);
    };
    const fail = (error: unknown) => {
      if (settled) return;
      settled = true;
      if (cancelTimer !== null) window.clearTimeout(cancelTimer);
      window.removeEventListener('focus', onWindowFocus);
      input.remove();
      reject(error);
    };
    const onWindowFocus = () => {
      // Older browsers do not emit the input `cancel` event. File chooser
      // selection dispatches `change` first; the short delay lets it win.
      cancelTimer = window.setTimeout(() => {
        if (!input.files?.length) finish(null);
      }, 100);
    };
    input.type = 'file';
    input.accept = [type.extension, ...(type.alternateExtensions ?? [])].join(',');
    input.hidden = true;
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) {
        finish(null);
        return;
      }
      if (file.size > MAX_FILE_BYTES) {
        fail(new Error(translate('file.errorFileTooLarge')));
        return;
      }
      try {
        onReading?.(file.name);
        finish({
          name: file.name,
          bytes: new Uint8Array(await file.arrayBuffer()),
          writableTarget: null,
        });
      } catch (error) {
        fail(error);
      }
    };
    input.oncancel = () => finish(null);
    window.addEventListener('focus', onWindowFocus);
    document.body.append(input);
    input.click();
  });
}
