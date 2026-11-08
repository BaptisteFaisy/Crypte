<!-- CrypteField — champ texte unique (ligne, zone, secret masqué).
     Styles : crypte.css .cry-field*. Le secret n'est jamais loggué. -->
<script lang="ts">
  interface Props {
    value?: string;
    label?: string;
    hint?: string;
    error?: string | null;
    placeholder?: string;
    type?: "text" | "password";
    /** Champ secret (clé API…) : masqué + bouton révéler. */
    secret?: boolean;
    multiline?: boolean;
    rows?: number;
    disabled?: boolean;
    monospace?: boolean;
    id?: string;
    oninput?: (event: Event) => void;
    /** Entrée (ligne) ou Ctrl+Entrée (zone). */
    onEnter?: () => void;
  }

  let {
    value = $bindable(""),
    label,
    hint,
    error = null,
    placeholder,
    type = "text",
    secret = false,
    multiline = false,
    rows = 3,
    disabled = false,
    monospace = false,
    id,
    oninput,
    onEnter,
  }: Props = $props();

  const autoId = $props.id();
  const fieldId = $derived(id ?? autoId);
  const errId = $derived(`${fieldId}-err`);
  let revealed = $state(false);

  const shownType = $derived(secret && !revealed ? "password" : type);

  function handleKey(e: KeyboardEvent): void {
    const want = multiline
      ? e.key === "Enter" && (e.ctrlKey || e.metaKey)
      : e.key === "Enter";
    if (want && onEnter && !disabled) {
      e.preventDefault();
      onEnter();
    }
  }
</script>

<div class="cry-field{monospace ? ' cry-field--mono' : ''}{error ? ' cry-field--error' : ''}">
  {#if label}
    <label class="cry-field__label" for={fieldId}>{label}</label>
  {/if}
  <div class="cry-field__wrap">
    {#if multiline}
      <textarea
        id={fieldId}
        class="cry-field__input"
        bind:value
        {placeholder}
        {rows}
        {disabled}
        aria-invalid={error ? true : undefined}
        aria-describedby={error ? errId : undefined}
        {oninput}
        onkeydown={handleKey}
      ></textarea>
    {:else}
      <input
        id={fieldId}
        class="cry-field__input"
        type={shownType}
        bind:value
        {placeholder}
        {disabled}
        aria-invalid={error ? true : undefined}
        aria-describedby={error ? errId : undefined}
        {oninput}
        onkeydown={handleKey}
      />
    {/if}
    {#if secret}
      <button
        type="button"
        class="cry-btn cry-btn--subtle cry-btn--sm"
        aria-pressed={revealed}
        aria-label={revealed ? "Masquer le secret" : "Révéler le secret"}
        {disabled}
        onclick={() => (revealed = !revealed)}
      >
        {revealed ? "Masquer" : "Voir"}
      </button>
    {/if}
  </div>
  {#if error}
    <p class="cry-field__error" id={errId}>{error}</p>
  {:else if hint}
    <p class="cry-field__hint">{hint}</p>
  {/if}
</div>
