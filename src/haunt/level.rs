//! Niveau en dalles 6×6 lu depuis une grille texte (`assets/haunt/`),
//! éditable à la main comme les sprites : une lettre par dalle (terrain,
//! créatures, décor) ; la dernière ligne donne les zones du « directeur »
//! (apparitions continues sous les pas de Kiro).
//!
//! Terrain : `.` air, `#` terre, `=` dalle (traversable par-dessous),
//! `B` briques, `^` pieux d'os, `~` ectoplasme (mortel).
//! Créatures et repères (voir [`ACTORS`]) : `P` départ, `k` reprise,
//! `R` porte de l'arène, `C` cercueil, `h` main de tombe, `b` chauve-souris,
//! `m` momie pressée, `l` Dame blanche, `g` meute de fantômes noirs,
//! `H` cercueil flottant. Décor ([`DECOR`]) : `t` tombe, `y` arbre mort,
//! `x` crâne, `o` citrouille. Zones : `M` momies, `G` fantômes noirs,
//! `B` chauves-souris, `Z` têtes de zombie, `.` calme.

use std::ops::RangeInclusive;

use anyhow::{bail, Result};

use super::body::Rect;

/// Côté d'une dalle (pixels).
pub const TILE: f32 = 6.0;
/// Lettres des créatures et repères (déclenchés en jeu).
pub const ACTORS: &str = "PkRChbmlgH";
/// Lettres du décor (dessiné, sans collision).
pub const DECOR: &str = "tyxo";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tile {
    Air,
    Earth,
    Slab,
    Brick,
    Spikes,
    Ecto,
}

impl Tile {
    fn from_char(ch: char) -> Option<Tile> {
        Some(match ch {
            '.' => Tile::Air,
            '#' => Tile::Earth,
            '=' => Tile::Slab,
            'B' => Tile::Brick,
            '^' => Tile::Spikes,
            '~' => Tile::Ecto,
            _ => return None,
        })
    }

    /// Bloque de tous les côtés (on se tient debout sur les pieux).
    pub fn solid(self) -> bool {
        matches!(self, Tile::Earth | Tile::Brick | Tile::Spikes)
    }
}

/// Zone du directeur d'apparitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Calm,
    Mummies,
    Ghosts,
    Bats,
    Heads,
}

/// Repère posé sur la grille : `x` = centre de sa dalle, `y` = bas de
/// sa dalle (le sol sous un objet posé), en pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mark {
    pub ch: char,
    pub x: f32,
    pub y: f32,
}

pub struct Level {
    pub cols: usize,
    pub rows: usize,
    tiles: Vec<Tile>,
    zones: Vec<Zone>,
    pub marks: Vec<Mark>,
}

impl Level {
    /// Niveau 1, « The Boneyard » (validé par les tests).
    pub fn first() -> Level {
        Level::parse(include_str!("../../assets/haunt/niveau-1.txt")).expect("niveau 1 valide")
    }

    /// Grille texte → niveau (lignes de même largeur, lettres connues,
    /// un départ `P`, une porte d'arène `R`).
    pub fn parse(src: &str) -> Result<Level> {
        let lines: Vec<&str> = src.lines().collect();
        let Some((meta, grid)) = lines.split_last() else {
            bail!("empty level");
        };
        let cols = meta.chars().count();
        if grid.is_empty() || cols == 0 {
            bail!("empty level");
        }
        let mut tiles = Vec::with_capacity(cols * grid.len());
        let mut marks = Vec::new();
        for (r, line) in grid.iter().enumerate() {
            if line.chars().count() != cols {
                bail!(
                    "row {r}: {} columns instead of {cols}",
                    line.chars().count()
                );
            }
            for (c, ch) in line.chars().enumerate() {
                if let Some(tile) = Tile::from_char(ch) {
                    tiles.push(tile);
                    continue;
                }
                if !ACTORS.contains(ch) && !DECOR.contains(ch) {
                    bail!("row {r}, column {c}: unknown {ch:?}");
                }
                tiles.push(Tile::Air);
                let (x, y) = (c as f32 * TILE + TILE / 2.0, (r + 1) as f32 * TILE);
                marks.push(Mark { ch, x, y });
            }
        }
        let zones = meta.chars().map(zone_of).collect::<Result<Vec<_>>>()?;
        if marks.iter().filter(|m| m.ch == 'P').count() != 1 {
            bail!("exactly one start (P) needed");
        }
        if !marks.iter().any(|m| m.ch == 'R') {
            bail!("arena gate (R) missing");
        }
        Ok(Level {
            cols,
            rows: grid.len(),
            tiles,
            zones,
            marks,
        })
    }

