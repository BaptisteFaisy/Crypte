//! Texte d'aide (`/help`) : commandes + touches + contexte.
//!
//! Extrait de `commands.rs` (limite 500 lignes) : pur, sans état.

/// Aide : les 22 commandes + rappel des touches + contexte agents.md.
pub fn help_text() -> &'static str {
    "Commands:\n\
     /usage — token usage (chat + session + all-time almanac)\n\
     /new — new chat (resets the current chat)\n\
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
     /permissions [yolo|auto] — tools mode: ask or run free (remembered)\n\
     /web <query> — web search + summary with cited sources\n\
     /tools [on|off] — model tools: browser, desktop, shell (default on)\n\
     /kiro — kiro .kiro/ status (steering, specs, hooks, mcp)\n\
     /help — this help\n\
     Keys: F5 scare · F7 hub · Ctrl+N · Ctrl+W close\n\
     Context: ./AGENTS.md loaded at startup (restart to reload)"
}
