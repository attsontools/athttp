<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import {
  NButton,
  NDropdown,
  NInput,
  NInputNumber,
  NSelect,
  NTabs,
  NTabPane,
  NModal,
  useMessage
} from 'naive-ui';
import type { HttpRequestSpec, HttpMethod, KV, MultipartField } from './types';
import type { VarContext } from './variables';
import { collectUnknownVars } from './variables';
import { parseCurl, toCurl } from './curl';
import { formatJsonBody, validateJsonBody } from './jsonBody';
import CodeEditor from '../ui/CodeEditor.vue';
import Kbd from '../ui/Kbd.vue';
import StableTextField from '../ui/StableTextField.vue';

const props = defineProps<{
  spec: HttpRequestSpec;
  sending: boolean;
  varContext: VarContext;
  pickFile(): Promise<string | null>;
}>();

const message = useMessage();

const emit = defineEmits<{
  (e: 'update:spec', v: HttpRequestSpec): void;
  (e: 'send'): void;
  (e: 'cancel'): void;
  (e: 'copy-curl', template: boolean): void;
  (e: 'apply-spec', spec: HttpRequestSpec): void;
}>();

const spec = computed({
  get: () => props.spec,
  set: (v) => emit('update:spec', v)
});

const requestTab = ref('params');
const urlOverlay = ref<HTMLDivElement | null>(null);
const bodyEditor = ref<{ replaceValue(value: string): void; focus(): void } | null>(null);
const jsonError = ref('');

const methodOptions: Array<{ label: HttpMethod; value: HttpMethod }> = [
  { label: 'GET', value: 'GET' },
  { label: 'POST', value: 'POST' },
  { label: 'PUT', value: 'PUT' },
  { label: 'PATCH', value: 'PATCH' },
  { label: 'DELETE', value: 'DELETE' },
  { label: 'HEAD', value: 'HEAD' },
  { label: 'OPTIONS', value: 'OPTIONS' }
];

const bodyTypeOptions = [
  { label: 'None', value: 'none' },
  { label: 'JSON', value: 'json' },
  { label: 'Form', value: 'form' },
  { label: 'Text', value: 'text' },
  { label: 'Multipart', value: 'multipart' }
];

const authTypeOptions = [
  { label: '不认证', value: 'none' },
  { label: 'Bearer Token', value: 'bearer' },
  { label: 'Basic Auth', value: 'basic' }
];

const bodyDisabled = computed(() => props.spec.method === 'GET' || props.spec.method === 'HEAD');

// ---- 变量高亮 ----
function highlightVars(input: string): string {
  if (!input) return '';
  const unknown = new Set(collectUnknownVars(input, props.varContext));
  return input.replace(/\{\{\s*([A-Za-z_][A-Za-z0-9_-]*)\s*\}\}/g, (_, name) => {
    const cls = unknown.has(name) ? 'var-miss' : 'var-hit';
    return `<span class="${cls}">{{${name}}}</span>`;
  });
}

const urlHtml = computed(() => highlightVars(props.spec.url));

function updateUrl(value: string) {
  spec.value = { ...props.spec, url: value };
}

function syncUrlScroll(event: Event) {
  if (urlOverlay.value) {
    urlOverlay.value.scrollLeft = (event.target as HTMLInputElement).scrollLeft;
  }
}

// ---- Params <-> URL 双向同步 ----
// 用 flush: 'sync' 让守卫在同一批变更内生效，避免 Vue 默认异步 flush 让
// syncing 在两个 watcher 之间被反复重置，触发 <HttpTool> 的递归更新告警。
// 再叠加一层"输出等值则不 emit"防御，杜绝无意义的往返。
let syncing = false;

function kvListEqual(a: KV[], b: KV[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    const x = a[i], y = b[i];
    if (x.key !== y.key || x.value !== y.value || x.enabled !== y.enabled) return false;
  }
  return true;
}

function encodeParamValue(v: string): string {
  return encodeURIComponent(v).replace(/%7B%7B/g, '{{').replace(/%7D%7D/g, '}}');
}

