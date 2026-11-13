import { describe, expect, it, vi } from 'vitest';

import { createPluginClientV2, type RpcMessageV2 } from '../src/frontend/pluginClientV2';

class FakePort {
  onmessage: ((event: MessageEvent<RpcMessageV2>) => void) | null = null;
  readonly sent: RpcMessageV2[] = [];
  start = vi.fn();
  close = vi.fn();

  postMessage(message: RpcMessageV2) {
    this.sent.push(message);
  }

  receive(message: RpcMessageV2) {
    this.onmessage?.({ data: message } as MessageEvent<RpcMessageV2>);
  }
}

describe('Host API 2 plugin client', () => {
  it('handshakes with API 2 and forwards HTTP stream events', async () => {
    const port = new FakePort();
    const client = createPluginClientV2(port, { appId: 'com.attson.http' });
    const handshake = client.handshake();
    const request = port.sent[0];

    expect(request).toMatchObject({
      jsonrpc: '2.0',
      apiVersion: '2.0',
      appId: 'com.attson.http',
      method: 'host.handshake'
    });
    port.receive({
      jsonrpc: '2.0',
      id: request.id!,
      apiVersion: '2.0',
      appId: 'com.attson.http',
      result: {
        apiVersion: '2.0',
        sessionId: 'session-1',
        correlationId: 'correlation-1',
        capabilities: ['host.http.request', 'host.http.stream', 'host.http.data'],
        metadata: { appId: 'com.attson.http', name: 'HTTP 请求', version: '0.1.0-alpha.0' },
        theme: 'dark'
      }
    });
    await expect(handshake).resolves.toMatchObject({ apiVersion: '2.0', theme: 'dark' });

    const onEvent = vi.fn();
    client.onStreamEvent(onEvent);
    port.receive({
      jsonrpc: '2.0',
      apiVersion: '2.0',
      appId: 'com.attson.http',
      method: 'host.http.streamEvent',
      params: { sessionId: 'stream-1', generation: 2, message: { kind: 'open' } }
    });

    expect(onEvent).toHaveBeenCalledWith({
      sessionId: 'stream-1',
      generation: 2,
      message: { kind: 'open' }
    });
  });

  it('accepts stream payloads up to the shared five MiB buffer contract', () => {
    const port = new FakePort();
    const client = createPluginClientV2(port, { appId: 'com.attson.http' });
    const onEvent = vi.fn();
    client.onStreamEvent(onEvent);

    port.receive({
      jsonrpc: '2.0',
      apiVersion: '2.0',
      appId: 'com.attson.http',
      method: 'host.http.streamEvent',
      params: {
        sessionId: 'stream-1',
        generation: 2,
        message: { kind: 'wsText', text: 'x'.repeat(5 * 1024 * 1024) }
      }
    });

    expect(onEvent).toHaveBeenCalledOnce();
  });
});
