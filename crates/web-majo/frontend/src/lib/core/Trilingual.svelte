<script>
  export let en = '';
  export let zh = '';
  export let ja = '';
  export let style = '';

  $: s = `color: var(--fg-secondary); font-size: var(--text-sm); ${style}`;
</script>

<span style={s}>
  {en}
  {#if zh}<span style="color: var(--fg-tertiary); margin: 0 8px;">·</span>{zh}{/if}
  {#if ja}<span style="color: var(--fg-tertiary); margin: 0 8px;">·</span>{ja}{/if}
</span>
