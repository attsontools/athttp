import type {
  HttpEnv,
  HttpEnvVar,
  HttpCollection,
  HttpCollectionFolder,
  HttpCollectionRequest,
  HttpHistoryItem,
  HttpRequestSpec,
  HttpResponseInfo,
  HttpTab,
  KV,
  TabKind
} from './types';
import type { ServiceTransport } from './transport';
import type { SseSpec, StreamMessage, WsSpec } from './types';

interface HttpTabRow {
  id: string;
  title: string;
  orderIndex: number;
  isActive: boolean;
  kind: string;
  specJson: string;
  updatedAt: number;
}

interface HttpHistoryRow {
  id: string;
  method: string;
  url: string;
  status: number | null;
  elapsedMs: number | null;
  bodyBytes: number | null;
  specJson: string;
  respSummary: string | null;
  createdAt: number;
}

interface HttpEnvRow {
  id: string;
  name: string;
  isActive: boolean;
  orderIndex: number;
  updatedAt: number;
}

interface HttpEnvVarRow {
  id: string;
  envId: string;
  key: string;
  value: string;
  enabled: boolean;
  orderIndex: number;
  updatedAt: number;
}

interface HttpCollectionRow {
  id: string;
  name: string;
  orderIndex: number;
  updatedAt: number;
  sourceUrl: string | null;
  sourceHeadersJson: string | null;
  syncIntervalSecs: number | null;
  lastSyncedAt: number | null;
  lastSyncError: string | null;
  baseUrl: string | null;
}

interface HttpCollectionFolderRow {
  id: string;
  collectionId: string;
  parentId: string | null;
  name: string;
  orderIndex: number;
  updatedAt: number;
}

interface HttpCollectionRequestRow {
  id: string;
  collectionId: string;
  folderId: string | null;
  name: string;
  method: string;
  specJson: string;
  orderIndex: number;
  updatedAt: number;
  sourceKey: string | null;
}

function tabFromRow(row: HttpTabRow): HttpTab {
  return {
    id: row.id,
    title: row.title,
    orderIndex: row.orderIndex,
    isActive: row.isActive,
    kind: (row.kind as TabKind) ?? 'http',
    spec: JSON.parse(row.specJson),
    lastResponse: null,
    lastError: null,
    sending: false
  };
}

function tabToRow(tab: HttpTab): HttpTabRow {
  return {
    id: tab.id,
    title: tab.title,
    orderIndex: tab.orderIndex,
    isActive: tab.isActive,
    kind: tab.kind,
    specJson: JSON.stringify(tab.spec),
    updatedAt: Date.now()
  };
}

function historyFromRow(row: HttpHistoryRow): HttpHistoryItem {
  return {
    id: row.id,
    method: row.method as HttpHistoryItem['method'],
    url: row.url,
    status: row.status,
    elapsedMs: row.elapsedMs,
    bodyBytes: row.bodyBytes,
    spec: JSON.parse(row.specJson) as HttpRequestSpec,
    respSummary: row.respSummary,
    createdAt: row.createdAt
  };
}

function historyToRow(item: HttpHistoryItem): HttpHistoryRow {
  return {
    id: item.id,
    method: item.method,
    url: item.url,
    status: item.status,
    elapsedMs: item.elapsedMs,
    bodyBytes: item.bodyBytes,
    specJson: JSON.stringify(item.spec),
    respSummary: item.respSummary,
    createdAt: item.createdAt
  };
}

function envFromRow(row: HttpEnvRow): HttpEnv {
  return { id: row.id, name: row.name, isActive: row.isActive, orderIndex: row.orderIndex };
}

function envVarFromRow(row: HttpEnvVarRow): HttpEnvVar {
  return {
    id: row.id,
    envId: row.envId,
    key: row.key,
    value: row.value,
    enabled: row.enabled,
    orderIndex: row.orderIndex
  };
}

function collectionFromRow(row: HttpCollectionRow): HttpCollection {
  let sourceHeaders: KV[] | null = null;
  if (row.sourceHeadersJson) {
    try { sourceHeaders = JSON.parse(row.sourceHeadersJson) as KV[]; } catch { sourceHeaders = null; }
  }
  return {
    id: row.id,
    name: row.name,
    orderIndex: row.orderIndex,
    updatedAt: row.updatedAt,
    sourceUrl: row.sourceUrl,
    sourceHeaders,
    syncIntervalSecs: row.syncIntervalSecs,
    lastSyncedAt: row.lastSyncedAt,
    lastSyncError: row.lastSyncError,
    baseUrl: row.baseUrl
  };
}

