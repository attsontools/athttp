<script setup lang="ts">
import { computed, ref } from 'vue';
import { NAutoComplete, NButton, NInput, NModal, NTabs, NTabPane } from 'naive-ui';
import type { HttpEnv, HttpEnvVar } from './types';
import { COMMON_ENV_VAR_OPTIONS } from './variables';

const props = defineProps<{
  show: boolean;
  envs: HttpEnv[];
  activeEnvId: string | null;
  activeEnvVars: HttpEnvVar[];
  globalVars: HttpEnvVar[];
  defaultTab: 'env' | 'vars';
}>();

const emit = defineEmits<{
  (e: 'update:show', v: boolean): void;
  (e: 'add-env', name: string): void;
  (e: 'rename-env', id: string, name: string): void;
  (e: 'delete-env', id: string): void;
  (e: 'set-active-env', id: string | null): void;
  (e: 'add-var', envId: string): void;
  (e: 'update-var', v: HttpEnvVar): void;
  (e: 'delete-var', id: string, envId: string): void;
}>();

const tab = ref<'env' | 'vars'>(props.defaultTab);
const scope = ref<'active' | 'global'>('active');
const newEnvName = ref('');

const currentVars = computed(() =>
  scope.value === 'global' ? props.globalVars : props.activeEnvVars
);
const scopeEnvId = computed(() =>
  scope.value === 'global' ? '' : props.activeEnvId ?? ''
);

const effective = computed(() => {
  const map = new Map<string, { key: string; value: string; from: string }>();
  for (const v of props.globalVars) if (v.enabled && v.key) {
    map.set(v.key, { key: v.key, value: v.value, from: '全局' });
  }
  for (const v of props.activeEnvVars) if (v.enabled && v.key) {
    const env = props.envs.find((e) => e.id === props.activeEnvId);
    map.set(v.key, { key: v.key, value: v.value, from: env?.name ?? '当前' });
  }
  return Array.from(map.values());
});

function updateShow(v: boolean) {
  emit('update:show', v);
}

function submitNewEnv() {
  const name = newEnvName.value.trim();
  if (!name) return;
  emit('add-env', name);
  newEnvName.value = '';
}

function renameEnv(env: HttpEnv, name: string) {
  emit('rename-env', env.id, name);
}

function deleteEnv(env: HttpEnv) {
  if (confirm(`删除环境 “${env.name}” 及其下所有变量？`)) emit('delete-env', env.id);
}

function updateVarKey(v: HttpEnvVar, key: string) {
  emit('update-var', { ...v, key });
}
function updateVarValue(v: HttpEnvVar, value: string) {
  emit('update-var', { ...v, value });
}
function updateVarEnabled(v: HttpEnvVar, enabled: boolean) {
  emit('update-var', { ...v, enabled });
}
function looksLikeToken(v: string): boolean {
  return v.length > 20 && v.includes('.');
}
</script>

