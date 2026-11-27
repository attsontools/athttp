<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { darkTheme, NConfigProvider, NMessageProvider } from 'naive-ui';

import HttpTool from './components/http/HttpTool.vue';
import { configureHttpStore } from './composables/useHttpStore';
import type { HttpHost, ThemeName } from './frontend/pluginHost';
import { formatServiceError } from './frontend/transport';
import { darkOverrides, lightOverrides } from './theme';

const props = defineProps<{ host: HttpHost }>();
configureHttpStore(props.host.transport);

const THEME_STORAGE_KEY = 'athttp.theme';
const theme = ref<ThemeName>('dark');
const connecting = ref(true);
const connectionError = ref('');
const standalone = props.host.mode === 'standalone';
const naiveTheme = computed(() => theme.value === 'dark' ? darkTheme : null);
const naiveOverrides = computed(() => theme.value === 'dark' ? darkOverrides : lightOverrides);

onMounted(async () => {
  try {
    theme.value = await props.host.initialize(applyTheme);
    applyTheme(theme.value);
  } catch (error) {
    connectionError.value = formatServiceError(error);
  } finally {
    connecting.value = false;
  }
});

onBeforeUnmount(() => props.host.dispose());

function applyTheme(value: ThemeName) {
  theme.value = value;
  document.documentElement.dataset.theme = value;
}

function toggleTheme() {
  if (!standalone) return;
  const nextTheme: ThemeName = theme.value === 'dark' ? 'light' : 'dark';
  applyTheme(nextTheme);
  try {
    localStorage.setItem(THEME_STORAGE_KEY, nextTheme);
  } catch {
    // The active theme remains usable when browser storage is unavailable.
  }
}
</script>

<template>
  <n-config-provider :theme="naiveTheme" :theme-overrides="naiveOverrides">
    <n-message-provider>
      <main class="app-shell" :class="{ 'is-standalone': standalone }">
        <header v-if="standalone" class="brandbar">
          <div class="brand">
            <span class="brand-mark" aria-hidden="true">H</span>
            <span class="brand-copy">
              <strong>AT HTTP</strong>
              <span>HTTP 请求</span>
            </span>
          </div>
          <div class="brand-actions">
            <span class="standalone-badge">独立应用</span>
            <button
              class="theme-toggle"
              type="button"
              :aria-label="theme === 'dark' ? '切换到浅色主题' : '切换到深色主题'"
              :title="theme === 'dark' ? '切换到浅色主题' : '切换到深色主题'"
              @click="toggleTheme"
            >
              <svg v-if="theme === 'dark'" viewBox="0 0 24 24" aria-hidden="true">
                <circle cx="12" cy="12" r="4" />
                <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
              </svg>
              <svg v-else viewBox="0 0 24 24" aria-hidden="true">
                <path d="M20.4 15.4A8.5 8.5 0 0 1 8.6 3.6 8.5 8.5 0 1 0 20.4 15.4Z" />
              </svg>
            </button>
          </div>
        </header>
        <section class="app-content">
          <div v-if="connecting" class="center-state">正在连接 HTTP 服务</div>
          <div v-else-if="connectionError" class="center-state error-state">
            <strong>无法启动 HTTP 请求</strong>
            <span>{{ connectionError }}</span>
          </div>
          <HttpTool v-else :host="host" />
        </section>
      </main>
    </n-message-provider>
  </n-config-provider>
</template>
