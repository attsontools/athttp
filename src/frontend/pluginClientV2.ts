import type { ServiceEvent } from './transport';

const API_VERSION = '2.0' as const;
const MAX_MESSAGE_BYTES = 8 * 1024 * 1024;

export interface RpcMessageV2 {
  jsonrpc: '2.0';
  id?: string;
  apiVersion: typeof API_VERSION;
  appId: string;
  method?: string;
  params?: unknown;
  result?: unknown;
  error?: {
    code: number;
    message: string;
    data?: { code?: string };
  };
}

interface MessagePortLike {
  onmessage: ((event: MessageEvent<RpcMessageV2>) => void) | null;
  postMessage(message: RpcMessageV2): void;
  start(): void;
  close(): void;
}

interface HostHandshake {
  apiVersion: typeof API_VERSION;
  sessionId: string;
  correlationId: string;
  capabilities: string[];
  metadata: { appId: string; name: string; version: string };
  theme: 'dark' | 'light';
}

type Pending = {
  resolve(value: unknown): void;
  reject(error: Error): void;
  timer: ReturnType<typeof setTimeout>;
};

export class HostRpcError extends Error {
  constructor(
    readonly code: string,
    message: string,
    readonly rpcCode?: number
  ) {
    super(message);
    this.name = 'HostRpcError';
  }
}

export function createPluginClientV2(
  port: MessagePortLike,
  options: { appId: string; timeoutMs?: number }
) {
  const timeoutMs = options.timeoutMs ?? 10_000;
  const pending = new Map<string, Pending>();
  const themeListeners = new Set<(theme: 'dark' | 'light') => void>();
  const streamListeners = new Set<(event: ServiceEvent) => void>();
  let nextId = 1;
  let closed = false;

  function send(message: RpcMessageV2) {
    if (messageBytes(message) > MAX_MESSAGE_BYTES) {
      throw new HostRpcError('HOST_MESSAGE_TOO_LARGE', 'Host API message is too large');
    }
    port.postMessage(message);
  }

  function call<T>(method: string, params: unknown = {}): Promise<T> {
    if (closed) return Promise.reject(new HostRpcError('HOST_SESSION_CLOSED', 'Host API session is closed'));
    const id = String(nextId++);
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        pending.delete(id);
        reject(new HostRpcError('HOST_TIMEOUT', `Host API request timed out: ${method}`));
      }, requestTimeoutMs(method, params, timeoutMs));
      pending.set(id, { resolve: resolve as (value: unknown) => void, reject, timer });
      try {
        send({
          jsonrpc: '2.0',
          id,
          apiVersion: API_VERSION,
          appId: options.appId,
          method,
          params
        });
      } catch (error) {
        pending.delete(id);
        clearTimeout(timer);
        reject(error);
      }
    });
  }

  port.onmessage = ({ data: message }) => {
    if (
      messageBytes(message) > MAX_MESSAGE_BYTES
      || message.jsonrpc !== '2.0'
      || message.apiVersion !== API_VERSION
      || message.appId !== options.appId
    ) return;
    if (message.id && ('result' in message || 'error' in message)) {
      const active = pending.get(message.id);
      if (!active) return;
      pending.delete(message.id);
      clearTimeout(active.timer);
      if (message.error) {
        active.reject(new HostRpcError(
          message.error.data?.code ?? 'HOST_RPC_ERROR',
          message.error.message,
          message.error.code
        ));
      } else {
        active.resolve(message.result);
      }
      return;
    }
    if (message.method === 'host.theme.changed') {
      const theme = (message.params as { theme?: unknown } | undefined)?.theme;
      if (theme === 'dark' || theme === 'light') {
        for (const listener of themeListeners) listener(theme);
      }
      return;
    }
    if (message.method === 'host.http.streamEvent' && isServiceEvent(message.params)) {
      for (const listener of streamListeners) listener(message.params);
    }
  };
  port.start();

  return {
    handshake: async () => validateHandshake(await call<unknown>('host.handshake'), options.appId),
    call,
    onThemeChange(listener: (theme: 'dark' | 'light') => void) {
      themeListeners.add(listener);
      return () => themeListeners.delete(listener);
    },
    onStreamEvent(listener: (event: ServiceEvent) => void) {
      streamListeners.add(listener);
      return () => streamListeners.delete(listener);
    },
    close() {
      if (closed) return;
      closed = true;
      port.onmessage = null;
      port.close();
      for (const request of pending.values()) {
        clearTimeout(request.timer);
        request.reject(new HostRpcError('HOST_SESSION_CLOSED', 'Host API session is closed'));
      }
      pending.clear();
      themeListeners.clear();
      streamListeners.clear();
    }
  };
}

