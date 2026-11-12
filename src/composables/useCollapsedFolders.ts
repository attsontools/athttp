import { ref } from 'vue';
import type { KVStorage } from './storage';

const STORAGE_KEY = 'attool.http.sidebar.expandedFolders';

// 默认折叠:只持久化被「展开」的 folder id,不在集合里的一律视为折叠。
// 对外暴露 isCollapsed 语义,内部用展开集实现,方便调用方无需关心存储细节。
export function useCollapsedFolders(storage: KVStorage = localStorage) {
  const expanded = ref<Set<string>>(load(storage));

  function isCollapsed(id: string): boolean {
    return !expanded.value.has(id);
  }

  function toggle(id: string) {
    const next = new Set(expanded.value);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    expanded.value = next;
    persist(storage, next);
  }

  return { expanded, isCollapsed, toggle };
}

function load(storage: KVStorage): Set<string> {
  const raw = storage.getItem(STORAGE_KEY);
  if (!raw) return new Set();
  try {
    const arr = JSON.parse(raw);
    return Array.isArray(arr) ? new Set(arr.filter((x): x is string => typeof x === 'string')) : new Set();
  } catch {
    return new Set();
  }
}

function persist(storage: KVStorage, set: Set<string>) {
  storage.setItem(STORAGE_KEY, JSON.stringify([...set]));
}
