<script setup lang="ts">
import { ref, useAttrs, watch } from 'vue';
import { NAutoComplete, NInput } from 'naive-ui';

defineOptions({ inheritAttrs: false });

const props = withDefaults(defineProps<{
  modelValue: string;
  kind?: 'native' | 'naive' | 'autocomplete';
}>(), {
  kind: 'native'
});

const emit = defineEmits<{
  'update:modelValue': [value: string];
}>();

const attrs = useAttrs();
const draft = ref(props.modelValue);

watch(
  () => props.modelValue,
  (next) => {
    if (next !== draft.value) draft.value = next;
  }
);

function update(value: string) {
  draft.value = value;
  emit('update:modelValue', value);
}

function onNativeInput(event: Event) {
  update((event.target as HTMLInputElement).value);
}
</script>

<template>
  <n-input
    v-if="kind === 'naive'"
    v-bind="attrs"
    :value="draft"
    @update:value="update"
  />
  <n-auto-complete
    v-else-if="kind === 'autocomplete'"
    v-bind="attrs"
    :value="draft"
    @update:value="update"
  />
  <input
    v-else
    v-bind="attrs"
    :value="draft"
    @input="onNativeInput"
  />
</template>
