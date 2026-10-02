<script lang="ts">
  import { Cloud, ArrowUpRight, Check, Link, Info } from '@lucide/svelte';
  import * as Card from '$lib/components/ui/card';
  import * as Field from '$lib/components/ui/field';
  import * as Alert from '$lib/components/ui/alert';
  import { Input } from '$lib/components/ui/input';
  import { Badge } from '$lib/components/ui/badge';
  import { Button } from '$lib/components/ui/button';
  import { Spinner } from '$lib/components/ui/spinner';
  import { api, type Overview } from '$lib/api';
  import { toast } from 'svelte-sonner';
  let {
    overview,
    refresh,
  }: { overview: Overview; refresh: () => Promise<void> } = $props();
  let callback = $state('');
  let loginUrl = $state('');
  let busy = $state(false);
  let error = $state('');
  async function begin() {
    // Open synchronously so the user gesture survives the API request.
    const popup = window.open('about:blank', '_blank');
    if (popup) popup.opener = null;
    busy = true;
    error = '';
    try {
      const result = await api<{ url: string }>('/api/cloud/auth/url');
      const url = new URL(result.url);
      if (url.protocol !== 'https:')
        throw new Error('The sign-in link is not secure.');
      loginUrl = url.href;
      if (popup) popup.location.href = loginUrl;
    } catch (e) {
      popup?.close();
      error = e instanceof Error ? e.message : 'Could not open sign-in.';
    } finally {
      busy = false;
    }
  }
  async function complete() {
    busy = true;
    error = '';
    try {
      await api('/api/cloud/auth/paste', {
        method: 'POST',
        body: JSON.stringify({ pasted_url: callback }),
      });
      callback = '';
      toast.success('Bambu account connected');
      await refresh();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Could not complete sign-in.';
    } finally {
      busy = false;
    }
  }
</script>

<div class="settings-layout">
  <div class="flex flex-col gap-6">
    <Card.Root
      ><Card.Header
        ><Card.Title>Your Bambu connection</Card.Title><Card.Description
          >One account session, shared across your printers.</Card.Description
        ><Card.Action><Cloud /></Card.Action></Card.Header
      ><Card.Content
        ><dl class="detail-list">
          <div>
            <dt>Account</dt>
            <dd>
              <Badge variant={overview.signed_in ? 'success' : 'warning'}
                >{overview.signed_in ? 'Signed in' : 'Not signed in'}</Badge
              >
            </dd>
          </div>
          <div>
            <dt>Cloud connection</dt>
            <dd>{overview.cloud_connected ? 'Connected' : 'Disconnected'}</dd>
          </div>
          <div>
            <dt>Region</dt>
            <dd>{overview.region || 'Not available'}</dd>
          </div>
          <div>
            <dt>Discovered printers</dt>
            <dd>{overview.printers.length}</dd>
          </div>
        </dl></Card.Content
      ></Card.Root
    >
    <Card.Root
      ><Card.Header
        ><Card.Title
          >{overview.signed_in
            ? 'Reconnect your account'
            : 'Connect your Bambu account'}</Card.Title
        ><Card.Description
          >Complete these two steps using Bambu’s sign-in page.</Card.Description
        ></Card.Header
      ><Card.Content>
        <form
          id="account-form"
          onsubmit={(event) => {
            event.preventDefault();
            void complete();
          }}
        >
          <Field.Group
            ><Field.Field
              ><Field.Title
                ><span class="step-number">1</span> Sign in with Bambu</Field.Title
              ><Field.Description
                >Open the secure sign-in page in a new tab.</Field.Description
              ><Button variant="outline" disabled={busy} onclick={begin}
                >Open Bambu sign-in<ArrowUpRight
                  data-icon="inline-end"
                /></Button
              >{#if loginUrl}<a
                  class="text-sm text-primary underline underline-offset-4"
                  href={loginUrl}
                  target="_blank"
                  rel="noreferrer">Open sign-in link again</a
                >{/if}</Field.Field
            ><Field.Separator /><Field.Field
              ><Field.Label for="callback"
                ><span class="step-number">2</span> Paste the return URL</Field.Label
              ><Field.Description
                >After sign-in, you may see an unreachable localhost page. Copy
                the full address from that tab and paste it here.</Field.Description
              ><Input
                id="callback"
                bind:value={callback}
                placeholder="http://localhost:13618/?ticket=…"
                autocomplete="off"
                disabled={busy}
                required
              /></Field.Field
            ></Field.Group
          >
        </form>
      </Card.Content><Card.Footer
        ><Button
          form="account-form"
          type="submit"
          disabled={busy || !callback.trim()}
          >{#if busy}<Spinner data-icon="inline-start" />{:else}<Link
              data-icon="inline-start"
            />{/if}Complete connection</Button
        ></Card.Footer
      ></Card.Root
    >
    {#if error}<Alert.Root variant="destructive"
        ><Alert.Title>Could not connect</Alert.Title><Alert.Description
          >{error}</Alert.Description
        ></Alert.Root
      >{/if}
  </div>
  <aside class="editorial-note">
    <span class="section-icon"><Check /></span>
    <h2>Keep the apps<br />you already love.</h2>
    <p>
      Your printer stays paired with Bambu Cloud, so Handy and Studio can work
      alongside your local apps.
    </p>
    <div class="flex items-start gap-2 text-sm text-muted-foreground">
      <Info class="mt-0.5 size-4 shrink-0" /><span
        >The official networking plugin stores the account session and signing
        material on this host.</span
      >
    </div>
  </aside>
</div>
