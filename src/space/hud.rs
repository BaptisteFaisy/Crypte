//! HUD flottant au-dessus du monde 3D : titre d'étape, caption « qui fait
//! quoi », bloc métriques — le tout cuit dans l'image (DESIGN §3, §5).
//!
//! Tout est dérivé de [`SceneState`] : aucune donnée supplémentaire ne
//! circule, le HUD raconte ce que la scène sait déjà.

use image::{Rgba, RgbaImage};

use super::paint::{accent, ink};
use super::SceneState;
use super::Step;
use crate::hub::font;

/// Dessine le HUD complet dans le cadre `w × h` (positions du DESIGN §3
/// ramenées de la référence 1440×900).
pub fn draw(cv: &mut RgbaImage, state: &SceneState, now: f64) {
    let (w, h) = (cv.width() as i64, cv.height() as i64);
    step_title(cv, w * 620 / 1440, h * 104 / 900, state, now);
    caption(cv, w * 620 / 1440, h * 806 / 900, state);
    metrics(cv, w * 1288 / 1440, h * 104 / 900, state);
}

/// Le grand titre : « 04 / 06 · ACT », ou « IDLE » au repos.
pub fn step_title(cv: &mut RgbaImage, x: i64, y: i64, state: &SceneState, now: f64) {
    let (idx, label) = match state.step {
        Some(s) => (format!("{:02} / 06 · NOW", s.rank()), s.label().to_string()),
        None => ("-- / 06".to_string(), "IDLE".to_string()),
    };
    font::paint(cv, x, y, &idx, 2, |_| ink(0.45));
    let big = 6;
    font::paint(cv, x, y + 16, &label, big, |row| {
        ink(0.95 - row as f32 * 0.06)
    });
    let _ = now;
}

/// La caption `[WHO] what happens` (DESIGN §5).
pub fn caption(cv: &mut RgbaImage, x: i64, y: i64, state: &SceneState) {
    let (who, what) = story(state);
    let chip = chip_color(&who);
    font::paint(cv, x, y, &format!("[{who}]"), 2, |_| chip);
    font::paint(
        cv,
        x + font::width(&format!("[{who}]")) * 2 + 8,
        y,
        &what,
        2,
        |_| ink(0.8),
    );
}

/// Qui agit, et quoi — déduit de l'état (DESIGN §5).
fn story(state: &SceneState) -> (String, String) {
    if let Some(k) = &state.kiro {
        if k.done_at.is_none() {
            let f = k
                .file
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "the prompt".to_string());
            return ("kiro".to_string(), format!("judges {f}"));
        }
    }
    if let Some((id, s)) = state.subs.iter().find(|(_, s)| s.done_at.is_none()) {
        let t = match &s.target {
            super::Target::Path(p) => p.display().to_string(),
            super::Target::Web => "the web".to_string(),
        };
        return ("sub-agents".to_string(), format!("#{id} works {t}"));
    }
    match (state.step, state.main_file.as_ref()) {
        (Some(Step::Answer), _) => ("main ghost".to_string(), "answers".to_string()),
        (_, Some(p)) => {
            let verb = if state.main_wrote { "Patches" } else { "Reads" };
            ("main ghost".to_string(), format!("{verb} {}", p.display()))
        }
        (Some(s), None) => ("main ghost".to_string(), s.label().to_lowercase()),
        (None, None) => ("you".to_string(), "ask the crypt…".to_string()),
    }
}

/// Couleur de la puce `who` (DESIGN §5).
fn chip_color(who: &str) -> Rgba<u8> {
    match who {
        "you" => ink(1.0),
        "main ghost" => ink(0.9),
        _ => accent(0.9),
    }
}

/// Le bloc métriques à droite : 6 lignes + règle + barre orange (CTX).
pub fn metrics(cv: &mut RgbaImage, x: i64, y: i64, state: &SceneState) {
    let changed = state.files.values().filter(|f| f.changed).count();
    let slabs: u32 = state.files.values().map(|f| f.slabs).sum();
    let rows: [(String, String); 6] = [
        ("STEP".to_string(), step_name(state)),
        ("FILES".to_string(), changed.to_string()),
        ("SLABS".to_string(), slabs.to_string()),
        ("SUBS".to_string(), state.subs.len().to_string()),
        ("CKPT".to_string(), state.checkpoint.to_string()),
        (
            "VIEW".to_string(),
            if state.hub { "HUB" } else { "PROJ" }.to_string(),
        ),
    ];
    for (i, r) in rows.iter().enumerate() {
        let yy = y + i as i64 * 22;
        for k in 0..18 {
            crate::space::raster::over(cv, x - 6, yy + k, ink(0.3), 1.0);
        }
        font::paint(cv, x, yy, &r.0, 2, |_| ink(0.4));
        font::paint(cv, x, yy + 10, &r.1, 2, |_| ink(0.9));
    }
    let bar = (changed.min(20) as i64) * 4;
    for k in 0..bar {
        crate::space::raster::over(cv, x + k, y + 6 * 22, accent(1.0), 1.0);
    }
}

/// Nom de l'étape courante pour le bloc métriques.
fn step_name(state: &SceneState) -> String {
    state
        .step
        .map(|s| s.label().to_string())
        .unwrap_or_else(|| "-".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn img() -> RgbaImage {
        RgbaImage::from_pixel(480, 270, Rgba([5, 5, 5, 255]))
    }

    fn state() -> SceneState {
        let mut s = SceneState::new();
        s.step = Some(Step::Act);
        s.main_file = Some(PathBuf::from("src/a.rs"));
        s.main_wrote = true;
        s
    }

    #[test]
    fn le_titre_affiche_l_étape() {
        let mut cv = img();
        step_title(&mut cv, 40, 20, &state(), 1.0);
        assert!(cv.pixels().any(|p| p[0] > 100), "titre visible");
    }

    #[test]
    fn la_caption_dit_qui_fait_quoi() {
        let (who, what) = story(&state());
        assert_eq!(who, "main ghost");
        assert!(what.starts_with("Patches"));
        let mut cv = img();
        caption(&mut cv, 40, 200, &state());
        assert!(cv.pixels().any(|p| p[0] > 60));
    }

    #[test]
    fn kiro_et_les_sous_agents_passent_dans_la_caption() {
        let mut s = state();
        s.kiro = Some(super::super::KiroState {
            file: None,
            started_at: 0.0,
            done_at: None,
            pushed: false,
        });
        assert_eq!(story(&s).0, "kiro");
        s.kiro = None;
        s.subs.insert(
            2,
            super::super::SubState {
                kind: super::super::AgentKind::Web,
                target: super::super::Target::Web,
                spawned_at: 0.0,
                done_at: None,
                pushed: false,
            },
        );
        let (who, what) = story(&s);
        assert_eq!(who, "sub-agents");
        assert!(what.contains("web"));
    }

    #[test]
    fn les_metriques_se_dessinent() {
        let mut cv = img();
        metrics(&mut cv, 400, 20, &state());
        assert!(cv.pixels().any(|p| p[0] > 60));
    }

    #[test]
    fn le_hud_tient_dans_le_cadre() {
        let mut cv = img();
        draw(&mut cv, &state(), 1.0);
        draw(&mut cv, &SceneState::new(), 1.0);
    }
}
