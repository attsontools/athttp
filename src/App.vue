<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { darkTheme, NConfigProvider, NMessageProvider } from 'naive-ui';

import HttpTool from './components/http/HttpTool.vue';
import { configureHttpStore } from './composables/useHttpStore';
import type { HttpHost, ThemeName } from './frontend/pluginHost';
import { darkOverrides, lightOverrides } from './theme';

const props = defineProps<{ host: HttpHost }>();
configureHttpStore(props.host.transport);

const theme = ref<ThemeName>('dark');
const connecting = ref(true);
const connectionError = ref('');
const naiveTheme = computed(() => theme.value === 'dark' ? darkTheme : null);
const naiveOverrides = computed(() => theme.value === 'dark' ? darkOverrides : lightOverrides);

onMounted(async () => {
  try {
    theme.value = await props.host.initialize(applyTheme);
    applyTheme(theme.value);
  } catch (error) {
    connectionError.value = error instanceof Error ? error.message : String(error);
  } finally {
    connecting.value = false;
  }
});

onBeforeUnmount(() => props.host.dispose());

function applyTheme(value: ThemeName) {
  theme.value = value;
  document.documentElement.dataset.theme = value;
}
</script>

<template>
  <n-config-provider :theme="naiveTheme" :theme-overrides="naiveOverrides">
    <n-message-provider>
      <main class="app-shell">
        <div v-if="connecting" class="center-state">正在连接 HTTP 服务</div>
        <div v-else-if="connectionError" class="center-state error-state">
          <strong>无法启动 HTTP 请求</strong>
          <span>{{ connectionError }}</span>
        </div>
        <HttpTool v-else />
      </main>
    </n-message-provider>
  </n-config-provider>
</template>
