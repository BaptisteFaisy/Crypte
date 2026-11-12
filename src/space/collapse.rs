//! Effondrement des dalles créées depuis le checkpoint (`/rewind`).
//!
//! Chaque fichier concerné perd ses dalles une à une, du sommet vers la
//! base : 120 ms de décalage par fichier, 70 ms par dalle, chute à
//! gravité 900 (DESIGN §7). Fonctions pures de `(vague, rang, now)`.

use std::path::Path;

use super::RewindState;

/// Décalage entre deux fichiers (s).
const FILE_STAGGER: f64 = 0.12;
/// Décalage entre deux dalles d'un même fichier (s).
const SLAB_STAGGER: f64 = 0.07;
/// Gravité de la chute (unités/s²).
const GRAV: f64 = 900.0;
/// Durée de chute d'une dalle avant disparition (s).
const GONE: f64 = 0.6;
/// Plafond de dalles animées par fichier (au-delà, tout part d'un coup).
const MAX_ANIM: f64 = 64.0;

/// Rang du fichier dans la vague (`None` s'il n'est pas concerné).
pub fn rank(rw: &RewindState, path: &Path) -> Option<usize> {
    rw.files.iter().position(|f| f == path)
}

/// Vrai tant que la vague anime encore quelque chose.
pub fn active(rw: &RewindState, now: f64) -> bool {
    let n = (rw.files.len() as f64).min(MAX_ANIM);
    now < rw.at + n * FILE_STAGGER + MAX_ANIM * SLAB_STAGGER + GONE
}

/// Dalles déjà parties à `t` (la dalle `k` tombe à `k·SLAB_STAGGER` et
/// disparaît `GONE` plus tard).
fn gone_count(t: f64, from: u32) -> u32 {
    if t < GONE - 1e-9 {
        return 0;
    }
    let g = ((t - GONE + 1e-9) / SLAB_STAGGER).floor() as i64 + 1;
    g.clamp(0, from as i64) as u32
}

/// Dalles encore visibles d'un fichier concerné à `now` (jamais sous `floor`).
pub fn visible_slabs(rw: &RewindState, rank: usize, now: f64, floor: u32) -> u32 {
    let from = rw.from.get(rank).copied().unwrap_or(floor).max(floor);
    let t = now - rw.at - rank as f64 * FILE_STAGGER;
    if t <= 0.0 {
        return from;
    }
    (from - gone_count(t, from)).max(floor)
}

/// Décalage vertical (négatif) de la dalle du sommet en train de tomber.
pub fn falling_offset(rw: &RewindState, rank: usize, now: f64) -> Option<f32> {
    let from = rw.from.get(rank).copied().unwrap_or(1);
    let t = now - rw.at - rank as f64 * FILE_STAGGER;
    if t <= 0.0 {
        return None;
    }
    let k = gone_count(t, from);
    if k >= from {
        return None;
    }
    let local = t - k as f64 * SLAB_STAGGER;
    if local < 0.0 || local >= GONE {
        return None;
    }
    Some(-(0.5 * GRAV * local * local) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn wave(from: u32) -> RewindState {
        RewindState {
            at: 10.0,
            files: vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")],
            from: vec![from, from],
        }
    }

    #[test]
    fn rang_du_fichier_concerné() {
        let rw = wave(4);
        assert_eq!(rank(&rw, Path::new("b.rs")), Some(1));
        assert_eq!(rank(&rw, Path::new("z.rs")), None);
    }

    #[test]
    fn rien_ne_tombe_avant_la_vague() {
        let rw = wave(4);
        assert_eq!(visible_slabs(&rw, 0, 9.0, 1), 4);
        assert_eq!(falling_offset(&rw, 0, 9.0), None);
    }

    #[test]
    fn les_dalles_partent_une_par_une_du_sommet() {
        let rw = wave(4);
        assert_eq!(visible_slabs(&rw, 0, 10.0, 1), 4);
        assert_eq!(visible_slabs(&rw, 0, 10.0 + GONE, 1), 3);
        assert_eq!(visible_slabs(&rw, 0, 10.0 + GONE + SLAB_STAGGER, 1), 2);
        assert_eq!(
            visible_slabs(&rw, 0, 10.0 + GONE + 3.0 * SLAB_STAGGER, 1),
            1
        );
    }

    #[test]
    fn le_second_fichier_part_en_décalé() {
        let rw = wave(4);
        assert_eq!(visible_slabs(&rw, 1, 10.0 + GONE, 1), 4);
        assert_eq!(visible_slabs(&rw, 1, 10.0 + FILE_STAGGER + GONE, 1), 3);
    }

    #[test]
    fn la_dalle_en_chute_a_un_décalage_négatif() {
        let rw = wave(4);
        let o = falling_offset(&rw, 0, 10.0 + 0.3).expect("en chute");
        assert!(o < 0.0);
        // Toutes les dalles parties : plus rien ne tombe.
        assert_eq!(
            falling_offset(&rw, 0, 10.0 + 3.0 * SLAB_STAGGER + GONE + 0.01),
            None
        );
    }

    #[test]
    fn la_vague_s_éteint() {
        let rw = wave(4);
        assert!(active(&rw, 10.5));
        assert!(!active(
            &rw,
            10.0 + 2.0 * FILE_STAGGER + MAX_ANIM * SLAB_STAGGER + GONE + 0.1
        ));
    }
}