function collectionToRow(c: HttpCollection): HttpCollectionRow {
  return {
    id: c.id,
    name: c.name,
    orderIndex: c.orderIndex,
    updatedAt: c.updatedAt,
    sourceUrl: c.sourceUrl ?? null,
    sourceHeadersJson: c.sourceHeaders ? JSON.stringify(c.sourceHeaders) : null,
    syncIntervalSecs: c.syncIntervalSecs ?? null,
    lastSyncedAt: c.lastSyncedAt ?? null,
    lastSyncError: c.lastSyncError ?? null,
    baseUrl: c.baseUrl ?? null
  };
}

function folderFromRow(row: HttpCollectionFolderRow): HttpCollectionFolder {
  return {
    id: row.id,
    collectionId: row.collectionId,
    parentId: row.parentId,
    name: row.name,
    orderIndex: row.orderIndex,
    updatedAt: row.updatedAt
  };
}

function requestFromRow(row: HttpCollectionRequestRow): HttpCollectionRequest {
  return {
    id: row.id,
    collectionId: row.collectionId,
    folderId: row.folderId,
    name: row.name,
    method: row.method as HttpCollectionRequest['method'],
    spec: JSON.parse(row.specJson) as HttpRequestSpec,
    orderIndex: row.orderIndex,
    updatedAt: row.updatedAt,
    sourceKey: row.sourceKey
  };
}

function requestToRow(row: HttpCollectionRequest): HttpCollectionRequestRow {
  return {
    id: row.id,
    collectionId: row.collectionId,
    folderId: row.folderId,
    name: row.name,
    method: row.method,
    specJson: JSON.stringify(row.spec),
    orderIndex: row.orderIndex,
    updatedAt: row.updatedAt,
    sourceKey: row.sourceKey ?? null
  };
}

export interface HttpApi {
  sendHttp(spec: HttpRequestSpec, cancelTokenId?: string): Promise<HttpResponseInfo>;
  cancelHttp(cancelTokenId: string): Promise<boolean>;

  listTabs(): Promise<HttpTab[]>;
  upsertTab(tab: HttpTab): Promise<void>;
  deleteTab(id: string): Promise<void>;
  setActiveTab(id: string): Promise<void>;

  listHistory(limit?: number): Promise<HttpHistoryItem[]>;
  insertHistory(item: HttpHistoryItem): Promise<void>;
  deleteHistory(id: string): Promise<void>;
  clearHistory(): Promise<void>;

  listEnvs(): Promise<HttpEnv[]>;
  upsertEnv(env: HttpEnv): Promise<void>;
  deleteEnv(id: string): Promise<void>;
  setActiveEnv(id: string): Promise<void>;

  listEnvVars(envId: string): Promise<HttpEnvVar[]>;
  upsertEnvVar(v: HttpEnvVar): Promise<void>;
  deleteEnvVar(id: string): Promise<void>;

  listCollections(): Promise<HttpCollection[]>;
  listCollectionFolders(): Promise<HttpCollectionFolder[]>;
  listCollectionRequests(): Promise<HttpCollectionRequest[]>;
  upsertCollection(row: HttpCollection): Promise<void>;
  upsertCollectionFolder(row: HttpCollectionFolder): Promise<void>;
  upsertCollectionRequest(row: HttpCollectionRequest): Promise<void>;
  deleteCollection(id: string): Promise<void>;
  deleteCollectionRequest(id: string): Promise<void>;
  deleteCollectionFolder(id: string): Promise<void>;
  fetchOpenApiUrl(url: string, headers: Array<{ key: string; value: string }>): Promise<string>;
}

