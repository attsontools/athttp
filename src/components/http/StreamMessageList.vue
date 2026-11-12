<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { StreamMessage } from './types';
import { messageTone } from './streamMessageTone';

const props = defineProps<{ messages: StreamMessage[] }>();
const autoScroll = ref(true);
const scroller = ref<HTMLDivElement | null>(null);

watch(
  () => props.messages.length,
  async () => {
    if (autoScroll.value && scroller.value) {
      await new Promise((r) => requestAnimationFrame(r));
      scroller.value.scrollTop = scroller.value.scrollHeight;
    }
  }
);

function fmtTime(ms: number) {
  const d = new Date(ms);
  return `${d.getHours().toString().padStart(2, '0')}:${d.getMinutes().toString().padStart(2, '0')}:${d.getSeconds().toString().padStart(2, '0')}.${d.getMilliseconds().toString().padStart(3, '0')}`;
}

function preview(m: StreamMessage): string {
  switch (m.kind) {
    case 'open': return `↑ ${m.status ?? ''} ${m.headers.length} headers`;
    case 'closed': return `⌀ ${m.code ?? ''} ${m.reason}`;
    case 'error': return `✕ ${m.message}`;
    case 'bufferTruncated': return `⚠ 已丢弃 ${m.dropped} 条旧消息`;
    case 'wsText': return `${m.direction === 'in' ? '↓' : '↑'} ${m.text}${m.truncated ? ' …(truncated)' : ''}`;
    case 'wsBinary': return `${m.direction === 'in' ? '↓' : '↑'} <binary ${m.bytesLen}B>`;
    case 'sseEvent': {
      const head = m.event !== 'message' ? `[${m.event}] ` : '';
      return `${head}${m.data}${m.truncated ? ' …(truncated)' : ''}`;
    }
    default: return '';
  }
}

const total = computed(() => props.messages.length);
</script>

<template>
  <div class="wrap">
    <div class="head">
      <span class="mono">{{ total }} 条</span>
      <label class="opt">
        <input type="checkbox" v-model="autoScroll" /> 自动滚动
      </label>
    </div>
    <div class="scroller" ref="scroller">
      <div v-if="messages.length === 0" class="empty">还没有消息</div>
      <div
        v-for="(m, i) in messages"
        :key="i"
        :class="['row', `tone-${messageTone(m)}`]"
      >
        <span class="time mono">{{ fmtTime(m.atMs) }}</span>
        <pre class="body mono">{{ preview(m) }}</pre>
      </div>
    </div>
  </div>
</template>

<style scoped>
.wrap { display: flex; flex-direction: column; height: 100%; }
.head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 4px 12px;
  border-bottom: 1px solid var(--line);
  font-size: var(--fs-xxs);
  color: var(--text-muted);
}
.opt { display: flex; gap: 4px; align-items: center; cursor: pointer; }
.scroller { flex: 1; overflow-y: auto; padding: 6px 8px; }
.empty {
  padding: 40px 12px;
  text-align: center;
  color: var(--text-muted);
  font-size: var(--fs-xs);
}
.row {
  display: grid;
  grid-template-columns: 90px 1fr;
  gap: 10px;
  padding: 4px 8px;
  border-bottom: 1px solid var(--line-weak, var(--line));
  align-items: baseline;
}
.time { color: var(--text-muted); font-size: var(--fs-xxs); }
.body {
  margin: 0;
  white-space: pre-wrap;
  word-break: break-word;
  font-size: var(--fs-xs);
}
.tone-ok { background: rgba(16,185,129,0.06); }
.tone-warn { background: rgba(245,158,11,0.08); }
.tone-err { background: rgba(239,68,68,0.08); }
.tone-muted { color: var(--text-muted); }
.tone-info { background: rgba(59,130,246,0.06); }
.mono { font-family: var(--font-mono, ui-monospace, monospace); }
</style>
