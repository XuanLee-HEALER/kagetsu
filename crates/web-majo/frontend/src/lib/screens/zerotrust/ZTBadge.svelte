<script>
  // ZeroTrust badge — small pill with inline shield icon, ngonpo blue accent.
  import { PIG } from '../../core/pigments.js';

  $: wrapStyle = `display: inline-flex; align-items: center; gap: 8px; padding: 4px 10px 4px 8px; background: linear-gradient(90deg, rgba(91,138,184,0.16) 0%, rgba(46,72,120,0.12) 100%); border: 1px solid ${PIG.ngonpo}; border-radius: var(--radius-pill); color: ${PIG.ngonpo}; font-size: 11px; font-weight: 600; letter-spacing: var(--tracking-wide); text-transform: uppercase;`;
</script>

<div style={wrapStyle}>
  <!-- simplified shield/lock icon -->
  <svg width="11" height="13" viewBox="0 0 11 13" fill="none" stroke="currentColor" stroke-width="1.4">
    <path d="M5.5 1 L9.5 2.5 V6.5 C9.5 9 7.5 11 5.5 12 C3.5 11 1.5 9 1.5 6.5 V2.5 Z" />
  </svg>
  ZeroTrust
</div>
