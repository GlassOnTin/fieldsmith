import { defineConfig } from 'vite';
import preact from '@preact/preset-vite';

/** Cross-origin isolation: shared wasm memory (the threaded build) needs both headers. A
 * host that omits them still works — the app falls back to the single-thread build. */
const isolation = {
  'Cross-Origin-Opener-Policy': 'same-origin',
  'Cross-Origin-Embedder-Policy': 'require-corp',
};

export default defineConfig({
  base: './',
  plugins: [preact()],
  build: {
    target: 'es2022',
    chunkSizeWarningLimit: 2000,
  },
  worker: {
    format: 'es',
  },
  server: {
    port: 5173,
    headers: isolation,
    // Example documents live in ../examples and are bundled with `?raw`.
    fs: { allow: ['..'] },
  },
  preview: {
    headers: isolation,
  },
});