watch(
  () => props.spec.url,
  () => {
    if (syncing) return;
    syncing = true;
    try {
      const idx = props.spec.url.indexOf('?');
      if (idx < 0) return;
      const q = new URLSearchParams(props.spec.url.slice(idx + 1));
      const nextParams: KV[] = [];
      q.forEach((v, k) => {
        nextParams.push({ key: k, value: v, enabled: true });
      });
      // 保留原来 disabled 的手动条目
      const disabled = props.spec.queryParams.filter((p) => !p.enabled || !p.key);
      const merged = [...nextParams, ...disabled];
      if (kvListEqual(merged, props.spec.queryParams)) return;
      spec.value = { ...props.spec, queryParams: merged };
    } finally {
      syncing = false;
    }
  },
  { flush: 'sync' }
);
watch(
  () => props.spec.queryParams,
  (params) => {
    if (syncing) return;
    syncing = true;
    try {
      const idx = props.spec.url.indexOf('?');
      const base = idx < 0 ? props.spec.url : props.spec.url.slice(0, idx);
      const parts: string[] = [];
      for (const p of params) {
        if (!p.enabled || !p.key) continue;
        // 保留 {{var}} 字面
        parts.push(`${encodeURIComponent(p.key)}=${encodeParamValue(p.value)}`);
      }
      const nextUrl = parts.length ? `${base}?${parts.join('&')}` : base;
      if (nextUrl === props.spec.url) return;
      spec.value = { ...props.spec, url: nextUrl };
    } finally {
      syncing = false;
    }
  },
  { deep: true, flush: 'sync' }
);

// ---- KV 操作 ----
function addRow(list: KV[]): KV[] {
  return [...list, { key: '', value: '', enabled: true }];
}
function updateHeaders(next: KV[]) { spec.value = { ...props.spec, headers: next }; }
function updateParams(next: KV[]) { spec.value = { ...props.spec, queryParams: next }; }
function updateAuthType(v: 'none' | 'bearer' | 'basic') {
  spec.value = { ...props.spec, auth: { ...props.spec.auth, type: v } };
}
function updateAuthField(k: 'bearerToken' | 'basicUser' | 'basicPass', v: string) {
  spec.value = { ...props.spec, auth: { ...props.spec.auth, [k]: v } };
}

// ---- Multipart ----
async function pickFile(idx: number) {
  try {
    const selected = await props.pickFile();
    if (!selected) return;
    const next = props.spec.multipartFields.map((f, i) => (i === idx ? { ...f, value: selected } : f));
    spec.value = { ...props.spec, multipartFields: next };
  } catch (error) {
    message.error(String((error as Error).message ?? error));
  }
}
function addMultipartField() {
  const next: MultipartField[] = [
    ...props.spec.multipartFields,
    { key: '', kind: 'text', value: '', enabled: true }
  ];
  spec.value = { ...props.spec, multipartFields: next };
}
function updateMultipart(idx: number, patch: Partial<MultipartField>) {
  const next = props.spec.multipartFields.map((f, i) => (i === idx ? { ...f, ...patch } : f));
  spec.value = { ...props.spec, multipartFields: next };
}
function removeMultipart(idx: number) {
  const next = props.spec.multipartFields.filter((_, i) => i !== idx);
  spec.value = { ...props.spec, multipartFields: next };
}

// ---- cURL 导入 ----
const importOpen = ref(false);
const importText = ref('');
const importError = ref('');

function openImport() { importOpen.value = true; importError.value = ''; importText.value = ''; }
function doImport() {
  const result = parseCurl(importText.value);
  if ('error' in result) {
    importError.value = result.error;
    return;
  }
  emit('apply-spec', result.spec);
  importOpen.value = false;
}

const menuOptions = computed(() => [
  { label: '从 cURL 导入…', key: 'import' },
  { label: '复制为 cURL（模板）', key: 'copy-template' },
  { label: '复制为 cURL（已展开）', key: 'copy-resolved' }
]);
function onMenu(key: string) {
  if (key === 'import') openImport();
  else if (key === 'copy-template') emit('copy-curl', true);
  else emit('copy-curl', false);
}

const currentCurl = computed(() => toCurl(props.spec, null));

// ---- header count / warnings ----
const activeHeaderCount = computed(() => props.spec.headers.filter((h) => h.enabled && h.key).length);
const activeParamCount = computed(() => props.spec.queryParams.filter((p) => p.enabled && p.key).length);
const urlUnknownVars = computed(() => collectUnknownVars(props.spec.url, props.varContext));

function updateBody(value: string) {
  jsonError.value = '';
  spec.value = { ...props.spec, body: value };
}

function formatJson() {
  const result = formatJsonBody(props.spec.body);
  if (!result.ok) {
    jsonError.value = result.error;
    bodyEditor.value?.focus();
    return;
  }
  jsonError.value = '';
  bodyEditor.value?.replaceValue(result.value);
}

function submit() {
  if (!bodyDisabled.value && props.spec.bodyType === 'json') {
    const result = validateJsonBody(props.spec.body);
    if (!result.ok) {
      requestTab.value = 'body';
      jsonError.value = result.error;
      void nextTick(() => bodyEditor.value?.focus());
      return;
    }
  }
  emit('send');
}

