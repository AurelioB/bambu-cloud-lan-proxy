import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

test('landing page, theme persistence, and installation navigation', async ({
  page,
}, testInfo) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.emulateMedia({ colorScheme: 'light' });
  await page.goto('/');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText(
    'Cloud,meet local.',
  );
  await page.screenshot({
    path: testInfo.outputPath('landing-light.png'),
    fullPage: true,
  });
  await page.screenshot({ path: testInfo.outputPath('landing-desktop.png') });
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await page.getByRole('button', { name: 'Switch color theme' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await page.screenshot({
    path: testInfo.outputPath('landing-dark.png'),
    fullPage: true,
  });
  await page.getByRole('link', { name: 'Set up your proxy' }).click();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText(
    'Install with Docker Compose',
  );
  expect(errors).toEqual([]);
});

test('documentation search works in the static build', async ({
  page,
}, testInfo) => {
  await page.goto('/docs/');
  await page.getByRole('button', { name: /Search/ }).click();
  const input = page.getByRole('textbox', { name: 'Search', exact: true });
  await input.fill('OIDC');
  await expect(page.locator('.pagefind-ui__result').first()).toBeVisible();
  await page.keyboard.press('Escape');
  await page.goto('/docs/api/');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText(
    'HTTP API reference',
  );
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await page.screenshot({
    path: testInfo.outputPath('api-docs.png'),
    fullPage: false,
  });
  const response = await page.request.get('/openapi.json');
  expect(response.ok()).toBeTruthy();
  expect((await response.json()).paths['/api/admin/overview']).toBeTruthy();
});

test('mobile navigation and layout have no horizontal overflow', async ({
  page,
}, testInfo) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/');
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBeTruthy();
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await page.screenshot({
    path: testInfo.outputPath('landing-mobile.png'),
    fullPage: true,
  });
  await page
    .getByRole('navigation', { name: 'Main navigation' })
    .getByRole('link', { name: 'Docs', exact: true })
    .click();
  await page.getByRole('button', { name: 'Menu', exact: true }).click();
  await page
    .getByRole('navigation', { name: 'Main', exact: true })
    .getByRole('link', { name: 'Standalone Linux', exact: true })
    .click();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText(
    'Install standalone on Linux',
  );
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBeTruthy();
});

test('public site never requests a private printer API', async ({ page }) => {
  const apiRequests: string[] = [];
  page.on('request', (request) => {
    if (/\/(api|internal)\//.test(new URL(request.url()).pathname))
      apiRequests.push(request.url());
  });
  for (const route of [
    '/',
    '/docs/',
    '/docs/authentication/',
    '/docs/features/',
  ])
    await page.goto(route);
  expect(apiRequests).toEqual([]);
});
