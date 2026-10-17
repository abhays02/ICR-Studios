import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'
import { fileURLToPath } from 'node:url'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  build: {
    outDir: 'dist-demo',
    rollupOptions: {
      input: fileURLToPath(new URL('./demo/index.html', import.meta.url)),
    },
  },
  server: {
    host: '127.0.0.1',
    port: 5174,
    proxy: {
      '/demo-api': 'http://127.0.0.1:8010',
    },
  },
})
