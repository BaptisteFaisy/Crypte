//! Vie sociale du hub : messages réels (orbes), bavardages, bulles.

use super::{Bubble, Glyph, Hub, Key, Mood, Orb, Signal, BUBBLE, MEET, MEET_DIST, ORB_SPEED};

impl Hub {
    /// Un message réel part : orbe de l'émetteur vers le destinataire.
    pub fn signal(&mut self, signal: Signal) {
        let (from, to, glyph) = match signal {
            Signal::Tell { from, to } => (Key::Agent(from), Key::Agent(to), Glyph::Alert),
            Signal::Ask { from, agent } => (Key::Agent(from), Key::Sub(agent), Glyph::Ask),
            Signal::Deliver { agent, to } => (Key::Sub(agent), Key::Agent(to), Glyph::Alert),
            Signal::Reply { chat } => {
                self.say(Key::Agent(chat), Glyph::Talk);
                return;
            }
        };
        let Some(start) = self.pos(from) else {
            return;
        };
        self.orbs.push(Orb {
            from: start,
            to,
            t: 0.0,
            on_arrival: glyph,
            trail: Vec::new(),
        });
    }

    /// Deux agents libres qui se croisent s'arrêtent pour bavarder.
    pub(super) fn maybe_meet(&mut self) {
        if self.panic > 0 {
            return;
        }
        let free: Vec<usize> = (0..self.spectres.len())
            .filter(|&i| {
                let s = &self.spectres[i];
                matches!(s.key, Key::Agent(_))
                    && s.mood == Mood::Idle
                    && s.meet == 0
                    && s.cooldown == 0
            })
            .collect();
        for (k, &a) in free.iter().enumerate() {
            for &b in &free[k + 1..] {
                let (sa, sb) = (&self.spectres[a], &self.spectres[b]);
                let (dx, dy) = (sb.x - sa.x, sb.y - sa.y);
                if dx * dx + dy * dy < MEET_DIST * MEET_DIST && self.roll(12) {
                    self.start_meeting(a, b);
                    return;
                }
            }
        }
    }

    /// Lance une conversation : face à face, bulles alternées.
    fn start_meeting(&mut self, a: usize, b: usize) {
        let (ka, kb) = (self.spectres[a].key, self.spectres[b].key);
        let a_left_of_b = self.spectres[a].x < self.spectres[b].x;
        for (i, partner, left) in [(a, kb, !a_left_of_b), (b, ka, a_left_of_b)] {
            let s = &mut self.spectres[i];
            s.meet = MEET;
            s.partner = Some(partner);
            s.left = left;
        }
        self.say(ka, Glyph::Talk);
        let reply = if self.roll(4) {
            Glyph::Love
        } else {
            Glyph::Talk
        };
        self.bubbles.push(Bubble {
            key: kb,
            glyph: reply,
            ttl: BUBBLE + 14,
        });
    }

    /// Fait voler les orbes ; à l'arrivée, bulle + sursaut du destinataire.
    pub(super) fn fly_orbs(&mut self) {
        let mut landed = Vec::new();
        for (k, orb) in self.orbs.iter_mut().enumerate() {
            orb.t += ORB_SPEED;
            if orb.t >= 1.0 {
                landed.push(k);
            }
        }
        for k in landed.into_iter().rev() {
            let orb = self.orbs.remove(k);
            self.say(orb.to, orb.on_arrival);
            if let Some(s) = self.spectres.iter_mut().find(|s| s.key == orb.to) {
                s.jump = 6;
            }
        }
        for k in 0..self.orbs.len() {
            if let Some(p) = self.orb_pos(&self.orbs[k]) {
                let trail = &mut self.orbs[k].trail;
                trail.push(p);
                if trail.len() > 5 {
                    trail.remove(0);
                }
            }
        }
    }

    /// Position courante d'un orbe : (x, y) au sol en dalles + hauteur
    /// d'arc 0..1 (le rendu la convertit en pixels).
    pub fn orb_pos(&self, orb: &Orb) -> Option<(f32, f32, f32)> {
        let (tx, ty) = self.pos(orb.to)?;
        let t = orb.t.clamp(0.0, 1.0);
        let x = orb.from.0 + (tx - orb.from.0) * t;
        let y = orb.from.1 + (ty - orb.from.1) * t;
        Some((x, y, (t * std::f32::consts::PI).sin()))
    }

    /// Bulle `glyph` sur `key` (remplace la précédente).
    pub(super) fn say(&mut self, key: Key, glyph: Glyph) {
        self.bubbles.retain(|b| b.key != key);
        self.bubbles.push(Bubble {
            key,
            glyph,
            ttl: BUBBLE,
        });
    }
}
