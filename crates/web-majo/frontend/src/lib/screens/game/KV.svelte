<script>
  export let label = '';
  export let value = '';
</script>

<!-- Tiny key/value block used inside the central plate. -->
<div>
  <div style="font: var(--t-eyebrow); letter-spacing: var(--tracking-wide); text-transform: uppercase; color: var(--fg-tertiary); font-size: 9px;">{label}</div>
  <div style="font-family: var(--font-mono); font-size: var(--text-lg); color: var(--fg-primary); line-height: 1.1;">{value}</div>
</div>
