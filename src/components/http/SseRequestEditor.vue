<script setup lang="ts">
import { NButton, NInput, NInputNumber, NSelect, NTabs, NTabPane } from 'naive-ui';
import type { SseSpec, KV } from './types';

const props = defineProps<{ spec: SseSpec; disabled: boolean }>();
const emit = defineEmits<{ (e: 'update:spec', v: SseSpec): void }>();

function set<K extends keyof SseSpec>(k: K, v: SseSpec[K]) {
  emit('update:spec', { ...props.spec, [k]: v });
}

const authTypeOptions = [
  { label: '不认证', value: 'none' },
  { label: 'Bearer Token', value: 'bearer' },
  { label: 'Basic Auth', value: 'basic' },
];

function addRow(list: KV[]): KV[] {
  return [...list, { key: '', value: '', enabled: true }];
}
</script>

<template>
  <div class="editor">
    <div class="url-row">
      <input
        class="url-input mono"
        :value="spec.url"
        placeholder="https://api.example.com/stream"
        :disabled="disabled"
        @input="(e: any) => set('url', e.target.value)"
      />
    </div>

    <n-tabs type="line" size="small">
      <n-tab-pane name="params" tab="Params">
        <div v-for="(row, i) in spec.queryParams" :key="i" class="kv-row">
          <input type="checkbox" v-model="row.enabled" :disabled="disabled" />
          <input class="kv-input mono" v-model="row.key" placeholder="key" :disabled="disabled" />
          <input class="kv-input mono" v-model="row.value" placeholder="value ({{var}})" :disabled="disabled" />
          <button class="kv-del" :disabled="disabled" @click="set('queryParams', spec.queryParams.filter((_, j) => j !== i))">✕</button>
        </div>
        <n-button size="tiny" secondary :disabled="disabled" @click="set('queryParams', addRow(spec.queryParams))">+ 添加 param</n-button>
      </n-tab-pane>

      <n-tab-pane name="headers" tab="Headers">
        <div v-for="(row, i) in spec.headers" :key="i" class="kv-row">
          <input type="checkbox" v-model="row.enabled" :disabled="disabled" />
          <input class="kv-input mono" v-model="row.key" placeholder="Header" :disabled="disabled" />
          <input class="kv-input mono" v-model="row.value" placeholder="值 ({{var}})" :disabled="disabled" />
          <button class="kv-del" :disabled="disabled" @click="set('headers', spec.headers.filter((_, j) => j !== i))">✕</button>
        </div>
        <n-button size="tiny" secondary :disabled="disabled" @click="set('headers', addRow(spec.headers))">+ 添加 header</n-button>
      </n-tab-pane>

      <n-tab-pane name="auth" :tab="`Auth · ${spec.auth.type}`">
        <n-select
          :value="spec.auth.type"
          :options="authTypeOptions"
          size="small"
          :disabled="disabled"
          style="width: 180px"
          @update:value="(v: any) => set('auth', { ...spec.auth, type: v })"
        />
        <div v-if="spec.auth.type === 'bearer'" class="auth-row">
          <input
            class="kv-input mono"
            type="password"
            :value="spec.auth.bearerToken ?? ''"
            placeholder="{{token}} 或 eyJ..."
            :disabled="disabled"
            @input="(e: any) => set('auth', { ...spec.auth, bearerToken: e.target.value })"
          />
        </div>
        <div v-if="spec.auth.type === 'basic'" class="auth-row auth-basic">
          <input
            class="kv-input mono"
            :value="spec.auth.basicUser ?? ''"
            placeholder="用户名"
            :disabled="disabled"
            @input="(e: any) => set('auth', { ...spec.auth, basicUser: e.target.value })"
          />
          <input
            class="kv-input mono"
            type="password"
            :value="spec.auth.basicPass ?? ''"
            placeholder="密码"
            :disabled="disabled"
            @input="(e: any) => set('auth', { ...spec.auth, basicPass: e.target.value })"
          />
        </div>
      </n-tab-pane>

      <n-tab-pane name="settings" tab="Settings">
        <label class="opt">
          <span>连接超时（秒）</span>
          <n-input-number
            :value="spec.timeoutSeconds ?? 30"
            :min="1"
            :max="300"
            size="small"
            style="width: 100px"
            :disabled="disabled"
            @update:value="(v: number | null) => set('timeoutSeconds', v ?? 30)"
          />
        </label>
        <label class="opt">
          <input type="checkbox" :checked="spec.verifySsl" :disabled="disabled" @change="(e: any) => set('verifySsl', e.target.checked)" />
          <span>校验 SSL 证书</span>
        </label>
        <label class="opt">
          <span>Last-Event-ID（重连用）</span>
          <n-input
            :value="spec.lastEventId ?? ''"
            size="small"
            :disabled="disabled"
            style="width: 220px"
            @update:value="(v: string) => set('lastEventId', v || undefined)"
          />
        </label>
      </n-tab-pane>
    </n-tabs>
  </div>
</template>

<style scoped>
.editor { display: grid; gap: 8px; padding: 8px 12px; }
.url-row { display: flex; gap: 6px; }
.url-input {
  flex: 1;
  padding: 6px 10px;
  background: var(--bg-elev);
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  color: var(--text);
  font-family: var(--font-mono, monospace);
  font-size: var(--fs-xs);
  outline: none;
}
.url-input:focus { border-color: var(--accent, #10b981); }
.kv-row { display: grid; grid-template-columns: 24px 1fr 2fr 32px; gap: 6px; align-items: center; padding: 2px 0; }
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
.kv-del { background: none; border: none; color: var(--text-muted); cursor: pointer; }
.auth-row { display: grid; grid-template-columns: 1fr; gap: 8px; padding-top: 8px; }
.auth-basic { grid-template-columns: 1fr 1fr; }
.opt { display: flex; align-items: center; gap: 8px; font-size: var(--fs-xs); padding: 4px 0; }
.mono { font-family: var(--font-mono, ui-monospace, monospace); }
</style>
