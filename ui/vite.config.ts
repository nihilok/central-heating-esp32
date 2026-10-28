import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [tailwindcss(), svelte()],
  server: {
    proxy: {
      '/api': 'http://192.168.1.168',
      '/token': 'http://192.168.1.168',
      '/check_token': 'http://192.168.1.168',
    },
  },
  build: {
    target: 'es2022',
    cssCodeSplit: false,
    rollupOptions: {
      output: {
        entryFileNames: 'assets/index.js',
        chunkFileNames: 'assets/[name].js',
        assetFileNames: (assetInfo) => {
          if (assetInfo.names?.some((n) => n.endsWith('.css'))) {
            return 'assets/index.css';
          }
          return 'assets/[name][extname]';
        },
      },
    },
  },
});