function requestTimeoutMs(method: string, params: unknown, fallback: number) {
  if (method !== 'host.http.request' || typeof params !== 'object' || params === null) return fallback;
  const serviceRequest = params as { method?: unknown; params?: unknown };
  if (serviceRequest.method !== 'http.send'
    || typeof serviceRequest.params !== 'object'
    || serviceRequest.params === null) return fallback;
  const request = (serviceRequest.params as { request?: unknown }).request;
  if (typeof request !== 'object' || request === null) return fallback;
  const seconds = (request as { timeoutSeconds?: unknown }).timeoutSeconds;
  if (typeof seconds !== 'number' || !Number.isFinite(seconds)) return fallback;
  return Math.min(300, Math.max(1, seconds)) * 1000 + 5_000;
}

export async function connectPluginV2(
  appId: string,
  options: { timeoutMs?: number; targetWindow?: Window } = {}
) {
  const targetWindow = options.targetWindow ?? window;
  const timeoutMs = options.timeoutMs ?? 10_000;
  return new Promise<ReturnType<typeof createPluginClientV2>>((resolve, reject) => {
    const timer = setTimeout(() => {
      targetWindow.removeEventListener('message', onMessage);
      reject(new HostRpcError('HOST_CONNECT_TIMEOUT', 'Host connection timed out'));
    }, timeoutMs);
    const onMessage = (event: MessageEvent) => {
      const data = event.data as { type?: unknown; appId?: unknown; apiVersion?: unknown };
      if (
        event.source !== targetWindow.parent
        || data?.type !== 'attool:connect'
        || data.appId !== appId
        || data.apiVersion !== API_VERSION
        || !event.ports[0]
      ) return;
      clearTimeout(timer);
      targetWindow.removeEventListener('message', onMessage);
      resolve(createPluginClientV2(event.ports[0], { appId, timeoutMs }));
    };
    targetWindow.addEventListener('message', onMessage);
  });
}

function validateHandshake(value: unknown, appId: string): HostHandshake {
  if (typeof value !== 'object' || value === null) throw invalidHandshake();
  const handshake = value as Partial<HostHandshake>;
  if (
    handshake.apiVersion !== API_VERSION
    || typeof handshake.sessionId !== 'string'
    || typeof handshake.correlationId !== 'string'
    || !Array.isArray(handshake.capabilities)
    || handshake.capabilities.some((capability) => typeof capability !== 'string')
    || (handshake.theme !== 'dark' && handshake.theme !== 'light')
    || typeof handshake.metadata !== 'object'
    || handshake.metadata === null
    || handshake.metadata.appId !== appId
  ) throw invalidHandshake();
  return handshake as HostHandshake;
}

function invalidHandshake() {
  return new HostRpcError('HOST_HANDSHAKE_INVALID', 'Host handshake response is invalid');
}

function isServiceEvent(value: unknown): value is ServiceEvent {
  if (typeof value !== 'object' || value === null) return false;
  const event = value as Partial<ServiceEvent>;
  return typeof event.sessionId === 'string'
    && typeof event.generation === 'number'
    && typeof event.message === 'object'
    && event.message !== null;
}

function messageBytes(value: unknown) {
  try {
    return new TextEncoder().encode(JSON.stringify(value)).byteLength;
  } catch {
    return MAX_MESSAGE_BYTES + 1;
  }
}
