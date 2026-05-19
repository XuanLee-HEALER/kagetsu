<script>
  export let style = '';
  export let raised = false;
  export let accent = false;
  export let padding = undefined;

  $: s = [
    `background: ${accent ? 'var(--accent-soft)' : 'var(--bg-surface0)'}`,
    `border: 1px solid ${accent ? 'var(--border-focus)' : 'var(--border-default)'}`,
    `border-radius: var(--radius-lg)`,
    `box-shadow: ${raised ? 'var(--shadow-2)' : 'var(--shadow-0)'}`,
    `padding: ${padding ?? 'var(--space-5)'}`,
    style,
  ].filter(Boolean).join('; ');
</script>

<div style={s}><slot /></div>
