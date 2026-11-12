<script setup lang="ts">
import { computed, ref } from 'vue';
import { NButton, NTabs, NTabPane } from 'naive-ui';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { save } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import type { HttpResponseInfo } from './types';

const props = defineProps<{
  response: HttpResponseInfo | null;
  error: string | null;
  sending: boolean;
}>();

const respTab = ref('body');
const bodyView = ref<'pretty' | 'raw' | 'preview'>('pretty');

const contentType = computed(() => {
  const h = props.response?.headers.find(([k]) => k.toLowerCase() === 'content-type');
  return (h?.[1] ?? '').toLowerCase();
});

const isJson = computed(() => contentType.value.includes('json'));
const isXml = computed(() => contentType.value.includes('xml'));
const isHtml = computed(() => contentType.value.includes('html'));
const isImage = computed(() => contentType.value.startsWith('image/'));
const isBinary = computed(
  () =>
    isImage.value ||
    contentType.value.includes('octet-stream') ||
    contentType.value.includes('pdf')
);

const previewAvailable = computed(() => isHtml.value || isImage.value);

const rawBody = computed(() => props.response?.body ?? '');

const prettyBody = computed(() => {
  if (!props.response) return '';
  const text = props.response.body;
  if (isJson.value) {
    try {
      return JSON.stringify(JSON.parse(text), null, 2);
    } catch {
      return text;
    }
  }
  return text;
});

const displayBody = computed(() => {
  const src = bodyView.value === 'raw' ? rawBody.value : prettyBody.value;
  if (src.length > 2 * 1024 * 1024) return src.slice(0, 2 * 1024 * 1024);
  return src;
});

const truncated = computed(() => (rawBody.value.length > 2 * 1024 * 1024 ? true : false));

const previewImageSrc = computed(() => {
  if (!props.response || !isImage.value) return '';
  const b64 = btoa(unescape(encodeURIComponent(props.response.body)));
  return `data:${contentType.value};base64,${b64}`;
});

function statusClass(): string {
  const s = props.response?.status ?? 0;
  if (s === 0) return 'st-err';
  if (s < 300) return 'st-2';
  if (s < 400) return 'st-3';
  if (s < 500) return 'st-4';
  return 'st-5';
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

async function copyBody() {
  if (!props.response) return;
  try {
    await writeText(bodyView.value === 'raw' ? rawBody.value : prettyBody.value);
  } catch {}
}

async function copyUrl() {
  if (!props.response) return;
  try {
    await writeText(props.response.finalUrl);
  } catch {}
}

async function saveBody() {
  if (!props.response) return;
  const path = await save({ defaultPath: 'response.txt' });
  if (!path) return;
  try {
    const bytes = new TextEncoder().encode(rawBody.value);
    // btoa 需要单字节字符串
    let binary = '';
    for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]);
    const b64 = btoa(binary);
    await invoke('write_binary_file', { path, base64: b64 });
  } catch (err) {
    console.warn('save failed', err);
  }
}

interface CookieInfo {
  raw: string;
  name: string;
  value: string;
  attributes: Record<string, string | boolean>;
}

const cookies = computed<CookieInfo[]>(() => {
  if (!props.response) return [];
  const list: CookieInfo[] = [];
  for (const [k, v] of props.response.headers) {
    if (k.toLowerCase() === 'set-cookie') list.push(parseCookie(v));
  }
  return list;
});

function parseCookie(raw: string): CookieInfo {
  const parts = raw.split(';').map((p) => p.trim());
  const first = parts.shift() ?? '';
  const eq = first.indexOf('=');
  const name = eq >= 0 ? first.slice(0, eq) : first;
  const value = eq >= 0 ? first.slice(eq + 1) : '';
  const attrs: Record<string, string | boolean> = {};
  for (const p of parts) {
    const eqp = p.indexOf('=');
    if (eqp >= 0) attrs[p.slice(0, eqp)] = p.slice(eqp + 1);
    else attrs[p] = true;
  }
  return { raw, name, value, attributes: attrs };
}
</script>

