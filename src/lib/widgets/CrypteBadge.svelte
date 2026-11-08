<!-- CrypteBadge — pastille de statut unique.
     Raccourcis sémantiques (types contracts.ts) ou ton+libellé libres.
     Le ton/label explicites gagnent sur la sémantique. -->
<script lang="ts">
  import type {
    AgentStatus,
    Mood,
    Priorite,
    StatutOrdre,
    Verdict,
  } from "../contracts";
  import type { BadgeTone } from "./types";

  interface Props {
    label?: string;
    tone?: BadgeTone;
    prio?: Priorite;
    agentStatus?: AgentStatus;
    mood?: Mood;
    verdict?: Verdict;
    statutOrdre?: StatutOrdre;
  }

  let {
    label,
    tone,
    prio,
    agentStatus,
    mood,
    verdict,
    statutOrdre,
  }: Props = $props();

  function fromPrio(p: Priorite): [BadgeTone, string] {
    if (p === "P0") return ["blood", "P0"];
    if (p === "P1") return ["pumpkin", "P1"];
    return ["wisp", "P2"];
  }

  function fromAgent(s: AgentStatus): [BadgeTone, string] {
    if (s === "running") return ["candle", "en cours"];
    if (s === "done") return ["ecto", "fini"];
    return ["blood", "échec"];
  }

  function fromMood(m: Mood): [BadgeTone, string] {
    if (m === "waiting") return ["candle", "attente"];
    if (m === "running") return ["spectre", "en cours"];
    if (m === "done") return ["ecto", "fini"];
    if (m === "failed") return ["blood", "échec"];
    return ["ash", "oisif"];
  }

  function fromVerdict(v: Verdict): [BadgeTone, string] {
    if (v === "applique") return ["ecto", "appliqué"];
    if (v === "refuse") return ["blood", "refusé"];
    return ["wisp", "lu"];
  }

  function fromStatut(s: StatutOrdre): [BadgeTone, string] {
    if (s === "actif") return ["ecto", "actif"];
    if (s === "expire") return ["candle", "expiré"];
    return ["ash", "révoqué"];
  }

  const resolved: [BadgeTone, string] = $derived.by(() => {
    if (prio) return fromPrio(prio);
    if (agentStatus) return fromAgent(agentStatus);
    if (mood) return fromMood(mood);
    if (verdict) return fromVerdict(verdict);
    if (statutOrdre) return fromStatut(statutOrdre);
    return ["ash", ""];
  });

  const shownTone: BadgeTone = $derived(tone ?? resolved[0]);
  const shownLabel: string = $derived(label ?? resolved[1]);
</script>

<span class="cry-badge cry-badge--{shownTone}">{shownLabel}</span>
