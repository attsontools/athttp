<script setup lang="ts">
import type { HttpTab, HttpRequestSpec, TabKind } from './types';
import { NDropdown } from 'naive-ui';

defineProps<{ tabs: HttpTab[]; activeId: string | null }>();
const emit = defineEmits<{
  (e: 'activate', id: string): void;
  (e: 'close', id: string): void;
  (e: 'new', kind: TabKind): void;
}>();

function onMouseDown(id: string, ev: MouseEvent) {
  if (ev.button === 1) {
    ev.preventDefault();
    emit('close', id);
  } else if (ev.button === 0) {
    emit('activate', id);
  }
}

const newOptions = [
  { label: '新 HTTP 请求', key: 'http' },
  { label: '新 SSE 会话', key: 'sse' },
  { label: '新 WebSocket 会话', key: 'ws' },
];

function onNewSelect(key: 'http' | 'sse' | 'ws') {
  emit('new', key);
}
</script>

<template>
  <div class="tab-bar">
    <div
      v-for="tab in tabs"
      :key="tab.id"
      :class="['tab', { active: tab.id === activeId, sending: tab.sending }]"
      @mousedown="onMouseDown(tab.id, $event)"
      :title="tab.title"
    >
      <span
        v-if="tab.kind === 'http'"
        class="method mono"
        :class="`m-${(tab.spec as HttpRequestSpec).method.toLowerCase()}`"
      >{{ (tab.spec as HttpRequestSpec).method }}</span>
      <span v-else-if="tab.kind === 'sse'" class="method mono m-sse">SSE</span>
      <span v-else-if="tab.kind === 'ws'"  class="method mono m-ws">WS</span>
      <span class="title">{{ tab.title || '新请求' }}</span>
      <button class="close" @click.stop="emit('close', tab.id)">✕</button>
    </div>
    <n-dropdown :options="newOptions" trigger="click" @select="onNewSelect">
      <button class="new" title="新 tab (⌘T)">+</button>
    </n-dropdown>
  </div>
</template>

<style scoped>
.tab-bar {
  display: flex;
  align-items: stretch;
  border-bottom: 1px solid var(--line);
  background: var(--bg-base);
  overflow-x: auto;
  scrollbar-width: thin;
}
.tab {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 10px;
  border-right: 1px solid var(--line);
  font-size: var(--fs-xs);
  color: var(--text-muted);
  cursor: pointer;
  min-width: 120px;
  max-width: 200px;
  user-select: none;
  position: relative;
}
.tab:hover { background: var(--bg-elev); }
.tab.active {
  background: var(--bg-elev);
  color: var(--text);
  box-shadow: inset 0 -2px 0 0 var(--accent);
}
.tab.sending::before {
  content: '';
  position: absolute;
  left: 6px;
  top: 50%;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--accent);
  transform: translateY(-50%);
  animation: pulse 1s ease-in-out infinite;
}
@keyframes pulse { 50% { opacity: 0.3; } }
.method {
  font-weight: 700;
  font-size: 10.5px;
  padding: 1px 4px;
  border-radius: var(--radius-sm);
  background: var(--accent-soft);
  color: var(--accent);
  line-height: 1.3;
}
.m-get     { background: var(--accent-soft);  color: var(--accent); }
.m-post    { background: var(--warning-soft); color: var(--warning); }
.m-put     { background: var(--info-soft);    color: var(--info); }
.m-patch   { background: var(--info-soft);    color: var(--info); }
.m-delete  { background: var(--error-soft);   color: var(--error); }
.m-head    { background: var(--accent-soft);  color: var(--text-muted); }
.m-options { background: var(--accent-soft);  color: var(--text-muted); }
.m-sse     { background: var(--purple-soft);  color: var(--purple); }
.m-ws      { background: var(--cyan-soft);    color: var(--cyan); }
.title {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.close {
  background: none;
  border: none;
  color: var(--text-muted);
  cursor: pointer;
  font-size: var(--fs-xxs);
  padding: 2px 4px;
  border-radius: 4px;
}
.close:hover { background: var(--bg-base); color: var(--error); }
.new {
  background: none;
  border: none;
  color: var(--text-muted);
  cursor: pointer;
  font-size: var(--fs-md);
  padding: 0 12px;
}
.new:hover { color: var(--text); }
.mono { font-family: var(--font-mono, ui-monospace, monospace); }
</style>
