<script setup lang="ts">
import { computed } from 'vue';
import type { HttpCollectionFolder, HttpCollectionRequest } from './types';
import { folderHasVisibleContent, folderRequestCount } from './collectionTree';
import { isSchemaSpec } from './schemaItem';

// 递归渲染一层 folder 及其子 folder。folder 树通过 parentId 关联,
// 本组件渲染 parentId === folder.id 的子 folder,从而支持任意深度的 tags 分组。
const props = defineProps<{
  collectionId: string;
  folder: HttpCollectionFolder;
  depth: number;
  allFolders: HttpCollectionFolder[];
  requestsFor: (collectionId: string, folderId: string | null) => HttpCollectionRequest[];
  menuFor: string | null;
  isCollapsed: (id: string) => boolean;
}>();

const emit = defineEmits<{
  (e: 'request-click', item: HttpCollectionRequest, ev: MouseEvent): void;
  (e: 'request-dblclick', item: HttpCollectionRequest): void;
  (e: 'request-context', item: HttpCollectionRequest, ev: MouseEvent): void;
  (e: 'open-request', item: HttpCollectionRequest, mode: 'active' | 'new'): void;
  (e: 'delete-request', id: string): void;
  (e: 'close-menu'): void;
  (e: 'toggle-folder', id: string): void;
}>();

// 每层缩进步长(px),标题按层级递进
const STEP = 14;
// 标题左侧「箭头 + 文件夹图标」占位宽度,请求行对齐到父标题文字起点,体现从属关系
const TITLE_ICON_WIDTH = 32;

const collapsed = computed(() => props.isCollapsed(props.folder.id));
const titleIndent = computed(() => 8 + props.depth * STEP);
// 请求行缩进 = 本层标题缩进 + 图标区宽度,使请求项左缘对齐到分组名起点
const rowIndent = computed(() => titleIndent.value + TITLE_ICON_WIDTH);
const requestCount = computed(() =>
  folderRequestCount(
    props.folder,
    props.collectionId,
    props.allFolders,
    (cid, fid) => props.requestsFor(cid, fid).length
  )
);

function childFolders(): HttpCollectionFolder[] {
  return props.allFolders.filter(
    (f) => f.collectionId === props.collectionId && f.parentId === props.folder.id
  );
}

function directRequests(): HttpCollectionRequest[] {
  return props.requestsFor(props.collectionId, props.folder.id);
}

// folder 自身或任意后代含有可见 request 才渲染,避免搜索过滤时留下空分组标题。
function hasVisibleContent(folder: HttpCollectionFolder): boolean {
  return folderHasVisibleContent(
    folder,
    props.collectionId,
    props.allFolders,
    (cid, fid) => props.requestsFor(cid, fid).length
  );
}
</script>

<template>
  <div v-if="hasVisibleContent(folder)" class="folder">
    <div
      class="folder-title"
      :style="{ paddingLeft: titleIndent + 'px' }"
      @click="emit('toggle-folder', folder.id)"
    >
      <span class="caret" :class="{ open: !collapsed }">
        <svg viewBox="0 0 24 24" width="10" height="10" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="9 6 15 12 9 18" />
        </svg>
      </span>
      <span class="folder-icon">
        <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
          <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
        </svg>
      </span>
      <span class="folder-name">{{ folder.name }}</span>
      <span class="folder-count">({{ requestCount }})</span>
    </div>

    <template v-if="!collapsed">
      <div
        v-for="request in directRequests()"
        :key="request.id"
        class="request-row"
        :style="{ paddingLeft: rowIndent + 'px' }"
        @mousedown.left="emit('request-click', request, $event)"
        @mousedown.middle="emit('request-click', request, $event)"
        @dblclick="emit('request-dblclick', request)"
        @contextmenu="emit('request-context', request, $event)"
      >
        <span v-if="isSchemaSpec(request.spec)" class="model-icon" title="数据模型">
          <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
            <path d="M12 2 3 7v10l9 5 9-5V7z" />
            <path d="M3 7l9 5 9-5" />
            <path d="M12 12v10" />
          </svg>
        </span>
        <span v-else class="method mono">{{ request.method }}</span>
        <span class="req-name">{{ request.name }}</span>
        <div v-if="menuFor === request.id" class="menu" @mouseleave="emit('close-menu')">
          <button @click="emit('open-request', request, 'new'); emit('close-menu')">在新 tab 打开</button>
          <button @click="emit('open-request', request, 'active'); emit('close-menu')">回填当前 tab</button>
          <button @click="emit('delete-request', request.id); emit('close-menu')">删除</button>
        </div>
      </div>

      <HttpCollectionTree
        v-for="child in childFolders()"
        :key="child.id"
        :collection-id="collectionId"
        :folder="child"
        :depth="depth + 1"
        :all-folders="allFolders"
        :requests-for="requestsFor"
        :menu-for="menuFor"
        :is-collapsed="isCollapsed"
        @request-click="(r, ev) => emit('request-click', r, ev)"
        @request-dblclick="(r) => emit('request-dblclick', r)"
        @request-context="(r, ev) => emit('request-context', r, ev)"
        @open-request="(r, mode) => emit('open-request', r, mode)"
        @delete-request="(id) => emit('delete-request', id)"
        @close-menu="emit('close-menu')"
        @toggle-folder="(id) => emit('toggle-folder', id)"
      />
    </template>
  </div>
</template>

<style scoped>
.folder-title {
  display: flex;
  align-items: center;
  gap: 5px;
  padding-top: 5px;
  padding-bottom: 5px;
  padding-right: 10px;
  color: var(--text-muted);
  font-size: var(--fs-xs);
  font-weight: 500;
  cursor: pointer;
  user-select: none;
}
.folder-title:hover { background: var(--bg-elev); color: var(--text); }
.caret {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 12px;
  flex: none;
  color: var(--text-faint);
  transition: transform 0.12s ease;
}
.caret.open { transform: rotate(90deg); }
.folder-icon {
  display: inline-flex;
  align-items: center;
  flex: none;
  color: var(--text-faint);
}
.folder-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.folder-count { flex: none; color: var(--text-faint); font-size: var(--fs-xxs); }
.request-row {
  display: grid;
  grid-template-columns: 42px 1fr;
  gap: 6px;
  align-items: center;
  padding-top: 6px;
  padding-bottom: 6px;
  padding-right: 10px;
  cursor: pointer;
  position: relative;
}
.request-row:hover { background: var(--bg-elev); }
.req-name {
  color: var(--text-muted);
  font-size: var(--fs-xs);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.method { color: var(--text); font-weight: 600; width: 42px; }
.model-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 42px;
  color: var(--accent, #8b5cf6);
}
.mono { font-family: var(--font-mono, ui-monospace, monospace); font-variant-numeric: tabular-nums; }
.menu {
  position: absolute;
  right: 6px;
  top: 8px;
  background: var(--bg-elev);
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 4px;
  display: grid;
  gap: 2px;
  z-index: 4;
  min-width: 140px;
}
.menu button {
  background: none;
  border: none;
  color: var(--text);
  padding: 4px 8px;
  text-align: left;
  font-size: var(--fs-xs);
  cursor: pointer;
  border-radius: 4px;
}
.menu button:hover { background: var(--bg-base); }
</style>
