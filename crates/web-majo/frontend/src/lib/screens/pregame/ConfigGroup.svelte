<script>
  import Eyebrow from '../../core/Eyebrow.svelte';

  export let label = '';
</script>

<div style="margin-bottom: 28px;">
  <Eyebrow style="margin-bottom: 10px;">{label}</Eyebrow>
  <div style="display: flex; flex-direction: column; gap: 8px;">
    <slot />
  </div>
</div>
