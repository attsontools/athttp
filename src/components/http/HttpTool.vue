<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { NButton, NSelect, useMessage } from 'naive-ui';
import { useHttpStore } from '../../composables/useHttpStore';
import { useSidebarWidth } from '../../composables/useSidebarWidth';
import type { HttpCollection, HttpCollectionRequest, HttpEnvVar, HttpRequestSpec, TabKind } from './types';
import { toCurl } from './curl';
import HttpSidebar from './HttpSidebar.vue';
import HttpTabBar from './HttpTabBar.vue';
import HttpRequestEditor from './HttpRequestEditor.vue';
import HttpResponseView from './HttpResponseView.vue';
import HttpEnvModal from './HttpEnvModal.vue';
import HttpOpenApiImportModal from './HttpOpenApiImportModal.vue';
import type { ImportPayload } from './HttpOpenApiImportModal.vue';
import HttpSchemaPreviewModal from './HttpSchemaPreviewModal.vue';
import { isSchemaSpec } from './schemaItem';
import HttpSyncSettingsModal from './HttpSyncSettingsModal.vue';
import SseTool from './SseTool.vue';
import WsTool from './WsTool.vue';
import type { HttpHost } from '../../frontend/pluginHost';

const props = defineProps<{ host: HttpHost }>();

const store = useHttpStore();
const message = useMessage();
const collapsed = ref(false);

// ---- 数据模型只读预览 ----
const schemaPreviewOpen = ref(false);
const schemaPreviewName = ref('');
const schemaPreviewBody = ref('');

function onOpenCollectionRequest(request: HttpCollectionRequest, mode: 'active' | 'new') {
  if (isSchemaSpec(request.spec)) {
    schemaPreviewName.value = request.name;
    schemaPreviewBody.value = request.spec.body;
    schemaPreviewOpen.value = true;
    return;
  }
  store.openCollectionRequest(request, mode);
}

// ---- 集合面板宽度拖拽 ----
const { width: sidebarWidth, setWidth: setSidebarWidth } = useSidebarWidth();
const draggingSidebar = ref(false);
let dragStartX = 0;
let dragStartWidth = 0;

function onSidebarResizeStart(ev: MouseEvent) {
  ev.preventDefault();
  draggingSidebar.value = true;
  dragStartX = ev.clientX;
  dragStartWidth = sidebarWidth.value;
  document.body.style.cursor = 'col-resize';
  document.body.style.userSelect = 'none';
  window.addEventListener('mousemove', onSidebarResizeMove);
  window.addEventListener('mouseup', onSidebarResizeEnd);
}

function onSidebarResizeMove(ev: MouseEvent) {
  setSidebarWidth(dragStartWidth + (ev.clientX - dragStartX));
}

function onSidebarResizeEnd() {
  draggingSidebar.value = false;
  document.body.style.cursor = '';
  document.body.style.userSelect = '';
  window.removeEventListener('mousemove', onSidebarResizeMove);
  window.removeEventListener('mouseup', onSidebarResizeEnd);
}
const envModalOpen = ref(false);
const envModalTab = ref<'env' | 'vars'>('vars');
const openApiImportOpen = ref(false);
const syncSettingsOpen = ref(false);
const syncSettingsCollectionId = ref<string | null>(null);
const syncSettingsCollection = computed<HttpCollection | null>(() =>
  syncSettingsCollectionId.value
    ? store.state.collections.find((c) => c.id === syncSettingsCollectionId.value) ?? null
    : null
);
const syncSettingsSyncing = computed(() =>
  syncSettingsCollection.value ? store.syncingIds.has(syncSettingsCollection.value.id) : false
);

const envOptions = computed(() => [
  { label: '不使用环境', value: '__none__' },
  ...store.state.envs.map((e) => ({ label: e.name, value: e.id })),
  { label: '管理环境…', value: '__manage__' }
]);

const currentEnvValue = computed(() => store.state.activeEnvId ?? '__none__');

async function onEnvSelect(value: string) {
  if (value === '__manage__') {
    envModalTab.value = 'env';
    envModalOpen.value = true;
    return;
  }
  await store.setActiveEnv(value === '__none__' ? null : value);
}

function openVarsModal() {
  envModalTab.value = 'vars';
  envModalOpen.value = true;
}

function updateSpec(next: HttpRequestSpec) {
  const active = store.activeTab.value;
  if (!active || active.kind !== 'http') return;
  active.spec = next;
  const url = next.url;
  if (url) {
    try {
      const u = new URL(url);
      const p = u.pathname.length > 1 ? u.pathname : u.host;
      active.title = `${next.method} ${p}`.slice(0, 40);
    } catch {
      active.title = `${next.method} ${url || '新请求'}`.slice(0, 40);
    }
  }
}

