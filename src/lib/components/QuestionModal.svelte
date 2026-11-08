<!-- QuestionModal — l'IA pose une question (bloc ```question).
  Miroir de question.rs render() : intro, options numérotées, rappel.
  Réponse au chiffre (1-6), au clic, ou en texte libre ; Esc = null. -->
<script lang="ts">
  import type { PendingQuestion } from "../contracts";
  import { shell } from "../crypte.svelte";

  interface Props {
    question: PendingQuestion;
  }

  let { question }: Props = $props();
  let free = $state("");

  function answer(text: string | null): void {
    shell.answerQuestion(text);
  }

  function onKey(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      answer(null);
      return;
    }
    const n = parseInt(event.key, 10);
    if (n >= 1 && n <= question.options.length) {
      answer(question.options[n - 1]);
      return;
    }
    if (event.key === "Enter" && free.trim()) {
      answer(free.trim());
    }
  }

  function submit(event: Event): void {
    event.preventDefault();
    if (free.trim()) answer(free.trim());
  }
</script>

<svelte:window onkeydown={onKey} />

<div
  class="veil"
  data-testid="question-modal"
  role="dialog"
  aria-modal="true"
  aria-label="La crypte vous interroge"
>
  <div class="card">
    <p class="intro">— the crypt asks you: {question.question}</p>
    <ol class="options">
      {#each question.options as option, i (i)}
        <li>
          <button
            class="option"
            onclick={() => answer(option)}
            data-testid={`question-option-${i + 1}`}
          >
            <span class="digit">{i + 1}</span>
            <span>{option}</span>
          </button>
        </li>
      {/each}
    </ol>
    <form class="free" onsubmit={submit}>
      <input
        class="crypte-input"
        placeholder="…or reply in your own words"
        bind:value={free}
        aria-label="Réponse libre"
        data-testid="question-free"
      />
      <button class="crypte-btn ghost" type="submit">send</button>
    </form>
  </div>
</div>

<style>
  .veil {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(12, 9, 8, 0.82);
    padding: 1rem;
  }
  .card {
    width: min(34rem, 100%);
    background: var(--cry-crypt);
    border: 1px solid var(--cry-candle-dark);
    border-radius: 0.5rem;
    padding: 1rem 1.25rem;
  }
  .intro {
    margin: 0 0 0.75rem;
    color: var(--cry-candle);
  }
  .options {
    list-style: none;
    margin: 0 0 0.75rem;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  .option {
    width: 100%;
    display: flex;
    align-items: baseline;
    gap: 0.75rem;
    text-align: left;
    background: var(--cry-ink);
    color: var(--cry-bone);
    border: 1px solid var(--cry-umber);
    border-radius: 0.375rem;
    padding: 0.45rem 0.7rem;
  }
  .option:hover {
    border-color: var(--cry-candle);
  }
  .digit {
    flex: none;
    width: 1.4rem;
    height: 1.4rem;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: 999px;
    background: var(--cry-candle);
    color: var(--cry-void);
    font-weight: 700;
    font-size: 0.85em;
  }
  .free {
    display: flex;
    gap: 0.5rem;
  }
  .free .crypte-input {
    flex: 1;
    min-width: 0;
  }
</style>