defineExpose({ submit });
</script>

<template>
  <div class="request-editor">
    <div class="url-row">
      <n-select
        :value="spec.method"
        :options="methodOptions"
        size="small"
        style="width: 110px"
        @update:value="(v: HttpMethod) => spec = { ...spec, method: v }"
      />
      <div class="url-input-wrap">
        <StableTextField
          class="url-input mono"
          :model-value="spec.url"
          placeholder="https://{{baseUrl}}/path"
          @update:model-value="updateUrl"
          @scroll="syncUrlScroll"
          @keyup.enter="submit"
        />
        <div ref="urlOverlay" class="url-overlay mono" v-html="urlHtml" aria-hidden="true"></div>
      </div>
      <n-button v-if="!sending" type="primary" @click="submit">
        <span class="send-label">发送<Kbd>⌘↩</Kbd></span>
      </n-button>
      <n-button v-else tertiary @click="emit('cancel')">取消</n-button>
      <n-dropdown :options="menuOptions" trigger="click" @select="onMenu">
        <n-button quaternary>⋯</n-button>
      </n-dropdown>
    </div>

    <div v-if="urlUnknownVars.length" class="var-warn">
      变量未定义：{{ urlUnknownVars.join(', ') }}
    </div>

    <n-tabs v-model:value="requestTab" type="line" size="small" pane-class="tab-pane">
      <n-tab-pane name="params" :tab="`Params (${activeParamCount})`">
        <div class="kv-table">
          <div v-for="(row, i) in spec.queryParams" :key="i" class="kv-row">
            <input type="checkbox" v-model="row.enabled" />
            <StableTextField class="kv-input mono" v-model="row.key" placeholder="参数名" />
            <StableTextField class="kv-input mono" v-model="row.value" placeholder="值 (支持 {{var}})" />
            <button class="kv-del" @click="updateParams(spec.queryParams.filter((_, j) => j !== i))">✕</button>
          </div>
          <n-button size="tiny" secondary @click="updateParams(addRow(spec.queryParams))">+ 添加 param</n-button>
        </div>
      </n-tab-pane>

      <n-tab-pane name="auth" :tab="`Auth · ${spec.auth.type}`">
        <div class="auth-pane">
          <n-select
            :value="spec.auth.type"
            :options="authTypeOptions"
            size="small"
            style="width: 180px"
            @update:value="updateAuthType"
          />
          <div v-if="spec.auth.type === 'bearer'" class="auth-row">
            <label>Token</label>
            <StableTextField
              class="kv-input mono"
              type="password"
              :model-value="spec.auth.bearerToken ?? ''"
              placeholder="{{token}} 或 eyJhb..."
              @update:model-value="(value: string) => updateAuthField('bearerToken', value)"
            />
          </div>
          <div v-if="spec.auth.type === 'basic'" class="auth-row auth-basic">
            <label>用户名</label>
            <StableTextField
              class="kv-input mono"
              :model-value="spec.auth.basicUser ?? ''"
              @update:model-value="(value: string) => updateAuthField('basicUser', value)"
            />
            <label>密码</label>
            <StableTextField
              class="kv-input mono"
              type="password"
              :model-value="spec.auth.basicPass ?? ''"
              @update:model-value="(value: string) => updateAuthField('basicPass', value)"
            />
          </div>
          <p v-if="spec.auth.type !== 'none'" class="muted">
            认证信息会作为 Authorization header 自动发送，不出现在 Headers 面板中。
          </p>
        </div>
      </n-tab-pane>

      <n-tab-pane name="headers" :tab="`Headers (${activeHeaderCount})`">
        <div class="kv-table">
          <div v-for="(row, i) in spec.headers" :key="i" class="kv-row">
            <input type="checkbox" v-model="row.enabled" />
            <StableTextField class="kv-input mono" v-model="row.key" placeholder="Header 名" />
            <StableTextField class="kv-input mono" v-model="row.value" placeholder="值 (支持 {{var}})" />
            <button class="kv-del" @click="updateHeaders(spec.headers.filter((_, j) => j !== i))">✕</button>
          </div>
          <n-button size="tiny" secondary @click="updateHeaders(addRow(spec.headers))">+ 添加 header</n-button>
        </div>
      </n-tab-pane>

      <n-tab-pane name="body" :tab="`Body · ${spec.bodyType}`">
        <div class="body-pane">
          <div class="body-opts">
            <n-select
              :value="spec.bodyType"
              :options="bodyTypeOptions"
              size="small"
              style="width: 160px"
              :disabled="bodyDisabled"
              @update:value="(v: any) => spec = { ...spec, bodyType: v }"
            />
            <span v-if="bodyDisabled" class="muted">{{ spec.method }} 请求不带 body</span>
            <n-button
              v-if="!bodyDisabled && spec.bodyType === 'json'"
              size="tiny"
              secondary
              @click="formatJson"
            >格式化</n-button>
          </div>

          <CodeEditor
            v-if="!bodyDisabled && (spec.bodyType === 'json' || spec.bodyType === 'text')"
            ref="bodyEditor"
            :model-value="spec.body"
            :language="spec.bodyType === 'json' ? 'json' : 'plaintext'"
            height="clamp(180px, 34vh, 360px)"
            @update:model-value="updateBody"
          />
          <p v-if="jsonError && spec.bodyType === 'json'" class="body-error">{{ jsonError }}</p>

          <div v-if="!bodyDisabled && spec.bodyType === 'form'" class="kv-table">
            <div v-for="(row, i) in spec.queryParams.filter(() => false)" :key="i">
              <!-- keep type -->
            </div>
            <StableTextField
              kind="naive"
              :model-value="spec.body"
              type="textarea"
              placeholder="k1=v1&k2=v2"
              :autosize="{ minRows: 4, maxRows: 12 }"
              @update:model-value="updateBody"
            />
          </div>

          <div v-if="!bodyDisabled && spec.bodyType === 'multipart'" class="kv-table">
            <div v-for="(field, i) in spec.multipartFields" :key="i" class="mp-row">
              <input type="checkbox" :checked="field.enabled" @change="(e: any) => updateMultipart(i, { enabled: e.target.checked })" />
              <StableTextField
                class="kv-input mono"
                :model-value="field.key"
                placeholder="字段名"
                @update:model-value="(value: string) => updateMultipart(i, { key: value })"
              />
              <n-select
                :value="field.kind"
                :options="[{label: 'Text', value: 'text'}, {label: 'File', value: 'file'}]"
                size="tiny"
                style="width: 90px"
                @update:value="(v: any) => updateMultipart(i, { kind: v, value: '' })"
              />
              <template v-if="field.kind === 'text'">
                <StableTextField
                  class="kv-input mono"
                  :model-value="field.value"
                  placeholder="值 (支持 {{var}})"
                  @update:model-value="(value: string) => updateMultipart(i, { value })"
                />
              </template>
              <template v-else>
                <n-button size="tiny" secondary @click="pickFile(i)">选择文件…</n-button>
                <span class="mp-file mono">{{ field.value || '（未选）' }}</span>
              </template>
              <button class="kv-del" @click="removeMultipart(i)">✕</button>
            </div>
            <n-button size="tiny" secondary @click="addMultipartField">+ 添加字段</n-button>
          </div>
        </div>
      </n-tab-pane>

      <n-tab-pane name="settings" tab="Settings">
        <div class="settings">
          <label class="opt">
            <span>超时（秒）</span>
            <n-input-number
              :value="spec.timeoutSeconds"
              :min="1"
              :max="300"
              size="small"
              style="width: 100px"
              @update:value="(v: number | null) => spec = { ...spec, timeoutSeconds: v ?? 30 }"
            />
          </label>
          <label class="opt">
            <input
              type="checkbox"
              :checked="spec.followRedirects"
              @change="(e: any) => spec = { ...spec, followRedirects: e.target.checked }"
            />
            <span>跟随 301 / 302（最多 10 次）</span>
          </label>
          <label class="opt">
            <input
              type="checkbox"
              :checked="spec.verifySsl"
              @change="(e: any) => spec = { ...spec, verifySsl: e.target.checked }"
            />
            <span>校验 SSL 证书</span>
          </label>
          <label class="opt">
            <input
              type="checkbox"
              :checked="spec.saveToHistory"
              @change="(e: any) => spec = { ...spec, saveToHistory: e.target.checked }"
            />
            <span>保存到历史</span>
          </label>
        </div>
      </n-tab-pane>
    </n-tabs>

    <n-modal v-model:show="importOpen" preset="card" style="width: 640px" title="从 cURL 导入">
      <div class="import-modal">
        <n-input
          v-model:value="importText"
          type="textarea"
          placeholder="粘贴 curl 命令……"
          :autosize="{ minRows: 8, maxRows: 20 }"
        />
        <p v-if="importError" class="err">{{ importError }}</p>
        <div class="import-actions">
          <n-button @click="importOpen = false">取消</n-button>
          <n-button type="primary" @click="doImport">导入</n-button>
        </div>
        <details v-if="!importText" class="hint">
          <summary>当前请求的 cURL 表示</summary>
          <pre class="mono">{{ currentCurl }}</pre>
        </details>
      </div>
    </n-modal>
  </div>