<template>
  <div class="response-view">
    <div v-if="!response && !error && !sending" class="empty">
      发送请求后在此查看响应
    </div>

    <div v-if="error" class="err">{{ error }}</div>

    <template v-if="response">
      <div class="summary">
        <span :class="['status-pill', 'mono', statusClass()]">
          <span class="dot" aria-hidden="true"></span>
          <span class="code">{{ response.status }}</span>
          <span class="text">{{ response.statusText }}</span>
        </span>
        <span class="muted mono">{{ response.elapsedMs }} ms · {{ formatSize(response.bodyBytes) }}</span>
        <span class="url mono" :title="response.finalUrl">{{ response.finalUrl }}</span>
        <n-button size="tiny" quaternary @click="copyUrl">复制 URL</n-button>
      </div>

      <n-tabs v-model:value="respTab" type="line" size="small">
        <n-tab-pane name="body" tab="Body">
          <div class="body-toolbar">
            <div class="view-toggle">
              <button :class="{ on: bodyView === 'pretty' }" @click="bodyView = 'pretty'">Pretty</button>
              <button :class="{ on: bodyView === 'raw' }" @click="bodyView = 'raw'">Raw</button>
              <button
                v-if="previewAvailable"
                :class="{ on: bodyView === 'preview' }"
                @click="bodyView = 'preview'"
              >Preview</button>
            </div>
            <div class="body-actions">
              <n-button size="tiny" secondary @click="copyBody">复制</n-button>
              <n-button size="tiny" secondary @click="saveBody">保存为文件</n-button>
            </div>
          </div>

          <div v-if="truncated" class="warn">
            响应超过 2 MB，仅显示前 2 MB · 使用「保存为文件」获取完整响应
          </div>

          <template v-if="bodyView === 'preview' && isHtml">
            <iframe class="preview-iframe" sandbox="allow-same-origin" :srcdoc="rawBody" />
          </template>
          <template v-else-if="bodyView === 'preview' && isImage">
            <div class="preview-img">
              <img :src="previewImageSrc" alt="response" />
            </div>
          </template>
          <template v-else>
            <pre v-if="!isBinary || bodyView === 'raw'" :class="['body', 'mono']">{{ displayBody }}</pre>
            <div v-else class="binary-hint">
              二进制内容（{{ contentType }}），请切换到 Preview 或保存为文件
            </div>
          </template>
        </n-tab-pane>

        <n-tab-pane name="headers" :tab="`Headers (${response.headers.length})`">
          <table class="headers">
            <tbody>
              <tr v-for="([k, v]) in response.headers" :key="k">
                <td class="k mono">{{ k }}</td>
                <td class="v mono">{{ v }}</td>
              </tr>
            </tbody>
          </table>
        </n-tab-pane>

        <n-tab-pane name="cookies" :tab="`Cookies (${cookies.length})`">
          <div v-if="cookies.length === 0" class="empty">无 Set-Cookie</div>
          <table v-else class="cookies">
            <thead>
              <tr>
                <th>Name</th>
                <th>Value</th>
                <th>Attributes</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="(c, i) in cookies" :key="i">
                <td class="mono">{{ c.name }}</td>
                <td class="mono">{{ c.value }}</td>
                <td class="mono attrs">
                  <span v-for="(v, k) in c.attributes" :key="k">
                    {{ k }}<template v-if="v !== true">={{ v }}</template>
                  </span>
                </td>
              </tr>
            </tbody>
          </table>
        </n-tab-pane>
      </n-tabs>
    </template>
  </div>
</template>

<style scoped>
.response-view { padding: 8px 12px; display: grid; gap: 8px; }
.empty { color: var(--text-muted); font-size: var(--fs-xs); text-align: center; padding: 24px; }
.err {
  color: var(--error);
  background: var(--error-soft);
  border: 1px solid var(--error-soft);
  border-radius: var(--radius-sm);
  padding: 8px 10px;
  font-size: var(--fs-xs);
}
.summary { display: flex; align-items: center; gap: 8px; font-size: var(--fs-xs); }
.summary .url { flex: 1; color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.status-pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 2px 10px;
  border-radius: var(--radius-pill);
  font-weight: 600;
  background: var(--accent-soft);
  color: var(--accent);
  border: 1px solid var(--accent-line);
}
.status-pill .dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: currentColor;
  box-shadow: 0 0 0 3px var(--accent-soft);
}
.status-pill.st-3 {
  background: var(--info-soft);
  color: var(--info);
  border-color: var(--info-soft);
}
.status-pill.st-3 .dot { box-shadow: 0 0 0 3px var(--info-soft); }
.status-pill.st-4 {
  background: var(--warning-soft);
  color: var(--warning);
  border-color: var(--warning-soft);
}
.status-pill.st-4 .dot { box-shadow: 0 0 0 3px var(--warning-soft); }
.status-pill.st-5, .status-pill.st-err {
  background: var(--error-soft);
  color: var(--error);
  border-color: var(--error-soft);
}
.status-pill.st-5 .dot, .status-pill.st-err .dot { box-shadow: 0 0 0 3px var(--error-soft); }
.muted { color: var(--text-muted); }
.mono { font-family: var(--font-mono, ui-monospace, monospace); font-variant-numeric: tabular-nums; }

.body-toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 4px 0;
}
.view-toggle {
  display: inline-flex;
  gap: 2px;
  padding: 2px;
  background: var(--bg-elev-2);
  border: 1px solid var(--line-strong);
  border-radius: var(--radius);
}
.view-toggle button {
  background: none;
  border: 0;
  color: var(--text-muted);
  padding: 3px 10px;
  font-size: var(--fs-xxs);
  border-radius: var(--radius-sm);
  cursor: pointer;
}
.view-toggle button:hover { color: var(--text); }
.view-toggle button.on {
  background: var(--bg-overlay);
  color: var(--text);
}
.body-actions { display: flex; gap: 4px; }

.warn {
  background: var(--warning-soft);
  color: var(--warning);
  padding: 4px 8px;
  font-size: var(--fs-xxs);
  border-radius: var(--radius-sm);
}
.body {
  margin: 0;
  padding: 10px 12px;
  background: var(--bg-elev);
  border-radius: var(--radius-sm);
  font-size: var(--fs-xs);
  color: var(--text);
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 500px;
  overflow: auto;
}
.preview-iframe {
  width: 100%;
  height: 500px;
  background: white;
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
}
.preview-img {
  display: flex;
  justify-content: center;
  padding: 12px;
  background: var(--bg-elev);
  border-radius: var(--radius-sm);
}
.preview-img img { max-width: 100%; max-height: 500px; }
.binary-hint {
  padding: 24px;
  text-align: center;
  color: var(--text-muted);
  font-size: var(--fs-xs);
  background: var(--bg-elev);
  border-radius: var(--radius-sm);
}
.headers, .cookies { width: 100%; border-collapse: collapse; font-size: var(--fs-xs); }
.headers td, .cookies td, .cookies th {
  padding: 4px 10px;
  border-bottom: 1px solid var(--line-weak, var(--line));
  vertical-align: top;
  text-align: left;
}
.cookies th { color: var(--text-muted); font-weight: 500; font-size: var(--fs-xxs); }
.headers .k { color: var(--text-muted); width: 30%; }
.headers .v { word-break: break-all; }
.attrs span { display: inline-block; margin-right: 8px; color: var(--text-muted); }
</style>
