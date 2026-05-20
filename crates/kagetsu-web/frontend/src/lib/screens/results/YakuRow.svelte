<script>
  import { PIG } from '../../core/pigments.js';

  export let name = '';
  export let han = 0;
  export let accent = false;

  $: nameColor = accent ? PIG.gser : 'var(--fg-primary)';
  $: hanColor = accent ? PIG.gser : 'var(--fg-secondary)';
</script>

<div style="display: flex; justify-content: space-between; align-items: center; padding: 10px 0; border-bottom: 1px solid var(--border-subtle);">
  <span style="font-size: 14px; color: {nameColor};">{name}</span>
  <span style="font-family: var(--font-mono); font-size: 14px; color: {hanColor};">{han} 翻</span>
</div>
