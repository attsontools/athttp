<script setup lang="ts">
import { computed } from 'vue';
import { NButton } from 'naive-ui';
import type { HttpTab, SseSpec } from './types';
import SseRequestEditor from './SseRequestEditor.vue';
import StreamMessageList from './StreamMessageList.vue';
import { useHttpStore } from '../../composables/useHttpStore';

const props = defineProps<{ tab: HttpTab }>();
const store = useHttpStore();

const spec = computed(() => props.tab.spec as SseSpec);
const status = computed(() => props.tab.session?.status ?? 'idle');
const canConnect = computed(() => status.value === 'idle' || status.value === 'closed' || status.value === 'error');
const canDisconnect = computed(() => status.value === 'connecting' || status.value === 'open');

function updateSpec(next: SseSpec) {
  props.tab.spec = next;
}

async function connect() {
  await store.openStream(props.tab.id, 'sse', spec.value);
}
async function disconnect() {
  await store.closeStream(props.tab.id);
}
</script>

<template>
  <div class="sse-tool">
    <SseRequestEditor
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
  </div>
</template>

<style scoped>
.sse-tool { display: flex; flex-direction: column; height: 100%; }
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
</style>
