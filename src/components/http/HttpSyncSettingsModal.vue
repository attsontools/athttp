<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { NAlert, NButton, NInput, NModal, NSelect } from 'naive-ui';
import type { HttpCollection } from './types';

interface HeaderRow { key: string; value: string; enabled: boolean }

const props = defineProps<{
  show: boolean;
  collection: HttpCollection | null;
  syncing: boolean;
}>();

const emit = defineEmits<{
  (e: 'update:show', v: boolean): void;
  (e: 'save', v: { url: string; headers: HeaderRow[]; intervalSecs: number | null; baseUrl: string }): void;
  (e: 'disconnect'): void;
  (e: 'sync-now'): void;
}>();

const url = ref('');
const baseUrl = ref('');
const headers = ref<HeaderRow[]>([]);
const intervalSecs = ref(0);
const intervalOptions = [
  { label: '关闭（不自动更新）', value: 0 },
  { label: '5 分钟', value: 300 },
  { label: '30 分钟', value: 1800 },
  { label: '1 小时', value: 3600 },
  { label: '6 小时', value: 21600 },
  { label: '24 小时', value: 86400 }
];

watch(
  () => [props.show, props.collection?.id] as const,
  () => {
    if (!props.show || !props.collection) return;
    url.value = props.collection.sourceUrl ?? '';
    baseUrl.value = props.collection.baseUrl ?? '';
    headers.value = (props.collection.sourceHeaders ?? []).map((h) => ({
      key: h.key,
      value: h.value,
      enabled: h.enabled ?? true
    }));
    intervalSecs.value = props.collection.syncIntervalSecs ?? 0;
  }
);

const lastSyncedText = computed(() => {
  const ts = props.collection?.lastSyncedAt;
  if (!ts) return '尚未同步';
  const diff = Date.now() - ts;
  if (diff < 60_000) return '刚刚同步';
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分前同步`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前同步`;
  return `${Math.floor(diff / 86_400_000)} 天前同步`;
});

function addHeader() { headers.value.push({ key: '', value: '', enabled: true }); }
function removeHeader(idx: number) { headers.value.splice(idx, 1); }

function close() { emit('update:show', false); }

function doSave() {
  emit('save', {
    url: url.value.trim(),
    headers: headers.value.filter((h) => h.key.trim()),
    intervalSecs: intervalSecs.value > 0 ? intervalSecs.value : null,
    baseUrl: baseUrl.value
  });
  close();
}

// 立即同步前先保存，避免同步用到的是未保存的旧值
function doSyncNow() {
  emit('save', {
    url: url.value.trim(),
    headers: headers.value.filter((h) => h.key.trim()),
    intervalSecs: intervalSecs.value > 0 ? intervalSecs.value : null,
    baseUrl: baseUrl.value
  });
  emit('sync-now');
}
</script>

<template>
  <n-modal
    :show="show"
    preset="card"
    title="编辑同步设置"
    :bordered="false"
    style="width: 620px; max-width: 94vw;"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <div class="body" v-if="collection">
      <div class="col-head">
        <span class="name">{{ collection.name }}</span>
        <span class="stamp">{{ lastSyncedText }}</span>
      </div>

      <n-alert v-if="collection.lastSyncError" type="error" :bordered="false">
        上次同步失败：{{ collection.lastSyncError }}
      </n-alert>

      <label>
        <span>JSON URL</span>
        <n-input v-model:value="url" size="small" placeholder="https://example.com/openapi.json" />
      </label>

      <div class="headers-wrap">
        <div class="headers-head">
          <span>请求头（可选，支持 &#123;&#123;var&#125;&#125;）</span>
          <n-button size="tiny" secondary @click="addHeader">添加</n-button>
        </div>
        <div v-for="(h, idx) in headers" :key="idx" class="header-row">
          <n-input v-model:value="h.key" size="small" placeholder="Key" />
          <n-input v-model:value="h.value" size="small" placeholder="Value" />
          <n-button size="tiny" quaternary @click="removeHeader(idx)">x</n-button>
        </div>
      </div>

      <div class="grid">
        <label>
          <span>Base URL（覆盖 OpenAPI servers[0]）</span>
          <n-input v-model:value="baseUrl" size="small" placeholder="留空使用 OpenAPI 内声明" />
        </label>
        <label>
          <span>自动更新</span>
          <n-select v-model:value="intervalSecs" :options="intervalOptions" size="small" />
        </label>
      </div>

      <div class="actions">
        <n-button quaternary @click="emit('disconnect')">断开同步</n-button>
        <n-button secondary :loading="syncing" :disabled="!url.trim()" @click="doSyncNow">立即同步</n-button>
        <n-button secondary @click="close">取消</n-button>
        <n-button type="primary" :disabled="!url.trim()" @click="doSave">保存</n-button>
      </div>
    </div>
  </n-modal>
</template>

<style scoped>
.body { display: grid; gap: 12px; }
.col-head {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  padding-bottom: 4px;
  border-bottom: 1px solid var(--line);
}
.col-head .name { font-weight: 600; color: var(--text); }
.col-head .stamp { color: var(--text-muted); font-size: var(--fs-xs); }
label { display: grid; gap: 5px; font-size: var(--fs-xs); color: var(--text-muted); }
.headers-wrap { display: grid; gap: 4px; }
.headers-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  color: var(--text-muted);
  font-size: var(--fs-xs);
}
.header-row {
  display: grid;
  grid-template-columns: 1fr 1fr auto;
  gap: 6px;
  align-items: center;
}
.grid { display: grid; grid-template-columns: 1.5fr 1fr; gap: 10px; }
.actions { display: flex; justify-content: flex-end; gap: 8px; flex-wrap: wrap; }
</style>
