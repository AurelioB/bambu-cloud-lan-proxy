<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import { Cable, Copy, Info } from '@lucide/svelte';
  import * as Sheet from '$lib/components/ui/sheet';
  import * as Field from '$lib/components/ui/field';
  import * as Alert from '$lib/components/ui/alert';
  import { Input } from '$lib/components/ui/input';
  import { Button } from '$lib/components/ui/button';
  import { Badge } from '$lib/components/ui/badge';
  import { Switch } from '$lib/components/ui/switch';
  import { Spinner } from '$lib/components/ui/spinner';
  import { Separator } from '$lib/components/ui/separator';
  import { api, type Printer } from '$lib/api';
  import { cameraLabel, copyText, endpointLabel } from '$lib/presentation';
  import { toast } from 'svelte-sonner';

  let {
    printer,
    saved,
    closed,
  }: {
    printer: Printer;
    saved: () => Promise<void>;
    closed: () => void;
  } = $props();
  // The parent keys this editor by printer serial; edits are a deliberate snapshot.
  let draft = $state(
    untrack(() => ({
      virtual_ip: printer.virtual_ip,
      physical_ip: printer.physical_ip,
      physical_access_code: '',
      enabled: printer.endpoint_enabled,
    })),
  );
  let open = $state(true);
  let saving = $state(false);
  let error = $state('');
  let disposed = false;
  onDestroy(() => {
    disposed = true;
  });
  const validIp = (value: string) =>
    /^(\d{1,3}\.){3}\d{1,3}$/.test(value) &&
    value.split('.').every((part) => Number(part) <= 255);
  let invalidIp = $derived(
    Boolean(draft.virtual_ip && !validIp(draft.virtual_ip)),
  );
  let invalidPhysicalIp = $derived(
    Boolean(draft.physical_ip && !validIp(draft.physical_ip)),
  );
  let invalidCode = $derived(
    Boolean(
      draft.physical_access_code && draft.physical_access_code.length !== 8,
    ),
  );
  async function save() {
    if (
      invalidIp ||
      invalidPhysicalIp ||
      invalidCode ||
      (draft.enabled && !draft.virtual_ip)
    )
      return;
    saving = true;
    error = '';
    try {
      await api(
        `/api/admin/printers/${encodeURIComponent(printer.serial)}/endpoint`,
        { method: 'PUT', body: JSON.stringify(draft) },
      );
      if (disposed) return;
      draft.physical_access_code = '';
      toast.success('Printer connection saved', {
        description:
          'Address changes need networking applied on the proxy host.',
      });
      await saved();
      if (disposed) return;
      open = false;
      closed();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Could not save connection.';
    } finally {
      saving = false;
    }
  }
</script>

<Sheet.Root
  bind:open
  onOpenChange={(value) => {
    if (!value) closed();
  }}
