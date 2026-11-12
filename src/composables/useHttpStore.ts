import { computed, reactive, ref, watch } from 'vue';
import type {
  HttpEnv,
  HttpEnvVar,
  HttpCollection,
  HttpCollectionFolder,
  HttpCollectionRequest,
  HttpHistoryItem,
  HttpRequestSpec,
  HttpResponseInfo,
  HttpSession,
  HttpTab,
  SseSpec,
  StreamMessage,
  TabKind,
  WsSpec
} from '../components/http/types';
import { makeEmptySpec, makeEmptySseSpec, makeEmptyWsSpec } from '../components/http/types';
import { applyVarsToSpec, applyVarsToSseSpec, applyVarsToWsSpec, makeVarContext, resolveVars } from '../components/http/variables';
import { createHttpApi, type HttpApi } from '../components/http/httpApi';
import { createStreamApi, type StreamApi } from '../components/http/streamApi';
import type { ServiceTransport } from '../frontend/transport';
import { parseOpenApiToCollection, type ImportedOpenApiCollection } from '../components/http/openapiImport';
import { mergeIntoCollection } from '../components/http/mergeIntoCollection';

function ulid(): string {
  // 简易 ID：时间戳 + 随机后缀。不追求严格 ULID 结构，够用即可
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

const DEBOUNCE_MS = 300;
const HISTORY_LIMIT = 500;
const RESP_SUMMARY_MAX = 4096;

interface HttpState {
  tabs: HttpTab[];
  activeTabId: string | null;
  history: HttpHistoryItem[];
  envs: HttpEnv[];
  activeEnvId: string | null;
  activeEnvVars: HttpEnvVar[];
  globalVars: HttpEnvVar[];
  collections: HttpCollection[];
  collectionFolders: HttpCollectionFolder[];
  collectionRequests: HttpCollectionRequest[];
  ready: boolean;
}

let _singleton: ReturnType<typeof createStore> | null = null;

type FullApi = HttpApi & {
  openStream: StreamApi['openStream'];
  closeStream: StreamApi['closeStream'];
  sendWsMessage: StreamApi['sendWsMessage'];
  listStreamMessages: StreamApi['listStreamMessages'];
  listen: StreamApi['listen'];
};

function createStore(api: FullApi) {
  const state = reactive<HttpState>({
    tabs: [],
    activeTabId: null,
    history: [],
    envs: [],
    activeEnvId: null,
    activeEnvVars: [],
    globalVars: [],
    collections: [],
    collectionFolders: [],
    collectionRequests: [],
    ready: false
  });

  const initError = ref<string | null>(null);
  const dirtyTabs = new Set<string>();
  let flushTimer: ReturnType<typeof setTimeout> | null = null;
  let suppressWatch = false;

  const activeTab = computed<HttpTab | null>(() => {
    return state.tabs.find((t) => t.id === state.activeTabId) ?? null;
  });

  const varContext = computed(() =>
    makeVarContext(state.activeEnvVars, state.globalVars)
  );

  async function init() {
    try {
      const [tabs, history, envs, globalVars, collections, collectionFolders, collectionRequests] = await Promise.all([
        api.listTabs(),
        api.listHistory(HISTORY_LIMIT),
        api.listEnvs(),
        api.listEnvVars(''),
        api.listCollections(),
        api.listCollectionFolders(),
        api.listCollectionRequests()
      ]);
      suppressWatch = true;
      state.tabs = tabs.length > 0 ? tabs : [makeDefaultTab()];
      for (const t of state.tabs) {
        if (!t.kind) t.kind = 'http';
        if (t.kind !== 'http' && !t.session) t.session = { status: 'idle' };
      }
      state.activeTabId =
        state.tabs.find((t) => t.isActive)?.id ?? state.tabs[0]?.id ?? null;
      if (!state.tabs.some((t) => t.isActive) && state.activeTabId) {
        const t = state.tabs.find((x) => x.id === state.activeTabId);
        if (t) t.isActive = true;
      }
      state.history = history;
      state.envs = envs;
      state.globalVars = globalVars;
      state.collections = collections;
      state.collectionFolders = collectionFolders;
      state.collectionRequests = collectionRequests;
      const active = envs.find((e) => e.isActive);
      state.activeEnvId = active?.id ?? null;
      state.activeEnvVars = active ? await api.listEnvVars(active.id) : [];
      state.ready = true;
      startSyncScheduler();
      // 保证初始 tab 落库
      for (const t of state.tabs) {
        await api.upsertTab(t).catch(() => {});
      }
    } catch (err) {
      initError.value = String(err);
    } finally {
      suppressWatch = false;
    }
  }

  function makeDefaultTab(): HttpTab {
    return {
      id: ulid(),
      title: '新请求',
      orderIndex: 0,
      isActive: true,
      kind: 'http',
      spec: makeEmptySpec(),
      lastResponse: null,
      lastError: null,
      sending: false
    };
  }

  function markDirty(id: string) {
    dirtyTabs.add(id);
    if (flushTimer) return;
    flushTimer = setTimeout(flushDirty, DEBOUNCE_MS);
  }

  async function flushDirty() {
    flushTimer = null;
    const ids = Array.from(dirtyTabs);
    dirtyTabs.clear();
    for (const id of ids) {
      const tab = state.tabs.find((t) => t.id === id);
      if (tab) {
        try {
          await api.upsertTab(tab);
        } catch (err) {
          console.warn('[http] upsert tab failed', err);
        }
      }
    }
  }

  async function flushDirtyNow() {
    if (flushTimer) {
      clearTimeout(flushTimer);
      flushTimer = null;
    }
    await flushDirty();
  }

  // 深度 watch tabs -> mark dirty
  watch(
    () => state.tabs,
    (newTabs) => {
      if (suppressWatch) return;
      for (const t of newTabs) markDirty(t.id);
    },
    { deep: true }
  );

  // ---- tabs API ----

  const streamUnlisten = new Map<string, () => void>();

  async function newTab(spec?: HttpRequestSpec | SseSpec | WsSpec, title?: string, kind: TabKind = 'http'): Promise<HttpTab> {
    const tab: HttpTab = {
      id: ulid(),
      title: title ?? '新请求',
      orderIndex: state.tabs.length,
      isActive: false,
      kind,
      spec: spec ?? (kind === 'sse' ? makeEmptySseSpec() : kind === 'ws' ? makeEmptyWsSpec() : makeEmptySpec()),
      lastResponse: null,
      lastError: null,
      sending: false,
      session: kind === 'http' ? undefined : { status: 'idle' },
      messages: kind === 'http' ? undefined : []
    };
    state.tabs.push(tab);
    await setActiveTab(tab.id);
    await api.upsertTab(tab).catch(() => {});
    return tab;
  }

  async function closeTab(id: string) {
    const idx = state.tabs.findIndex((t) => t.id === id);
    if (idx < 0) return;
    const tab = state.tabs[idx];
    if (tab.kind === 'sse' || tab.kind === 'ws') {
      await closeStream(id).catch(() => {});
    }
    state.tabs.splice(idx, 1);
    await api.deleteTab(id).catch(() => {});
    if (state.tabs.length === 0) {
      await newTab();
    } else if (state.activeTabId === id) {
      const next = state.tabs[Math.min(idx, state.tabs.length - 1)];
      await setActiveTab(next.id);
    }
  }

  // ---- stream API ----

  async function openStream(sessionId: string, kind: 'sse' | 'ws', spec: SseSpec | WsSpec) {
    const tab = state.tabs.find((t) => t.id === sessionId);
    if (!tab) throw new Error('tab not found');
    tab.session = { status: 'connecting' };
    tab.messages = [];
    let resolvedSpec: SseSpec | WsSpec;
    try {
      resolvedSpec = kind === 'sse'
        ? applyVarsToSseSpec(spec as SseSpec, varContext.value)
        : applyVarsToWsSpec(spec as WsSpec, varContext.value);
    } catch (err) {
      tab.session = { status: 'error', error: String((err as Error).message ?? err) };
      throw err;
    }
    const unlisten = await api.listen(sessionId, (msg: StreamMessage) => {
      tab.messages!.push(msg);
      if (msg.kind === 'open') tab.session = { status: 'open', openedAt: msg.atMs };
      else if (msg.kind === 'closed') tab.session = { status: 'closed', openedAt: (tab.session as HttpSession)?.openedAt, closedAt: msg.atMs };
      else if (msg.kind === 'error') tab.session = { status: 'error', error: msg.message, openedAt: (tab.session as HttpSession)?.openedAt };
    });
    streamUnlisten.set(sessionId, unlisten);
    try {
      await api.openStream(sessionId, kind, resolvedSpec);
    } catch (err) {
      tab.session = { status: 'error', error: String(err) };
      unlisten();
      streamUnlisten.delete(sessionId);
      throw err;
    }
  }

  async function closeStream(sessionId: string) {
    await api.closeStream(sessionId).catch(() => {});
    const tab = state.tabs.find((t) => t.id === sessionId);
    if (tab && tab.session) {
      tab.session = { ...tab.session, status: 'closed', closedAt: Date.now() };
    }
    const un = streamUnlisten.get(sessionId);
    if (un) {
      un();
      streamUnlisten.delete(sessionId);
    }
  }

  async function sendWsMessage(sessionId: string, text: string) {
    const tab = state.tabs.find((t) => t.id === sessionId);
    if (!tab || tab.session?.status !== 'open') {
      throw new Error('session not open');
    }
    const resolved = resolveVars(text, varContext.value);
    await api.sendWsMessage(sessionId, resolved);
  }

  async function listStreamMessages(sessionId: string) {
    const msgs = await api.listStreamMessages(sessionId);
    const tab = state.tabs.find((t) => t.id === sessionId);
    if (tab) tab.messages = msgs;
  }

  async function setActiveTab(id: string) {
    state.activeTabId = id;
    for (const t of state.tabs) t.isActive = t.id === id;
    await api.setActiveTab(id).catch(() => {});
    const tab = state.tabs.find((t) => t.id === id);
    if (tab && (tab.kind === 'sse' || tab.kind === 'ws') && (tab.messages?.length ?? 0) === 0) {
      // 静默失败：session 不存在则返回空数组
      await api.listStreamMessages(id).then((msgs) => {
        if (msgs.length > 0) tab.messages = msgs;
      }).catch(() => {});
    }
  }

  async function reorderTabs(orderedIds: string[]) {
    const byId = new Map(state.tabs.map((t) => [t.id, t]));
    const next: HttpTab[] = [];
    orderedIds.forEach((id, i) => {
      const t = byId.get(id);
      if (t) {
        t.orderIndex = i;
        next.push(t);
      }
    });
    state.tabs = next;
    await flushDirtyNow();
  }

  function loadIntoTab(spec: HttpRequestSpec, target?: 'active' | 'new', title?: string): void {
    const mode = target ?? 'active';
    if (mode === 'new' || !activeTab.value) {
      void newTab(spec, title ?? defaultTitle(spec));
      return;
    }
    activeTab.value.spec = spec;
    activeTab.value.title = title ?? defaultTitle(spec);
  }

  function defaultTitle(spec: HttpRequestSpec): string {
    try {
      const u = new URL(spec.url);
      const path = u.pathname.length > 1 ? u.pathname : u.host;
      return `${spec.method} ${path}`.slice(0, 40);
    } catch {
      return `${spec.method} ${spec.url || '新请求'}`.slice(0, 40);
    }
  }

  // ---- send / cancel ----

  async function send(tabId?: string): Promise<void> {
    const tab = tabId ? state.tabs.find((t) => t.id === tabId) : activeTab.value;
    if (!tab || tab.kind !== 'http') return;
    const httpSpec = tab.spec as HttpRequestSpec;
    tab.lastError = null;
    tab.sending = true;
    const start = Date.now();
    let spec: HttpRequestSpec | null = null;
    try {
      spec = applyVarsToSpec(httpSpec, varContext.value);
      const cancelId = ulid();
      tab.cancelTokenId = cancelId;
      await flushDirtyNow();
      const resp = await api.sendHttp(spec, cancelId);
      tab.lastResponse = resp;
      if (httpSpec.saveToHistory) {
        const item: HttpHistoryItem = {
          id: ulid(),
          method: httpSpec.method,
          url: spec?.url ?? httpSpec.url,
          status: resp.status,
          elapsedMs: resp.elapsedMs,
          bodyBytes: resp.bodyBytes,
          spec: JSON.parse(JSON.stringify(httpSpec)),
          respSummary: resp.body.slice(0, RESP_SUMMARY_MAX),
          createdAt: Date.now()
        };
        state.history.unshift(item);
        if (state.history.length > HISTORY_LIMIT) state.history.length = HISTORY_LIMIT;
        await api.insertHistory(item).catch(() => {});
      }
    } catch (err) {
      const msg = String((err as Error).message ?? err);
      tab.lastError = msg;
      tab.lastResponse = null;
      if (httpSpec.saveToHistory && !/取消/.test(msg)) {
        const item: HttpHistoryItem = {
          id: ulid(),
          method: httpSpec.method,
          url: spec?.url ?? httpSpec.url,
          status: null,
          elapsedMs: Date.now() - start,
          bodyBytes: null,
          spec: JSON.parse(JSON.stringify(httpSpec)),
          respSummary: msg.slice(0, RESP_SUMMARY_MAX),
          createdAt: Date.now()
        };
        state.history.unshift(item);
        await api.insertHistory(item).catch(() => {});
      }
    } finally {
      tab.sending = false;
      tab.cancelTokenId = undefined;
    }
  }

  async function cancel(tabId?: string): Promise<void> {
    const tab = tabId ? state.tabs.find((t) => t.id === tabId) : activeTab.value;
    if (!tab || !tab.cancelTokenId) return;
    await api.cancelHttp(tab.cancelTokenId).catch(() => {});
  }

  // ---- history API ----

  async function deleteHistory(id: string) {
    state.history = state.history.filter((h) => h.id !== id);
    await api.deleteHistory(id).catch(() => {});
  }

  async function clearHistory() {
    state.history = [];
    await api.clearHistory().catch(() => {});
  }

  // ---- env / vars API ----

  async function addEnv(name: string): Promise<HttpEnv> {
    const env: HttpEnv = {
      id: ulid(),
      name,
      isActive: state.envs.length === 0,
      orderIndex: state.envs.length
    };
    state.envs.push(env);
    await api.upsertEnv(env).catch(() => {});
    if (env.isActive) await setActiveEnv(env.id);
    return env;
  }

  async function renameEnv(id: string, name: string) {
    const env = state.envs.find((e) => e.id === id);
    if (!env) return;
    env.name = name;
    await api.upsertEnv(env).catch(() => {});
  }

  async function deleteEnv(id: string) {
    state.envs = state.envs.filter((e) => e.id !== id);
    if (state.activeEnvId === id) {
      state.activeEnvId = null;
      state.activeEnvVars = [];
    }
    await api.deleteEnv(id).catch(() => {});
  }

  async function setActiveEnv(id: string | null) {
    state.activeEnvId = id;
    for (const e of state.envs) e.isActive = e.id === id;
    if (id) {
      await api.setActiveEnv(id).catch(() => {});
      state.activeEnvVars = await api.listEnvVars(id);
    } else {
      state.activeEnvVars = [];
    }
  }

  async function upsertVar(v: HttpEnvVar) {
    const list = v.envId === '' ? state.globalVars : state.activeEnvVars;
    const idx = list.findIndex((x) => x.id === v.id);
    if (idx >= 0) list[idx] = { ...v };
    else list.push({ ...v });
    await api.upsertEnvVar(v).catch(() => {});
  }

  async function deleteVar(id: string, envId: string) {
    if (envId === '') state.globalVars = state.globalVars.filter((v) => v.id !== id);
    else state.activeEnvVars = state.activeEnvVars.filter((v) => v.id !== id);
    await api.deleteEnvVar(id).catch(() => {});
  }

  function makeVar(envId: string, key = '', value = ''): HttpEnvVar {
    const list = envId === '' ? state.globalVars : state.activeEnvVars;
    return {
      id: ulid(),
      envId,
      key,
      value,
      enabled: true,
      orderIndex: list.length
    };
  }

  async function importCollection(
    imported: ImportedOpenApiCollection,
    opts?: {
      sourceUrl?: string;
      sourceHeaders?: Array<{ key: string; value: string; enabled?: boolean }>;
      syncIntervalSecs?: number;
      baseUrl?: string;
    }
  ) {
    const now = Date.now();
    const collection: HttpCollection = {
      ...imported.collection,
      updatedAt: now,
      sourceUrl: opts?.sourceUrl ?? null,
      sourceHeaders: opts?.sourceHeaders ? normalizeSourceHeaders(opts.sourceHeaders) : null,
      syncIntervalSecs: opts?.syncIntervalSecs ?? null,
      lastSyncedAt: opts?.sourceUrl ? now : null,
      lastSyncError: null,
      baseUrl: opts?.baseUrl ?? null
    };
    const folders: HttpCollectionFolder[] = imported.folders.map((f) => ({ ...f, updatedAt: now }));
    const requests: HttpCollectionRequest[] = imported.requests.map((r) => ({ ...r, updatedAt: now }));

    state.collections.push(collection);
    state.collectionFolders.push(...folders);
    state.collectionRequests.push(...requests);

    await api.upsertCollection(collection);
    for (const folder of folders) await api.upsertCollectionFolder(folder);
    for (const request of requests) await api.upsertCollectionRequest(request);
  }

  function normalizeSourceHeaders(headers: Array<{ key: string; value: string; enabled?: boolean }>) {
    return headers.filter((h) => h.key.trim()).map((h) => ({
      key: h.key,
      value: h.value,
      enabled: h.enabled ?? true
    }));
  }

  const syncingIds = reactive(new Set<string>());
  let syncTimer: ReturnType<typeof setInterval> | null = null;

  async function updateCollectionSyncConfig(
    id: string,
    patch: Partial<Pick<HttpCollection, 'sourceUrl' | 'sourceHeaders' | 'syncIntervalSecs' | 'baseUrl'>>
  ) {
    const col = state.collections.find((c) => c.id === id);
    if (!col) return;
    Object.assign(col, patch);
    col.updatedAt = Date.now();
    await api.upsertCollection(col).catch(() => {});
  }

  async function syncCollection(
    id: string,
    opts?: { manual?: boolean }
  ): Promise<{ added: number; updated: number; deleted: number }> {
    const col = state.collections.find((c) => c.id === id);
    if (!col || !col.sourceUrl) return { added: 0, updated: 0, deleted: 0 };
    if (syncingIds.has(id)) return { added: 0, updated: 0, deleted: 0 };
    syncingIds.add(id);
    try {
      const resolvedHeaders = (col.sourceHeaders ?? [])
        .filter((h) => h.enabled !== false && h.key.trim())
        .map((h) => ({ key: h.key, value: resolveVars(h.value, varContext.value) }));
      const resolvedUrl = resolveVars(col.sourceUrl, varContext.value);
      const body = await api.fetchOpenApiUrl(resolvedUrl, resolvedHeaders);
      const incoming = parseOpenApiToCollection(body, {
        baseUrl: col.baseUrl ?? undefined,
        collectionName: col.name
      });
      const existing = {
        collection: col,
        folders: state.collectionFolders.filter((f) => f.collectionId === id),
        requests: state.collectionRequests.filter((r) => r.collectionId === id)
      };
      const merged = mergeIntoCollection(existing, incoming);
      if (!state.collections.some((c) => c.id === id)) {
        // 集合在同步过程中被删除；放弃写回 state/DB，避免复活已删除的集合
        return { added: 0, updated: 0, deleted: 0 };
      }
      state.collectionFolders = state.collectionFolders
        .filter((f) => f.collectionId !== id)
        .concat(merged.folders);
      state.collectionRequests = state.collectionRequests
        .filter((r) => r.collectionId !== id)
        .concat(merged.requests);
      for (const f of merged.folders) await api.upsertCollectionFolder(f).catch(() => {});
      for (const r of merged.requests) await api.upsertCollectionRequest(r).catch(() => {});
      for (const rid of merged.deletedRequestIds) await api.deleteCollectionRequest(rid).catch(() => {});
      for (const fid of merged.deletedFolderIds) await api.deleteCollectionFolder(fid).catch(() => {});
      col.lastSyncedAt = Date.now();
      col.lastSyncError = null;
      col.updatedAt = Date.now();
      await api.upsertCollection(col).catch(() => {});
      return merged.diff;
    } catch (err) {
      const msg = String((err as Error).message ?? err);
      if (state.collections.some((c) => c.id === id)) {
        col.lastSyncError = msg;
        col.updatedAt = Date.now();
        await api.upsertCollection(col).catch(() => {});
      }
      if (opts?.manual) throw err;
      return { added: 0, updated: 0, deleted: 0 };
    } finally {
      syncingIds.delete(id);
    }
  }

  function startSyncScheduler() {
    if (syncTimer) return;
    syncTimer = setInterval(() => {
      const now = Date.now();
      for (const c of state.collections) {
        if (!c.sourceUrl || !c.syncIntervalSecs) continue;
        if (syncingIds.has(c.id)) continue;
        const due = (c.lastSyncedAt ?? 0) + c.syncIntervalSecs * 1000;
        if (now >= due) void syncCollection(c.id);
      }
    }, 60_000);
  }

  function stopSyncScheduler() {
    if (syncTimer) {
      clearInterval(syncTimer);
      syncTimer = null;
    }
  }

  async function deleteCollection(id: string) {
    state.collections = state.collections.filter((c) => c.id !== id);
    state.collectionFolders = state.collectionFolders.filter((f) => f.collectionId !== id);
    state.collectionRequests = state.collectionRequests.filter((r) => r.collectionId !== id);
    syncingIds.delete(id);
    await api.deleteCollection(id).catch(() => {});
  }

  async function deleteCollectionRequest(id: string) {
    state.collectionRequests = state.collectionRequests.filter((r) => r.id !== id);
    await api.deleteCollectionRequest(id).catch(() => {});
  }

  async function openCollectionRequest(request: HttpCollectionRequest, target: 'active' | 'new' = 'active') {
    loadIntoTab(JSON.parse(JSON.stringify(request.spec)), target, request.name);
  }

  return {
    state,
    initError,
    activeTab,
    varContext,
    init,
    flushDirtyNow,
    newTab,
    closeTab,
    setActiveTab,
    reorderTabs,
    loadIntoTab,
    send,
    cancel,
    deleteHistory,
    clearHistory,
    addEnv,
    renameEnv,
    deleteEnv,
    setActiveEnv,
    upsertVar,
    deleteVar,
    makeVar,
    importCollection,
    deleteCollection,
    deleteCollectionRequest,
    openCollectionRequest,
    syncingIds,
    syncCollection,
    updateCollectionSyncConfig,
    stopSyncScheduler,
    openStream,
    closeStream,
    sendWsMessage,
    listStreamMessages,
    fetchOpenApiUrl: api.fetchOpenApiUrl
  };
}

export function configureHttpStore(transport: ServiceTransport) {
  _singleton?.stopSyncScheduler();
  const stream = createStreamApi(transport);
  _singleton = createStore({ ...createHttpApi(transport), ...stream } as FullApi);
  return _singleton;
}

export function useHttpStore() {
  if (!_singleton) throw new Error('HTTP_SERVICE_DISCONNECTED: store is not configured');
  return _singleton;
}

export function _resetHttpStoreForTest(api: FullApi) {
  _singleton?.stopSyncScheduler?.();
  _singleton = createStore(api);
  return _singleton;
}

export type HttpStore = ReturnType<typeof useHttpStore>;

export type { HttpResponseInfo };
