// Minimal hash router — no deps. Subscribes via window.addEventListener('hashchange').
// Components import `route` (a writable store) and switch on its value.

import { writable } from 'svelte/store';

function readHash() {
  if (typeof window === 'undefined') return '';
  return window.location.hash.replace(/^#/, '');
}

export const route = writable(readHash());

if (typeof window !== 'undefined') {
  window.addEventListener('hashchange', () => {
    route.set(readHash());
  });
}

export function go(target) {
  if (typeof window === 'undefined') return;
  window.location.hash = target;
}
