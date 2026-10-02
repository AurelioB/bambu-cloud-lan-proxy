<script lang="ts">
  import { onDestroy } from 'svelte';
  import { ArrowRight, LockKeyhole, LogIn } from '@lucide/svelte';
  import * as Card from '$lib/components/ui/card';
  import * as Field from '$lib/components/ui/field';
  import { Input } from '$lib/components/ui/input';
  import { Button } from '$lib/components/ui/button';
  import { Spinner } from '$lib/components/ui/spinner';
  import { api, type AuthSession } from '$lib/api';
  let {
    options,
    signedIn,
    message = '',
  }: {
    options: AuthSession;
    signedIn: (session: AuthSession) => Promise<void>;
    message?: string;
  } = $props();
  let username = $state('');
  let password = $state('');
  let accessCode = $state('');
  let busy = $state(false);
  let error = $state('');
  let disposed = false;
  onDestroy(() => {
    disposed = true;
  });
  async function login() {
    busy = true;
    error = '';
    try {
      const session = await api<AuthSession>('/api/auth/login', {
        method: 'POST',
        body: JSON.stringify(
          options.access_code
            ? { access_code: accessCode }
            : { username, password },
        ),
      });
      if (!disposed) await signedIn({ ...options, ...session });
    } catch (e) {
      error = e instanceof Error ? e.message : 'Could not sign in.';
    } finally {
      password = '';
      accessCode = '';
      busy = false;
    }
  }
  async function oidc() {
    busy = true;
    error = '';
    try {
      const { url } = await api<{ url: string }>('/api/auth/oidc/start', {
        method: 'POST',
      });
      const target = new URL(url);
      if (!['https:', 'http:'].includes(target.protocol))
        throw new Error('Invalid sign-in address.');
      if (!disposed) window.location.assign(target.href);
    } catch (e) {
      error =
        e instanceof Error ? e.message : 'Could not start single sign-on.';
      busy = false;
    }
  }
</script>

<Card.Root>
  <Card.Header>
    <span class="section-icon"><LockKeyhole /></span>
    <Card.Title>Welcome to your workspace</Card.Title>
    <Card.Description
      >Sign in to access printers, connections, and permissions.</Card.Description
    >
  </Card.Header>
  <Card.Content>
    <Field.Group>
      {#if options.oidc}
        <Button onclick={oidc} disabled={busy}>
          {#if busy}<Spinner data-icon="inline-start" />{:else}<LogIn
              data-icon="inline-start"
            />{/if}
          Continue with single sign-on
        </Button>
        {#if options.local}<Field.Separator
            >Or use your local account</Field.Separator
          >{/if}
      {/if}
      {#if options.local || options.access_code}
        <form
          id="unlock"
          onsubmit={(event) => {
            event.preventDefault();
            void login();
          }}
        >
          <Field.Group>
            {#if options.local}
              <Field.Field data-disabled={busy}>
                <Field.Label for="admin-username">Username</Field.Label>
                <Input
                  id="admin-username"
                  bind:value={username}
                  autocomplete="username"
                  required
                  disabled={busy}
                  maxlength={128}
                />
              </Field.Field>
              <Field.Field data-invalid={Boolean(error)} data-disabled={busy}>
                <Field.Label for="admin-password">Password</Field.Label>
                <Input
                  id="admin-password"
                  type="password"
                  bind:value={password}
                  autocomplete="current-password"
                  required
                  disabled={busy}
                  maxlength={1024}
                  aria-invalid={Boolean(error)}
                  aria-describedby={error ? 'login-error' : undefined}
                />
              </Field.Field>
            {:else}
              <Field.Field data-invalid={Boolean(error)} data-disabled={busy}>
                <Field.Label for="access-code">Proxy access code</Field.Label>
                <Input
                  id="access-code"
                  type="password"
                  bind:value={accessCode}
                  autocomplete="current-password"
                  required
                  disabled={busy}
                  maxlength={1024}
                  aria-invalid={Boolean(error)}
                  aria-describedby={error ? 'login-error' : undefined}
                />
              </Field.Field>
            {/if}
          </Field.Group>
        </form>
      {/if}
      {#if error || message}<p
          id="login-error"
          role="alert"
          class="text-sm text-destructive"
        >
          {error || message}
        </p>{/if}
    </Field.Group>
  </Card.Content>
  <Card.Footer class="flex-col items-stretch gap-4">
    {#if options.local || options.access_code}
      <Button
        form="unlock"
        type="submit"
        disabled={busy ||
          (options.local ? !username || !password : !accessCode)}
      >
        {#if busy}<Spinner data-icon="inline-start" />{/if}Open workspace<ArrowRight
          data-icon="inline-end"
        />
      </Button>
    {/if}
    <p class="text-xs text-muted-foreground">
      Your browser receives a private, expiring session. This sign-in is
      separate from your Bambu account.
    </p>
  </Card.Footer>
</Card.Root>
