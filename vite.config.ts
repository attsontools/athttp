/// <reference types="vitest" />
import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

export default defineConfig({
  base: './',
  plugins: [vue()],
  clearScreen: false,
  server: {
    strictPort: true,
    port: 1426,
    host: '127.0.0.1'
  },
  optimizeDeps: {
    exclude: ['monaco-editor']
  },
  build: {
    target: 'es2022'
  },
  test: {
    exclude: ['**/node_modules/**', '**/dist/**']
  }
});
