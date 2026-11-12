import type { ServiceEvent, ServiceTransport } from './transport';

export type ThemeName = 'dark' | 'light';

const APP_ID = 'com.attson.http';
const REQUIRED_CAPABILITIES = [
  'host.http.request',
  'host.http.stream',
  'host.http.data'
] as const;

interface PluginHandshake {
  theme: ThemeName;
  capabilities: string[];
}

interface PluginConnection {
  handshake(): Promise<PluginHandshake>;
  call<T>(method: string, params?: unknown): Promise<T>;
  onThemeChange(listener: (theme: ThemeName) => void): () => void;
  onStreamEvent(listener: (event: ServiceEvent) => void): () => void;
  close(): void;
}

type ConnectPlugin = (appId: string) => Promise<PluginConnection>;

export interface HttpHost {
  readonly mode: 'plugin';
  readonly transport: ServiceTransport;
  initialize(onTheme: (theme: ThemeName) => void): Promise<ThemeName>;
  dispose(): void;
}

export function createPluginHost(connect: ConnectPlugin): HttpHost {
  let connection: PluginConnection | null = null;
  let removeThemeListener: (() => void) | null = null;
  let removeStreamListener: (() => void) | null = null;
  const streamListeners = new Set<(event: ServiceEvent) => void>();

  const transport: ServiceTransport = {
    async request<T>(method: string, params: unknown = {}): Promise<T> {
      if (!connection) throw new Error('HTTP_SERVICE_DISCONNECTED');
      return await connection.call<T>(hostMethod(method), { method, params });
    },
    subscribe(listener) {
      streamListeners.add(listener);
      return () => streamListeners.delete(listener);
    },
    close() {
      connection?.close();
    }
  };

  return {
    mode: 'plugin',
    transport,
    async initialize(onTheme) {
      connection = await connect(APP_ID);
      const handshake = await connection.handshake();
      const missing = REQUIRED_CAPABILITIES.filter(
        (capability) => !handshake.capabilities.includes(capability)
      );
      if (missing.length > 0) {
        connection.close();
        connection = null;
        throw new Error(`HOST_PERMISSION_DENIED: ${missing.join(', ')}`);
      }
      removeThemeListener = connection.onThemeChange(onTheme);
      removeStreamListener = connection.onStreamEvent((event) => {
        for (const listener of streamListeners) listener(event);
      });
      return handshake.theme;
    },
    dispose() {
      removeThemeListener?.();
      removeStreamListener?.();
      removeThemeListener = null;
      removeStreamListener = null;
      streamListeners.clear();
      connection?.close();
      connection = null;
    }
  };
}

function hostMethod(serviceMethod: string): string {
  if (serviceMethod.startsWith('http.')) return 'host.http.request';
  if (serviceMethod.startsWith('stream.')) return 'host.http.stream';
  if (serviceMethod.startsWith('data.')) return 'host.http.data';
  throw new Error(`HTTP_SERVICE_METHOD_NOT_FOUND: ${serviceMethod}`);
}
