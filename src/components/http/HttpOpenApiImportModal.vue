<script setup lang="ts">
import { nextTick, onUnmounted, ref, watch } from 'vue';
import { NAlert, NButton, NInput, NModal, NSelect } from 'naive-ui';
import { parseOpenApiToCollection, type ImportedOpenApiCollection } from './openapiImport';
import { useOpenApiImportWorker } from '../../composables/useOpenApiImportWorker';
import CodeEditor from '../ui/CodeEditor.vue';

type Source = 'file' | 'url';

interface HeaderRow { key: string; value: string; enabled: boolean }

export interface ImportPayload {
  imported: ImportedOpenApiCollection;
  source?: {
    url: string;
    headers: HeaderRow[];
    intervalSecs: number | null;
    baseUrl: string;
  };
}

const props = defineProps<{
  show: boolean;
  fetchOpenApi: (url: string, headers: Array<{ key: string; value: string }>) => Promise<string>;
}>();

const emit = defineEmits<{
  (e: 'update:show', v: boolean): void;
  (e: 'import', v: ImportPayload): void;
}>();

const source = ref<Source>('file');
const worker = useOpenApiImportWorker();

const jsonText = ref('');
const baseUrl = ref('');
const collectionName = ref('');
const sourceUrl = ref('');
const sourceHeaders = ref<HeaderRow[]>([]);
const intervalOptions = [
  { label: '关闭（不自动更新）', value: 0 },
  { label: '5 分钟', value: 300 },
  { label: '30 分钟', value: 1800 },
  { label: '1 小时', value: 3600 },
  { label: '6 小时', value: 21600 },
  { label: '24 小时', value: 86400 }
];
const intervalSecs = ref(0);
const fetching = ref(false);
const error = ref('');
const preview = ref<ImportedOpenApiCollection | null>(null);
const parsing = ref(false);
const formatting = ref(false);

const PREVIEW_DEBOUNCE_MS = 300;
let debounceTimer: ReturnType<typeof setTimeout> | null = null;
// Monotonic guard: every change to the preview intent bumps this. An async
// parse may only write to the UI if its captured gen still matches — stale
// results (superseded input, cleared text) are dropped.
let previewGen = 0;

function schedulePreview() {
  if (debounceTimer) clearTimeout(debounceTimer);
  const gen = ++previewGen;
  const text = jsonText.value;
  if (!text.trim()) {
    preview.value = null;
    error.value = '';
    parsing.value = false;
    return;
  }
  parsing.value = true;
  debounceTimer = setTimeout(() => {
    void runPreview(text, gen);
  }, PREVIEW_DEBOUNCE_MS);
}

async function runPreview(text: string, gen: number) {
  const outcome = await worker.parse(
    text,
    { baseUrl: baseUrl.value || undefined, collectionName: collectionName.value || undefined },
    'preview',
  );
  // Superseded by a newer intent (keystroke, clear, or file pick) — drop it.
  if (gen !== previewGen || outcome === null) return;
  parsing.value = false;
  if (outcome.ok) {
    preview.value = outcome.result;
    error.value = '';
  } else {
    preview.value = null;
    // Preview errors stay silent (invalid partial JSON while typing); doImport surfaces them.
    error.value = '';
  }
}

watch([jsonText, baseUrl, collectionName], schedulePreview);

onUnmounted(() => {
  if (debounceTimer) { clearTimeout(debounceTimer); debounceTimer = null; }
});

watch(
  () => props.show,
  (show) => {
    if (!show) return;
    source.value = 'file';
    previewGen++;
    jsonText.value = '';
    baseUrl.value = '';
    collectionName.value = '';
    sourceUrl.value = '';
    sourceHeaders.value = [];
    intervalSecs.value = 0;
    fetching.value = false;
    error.value = '';
    preview.value = null;
    parsing.value = false;
  },
);

function addHeader() {
  sourceHeaders.value.push({ key: '', value: '', enabled: true });
}
function removeHeader(idx: number) {
  sourceHeaders.value.splice(idx, 1);
}

