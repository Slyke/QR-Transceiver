import path from 'node:path';
import { test, expect } from '@playwright/test';

test('payload width preferences, addressed text rows, stable scrolling and saved section order', async ({ page, request }) => {
  const rpc = async (method: string, params: unknown = {}) => {
    const response = await request.post('/dev-rpc', { data: { method, params } });
    const body = await response.json();
    expect(body.ok, JSON.stringify(body.error)).toBe(true);
    return body.result;
  };
  const job = await rpc('enqueue', { paths: [path.resolve('tests/fixtures/webcam-2k.png')] });
  try {
    await page.goto('/');
    await page
      .getByRole('button', { name: /webcam-2k.png/ })
      .first()
      .click();
    await page.getByLabel('Follow active bytes').uncheck();
    await page.getByLabel('Bytes per row', { exact: true }).selectOption('64');
    await page.getByLabel('Show ASCII').uncheck();
    await expect(page.locator('.hex-row').first().locator('button')).toHaveCount(64);
    await expect(page.locator('.hex-row .ascii')).toHaveCount(0);
    await page.getByRole('button', { name: 'Text', exact: true }).click();
    await page.getByLabel('Jump to byte offset').fill('0');
    await page.getByRole('button', { name: 'Go', exact: true }).click();
    await expect(page.locator('.text-row .address').first()).toHaveText('00000000');
    await expect(page.locator('.text-row .address').nth(3)).toHaveText('000000c0');
    const viewport = page.getByRole('grid', { name: 'File bytes' });
    await viewport.evaluate((element) => (element.scrollTop = 182));
    await expect(page.locator('.text-row .address').first()).toHaveText('000001c0');
    for (let i = 0; i < 2; i++)
      await page.waitForResponse(
        (response) => response.url().endsWith('/dev-rpc') && response.request().postDataJSON()?.method === 'status'
      );
    expect(await viewport.evaluate((element) => element.scrollTop)).toBe(182);
    await expect(page.locator('.text-row .address').first()).toHaveText('000001c0');
    await page.screenshot({ path: 'test-results/text-offsets.png', fullPage: true });
    await page.getByRole('button', { name: 'Move Payload inspector up', exact: true }).click();
    await page.getByRole('button', { name: 'Move Payload inspector up', exact: true }).click();
    if (!(await page.getByRole('button', { name: 'Move Payload inspector up', exact: true }).isDisabled()))
      await page.getByRole('button', { name: 'Move Payload inspector up', exact: true }).click();
    await expect(page.locator('.transfer-sections > section').first()).toHaveAttribute('data-section', 'payload');
    await page.reload();
    await page
      .getByRole('button', { name: /webcam-2k.png/ })
      .first()
      .click();
    await expect(page.locator('.transfer-sections > section').first()).toHaveAttribute('data-section', 'payload');
    await expect(page.getByLabel('Bytes per row', { exact: true })).toHaveValue('64');
    await expect(page.getByLabel('Show ASCII')).not.toBeChecked();
    await expect(page.locator('.send-controls')).not.toHaveAttribute('open', '');
    await expect(page.getByRole('button', { name: 'Toggle theme' }).locator('svg circle')).toHaveCount(1);
    await expect(page.getByText('OFFLINE BY DESIGN', { exact: true })).toHaveCount(0);
    await page
      .getByRole('navigation')
      .getByRole('button', { name: /Settings/ })
      .click();
    await expect(page.getByRole('tabpanel')).toHaveCount(1);
    await page.getByRole('tab', { name: 'Camera', exact: true }).click();
    await page.getByRole('button', { name: 'Move Camera up', exact: true }).click();
    await expect(page.getByRole('tab').nth(2)).toHaveText('Camera');
    await page.getByRole('tab', { name: 'MCP', exact: true }).click();
    await expect(page.getByLabel('Enable MCP server')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Apply settings', exact: true })).toHaveCount(0);
    await page.getByRole('tab', { name: 'QR & timing', exact: true }).focus();
    await page.keyboard.press('ArrowRight');
    await expect(page.getByRole('tab', { name: 'Feedback & ACKs' })).toHaveAttribute('aria-selected', 'true');
    await page.reload();
    await page
      .getByRole('navigation')
      .getByRole('button', { name: /Settings/ })
      .click();
    await expect(page.getByRole('tab').nth(2)).toHaveText('Camera');
    await page.screenshot({ path: 'test-results/settings-tabs.png', fullPage: true });
  } finally {
    await rpc('action', { id: job.id, action: 'cancel' });
    await rpc('action', { id: job.id, action: 'remove' });
  }
});
