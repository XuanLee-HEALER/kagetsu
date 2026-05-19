<script>
  import { route } from './lib/router.js';
  import Home from './lib/screens/Home.svelte';
  import Frame from './lib/screens/Frame.svelte';

  import MenuScreen from './lib/screens/MenuScreen.svelte';
  import ConfigScreen from './lib/screens/ConfigScreen.svelte';
  import LobbyScreen from './lib/screens/LobbyScreen.svelte';
  import RoomScreen from './lib/screens/RoomScreen.svelte';
  import GameScreen from './lib/screens/GameScreen.svelte';
  import ZeroTrustGameScreen from './lib/screens/ZeroTrustGameScreen.svelte';
  import HandResultScreen from './lib/screens/HandResultScreen.svelte';
  import MatchEndScreen from './lib/screens/MatchEndScreen.svelte';

  // ROUTES — keyed by hash without leading '#'.
  // `component` is the svelte component to render inside the Frame.
  // `props` are passed verbatim (used for GameScreen's 3 modes).
  const ROUTES = {
    'g-normal': { component: GameScreen, label: '01 ・ Normal', props: { mode: 'NORMAL' } },
    'g-command': { component: GameScreen, label: '02 ・ Command 命令输入', props: { mode: 'COMMAND', commandText: 'discard p4' } },
    'g-modal': { component: GameScreen, label: '03 ・ Action modal 唤起', props: { mode: 'NORMAL', showActionModal: true } },
    'zt-game': { component: ZeroTrustGameScreen, label: '04 ・ ZeroTrust 对局', props: {} },
    'menu': { component: MenuScreen, label: '05 ・ Main menu', props: {} },
    'config': { component: ConfigScreen, label: '06 ・ Pre-game config', props: {} },
    'lobby': { component: LobbyScreen, label: '07 ・ LAN lobby', props: {} },
    'room': { component: RoomScreen, label: '08 ・ Room waiting', props: {} },
    'hand-result': { component: HandResultScreen, label: '09 ・ Hand result', props: {} },
    'match-end': { component: MatchEndScreen, label: '10 ・ Match end', props: {} },
  };

  $: current = ROUTES[$route];
</script>

{#if !$route || !current}
  <Home />
{:else}
  <Frame label={current.label}>
    <svelte:component this={current.component} {...current.props} />
  </Frame>
{/if}