async function onFile(e: Event) {
  const input = e.target as HTMLInputElement;
  const file = input.files?.[0];
  if (!file) return;
  try {
    const text = await file.text();
    jsonText.value = text;
    const outcome = await worker.parse(text, {}, 'preview');
    if (outcome && outcome.ok) {
      baseUrl.value = outcome.result.baseUrl;
      collectionName.value = outcome.result.collection.name;
      preview.value = outcome.result;
      error.value = '';
    } else if (outcome && !outcome.ok) {
      error.value = outcome.error;
    }
    // Writing jsonText/baseUrl/collectionName above queued a debounced preview
    // via the watcher (Vue flushes it on the next microtask). Wait for that
    // flush, then bump the guard + clear the timer so the redundant re-parse
    // is dropped and our file result stays authoritative.
    await nextTick();
    previewGen++;
    if (debounceTimer) { clearTimeout(debounceTimer); debounceTimer = null; }
    parsing.value = false;
  } catch (err) {
    error.value = String((err as Error).message ?? err);
    parsing.value = false;
  } finally {
    input.value = '';
  }
}

async function doFetch() {
  const url = sourceUrl.value.trim();
  if (!url) {
    error.value = '请填写 JSON URL';
    return;
  }
  fetching.value = true;
  error.value = '';
  try {
    const headers = sourceHeaders.value
      .filter((h) => h.enabled && h.key.trim())
      .map((h) => ({ key: h.key, value: h.value }));
    const body = await props.fetchOpenApi(url, headers);
    jsonText.value = body;
    try {
      const parsed = parseOpenApiToCollection(body);
      baseUrl.value = baseUrl.value || parsed.baseUrl;
      collectionName.value = collectionName.value || parsed.collection.name;
    } catch (parseErr) {
      error.value = String((parseErr as Error).message ?? parseErr);
    }
  } catch (err) {
    error.value = String((err as Error).message ?? err);
  } finally {
    fetching.value = false;
  }
}

function close() {
  emit('update:show', false);
}

async function doImport() {
  const outcome = await worker.parse(
    jsonText.value,
    { baseUrl: baseUrl.value || undefined, collectionName: collectionName.value || undefined },
    'import',
  );
  if (!outcome) return;
  if (outcome.ok) {
    const payload: ImportPayload = { imported: outcome.result };
    if (source.value === 'url' && sourceUrl.value.trim()) {
      payload.source = {
        url: sourceUrl.value.trim(),
        headers: sourceHeaders.value.filter((h) => h.key.trim()),
        intervalSecs: intervalSecs.value > 0 ? intervalSecs.value : null,
        baseUrl: baseUrl.value,
      };
    }
    emit('import', payload);
    close();
  } else {
    error.value = outcome.error;
  }
}

// 在 worker 里解析+重新缩进,避免大文档在主线程 JSON.parse/stringify 卡顿。
async function doFormat() {
  if (formatting.value || !jsonText.value.trim()) return;
  formatting.value = true;
  try {
    const out = await worker.format(jsonText.value, 2, 'format');
    if (!out) return;
    if (out.ok) {
      jsonText.value = out.text;
      error.value = '';
    } else {
      error.value = out.error;
    }
  } finally {
    formatting.value = false;
  }
}
</script>