>
  <Sheet.Content
    onCloseAutoFocus={(event) => event.preventDefault()}
    class="overflow-y-auto data-[side=right]:w-full data-[side=right]:sm:max-w-lg"
  >
    <Sheet.Header class="px-6 pt-8"
      ><span class="section-icon"><Cable /></span><Sheet.Title
        >{printer.name || 'Printer connection'}</Sheet.Title
      ><Sheet.Description
        >{printer.model || 'Bambu Lab printer'} · Connection settings</Sheet.Description
      ></Sheet.Header
    >
    <form
      onsubmit={(event) => {
        event.preventDefault();
        void save();
      }}
      class="flex flex-1 flex-col gap-6 px-6 pb-6"
    >
      <div class="flex flex-wrap gap-2">
        <Badge
          variant={printer.endpoint_health.state === 'healthy'
            ? 'success'
            : 'secondary'}>{endpointLabel(printer)}</Badge
        ><Badge variant="outline">{cameraLabel(printer)}</Badge>
      </div>
      <div class="flex items-center justify-between gap-3">
        <div class="min-w-0">
          <p class="text-xs text-muted-foreground">Printer serial</p>
          <code class="break-all text-sm">{printer.serial}</code>
        </div>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Copy printer serial"
          onclick={() => copyText(printer.serial)}><Copy /></Button
        >
      </div>
      <Separator />
      <Field.Group>
        {#if printer.capabilities.runtime}
          <dl
            class="flex flex-col gap-3 text-sm"
            aria-label="Observed printer capabilities"
          >
            <div class="flex flex-wrap justify-between gap-2">
              <dt class="text-muted-foreground">Firmware</dt>
              <dd>{printer.capabilities.runtime.firmware ?? 'Not reported'}</dd>
            </div>
            <div class="flex flex-wrap justify-between gap-2">
              <dt class="text-muted-foreground">Attached AMS</dt>
              <dd>
                {printer.capabilities.runtime.ams_units ?? 'Not reported'}
              </dd>
            </div>
            <div class="flex flex-wrap justify-between gap-2">
              <dt class="text-muted-foreground">Filament slots</dt>
              <dd>
                {printer.capabilities.runtime.filament_slots ?? 'Not reported'}
              </dd>
            </div>
          </dl>
          {#if printer.capabilities.runtime.sensors.length}
            <div class="flex flex-wrap gap-2" aria-label="Reported sensors">
              {#each printer.capabilities.runtime.sensors as sensor (sensor)}<Badge
                  variant="secondary">{sensor.replaceAll('_', ' ')}</Badge
                >{/each}
            </div>
          {/if}
          {#if Object.keys(printer.capabilities.runtime.firmware_controls).length}
            <dl
              class="flex flex-col gap-2 text-sm"
              aria-label="Firmware-reported functions"
            >
              {#each Object.entries(printer.capabilities.runtime.firmware_controls) as [name, supported] (name)}<div
                  class="flex flex-wrap justify-between gap-2"
                >
                  <dt class="text-muted-foreground">
                    {name.replaceAll('_', ' ')}
                  </dt>
                  <dd>
                    {supported ? 'Reported available' : 'Reported unavailable'}
                  </dd>
                </div>{/each}
            </dl>
            <p class="text-xs text-muted-foreground">
              Reported firmware functions do not change your proxy permissions
              or guarantee command success.
            </p>
          {/if}
          <Separator />
        {/if}
        <Field.Field orientation="horizontal"
          ><Field.Content
            ><Field.Label for="endpoint-enabled"
              >Make available to local apps</Field.Label
            ><Field.Description
              >This printer gets its own local address.</Field.Description
            ></Field.Content
          ><Switch
            id="endpoint-enabled"
            bind:checked={draft.enabled}
            disabled={saving}
          /></Field.Field
        >
        <Field.Field data-invalid={invalidIp}
          ><Field.Label for="virtual-ip">Virtual printer address</Field.Label
          ><Input
            id="virtual-ip"
            bind:value={draft.virtual_ip}
            placeholder="192.168.1.201"
            aria-invalid={invalidIp}
            required={draft.enabled}
            disabled={saving}
          /><Field.Description
            >{invalidIp
              ? 'Enter a valid IPv4 address.'
              : 'Use a reserved address outside your router’s DHCP pool. Enter this address in Bambuddy or Studio.'}</Field.Description
          ></Field.Field
        >
        <Field.Field data-invalid={invalidPhysicalIp}
          ><Field.Label for="physical-ip">Physical printer address</Field.Label
          ><Input
            id="physical-ip"
            bind:value={draft.physical_ip}
            placeholder="192.168.1.50"
            aria-invalid={invalidPhysicalIp}
            disabled={saving}
          /><Field.Description
            >{invalidPhysicalIp
              ? 'Enter a valid IPv4 address.'
              : 'Optional for cloud status. Used for local telemetry, files, and supported cameras.'}</Field.Description
          ></Field.Field
        >
        <Field.Field data-invalid={invalidCode}
          ><Field.Label for="physical-code">Printer LAN access code</Field.Label
          ><Input
            id="physical-code"
            type="password"
            bind:value={draft.physical_access_code}
            placeholder={printer.physical_access_configured
              ? 'Configured — leave blank to keep'
              : '8-character access code'}
            autocomplete="new-password"
            aria-invalid={invalidCode}
            disabled={saving}
          /><Field.Description
            >{invalidCode
              ? 'The printer access code must have 8 characters.'
              : 'Find this on the printer. Saved codes are never displayed.'}</Field.Description
          ></Field.Field
        >
      </Field.Group>
      <Alert.Root
        ><Info /><Alert.Title>Changing addresses?</Alert.Title
        ><Alert.Description
          >After saving, run <code>./scripts/apply-endpoints.sh</code> on the proxy
          host to apply addresses and certificates. Then check that the endpoint is
          connected.</Alert.Description
        ></Alert.Root
      >
      {#if error}<Alert.Root variant="destructive"
          ><Alert.Title>Connection was not saved</Alert.Title><Alert.Description
            >{error}</Alert.Description
          ></Alert.Root
        >{/if}
      <Sheet.Footer class="mt-auto px-0"
        ><Button
          type="submit"
          disabled={saving ||
            invalidIp ||
            invalidPhysicalIp ||
            invalidCode ||
            (draft.enabled && !draft.virtual_ip)}
          >{#if saving}<Spinner data-icon="inline-start" />{/if}Save connection</Button
        ><Button
          variant="outline"
          disabled={saving}
          onclick={() => {
            open = false;
            closed();
          }}>Cancel</Button
        ></Sheet.Footer
      >
    </form>
  </Sheet.Content>
</Sheet.Root>
