<script>
  // Single event-log line — timestamp · who · message (tone-colored).
  import { PIG } from '../../core/pigments.js';

  export let t = '';
  export let who = '';
  export let msg = '';
  /** @type {'ok'|'info'|'dim'|undefined} */
  export let tone = undefined;

  $: color = tone === 'ok' ? PIG.ljangkhu
    : tone === 'info' ? PIG.ngonpo
    : tone === 'dim' ? 'var(--fg-disabled)'
    : 'var(--fg-secondary)';
</script>

<div style="display: grid; grid-template-columns: auto auto 1fr; gap: 8px;">
  <span style="color: var(--fg-disabled);">{t}</span>
  <span style="color: {PIG.dri_med}; font-weight: 600; min-width: 22px;">{who}</span>
  <span style="color: {color};">{msg}</span>
</div>
