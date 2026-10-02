import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

test('Handy keeps its independent path when the simulated proxy goes offline', async ({
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
  await page.goto('/#handy-connection');
  const diagram = page.locator('handy-connection');
  await diagram.scrollIntoViewIfNeeded();
  await expect(diagram).toHaveAttribute('data-paused', 'false');
  const map = diagram.getByRole('img');
  await expect(map).toHaveAccessibleName(
    /Handy traffic never passes through the proxy/,
  );
  await diagram
    .getByRole('checkbox', { name: 'Simulate proxy offline' })
    .check();
  await expect(diagram).toHaveAttribute('data-offline', 'true');
  await expect(diagram.locator('[data-explanation]')).toContainText(
    'Handy’s direct cloud path is unchanged',
  );
  await expect(diagram.locator('[data-proxy-state]')).toHaveText(
    'Offline · simulated',
  );
  const packetDisplay = (selector: string) =>
    diagram
      .locator(selector)
      .first()
      .evaluate((element) => getComputedStyle(element, '::after').display);
  expect(await packetDisplay('.proxy-row .connection-wire')).toBe('none');
  expect(await packetDisplay('.handy-row .connection-wire')).not.toBe('none');
  await diagram.getByRole('button', { name: 'Pause animation' }).click();
  await expect(diagram).toHaveAttribute('data-paused', 'true');
  await page
    .locator('#handy-connection')
    .screenshot({ path: info.outputPath('handy-independent-light.png') });
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await page.getByRole('button', { name: 'Switch color theme' }).click();
  await diagram.scrollIntoViewIfNeeded();
  await page
    .locator('#handy-connection')
    .screenshot({ path: info.outputPath('handy-independent-dark.png') });
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await diagram.getByRole('checkbox').uncheck();
  await expect(diagram.locator('[data-explanation]')).toContainText(
    'Both paths are available',
  );
  expect(await packetDisplay('.proxy-row .connection-wire')).not.toBe('none');
  expect(unexpected).toEqual([]);
});

test('Handy diagram supports reduced motion, mobile, and offscreen pause', async ({
  page,
}, info) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/#handy-connection');
  const diagram = page.locator('handy-connection');
  await diagram.scrollIntoViewIfNeeded();
  await expect(diagram).toHaveAttribute('data-paused', 'true');
  expect(
    await diagram
      .locator('.connection-wire')
      .first()
      .evaluate(
        (element) => getComputedStyle(element, '::after').animationPlayState,
      ),
  ).toBe('paused');
  await diagram.getByRole('button', { name: 'Play animation' }).click();
  await expect(diagram).toHaveAttribute('data-paused', 'false');
  await page.locator('footer').scrollIntoViewIfNeeded();
  await expect(diagram).toHaveAttribute('data-paused', 'true');
  await diagram.scrollIntoViewIfNeeded();
  await expect(diagram).toHaveAttribute('data-paused', 'false');
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await expect(diagram).toHaveAttribute('data-paused', 'true');
  await page
    .locator('#handy-connection')
    .screenshot({ path: info.outputPath('handy-independent-mobile.png') });
  for (const width of [390, 320]) {
    await page.setViewportSize({ width, height: 844 });
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBeTruthy();
    const bounds = await diagram
      .locator('.connection-node')
      .evaluateAll((elements) =>
        elements.map((element) => ({
          left: element.getBoundingClientRect().left,
          right: element.getBoundingClientRect().right,
        })),
      );
    expect(
      bounds.every(({ left, right }) => left >= 0 && right <= width),
    ).toBeTruthy();
  }
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
});

test('Handy diagram remains readable without JavaScript', async ({
  browser,
}) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  await page.goto('http://127.0.0.1:4180/#handy-connection');
  const diagram = page.locator('handy-connection');
  await expect(diagram.getByRole('img')).toBeVisible();
  await expect(diagram.getByRole('checkbox')).toBeDisabled();
  await expect(diagram.getByRole('button')).toBeDisabled();
  await expect(diagram.locator('[data-explanation]')).toContainText(
    'Handy uses its direct cloud connection',
  );
  await expect(diagram).toHaveAttribute('data-paused', 'true');
  await context.close();
});
