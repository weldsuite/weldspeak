//! Turn a user's edit into vocabulary the next dictation can use.
//!
//! After text is inserted, people often backspace a mangled name and type the
//! real one. That pair — what we wrote, what they meant — is the highest-signal
//! training data a dictation app gets, so it is worth capturing automatically
//! rather than asking them to fill a dictionary form.

use serde::{Deserialize, Serialize};

/// One replacement: the words we inserted, and what the user typed instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Correction {
    pub heard: String,
    pub meant: String,
}

impl Correction {
    pub fn new(heard: impl Into<String>, meant: impl Into<String>) -> Option<Self> {
        let heard = normalize_span(&heard.into())?;
        let meant = normalize_span(&meant.into())?;
        if heard == meant {
            return None;
        }
        Some(Self { heard, meant })
    }
}

const MAX_SPAN: usize = 80;
const MAX_WORDS: usize = 80;

/// Words that are never worth putting in a personal glossary by themselves.
const COMMON: &[&str] = &[
    "the", "and", "for", "with", "this", "that", "from", "have", "will", "would", "should",
    "please", "thanks", "thank", "hello", "there", "here", "they", "them", "then", "than", "when",
    "what", "which", "where", "your", "you", "about", "after", "before", "because", "could",
    "just", "like", "some", "more", "also", "into", "over", "under", "again", "other", "these",
    "those", "been", "being", "were", "was", "are", "not", "but", "had", "has", "its", "our",
    "out", "all", "any", "can", "did", "get", "got", "let", "may", "see", "use", "way", "who",
    "how", "why", "yes", "yeah", "okay", "ok", "well", "really", "very", "much", "make", "made",
    "need", "want", "look", "looks", "good", "great", "right", "left", "next", "last", "first",
    "today", "tomorrow",
];

/// Replacements implied by comparing the inserted dictation to the field later.
pub fn from_edit(inserted: &str, field: &str) -> Vec<Correction> {
    let inserted = inserted.trim();
    let field = field.trim();
    if inserted.is_empty() || field.is_empty() || inserted == field {
        return Vec::new();
    }
    if field.contains(inserted) {
        return Vec::new();
    }

    let old = words(inserted);
    let hay = words(field);
    if old.is_empty() || hay.is_empty() || old.len() > MAX_WORDS {
        return Vec::new();
    }

    let Some(window) = best_window(&old, &hay) else {
        return Vec::new();
    };

    let changed = word_replacements(&old, &window);
    if changed.len() * 2 > old.len().max(1) && old.len() > 4 {
        return Vec::new();
    }

    learnable(changed)
}

/// Replacements the user made inside the dictation, found by diffing the
/// field just after the paste (`before`) against the field later (`after`).
///
/// This is the reliable path, and it is how Wispr Flow's auto-dictionary
/// behaves: the only difference between the two snapshots is the user's own
/// edit, wherever the cursor was — a double-clicked word mid-sentence, a
/// retyped name, a fixed capital. Edits outside the dictation, or ones that
/// spill across its edges, are someone else's text and teach nothing.
pub fn from_field_change(inserted: &str, before: &str, after: &str) -> Vec<Correction> {
    let inserted = inserted.trim();
    if inserted.is_empty() || before == after {
        return Vec::new();
    }

    let (prefix, suffix) = common_ends(before, after);
    let changed_from = prefix;
    let changed_to = before.len() - suffix;

    // A field can contain the same sentence twice; the right copy is the one
    // the edit landed in.
    let Some(start) = before
        .match_indices(inserted)
        .map(|(at, _)| at)
        .find(|&at| at <= changed_from && changed_to <= at + inserted.len())
    else {
        return Vec::new();
    };
    let end = start + inserted.len();

    // Everything past the dictation is untouched, so the edited dictation is
    // the same span shifted by however much the edit grew or shrank it.
    let tail = before.len() - end;
    let Some(edited) = after.get(start..after.len() - tail) else {
        return Vec::new();
    };

    let old = words(inserted);
    let new = words(edited);
    if old.len() > MAX_WORDS * 4 {
        return Vec::new();
    }
    learnable(word_replacements(&old, &new))
}

