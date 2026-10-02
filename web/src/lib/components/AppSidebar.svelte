<script lang="ts">
  import {
    Boxes,
    LayoutGrid,
    Cable,
    Cloud,
    ShieldCheck,
    BookOpen,
    ArrowUpRight,
    LockKeyhole,
  } from '@lucide/svelte';
  import * as Sidebar from '$lib/components/ui/sidebar';
  import { Badge } from '$lib/components/ui/badge';
  let {
    active,
    navigate,
    count,
    connected,
    lock,
  }: {
    active: string;
    navigate: (page: string) => void;
    count: number;
    connected: boolean;
    lock: () => void;
  } = $props();
  const sidebar = Sidebar.useSidebar();
  const items = [
    { id: 'overview', label: 'Printers', icon: LayoutGrid },
    { id: 'connections', label: 'Connections', icon: Cable },
    { id: 'account', label: 'Bambu account', icon: Cloud },
    { id: 'settings', label: 'Permissions', icon: ShieldCheck },
  ];
  function go(page: string) {
    navigate(page);
    sidebar.setOpenMobile(false);
  }
</script>

<Sidebar.Root role="navigation" aria-label="Workspace navigation">
  <Sidebar.Header class="px-5 py-7">
    <a
      href="?tab=overview"
      onclick={(event) => {
        event.preventDefault();
        go('overview');
      }}
      class="brand"
    >
      <span class="brand-mark"><Boxes /></span>
      <span
        ><strong>bambu<span class="text-primary">.</span></strong><small
          >CLOUD LAN PROXY</small
        ></span
      >
    </a>
  </Sidebar.Header>
  <Sidebar.Content class="px-3">
    <Sidebar.Group>
      <Sidebar.GroupLabel>Workspace</Sidebar.GroupLabel>
      <Sidebar.Menu>
        {#each items as item (item.id)}
          <Sidebar.MenuItem>
            <Sidebar.MenuButton
              isActive={active === item.id}
              onclick={() => go(item.id)}
              class="h-11"
              aria-current={active === item.id ? 'page' : undefined}
            >
              <item.icon /><span>{item.label}</span>
            </Sidebar.MenuButton>
            {#if item.id === 'overview'}<Sidebar.MenuBadge
                >{count}</Sidebar.MenuBadge
              >{/if}
          </Sidebar.MenuItem>
        {/each}
      </Sidebar.Menu>
    </Sidebar.Group>
    <div class="sidebar-note">
      <div class="flex items-center gap-2">
        <span class="status-dot" data-tone={connected ? 'success' : 'warning'}
        ></span><strong
          >{connected ? 'Cloud, meet local.' : 'Let’s get connected.'}</strong
        >
      </div>
      <p>
        {connected
          ? 'Your favorite local apps. Your Bambu account. Working together.'
          : 'Connect your Bambu account to bring your printers into one place.'}
      </p>
      <Badge variant="outline">Self-hosted · Your workspace</Badge>
    </div>
  </Sidebar.Content>
  <Sidebar.Footer class="gap-4 p-5">
    <Sidebar.Menu
      ><Sidebar.MenuItem
        ><Sidebar.MenuButton>
          {#snippet child({ props })}<a
              {...props}
              href="https://github.com/AurelioB/bambu-cloud-lan-proxy#readme"
              target="_blank"
              rel="noreferrer"
              ><BookOpen /><span>Setup guide</span><ArrowUpRight /></a
            >{/snippet}
        </Sidebar.MenuButton></Sidebar.MenuItem
      >
      <Sidebar.MenuItem
        ><Sidebar.MenuButton onclick={lock}
          ><LockKeyhole /><span>Sign out</span></Sidebar.MenuButton
        ></Sidebar.MenuItem
      ></Sidebar.Menu
    >
    <p class="text-xs text-muted-foreground">
      Made for your entire printer collection.
    </p>
  </Sidebar.Footer>
</Sidebar.Root>
