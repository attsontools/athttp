import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { open, save } from '@tauri-apps/plugin-dialog';

import type { HttpHost, ThemeName } from './pluginHost';
import type { ServiceEvent, ServiceTransport } from './transport';

const STORAGE_KEY = 'athttp.theme';

export interface StandaloneDependencies {
  invoke<T>(command: string, args?: unknown): Promise<T>;
  listen<T>(event: string, listener: (event: { payload: T }) => void): Promise<() => void>;
  storage: Pick<Storage, 'getItem'>;
  pickFile(): Promise<string | null>;
  copyText(text: string): Promise<void>;
  saveFile(input: { suggestedName: string; content: string }): Promise<boolean>;
}

export function createStandaloneHost(
  dependencies: StandaloneDependencies = browserDependencies()
): HttpHost {
  const transport: ServiceTransport = {
    async request<T>(method: string, params: unknown = {}): Promise<T> {
      return await dependencies.invoke<T>('http_service_request', { method, params });
    },
    subscribe(listener) {
      let disposed = false;
      let remove: (() => void) | null = null;
      void dependencies.listen<ServiceEvent>('http-service-event', (event) => {
        if (!disposed) listener(event.payload);
      }).then((unlisten) => {
        if (disposed) unlisten();
        else remove = unlisten;
      });
      return () => {
        disposed = true;
        remove?.();
      };
    },
    close() {}
  };

  return {
    mode: 'standalone',
    transport,
    async initialize(onTheme) {
      await dependencies.invoke('connect_http_service');
      const theme: ThemeName = dependencies.storage.getItem(STORAGE_KEY) === 'light' ? 'light' : 'dark';
      onTheme(theme);
      return theme;
    },
    pickFile: dependencies.pickFile,
    copyText: dependencies.copyText,
    saveFile: dependencies.saveFile,
    dispose() {
      transport.close();
    }
  };
}

function browserDependencies(): StandaloneDependencies {
  return {
    invoke,
    listen,
    storage: localStorage,
    async pickFile() {
      const path = await open({ multiple: false });
      return typeof path === 'string' ? path : null;
    },
    async copyText(text: string) {
      await writeText(text);
    },
    async saveFile(input) {
      const path = await save({ defaultPath: input.suggestedName });
      if (!path) return false;
      await invoke('write_text_file', { path, content: input.content });
      return true;
    }
  };
}