/// Byte lengths of the common prefix and suffix, on char boundaries and never
/// overlapping in the shorter string.
fn common_ends(left: &str, right: &str) -> (usize, usize) {
    let prefix: usize = left
        .chars()
        .zip(right.chars())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    let room = left.len().min(right.len()) - prefix;
    let mut suffix = 0;
    for (a, b) in left.chars().rev().zip(right.chars().rev()) {
        if a != b || suffix + a.len_utf8() > room {
            break;
        }
        suffix += a.len_utf8();
    }
    (prefix, suffix)
}

/// Keep only replacements that are spelling fixes worth remembering.
fn learnable(pairs: Vec<(String, String)>) -> Vec<Correction> {
    pairs
        .into_iter()
        .filter_map(|(heard, meant)| {
            let heard = trim_edge_punctuation(&heard);
            let meant = trim_edge_punctuation(&meant);
            let correction = Correction::new(heard, meant)?;
            is_learnable(&correction).then_some(correction)
        })
        .take(8)
        .collect()
}

/// Longest heard or meant phrase a correction may span.
///
/// A learned correction is replayed on every later dictation, so it has to be
/// a name or term, not a rephrased clause.
const MAX_CORRECTION_WORDS: usize = 4;

/// Words people swap for grammar, not because the recognizer misheard a name.
/// Learning "their" → "there" would rewrite every future "their".
const HOMOPHONES: &[&str] = &[
    "there", "their", "they're", "theyre", "to", "too", "two", "your", "you're", "youre", "its",
    "it's", "then", "than", "affect", "effect", "were", "where", "we're", "hear", "here", "know",
    "no", "new", "knew", "right", "write", "weather", "whether", "by", "buy", "bye", "for", "four",
    "fore", "one", "won", "a", "an", "the", "and", "is", "was", "are", "i", "it", "of", "in", "on",
    "at", "be", "been", "see", "sea", "so", "sew", "wait", "weight", "week", "weak", "which",
    "witch", "whose", "who's", "accept", "except", "loose", "lose",
];

/// Whether a replacement is a recognizer mistake the user fixed, rather than
/// an edit of what they wanted to say.
///
/// A fix looks like the word that was heard ("in colonel" → "Inconel 625",
/// "cloud code" → "Claude Code"). Changing "John" to "Sarah" is a change of
/// mind; remembering it would silently swap every future "John".
pub fn is_learnable(correction: &Correction) -> bool {
    let heard = &correction.heard;
    let meant = &correction.meant;
    if heard.split_whitespace().count() > MAX_CORRECTION_WORDS
        || meant.split_whitespace().count() > MAX_CORRECTION_WORDS
    {
        return false;
    }

    let meant_lower = meant.to_lowercase();
    if HOMOPHONES.contains(&meant_lower.as_str()) || COMMON.contains(&meant_lower.as_str()) {
        return false;
    }

    let heard_key = spelling_key(heard);
    let meant_key = spelling_key(meant);
    if meant_key.chars().count() < 2 {
        return false;
    }
    // The homophone list is English. In any language, swapping one short
    // lowercase word for another ("dat" → "dit", "le" → "la") is grammar or a
    // change of mind; names and codes carry a capital or a digit.
    let distinctive = meant
        .chars()
        .any(|ch| ch.is_uppercase() || ch.is_ascii_digit());
    if meant_key.chars().count() <= 3 && !distinctive {
        return false;
    }
    if heard_key == meant_key {
        // Same letters and digits: a change of case or spacing teaches
        // something ("iphone" → "iPhone", "weld speak" → "WeldSpeak"); a moved
        // apostrophe or hyphen is grammar.
        return without_punctuation(heard) != without_punctuation(meant);
    }

    let distance = edit_distance(&heard_key, &meant_key);
    let longest = heard_key.chars().count().max(meant_key.chars().count());
    distance * 2 <= longest
}

