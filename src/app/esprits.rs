//! Panneau « hub des esprits » : la scène pixel art de `crate::hub`
//! dans le slot keeper (haut du panneau droit), affichage seul.

use ratatui::{
    layout::Rect,
    style::Style,
    widgets::{Block, BorderType, Borders},
    Frame,
};

use super::App;
use crate::hub::{self, Signal};
use crate::sessions::AgentStatus;
use crate::theme::{self, BONE, SPECTRE, UMBER};

impl App {
    /// Frame complète + image Kitty animée du hub à écrire après (si le hub
    /// est affiché dans un terminal Kitty).
    pub(crate) fn render_frame(&mut self, frame: &mut Frame) -> Option<String> {
        self.draw(frame);
        self.hub.view.take_pending()
    }

    /// Scène pixel des esprits dans `area` (slot keeper, sans liste).
    pub(crate) fn draw_hub(&mut self, frame: &mut Frame, area: Rect) {
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(theme::lerp(UMBER, SPECTRE, 0.35)))
            .title(theme::title("spirit hub", SPECTRE, BONE));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let cell = self.bank.kitty_cell();
        hub::render::draw(
            &mut self.hub,
            &self.sessions,
            inner,
            frame.buffer_mut(),
            cell,
        );
        self.bank.protect(inner);
    }

    /// Démo hors ligne (`--smoke`, `--apercu`) : 3 chats, des sous-agents
    /// dans tous les états, un chat en attente, des messages en vol.
    pub(crate) fn populate_hub_demo(&mut self) {
        self.add_chat();
        self.add_chat();
        let names = ["Crypt", "Ossuary", "Belfry"];
        for (i, name) in names.iter().enumerate() {
            if let Some(chat) = self.sessions.get_mut(i) {
                chat.name = (*name).to_string();
            }
        }
        let prompts = [
            "dig through the logs",
            "rewrite the test",
            "read the diff",
            "summarize",
            "translate",
        ];
        let owners = [0, 0, 1, 2, 2];
        let mut ids = Vec::new();
        for (prompt, owner) in prompts.iter().zip(owners) {
            ids.extend(self.sessions.add_agent(owner, prompt, Vec::new()));
        }
        for (id, status) in ids
            .iter()
            .zip([AgentStatus::Done, AgentStatus::Failed])
            .skip(1)
        {
            if let Some((_, agent)) = self.sessions.find_agent_mut(*id) {
                agent.status = status;
            }
        }
        if let Some(chat) = self.sessions.get_mut(1) {
            chat.sending = true;
        }
        self.sessions.select(0);
        self.hub.sync(&self.sessions);
        self.hub.signal(Signal::Tell { from: 0, to: 2 });
        if let Some(&id) = ids.first() {
            self.hub.signal(Signal::Deliver { agent: id, to: 1 });
        }
    }
}
