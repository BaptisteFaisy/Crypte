//! Environnement du processus vu par les tests.
//!
//! `std::env` est global au processus : un test qui pose `KH_KIRO_PROJECT`
//! change la lecture d'un autre test qui tourne en parallèle (constaté
//! entre `agents::tests::merged_joint_agents_et_steering_always` et
//! `app::ctors::tests::lazy_absent_sans_bruit_ni_reessai`).
//!
//! Un seul verrou pour tout le crate + [`EnvScope`], qui restaure les
//! variables suivies en `Drop` (même quand le test panique).

use std::ffi::{OsStr, OsString};
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Verrou unique de l'environnement. Empoisonnement ignoré : un test qui
/// panique ne doit pas condamner les suivants.
static LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Emprise sur l'environnement : verrouille tant qu'elle vit et restaure
/// les variables suivies en sortie.
pub(crate) struct EnvScope {
    _lock: MutexGuard<'static, ()>,
    saved: Vec<(&'static str, Option<OsString>)>,
}

impl EnvScope {
    /// Verrouille l'environnement et prend la main sur `keys`.
    /// `&[]` = verrou seul, pour un test qui se contente de lire.
    pub(crate) fn new(keys: &[&'static str]) -> Self {
        let lock = LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let saved = keys
            .iter()
            .map(|key| (*key, std::env::var_os(key)))
            .collect();
        Self { _lock: lock, saved }
    }

    /// Pose `key` (déjà suivie) à `value`.
    pub(crate) fn set(&self, key: &str, value: impl AsRef<OsStr>) {
        self.check(key);
        std::env::set_var(key, value);
    }

    /// Retire `key` (déjà suivie).
    pub(crate) fn unset(&self, key: &str) {
        self.check(key);
        std::env::remove_var(key);
    }

    /// Une variable non déclarée à `new` échapperait à la restauration.
    fn check(&self, key: &str) {
        debug_assert!(
            self.saved.iter().any(|(saved, _)| *saved == key),
            "variable non suivie par EnvScope : {key}"
        );
    }
}

impl Drop for EnvScope {
    fn drop(&mut self) {
        for (key, value) in self.saved.drain(..) {
            match value {
                Some(saved) => std::env::set_var(key, saved),
                None => std::env::remove_var(key),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une clé par test : le verrou ne protège pas la mise en place
    /// initiale (faite avant `EnvScope`), deux tests sur la même clé se
    /// marcheraient dessus.
    const ABSENT: &str = "KH_TEST_ENV_SCOPE_ABSENT";
    const PRESENT: &str = "KH_TEST_ENV_SCOPE_PRESENT";

    #[test]
    fn set_puis_drop_restaure_valeur_absente() {
        std::env::remove_var(ABSENT);
        {
            let env = EnvScope::new(&[ABSENT]);
            env.set(ABSENT, "pendant");
            assert_eq!(std::env::var(ABSENT).as_deref(), Ok("pendant"));
        }
        assert!(std::env::var(ABSENT).is_err(), "valeur initiale (absente)");
    }

    #[test]
    fn unset_puis_drop_restaure_valeur_initiale() {
        std::env::set_var(PRESENT, "avant");
        {
            let env = EnvScope::new(&[PRESENT]);
            env.unset(PRESENT);
            assert!(std::env::var(PRESENT).is_err());
        }
        assert_eq!(std::env::var(PRESENT).as_deref(), Ok("avant"));
        std::env::remove_var(PRESENT);
    }
}