    /// Dalle (colonne, ligne) ; murs de briques hors des bords gauche et
    /// droit, air au-dessus et au-dessous (on tombe dans le noir).
    pub fn tile(&self, c: i64, r: i64) -> Tile {
        if c < 0 || c >= self.cols as i64 {
            return Tile::Brick;
        }
        if r < 0 || r >= self.rows as i64 {
            return Tile::Air;
        }
        self.tiles[r as usize * self.cols + c as usize]
    }

    /// Dalle sous le pixel (x, y).
    pub fn at(&self, x: f32, y: f32) -> Tile {
        self.tile((x / TILE).floor() as i64, (y / TILE).floor() as i64)
    }

    pub fn width(&self) -> f32 {
        self.cols as f32 * TILE
    }

    pub fn height(&self) -> f32 {
        self.rows as f32 * TILE
    }

    /// Zone du directeur à l'abscisse `x`.
    pub fn zone(&self, x: f32) -> Zone {
        let c = ((x / TILE).floor().max(0.0) as usize).min(self.cols - 1);
        self.zones[c]
    }

    /// Premier repère de lettre `ch`.
    pub fn mark(&self, ch: char) -> Option<Mark> {
        self.marks.iter().copied().find(|m| m.ch == ch)
    }

    /// Haut du premier sol (plein ou dalle) sous (x, y).
    pub fn ground_below(&self, x: f32, y: f32) -> Option<f32> {
        let c = (x / TILE).floor() as i64;
        let first = (y / TILE).floor().max(0.0) as i64;
        (first..self.rows as i64)
            .find(|&r| {
                let t = self.tile(c, r);
                t.solid() || t == Tile::Slab
            })
            .map(|r| r as f32 * TILE)
    }

    /// Mur touché par `r` qui avance de `vx` : nouvelle abscisse collée.
    pub fn block_x(&self, r: &Rect, vx: f32) -> Option<f32> {
        if vx == 0.0 {
            return None;
        }
        let rows = span(r.y, r.bottom());
        let mut hit: Option<i64> = None;
        for c in span(r.x, r.right()) {
            if rows.clone().any(|row| self.tile(c, row).solid()) {
                hit = Some(match hit {
                    Some(h) if vx > 0.0 => h.min(c),
                    Some(h) => h.max(c),
                    None => c,
                });
            }
        }
        let c = hit? as f32;
        Some(if vx > 0.0 {
            c * TILE - r.w
        } else {
            (c + 1.0) * TILE
        })
    }

    /// Sol traversé ou touché par les pieds en descendant (`old_feet` →
    /// bas de `r`) : haut de la dalle. Les dalles fines portent sauf si
    /// `drop`. Toucher suffit : à 240 Hz un pas descend de moins d'un
    /// centième de pixel, Kiro ne doit pas « décoller » entre deux pas.
    pub fn floor(&self, r: &Rect, old_feet: f32, drop: bool) -> Option<f32> {
        let first = (old_feet / TILE).floor() as i64;
        let last = (r.bottom() / TILE).floor() as i64;
        for row in first..=last {
            let top = row as f32 * TILE;
            if top < old_feet - 0.5 {
                continue;
            }
            for c in span(r.x, r.right()) {
                let t = self.tile(c, row);
                if t.solid() || (t == Tile::Slab && !drop) {
                    return Some(top);
                }
            }
        }
        None
    }

    /// Plafond heurté par le haut de `r` : bas de la dalle.
    pub fn ceiling(&self, r: &Rect) -> Option<f32> {
        let row = (r.y / TILE).floor() as i64;
        span(r.x, r.right())
            .any(|c| self.tile(c, row).solid())
            .then(|| (row + 1) as f32 * TILE)
    }

    /// Danger sous `r` : ectoplasme (dans la boîte) d'abord, puis pieux
    /// (pieds compris : debout dessus, on se blesse).
    pub fn hazard(&self, r: &Rect) -> Option<Tile> {
        let cols = span(r.x + 2.0, r.right() - 2.0);
        let inside = |t: Tile| {
            cols.clone()
                .any(|c| span(r.y, r.bottom()).any(|row| self.tile(c, row) == t))
        };
        if inside(Tile::Ecto) {
            return Some(Tile::Ecto);
        }
        let feet = span(r.y, r.bottom() + 1.0);
        cols.clone()
            .any(|c| feet.clone().any(|row| self.tile(c, row) == Tile::Spikes))
            .then_some(Tile::Spikes)
    }
}

/// Dalles couvertes par l'intervalle de pixels `lo..hi`.
fn span(lo: f32, hi: f32) -> RangeInclusive<i64> {
    (lo / TILE).floor() as i64..=((hi - 0.01) / TILE).floor() as i64
}

/// Lettre de zone → zone du directeur.
fn zone_of(ch: char) -> Result<Zone> {
    Ok(match ch {
        '.' => Zone::Calm,
        'M' => Zone::Mummies,
        'G' => Zone::Ghosts,
        'B' => Zone::Bats,
        'Z' => Zone::Heads,
        _ => bail!("unknown zone {ch:?}"),
    })
}
