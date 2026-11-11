//! Le fond 3D plein écran : une image Kitty placée DERRIÈRE les cellules.
//!
//! Mécanisme vérifié sur Ghostty 1.3.1 (voir le plan d'architecture) : un
//! z-index sous `INT32_MIN/2` fait dessiner l'image sous les fonds de
//! cellules ; le premier envoi place (`a=T`, curseur en 0,0), les suivants
//! remplacent l'image en place (`a=t`). Le texte ratatui (fond `Reset`)
//! reste net par-dessus ; le voile du panneau de chat est cuit dans l'image.

use image::RgbaImage;
use tokio::sync::mpsc::Receiver;

use super::render::Space;
use super::scan::ProjectTree;
use super::{SceneEvent, SceneState};
use crate::hub::live::{base64, png_fast};

/// Sous `INT32_MIN/2` : dessinée sous les fonds de cellules (spec Kitty).
const Z_BELOW: i64 = -1_073_741_825;
/// Identifiant d'image Kitty du fond (hors plage du hub et du jeu).
pub const IMG_ID: u32 = 900_001;
/// Pas de simulation (30 fps visés).
const DT: f32 = 1.0 / 30.0;
/// Taille maximale d'un morceau base64 (comme `hub/live.rs`).
const CHUNK: usize = 4096;

/// La vue 3D : état logique, monde, file d'événements, horloge.
pub struct SpaceView {
    /// Le monde logique (plié depuis les événements).
    pub state: SceneState,
    /// Le monde posé + la caméra.
    pub space: Space,
    /// Rectangle du panneau de chat en pixels plein cadre (voile cuit).
    pub panel: Option<[i64; 4]>,
    rx: Receiver<SceneEvent>,
    now: f64,
    placed: bool,
    half: RgbaImage,
    full: RgbaImage,
}

impl SpaceView {
    /// Vue neuve sur un projet.
    pub fn new(rx: Receiver<SceneEvent>, tree: &ProjectTree) -> Self {
        Self {
            state: SceneState::new(),
            space: Space::new(tree),
            panel: None,
            rx,
            now: 0.0,
            placed: false,
            half: RgbaImage::new(2, 2),
            full: RgbaImage::new(4, 4),
        }
    }

    /// Avale les événements en attente et avance l'horloge d'un pas.
    pub fn pump(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            self.state.apply(&ev, self.now);
        }
        self.now += DT as f64;
    }

    /// Séquence Kitty de la frame courante pour un terminal de `cols × rows`
    /// cellules de `cell` pixels. `None` si le terminal n'est pas assez grand.
    pub fn sequence(&mut self, cols: u16, rows: u16, cell: (u16, u16)) -> Option<String> {
        let bw = u32::from(cols) * u32::from(cell.0);
        let bh = u32::from(rows) * u32::from(cell.1);
        if bw < 16 || bh < 16 {
            return None;
        }
        let (hw, hh) = (bw / 2, bh / 2);
        if self.half.dimensions() != (hw, hh) {
            self.half = RgbaImage::new(hw, hh);
            self.full = RgbaImage::new(bw, bh);
        }
        let panel = self.panel.map(|p| [p[0] / 2, p[1] / 2, p[2] / 2, p[3] / 2]);
        self.space
            .frame(&mut self.half, &self.state, self.now, DT, panel);
        upscale2(&self.half, &mut self.full);
        let png = png_fast(&self.full)?;
        let seq = chunks(&base64(&png), self.placed);
        self.placed = true;
        Some(seq)
    }

    /// Efface l'image du terminal (bascule F8 vers l'arrêt).
    pub fn delete() -> String {
        format!("\x1b_Ga=d,q=2,i={IMG_ID}\x1b\\")
    }
}

/// Séquence Kitty : premier envoi = transmission + placement (curseur en
/// 0,0, z sous les cellules) ; ensuite = remplacement des données en place.
fn chunks(data: &str, placed: bool) -> String {
    let action = if placed { "t" } else { "T" };
    let mut out = String::new();
    if !placed {
        out.push_str("\x1b[H");
    }
    let bytes = data.as_bytes();
    let n = bytes.len().div_ceil(CHUNK);
    for i in 0..n {
        let more = u8::from(i + 1 < n);
        let head = if i == 0 {
            if placed {
                format!("\x1b_Ga={action},f=100,q=2,i={IMG_ID},")
            } else {
                format!("\x1b_Ga={action},f=100,q=2,z={Z_BELOW},i={IMG_ID},")
            }
        } else {
            String::new()
        };
        let slice = &bytes[i * CHUNK..bytes.len().min((i + 1) * CHUNK)];
        out.push_str(&format!(
            "{}m={more};{}\x1b\\",
            head,
            std::str::from_utf8(slice).unwrap_or("")
        ));
    }
    out
}

/// Agrandissement ×2 au plus proche voisin (moitié → plein cadre).
fn upscale2(half: &RgbaImage, full: &mut RgbaImage) {
    let (hw, hh) = half.dimensions();
    for y in 0..hh {
        for x in 0..hw {
            let p = *half.get_pixel(x, y);
            for dy in 0..2u32 {
                for dx in 0..2u32 {
                    full.put_pixel(x * 2 + dx, y * 2 + dy, p);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::scan::{DirNode, FileNode};
    use tokio::sync::mpsc;

    fn tree() -> ProjectTree {
        ProjectTree {
            root: PathBuf::from("/p"),
            dirs: vec![DirNode {
                path: PathBuf::from("src"),
                parent: None,
            }],
            files: vec![FileNode {
                path: PathBuf::from("src/a.rs"),
                dir: 0,
                lines: 300,
            }],
        }
    }

    use std::path::PathBuf;

    #[test]
    fn la_première_frame_place_les_suivantes_remplacent() {
        let (_tx, rx) = mpsc::channel(4);
        let mut v = SpaceView::new(rx, &tree());
        let first = v.sequence(40, 12, (10, 20)).expect("assez grand");
        assert!(first.starts_with("\x1b[H\x1b_Ga=T,"), "placement initial");
        assert!(first.contains("z=-1073741825"));
        let next = v.sequence(40, 12, (10, 20)).expect("assez grand");
        assert!(next.contains("\x1b_Ga=t,"), "remplacement en place");
        assert!(!next.starts_with("\x1b[H"), "pas de re-placement");
    }

    #[test]
    fn trop_petit_ne_produit_rien() {
        let (_tx, rx) = mpsc::channel(4);
        let mut v = SpaceView::new(rx, &tree());
        assert!(v.sequence(1, 1, (10, 20)).is_none());
    }

    #[test]
    fn les_événements_sont_pliés_au_pump() {
        let (tx, rx) = mpsc::channel(4);
        let mut v = SpaceView::new(rx, &tree());
        tx.try_send(SceneEvent::HubOpen).unwrap();
        v.pump();
        assert!(v.state.hub);
    }

    #[test]
    fn suppression_de_l_image() {
        assert!(SpaceView::delete().contains("a=d"));
        assert!(SpaceView::delete().contains(&IMG_ID.to_string()));
    }

    #[test]
    fn upscale_double_chaque_pixel() {
        let mut half = RgbaImage::new(2, 2);
        half.put_pixel(1, 1, image::Rgba([9, 9, 9, 255]));
        let mut full = RgbaImage::new(4, 4);
        upscale2(&half, &mut full);
        assert_eq!(full.get_pixel(3, 3)[0], 9);
        assert_eq!(full.get_pixel(0, 0)[0], 0);
    }
}
