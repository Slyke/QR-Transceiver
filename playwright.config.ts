import { defineConfig } from './desktop/node_modules/@playwright/test/index.mjs';
export default defineConfig({ testDir: './desktop/tests-ui', timeout: 30000, use: { baseURL: 'http://127.0.0.1:1420', viewport: { width: 1440, height: 1100 } }, reporter: 'list' });
