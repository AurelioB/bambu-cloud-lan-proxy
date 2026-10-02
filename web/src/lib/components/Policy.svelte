<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import {
    Eye,
    SlidersHorizontal,
    ShieldCheck,
    Info,
    RotateCcw,
  } from '@lucide/svelte';
  import * as Card from '$lib/components/ui/card';
  import * as Field from '$lib/components/ui/field';
  import * as ToggleGroup from '$lib/components/ui/toggle-group';
  import * as Alert from '$lib/components/ui/alert';
  import { Button } from '$lib/components/ui/button';
  import { Badge } from '$lib/components/ui/badge';
  import { Switch } from '$lib/components/ui/switch';
  import { Spinner } from '$lib/components/ui/spinner';
  import { api, type Settings } from '$lib/api';
  import { toast } from 'svelte-sonner';
  let {
    initial,
    saved,
  }: {
    initial: Settings;
    saved: (value: Settings) => void;
  } = $props();
  let settings = $state<Settings>(untrack(() => ({ ...initial })));
  let baseline = $state(untrack(() => JSON.stringify(initial)));
  let dirty = $derived(JSON.stringify(settings) !== baseline);
  let saving = $state(false);
  let error = $state('');
  let disposed = false;
  onDestroy(() => {
    disposed = true;
  });
  const levels = [
    { value: 'read_only', title: 'Read only', icon: Eye },
    { value: 'controls', title: 'Basic controls', icon: SlidersHorizontal },
    { value: 'full_control', title: 'Full control', icon: ShieldCheck },
  ] as const;
  const descriptions = {
    read_only:
      'Status and read queries are available. Printer controls remain blocked.',
    controls:
      'Allowlisted controls such as lights, pause, and resume are available.',
    full_control:
      'Valid printer commands pass through Bambu’s signing plugin. Individual permissions below still apply.',
  };
  const permissions: {
    key: Exclude<keyof Settings, 'mode'>;
    title: string;
    description: string;
    group: string;
  }[] = [
    {
      key: 'allow_cancel',
      title: 'Cancel prints',
      description: 'Let local apps stop an active job.',
      group: 'Printing',
    },
    {
      key: 'cloud_print_enabled',
      title: 'Send new print jobs',
      description:
        'Upload staged 3MF files through Bambu Cloud. This permission is independent of the command level.',
      group: 'Printing',
    },
    {
      key: 'remote_reprint_enabled',
      title: 'Reprint stored jobs',
      description: 'Start files already on the printer. Requires Full control.',
      group: 'Printing',
    },
    {
      key: 'advertise_controls',
      title: 'Show controls in local apps',
      description: 'Advertise the controls allowed by your policy.',
      group: 'Printing',
    },
    {
      key: 'storage_allow_upload',
      title: 'Upload printer files',
      description: 'Allow files in staging to be copied to printer storage.',
      group: 'File access',
    },
    {
      key: 'storage_allow_delete',
      title: 'Delete printer files',
      description:
        'Allow removal of supported files. Deleted printer files cannot be restored by the proxy.',
      group: 'File access',
    },
  ];
  async function save() {
    saving = true;
    error = '';
    try {
      const next = await api<Settings>('/api/admin/settings', {
        method: 'PUT',
        body: JSON.stringify(settings),
      });
      if (disposed) return;
      settings = next;
      baseline = JSON.stringify(next);
      saved(next);
      toast.success('Permissions saved');
    } catch (e) {
      error = e instanceof Error ? e.message : 'Could not save permissions.';
    } finally {
      saving = false;
    }
  }
</script>

<div class="settings-layout">
  <form
    class="flex min-w-0 flex-col gap-6"
    onsubmit={(event) => {
      event.preventDefault();
      void save();
    }}
  >
    <Card.Root
      ><Card.Header
        ><Card.Title>Choose how much control to share</Card.Title
        ><Card.Description
          >Set the command level for all printers in this workspace.</Card.Description
        ></Card.Header
      ><Card.Content>
        <Field.Group
          ><Field.Set
            ><Field.Legend class="sr-only">Cloud command level</Field.Legend
            ><ToggleGroup.Root
              type="single"
              value={settings.mode}
              onValueChange={(value) => {
                if (levels.some((level) => level.value === value))
                  settings.mode = value as Settings['mode'];
              }}
              variant="outline"
              spacing={2}
              class="flex-wrap"
              disabled={saving}
              aria-label="Cloud command level"
              >{#each levels as level (level.value)}<ToggleGroup.Item
                  value={level.value}
                  ><level.icon />{level.title}</ToggleGroup.Item
                >{/each}</ToggleGroup.Root
            ><Field.Description>{descriptions[settings.mode]}</Field.Description
            ></Field.Set
          ></Field.Group
        >
      </Card.Content></Card.Root
    >
    {#each ['Printing', 'File access'] as group (group)}<Card.Root
        ><Card.Header
          ><Card.Title>{group}</Card.Title><Card.Description
            >{group === 'Printing'
              ? 'Decide what your connected apps can request.'
              : 'Read access remains available. Choose which file changes to allow.'}</Card.Description
          ></Card.Header
        ><Card.Content
          ><Field.Set disabled={saving}>
            <Field.Legend class="sr-only">{group} permissions</Field.Legend>
            <Field.Group
              >{#each permissions.filter((item) => item.group === group) as item (item.key)}<Field.Field
                  orientation="horizontal"
                  data-disabled={saving}
                  ><Field.Content
                    ><Field.Label for={item.key}>{item.title}</Field.Label
                    ><Field.Description>{item.description}</Field.Description
                    ></Field.Content
                  ><Switch
                    id={item.key}
                    bind:checked={settings[item.key]}
                    disabled={saving}
                  /></Field.Field
                >{/each}</Field.Group
            ></Field.Set
          ></Card.Content
        ></Card.Root
      >{/each}
    {#if error}<Alert.Root variant="destructive"
        ><Alert.Title>Permissions were not saved</Alert.Title><Alert.Description
          >{error}</Alert.Description
        ></Alert.Root
      >{/if}
    <div class="save-bar">
      <span class="text-sm text-muted-foreground" role="status"
        >{dirty ? 'You have unsaved changes' : 'All changes saved'}</span
      >
      <div class="flex gap-2">
        <Button
          variant="ghost"
          disabled={!dirty || saving}
          onclick={() => {
            settings = JSON.parse(baseline);
          }}><RotateCcw data-icon="inline-start" />Reset</Button
        ><Button type="submit" disabled={!dirty || saving}
          >{#if saving}<Spinner data-icon="inline-start" />{/if}Save permissions</Button
        >
      </div>
    </div>
  </form>
  <aside class="flex flex-col gap-4">
    <div class="editorial-note">
      <span class="section-icon"><ShieldCheck /></span>
      <h2>Your workspace.<br />Your boundaries.</h2>
      <p>
        These settings apply to requests from local apps. Your Bambu Handy and
        Studio cloud connections remain separate.
      </p>
      <Badge variant="outline">Applies to all printers</Badge>
    </div>
    <Alert.Root
      ><Info /><Alert.Title>Changes are explicit</Alert.Title><Alert.Description
        >Permissions take effect when you save. Previously sent commands are
        never replayed.</Alert.Description
      ></Alert.Root
    >
  </aside>
</div>