<template>
  <n-modal
    :show="show"
    preset="card"
    title="导入 OpenAPI JSON"
    :bordered="false"
    style="width: 680px; max-width: 94vw;"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <div class="import-modal">
      <div class="source-switch">
        <button :class="{ active: source === 'file' }" @click="source = 'file'">本地文件</button>
        <button :class="{ active: source === 'url' }" @click="source = 'url'">URL</button>
      </div>

      <template v-if="source === 'file'">
        <label class="file-pick">
          <input type="file" accept=".json,application/json" @change="onFile" />
          <span>选择 openapi.json</span>
        </label>
      </template>

      <template v-else>
        <div class="url-block">
          <label>
            <span>JSON URL</span>
            <n-input v-model:value="sourceUrl" size="small" placeholder="https://example.com/openapi.json" />
          </label>
          <div class="headers-wrap">
            <div class="headers-head">
              <span>请求头（可选，支持 &#123;&#123;var&#125;&#125;）</span>
              <n-button size="tiny" secondary @click="addHeader">添加</n-button>
            </div>
            <div v-for="(h, idx) in sourceHeaders" :key="idx" class="header-row">
              <n-input v-model:value="h.key" size="small" placeholder="Key" />
              <n-input v-model:value="h.value" size="small" placeholder="Value" />
              <n-button size="tiny" quaternary @click="removeHeader(idx)">x</n-button>
            </div>
          </div>
          <div class="interval-row">
            <label>
              <span>自动更新</span>
              <n-select
                v-model:value="intervalSecs"
                :options="intervalOptions"
                size="small"
                style="width: 200px"
              />
            </label>
            <n-button size="small" :loading="fetching" :disabled="!sourceUrl.trim()" @click="doFetch">
              拉取并预览
            </n-button>
          </div>
        </div>
      </template>

      <div class="grid">
        <label>
          <span>集合名称</span>
          <n-input v-model:value="collectionName" size="small" placeholder="默认读取 info.title" />
        </label>
        <label>
          <span>Base URL</span>
          <n-input v-model:value="baseUrl" size="small" placeholder="https://api.example.com 或 {{baseUrl}}" />
        </label>
      </div>

      <label class="json-box">
        <span class="json-box-head">
          <span>JSON 内容</span>
          <n-button
            text
            size="tiny"
            :disabled="!jsonText.trim() || formatting"
            :loading="formatting"
            @click="doFormat"
          >
            格式化
          </n-button>
        </span>
        <CodeEditor v-model="jsonText" language="json" :height="260" />
      </label>

      <n-alert v-if="error" type="error" :bordered="false">{{ error }}</n-alert>
      <div v-else-if="parsing" class="preview">解析中…</div>
      <div v-else-if="preview" class="preview">
        将导入 {{ preview.requests.length }} 个请求、{{ preview.folders.length }} 个目录到集合
        <span class="mono">{{ preview.collection.name }}</span>
      </div>

      <div class="actions">
        <n-button secondary @click="close">取消</n-button>
        <n-button type="primary" :disabled="!jsonText.trim()" @click="doImport">导入集合</n-button>
      </div>
    </div>
  </n-modal>
</template>

<style scoped>
.import-modal { display: grid; gap: 12px; }
.file-pick {
  display: inline-flex;
  width: max-content;
  cursor: pointer;
  color: var(--accent);
  font-size: var(--fs-xs);
}
.file-pick input { display: none; }
.grid { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
label { display: grid; gap: 5px; font-size: var(--fs-xs); color: var(--text-muted); }
.json-box-head { display: flex; align-items: center; justify-content: space-between; }
.preview {
  padding: 8px 10px;
  border: 1px solid var(--line);
  border-radius: var(--radius);
  background: var(--bg-elev-2);
  color: var(--text-muted);
  font-size: var(--fs-xs);
}
.mono { font-family: var(--font-mono); color: var(--text); }
.actions { display: flex; justify-content: flex-end; gap: 8px; }

.source-switch {
  display: inline-flex;
  gap: 4px;
  padding: 2px;
  border: 1px solid var(--line);
  border-radius: var(--radius);
  background: var(--bg-elev-2);
  width: max-content;
}
.source-switch button {
  border: 0;
  background: transparent;
  color: var(--text-muted);
  padding: 4px 12px;
  border-radius: var(--radius-sm);
  cursor: pointer;
  font-size: var(--fs-xs);
}
.source-switch button.active { background: var(--bg-base); color: var(--text); }

.url-block { display: grid; gap: 10px; }
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
.interval-row {
  display: flex;
  justify-content: space-between;
  align-items: end;
  gap: 8px;
}
</style>
