<script>
  import KeyBadge from '../../core/KeyBadge.svelte';

  export let k = '';
  export let label = '';
  export let detail = '';
  /** @type {'default'|'primary'|'danger'|'ok'|'info'|undefined} */
  export let tone = undefined;
  export let selected = false;
  export let disabled = false;

  $: bg = selected ? 'var(--accent-soft)' : 'transparent';
  $: borderColor = selected ? 'var(--border-focus)' : 'transparent';
  $: opacity = disabled ? 0.45 : 1;
  $: keyTone = selected ? 'primary' : (tone || 'default');
  $: labelColor = disabled ? 'var(--fg-disabled)' : 'var(--fg-primary)';
</script>

<!-- One row inside the action modal — key chip · label · detail. -->
<div style="display: grid; grid-template-columns: auto 140px 1fr; align-items: center; gap: 14px; padding: 10px 12px; background: {bg}; border: 1px solid {borderColor}; border-radius: var(--radius-md); opacity: {opacity};">
  <KeyBadge {k} tone={keyTone} />
  <span style="font-weight: 500; color: {labelColor}; font-size: 14px;">{label}</span>
  <span style="color: var(--fg-tertiary); font-size: 12px;">{detail}</span>
</div>
