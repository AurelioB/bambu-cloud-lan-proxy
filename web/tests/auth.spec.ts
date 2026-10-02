import { test, expect, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { overview, settings } from './fixtures';

async function setup(
  page: Page,
  mode: 'local' | 'oidc' | 'both' = 'local',
  signedIn = false,
) {
  let active = signedIn;
  const privateRequests: string[] = [];
  await page.addInitScript(() =>
    sessionStorage.setItem('proxy-access-code', 'STALE123'),
  );
  await page.route('**/api/**', async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === '/api/auth/session')
      return route.fulfill({
        json: {
          authenticated: active,
          username: active ? 'admin' : null,
          csrf_token: active ? 'test-csrf' : null,
          access_code: false,
          local: mode !== 'oidc',
          oidc: mode !== 'local',
        },
      });
    if (path === '/api/auth/login') {
      const body = route.request().postDataJSON();
      active =
        body.username === 'admin' && body.password === 'correct-password';
      return route.fulfill({
        status: active ? 200 : 401,
        json: active
          ? { authenticated: true, username: 'admin', csrf_token: 'test-csrf' }
          : { detail: 'Credentials were not accepted.' },
      });
    }
    if (path === '/api/auth/logout') {
      active = false;
      return route.fulfill({ status: 204 });
    }
    if (path === '/api/auth/oidc/start')
      return route.fulfill({
        json: { url: 'https://identity.example.test/authorize' },
      });
    privateRequests.push(path);
    expect(route.request().headers()['x-cloud-lan-proxy-key']).toBeUndefined();
    if (!active)
      return route.fulfill({
        status: 401,
        json: { detail: 'Sign-in required.' },
      });
    if (path === '/api/admin/overview')
      return route.fulfill({ json: overview });
    if (path === '/api/admin/settings') {
      if (route.request().method() === 'PUT')
        expect(route.request().headers()['x-csrf-token']).toBe('test-csrf');
      return route.fulfill({
        json:
          route.request().method() === 'PUT'
            ? route.request().postDataJSON()
            : settings,
      });
    }
    return route.fulfill({ status: 404 });
  });
  return privateRequests;
}

for (const tab of ['overview', 'connections', 'account', 'settings']) {
  test(`unauthenticated deep link to ${tab} never fetches or renders private content`, async ({
    page,
  }) => {
    const requests = await setup(page);
    await page.goto('/?tab=' + tab);
    await expect(page.getByLabel('Username', { exact: true })).toBeVisible();
    await expect(page.getByTestId('printer-card')).toHaveCount(0);
    await expect(
      page.getByRole('button', { name: 'Sign out', exact: true }),
    ).toHaveCount(0);
    await expect(page.getByText('DEMO000000001', { exact: false })).toHaveCount(
      0,
    );
    expect(requests).toEqual([]);
    expect(
      await page.evaluate(() => sessionStorage.getItem('proxy-access-code')),
    ).toBeNull();
  });
}

test('local login, CSRF-protected settings, reload, and server sign-out', async ({
  page,
}, info) => {
  await setup(page);
  await page.goto('/login');
  await page.screenshot({
    path: info.outputPath('local-sign-in.png'),
    fullPage: true,
  });
  await page.getByLabel('Username', { exact: true }).fill('admin');
  await page.getByLabel('Password', { exact: true }).fill('wrong');
  await page.getByRole('button', { name: 'Open workspace' }).click();
  await expect(page.getByRole('alert')).toContainText(
    'Credentials were not accepted',
  );
  await expect(page.getByLabel('Password', { exact: true })).toHaveValue('');
  await page.getByLabel('Password', { exact: true }).fill('correct-password');
  await page.getByRole('button', { name: 'Open workspace' }).click();
  await expect(page.getByTestId('printer-card')).toHaveCount(5);
  expect(
    await page.evaluate(() =>
      JSON.stringify({ ...sessionStorage, ...localStorage }),
    ),
  ).not.toContain('correct-password');
  await page.getByRole('button', { name: 'Permissions', exact: true }).click();
  await page.getByRole('switch', { name: 'Delete printer files' }).click();
  await page.getByRole('button', { name: 'Save permissions' }).click();
  await expect(page.getByText('All changes saved')).toBeVisible();
  await page.reload();
  await expect(
    page.getByRole('heading', { name: 'Permissions, on your terms' }),
  ).toBeVisible();
  const logout = page.waitForRequest('**/api/auth/logout');
  await page.getByRole('button', { name: 'Sign out', exact: true }).click();
  expect((await logout).headers()['x-csrf-token']).toBe('test-csrf');
  await expect(page.getByLabel('Username', { exact: true })).toBeVisible();
  await page.reload();
  await expect(page.getByLabel('Username', { exact: true })).toBeVisible();
  await expect(
    page.getByRole('heading', { name: 'Permissions, on your terms' }),
  ).toHaveCount(0);
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
});

