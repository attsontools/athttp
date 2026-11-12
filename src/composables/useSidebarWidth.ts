import { ref } from 'vue';
import type { KVStorage } from './storage';

const STORAGE_KEY = 'attool.http.sidebar.width';
export const DEFAULT_WIDTH = 240;
export const MIN_WIDTH = 180;
export const MAX_WIDTH = 560;

function clamp(n: number): number {
  if (!Number.isFinite(n)) return DEFAULT_WIDTH;
  return Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, Math.round(n)));
}

export function useSidebarWidth(storage: KVStorage = localStorage) {
  const raw = Number(storage.getItem(STORAGE_KEY));
  const width = ref(storage.getItem(STORAGE_KEY) === null ? DEFAULT_WIDTH : clamp(raw));

  function setWidth(next: number) {
    width.value = clamp(next);
    storage.setItem(STORAGE_KEY, String(width.value));
  }

  function reset() {
    setWidth(DEFAULT_WIDTH);
  }

  return { width, setWidth, reset };
}
