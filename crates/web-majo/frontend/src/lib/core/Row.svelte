<script>
  export let label = '';
  /** When value is a plain string, rendered as a span; otherwise use a <slot> instead. */
  export let value = undefined;
  export let mono = false;
</script>

<div style="display: flex; align-items: center; justify-content: space-between; gap: 12px;">
  <span style="color: var(--fg-tertiary); font-size: 12px;">{label}</span>
  <span style="color: var(--fg-primary); font-size: 13px; font-family: {mono ? 'var(--font-mono)' : 'var(--font-sans)'};">
    {#if value !== undefined}{value}{:else}<slot />{/if}
  </span>
</div>