</template>

<style scoped>
.request-editor { display: grid; gap: 8px; padding: 8px 12px; }
.url-row {
  display: flex;
  gap: 6px;
  align-items: center;
  padding: 4px;
  background: var(--bg-elev-2);
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-md);
}
.send-label {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}
.url-input-wrap {
  flex: 1;
  position: relative;
  display: flex;
  align-items: stretch;
  min-width: 0;
  overflow: hidden;
}
.url-input {
  flex: 1;
  min-width: 0;
  padding: 6px 10px;
  background: var(--bg-elev);
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  color: transparent;
  caret-color: var(--text);
  font-family: var(--font-mono, monospace);
  font-size: var(--fs-xs);
  line-height: 18px;
  box-sizing: border-box;
  outline: none;
  position: relative;
  z-index: 2;
  width: 100%;
}
.url-input:focus { border-color: var(--accent); }
.url-overlay {
  position: absolute;
  inset: 1px;
  padding: 6px 10px;
  pointer-events: none;
  font-family: var(--font-mono, monospace);
  font-size: var(--fs-xs);
  line-height: 18px;
  box-sizing: border-box;
  color: var(--text);
  white-space: pre;
  overflow: hidden;
  z-index: 1;
}
:deep(.var-hit) { background: var(--accent-soft); border-radius: 3px; }
:deep(.var-miss) { background: var(--error-soft); border-radius: 3px; }

