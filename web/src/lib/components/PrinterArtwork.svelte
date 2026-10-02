<script lang="ts">
  let { dual = false }: { dual?: boolean } = $props();
</script>

<!-- An original, schematic printer illustration; not a model photograph. -->
<svg viewBox="0 0 240 180" fill="none" aria-hidden="true" class="printer-art">
  <ellipse cx="121" cy="161" rx="78" ry="9" fill="currentColor" opacity=".06" />
  <path
    d="M61 36 165 22 190 39v111l-105 15-24-17Z"
    fill="var(--card)"
    stroke="currentColor"
    stroke-width="1.5"
  />
  <path
    d="m61 36 24 16 105-13M85 52v113M165 22v112l25 16"
    stroke="currentColor"
    stroke-width="1.5"
  />
  <path
    d="m95 62 83-11v85l-83 12Z"
    fill="var(--muted)"
    stroke="currentColor"
    stroke-opacity=".3"
  />
  <path
    d="m107 66 1 65m58-74 1 65m-68-37 73-10"
    stroke="currentColor"
    stroke-opacity=".28"
    stroke-width="2"
  />
  <path
    d="m105 126 54-8 13 9-54 8Z"
    fill="var(--primary)"
    fill-opacity=".15"
    stroke="var(--primary)"
  />
  <path d="m119 117 23-3 12 7-24 3Z" fill="var(--primary)" fill-opacity=".35" />
  <path d="m128 114 1-16 15-2 1 17" stroke="var(--primary)" stroke-width="2" />
  <path
    d="m123 73 23-3v17l-23 3Z"
    fill="var(--card)"
    stroke="currentColor"
    stroke-width="1.5"
  />
  <path d="m134 89 1 5" stroke="var(--primary)" stroke-width="3" />
  {#if dual}<path
      d="m141 88 1 5"
      stroke="var(--primary)"
      stroke-width="3"
    />{/if}
  <path
    d="m67 49 11 7v18l-11-7Z"
    fill="var(--primary)"
    fill-opacity=".18"
    stroke="currentColor"
    stroke-opacity=".3"
  />
  <path d="m71 56 4 3m-4 4 4 3" stroke="var(--primary)" stroke-width="1.5" />
  <circle cx="178" cy="47" r="2" fill="var(--primary)" />
  <path
    d="m102 154 29-4m4-1 9-1"
    stroke="currentColor"
    stroke-opacity=".25"
    stroke-width="2"
  />
</svg>
