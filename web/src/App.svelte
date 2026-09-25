<script lang="ts">
  import ActivityIcon from "@lucide/svelte/icons/activity";
  import CloudIcon from "@lucide/svelte/icons/cloud";
  import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
  import FolderIcon from "@lucide/svelte/icons/folder";
  import LogInIcon from "@lucide/svelte/icons/log-in";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
  import VideoIcon from "@lucide/svelte/icons/video";
  import * as Alert from "$lib/components/ui/alert";
  import { Badge } from "$lib/components/ui/badge";
  import { Button } from "$lib/components/ui/button";
  import * as Card from "$lib/components/ui/card";
  import * as Field from "$lib/components/ui/field";
  import { Input } from "$lib/components/ui/input";
  import { Separator } from "$lib/components/ui/separator";
  import { Spinner } from "$lib/components/ui/spinner";
  import { Switch } from "$lib/components/ui/switch";
  import * as Tabs from "$lib/components/ui/tabs";
  import * as ToggleGroup from "$lib/components/ui/toggle-group";
  import { Toaster } from "$lib/components/ui/sonner";
  import { toast } from "svelte-sonner";
  import { onMount } from "svelte";
  import { api, type Overview, type Settings } from "$lib/api";

  let accessCode = $state(sessionStorage.getItem("proxy-access-code") ?? "");
  let callbackUrl = $state("");
  let overview = $state<Overview | null>(null);
  let settings = $state<Settings | null>(null);
  let loading = $state(false);
  let authenticated = $state(false);

  async function refresh() {
    if (!accessCode) return;
    loading = true;
    try {
      overview = await api<Overview>("/api/admin/overview", accessCode);
      settings = await api<Settings>("/api/admin/settings", accessCode);
      authenticated = true;
      sessionStorage.setItem("proxy-access-code", accessCode);
    } catch {
      authenticated = false;
      overview = null;
    } finally { loading = false; }
  }

  async function beginLogin() {
    try {
      const response = await api<{ url: string }>("/api/cloud/auth/url", accessCode);
      window.open(response.url, "_blank", "noopener,noreferrer");
    } catch (error) { toast.error(error instanceof Error ? error.message : "Could not start sign-in"); }
  }

  async function finishLogin() {
    loading = true;
    try {
      await api("/api/cloud/auth/paste", accessCode, { method: "POST", body: JSON.stringify({ pasted_url: callbackUrl }) });
      callbackUrl = "";
      toast.success("Bambu account connected");
      await refresh();
    } catch (error) { toast.error(error instanceof Error ? error.message : "Sign-in failed"); }
    finally { loading = false; }
  }

  async function saveSettings() {
    if (!settings) return;
    loading = true;
    try {
      settings = await api<Settings>("/api/admin/settings", accessCode, { method: "PUT", body: JSON.stringify(settings) });
      toast.success("Settings saved");
      await refresh();
    } catch (error) { toast.error(error instanceof Error ? error.message : "Settings were not saved"); }
    finally { loading = false; }
  }

  async function downloadCa() {
    try {
      const response = await fetch("/api/admin/ca", { headers: { "X-Cloud-Lan-Proxy-Key": accessCode } });
      if (!response.ok) throw new Error((await response.text()) || "Proxy CA is unavailable");
      const url = URL.createObjectURL(await response.blob());
      const link = document.createElement("a");
      link.href = url; link.download = "bambu-cloud-lan-proxy-ca.crt"; link.click();
      URL.revokeObjectURL(url);
    } catch (error) { toast.error(error instanceof Error ? error.message : "Could not export CA"); }
  }

  onMount(() => {
    void refresh();
    const timer = window.setInterval(() => authenticated && void refresh(), 15_000);
    return () => window.clearInterval(timer);
  });
</script>