async function copyCurrentCurl(template: boolean) {
  const active = store.activeTab.value;
  if (!active || active.kind !== 'http') return;
  const text = toCurl(active.spec as HttpRequestSpec, template ? null : store.varContext.value);
  try {
    await props.host.copyText(text);
  } catch (error) {
    message.error(String((error as Error).message ?? error));
  }
}

function onLoadHistory(item: import('./types').HttpHistoryItem, mode: 'active' | 'new') {
  store.loadIntoTab(JSON.parse(JSON.stringify(item.spec)), mode);
}

async function fetchOpenApi(url: string, headers: Array<{ key: string; value: string }>) {
  return await store.fetchOpenApiUrl(url, headers);
}

async function onImportOpenApi(payload: ImportPayload) {
  await store.importCollection(payload.imported, payload.source ? {
    sourceUrl: payload.source.url,
    sourceHeaders: payload.source.headers,
    syncIntervalSecs: payload.source.intervalSecs ?? undefined,
    baseUrl: payload.source.baseUrl
  } : undefined);
}

function openSyncSettings(id: string) {
  syncSettingsCollectionId.value = id;
  syncSettingsOpen.value = true;
}

async function onSyncNow(id: string) {
  try {
    const diff = await store.syncCollection(id, { manual: true });
    message.success(`新增 ${diff.added} · 更新 ${diff.updated} · 删除 ${diff.deleted}`);
  } catch (err) {
    message.error(String((err as Error).message ?? err));
  }
}

async function onSyncSettingsSave(patch: { url: string; headers: Array<{ key: string; value: string; enabled: boolean }>; intervalSecs: number | null; baseUrl: string }) {
  if (!syncSettingsCollectionId.value) return;
  await store.updateCollectionSyncConfig(syncSettingsCollectionId.value, {
    sourceUrl: patch.url || null,
    sourceHeaders: patch.headers.length ? patch.headers : null,
    syncIntervalSecs: patch.intervalSecs,
    baseUrl: patch.baseUrl || null
  });
}

async function onSyncSettingsDisconnect() {
  if (!syncSettingsCollectionId.value) return;
  await store.updateCollectionSyncConfig(syncSettingsCollectionId.value, {
    sourceUrl: null,
    sourceHeaders: null,
    syncIntervalSecs: null
  });
  syncSettingsOpen.value = false;
}

async function onSyncSettingsSyncNow() {
  if (!syncSettingsCollectionId.value) return;
  await onSyncNow(syncSettingsCollectionId.value);
}

// ---- 快捷键 ----
function onKey(ev: KeyboardEvent) {
  const meta = ev.metaKey || ev.ctrlKey;
  if (!meta) return;
  const k = ev.key.toLowerCase();
  if (k === 'enter') {
    ev.preventDefault();
    if (store.activeTab.value?.sending) store.cancel();
    else void store.send();
  } else if (k === 't') {
    ev.preventDefault();
    void store.newTab();
  } else if (k === 'w') {
    ev.preventDefault();
    if (store.state.activeTabId) void store.closeTab(store.state.activeTabId);
  } else if (k === 'b') {
    ev.preventDefault();
    collapsed.value = !collapsed.value;
  } else if (k === 'e') {
    ev.preventDefault();
    envModalTab.value = 'vars';
    envModalOpen.value = true;
  }
}

function onOpenVarsEvent() {
  envModalTab.value = 'vars';
  envModalOpen.value = true;
}

onMounted(async () => {
  await store.init();
  window.addEventListener('keydown', onKey);
  window.addEventListener('attool:http-open-vars', onOpenVarsEvent);
});
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKey);
  window.removeEventListener('attool:http-open-vars', onOpenVarsEvent);
  window.removeEventListener('mousemove', onSidebarResizeMove);
  window.removeEventListener('mouseup', onSidebarResizeEnd);
  void store.flushDirtyNow();
});
</script>

