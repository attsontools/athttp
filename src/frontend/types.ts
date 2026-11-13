export type HttpMethod = 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE' | 'HEAD' | 'OPTIONS';
export type BodyType = 'none' | 'json' | 'form' | 'text' | 'multipart';
export type AuthType = 'none' | 'bearer' | 'basic';

export interface KV {
  key: string;
  value: string;
  enabled: boolean;
  description?: string;
}

export interface MultipartField {
  key: string;
  kind: 'text' | 'file';
  value: string;
  enabled: boolean;
}

export interface HttpAuth {
  type: AuthType;
  bearerToken?: string;
  basicUser?: string;
  basicPass?: string;
}

export interface HttpRequestSpec {
  method: HttpMethod;
  url: string;
  headers: KV[];
  queryParams: KV[];
  auth: HttpAuth;
  bodyType: BodyType;
  body: string;
  multipartFields: MultipartField[];
  timeoutSeconds: number;
  followRedirects: boolean;
  verifySsl: boolean;
  saveToHistory: boolean;
  /** 标记该条目为数据模型(OpenAPI schema),而非可发送请求 */
  metaKind?: 'schema';
}

export interface HttpResponseInfo {
  status: number;
  statusText: string;
  headers: Array<[string, string]>;
  body: string;
  bodyBytes: number;
  elapsedMs: number;
  finalUrl: string;
}

export type TabKind = 'http' | 'sse' | 'ws';
export type Direction = 'in' | 'out';
export type SessionStatus = 'idle' | 'connecting' | 'open' | 'closed' | 'error';

export interface SseSpec {
  url: string;
  headers: KV[];
  queryParams: KV[];
  auth: HttpAuth;
  timeoutSeconds?: number;
  verifySsl: boolean;
  lastEventId?: string;
}

export interface WsTemplate {
  name: string;
  text: string;
}

export interface WsSpec {
  url: string;
  headers: KV[];
  queryParams: KV[];
  auth: HttpAuth;
  verifySsl: boolean;
  subprotocols: string[];
  pingIntervalSeconds?: number;
  templates: WsTemplate[];
}

export type StreamMessage =
  | { kind: 'open'; atMs: number; status: number | null; headers: [string, string][] }
  | { kind: 'sseEvent'; atMs: number; event: string; data: string; id: string | null; retryMs: number | null; truncated: boolean }
  | { kind: 'wsText'; atMs: number; direction: Direction; text: string; truncated: boolean }
  | { kind: 'wsBinary'; atMs: number; direction: Direction; bytesLen: number; previewB64: string }
  | { kind: 'closed'; atMs: number; code: number | null; reason: string }
  | { kind: 'error'; atMs: number; message: string }
  | { kind: 'bufferTruncated'; atMs: number; dropped: number };

export interface HttpSession {
  status: SessionStatus;
  openedAt?: number;
  closedAt?: number;
  error?: string;
}

export function makeEmptySseSpec(): SseSpec {
  return {
    url: '',
    headers: [],
    queryParams: [],
    auth: { type: 'none' },
    timeoutSeconds: 30,
    verifySsl: true,
    lastEventId: undefined,
  };
}

export function makeEmptyWsSpec(): WsSpec {
  return {
    url: '',
    headers: [],
    queryParams: [],
    auth: { type: 'none' },
    verifySsl: true,
    subprotocols: [],
    pingIntervalSeconds: undefined,
    templates: [],
  };
}

export interface HttpTab {
  id: string;
  title: string;
  orderIndex: number;
  isActive: boolean;
  kind: TabKind;
  spec: HttpRequestSpec | SseSpec | WsSpec;
  lastResponse: HttpResponseInfo | null;
  lastError: string | null;
  sending: boolean;
  cancelTokenId?: string;
  session?: HttpSession;
  messages?: StreamMessage[];
}

export interface HttpHistoryItem {
  id: string;
  method: HttpMethod;
  url: string;
  status: number | null;
  elapsedMs: number | null;
  bodyBytes: number | null;
  spec: HttpRequestSpec;
  respSummary: string | null;
  createdAt: number;
}

export interface HttpEnv {
  id: string;
  name: string;
  isActive: boolean;
  orderIndex: number;
}

export interface HttpEnvVar {
  id: string;
  envId: string;
  key: string;
  value: string;
  enabled: boolean;
  orderIndex: number;
}

export interface HttpCollection {
  id: string;
  name: string;
  orderIndex: number;
  updatedAt: number;
  sourceUrl?: string | null;
  sourceHeaders?: KV[] | null;
  syncIntervalSecs?: number | null;
  lastSyncedAt?: number | null;
  lastSyncError?: string | null;
  baseUrl?: string | null;
}

export interface HttpCollectionFolder {
  id: string;
  collectionId: string;
  parentId: string | null;
  name: string;
  orderIndex: number;
  updatedAt: number;
}

export interface HttpCollectionRequest {
  id: string;
  collectionId: string;
  folderId: string | null;
  name: string;
  method: HttpMethod;
  spec: HttpRequestSpec;
  orderIndex: number;
  updatedAt: number;
  sourceKey?: string | null;
}

export function makeEmptySpec(): HttpRequestSpec {
  return {
    method: 'GET',
    url: '',
    headers: [],
    queryParams: [],
    auth: { type: 'none' },
    bodyType: 'none',
    body: '',
    multipartFields: [],
    timeoutSeconds: 30,
    followRedirects: true,
    verifySsl: true,
    saveToHistory: true
  };
}
