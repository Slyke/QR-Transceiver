import { test, expect } from '@playwright/test';
import { spawn, execFileSync } from 'node:child_process';
import { mkdtempSync, existsSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
test('out-of-order receiving renders holes, updates digest and verifies completion', async ({ page, request }) => {
  const state = mkdtempSync(path.join(tmpdir(), 'qrtx-ui-peer-'));
  const binary = path.resolve('target/debug/qrtransceiver');
  const source = path.join(state, 'incoming.bin');
  writeFileSync(source, Buffer.from(Array.from({ length: 1800 }, (_, i) => i % 256)));
  const service = spawn(binary, ['--state', path.join(state, 'engine'), 'serve', '--allow-root', state], { stdio: 'ignore' });
  const cli = (...args: string[]) =>
    JSON.parse(execFileSync(binary, ['--state', path.join(state, 'engine'), ...args], { encoding: 'utf8' }));
  const rpc = async (method: string, params: unknown) => {
    const response = await request.post('/dev-rpc', { data: { method, params } });
    const body = await response.json();
    expect(body.ok, JSON.stringify(body.error)).toBe(true);
    return body.result;
  };
  try {
    await expect.poll(() => existsSync(path.join(state, 'engine', 'engine.sock'))).toBe(true);
    const job = cli('send', source);
    const data: { text: string; frame: { index: number } }[] = [];
    for (let i = 0; i < 12 && data.length < 3; i++) {
      const frame = cli('next');
      if (!frame) continue;
      if (frame.frame.kind === 'Manifest') await rpc('ingest', { text: frame.text });
      if (frame.frame.kind === 'Data') data.push(frame);
    }
    expect(data).toHaveLength(3);
    await rpc('ingest', { text: data[1].text });
    await page.goto('/');
    await page
      .getByRole('button', { name: /incoming.bin/ })
      .last()
      .click();
    await expect(page.locator('.hex-values button.unavailable').first()).toBeVisible();
    await expect(page.locator('.digest')).toHaveText(/^[0-9a-f]{64}$/);
    const firstDigest = await page.locator('.digest').textContent();
    await page.getByLabel('Follow active bytes').uncheck();
    await page.getByLabel('Jump to byte offset').fill('0');
    await page.getByRole('button', { name: 'Go', exact: true }).click();
    await expect(page.locator('.hex-values button').first()).toHaveText('--');
    await rpc('ingest', { text: data[2].text });
    await expect(page.locator('.digest')).not.toHaveText(firstDigest!);
    await rpc('ingest', { text: data[0].text });
    await expect(page.getByText('VERIFIED & COMMITTED', { exact: true })).toBeVisible();
    await expect(page.locator('.hex-values button').first()).toHaveText('00');
    await expect(page.locator('.hex-values button.unavailable')).toHaveCount(0);
    await page.screenshot({ path: 'test-results/receive-verified.png', fullPage: true });
    await rpc('action', { id: job.id, action: 'remove' });
  } finally {
    service.kill();
    await new Promise<void>((resolve) => {
      if (service.exitCode !== null) resolve();
      else service.once('exit', () => resolve());
    });
    rmSync(state, { recursive: true, force: true });
  }
});