<template>
  <n-modal
    :show="show"
    preset="card"
    style="width: 720px"
    title="环境与变量"
    @update:show="updateShow"
  >
    <n-tabs v-model:value="tab" type="line" size="small">
      <n-tab-pane name="env" tab="环境">
        <div class="envs">
          <div v-if="envs.length === 0" class="empty">
            暂无环境。变量将只使用「全局」作用域。
          </div>
          <div v-for="env in envs" :key="env.id" class="env-row">
            <input
              type="radio"
              :checked="env.id === activeEnvId"
              @change="emit('set-active-env', env.id)"
            />
            <input
              class="kv-input mono"
              :value="env.name"
              @change="(e: any) => renameEnv(env, e.target.value)"
            />
            <n-button size="tiny" quaternary @click="deleteEnv(env)">删除</n-button>
          </div>
          <div class="env-row env-add">
            <span></span>
            <n-input
              v-model:value="newEnvName"
              size="small"
              placeholder="新环境名，例如 prod / staging"
              @keyup.enter="submitNewEnv"
            />
            <n-button size="tiny" secondary @click="submitNewEnv">添加</n-button>
          </div>
          <div v-if="envs.length" class="env-clear">
            <n-button size="tiny" quaternary @click="emit('set-active-env', null)">不使用任何环境</n-button>
          </div>
        </div>
      </n-tab-pane>

      <n-tab-pane name="vars" tab="变量">
        <div class="scope-switch">
          <button :class="{ on: scope === 'active' }" @click="scope = 'active'">
            当前环境
            <span class="scope-hint">{{ envs.find(e => e.id === activeEnvId)?.name ?? '（无）' }}</span>
          </button>
          <button :class="{ on: scope === 'global' }" @click="scope = 'global'">全局</button>
        </div>

        <div v-if="scope === 'active' && !activeEnvId" class="empty">
          未选择当前环境。切换到「全局」，或在「环境」tab 里创建/激活一个环境。
        </div>

        <div v-else class="vars">
          <div v-for="v in currentVars" :key="v.id" class="var-row">
            <input type="checkbox" :checked="v.enabled" @change="(e: any) => updateVarEnabled(v, e.target.checked)" />
            <n-auto-complete
              :value="v.key"
              :options="COMMON_ENV_VAR_OPTIONS"
              size="small"
              placeholder="变量名"
              @update:value="(value: string) => updateVarKey(v, value)"
            />
            <input
              class="kv-input mono"
              :type="looksLikeToken(v.value) ? 'password' : 'text'"
              :value="v.value"
              placeholder="值"
              @input="(e: any) => updateVarValue(v, e.target.value)"
            />
            <button class="kv-del" @click="emit('delete-var', v.id, v.envId)">✕</button>
          </div>
          <n-button size="tiny" secondary @click="emit('add-var', scopeEnvId)">+ 添加变量</n-button>
        </div>

        <div class="effective">
          <h4>有效变量（预览）</h4>
          <div v-if="effective.length === 0" class="muted">无</div>
          <div v-for="e in effective" :key="e.key" class="eff-row mono">
            <span class="k">{{ e.key }}</span>
            <span class="eq">=</span>
            <span class="v">{{ e.value }}</span>
            <span class="from">← {{ e.from }}</span>
          </div>
        </div>
      </n-tab-pane>
    </n-tabs>
  </n-modal>
</template>

<style scoped>
.envs { display: grid; gap: 6px; padding: 4px 0; }
.env-row { display: grid; grid-template-columns: 20px 1fr auto; gap: 8px; align-items: center; }
.env-add { border-top: 1px dashed var(--line); padding-top: 8px; margin-top: 4px; }
.env-clear { padding-top: 6px; }
.empty { padding: 12px; color: var(--text-muted); font-size: var(--fs-xs); text-align: center; }

.scope-switch { display: flex; gap: 4px; padding: 4px 0; }
.scope-switch button {
  background: none;
  border: 1px solid var(--line);
  color: var(--text-muted);
  padding: 4px 10px;
  border-radius: var(--radius-sm);
  cursor: pointer;
  font-size: var(--fs-xs);
}
.scope-switch button.on { background: var(--bg-elev); color: var(--text); border-color: var(--accent, #10b981); }
.scope-hint { color: var(--text-muted); margin-left: 4px; font-size: var(--fs-xxs); }

.vars { display: grid; gap: 4px; padding: 4px 0; }
.var-row { display: grid; grid-template-columns: 24px 1fr 2fr 32px; gap: 6px; align-items: center; }

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
.kv-input:focus { border-color: var(--accent, #10b981); }
.kv-del { background: none; border: none; color: var(--text-muted); cursor: pointer; }
.kv-del:hover { color: #ef4444; }

.effective {
  margin-top: 12px;
  padding-top: 8px;
  border-top: 1px solid var(--line);
}
.effective h4 { margin: 0 0 6px; font-size: var(--fs-xs); color: var(--text-muted); font-weight: 500; }
.eff-row { display: flex; gap: 6px; font-size: var(--fs-xxs); padding: 2px 0; }
.eff-row .k { color: var(--accent, #10b981); }
.eff-row .from { color: var(--text-muted); margin-left: auto; }
.mono { font-family: var(--font-mono, ui-monospace, monospace); font-variant-numeric: tabular-nums; }
.muted { color: var(--text-muted); font-size: var(--fs-xs); }
</style>
