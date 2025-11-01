import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

const rootDir = fileURLToPath(new URL('.', import.meta.url));

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@dreamquill/ts-sdk': resolve(rootDir, '../ts-sdk/src')
    }
  },
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      '/api': 'http://127.0.0.1:5174'
    }
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true
  }
});
