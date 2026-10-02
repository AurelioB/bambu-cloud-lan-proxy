import { test, expect, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { overview, settings } from './fixtures';

async function mock(page: Page, data = overview) {
  let authenticated = true;
  await page.route('**/api/**', async (route) => {
    const url = new URL(route.request().url());
    if (url.pathname === '/api/auth/session')
      return route.fulfill({
        json: {
          authenticated,
          username: authenticated ? 'Administrator' : null,
          csrf_token: authenticated ? 'mock-csrf' : null,
          access_code: true,
          local: false,
          oidc: false,
        },
      });
    if (url.pathname === '/api/auth/logout') {
      authenticated = false;
      return route.fulfill({ status: 204 });
    }
    if (url.pathname === '/api/auth/login') {
      if (route.request().postDataJSON().access_code !== 'DEMO1234')
        return route.fulfill({
          status: 401,
          json: { detail: 'Credentials were not accepted.' },
        });
      authenticated = true;
      return route.fulfill({
        json: {
          authenticated,
          username: 'Administrator',
          csrf_token: 'mock-csrf',
        },
      });
    }
    if (url.pathname === '/api/admin/overview')
      return route.fulfill({ json: data });
    if (url.pathname === '/api/admin/settings')
      return route.fulfill({
        json:
          route.request().method() === 'PUT'
            ? route.request().postDataJSON()
            : settings,
      });
    if (url.pathname.endsWith('/endpoint'))
      return route.fulfill({ json: { saved: true } });
    return route.fulfill({ status: 404 });
  });
}

test('mixed capabilities, filters, list layout, theme, and screenshots', async ({
  page,
}, info) => {
  await page.setViewportSize({ width: 1440, height: 1100 });
  await mock(page);
  await page.goto('/');
  await expect(page.getByTestId('printer-card')).toHaveCount(5);
  await expect(
    page.getByTestId('printer-card').filter({ hasText: 'Little maker' }),
  ).toContainText('Camera relay ready');
  await expect(page.getByText('2 units · 8 slots')).toHaveCount(1);
  await expect(page.getByText('Reported by printer')).toHaveCount(2);
  await expect(
    page.getByTestId('printer-card').filter({ hasText: 'New arrival' }),
  ).toContainText('Capabilities unknown');
  await page.screenshot({
    path: info.outputPath('overview-light.png'),
    fullPage: true,
  });
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await page.getByLabel('Search printers').fill('mini');
  await expect(page.getByTestId('printer-card')).toHaveCount(1);
  await page.getByLabel('Search printers').fill('nothing');
  await expect(page.getByText('No matching printers')).toBeVisible();
  await page.getByRole('button', { name: 'Clear filters' }).click();
  await page
    .getByRole('radio', { name: 'Needs attention', exact: true })
    .click();
  await expect(page.getByTestId('printer-card')).toHaveCount(2);
  await page.getByRole('radio', { name: 'All printers' }).click();
  await page.getByRole('radio', { name: 'List view' }).click();
  await expect(page.getByRole('table')).toBeVisible();
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await page.getByRole('radio', { name: 'Card view' }).click();
  await page.getByRole('button', { name: 'Choose appearance' }).click();
  await page.getByRole('menuitem', { name: 'Dark', exact: true }).click();
  await expect(page.locator('html')).toHaveClass(/dark/);
  await page.screenshot({
    path: info.outputPath('overview-dark.png'),
    fullPage: true,
  });
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
});

test('observed capabilities appear in configuration and unknown toolheads remain unknown', async ({
  page,
}) => {
  const data = structuredClone(overview);
  const unknown = data.printers[4];
  unknown.capabilities.source = 'runtime_telemetry';
  unknown.capabilities.model_known = false;
  unknown.capabilities.observed_fields = ['ams_units'];
  unknown.capabilities.runtime = {
    updated_at: 1_790_000_000,
    firmware: null,
    ams_units: 0,
    filament_slots: 0,
    ams: {},
    sensors: [],
    firmware_controls: {},
    dual_nozzle: null,
    chamber_temperature: null,
  };
  await mock(page, data);
  await page.goto('/');
  await expect(
    page.getByTestId('printer-card').filter({ hasText: 'New arrival' }),
  ).toContainText('Capabilities unknown');
  await page
    .getByRole('button', { name: 'Configure Workshop', exact: true })
    .click();
  const details = page.getByRole('dialog');
  await expect(details).toContainText('01.02.03.04');
  await expect(
    details.getByLabel('Observed printer capabilities'),
  ).toContainText(/Filament slots\s*8/);
  await expect(details.getByLabel('Reported sensors')).toContainText(
    'chamber temperature',
  );
  await expect(details.getByLabel('Firmware-reported functions')).toContainText(
    'Reported unavailable',
  );
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
});

test('failed saves retain drafts and permit a safe retry', async ({ page }) => {
  await mock(page);
  await page.goto('/');
  await page
    .getByRole('button', { name: 'Configure Workshop', exact: true })
    .click();
  await page
    .getByLabel('Virtual printer address', { exact: true })
    .fill('192.0.2.211');
  await page.route('**/api/admin/printers/*/endpoint', (route) =>
    route.fulfill({ status: 409, body: 'Address is already assigned.' }),
  );
  await page.getByRole('button', { name: 'Save connection' }).click();
  await expect(
    page.getByRole('alert').filter({ hasText: 'Connection was not saved' }),
  ).toContainText('Address is already assigned');
  await expect(
    page.getByLabel('Virtual printer address', { exact: true }),
  ).toHaveValue('192.0.2.211');
  await page.getByRole('button', { name: 'Cancel', exact: true }).click();
  await expect(page.getByRole('dialog')).not.toBeVisible();
  await expect(
    page.getByRole('button', { name: 'Configure Workshop', exact: true }),
  ).toBeFocused();
  await page.goto('/?tab=settings');
  await page.getByRole('switch', { name: 'Delete printer files' }).click();
  await page.route('**/api/admin/settings', (route) =>
    route.fulfill({ status: 503, body: 'Temporarily unavailable' }),
  );
  await page.getByRole('button', { name: 'Save permissions' }).click();
  await expect(
    page.getByRole('alert').filter({ hasText: 'Permissions were not saved' }),
  ).toBeVisible();
  await expect(
    page.getByRole('switch', { name: 'Delete printer files' }),
  ).toBeChecked();
  await expect(page.getByText('You have unsaved changes')).toBeVisible();
  await page.route('**/api/admin/settings', (route) =>
    route.fulfill({ json: route.request().postDataJSON() }),
  );
  await page.getByRole('button', { name: 'Save permissions' }).click();
  await expect(page.getByText('All changes saved')).toBeVisible();
});

test('account callback and certificate download preserve the API contract', async ({
  page,
  context,
}, info) => {
  await mock(page);
  await context.route('https://signin.example.test/**', (route) =>
    route.fulfill({ body: 'Mock Bambu sign-in' }),
  );
  await page.route('**/api/cloud/auth/url', (route) =>
    route.fulfill({ json: { url: 'https://signin.example.test/login' } }),
  );
  await page.route('**/api/cloud/auth/paste', (route) =>
    route.fulfill({ json: { ok: true } }),
  );
  await page.goto('/?tab=account');
  const popupPromise = page.waitForEvent('popup');
  await page
    .getByRole('button', { name: 'Open Bambu sign-in', exact: true })
    .click();
  const popup = await popupPromise;
  await popup.waitForURL('https://signin.example.test/login');
  expect(await popup.evaluate(() => window.opener)).toBeNull();
  await popup.close();
  await page
    .getByLabel('Paste the return URL', { exact: false })
    .fill('http://localhost:13618/?ticket=DEMO');
  const request = page.waitForRequest('**/api/cloud/auth/paste');
  await page.getByRole('button', { name: 'Complete connection' }).click();
  const callback = await request;
  expect(callback.postDataJSON()).toEqual({
    pasted_url: 'http://localhost:13618/?ticket=DEMO',
  });
  expect(callback.headers()['x-cloud-lan-proxy-key']).toBeUndefined();
  expect(callback.headers()['x-csrf-token']).toBe('mock-csrf');
  await expect(
    page.getByLabel('Paste the return URL', { exact: false }),
  ).toHaveValue('');
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await page.getByRole('button', { name: 'Connections', exact: true }).click();
  await page.route('**/api/admin/ca', (route) =>
    route.fulfill({
      contentType: 'application/x-pem-file',
      body: 'MOCK PUBLIC CERTIFICATE',
    }),
  );
  const download = page.waitForEvent('download');
  await page
    .getByRole('button', { name: 'Download proxy certificate' })
    .click();
  expect((await download).suggestedFilename()).toBe(
    'bambu-cloud-lan-proxy-ca.crt',
  );
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({
    path: info.outputPath('connections.png'),
    fullPage: true,
  });
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
});

test('endpoint drafts survive polling, validate input, and save only the selected printer', async ({
  page,
}, info) => {
  await page.setViewportSize({ width: 1440, height: 1100 });
  await mock(page);
  await page.clock.install();
  await page.goto('/');
  await page
    .getByRole('button', { name: 'Configure Workshop', exact: true })
    .click();
  await page
    .getByLabel('Virtual printer address', { exact: true })
    .fill('192.0.2.210');
  await page.getByLabel('Printer LAN access code').fill('TEST5678');
  await page.clock.fastForward(16000);
  await expect(
    page.getByLabel('Virtual printer address', { exact: true }),
  ).toHaveValue('192.0.2.210');
  await expect(page.getByLabel('Printer LAN access code')).toHaveValue(
    'TEST5678',
  );
  await page.screenshot({ path: info.outputPath('printer-connection.png') });
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await page
    .getByLabel('Virtual printer address', { exact: true })
    .fill('999.1.2.3');
  await expect(
    page.getByRole('button', { name: 'Save connection' }),
  ).toBeDisabled();
  await page
    .getByLabel('Virtual printer address', { exact: true })
    .fill('192.0.2.210');
  const request = page.waitForRequest(
    (r) => r.method() === 'PUT' && r.url().endsWith('/DEMO000000001/endpoint'),
  );
  await page.getByRole('button', { name: 'Save connection' }).click();
  expect((await request).postDataJSON()).toEqual({
    virtual_ip: '192.0.2.210',
    physical_ip: '192.0.2.51',
    physical_access_code: 'TEST5678',
    enabled: true,
  });
  await expect(page.getByRole('dialog')).not.toBeVisible();
});

test('permissions survive refresh, reset and persist on save', async ({
  page,
}, info) => {
  await mock(page);
  await page.clock.install();
  await page.goto('/?tab=settings');
  await page.getByRole('switch', { name: 'Delete printer files' }).click();
  await page.clock.fastForward(16000);
  await expect(
    page.getByRole('switch', { name: 'Delete printer files' }),
  ).toBeChecked();
  await expect(page.getByText('You have unsaved changes')).toBeVisible();
  await page.getByRole('button', { name: 'Reset' }).click();
  await expect(
    page.getByRole('switch', { name: 'Delete printer files' }),
  ).not.toBeChecked();
  await page.getByRole('radio', { name: 'Read only', exact: true }).click();
  await page.getByRole('button', { name: 'Save permissions' }).click();
  await expect(page.getByText('All changes saved')).toBeVisible();
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({
    path: info.outputPath('permissions.png'),
    fullPage: true,
  });
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
});

test('mobile navigation, connection guide, account, and empty workspace', async ({
  page,
}, info) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await mock(page);
  await page.goto('/');
  await expect(page.getByTestId('printer-card')).toHaveCount(5);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBeTruthy();
  await page.screenshot({
    path: info.outputPath('mobile.png'),
    fullPage: true,
  });
  await page.screenshot({ path: info.outputPath('mobile-viewport.png') });
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await page.getByRole('button', { name: 'Toggle Sidebar' }).click();
  await page.getByRole('button', { name: 'Connections', exact: true }).click();
  await expect(page.getByRole('dialog')).not.toBeVisible();
  await expect(
    page.getByRole('heading', { name: 'Connect your favorite apps' }),
  ).toBeVisible();
  await page
    .getByRole('button', { name: 'Why is my camera unavailable?' })
    .click();
  await expect(
    page.getByText('Image cameras share port 6000 with printer files.', {
      exact: false,
    }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBeTruthy();
  await page.goto('/?tab=account');
  await page.screenshot({
    path: info.outputPath('account-mobile.png'),
    fullPage: true,
  });
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await page.route('**/api/admin/overview', (route) =>
    route.fulfill({ json: { ...overview, printers: [] } }),
  );
  await page.goto('/');
  await expect(page.getByText('Your workspace starts here')).toBeVisible();
});

test('unlock errors, network recovery, and lock do not expose stale authenticated content', async ({
  page,
}) => {
  await mock(page);
  await page.goto('/');
  await expect(page.getByTestId('printer-card')).toHaveCount(5);
  await page.route('**/api/admin/overview', (route) =>
    route.fulfill({ status: 503 }),
  );
  await page.getByRole('button', { name: 'Refresh', exact: true }).click();
  await expect(page.getByText('Connection interrupted')).toBeVisible();
  await expect(page.getByTestId('printer-card')).toHaveCount(5);
  await page.route('**/api/admin/overview', (route) =>
    route.fulfill({ json: overview }),
  );
  await page.getByRole('button', { name: 'Refresh', exact: true }).click();
  await expect(page.getByText('Connection interrupted')).not.toBeVisible();
  await page.getByRole('button', { name: 'Sign out' }).click();
  await expect(
    page.getByRole('heading', { name: 'Printer workspace' }),
  ).not.toBeVisible();
  await page.route('**/api/admin/overview', (route) =>
    route.fulfill({ status: 401 }),
  );
  await page.getByLabel('Proxy access code', { exact: true }).fill('WRONG');
  await page.getByRole('button', { name: 'Open workspace' }).click();
  await expect(page.getByRole('alert')).toContainText('not accepted');
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
});
