<script lang="ts">
  import { onMount, tick } from 'svelte';
  import {
    Boxes,
    RefreshCw,
    Moon,
    Sun,
    Monitor,
    CircleAlert,
  } from '@lucide/svelte';
  import { ModeWatcher, mode, setMode } from 'mode-watcher';
  import * as Sidebar from '$lib/components/ui/sidebar';
  import * as Card from '$lib/components/ui/card';
  import * as Alert from '$lib/components/ui/alert';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
  import { Button } from '$lib/components/ui/button';
  import { Badge } from '$lib/components/ui/badge';
  import { Skeleton } from '$lib/components/ui/skeleton';
  import { Spinner } from '$lib/components/ui/spinner';
  import { Toaster } from '$lib/components/ui/sonner';
  import SignIn from '$lib/components/SignIn.svelte';
  import AppSidebar from '$lib/components/AppSidebar.svelte';
  import Fleet from '$lib/components/Fleet.svelte';
  import Account from '$lib/components/Account.svelte';
  import Policy from '$lib/components/Policy.svelte';
  import Connections from '$lib/components/Connections.svelte';
  import EndpointSheet from '$lib/components/EndpointSheet.svelte';
  import {
    api,
    setCsrfToken,
    type AuthSession,
    type Overview,
    type Settings,
    type Printer,
  } from '$lib/api';

  const pages: Record<string, { title: string; subtitle: string }> = {
    overview: {
      title: 'Printer workspace',
      subtitle: 'Every printer. One place to keep things connected.',
    },
    connections: {
      title: 'Connect your favorite apps',
      subtitle:
        'A local address for every printer, with cloud control behind it.',
    },
    account: {
      title: 'Your Bambu account',
      subtitle: 'The cloud connection that brings your workspace together.',
    },
    settings: {
      title: 'Permissions, on your terms',
      subtitle: 'Choose what connected apps can do across your printers.',
    },
  };
  let authOptions = $state<AuthSession | null>(null);
  let hasSession = $state(false);
  let signingOut = $state(false);
  let signOutError = $state('');
  let overview = $state<Overview | null>(null);
  let settings = $state<Settings | null>(null);
  let authenticated = $state(false);
  let loading = $state(false);
  let initialLoading = $state(true);
  let error = $state('');
  let activePage = $state('overview');
  let editing = $state<Printer | null>(null);
  let lastUpdated = $state('');
  let requestGeneration = 0;
  let editorTrigger: HTMLElement | null = null;

  function editPrinter(printer: Printer) {
    editorTrigger =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    editing = printer;
  }
  async function closeEditor() {
    editing = null;
    await tick();
    if (editorTrigger?.isConnected) editorTrigger.focus();
    editorTrigger = null;
  }

  function pageFromUrl() {
    const value = new URLSearchParams(location.search).get('tab') ?? 'overview';
    activePage = pages[value] ? value : 'overview';
  }
  function navigate(page: string) {
    if (!pages[page]) return;
    activePage = page;
    const url = new URL(location.href);
    url.searchParams.set('tab', page);
    history.pushState({}, '', url);
  }
  function clearPrivate() {
    requestGeneration++;
    hasSession = false;
    authenticated = false;
    overview = null;
    settings = null;
    editing = null;
    error = '';
    loading = false;
    initialLoading = false;
  }
  function expired() {
    clearPrivate();
    setCsrfToken(null);
    error = 'Your session has ended. Please sign in again.';
  }
  async function lock() {
    if (signingOut) return;
    clearPrivate();
    signingOut = true;
    signOutError = '';
    try {
      await api('/api/auth/logout', { method: 'POST' });
      setCsrfToken(null);
      history.replaceState({}, '', '/login');
      channel?.postMessage('logout');
    } catch {
      signOutError =
        'Sign-out could not be confirmed by the server. Retry before leaving this browser unattended.';
    } finally {
      signingOut = false;
    }
  }
  async function signedIn(session: AuthSession) {
    authOptions = session;
    hasSession = session.authenticated;
    setCsrfToken(session.csrf_token);
    if (location.pathname === '/login') history.replaceState({}, '', '/');
    await refresh();
  }
  async function bootstrap() {
    const generation = requestGeneration;
    initialLoading = true;
    try {
      const session = await api<AuthSession>('/api/auth/session');
      if (generation !== requestGeneration) return;
      authOptions = session;
      hasSession = session.authenticated;
      setCsrfToken(session.csrf_token);
      if (session.authenticated) await refresh();
      else if (new URLSearchParams(location.search).has('auth_error')) {
        error =
          'Single sign-on was not completed or this account is not allowed. Please try again or contact your administrator.';
        history.replaceState({}, '', '/login');
      }
    } catch {
      error =
        'Could not check sign-in options. Check the connection and retry.';
    } finally {
      initialLoading = false;
    }
  }
  let channel: BroadcastChannel | null = null;
  async function refresh() {
    if (!hasSession || loading || signingOut) return;
    const generation = ++requestGeneration;
    const needsSettings = !settings;
    loading = true;
    try {
      const [next, initialSettings] = await Promise.all([
        api<Overview>('/api/admin/overview'),
        needsSettings
          ? api<Settings>('/api/admin/settings')
          : Promise.resolve(null),
      ]);
      if (generation !== requestGeneration) return;
      overview = {
        ...next,
        printers: next.printers.map((p) => ({
          ...p,
          physical_access_code: '',
        })),
      };
      if (initialSettings) settings = initialSettings;
      authenticated = true;
      error = '';
      lastUpdated = new Date().toLocaleTimeString([], {
        hour: '2-digit',
        minute: '2-digit',
      });
    } catch (e) {
      if (generation !== requestGeneration) return;
      error = authenticated
        ? 'Could not refresh the workspace. The last received status is shown below.'
        : 'Could not reach the proxy. Check the connection and try again.';
    } finally {
      if (generation === requestGeneration) {
        loading = false;
        initialLoading = false;
      } else if (!hasSession) initialLoading = false;
    }
  }
  onMount(() => {
    pageFromUrl();
    // Remove the credential left by older versions; never send it as a fallback.
    sessionStorage.removeItem('proxy-access-code');
    if ('BroadcastChannel' in window) {
      channel = new BroadcastChannel('bambu-admin-session');
      channel.onmessage = () => expired();
    }
    window.addEventListener('session-expired', expired);
    const resume = (event: PageTransitionEvent) => {
      if (event.persisted) {
        clearPrivate();
        void bootstrap();
      }
    };
    window.addEventListener('pageshow', resume);
    void bootstrap();
    const timer = window.setInterval(() => {
      if (authenticated && !document.hidden) void refresh();
    }, 15000);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener('session-expired', expired);
      window.removeEventListener('pageshow', resume);
      channel?.close();
      requestGeneration++;
    };
  });
