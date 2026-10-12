import path from 'node:path';
import { test, expect } from '@playwright/test';

test('live send controls, uninterrupted loops, compact hex rows and resizable dark detached display', async ({ page, request }) => {
  const rpc = async (method: string, params: unknown = {}) => {
    const response = await request.post('/dev-rpc', { data: { method, params } });
    const body = await response.json();
    expect(body.ok, JSON.stringify(body.error)).toBe(true);
    return body.result;
  };
  const original = (await rpc('status')).profile;
  await rpc('set_profile', { ...original, mode: 'send', policy: 'loop', chunk_size: 256 });
  const job = await rpc('enqueue', { paths: [path.resolve('tests/fixtures/webcam-2k.png')] });
  await rpc('action', { id: job.id, action: 'priority', priority: 1000 });
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  try {
    await page.setViewportSize({ width: 2000, height: 1100 });
    await page.goto('/');
    await page
      .getByRole('button', { name: /webcam-2k.png/ })
      .first()
      .click();
    await page.getByLabel('Follow active bytes').uncheck();
    await expect(page.locator('.hex-row').first().locator('button')).toHaveCount(64);
    await expect(page.locator('.hex-row .address').nth(1)).toHaveText('00000040');
    const first = await page.locator('.hex-row').first().locator('button').nth(0).boundingBox();
    const second = await page.locator('.hex-row').first().locator('button').nth(1).boundingBox();
    expect(second!.x - first!.x).toBe(16);
    await expect(page.locator('.send-controls')).not.toHaveAttribute('open', '');
    await expect(page.getByRole('button', { name: 'Fullscreen display', exact: true })).toHaveCount(0);
    await page.getByText('Send controls', { exact: false }).click();
    await page.getByLabel('Frame rate (FPS)', { exact: true }).fill('10');
    await expect.poll(async () => (await rpc('transfer', { id: job.id })).job.profile.frame_ms).toBe(100);
    await page.getByRole('button', { name: /Start display/ }).click();
    await expect(page.locator('.qr-image svg')).toBeVisible();
    const frames: (number | null)[] = [];
    const listener = async (response: import('@playwright/test').Response) => {
      if (response.url().endsWith('/dev-rpc') && response.request().postDataJSON()?.method === 'next_frame') {
        frames.push((await response.json()).result?.duration_ms ?? null);
      }
    };
    page.on('response', listener);
    await expect.poll(() => frames.length, { timeout: 10000 }).toBeGreaterThan(25);
    page.off('response', listener);
    expect(frames.every((duration) => duration === 100)).toBe(true);
    await page.getByLabel('Frame rate (FPS)', { exact: true }).fill('5');
    await expect.poll(async () => (await rpc('transfer', { id: job.id })).job.profile.frame_ms).toBe(200);
    const updatedFrame = await page.waitForResponse(
      (response) => response.url().endsWith('/dev-rpc') && response.request().postDataJSON()?.method === 'next_frame'
    );
    expect((await updatedFrame.json()).result.duration_ms).toBe(200);
    await page.getByRole('button', { name: /Pause display/ }).click();
    const popupPromise = page.waitForEvent('popup');
    await page.getByRole('button', { name: /Detach display/ }).click();
    const popup = await popupPromise;
    await popup.setViewportSize({ width: 1100, height: 1100 });
    const qr = popup.locator('.detached-qr svg');
    await expect(qr).toBeVisible();
    await expect(popup.locator('.detached')).toHaveCSS('background-color', 'rgb(5, 6, 8)');
    expect((await qr.boundingBox())!.width).toBeGreaterThan(900);
    await popup.setViewportSize({ width: 500, height: 400 });
    await expect.poll(async () => (await qr.boundingBox())!.width).toBeLessThan(400);
    const oldViewBox = await qr.getAttribute('viewBox');
    await page.getByLabel('Quiet zone (modules)', { exact: true }).fill('8');
    await expect(qr).not.toHaveAttribute('viewBox', oldViewBox!);
    await popup.screenshot({ path: 'test-results/detached-dark.png' });
    await popup.close();
    await page.screenshot({ path: 'test-results/send-controls-wide.png', fullPage: true });
    await page.setViewportSize({ width: 720, height: 1100 });
    await expect(page.locator('.hex-row').first().locator('button')).toHaveCount(16);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
    expect(errors).toEqual([]);
  } finally {
    await rpc('action', { id: job.id, action: 'cancel' });
    await rpc('action', { id: job.id, action: 'remove' });
    await rpc('set_profile', original);
  }
});
