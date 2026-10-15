import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

const target = process.env.SONICBOOM_BACKEND_URL || 'http://127.0.0.1:17842';
export default defineConfig({
  plugins: [vue()],
  define: { __SONICBOOM_BACKEND_PORT__: JSON.stringify(new URL(target).port || "80") },
  build: { assetsDir: 'assets', emptyOutDir: true },
  server: {
    port: 5173, strictPort: true,
    proxy: {
      '^/(?:$|admin(?:/|$)|setup(?:/|$)|api/|v1/|health$|ready$)': {
        target, changeOrigin: true,
        configure(proxy) {
          proxy.on('proxyReq', (request, incoming) => {
            // Preserve the server's same-origin mutation checks through the local proxy.
            if (incoming.headers.origin === 'http://localhost:5173') request.setHeader('origin', new URL(target).origin);
          });
        },
      },
    },
  },
});
