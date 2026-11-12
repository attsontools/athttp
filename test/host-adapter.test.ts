import { describe, expect, it, vi } from 'vitest';

import { createPluginHost } from '../src/frontend/pluginHost';

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
