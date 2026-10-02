<script lang="ts">
  import {
    Cable,
    Download,
    ArrowUpRight,
    Copy,
    ShieldCheck,
    Network,
    Folder,
    Video,
    Radio,
  } from '@lucide/svelte';
  import * as Card from '$lib/components/ui/card';
  import * as Table from '$lib/components/ui/table';
  import * as Accordion from '$lib/components/ui/accordion';
  import * as Empty from '$lib/components/ui/empty';
  import { Button } from '$lib/components/ui/button';
  import { Badge } from '$lib/components/ui/badge';
  import { apiRequest, type Overview, type Printer } from '$lib/api';
  import { copyText, endpointLabel } from '$lib/presentation';
  import { toast } from 'svelte-sonner';
  let {
    overview,
    edit,
    navigate,
  }: {
    overview: Overview;
    edit: (p: Printer) => void;
    navigate: (page: string) => void;
  } = $props();
  const services = [
    {
      name: 'Printer status & controls',
      protocol: 'MQTT over TLS',
      port: '8883',
      icon: Radio,
    },
    {
      name: 'File transfer',
      protocol: 'Implicit FTPS',
      port: '990',
      icon: Folder,
    },
    {
      name: 'Stored jobs & timelapses',
      protocol: 'Native files',
      port: '6000',
      icon: Network,
    },
    {
      name: 'Live camera',
      protocol: 'RTSPS / image · supported models',
      port: '322 / 6000',
      icon: Video,
    },
  ];
  let downloading = $state(false);
  async function downloadCa() {
    downloading = true;
    try {
      const response = await apiRequest('/api/admin/ca');
      if (!response.ok)
        throw new Error('The proxy certificate is unavailable.');
      const url = URL.createObjectURL(await response.blob());
      const link = document.createElement('a');
      link.href = url;
      link.download = 'bambu-cloud-lan-proxy-ca.crt';
      document.body.append(link);
      link.click();
      link.remove();
      window.setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch (e) {
      toast.error(
        e instanceof Error ? e.message : 'Could not download certificate.',
      );
    } finally {
      downloading = false;
    }
  }
</script>

<section class="guide-banner">
  <span class="section-icon"><Cable /></span>
  <div>
    <h2>A familiar connection. For every printer.</h2>
    <p>
      In Bambuddy or Studio, add a LAN printer using its virtual address, serial
      number, and your proxy access code.
    </p>
  </div>
</section>
<Card.Root
  ><Card.Header
    ><Card.Title>Local printer addresses</Card.Title><Card.Description
      >Each printer has its own address. Standard ports stay the same.</Card.Description
    ></Card.Header
  ><Card.Content>
    {#if overview.printers.length}<Table.Root
        ><Table.Header
          ><Table.Row
            ><Table.Head>Printer</Table.Head><Table.Head
              >Virtual address</Table.Head
            ><Table.Head>Endpoint</Table.Head><Table.Head
              ><span class="sr-only">Actions</span></Table.Head
            ></Table.Row
          ></Table.Header
        ><Table.Body
          >{#each overview.printers as p (p.serial)}<Table.Row
              ><Table.Cell
                ><strong>{p.name || 'Bambu Lab printer'}</strong>
                <p class="text-xs text-muted-foreground">
                  {p.model || 'Model unknown'}
                </p></Table.Cell
              ><Table.Cell
                ><div class="flex items-center gap-2">
                  <code>{p.virtual_ip || 'Not set'}</code
                  >{#if p.virtual_ip}<Button
                      size="icon-sm"
                      variant="ghost"
                      aria-label={`Copy address for ${p.name}`}
                      onclick={() => copyText(p.virtual_ip)}><Copy /></Button
                    >{/if}
                </div></Table.Cell
              ><Table.Cell
                ><Badge
                  variant={p.endpoint_health.state === 'healthy'
                    ? 'success'
                    : 'secondary'}>{endpointLabel(p)}</Badge
                ></Table.Cell
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
      >{:else}<Empty.Root
        ><Empty.Header
          ><Empty.Media variant="icon"><Network /></Empty.Media><Empty.Title
            >No printers to connect</Empty.Title
          ><Empty.Description
            >Sign in to your Bambu account to discover your printers.</Empty.Description
          ></Empty.Header
        ><Empty.Content
          ><Button onclick={() => navigate('account')}>Connect account</Button
          ></Empty.Content
        ></Empty.Root
      >{/if}
  </Card.Content></Card.Root
>
<div class="grid gap-6 lg:grid-cols-2">
  <Card.Root
    ><Card.Header
      ><Card.Title>Services at each address</Card.Title><Card.Description
        >Use these ports when your app asks for connection details.</Card.Description
      ></Card.Header
    ><Card.Content
      ><dl class="port-list">
        {#each services as service (service.port)}<div>
            <dt>
              <span class="stat-icon"><service.icon /></span>
              <span
                ><strong>{service.name}</strong><small>{service.protocol}</small
                ></span
              >
            </dt>
            <dd><code>{service.port}</code></dd>
          </div>{/each}
      </dl></Card.Content
    ></Card.Root
  >
  <Card.Root
    ><Card.Header
      ><Card.Title>Build trust with your apps</Card.Title><Card.Description
        >Some clients need the public proxy certificate to verify a secure
        connection.</Card.Description
      ><Card.Action><ShieldCheck /></Card.Action></Card.Header
    ><Card.Content class="flex flex-col gap-5"
      ><p class="text-sm text-muted-foreground">
        Import the proxy CA into clients that validate LAN certificates. This
        download contains only the public certificate.
      </p>
      <Accordion.Root type="single"
        ><Accordion.Item value="camera"
          ><Accordion.Trigger>Why is my camera unavailable?</Accordion.Trigger
          ><Accordion.Content
            >Camera access depends on the printer model and local connection.
            Camera relays need the physical printer address, LAN access code,
            and local liveview enabled. Image cameras share port 6000 with
            printer files. RTSPS cameras use port 322. Availability depends on
            model and firmware.</Accordion.Content
          ></Accordion.Item
        ><Accordion.Item value="ip"
          ><Accordion.Trigger
            >What happens after an address change?</Accordion.Trigger
          ><Accordion.Content
            >Save the printer connection, then run <code
              >./scripts/apply-endpoints.sh</code
            > on the host to update its addresses and certificates. Check endpoint
            health here afterward.</Accordion.Content
          ></Accordion.Item
        ><Accordion.Item value="cloud"
          ><Accordion.Trigger
            >Do all printers share a cloud session?</Accordion.Trigger
          ><Accordion.Content
            >Yes. One Bambu account session serves the printers in this
            workspace. Each printer has an independent local address and its own
            connection settings.</Accordion.Content
          ></Accordion.Item
        ></Accordion.Root
      ></Card.Content
    ><Card.Footer
      ><Button variant="outline" onclick={downloadCa} disabled={downloading}
        ><Download data-icon="inline-start" />{downloading
          ? 'Downloading…'
          : 'Download proxy certificate'}</Button
      ></Card.Footer
    ></Card.Root
  >
</div>