/// Lowercased letters and digits: what a word sounds like on the page.
fn spelling_key(text: &str) -> String {
    text.chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn without_punctuation(text: &str) -> String {
    text.chars()
        .filter(|ch| ch.is_alphanumeric() || ch.is_whitespace())
        .collect()
}

fn trim_edge_punctuation(text: &str) -> &str {
    text.trim_matches(|ch: char| !ch.is_alphanumeric())
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right: Vec<char> = right.chars().collect();
    let mut prev: Vec<usize> = (0..=right.len()).collect();
    let mut curr = vec![0; right.len() + 1];
    for (i, a) in left.chars().enumerate() {
        curr[0] = i + 1;
        for (j, b) in right.iter().enumerate() {
            let substitute = prev[j] + usize::from(a != *b);
            curr[j + 1] = substitute.min(prev[j + 1] + 1).min(curr[j] + 1);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[right.len()]
}

/// Replacements implied by backspaces and typing after an insertion.
pub fn from_keystrokes(
    inserted: &str,
    backspaces: usize,
    typed: &str,
    undid: bool,
) -> Option<Correction> {
    let inserted = inserted.trim_end();
    let typed = typed.trim();
    if typed.is_empty() {
        return None;
    }

    if undid {
        let heard = inserted.trim();
        if heard.split_whitespace().count() > 12 || typed.split_whitespace().count() > 12 {
            return None;
        }
        return Correction::new(heard, typed).filter(is_learnable);
    }

    if backspaces == 0 {
        return None;
    }

    let chars: Vec<char> = inserted.chars().collect();
    if backspaces > chars.len() {
        return None;
    }
    let delete_at = chars.len() - backspaces;
    let word_start = chars[..delete_at]
        .iter()
        .rposition(|ch| ch.is_whitespace())
        .map(|index| index + 1)
        .unwrap_or(0);
    let heard: String = chars[word_start..].iter().collect();
    let kept: String = chars[word_start..delete_at].iter().collect();
    let meant = format!("{kept}{typed}");
    Correction::new(trim_edge_punctuation(&heard), trim_edge_punctuation(&meant))
        .filter(is_learnable)
}

/// Newest correction for a given `heard` wins; cap the list so it cannot grow without bound.
pub fn merge(list: &mut Vec<Correction>, next: Correction) {
    list.retain(|item| !item.heard.eq_ignore_ascii_case(&next.heard));
    list.insert(0, next);
    list.truncate(100);
}

fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| word.to_string())
        .filter(|word| !word.is_empty())
        .collect()
}

fn best_window(old: &[String], hay: &[String]) -> Option<Vec<String>> {
    if hay.len() <= old.len() + 4 {
        if distance_ratio(old, hay) < 0.5 {
            return Some(hay.to_vec());
        }
        return None;
    }

    let min_len = old.len().saturating_sub(2).max(1);
    let max_len = (old.len() + 4).min(hay.len());
    let mut best: Option<(i32, usize, usize)> = None;

    for len in min_len..=max_len {
        for start in 0..=hay.len() - len {
            let window = &hay[start..start + len];
            let ratio = distance_ratio(old, window);
            let score = (ratio * 1000.0) as i32;
            if best.is_none_or(|(best_score, _, _)| score < best_score) {
                best = Some((score, start, len));
            }
        }
    }

    let (score, start, len) = best?;
    if score > 450 {
        return None;
    }
    Some(hay[start..start + len].to_vec())
}

fn distance_ratio(left: &[String], right: &[String]) -> f32 {
    let denom = left.len().max(right.len()).max(1) as f32;
    1.0 - lcs_len(left, right) as f32 / denom
}

fn lcs_len(left: &[String], right: &[String]) -> usize {
    let mut prev = vec![0usize; right.len() + 1];
    let mut curr = vec![0usize; right.len() + 1];
    for a in left {
        for (j, b) in right.iter().enumerate() {
            curr[j + 1] = if a.eq_ignore_ascii_case(b) {
                prev[j] + 1
            } else {
                curr[j].max(prev[j + 1])
            };
        }
        std::mem::swap(&mut prev, &mut curr);
        curr.fill(0);
    }
    prev[right.len()]
}

/// Replaced runs of words between `old` and `new`, as (heard, meant) pairs.
///
/// Words match exactly, case included: a fix like "cloud code" → "Claude Code"
/// has to stay one pair. Matching "code" to "Code" case-insensitively would
/// split it and teach "code" → "Code" on its own, capitalising every later
/// "code". Pure insertions and deletions are additions, not corrections.
fn word_replacements(old: &[String], new: &[String]) -> Vec<(String, String)> {
    let mut table = vec![vec![0u16; new.len() + 1]; old.len() + 1];
    for i in 0..old.len() {
        for j in 0..new.len() {
            table[i + 1][j + 1] = if old[i] == new[j] {
                table[i][j] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }

    let mut ops = Vec::new();
    let mut i = old.len();
    let mut j = new.len();
    while i > 0 && j > 0 {
        if old[i - 1] == new[j - 1] && table[i][j] == table[i - 1][j - 1] + 1 {
            ops.push(Op::Keep);
            i -= 1;
            j -= 1;
        } else if table[i - 1][j] >= table[i][j - 1] {
            ops.push(Op::Del(old[i - 1].clone()));
            i -= 1;
        } else {
            ops.push(Op::Ins(new[j - 1].clone()));
            j -= 1;
        }
    }
    while i > 0 {
        ops.push(Op::Del(old[i - 1].clone()));
        i -= 1;
    }
    while j > 0 {
        ops.push(Op::Ins(new[j - 1].clone()));
        j -= 1;
    }
    ops.reverse();

    let mut pairs = Vec::new();
    let mut dels = Vec::new();
    let mut inss = Vec::new();
    for op in ops {
        match op {
            Op::Keep => flush(&mut dels, &mut inss, &mut pairs),
            Op::Del(word) => dels.push(word),
            Op::Ins(word) => inss.push(word),
        }
    }
    flush(&mut dels, &mut inss, &mut pairs);
    pairs
}

fn flush(dels: &mut Vec<String>, inss: &mut Vec<String>, pairs: &mut Vec<(String, String)>) {
    if !dels.is_empty() && !inss.is_empty() {
        pairs.push((dels.join(" "), inss.join(" ")));
    }
    dels.clear();
    inss.clear();
}

enum Op {
    Keep,
    Del(String),
    Ins(String),
}

fn normalize_span(text: &str) -> Option<String> {
    let trimmed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.is_empty() || trimmed.len() > MAX_SPAN {
        return None;
    }
    if !trimmed.chars().any(|ch| ch.is_alphanumeric()) {
        return None;
    }
    Some(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learns_a_mangled_name() {
        let learned = from_edit(
            "the weld on the inconel looks good",
            "the weld on the Inconel 625 looks good",
        );
        assert_eq!(
            learned,
            vec![Correction {
                heard: "inconel".into(),
                meant: "Inconel 625".into(),
            }]
        );
    }

    #[test]
    fn ignores_text_the_user_appended() {
        let learned = from_edit("the weld looks good", "the weld looks good. Ship Friday.");
        assert!(learned.is_empty());
    }

    #[test]
    fn ignores_an_unrelated_document() {
        let learned = from_edit(
            "hello there",
            "Minutes of the Tuesday standup about payroll",
        );
        assert!(learned.is_empty());
    }

    #[test]
    fn learns_from_backspacing_the_last_word() {
        let learned = from_keystrokes("the weld on the inconel", 7, "Inconel 625", false);
        assert_eq!(
            learned,
            Some(Correction {
                heard: "inconel".into(),
                meant: "Inconel 625".into(),
            })
        );
    }

    #[test]
    fn learns_a_full_undo_when_the_utterance_is_short() {
        let learned = from_keystrokes("in colonel", 0, "Inconel 625", true);
        assert_eq!(
            learned,
            Some(Correction {
                heard: "in colonel".into(),
                meant: "Inconel 625".into(),
            })
        );
    }

    fn correction(heard: &str, meant: &str) -> Correction {
        Correction {
            heard: heard.into(),
            meant: meant.into(),
        }
    }

    #[test]
    fn learns_a_word_fixed_mid_sentence() {
        // The user double-clicked a word in the middle and retyped it: no
        // backspaces at the end, so only the field diff can see this.
        let before = "Notes: Send the spec to the cloud code team today. Thanks";
        let after = "Notes: Send the spec to the Claude Code team today. Thanks";
        let learned =
            from_field_change("Send the spec to the cloud code team today.", before, after);
        assert_eq!(learned, vec![correction("cloud code", "Claude Code")]);
    }

    #[test]
    fn learns_several_fixes_in_one_dictation() {
        let inserted = "ask wisper flow and the inconel supplier";
        let after = "ask Wispr Flow and the Inconel supplier";
        let learned = from_field_change(inserted, inserted, after);
        assert_eq!(
            learned,
            vec![
                correction("wisper flow", "Wispr Flow"),
                correction("inconel", "Inconel"),
            ]
        );
    }

    #[test]
    fn ignores_edits_outside_the_dictation() {
        let before = "Dear team, the weld looks good.";
        let after = "Dear all, the weld looks good.";
        assert!(from_field_change("the weld looks good.", before, after).is_empty());
    }

    #[test]
    fn ignores_an_edit_that_spills_past_the_dictation() {
        let before = "Intro. the weld looks good. Outro.";
        let after = "Intro. the weld looks great, all done.";
        assert!(from_field_change("the weld looks good.", before, after).is_empty());
    }

    #[test]
    fn picks_the_copy_of_a_repeated_sentence_that_was_edited() {
        let before = "check inconel. check inconel.";
        let after = "check inconel. check Inconel 625.";
        assert_eq!(
            from_field_change("check inconel.", before, after),
            vec![correction("inconel", "Inconel 625")]
        );
    }

    #[test]
    fn handles_multibyte_text_around_the_edit() {
        let before = "Café — the weldspeek app — ok";
        let after = "Café — the WeldSpeak app — ok";
        assert_eq!(
            from_field_change("the weldspeek app", before, after),
            vec![correction("weldspeek", "WeldSpeak")]
        );
    }

    #[test]
    fn a_change_of_mind_is_not_learned() {
        // Remembering this would swap every future "John" for "Sarah".
        assert!(
            from_field_change("send it to John", "send it to John", "send it to Sarah").is_empty()
        );
        assert!(!is_learnable(&correction("Tuesday", "Friday")));
    }

    #[test]
    fn grammar_fixes_are_not_learned() {
        for (heard, meant) in [
            ("their", "there"),
            ("its", "it's"),
            ("to", "too"),
            ("your", "you're"),
        ] {
            assert!(
                !is_learnable(&correction(heard, meant)),
                "{heard} → {meant}"
            );
        }
    }

    #[test]
    fn short_word_swaps_in_other_languages_are_not_learned() {
        for (heard, meant) in [("dat", "dit"), ("der", "die"), ("le", "la")] {
            assert!(
                !is_learnable(&correction(heard, meant)),
                "{heard} → {meant}"
            );
        }
        // A short term with a capital is a name worth keeping.
        assert!(is_learnable(&correction("aws", "AWS")));
    }

    #[test]
    fn learns_a_non_english_name() {
        let inserted = "stuur het naar de weld speak groep";
        let after = "stuur het naar de WeldSpeak groep";
        assert_eq!(
            from_field_change(inserted, inserted, after),
            vec![correction("weld speak", "WeldSpeak")]
        );
    }

    #[test]
    fn rewording_a_clause_is_not_learned() {
        assert!(!is_learnable(&correction(
            "we should probably ship this on friday",
            "let's ship on Friday",
        )));
    }

    #[test]
    fn recognizer_slips_and_capitalisation_are_learned() {
        for (heard, meant) in [
            ("in colonel", "Inconel 625"),
            ("cooper netties", "Kubernetes"),
            ("iphone", "iPhone"),
            ("weld speak", "WeldSpeak"),
        ] {
            assert!(is_learnable(&correction(heard, meant)), "{heard} → {meant}");
        }
    }

    #[test]
    fn punctuation_around_a_fixed_word_is_not_part_of_the_term() {
        let learned = from_field_change(
            "check the inconel.",
            "check the inconel.",
            "check the Inconel 625.",
        );
        assert_eq!(learned, vec![correction("inconel", "Inconel 625")]);
    }

    #[test]
    fn newest_correction_for_the_same_heard_word_wins() {
        let mut list = vec![Correction {
            heard: "inconel".into(),
            meant: "Inconel".into(),
        }];
        merge(
            &mut list,
            Correction {
                heard: "inconel".into(),
                meant: "Inconel 625".into(),
            },
        );
        assert_eq!(list[0].meant, "Inconel 625");
        assert_eq!(list.len(), 1);
    }
}
