import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { readFile } from 'node:fs/promises';
export default defineConfig({
  plugins: [
    svelte(),
    {
      name: 'qrtx-local-development',
      configureServer: (server) => {
        server.middlewares.use('/dev-rpc', async (request, response) => {
          // Development-only bridge. Credentials never enter the browser bundle.
          try {
            if (request.method !== 'POST') {
              response.statusCode = 405;
              response.end();
              return;
            }
            const origin = request.headers.origin;
            if (origin && !['http://127.0.0.1:1420', 'http://localhost:1420'].includes(origin)) {
              response.statusCode = 403;
              response.end();
              return;
            }
            if (!request.headers['content-type']?.startsWith('application/json')) {
              response.statusCode = 415;
              response.end();
              return;
            }
            const config = JSON.parse(await readFile(process.env.QRTX_DEV_CONFIG ?? '../.qrtx/config.json', 'utf8'));
            const chunks: Buffer[] = [];
            let length = 0;
            for await (const chunk of request) {
              length += chunk.length;
              if (length > 1048576) throw new Error('Request too large');
              chunks.push(chunk);
            }
            const upstream = await fetch('http://127.0.0.1:38475/v1/rpc', {
              method: 'POST',
              headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${config.tokens[0].token}` },
              body: Buffer.concat(chunks).toString()
            });
            response.setHeader('Content-Type', 'application/json');
            response.end(await upstream.text());
          } catch (error) {
            response.statusCode = 503;
            response.end(
              JSON.stringify({ ok: false, error: { message: error instanceof Error ? error.message : 'Local engine unavailable' } })
            );
          }
        });
      }
    }
  ],
  server: { port: 1420, strictPort: true },
  clearScreen: false
});