export function createHttpApi(transport: ServiceTransport): HttpApi {
  return {
    async sendHttp(spec, cancelTokenId) {
      return await transport.request<HttpResponseInfo>('http.send', {
        request: spec,
        cancelTokenId: cancelTokenId ?? null
      });
    },
    async cancelHttp(cancelTokenId) {
      const result = await transport.request<{ cancelled: boolean }>('http.cancel', { cancelTokenId });
      return result.cancelled;
    },

    async listTabs() {
      const rows = await transport.request<HttpTabRow[]>('data.tabs.list');
      return rows.map(tabFromRow);
    },
    async upsertTab(tab) {
      await transport.request('data.tabs.upsert', { row: tabToRow(tab) });
    },
    async deleteTab(id) {
      await transport.request('data.tabs.delete', { id });
    },
    async setActiveTab(id) {
      await transport.request('data.tabs.setActive', { id });
    },

    async listHistory(limit) {
      const rows = await transport.request<HttpHistoryRow[]>('data.history.list', { limit: limit ?? 500 });
      return rows.map(historyFromRow);
    },
    async insertHistory(item) {
      await transport.request('data.history.insert', { row: historyToRow(item) });
    },
    async deleteHistory(id) {
      await transport.request('data.history.delete', { id });
    },
    async clearHistory() {
      await transport.request('data.history.clear');
    },

    async listEnvs() {
      const rows = await transport.request<HttpEnvRow[]>('data.envs.list');
      return rows.map(envFromRow);
    },
    async upsertEnv(env) {
      await transport.request('data.envs.upsert', {
        row: {
          id: env.id,
          name: env.name,
          isActive: env.isActive,
          orderIndex: env.orderIndex,
          updatedAt: Date.now()
        }
      });
    },
    async deleteEnv(id) {
      await transport.request('data.envs.delete', { id });
    },
    async setActiveEnv(id) {
      await transport.request('data.envs.setActive', { id });
    },

    async listEnvVars(envId) {
      const rows = await transport.request<HttpEnvVarRow[]>('data.envs.vars.list', { envId });
      return rows.map(envVarFromRow);
    },
    async upsertEnvVar(v) {
      await transport.request('data.envs.vars.upsert', {
        row: {
          id: v.id,
          envId: v.envId,
          key: v.key,
          value: v.value,
          enabled: v.enabled,
          orderIndex: v.orderIndex,
          updatedAt: Date.now()
        }
      });
    },
    async deleteEnvVar(id) {
      await transport.request('data.envs.vars.delete', { id });
    },

    async listCollections() {
      const rows = await transport.request<HttpCollectionRow[]>('data.collections.list');
      return rows.map(collectionFromRow);
    },
    async listCollectionFolders() {
      const rows = await transport.request<HttpCollectionFolderRow[]>('data.collections.folders.list');
      return rows.map(folderFromRow);
    },
    async listCollectionRequests() {
      const rows = await transport.request<HttpCollectionRequestRow[]>('data.collections.requests.list');
      return rows.map(requestFromRow);
    },
    async upsertCollection(row) {
      await transport.request('data.collections.upsert', { row: collectionToRow(row) });
    },
    async upsertCollectionFolder(row) {
      await transport.request('data.collections.folders.upsert', { row });
    },
    async upsertCollectionRequest(row) {
      await transport.request('data.collections.requests.upsert', { row: requestToRow(row) });
    },
    async deleteCollection(id) {
      await transport.request('data.collections.delete', { id });
    },
    async deleteCollectionRequest(id) {
      await transport.request('data.collections.requests.delete', { id });
    },
    async deleteCollectionFolder(id) {
      await transport.request('data.collections.folders.delete', { id });
    },
    async fetchOpenApiUrl(url, headers) {
      return await transport.request<string>('data.openapi.import', { url, headers });
    }
  };
}

export interface StreamApi {
  openStream(sessionId: string, kind: 'sse' | 'ws', spec: SseSpec | WsSpec): Promise<void>;
  closeStream(sessionId: string): Promise<void>;
  sendWsMessage(sessionId: string, text: string): Promise<void>;
  listStreamMessages(sessionId: string): Promise<StreamMessage[]>;
  listen(sessionId: string, onMessage: (message: StreamMessage) => void): Promise<() => void>;
}

export function createStreamApi(transport: ServiceTransport): StreamApi {
  const generations = new Map<string, number>();
  return {
    async openStream(sessionId, kind, spec) {
      const result = await transport.request<{ generation: number }>('stream.open', {
        sessionId,
        kind,
        spec
      });
      generations.set(sessionId, result.generation);
    },
    async closeStream(sessionId) {
      await transport.request('stream.close', { sessionId });
    },
    async sendWsMessage(sessionId, text) {
      await transport.request('stream.send', { sessionId, text });
    },
    async listStreamMessages(sessionId) {
      return await transport.request<StreamMessage[]>('stream.snapshot', { sessionId });
    },
    async listen(sessionId, onMessage) {
      return transport.subscribe((event) => {
        if (event.sessionId !== sessionId) return;
        if (event.generation !== generations.get(sessionId)) return;
        onMessage(event.message);
      });
    }
  };
}
