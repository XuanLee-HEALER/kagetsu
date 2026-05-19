<script>
  export let style = '';
  $: s = `height: 1px; background: var(--border-subtle); ${style}`;
</script>

<div style={s}></div>
