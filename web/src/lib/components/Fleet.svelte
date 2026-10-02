<script lang="ts">
  import {
    Search,
    LayoutGrid,
    List,
    ArrowUpRight,
    ArrowRight,
    Cloud,
    Network,
    Printer as PrinterIcon,
    ShieldCheck,
    Video,
    CircleHelp,
    Thermometer,
    Layers,
    Copy,
    Cable,
  } from '@lucide/svelte';
  import * as Card from '$lib/components/ui/card';
  import * as InputGroup from '$lib/components/ui/input-group';
  import * as ToggleGroup from '$lib/components/ui/toggle-group';
  import * as Empty from '$lib/components/ui/empty';
  import * as Table from '$lib/components/ui/table';
  import { Badge } from '$lib/components/ui/badge';
  import { Button } from '$lib/components/ui/button';
  import { Separator } from '$lib/components/ui/separator';
  import PrinterArtwork from './PrinterArtwork.svelte';
  import type { Overview, Printer } from '$lib/api';
  import {
    cameraLabel,
    copyText,
    endpointLabel,
    needsAttention,
    printerState,
    toolheadLabel,
  } from '$lib/presentation';

  let {
    overview,
    edit,
    navigate,
  }: {
    overview: Overview;
    edit: (printer: Printer) => void;
    navigate: (page: string) => void;
  } = $props();
  let query = $state('');
  let filter = $state('all');
  let view = $state('grid');
  let online = $derived(overview.printers.filter((p) => p.online).length);
  let ready = $derived(
    overview.printers.filter(
      (p) =>
        p.endpoint_enabled &&
        p.virtual_ip &&
        p.endpoint_health.state === 'healthy',
    ).length,
  );
  let attention = $derived(overview.printers.filter(needsAttention).length);
  let filtered = $derived(
    overview.printers.filter(
      (p) =>
        `${p.name} ${p.model} ${p.serial} ${p.virtual_ip}`
          .toLowerCase()
          .includes(query.toLowerCase().trim()) &&
        (filter === 'online'
          ? p.online
          : filter === 'attention'
            ? needsAttention(p)
            : true),
    ),
  );
</script>

<section class="welcome-panel" aria-label="Connection overview">
  <div class="welcome-copy">
    <p class="eyebrow">A BRIDGE TO YOUR WORKSPACE</p>
    <h2>Your printers,<br />connected<span class="text-primary">.</span></h2>
    <p>
      Bambu’s cloud and your local apps.<br />A shared home for every printer.
    </p>
    <Button variant="outline" size="sm" onclick={() => navigate('connections')}
      >Connection guide<ArrowUpRight data-icon="inline-end" /></Button
    >
  </div>
  <div
    class="connection-map"
    aria-label="Local apps connect through the proxy to Bambu Cloud and your printers"
  >
    <div class="route-node">
      <span class="route-icon"><Network /></span><strong>Your apps</strong
      ><small>Bambuddy · Studio</small>
    </div>
    <span class="route-line"></span>
    <div class="route-node route-hub">
      <span class="route-icon"><Cable /></span><strong>LAN Proxy</strong><Badge
        variant={overview.healthy ? 'success' : 'warning'}
        >{overview.healthy ? 'Running' : 'Check proxy'}</Badge
      >
    </div>
    <span class="route-line"></span>
    <div class="route-node">
      <span class="route-icon"><Cloud /></span><strong>Bambu Cloud</strong
      ><small>{overview.cloud_connected ? 'Connected' : 'Disconnected'}</small>
    </div>
  </div>
</section>

<section class="fleet-stats" aria-label="Printer summary">
  <div>
    <span class="stat-icon"><PrinterIcon /></span>
    <div>
      <p>Printers online</p>
      <strong>{online}<small> / {overview.printers.length}</small></strong>
    </div>
  </div>
  <div>
    <span class="stat-icon"><Network /></span>
    <div>
      <p>Local endpoints ready</p>
      <strong>{ready}<small> available to your apps</small></strong>
    </div>
  </div>
  <div>
    <span class="stat-icon" data-tone={attention ? 'warning' : 'success'}
      ><ShieldCheck /></span
    >
    <div>
      <p>Needs attention</p>
      <strong
        >{attention}<small>{attention ? ' to review' : ' looking good'}</small
        ></strong
      >
    </div>
  </div>
