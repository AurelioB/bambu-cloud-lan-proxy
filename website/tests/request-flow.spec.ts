import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

test('print, reprint, query and storage routes preserve their distinct permissions and fallback rules', async ({
  page,
}) => {
  await page.goto('/#request-flow');
  const flow = page.locator('request-flow');
  for (const [name, rule] of [
    ['Start a staged job', 'cloud printing to be enabled'],
    ['Reprint a stored job', 'remote reprint permission'],
    [
      'Read-only queries',
      'different from the local-first telemetry subscription',
    ],
  ]) {
    await flow.getByRole('radio', { name }).check();
    await expect(flow).toHaveAttribute('data-route', 'cloud');
    await expect(flow.getByRole('checkbox')).toBeHidden();
    await expect(flow.locator('[data-route-rule]')).toContainText(rule);
    await expect(flow.locator('.flow-reply').first()).toBeHidden();
  }
  await expect(flow.locator('[data-explanation]')).toContainText(
    'not the signed-control path',
  );
  await flow.getByRole('radio', { name: 'Storage uploads / deletes' }).check();
  await expect(flow).toHaveAttribute('data-route', 'local');
  await expect(flow.locator('[data-explanation]')).toContainText(
    'permissions are checked first',
  );
  await flow.getByRole('checkbox').check();
  await expect(flow).toHaveAttribute('data-route', 'cloud');
  await expect(flow.locator('[data-explanation]')).toContainText(
    'not by replaying a mutation',
  );
  await flow.getByRole('radio', { name: 'Camera access' }).check();
  await expect(flow).toHaveAttribute('data-route', 'local');
  await expect(flow.locator('[data-fallback]')).toBeDisabled();
});

test('routes files locally or through the cloud without sending real requests', async ({
  page,
}, info) => {
  const unexpected: string[] = [];
  page.on('request', (request) => {
    const url = new URL(request.url());
    if (
      url.origin !== 'http://127.0.0.1:4180' ||
      /^\/(api|internal)\//.test(url.pathname)
    )
      unexpected.push(url.href);
  });
  await page.setViewportSize({ width: 1280, height: 1000 });
  await page.goto('/#request-flow');
  const flow = page.locator('request-flow');
  await flow.scrollIntoViewIfNeeded();
  await expect(flow).toHaveAttribute('data-route', 'local');
  await expect(flow).toHaveAttribute('data-paused', 'false');
  await expect(flow.locator('[data-explanation]')).toContainText('printer LAN');
  await page
    .locator('#request-flow')
    .screenshot({ path: info.outputPath('request-flow-local.png') });
  await flow.getByRole('checkbox').check();
  await expect(flow).toHaveAttribute('data-route', 'cloud');
  await expect(flow.locator('[data-explanation]')).toContainText(
    'media tunnel',
  );
  await flow.getByRole('button', { name: 'Pause animation' }).click();
  await expect(flow).toHaveAttribute('data-paused', 'true');
  await page
    .locator('#request-flow')
    .screenshot({ path: info.outputPath('request-flow-cloud.png') });
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await page.getByRole('button', { name: 'Switch color theme' }).click();
  await flow.scrollIntoViewIfNeeded();
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await page
    .locator('#request-flow')
    .screenshot({ path: info.outputPath('request-flow-dark.png') });
  expect(unexpected).toEqual([]);
});

test('camera is local, controls use cloud, reports reverse direction, and staging stops at the proxy', async ({
  page,
}) => {
  await page.goto('/#request-flow');
  const flow = page.locator('request-flow');
  await flow.getByRole('radio', { name: 'Camera access' }).check();
  await expect(flow).toHaveAttribute('data-route', 'local');
  await expect(flow.getByRole('checkbox')).toBeHidden();
  await expect(flow.locator('[data-route-rule]')).toContainText(
    'No cloud fallback',
  );
  await flow.getByRole('radio', { name: 'Printer controls' }).check();
  await expect(flow).toHaveAttribute('data-route', 'cloud');
  await expect(flow.locator('[data-explanation]')).toContainText(
    'checks permissions',
  );
  await flow.getByRole('radio', { name: 'Status reports' }).check();
  await expect(flow).toHaveAttribute('data-route', 'local');
  await expect(flow.locator('.flow-request').first()).toBeHidden();
  await expect(flow.locator('[data-explanation]')).toContainText(
    'subscribed telemetry stream',
  );
  await flow.getByRole('checkbox').check();
  await expect(flow).toHaveAttribute('data-route', 'cloud');
  await expect(flow.locator('[data-explanation]')).toContainText(
    'incoming telemetry reports',
  );
  await flow.getByRole('radio', { name: 'Job staging' }).check();
  await expect(flow).toHaveAttribute('data-route', 'staging');
  await expect(flow.locator('[data-route-label]')).toHaveText(
    'Stops at the proxy',
  );
  await expect(flow.getByRole('checkbox')).toBeHidden();
  // Native radio keyboard navigation is retained.
  await page.keyboard.press('ArrowLeft');
  await expect(
    flow.getByRole('radio', { name: 'Status reports' }),
  ).toBeChecked();
});

test('reduced motion, explicit play, and offscreen pause', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/#request-flow');
  const flow = page.locator('request-flow');
  await flow.scrollIntoViewIfNeeded();
  await expect(flow).toHaveAttribute('data-paused', 'true');
  await expect(
    flow.getByRole('button', { name: 'Play animation' }),
  ).toBeEnabled();
  await expect(
    flow.locator('.flow-desktop [data-flow-route="local"] .flow-request'),
  ).toHaveCSS('animation-play-state', 'paused');
  await flow.getByRole('button', { name: 'Play animation' }).click();
  await expect(flow).toHaveAttribute('data-paused', 'false');
  await page.locator('footer').scrollIntoViewIfNeeded();
  await expect(flow).toHaveAttribute('data-paused', 'true');
  await flow.scrollIntoViewIfNeeded();
  await expect(flow).toHaveAttribute('data-paused', 'false');
  await flow.getByRole('button', { name: 'Pause animation' }).click();
  await expect(flow).toHaveAttribute('data-paused', 'true');
});

test('mobile diagram and controls fit narrow screens', async ({
  page,
}, info) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/#request-flow');
  const flow = page.locator('request-flow');
  await flow.getByRole('radio', { name: 'Printer controls' }).check();
  await expect(flow.locator('.flow-mobile')).toBeVisible();
  await expect(flow.locator('.flow-desktop')).toBeHidden();
  await page
    .locator('#request-flow')
    .screenshot({ path: info.outputPath('request-flow-mobile.png') });
  for (const width of [390, 320]) {
    await page.setViewportSize({ width, height: 844 });
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBeTruthy();
    const nodes = await flow.locator('.flow-node').evaluateAll((elements) =>
      elements.map((element) => {
        const rect = element.getBoundingClientRect();
        return { left: rect.left, right: rect.right };
      }),
    );
    expect(
      nodes.every(({ left, right }) => left >= 0 && right <= width),
    ).toBeTruthy();
  }
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
});

test('without JavaScript the local route remains readable and controls cannot mislead', async ({
  browser,
}) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  await page.goto('http://127.0.0.1:4180/#request-flow');
  const flow = page.locator('request-flow');
  await expect(
    flow.getByRole('radio', { name: 'File browsing' }),
  ).toBeChecked();
  await expect(
    flow.getByRole('radio', { name: 'Printer controls' }),
  ).toBeDisabled();
  await expect(
    flow.getByRole('button', { name: 'Play animation' }),
  ).toBeDisabled();
  await expect(flow.locator('[data-explanation]')).toContainText('printer LAN');
  await context.close();
});
