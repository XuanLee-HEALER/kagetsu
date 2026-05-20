<script>
  import { PIG } from '../../core/pigments.js';

  export let t = '';
  /** @type {'accent'|'danger'|undefined} */
  export let tone = undefined;

  $: color = tone === 'accent' ? PIG.gser
    : tone === 'danger' ? PIG.mtshal
    : 'var(--fg-primary)';
</script>

<!-- Tiny labeled chip (e.g. 庄 / 立 badges). -->
<span style="font-size: 10px; font-weight: 600; color: {color}; border: 1px solid {color}; border-radius: 2px; padding: 1px 4px; line-height: 1;">{t}</span>
