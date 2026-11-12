import { describe, expect, it, vi } from 'vitest';

import { createPluginHost } from '../src/frontend/pluginHost';
import { createStandaloneHost } from '../src/frontend/standaloneHost';
import { formatServiceError } from '../src/frontend/transport';

describe('plugin host adapter', () => {
  it('handshakes with the HTTP app id and routes service methods through declared Host APIs', async () => {
    const call = vi.fn(async (_method: string, _params?: unknown) => null);
    const close = vi.fn();
    const connect = vi.fn(async () => ({
      handshake: async () => ({
        theme: 'dark' as const,
        capabilities: ['host.http.request', 'host.http.stream', 'host.http.data']
      }),
      async call<T>(method: string, params?: unknown): Promise<T> {
        return await call(method, params) as T;
      },
      onThemeChange: () => vi.fn(),
      onStreamEvent: () => vi.fn(),
      close
    }));
    const host = createPluginHost(connect);

    await expect(host.initialize(vi.fn())).resolves.toBe('dark');
    expect(connect).toHaveBeenCalledWith('com.attson.http');

    await host.transport.request('http.send', { request: { method: 'GET' } });
    await host.transport.request('data.tabs.list', {});
    await host.transport.request('stream.snapshot', { sessionId: 'session-1' });

    expect(call).toHaveBeenNthCalledWith(1, 'host.http.request', {
      method: 'http.send',
      params: { request: { method: 'GET' } }
    });
    expect(call).toHaveBeenNthCalledWith(2, 'host.http.data', {
      method: 'data.tabs.list',
      params: {}
    });
    expect(call).toHaveBeenNthCalledWith(3, 'host.http.stream', {
      method: 'stream.snapshot',
      params: { sessionId: 'session-1' }
    });

    host.dispose();
    expect(close).toHaveBeenCalledOnce();
  });

  it('rejects a host that did not grant every HTTP capability', async () => {
    const host = createPluginHost(async () => ({
      handshake: async () => ({
        theme: 'dark' as const,
        capabilities: ['host.http.request', 'host.http.data']
      }),
      async call<T>(): Promise<T> { return null as T; },
      onThemeChange: () => vi.fn(),
      onStreamEvent: () => vi.fn(),
      close: vi.fn()
    }));

    await expect(host.initialize(vi.fn())).rejects.toThrow('HOST_PERMISSION_DENIED');
  });
});

describe('standalone host adapter', () => {
  it('formats structured service errors for the unavailable state', () => {
    expect(formatServiceError({
      code: 'HTTP_SERVICE_VERSION_UNSUPPORTED',
      message: 'service version mismatch',
      retryable: false
    })).toBe('HTTP_SERVICE_VERSION_UNSUPPORTED: service version mismatch');
    expect(formatServiceError(new Error('network unavailable'))).toBe('network unavailable');
  });

  it('connects once and forwards service requests and events through Tauri commands', async () => {
    let serviceListener: ((event: { payload: unknown }) => void) | undefined;
    const remove = vi.fn();
    const invoke = vi.fn(async (command: string, _args?: unknown): Promise<unknown> => {
      if (command === 'http_service_request') return { ok: true };
      return null;
    });
    const host = createStandaloneHost({
      async invoke<T>(command: string, args?: unknown): Promise<T> {
        return await invoke(command, args) as T;
      },
      listen: vi.fn(async (_event, listener) => {
        serviceListener = listener;
        return remove;
      }),
      storage: { getItem: () => 'light' }
    });
    const onTheme = vi.fn();
    const onEvent = vi.fn();

    await expect(host.initialize(onTheme)).resolves.toBe('light');
    expect(invoke).toHaveBeenCalledWith('connect_http_service', undefined);
    expect(onTheme).toHaveBeenCalledWith('light');

    const unsubscribe = host.transport.subscribe(onEvent);
    await host.transport.request('data.tabs.list', {});
    expect(invoke).toHaveBeenLastCalledWith('http_service_request', {
      method: 'data.tabs.list',
      params: {}
    });

    serviceListener?.({
      payload: { sessionId: 's1', generation: 1, message: { kind: 'open' } }
    });
    expect(onEvent).toHaveBeenCalledWith(expect.objectContaining({ sessionId: 's1' }));
    unsubscribe();
    await Promise.resolve();
    expect(remove).toHaveBeenCalledOnce();
  });
});
