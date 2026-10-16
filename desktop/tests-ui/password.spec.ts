import { test, expect } from '@playwright/test';
import { spawn, execFileSync } from 'node:child_process';
import { mkdtempSync, existsSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

test('optional encryption and password retries preserve the received bundle', async ({ page, request }) => {
  test.setTimeout(60000);
  const state = mkdtempSync(path.join(tmpdir(), 'qrtx-password-ui-'));
  const binary = path.resolve('target/debug/qrtransceiver');
  const source = path.join(state, 'example.cqs.json.gz');
  writeFileSync(source, Buffer.from(Array.from({ length: 1800 }, (_, i) => i % 256)));
  const service = spawn(binary, ['--state', path.join(state, 'engine'), 'serve', '--allow-root', state], { stdio: 'ignore' });
  const cli = (args: string[], input?: string) =>
    JSON.parse(execFileSync(binary, ['--state', path.join(state, 'engine'), ...args], { encoding: 'utf8', input }));
  const rpc = async (method: string, params: unknown = {}) => {
    const response = await request.post('/dev-rpc', { data: { method, params } });
    const body = await response.json();
    expect(body.ok, JSON.stringify(body.error)).toBe(true);
    return body.result;
  };
  const queued: string[] = [];
  try {
    await expect.poll(() => existsSync(path.join(state, 'engine', 'engine.sock'))).toBe(true);
    await page.goto('/');
    const encryption = page.getByLabel('Encrypt new transfer with a password');
    await expect(encryption).not.toBeChecked();
    await expect(page.getByLabel('Send password', { exact: true })).toHaveCount(0);
    await page.getByLabel('Receive password', { exact: true }).fill('preentered');
    await page.getByRole('button', { name: 'Use password', exact: true }).click();
    await expect(page.getByText('Password is ready for incoming bundles until the app closes or you clear it.')).toBeVisible();
    await page.getByRole('button', { name: 'Clear password', exact: true }).click();
    await encryption.check();
    await page.getByLabel('Send password', { exact: true }).fill(' UI secret 🔒 ');
    await page.getByLabel('File or directory path').fill(path.resolve('tests/fixtures/hello.txt'));
    await page.getByRole('button', { name: 'Queue path', exact: true }).click();
    await expect(page.getByRole('button', { name: /transfer.qrtxenc/ })).toBeVisible();
    const outgoing = (await rpc('transfers')).find((j: { direction: string }) => j.direction === 'send');
    queued.push(outgoing.id);
    expect(outgoing.password_bundle).toBe(true);
    expect(outgoing.manifest.files).toHaveLength(1);
    expect(JSON.stringify(outgoing)).not.toContain('UI secret');
    await expect(page.getByLabel('Send password', { exact: true })).toHaveValue('');
    await rpc('display_settings', { qr_pixels: 800 });
    expect((await rpc('transfer', { id: outgoing.id })).job.manifest_hash).toBe(outgoing.manifest_hash);
    await rpc('action', { id: outgoing.id, action: 'cancel' });
    await rpc('action', { id: outgoing.id, action: 'remove' });
    queued.pop();

    const job = cli(['send', '--password-stdin', source], ' UI secret 🔒 ');
    queued.push(job.id);
    const data: string[] = [];
    for (let i = 0; i < 20 && data.length < job.manifest.files[0].chunks; i++) {
      const frame = cli(['next']);
      if (frame?.frame.kind === 'Manifest') await rpc('ingest', { text: frame.text });
      if (frame?.frame.kind === 'Data') data.push(frame.text);
    }
    await expect(page.getByLabel('Transfer password', { exact: true })).toBeVisible();
    for (const text of data) await rpc('ingest', { text });
    const received = (await rpc('transfer', { id: job.id })).job.bytes;
    await page.getByLabel('Transfer password', { exact: true }).fill('wrong');
    await page.getByRole('button', { name: 'Unlock transfer', exact: true }).click();
    await expect(page.locator('.password-panel .error')).toContainText('incorrect password');
    expect((await rpc('transfer', { id: job.id })).job.bytes).toBe(received);
    await page.getByLabel('Transfer password', { exact: true }).fill(' UI secret 🔒 ');
    await page.getByRole('button', { name: 'Unlock transfer', exact: true }).click();
    await expect(page.getByLabel('Transfer password', { exact: true })).toHaveCount(0);
    await page.getByRole('button', { name: /transfer.qrtxenc/ }).click();
    await expect(page.getByText('Decrypted and verified files')).toBeVisible();
    await expect(page.getByText(/example.cqs.json.gz ·/)).toBeVisible();
    expect(await page.evaluate(() => JSON.stringify(localStorage))).not.toContain('UI secret');
    await page.screenshot({ path: 'test-results/password-unlocked.png', fullPage: true });
    await encryption.uncheck();
    await expect(page.getByLabel('Send password', { exact: true })).toHaveCount(0);
  } finally {
    for (const id of queued) {
      await rpc('action', { id, action: 'cancel' }).catch(() => {});
      await rpc('action', { id, action: 'remove' }).catch(() => {});
    }
    service.kill();
    await new Promise<void>((resolve) => {
      if (service.exitCode !== null) resolve();
      else service.once('exit', () => resolve());
    });
    rmSync(state, { recursive: true, force: true });
  }
});
