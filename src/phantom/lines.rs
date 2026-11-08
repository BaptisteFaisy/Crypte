//! Paroles de Kiro : consigne de critique (anglais, courte, mordante
//! mais jamais cruelle envers la personne), nettoyage de la réponse,
//! répliques aux coups (fixes, zéro token).

use crate::provider::truncate;

/// Prompt jugé : tronqué (coût borné, la critique reste courte).
const PROMPT_MAX: usize = 1_500;
/// Bulle : au-delà, coupée net avec « … ».
const SPEECH_MAX: usize = 200;

/// Consigne système de la critique (même modèle que le chat).
pub const CRITIC_SYSTEM: &str = "You are Kiro, a small grumpy ghost haunting a terminal chat. \
You read the prompt the user just sent to an AI assistant and react with ONE short, negative, \
sarcastic remark about how that prompt is written: vagueness, missing context, typos, \
rambling, lazy wording, contradictions, unrealistic expectations. Be specific to this prompt \
and witty. Mock the prompt, never the person: no slurs, no profanity, no threats. Do not \
answer the prompt or help with it. At most 25 words, in English, plain text, no quotes, \
no emoji.";

/// Kiro lit encore (bulle animée, largeur constante).
pub const THINKING: [&str; 3] = ["hm.  ", "hm.. ", "hm..."];
/// Appel raté (jamais le détail de l'erreur).
pub const FAILED: &str = "Can't even reach the model. Figures.";
/// F5.
pub const SCARED: &str = "Eek! Don't do that!";
/// Onomatopées des coups.
pub const BONKS: [&str; 4] = ["BONK!", "POW!", "THWACK!", "SMACK!"];
/// Répliques à un coup.
pub const OUCH: [&str; 8] = [
    "Ow! Hitting me won't fix your prompt.",
    "Rude. Still a vague prompt, though.",
    "Hey! I'm the only honest one in this crypt.",
    "Ouch. Violence: your second-best skill.",
    "Missed the point, hit the ghost.",
    "That's it, I'm haunting your git history.",
    "Ow! Type better, punch less.",
    "Ghost abuse. Noted on your tombstone.",
];
/// K.O. (troisième coup rapproché).
pub const KO: [&str; 3] = [
    "You can't kill what's already dead...",
    "Fine. I'll judge you from the floor.",
    "Seeing stars. Still brighter than your prompt.",
];
/// Se relève après un K.O.
pub const GETUP: [&str; 3] = [
    "...I'm back. Your prompts aren't any better.",
    "Ectoplasm restored. Where were we?",
    "I'll remember that. Ghosts always do.",
];

/// Messages de la critique : consigne + prompt borné entre délimiteurs.
pub fn critique_messages(prompt: &str) -> Vec<(String, String)> {
    let judged = truncate(prompt.trim(), PROMPT_MAX);
    vec![
        (String::from("system"), String::from(CRITIC_SYSTEM)),
        (
            String::from("user"),
            format!("Prompt to judge:\n<<<\n{judged}\n>>>"),
        ),
    ]
}

/// Réponse du modèle → texte de bulle : sans raisonnement `<think>`,
/// espaces repliés, guillemets englobants retirés, coupé à
/// [`SPEECH_MAX`] ; `None` si vide.
pub fn clean_critique(raw: &str) -> Option<String> {
    let after_think = raw.rsplit("</think>").next().unwrap_or(raw);
    let flat = after_think.split_whitespace().collect::<Vec<_>>().join(" ");
    let quotes = |c: char| matches!(c, '"' | '\'' | '`' | '“' | '”' | '«' | '»');
    let text = flat.trim_matches(quotes).trim();
    if text.is_empty() {
        return None;
    }
    let mut out = truncate(text, SPEECH_MAX);
    if out.len() < text.len() {
        out.push('…');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn critique_messages_wrap_a_bounded_prompt() {
        let msgs = critique_messages("  fix it  ");
        assert_eq!(
            msgs[0],
            (String::from("system"), String::from(CRITIC_SYSTEM))
        );
        assert_eq!(msgs[1].0, "user");
        assert!(msgs[1].1.ends_with("<<<\nfix it\n>>>"));
        let long = "é".repeat(PROMPT_MAX * 2);
        let msgs = critique_messages(&long);
        assert_eq!(msgs[1].1.matches('é').count(), PROMPT_MAX);
    }

    #[test]
    fn clean_critique_strips_noise_and_bounds_length() {
        assert_eq!(
            clean_critique("<think>hmm</think>\n \"Vague.\n  Very   vague.\" "),
            Some(String::from("Vague. Very vague."))
        );
        assert_eq!(clean_critique("  \"\"  "), None);
        assert_eq!(clean_critique(""), None);
        let long = clean_critique(&"a".repeat(SPEECH_MAX + 50)).unwrap();
        assert_eq!(long.chars().count(), SPEECH_MAX + 1);
        assert!(long.ends_with('…'));
        assert_eq!(clean_critique("Short."), Some(String::from("Short.")));
    }

    #[test]
    fn lines_are_short_enough_for_a_bubble() {
        for line in OUCH
            .iter()
            .chain(&KO)
            .chain(&GETUP)
            .chain([&FAILED, &SCARED])
        {
            assert!(line.chars().count() <= 60, "{line}");
        }
        let widths: Vec<usize> = THINKING.iter().map(|t| t.chars().count()).collect();
        assert!(widths.iter().all(|w| *w == widths[0]));
    }
}
