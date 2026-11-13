import { describe, expect, it, vi } from 'vitest';

import { createStreamApi } from './serviceApi';
import type { ServiceEvent, ServiceTransport } from './transport';
import type { StreamMessage, WsSpec } from './types';

const spec: WsSpec = {
  url: 'ws://127.0.0.1:18765/ws',
  headers: [],
  queryParams: [],
  auth: { type: 'none' },
  verifySsl: true,
  subprotocols: [],
  templates: []
};

describe('createStreamApi', () => {
  it('recovers stream events emitted before open returns its generation', async () => {
    const openMessage: StreamMessage = {
      kind: 'open',
      atMs: 1_000,
      status: null,
      headers: []
    };
    const listeners = new Set<(event: ServiceEvent) => void>();
    const transport: ServiceTransport = {
      async request<T>(method: string): Promise<T> {
        if (method === 'stream.open') {
          for (const listener of listeners) {
            listener({ sessionId: 'ws-1', generation: 7, message: openMessage });
          }
          return { generation: 7 } as T;
        }
        if (method === 'stream.snapshot') return [openMessage] as T;
        throw new Error(`unexpected method: ${method}`);
      },
      subscribe(listener) {
        listeners.add(listener);
        return () => listeners.delete(listener);
      },
      close: vi.fn()
    };
    const onMessage = vi.fn();
    const api = createStreamApi(transport);

    await api.listen('ws-1', onMessage);
    await api.openStream('ws-1', 'ws', spec);

    expect(onMessage).toHaveBeenCalledOnce();
    expect(onMessage).toHaveBeenCalledWith(openMessage);
  });
});
