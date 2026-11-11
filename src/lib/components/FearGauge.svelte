<!-- FearGauge — jauge de contexte (miroir de fear_gauge, app/ui.rs).
  `fear ██████▍      42%`, 12 cellules en huitièmes de bloc, dégradé
  bougie → sang par cellule, % coloré (80+ sang, 40+ bougie, sinon cendre).
  Largeur fixe, comme le TUI. -->
<script lang="ts">
  import { CRYPTE_THEME } from "../contracts";

  interface Props {
    used: number;
    window: number;
  }

  let { used, window }: Props = $props();

  const EIGHTHS = [" ", "▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];
  const FEAR_W = 12;

  function lerp(a: string, b: string, t: number): string {
    const pa = [1, 3, 5].map((i) => parseInt(a.slice(i, i + 2), 16));
    const pb = [1, 3, 5].map((i) => parseInt(b.slice(i, i + 2), 16));
    const mix = pa.map((v, i) => Math.round(v + (pb[i] - v) * t));
    return `#${mix.map((v) => v.toString(16).padStart(2, "0")).join("")}`;
  }

  let max = $derived(Math.max(1, window));
  let ratio = $derived(used / max);
  let filled = $derived(Math.min(1, ratio) * FEAR_W);
  let cells = $derived(
    Array.from({ length: FEAR_W }, (_, i) => {
      const part = Math.min(1, Math.max(0, filled - i));
      const glyph = EIGHTHS[Math.round(part * 8)];
      const color = lerp(
        CRYPTE_THEME.candle,
        CRYPTE_THEME.blood,
        i / (FEAR_W - 1),
      );
      return { glyph, color };
    }),
  );
  let pct = $derived(Math.min(999, Math.floor((used * 100) / max)));
  let pctColor = $derived(
    pct >= 80
      ? CRYPTE_THEME.blood
      : pct >= 40
        ? CRYPTE_THEME.candle
        : CRYPTE_THEME.ash,
  );
</script>

<span
  class="fear"
  role="meter"
  aria-label="peur (contexte)"
  aria-valuemin={0}
  aria-valuemax={100}
  aria-valuenow={Math.min(100, pct)}
  data-testid="fear-gauge"
>
  <span class="fear-label">fear</span>
  <span class="fear-track" aria-hidden="true"
    >{#each cells as cell}<span style:color={cell.color}>{cell.glyph}</span
      >{/each}</span
  >
  <span class="fear-pct" style:color={pctColor}>{String(pct).padStart(3, " ")}%</span>
</span>

<style>
  .fear {
    display: inline-flex;
    align-items: baseline;
    gap: 0.5ch;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .fear-label {
    color: var(--cry-sand);
  }
  .fear-track {
    background: var(--cry-track);
    letter-spacing: 0;
  }
  .fear-pct {
    font-weight: 700;
    white-space: pre;
  }
</style>