</section>

<section class="flex flex-col gap-5" aria-labelledby="fleet-title">
  <div class="section-heading">
    <div>
      <h2 id="fleet-title">
        Your printers <span class="text-muted-foreground"
          >({overview.printers.length})</span
        >
      </h2>
      <p>Connection health and capabilities, at a glance.</p>
    </div>
    <Button variant="outline" size="sm" onclick={() => navigate('account')}
      ><Cloud data-icon="inline-start" />Manage account</Button
    >
  </div>
  <div class="fleet-toolbar">
    <div class="w-full sm:max-w-72">
      <InputGroup.Root
        ><InputGroup.Input
          aria-label="Search printers"
          placeholder="Find a printer…"
          bind:value={query}
        /><InputGroup.Addon><Search /></InputGroup.Addon></InputGroup.Root
      >
    </div>
    <ToggleGroup.Root
      type="single"
      value={filter}
      onValueChange={(value) => {
        if (value) filter = value;
      }}
      size="sm"
      aria-label="Filter printers"
    >
      <ToggleGroup.Item value="all">All printers</ToggleGroup.Item
      ><ToggleGroup.Item value="online">Online</ToggleGroup.Item
      ><ToggleGroup.Item value="attention">Needs attention</ToggleGroup.Item>
    </ToggleGroup.Root>
    <ToggleGroup.Root
      class="sm:ml-auto"
      type="single"
      value={view}
      onValueChange={(value) => {
        if (value) view = value;
      }}
      variant="outline"
      size="sm"
      aria-label="Printer layout"
      ><ToggleGroup.Item value="grid" aria-label="Card view"
        ><LayoutGrid /></ToggleGroup.Item
      ><ToggleGroup.Item value="list" aria-label="List view"
        ><List /></ToggleGroup.Item
      ></ToggleGroup.Root
    >
  </div>
  {#if filtered.length === 0}
    <Empty.Root class="min-h-72 border border-dashed"
      ><Empty.Header
        ><Empty.Media variant="icon"><PrinterIcon /></Empty.Media><Empty.Title
          >{overview.printers.length
            ? 'No matching printers'
            : 'Your workspace starts here'}</Empty.Title
        ><Empty.Description
          >{overview.printers.length
            ? 'Try another name, model, or filter.'
            : 'Connect your Bambu account. Its printers will appear here automatically.'}</Empty.Description
        ></Empty.Header
      ><Empty.Content
        ><Button
          variant="outline"
          onclick={() => {
            if (overview.printers.length) {
              query = '';
              filter = 'all';
            } else navigate('account');
          }}
          >{overview.printers.length
            ? 'Clear filters'
            : 'Connect Bambu account'}</Button
        ></Empty.Content
      ></Empty.Root
    >
  {:else if view === 'list'}
    <div class="table-panel">
      <Table.Root
        ><Table.Caption
          >Printer state and local endpoint health are reported separately.</Table.Caption
        ><Table.Header
          ><Table.Row
            ><Table.Head>Printer</Table.Head><Table.Head>Status</Table.Head
            ><Table.Head>Local address</Table.Head><Table.Head
              >Endpoint</Table.Head
            ><Table.Head>Camera</Table.Head><Table.Head
              ><span class="sr-only">Actions</span></Table.Head
            ></Table.Row
          ></Table.Header
        ><Table.Body
          >{#each filtered as p (p.serial)}<Table.Row
              ><Table.Cell
                ><strong>{p.name || 'Bambu Lab printer'}</strong>
                <p class="text-xs text-muted-foreground">
                  {p.model || 'Unknown model'}
                </p></Table.Cell
              ><Table.Cell
                ><Badge variant={p.online ? 'success' : 'secondary'}
                  >{printerState(p)}</Badge
                ></Table.Cell
              ><Table.Cell><code>{p.virtual_ip || 'Not set'}</code></Table.Cell
              ><Table.Cell>{endpointLabel(p)}</Table.Cell><Table.Cell
                >{cameraLabel(p)}</Table.Cell
              ><Table.Cell
                ><Button
                  variant="ghost"
                  size="sm"
                  aria-label={`Configure ${p.name}`}
                  onclick={() => edit(p)}
                  >Configure<ArrowUpRight data-icon="inline-end" /></Button
                ></Table.Cell
              ></Table.Row
            >{/each}</Table.Body
        ></Table.Root
      >
    </div>
  {:else}
    <div class="printer-grid">
      {#each filtered as p (p.serial)}
        <Card.Root class="printer-card" data-testid="printer-card">
          <Card.Header
            ><Card.Title>{p.name || 'Bambu Lab printer'}</Card.Title
            ><Card.Description
              >{p.model || 'Model unavailable'}</Card.Description
            ><Card.Action
              ><Badge variant={p.online ? 'success' : 'secondary'}
                ><span
                  class="status-dot"
                  data-tone={p.online ? 'success' : 'muted'}
                ></span>{printerState(p)}</Badge
              ></Card.Action
            ></Card.Header
          >
          <Card.Content class="flex flex-col gap-4">
            <div class="printer-stage">
              <span class="stage-label">{p.model || 'BAMBU LAB'}</span
              ><PrinterArtwork dual={p.capabilities.dual_nozzle} /><span
                class="stage-caption">{toolheadLabel(p)}</span
              >
            </div>
            <div class="endpoint-summary">
              <div>
                <p>LOCAL ADDRESS</p>
                <code>{p.virtual_ip || 'Not configured'}</code>
              </div>
              {#if p.virtual_ip}<Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={`Copy address for ${p.name}`}
                  onclick={() => copyText(p.virtual_ip)}><Copy /></Button
                >{/if}
            </div>
            <Separator />
            <dl class="service-list">
              <div>
                <dt><Network />Local endpoint</dt>
                <dd
                  data-tone={p.endpoint_health.state === 'healthy'
                    ? 'success'
                    : 'warning'}
                >
                  {endpointLabel(p)}
                </dd>
              </div>
              <div>
                <dt><ShieldCheck />Cloud signing</dt>
                <dd>
                  {p.secure_control
                    ? 'Certificate ready'
                    : 'Certificate needed'}
                </dd>
              </div>
              <div>
                <dt><Video />Camera</dt>
                <dd>{cameraLabel(p)}</dd>
              </div>
              {#if p.capabilities.runtime?.ams_units != null}
                <div>
                  <dt><Layers />Attached AMS</dt>
                  <dd>
                    {p.capabilities.runtime.ams_units}
                    {p.capabilities.runtime.ams_units === 1
                      ? 'unit'
                      : 'units'}{p.capabilities.runtime.filament_slots != null
                      ? ` · ${p.capabilities.runtime.filament_slots} slots`
                      : ''}
                  </dd>
                </div>
              {/if}
              {#if p.capabilities.source === 'runtime_telemetry'}
                <div>
                  <dt>Capabilities</dt>
                  <dd><Badge variant="outline">Reported by printer</Badge></dd>
                </div>
              {/if}
            </dl>
          </Card.Content>
          <Card.Footer class="justify-between gap-2"
            ><div class="capability-icons" aria-label="Printer capabilities">
              {#if toolheadLabel(p) === 'Capabilities unknown'}<CircleHelp
                /><span>Unknown model</span>{:else}<Layers /><span
                  >{toolheadLabel(p)}</span
                >{#if p.capabilities.chamber_temperature}<span
                    title="Chamber temperature sensor"><Thermometer /></span
                  >{/if}{/if}
            </div>
            <Button
              variant="ghost"
              size="sm"
              aria-label={`Configure ${p.name}`}
              onclick={() => edit(p)}
              >Configure<ArrowRight data-icon="inline-end" /></Button
            ></Card.Footer
          >
        </Card.Root>
      {/each}
    </div>
  {/if}
  <p class="fleet-footnote">
    <span class="status-dot" data-tone="muted"></span>Status refreshes every 15
    seconds. Fresh telemetry supplies observed capabilities; model profiles fill
    gaps. Live camera and print controls are available in your connected apps.
  </p>
</section>
