<!--
  Wrapper that hosts a single 1440×900 artboard. Provides:
  - dark deepest background outside the artboard
  - a top bar with link back home and the artboard label
  - centering on screens wider than 1440, scrolling on smaller
-->
<script>
  import DorjeMark from '../core/DorjeMark.svelte';

  export let label = '';
</script>

<div style="min-height: 100vh; background: var(--bg-deepest); color: var(--fg-primary); font-family: var(--font-sans);">
  <div style="position: sticky; top: 0; z-index: 50; backdrop-filter: blur(12px); background: rgba(12,12,20,0.78); border-bottom: 1px solid var(--border-subtle); padding: 10px 24px; display: flex; align-items: center; gap: 16px;">
    <a href={'#'} style="display: flex; align-items: center; gap: 10px; color: var(--fg-tertiary); text-decoration: none; font: var(--t-eyebrow); letter-spacing: var(--tracking-widest); text-transform: uppercase;">
      <DorjeMark size={16} />
      ← kagetsu
    </a>
    <span style="color: var(--fg-disabled);">·</span>
    <span style="font-family: var(--font-mono); font-size: 12px; color: var(--fg-secondary);">{label}</span>
  </div>

  <div style="display: flex; justify-content: center; padding: 24px 0; min-width: 1440px;">
    <div style="width: 1440px; height: 900px; box-shadow: 0 24px 60px rgba(0,0,0,0.55); border-radius: 4px; overflow: hidden;">
      <slot />
    </div>
  </div>
</div>
