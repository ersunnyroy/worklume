import tailwindcss from '@tailwindcss/vite';
import { tanstackRouter } from '@tanstack/router-plugin/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';
import securityHeaders from './security-headers.json' with { type: 'json' };

/**
 * The response headers CloudFront sends with the app (E00-S03 reads the same file). The preview
 * server sends them too, so the Playwright smoke tests run under the production CSP.
 */
function headersFor(apiOrigin: string): Record<string, string> {
  return Object.fromEntries(
    Object.entries(securityHeaders).map(([name, value]) => [
      name,
      value.replace('{{API_ORIGIN}}', apiOrigin),
    ]),
  );
}

export default defineConfig({
  plugins: [
    // Generates src/routeTree.gen.ts from src/routes and splits each route into its own chunk.
    // It must run before the React plugin.
    tanstackRouter({ target: 'react', autoCodeSplitting: true, quoteStyle: 'single' }),
    react(),
    tailwindcss(),
  ],
  build: {
    target: 'es2023',
  },
  server: {
    port: 5173,
    strictPort: true,
  },
  preview: {
    port: 4173,
    strictPort: true,
    headers: headersFor('http://localhost:8000'),
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.{ts,tsx}'],
    setupFiles: ['./src/test/setup.ts'],
    restoreMocks: true,
    unstubGlobals: true,
    unstubEnvs: true,
  },
});
