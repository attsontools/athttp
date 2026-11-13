<script setup lang="ts">
import { NModal } from 'naive-ui';
import CodeEditor from '../ui/CodeEditor.vue';

defineProps<{
  show: boolean;
  name: string;
  body: string;
}>();

const emit = defineEmits<{
  (e: 'update:show', value: boolean): void;
}>();
</script>

<template>
  <n-modal
    :show="show"
    preset="card"
    :title="`数据模型 · ${name}`"
    style="width: 640px; max-width: 90vw"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <CodeEditor :model-value="body" language="json" :readonly="true" height="60vh" />
  </n-modal>
</template>