<template>
  <div class="http-root">
    <HttpSidebar
      :items="store.state.history"
      :collections="store.state.collections"
      :folders="store.state.collectionFolders"
      :requests="store.state.collectionRequests"
      :syncing-ids="store.syncingIds"
      :collapsed="collapsed"
      :width="sidebarWidth"
      :dragging="draggingSidebar"
      @load="onLoadHistory"
      @open-request="onOpenCollectionRequest"
      @delete-collection="store.deleteCollection"
      @delete-request="store.deleteCollectionRequest"
      @import-openapi="openApiImportOpen = true"
      @sync-collection="(id: string) => onSyncNow(id)"
      @open-sync-settings="(id: string) => openSyncSettings(id)"
      @delete="(id: string) => store.deleteHistory(id)"
      @clear="() => store.clearHistory()"
      @toggle-collapse="collapsed = !collapsed"
    />
    <div
      v-if="!collapsed"
      class="sidebar-resizer"
      :class="{ active: draggingSidebar }"
      title="拖拽调整宽度"
      @mousedown="onSidebarResizeStart"
    ></div>
    <div class="main">
      <div class="topbar">
        <div class="topbar-left">
          <span class="label">环境</span>
          <n-select
            :value="currentEnvValue"
            :options="envOptions"
            size="small"
            style="width: 180px"
            @update:value="onEnvSelect"
          />
        </div>
        <n-button size="tiny" secondary @click="openVarsModal">
          <span class="mono">{{ '{}' }}</span>&nbsp;变量
        </n-button>
      </div>

      <HttpTabBar
        :tabs="store.state.tabs"
        :active-id="store.state.activeTabId"
        @activate="(id: string) => store.setActiveTab(id)"
        @close="(id: string) => store.closeTab(id)"
        @new="(kind: TabKind) => store.newTab(undefined, undefined, kind)"
      />

      <template v-if="store.activeTab.value">
        <template v-if="store.activeTab.value.kind === 'http'">
          <HttpRequestEditor
            :spec="store.activeTab.value.spec as any"
            :sending="store.activeTab.value.sending"
            :var-context="store.varContext.value"
            :pick-file="host.pickFile"
            @update:spec="updateSpec"
            @send="() => store.send()"
            @cancel="() => store.cancel()"
            @copy-curl="copyCurrentCurl"
            @apply-spec="(s: HttpRequestSpec) => store.loadIntoTab(s, 'active')"
          />
          <HttpResponseView
            :response="store.activeTab.value.lastResponse"
            :error="store.activeTab.value.lastError"
            :sending="store.activeTab.value.sending"
            :host="host"
          />
        </template>
        <SseTool v-else-if="store.activeTab.value.kind === 'sse'" :tab="store.activeTab.value" />
        <WsTool v-else-if="store.activeTab.value.kind === 'ws'" :tab="store.activeTab.value" />
      </template>
    </div>

    <HttpEnvModal
      v-model:show="envModalOpen"
      :envs="store.state.envs"
      :active-env-id="store.state.activeEnvId"
      :active-env-vars="store.state.activeEnvVars"
      :global-vars="store.state.globalVars"
      :default-tab="envModalTab"
      @add-env="(name: string) => store.addEnv(name)"
      @rename-env="(id: string, name: string) => store.renameEnv(id, name)"
      @delete-env="(id: string) => store.deleteEnv(id)"
      @set-active-env="(id: string | null) => store.setActiveEnv(id)"
      @add-var="(envId: string) => store.upsertVar(store.makeVar(envId))"
      @update-var="(v: HttpEnvVar) => store.upsertVar(v)"
      @delete-var="(id: string, envId: string) => store.deleteVar(id, envId)"
    />
    <HttpOpenApiImportModal
      v-model:show="openApiImportOpen"
      :fetch-open-api="fetchOpenApi"
      @import="onImportOpenApi"
    />
    <HttpSchemaPreviewModal
      v-model:show="schemaPreviewOpen"
      :name="schemaPreviewName"
      :body="schemaPreviewBody"
    />
    <HttpSyncSettingsModal
      v-model:show="syncSettingsOpen"
      :collection="syncSettingsCollection"
      :syncing="syncSettingsSyncing"
      @save="onSyncSettingsSave"
      @disconnect="onSyncSettingsDisconnect"
      @sync-now="onSyncSettingsSyncNow"
    />
  </div>
</template>

<style scoped>
.http-root {
  display: flex;
  height: 100%;
  overflow: hidden;
  background: var(--bg-base);
}
.sidebar-resizer {
  flex: none;
  width: 5px;
  margin-left: -3px;
  margin-right: -2px;
  cursor: col-resize;
  position: relative;
  z-index: 5;
  background: transparent;
  transition: background 0.15s ease;
}
.sidebar-resizer:hover,
.sidebar-resizer.active {
  background: var(--accent);
}
.main {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.topbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 6px 12px;
  border-bottom: 1px solid var(--line);
  gap: 12px;
}
.topbar-left { display: flex; align-items: center; gap: 6px; }
.label { color: var(--text-muted); font-size: var(--fs-xs); }
.mono { font-family: var(--font-mono, ui-monospace, monospace); }
</style>