test('OIDC-only has no local or access-code bypass and uses a server-generated sign-in URL', async ({
  page,
  context,
}) => {
  const requests = await setup(page, 'oidc');
  await context.route('https://identity.example.test/**', (route) =>
    route.fulfill({ body: 'Mock identity provider' }),
  );
  await page.goto('/?tab=settings');
  await expect(
    page.getByRole('button', { name: 'Continue with single sign-on' }),
  ).toBeVisible();
  await expect(page.getByLabel('Password', { exact: true })).toHaveCount(0);
  await expect(
    page.getByLabel('Proxy access code', { exact: true }),
  ).toHaveCount(0);
  expect(requests).toEqual([]);
  await page
    .getByRole('button', { name: 'Continue with single sign-on' })
    .click();
  await page.waitForURL('https://identity.example.test/authorize');
});

test('both mode offers local and OIDC and safely handles rejected callbacks', async ({
  page,
}, info) => {
  await setup(page, 'both');
  await page.goto('/login?auth_error=oidc');
  await expect(
    page.getByRole('button', { name: 'Continue with single sign-on' }),
  ).toBeVisible();
  await expect(page.getByLabel('Username', { exact: true })).toBeVisible();
  await expect(page.getByRole('alert')).toContainText('not allowed');
  await page.screenshot({
    path: info.outputPath('oidc-and-local-sign-in.png'),
    fullPage: true,
  });
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
});

test('session expiration from any private API clears all private views', async ({
  page,
}) => {
  await setup(page, 'local', true);
  await page.goto('/?tab=settings');
  await page.getByRole('switch', { name: 'Delete printer files' }).click();
  await page.route('**/api/admin/settings', (route) =>
    route.fulfill({ status: 401, json: { detail: 'Sign-in required.' } }),
  );
  await page.getByRole('button', { name: 'Save permissions' }).click();
  await expect(page.getByLabel('Username', { exact: true })).toBeVisible();
  await expect(page.getByRole('switch')).toHaveCount(0);
  await expect(page.getByRole('alert')).toContainText('session has ended');
});

test('late private responses cannot reopen the workspace after sign-out', async ({
  page,
}) => {
  await setup(page, 'local', true);
  await page.goto('/');
  await expect(page.getByTestId('printer-card')).toHaveCount(5);
  let release!: () => Promise<void>;
  let requested!: () => void;
  const pending = new Promise<void>((resolve) => {
    requested = resolve;
  });
  await page.route('**/api/admin/overview', (route) => {
    release = () => route.fulfill({ json: overview });
    requested();
  });
  await page.getByRole('button', { name: 'Refresh', exact: true }).click();
  await pending;
  await page.getByRole('button', { name: 'Sign out', exact: true }).click();
  await expect(page.getByLabel('Username', { exact: true })).toBeVisible();
  await release();
  await expect(page.getByTestId('printer-card')).toHaveCount(0);
});

test('unavailable authentication fails closed and failed logout requires retry', async ({
  page,
}) => {
  const requests = await setup(page, 'both');
  await page.route('**/api/auth/session', (route) =>
    route.fulfill({ status: 503 }),
  );
  await page.goto('/?tab=account');
  await expect(page.getByText('Sign-in unavailable')).toBeVisible();
  expect(requests).toEqual([]);
  await page.unroute('**/api/auth/session');
  await page.goto('/');
  await page.getByLabel('Username', { exact: true }).fill('admin');
  await page.getByLabel('Password', { exact: true }).fill('correct-password');
  await page.getByRole('button', { name: 'Open workspace' }).click();
  await expect(page.getByTestId('printer-card')).toHaveCount(5);
  await page.route('**/api/auth/logout', (route) =>
    route.fulfill({ status: 503 }),
  );
  await page.getByRole('button', { name: 'Sign out', exact: true }).click();
  await expect(
    page.getByText('Sign-out could not be confirmed', { exact: false }),
  ).toBeVisible();
  await expect(page.getByTestId('printer-card')).toHaveCount(0);
  await expect(page.getByLabel('Username', { exact: true })).toHaveCount(0);
  await page.unroute('**/api/auth/logout');
  await page.getByRole('button', { name: 'Retry sign-out' }).click();
  await expect(page.getByLabel('Username', { exact: true })).toBeVisible();
});
