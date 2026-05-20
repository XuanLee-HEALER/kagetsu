<script>
  export let style = '';
  $: s = `font: var(--t-eyebrow); letter-spacing: var(--tracking-widest); text-transform: uppercase; color: var(--fg-tertiary); ${style}`;
</script>

<div style={s}><slot /></div>
