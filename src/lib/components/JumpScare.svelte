<!-- JumpScare — F5 : une créature surgit plein écran 2 s (SCARE).
  Miroir de apparition.rs (SCARY[8], bord sang). En mouvement réduit :
  affichage statique, sans flash ni tremblement. -->
<script lang="ts">
  import type { ApparitionState } from "../contracts";

  interface Props {
    apparition: ApparitionState;
  }

  let { apparition }: Props = $props();

  let reduced = $state(false);

  $effect(() => {
    const mq = window.matchMedia("(prefers-reduced-motion: reduce)");
    reduced = mq.matches;
    const onChange = (e: MediaQueryListEvent): void => {
      reduced = e.matches;
    };
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  });
</script>

<div
  class="scare"
  class:calm={reduced}
  data-testid="jumpscare"
  role="alert"
  aria-label={`Apparition : ${apparition.creature.name}`}
>
  <p class="name crypte-title">{apparition.creature.name}</p>
  <p class="lore">{apparition.creature.lore}</p>
</div>

<style>
  .scare {
    position: fixed;
    inset: 0;
    z-index: 70;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 1rem;
    text-align: center;
    padding: 2rem;
    background: var(--cry-dried);
    border: 0.5rem solid var(--cry-blood);
    animation: shake 0.12s infinite;
  }
  .scare.calm {
    animation: none;
    background: rgba(110, 16, 26, 0.94);
  }
  .name {
    margin: 0;
    font-size: clamp(2rem, 8vw, 4.5rem);
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }
  .lore {
    margin: 0;
    max-width: 36rem;
    font-family: var(--cry-font-title);
    font-style: italic;
    font-size: 1.15rem;
    color: var(--cry-bone);
  }
  @keyframes shake {
    0% {
      transform: translate(0, 0);
    }
    25% {
      transform: translate(-6px, 3px);
    }
    50% {
      transform: translate(5px, -4px);
    }
    75% {
      transform: translate(-3px, -3px);
    }
    100% {
      transform: translate(4px, 4px);
    }
  }
</style>
