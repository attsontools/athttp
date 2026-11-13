<script setup lang="ts">
import { computed, ref } from 'vue';
import { NButton, NInput, NSelect } from 'naive-ui';
import type { HttpTab, WsSpec, WsTemplate } from './types';
import WsRequestEditor from './WsRequestEditor.vue';
import StreamMessageList from './StreamMessageList.vue';
import { useHttpStore } from '../../composables/useHttpStore';

const props = defineProps<{ tab: HttpTab }>();
const store = useHttpStore();

const spec = computed(() => props.tab.spec as WsSpec);
const status = computed(() => props.tab.session?.status ?? 'idle');
const canConnect = computed(() => status.value === 'idle' || status.value === 'closed' || status.value === 'error');
const canDisconnect = computed(() => status.value === 'connecting' || status.value === 'open');
const canSend = computed(() => status.value === 'open');

const draft = ref('');
const templateName = ref('');

function updateSpec(next: WsSpec) {
  props.tab.spec = next;
}

async function connect() {
  await store.openStream(props.tab.id, 'ws', spec.value);
}
async function disconnect() {
  await store.closeStream(props.tab.id);
}
async function send() {
  if (!canSend.value || !draft.value) return;
  await store.sendWsMessage(props.tab.id, draft.value);
}

const templateOptions = computed(() =>
  spec.value.templates.map((t: WsTemplate) => ({ label: t.name, value: t.name }))
);
function loadTemplate(name: string) {
  const t = spec.value.templates.find((x) => x.name === name);
  if (t) draft.value = t.text;
}
function saveTemplate() {
  const name = templateName.value.trim();
  if (!name || !draft.value) return;
  const next = spec.value.templates.filter((t) => t.name !== name);
  next.push({ name, text: draft.value });
  updateSpec({ ...spec.value, templates: next });
  templateName.value = '';
}
function deleteTemplate(name: string) {
  updateSpec({ ...spec.value, templates: spec.value.templates.filter((t) => t.name !== name) });
}
</script>

<template>
  <div class="ws-tool">
    <WsRequestEditor
      :spec="spec"
      :disabled="!canConnect"
      @update:spec="updateSpec"
    />
    <div class="ctrl">
      <span :class="['status', `st-${status}`]">{{ status.toUpperCase() }}</span>
      <n-button v-if="canConnect" type="primary" size="small" @click="connect">连接</n-button>
      <n-button v-if="canDisconnect" tertiary size="small" @click="disconnect">断开</n-button>
    </div>
    <StreamMessageList :messages="tab.messages ?? []" />
    <div class="send">
      <div class="tpl-row">
        <n-select
          :options="templateOptions"
          size="small"
          placeholder="加载模板…"
          clearable
          style="width: 160px"
          @update:value="(v: string | null) => v && loadTemplate(v)"
        />
        <n-input
          v-model:value="templateName"
          size="small"
          placeholder="模板名"
          style="width: 120px"
        />
        <n-button size="small" secondary :disabled="!templateName || !draft" @click="saveTemplate">保存模板</n-button>
        <n-button
          v-if="spec.templates.length"
          size="small"
          quaternary
          @click="deleteTemplate(templateName)"
        >删除同名</n-button>
      </div>
      <n-input
        v-model:value="draft"
        type="textarea"
        placeholder="要发送的文本 (支持 {{var}})"
        :autosize="{ minRows: 2, maxRows: 6 }"
      />
      <n-button type="primary" :disabled="!canSend || !draft" @click="send">发送</n-button>
    </div>
  </div>
</template>

<style scoped>
.ws-tool { display: flex; flex-direction: column; height: 100%; }
.ctrl {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 12px;
  border-top: 1px solid var(--line);
  border-bottom: 1px solid var(--line);
}
.status {
  font-family: var(--font-mono, monospace);
  font-size: var(--fs-xxs);
  padding: 2px 6px;
  border-radius: var(--radius-sm);
}
.st-idle { color: var(--text-muted); border: 1px solid var(--line); }
.st-connecting { color: #f59e0b; border: 1px solid #f59e0b; }
.st-open { color: #10b981; border: 1px solid #10b981; }
.st-closed { color: var(--text-muted); border: 1px solid var(--line); }
.st-error { color: #ef4444; border: 1px solid #ef4444; }
.send {
  display: grid;
  gap: 6px;
  padding: 8px 12px;
  border-top: 1px solid var(--line);
  background: var(--bg-elev);
}
.tpl-row { display: flex; gap: 6px; align-items: center; }
</style>
