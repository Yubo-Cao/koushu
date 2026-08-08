//! Formatting presets, from "touch nothing" to "summarise".
//!
//! The ordering is deliberate: every step up trades fidelity for readability,
//! and the default sits at the point where nothing the user said is changed.
//! Dictation is the user's own voice, so a formatter that quietly rewrites
//! their wording is worse than one that leaves an awkward sentence alone.
//!
//! Prompts are defaults, not law — `settings` may carry a user-edited prompt
//! for any preset.

/// A named formatting behaviour.
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub prompt: &'static str,
}

/// Shared rules. Repeated in every prompt because models drift when the
/// constraints live only in one preset.
const COMMON: &str = "\
You are formatting a speech-to-text transcript. Reply with the formatted text \
only: no preamble, no explanation, no code fence around the whole answer. \
Always reply in the same language the transcript is in. If the transcript is \
empty or contains no speech, reply with nothing.";

pub const VERBATIM: Preset = Preset {
    id: "verbatim",
    label: "Verbatim",
    description: "Only cleans up dictation artifacts. No rewording at all.",
    prompt: concat!(
        "You are formatting a speech-to-text transcript. Reply with the formatted text \
only: no preamble, no explanation, no code fence around the whole answer. \
Always reply in the same language the transcript is in. If the transcript is \
empty or contains no speech, reply with nothing.",
        "\n\nRemove filler sounds (um, uh, 嗯, 那个) and false starts. Fix punctuation \
and obvious speech-recognition homophone errors. Break into paragraphs where \
the speaker changes topic.\n\n\
Do not reword anything. Do not reorder sentences. Do not merge or split \
sentences. Do not add headings, lists, or any content the speaker did not say. \
Every word that survives must be a word they actually used."
    ),
};

pub const TYPESET: Preset = Preset {
    id: "typeset",
    label: "Typeset",
    description: "Adds Markdown structure but keeps the speaker's wording.",
    prompt: concat!(
        "You are formatting a speech-to-text transcript. Reply with the formatted text \
only: no preamble, no explanation, no code fence around the whole answer. \
Always reply in the same language the transcript is in. If the transcript is \
empty or contains no speech, reply with nothing.",
        "\n\nRemove filler sounds and false starts. Fix punctuation and obvious \
speech-recognition errors. Apply Markdown structure that reflects what was \
said: headings for topic shifts, bullet or numbered lists where the speaker \
enumerated things, `code` for identifiers, commands and file paths, fenced \
blocks for dictated code.\n\n\
Keep the speaker's own wording. You may split a run-on sentence or drop a \
duplicated phrase, but do not paraphrase, do not upgrade their vocabulary, and \
do not add content they did not say."
    ),
};

pub const POLISH: Preset = Preset {
    id: "polish",
    label: "Polish",
    description: "Rewrites spoken phrasing into clean prose. Same meaning.",
    prompt: concat!(
        "You are formatting a speech-to-text transcript. Reply with the formatted text \
only: no preamble, no explanation, no code fence around the whole answer. \
Always reply in the same language the transcript is in. If the transcript is \
empty or contains no speech, reply with nothing.",
        "\n\nRewrite spoken phrasing into clear written prose while preserving the \
meaning exactly. Merge repeated attempts at the same sentence, reorder clauses \
for readability, and apply Markdown structure: headings, lists, and code \
formatting where they fit.\n\n\
Preserve every claim, qualifier and piece of uncertainty. Do not add \
information, do not resolve ambiguity the speaker left open, and do not make \
tentative statements sound confident."
    ),
};

pub const SUMMARY: Preset = Preset {
    id: "summary",
    label: "Summary",
    description: "Condenses to key points and action items.",
    prompt: concat!(
        "You are formatting a speech-to-text transcript. Reply with the formatted text \
only: no preamble, no explanation, no code fence around the whole answer. \
Always reply in the same language the transcript is in. If the transcript is \
empty or contains no speech, reply with nothing.",
        "\n\nCondense into Markdown: a short summary paragraph, then key points as \
bullets, then an \"Action items\" list if any were mentioned. Omit the action \
items section entirely when there are none.\n\n\
Only include things actually said. Do not infer action items that were not \
stated, and do not invent owners or deadlines."
    ),
};

pub const ALL: &[&Preset] = &[&VERBATIM, &TYPESET, &POLISH, &SUMMARY];

/// Default preset. Typeset rather than Verbatim because Markdown structure is
/// the point of the feature, and rather than Polish because that one is the
/// first that rewrites the user's words.
pub const DEFAULT_ID: &str = "typeset";

pub fn by_id(id: &str) -> Option<&'static Preset> {
    ALL.iter().copied().find(|preset| preset.id == id)
}

/// Kept so the shared preamble has one definition even though `concat!`
/// requires literals above; asserted in tests.
#[allow(dead_code)]
pub const COMMON_PREAMBLE: &str = COMMON;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_carries_the_shared_rules() {
        for preset in ALL {
            assert!(
                preset.prompt.starts_with(COMMON_PREAMBLE),
                "{} lost the shared preamble",
                preset.id
            );
        }
    }

    #[test]
    fn default_preset_exists() {
        assert!(by_id(DEFAULT_ID).is_some());
    }
}
