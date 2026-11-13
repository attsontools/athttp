import { describe, expect, it, vi } from 'vitest';

import { createHttpApi, createStreamApi } from '../src/frontend/serviceApi';
import type { HttpTab, StreamMessage } from '../src/frontend/types';
import type { ServiceTransport } from '../src/frontend/transport';

function makeTransport() {
  const listeners = new Set<(event: Parameters<ServiceTransport['subscribe']>[0] extends (event: infer T) => void ? T : never) => void>();
  const request = vi.fn(async (method: string, _params?: unknown): Promise<unknown> => {
    if (method === 'data.tabs.list') {
      return [{
        id: 'tab-1',
        title: 'Users',
        orderIndex: 0,
        isActive: true,
        kind: 'http',
        specJson: '{"method":"GET","url":"https://example.com"}',
        updatedAt: 1
      }];
    }
    if (method === 'http.cancel') return { cancelled: true };
    if (method === 'stream.open') {
      const calls = request.mock.calls.filter(([name]) => name === 'stream.open').length;
      return { generation: calls };
    }
    if (method === 'stream.snapshot') return [];
    return null;
  });
  const transport: ServiceTransport = {
    async request<T>(method: string, params?: unknown): Promise<T> {
      return await request(method, params) as T;
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    close: vi.fn()
  };
  return {
    request,
    transport,
    emit(event: Parameters<typeof listeners.add>[0] extends (event: infer T) => void ? T : never) {
      for (const listener of listeners) listener(event);
    }
  };
}

describe('service API adapters', () => {
  it('maps the persisted tab contract to the HTTP workspace model', async () => {
    const { request, transport } = makeTransport();
    const api = createHttpApi(transport);

    await expect(api.listTabs()).resolves.toEqual([
      expect.objectContaining({
        id: 'tab-1',
        title: 'Users',
        kind: 'http',
        spec: { method: 'GET', url: 'https://example.com' },
        sending: false
      })
    ]);

    const tab = (await api.listTabs())[0] as HttpTab;
    await api.upsertTab(tab);
    expect(request).toHaveBeenLastCalledWith('data.tabs.upsert', {
      row: expect.objectContaining({
        id: 'tab-1',
        kind: 'http',
        specJson: '{"method":"GET","url":"https://example.com"}'
      })
    });
  });

  it('unwraps service cancellation results', async () => {
    const { request, transport } = makeTransport();
    const api = createHttpApi(transport);

    await expect(api.cancelHttp('cancel-1')).resolves.toBe(true);
    expect(request).toHaveBeenCalledWith('http.cancel', { cancelTokenId: 'cancel-1' });
  });

  it('delivers only the current stream generation to a session listener', async () => {
    const { transport, emit } = makeTransport();
    const api = createStreamApi(transport);
    const onMessage = vi.fn<(message: StreamMessage) => void>();

    await api.openStream('session-1', 'sse', {
      url: 'https://example.com/events',
      headers: [],
      queryParams: [],
      auth: { type: 'none' },
      timeoutSeconds: 0,
      verifySsl: true,
      lastEventId: ''
    });
    const unlisten = await api.listen('session-1', onMessage);

    emit({
      sessionId: 'session-1',
      generation: 1,
      message: { kind: 'sseEvent', atMs: 1, event: '', data: 'first', id: null, retryMs: null, truncated: false }
    });
    await api.openStream('session-1', 'sse', {
      url: 'https://example.com/events',
      headers: [],
      queryParams: [],
      auth: { type: 'none' },
      timeoutSeconds: 0,
      verifySsl: true,
      lastEventId: ''
    });
    emit({
      sessionId: 'session-1',
      generation: 1,
      message: { kind: 'sseEvent', atMs: 2, event: '', data: 'stale', id: null, retryMs: null, truncated: false }
    });
    emit({
      sessionId: 'session-1',
      generation: 2,
      message: { kind: 'sseEvent', atMs: 3, event: '', data: 'current', id: null, retryMs: null, truncated: false }
    });

    expect(onMessage).toHaveBeenCalledTimes(2);
    expect(onMessage).toHaveBeenLastCalledWith(expect.objectContaining({ data: 'current' }));
    unlisten();
  });
});