</script>

<svelte:window onpopstate={pageFromUrl} />
<svelte:head
  ><title
    >{authenticated ? pages[activePage].title : 'Welcome'} · Bambu Cloud LAN Proxy</title
  ><meta
    name="description"
    content="Bring your Bambu printers and local apps together. Manage connections, capabilities, and permissions in one workspace."
  /></svelte:head
>
<ModeWatcher />
<Toaster />

{#snippet themeMenu()}
  <DropdownMenu.Root
    ><DropdownMenu.Trigger
      >{#snippet child({ props })}<Button
          {...props}
          variant="ghost"
          size="icon-sm"
          aria-label="Choose appearance"
          >{#if mode.current === 'dark'}<Moon />{:else}<Sun />{/if}</Button
        >{/snippet}</DropdownMenu.Trigger
    ><DropdownMenu.Content align="end"
      ><DropdownMenu.Group
        ><DropdownMenu.Label>Appearance</DropdownMenu.Label><DropdownMenu.Item
          onclick={() => setMode('light')}><Sun />Light</DropdownMenu.Item
        ><DropdownMenu.Item onclick={() => setMode('dark')}
          ><Moon />Dark</DropdownMenu.Item
        ><DropdownMenu.Item onclick={() => setMode('system')}
          ><Monitor />Use system setting</DropdownMenu.Item
        ></DropdownMenu.Group
      ></DropdownMenu.Content
    ></DropdownMenu.Root
  >
{/snippet}

{#if initialLoading}
  <main
    class="mx-auto flex max-w-6xl flex-col gap-8 p-8"
    aria-label="Loading workspace"
    aria-busy="true"
  >
    <Skeleton class="h-10 w-56" /><Skeleton class="h-64 w-full" />
    <div class="grid gap-6 md:grid-cols-3">
      {#each [1, 2, 3] as id (id)}<Skeleton class="h-80 w-full" />{/each}
    </div>
    <p role="status" class="text-sm text-muted-foreground">
      Opening your workspace…
    </p>
  </main>
{:else if !authenticated || !overview}
  <div class="login-page">
    <header class="flex items-center justify-between">
      <a class="brand" href="/"
        ><span class="brand-mark"><Boxes /></span><span
          ><strong>bambu<span class="text-primary">.</span></strong><small
            >CLOUD LAN PROXY</small
          ></span
        ></a
      >{@render themeMenu()}
    </header>
    <main class="login-layout">
      <section class="login-copy">
        <p class="eyebrow">YOUR PRINTERS. MORE POSSIBILITIES.</p>
        <h1>
          A little cloud.<br />A lot of
          <span class="text-primary">connection.</span>
        </h1>
        <p>
          Bring your Bambu printers and favorite local apps together. All in
          your own workspace.
        </p>
        <div class="flex flex-wrap gap-2">
          <Badge variant="outline">Multiple printers</Badge><Badge
            variant="outline">Local connections</Badge
          ><Badge variant="outline">Your permissions</Badge>
        </div>
      </section>
      {#if signingOut || signOutError}
        <Card.Root>
          <Card.Header
            ><Card.Title>Securing your workspace</Card.Title><Card.Description>
              {signOutError || 'Signing out…'}
            </Card.Description></Card.Header
          >
          <Card.Footer
            ><Button disabled={signingOut} onclick={lock}>Retry sign-out</Button
            ></Card.Footer
          >
        </Card.Root>
      {:else if authOptions}
        <SignIn options={authOptions} {signedIn} message={error} />
      {:else}
        <Card.Root>
          <Card.Header
            ><Card.Title>Sign-in unavailable</Card.Title><Card.Description
              >{error}</Card.Description
            ></Card.Header
          >
          <Card.Footer
            ><Button onclick={bootstrap}>Retry connection</Button></Card.Footer
          >
        </Card.Root>
      {/if}
    </main>
    <footer class="text-xs text-muted-foreground">
      Self-hosted. Built for Bambu Lab printers.
    </footer>
  </div>
{:else}
  <Sidebar.Provider style="--sidebar-width: 15.5rem;">
    <AppSidebar
      active={activePage}
      {navigate}
      count={overview.printers.length}
      connected={overview.cloud_connected}
      {lock}
    />
    <div class="workspace-body">
      <header class="topbar">
        <div class="flex items-center gap-3">
          <Sidebar.Trigger /><span class="text-sm text-muted-foreground"
            >Workspace</span
          ><span class="text-muted-foreground">/</span><span class="text-sm"
            >{activePage === 'overview'
              ? 'Printers'
              : activePage === 'account'
                ? 'Account'
                : activePage === 'settings'
                  ? 'Permissions'
                  : 'Connections'}</span
          >
        </div>
        <div class="flex items-center gap-3">
          <span class="connection-status"
            ><span
              class="status-dot"
              data-tone={error
                ? 'warning'
                : overview.cloud_connected
                  ? 'success'
                  : 'warning'}
            ></span>{error
              ? 'Status may be outdated'
              : overview.cloud_connected
                ? 'Cloud connected'
                : 'Cloud disconnected'}</span
          >{@render themeMenu()}
        </div>
      </header>
      <main id="main-content" class="workspace-content">
        <div class="page-heading">
          <div>
            <p class="eyebrow">
              {activePage === 'overview'
                ? 'WORKSPACE OVERVIEW'
                : 'WORKSPACE SETTINGS'}
            </p>
            <h1>{pages[activePage].title}</h1>
            <p>{pages[activePage].subtitle}</p>
          </div>
          <Button
            variant="outline"
            size="sm"
            onclick={() => refresh()}
            disabled={loading}
            >{#if loading}<Spinner data-icon="inline-start" />{:else}<RefreshCw
                data-icon="inline-start"
              />{/if}Refresh</Button
          >
        </div>
        {#if error}<Alert.Root variant="destructive"
            ><CircleAlert /><Alert.Title>Connection interrupted</Alert.Title
            ><Alert.Description>{error}</Alert.Description></Alert.Root
          >{/if}
        {#if activePage === 'overview'}<Fleet
            {overview}
            edit={editPrinter}
            {navigate}
          />
        {:else if activePage === 'account'}<Account {overview} {refresh} />
        {:else if activePage === 'settings' && settings}<Policy
            initial={settings}
            saved={(value) => (settings = value)}
          />
        {:else if activePage === 'connections'}<Connections
            {overview}
            edit={editPrinter}
            {navigate}
          />{/if}
        <footer class="workspace-footer">
          <span>Bambu Cloud LAN Proxy</span><span
            >{error
              ? 'Showing last received status'
              : 'Last synced ' + lastUpdated}</span
          >
        </footer>
      </main>
    </div>
    {#if editing}{#key editing.serial}<EndpointSheet
          printer={editing}
          saved={refresh}
          closed={closeEditor}
        />{/key}{/if}
  </Sidebar.Provider>
{/if}
