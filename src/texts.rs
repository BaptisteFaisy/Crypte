//! Texte d'aide (`/help`) : commandes + touches + contexte.
//!
//! Extrait de `commands.rs` (limite 500 lignes) : pur, sans état.

/// Aide : les 33 commandes + rappel des touches + contexte agents.md.
pub fn help_text() -> &'static str {
    "Commands:\n\
     /usage — token usage (chat + session + all-time almanac)\n\
     /new — new chat (resets the current chat)\n\
     /clear — clear the current chat conversation\n\
     /bestiary — show a random creature in the side panel\n\
     /thinking [level] — reasoning level (default/low/medium/high)\n\
     /model [name|reset] — show or change the model\n\
     /agent [@<n>] <text> — sub-agent (reply goes to chat n if targeted)\n\
     /agents [all] — list sub-agents (all = every chat)\n\
     /ask #<id> <text> — follow up with an agent\n\
     /tell <n> <text> — talk to chat n\n\
     /msg <n|@n|#id|all> <text> — talk to a chat, an agent, all\n\
     /link <a> <b> — replies from a go to b (auto)\n\
     /links — list auto links\n\
     /unlink <a> <b> — remove a link\n\
     /compact — compact the current chat history\n\
     /connect [openai|muse] — connect (status if no argument)
     /settings [name] — providers screen: encrypted API keys
     /settings add <name> <base_url> [model] — custom provider\n\
     /permissions [yolo|auto|<tool> <rule>|list|reset] — tools mode + per-tool rules (remembered)\n\
     /web <query> — web search + summary with cited sources\n\
     /tools [on|off] — model tools: browser, desktop, shell (default on)\n\
     /kiro — kiro .kiro/ status (steering, specs, hooks, mcp)\n\
     /spec <name> [run] | /spec new <name> <idea> — kiro spec workflow\n\
     /mcp [refresh] — MCP servers: status, tools (refresh reconnects)\n\
     /hooks — kiro hooks by trigger + recent firings\n\
     /skills — list reusable skills (name + description)\n\
     /skill <name> [text] — inject a skill atop the next send\n\
     /checkpoints — list file snapshots (newest first)\n\
     /rewind <n|name> — restore a checkpoint\n\
     /remember <text> — memorize into .kiro/MEMORY.md\n\
     /critic [on|off] — Kiro roams and judges your prompts (click to hit him)\n\
     /vps [ip|name|off|status|add|del|install] — run this TUI on a VPS\n\
     /help — this help\n\
     Questions: the AI may ask you — reply with the number or in your own words\n\
     Keys: F5 scare · F6 shop · F9 haunt (game) · Ctrl+N · Ctrl+W close (+ click)\n\
     Shop (F6 or tab click): 1 token consumed = 1 coin, ambiance perks\n\
     Context: ./AGENTS.md loaded at startup (restart to reload)"
}
