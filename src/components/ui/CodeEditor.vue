<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue';
import { loadMonaco } from '../../composables/useMonacoLoader';
import type { editor as MonacoEditor } from 'monaco-editor';
import { watchMonacoTheme, type MonacoThemeName } from '../../theme/monacoTheme';

const LARGE_INPUT = 1_000_000;
const LONG_LINE = 50_000;

function maxLineLength(text: string): number {
  let max = 0;
  let start = 0;
  for (let i = 0; i < text.length; i++) {
    if (text.charCodeAt(i) === 10) { // '\n'
      if (i - start > max) max = i - start;
      start = i + 1;
    }
  }
  if (text.length - start > max) max = text.length - start;
  return max;
}

function resolveLargeOptions(text: string) {
  const isLarge = text.length > LARGE_INPUT;
  const noWrap = maxLineLength(text) > LONG_LINE;
  return {
    wordWrap: noWrap ? 'off' as const : 'on' as const,
    folding: !isLarge,
    renderValidationDecorations: isLarge ? 'off' as const : 'editable' as const,
    // 下面这些默认全开，对超长单行都是每帧扫描；大文件全部按需降级。
    bracketPairColorization: { enabled: !isLarge },
    guides: { bracketPairs: !isLarge, indentation: !isLarge, highlightActiveIndentation: !isLarge },
    matchBrackets: (isLarge ? 'never' : 'always') as 'never' | 'always',
    occurrencesHighlight: (isLarge ? 'off' : 'singleFile') as 'off' | 'singleFile',
    selectionHighlight: !isLarge,
    codeLens: !isLarge,
    links: !isLarge,
    renderWhitespace: (isLarge ? 'none' : 'selection') as 'none' | 'selection',
    stopRenderingLineAfter: isLarge ? 5000 : 10000,
    // Monaco 内建的大文件优化默认在 20MB / 30w 行才触发，手动放宽阈值触发不到就自己关。
    largeFileOptimizations: true,
  };
}

const props = defineProps<{
  modelValue: string;
  language?: string;
  readonly?: boolean;
  height?: number | string;
}>();

const emit = defineEmits<{
  'update:modelValue': [value: string];
}>();

const container = ref<HTMLDivElement | null>(null);
const instance = shallowRef<MonacoEditor.IStandaloneCodeEditor | null>(null);
let suppress = false;
// 记录上次跑 resolveLargeOptions 时的长度，用来限流 maxLineLength 的 O(n) 扫描。
let lastResolvedLen = 0;
let stopWatchingTheme: (() => void) | null = null;
let unmounted = false;
const RESOLVE_LEN_DELTA = 16_384;

function maybeResolve(editor: MonacoEditor.IStandaloneCodeEditor, value: string) {
  const crossedLargeBoundary = (value.length > LARGE_INPUT) !== (lastResolvedLen > LARGE_INPUT);
  if (crossedLargeBoundary || Math.abs(value.length - lastResolvedLen) >= RESOLVE_LEN_DELTA) {
    editor.updateOptions(resolveLargeOptions(value));
    lastResolvedLen = value.length;
  }
}

onMounted(async () => {
  if (!container.value) return;
  const monaco = await loadMonaco();
  if (unmounted || !container.value) return;
  let initialTheme: MonacoThemeName = 'vs-dark';
  stopWatchingTheme = watchMonacoTheme((theme) => {
    initialTheme = theme;
    monaco.editor.setTheme(theme);
  });
  const initialOpts = resolveLargeOptions(props.modelValue);
  lastResolvedLen = props.modelValue.length;
  const editor = monaco.editor.create(container.value, {
    value: props.modelValue,
    language: props.language ?? 'json',
    readOnly: !!props.readonly,
    automaticLayout: true,
    minimap: { enabled: false },
    fontSize: 13,
    scrollBeyondLastLine: false,
    tabSize: 2,
    largeFileOptimizations: true,
    wordWrap: initialOpts.wordWrap,
    folding: initialOpts.folding,
    renderValidationDecorations: initialOpts.renderValidationDecorations,
    theme: initialTheme,
  });
  editor.onDidChangeModelContent(() => {
    if (suppress) return;
    const value = editor.getValue();
    // Monaco 自己接住的粘贴/输入也要重算选项；否则 wordWrap 一直停在初始状态。
    maybeResolve(editor, value);
    emit('update:modelValue', value);
  });
  instance.value = editor;
});

watch(
  () => props.modelValue,
  (next) => {
    const editor = instance.value;
    if (!editor) return;
    if (editor.getValue() === next) return;
    editor.updateOptions(resolveLargeOptions(next));
    lastResolvedLen = next.length;
    suppress = true;
    editor.setValue(next);
    suppress = false;
  },
);

watch(
  () => props.language,
  async (lang) => {
    const editor = instance.value;
    if (!editor) return;
    const monaco = await loadMonaco();
    const model = editor.getModel();
    if (model) monaco.editor.setModelLanguage(model, lang ?? 'json');
  },
);

watch(
  () => props.readonly,
  (ro) => {
    instance.value?.updateOptions({ readOnly: !!ro });
  },
);

onBeforeUnmount(() => {
  unmounted = true;
  stopWatchingTheme?.();
  stopWatchingTheme = null;
  instance.value?.dispose();
  instance.value = null;
});
</script>

<template>
  <div
    ref="container"
    class="code-editor"
    :style="{ height: typeof height === 'number' ? `${height}px` : (height ?? '320px') }"
  />
</template>

<style scoped>
.code-editor {
  width: 100%;
  border: 1px solid var(--line);
  border-radius: var(--radius);
  overflow: hidden;
}
</style>