<Toaster richColors />
<main class="mx-auto flex min-h-screen max-w-6xl flex-col gap-6 px-4 py-8 md:px-8">
  <header class="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
    <div class="flex flex-col gap-1">
      <p class="text-sm font-medium text-muted-foreground">H2C local control plane</p>
      <h1 class="text-2xl font-semibold tracking-tight">Bambu Cloud LAN Proxy</h1>
    </div>
    {#if authenticated}
      <div class="flex items-center gap-2">
        <Badge variant={overview?.healthy ? "secondary" : "destructive"}>{overview?.healthy ? "Proxy healthy" : "Needs attention"}</Badge>
        <Button variant="outline" size="sm" onclick={refresh} disabled={loading}>
          {#if loading}<Spinner data-icon="inline-start" />{:else}<RefreshCwIcon data-icon="inline-start" />{/if}Refresh
        </Button>
      </div>
    {/if}
  </header>

  {#if !authenticated}
    <Card.Root class="mx-auto mt-12 w-full max-w-md">
      <Card.Header><Card.Title>Open the admin console</Card.Title><Card.Description>Use the proxy access code created during setup. It stays in this browser tab.</Card.Description></Card.Header>
      <Card.Content>
        <Field.Group><Field.Field><Field.Label for="access-code">Proxy access code</Field.Label><Input id="access-code" type="password" bind:value={accessCode} autocomplete="current-password" /></Field.Field></Field.Group>
      </Card.Content>
      <Card.Footer><Button class="w-full" onclick={refresh} disabled={loading || !accessCode}>{#if loading}<Spinner data-icon="inline-start" />{:else}<LogInIcon data-icon="inline-start" />{/if}Continue</Button></Card.Footer>
    </Card.Root>
  {:else}
    <Tabs.Root value="overview" class="flex flex-col gap-5">
      <Tabs.List>
        <Tabs.Trigger value="overview">Overview</Tabs.Trigger><Tabs.Trigger value="account">Bambu account</Tabs.Trigger><Tabs.Trigger value="settings">Control policy</Tabs.Trigger><Tabs.Trigger value="connections">Connections</Tabs.Trigger>
      </Tabs.List>

      <Tabs.Content value="overview" class="flex flex-col gap-5">
        <section class="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
          <Card.Root size="sm"><Card.Header><Card.Title>Cloud session</Card.Title><Card.Action><CloudIcon /></Card.Action></Card.Header><Card.Content><Badge variant={overview?.cloud_connected ? "secondary" : "outline"}>{overview?.cloud_connected ? "Connected" : "Disconnected"}</Badge></Card.Content></Card.Root>
          <Card.Root size="sm"><Card.Header><Card.Title>Account</Card.Title><Card.Action><ShieldCheckIcon /></Card.Action></Card.Header><Card.Content><Badge variant={overview?.signed_in ? "secondary" : "outline"}>{overview?.signed_in ? "Signed in" : "Not signed in"}</Badge></Card.Content></Card.Root>
          <Card.Root size="sm"><Card.Header><Card.Title>File services</Card.Title><Card.Action><FolderIcon /></Card.Action></Card.Header><Card.Content><Badge variant="secondary">Configured</Badge></Card.Content></Card.Root>
          <Card.Root size="sm"><Card.Header><Card.Title>Local camera</Card.Title><Card.Action><VideoIcon /></Card.Action></Card.Header><Card.Content><Badge variant="outline">LAN dependent</Badge></Card.Content></Card.Root>
        </section>
        <Card.Root>
          <Card.Header><Card.Title>Printers</Card.Title><Card.Description>Account-bound printers currently exposed to local clients.</Card.Description></Card.Header>
          <Card.Content>
            {#if overview?.printers.length}
              <div class="flex flex-col gap-3">
                {#each overview.printers as printer (printer.serial)}
                  <div class="flex flex-col gap-2 md:flex-row md:items-center md:justify-between">
                    <div class="flex flex-col gap-1"><span class="font-medium">{printer.name || printer.serial}</span><span class="font-mono text-xs text-muted-foreground">{printer.serial}</span></div>
                    <div class="flex flex-wrap gap-2"><Badge variant={printer.online ? "secondary" : "outline"}>{printer.online ? "Online" : "Offline"}</Badge><Badge variant="outline">{printer.state}</Badge><Badge variant={printer.secure_control ? "secondary" : "outline"}>{printer.secure_control ? "Control ready" : "Awaiting certificate"}</Badge></div>
                  </div><Separator />
                {/each}
              </div>
            {:else}
              <Alert.Root><ActivityIcon /><Alert.Title>No printer discovered yet</Alert.Title><Alert.Description>Connect the Bambu account, then allow the cloud session a few seconds to refresh.</Alert.Description></Alert.Root>
            {/if}
          </Card.Content>
        </Card.Root>
      </Tabs.Content>

      <Tabs.Content value="account">
        <Card.Root class="max-w-2xl">
          <Card.Header><Card.Title>Bambu account session</Card.Title><Card.Description>The official networking plugin keeps the session and signing material. The proxy never displays those credentials.</Card.Description></Card.Header>
          <Card.Content>
            <Field.Group>
              <Field.Field><Button variant="outline" onclick={beginLogin}><ExternalLinkIcon data-icon="inline-start" />Open Bambu sign-in</Button><Field.Description>After signing in, the browser may show an unreachable localhost page. Copy its complete URL.</Field.Description></Field.Field>
              <Field.Field><Field.Label for="callback-url">Browser callback URL</Field.Label><Input id="callback-url" bind:value={callbackUrl} placeholder="http://localhost:13618/?ticket=…" /></Field.Field>
            </Field.Group>
          </Card.Content>
          <Card.Footer><Button onclick={finishLogin} disabled={loading || !callbackUrl}>Complete sign-in</Button></Card.Footer>
        </Card.Root>
      </Tabs.Content>

      <Tabs.Content value="settings">
        <Card.Root class="max-w-3xl">
          <Card.Header><Card.Title>Control policy</Card.Title><Card.Description>Changes apply to new local requests without replaying anything already sent.</Card.Description></Card.Header>
          <Card.Content>
            {#if settings}
              <Field.Group>
                <Field.Set><Field.Legend>Cloud command level</Field.Legend><Field.Description>Read queries always remain available. Full control passes valid commands to the official signing plugin.</Field.Description><ToggleGroup.Root type="single" variant="outline" spacing={2} bind:value={settings.mode}><ToggleGroup.Item value="read_only">Read only</ToggleGroup.Item><ToggleGroup.Item value="controls">Basic controls</ToggleGroup.Item><ToggleGroup.Item value="full_control">Full control</ToggleGroup.Item></ToggleGroup.Root></Field.Set>
                <Field.Separator />
                <Field.Field orientation="horizontal"><Field.Content><Field.Title>Allow print cancellation</Field.Title><Field.Description>Enables the stop command from local clients.</Field.Description></Field.Content><Switch id="allow-cancel" bind:checked={settings.allow_cancel} /></Field.Field>
                <Field.Field orientation="horizontal"><Field.Content><Field.Title>Allow cloud print submission</Field.Title><Field.Description>Uploads staged 3MF jobs through the official plugin.</Field.Description></Field.Content><Switch id="cloud-print" bind:checked={settings.cloud_print_enabled} /></Field.Field>
                <Field.Field orientation="horizontal"><Field.Content><Field.Title>Allow reprinting remote files</Field.Title><Field.Description>Permits a local client to submit an existing printer file through the cloud.</Field.Description></Field.Content><Switch id="remote-reprint" bind:checked={settings.remote_reprint_enabled} /></Field.Field>
                <Field.Field orientation="horizontal"><Field.Content><Field.Title>Advertise controls</Field.Title><Field.Description>Publishes control capabilities to LAN clients when policy permits them.</Field.Description></Field.Content><Switch id="advertise-controls" bind:checked={settings.advertise_controls} /></Field.Field>
                <Field.Field orientation="horizontal"><Field.Content><Field.Title>Allow printer-file upload</Field.Title><Field.Description>Remote uploads remain independently gated and restricted to staging.</Field.Description></Field.Content><Switch id="remote-upload" bind:checked={settings.storage_allow_upload} /></Field.Field>
                <Field.Field orientation="horizontal"><Field.Content><Field.Title>Allow printer-file deletion</Field.Title><Field.Description>Remote deletion remains independently gated.</Field.Description></Field.Content><Switch id="remote-delete" bind:checked={settings.storage_allow_delete} /></Field.Field>
              </Field.Group>
            {/if}
          </Card.Content>
          <Card.Footer><Button onclick={saveSettings} disabled={loading || !settings}>Save policy</Button></Card.Footer>
        </Card.Root>
      </Tabs.Content>

      <Tabs.Content value="connections">
        <div class="grid gap-4 md:grid-cols-2">
          <Card.Root><Card.Header><Card.Title>Bambuddy / Studio</Card.Title><Card.Description>Use the proxy address as a LAN printer.</Card.Description></Card.Header><Card.Content><dl class="flex flex-col gap-3 text-sm"><div class="flex justify-between gap-4"><dt class="text-muted-foreground">MQTT TLS</dt><dd class="font-mono">8883</dd></div><div class="flex justify-between gap-4"><dt class="text-muted-foreground">Implicit FTPS</dt><dd class="font-mono">990</dd></div><div class="flex justify-between gap-4"><dt class="text-muted-foreground">Native files</dt><dd class="font-mono">6000</dd></div><div class="flex justify-between gap-4"><dt class="text-muted-foreground">RTSPS camera</dt><dd class="font-mono">322</dd></div></dl></Card.Content></Card.Root>
          <Card.Root><Card.Header><Card.Title>Client trust</Card.Title><Card.Description>Export only the public proxy CA for clients that validate the LAN certificate.</Card.Description></Card.Header><Card.Content><Button variant="outline" onclick={downloadCa}><ShieldCheckIcon data-icon="inline-start" />Download proxy CA</Button></Card.Content></Card.Root>
          <Alert.Root class="md:col-span-2"><ShieldCheckIcon /><Alert.Title>Private network only</Alert.Title><Alert.Description>Do not expose these printer-shaped ports or this admin console directly to the public internet.</Alert.Description></Alert.Root>
        </div>
      </Tabs.Content>
    </Tabs.Root>
  {/if}
</main>
