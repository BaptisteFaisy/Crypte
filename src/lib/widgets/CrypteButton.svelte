<!-- CrypteButton — bouton unique (styles : crypte.css .cry-btn*). -->
<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    variant?: "primary" | "danger" | "ghost" | "subtle";
    size?: "sm" | "md";
    disabled?: boolean;
    title?: string;
    type?: "button" | "submit" | "reset";
    /** État enfoncé (bascule) ; undefined = bouton simple. */
    pressed?: boolean;
    onclick?: (event: MouseEvent) => void;
    children: Snippet;
  }

  let {
    variant = "primary",
    size = "md",
    disabled = false,
    title,
    type = "button",
    pressed,
    onclick,
    children,
  }: Props = $props();
</script>

<button
  class="cry-btn cry-btn--{variant}{size === 'sm' ? ' cry-btn--sm' : ''}"
  {type}
  {disabled}
  {title}
  aria-pressed={pressed ?? undefined}
  {onclick}
>
  {@render children()}
</button>
