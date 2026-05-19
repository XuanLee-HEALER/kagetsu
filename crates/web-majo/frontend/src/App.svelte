<script>
  import { route } from './lib/router.js';
  import Home from './lib/screens/Home.svelte';
  import Frame from './lib/screens/Frame.svelte';
  import MenuScreen from './lib/screens/MenuScreen.svelte';

  // Other screens get imported as they are translated. Routes that resolve
  // to undefined render a placeholder telling the user the screen is WIP.

  const ROUTES = {
    'menu': { component: MenuScreen, label: '05 ・ Main menu', mode: undefined },
    // wired below as screens land:
    'g-normal': { component: null, label: '01 ・ Normal' },
    'g-command': { component: null, label: '02 ・ Command 命令输入' },
    'g-modal': { component: null, label: '03 ・ Action modal 唤起' },
    'zt-game': { component: null, label: '04 ・ ZeroTrust 对局' },
    'config': { component: null, label: '06 ・ Pre-game config' },
    'lobby': { component: null, label: '07 ・ LAN lobby' },
    'room': { component: null, label: '08 ・ Room waiting' },
    'hand-result': { component: null, label: '09 ・ Hand result' },
    'match-end': { component: null, label: '10 ・ Match end' },
  };

  $: current = ROUTES[$route];
</script>

{#if !$route || !current}
  <Home />
{:else if !current.component}
  <Frame label={current.label}>
    <div style="height: 100%; display: flex; align-items: center; justify-content: center; flex-direction: column; gap: 12px; color: var(--fg-tertiary); font-family: var(--font-mono); background: var(--bg-base);">
      <div style="font-size: 28px; font-family: var(--font-serif); color: var(--fg-secondary);">WIP</div>
      <div>{current.label}</div>
      <a href={'#'} style="color: var(--accent); text-decoration: none; margin-top: 12px;">← back to index</a>
    </div>
  </Frame>
{:else}
  <Frame label={current.label}>
    <svelte:component this={current.component} mode={current.mode} />
  </Frame>
{/if}
