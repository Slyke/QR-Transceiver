import path from 'node:path';
import { test, expect } from '@playwright/test';
test('real engine queue, QR rendering, payload inspection, settings and responsive layout', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto('/');
  await expect(page.getByText('Local engine', { exact: true })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Optical workspace' })).toBeVisible();
  await page.getByLabel('File or directory path', { exact: true }).fill(path.resolve('tests/fixtures/hello.txt'));
  await page.getByRole('button', { name: 'Queue path', exact: true }).click();
  await expect(page.getByRole('button', { name: /hello.txt/ }).first()).toBeVisible();
  await expect(page.getByRole('grid', { name: 'File bytes' })).toBeVisible();
  await page.getByRole('button', { name: /Start display/ }).click();
  await expect(page.locator('.qr-image svg')).toBeVisible();
  await expect(page.getByText('TRANSMITTING', { exact: true })).toBeVisible();
  await page.screenshot({ path: 'test-results/workspace-dark.png', fullPage: true });
  await page.getByRole('button', { name: /Pause display/ }).click();
  await page.getByRole('button', { name: 'Toggle theme' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await page.screenshot({ path: 'test-results/workspace-light.png', fullPage: true });
  await page
    .getByRole('navigation')
    .getByRole('button', { name: /Settings/ })
    .click();
  await page.getByRole('tab', { name: 'Feedback & ACKs' }).click();
  await expect(page.getByText('ACK frames and remove them from rotation')).toBeVisible();
  await page.getByRole('tab', { name: 'QR & timing' }).click();
  await page.getByLabel('Frame duration (ms)', { exact: true }).fill('300');
  await page.getByRole('button', { name: 'Apply settings' }).click();
  await expect(page.getByText('Settings applied')).toBeVisible();
  await page.setViewportSize({ width: 720, height: 1100 });
  await page
    .getByRole('navigation')
    .getByRole('button', { name: /Dashboard/ })
    .click();
  await expect(page.getByRole('heading', { name: 'Optical workspace' })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
  await page.screenshot({ path: 'test-results/workspace-narrow.png', fullPage: true });
  expect(errors).toEqual([]);
});