.var-warn { color: var(--warning); font-size: var(--fs-xxs); padding: 2px 4px; }

.kv-table { display: grid; gap: 0; padding: 4px 0; }
.kv-row {
  display: grid;
  grid-template-columns: 24px 1fr 2fr 32px;
  gap: 6px;
  align-items: center;
  padding: 3px 4px;
  border-radius: var(--radius-sm);
}
.kv-row + .kv-row { border-top: 1px solid var(--line); }
.kv-row input[type="checkbox"] {
  appearance: none;
  -webkit-appearance: none;
  width: 14px;
  height: 14px;
  border: 1.5px solid var(--line-strong);
  border-radius: var(--radius-sm);
  background: transparent;
  cursor: pointer;
  display: grid;
  place-items: center;
  position: relative;
}
.kv-row input[type="checkbox"]:checked {
  background: var(--accent);
  border-color: var(--accent);
}
.kv-row input[type="checkbox"]:checked::after {
  content: "✓";
  color: var(--accent-fg);
  font-size: 10px;
  font-weight: 700;
  line-height: 1;
}
.mp-row {
  display: grid;
  grid-template-columns: 24px 1fr 90px 2fr 32px;
  gap: 6px;
  align-items: center;
}
.mp-file { color: var(--text-muted); font-size: var(--fs-xxs); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.kv-input {
  padding: 4px 8px;
  background: var(--bg-elev);
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  color: var(--text);
  font-size: var(--fs-xs);
  font-family: var(--font-mono, monospace);
  outline: none;
}
.kv-input:focus { border-color: var(--accent); }
.kv-del { background: none; border: none; color: var(--text-muted); cursor: pointer; }
.kv-del:hover { color: var(--error); }

.body-pane { display: grid; gap: 10px; padding: 4px 0; }
.body-opts { display: flex; align-items: center; gap: 12px; }
.body-opts .n-button { margin-left: auto; }
.body-error {
  margin: 0;
  padding: 6px 8px;
  color: var(--error);
  background: var(--error-soft);
  border-radius: var(--radius-sm);
  font-size: var(--fs-xxs);
}

.settings { display: grid; gap: 10px; padding: 4px 0; }
.opt { display: flex; align-items: center; gap: 8px; font-size: var(--fs-xs); }

.auth-pane { display: grid; gap: 10px; padding: 4px 0; }
.auth-row { display: grid; grid-template-columns: 80px 1fr; gap: 8px; align-items: center; }
.auth-basic { grid-template-columns: 80px 1fr 80px 1fr; }

.muted { color: var(--text-muted); font-size: var(--fs-xs); }
.mono { font-family: var(--font-mono, ui-monospace, monospace); font-variant-numeric: tabular-nums; }

.import-modal { display: grid; gap: 10px; }
.import-actions { display: flex; justify-content: flex-end; gap: 8px; }
.err { color: var(--error); font-size: var(--fs-xs); }
.hint pre {
  background: var(--bg-elev);
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  font-size: var(--fs-xxs);
  color: var(--text-muted);
  white-space: pre-wrap;
  max-height: 200px;
  overflow: auto;
}
</style>
