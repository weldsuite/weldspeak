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

    changed
        .into_iter()
        .filter_map(|(heard, meant)| Correction::new(heard, meant))
        .take(8)
        .collect()
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
        return Correction::new(heard, typed);
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
    Correction::new(heard, meant)
}

/// Names and jargon from a finished dictation that belong in the glossary.
///
/// Conservative on purpose: adding "Please" or "Today" would drown the terms
/// that actually change recognition. Digits, camel-ish tokens, and mid-sentence
/// capitals are the shapes people bother to put in a dictionary by hand.
pub fn glossary_candidates(text: &str) -> Vec<String> {
    let tokens = words(text);
    let mut found = Vec::new();

    for (index, token) in tokens.iter().enumerate() {
        let trimmed = token.trim_matches(|ch: char| !ch.is_alphanumeric() && ch != '-');
        if trimmed.is_empty()
            || found
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(trimmed))
        {
            continue;
        }
        if trimmed.chars().all(|ch| ch.is_ascii_digit()) && (2..=6).contains(&trimmed.len()) {
            if let Some(last) = found.last_mut() {
                if last.chars().any(|ch| ch.is_alphabetic())
                    && !last.chars().any(|ch| ch.is_ascii_digit())
                {
                    last.push(' ');
                    last.push_str(trimmed);
                    continue;
                }
            }
        }
        if looks_like_glossary_term(trimmed, index == 0) {
            found.push(trimmed.to_string());
        }
        if found.len() == 6 {
            break;
        }
    }

    found
}

/// Newest correction for a given `heard` wins; cap the list so it cannot grow without bound.
pub fn merge(list: &mut Vec<Correction>, next: Correction) {
    list.retain(|item| !item.heard.eq_ignore_ascii_case(&next.heard));
    list.insert(0, next);
    list.truncate(100);
}

pub fn merge_term(list: &mut Vec<String>, term: &str) {
    let Some(term) = normalize_span(term) else {
        return;
    };
    list.retain(|item| !item.eq_ignore_ascii_case(&term));
    list.insert(0, term);
    list.truncate(100);
}

fn looks_like_glossary_term(token: &str, sentence_start: bool) -> bool {
    if token.len() > MAX_SPAN || token.len() < 3 {
        return false;
    }
    let lower = token.to_ascii_lowercase();
    if COMMON.contains(&lower.as_str()) {
        return false;
    }
    if token.chars().any(|ch| ch.is_ascii_digit()) {
        return token.chars().any(|ch| ch.is_ascii_alphabetic());
    }
    if token.contains('-') && token.chars().any(|ch| ch.is_ascii_alphabetic()) {
        return true;
    }
    if sentence_start {
        return false;
    }
    let mut chars = token.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_uppercase() && token.len() >= 4
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

fn word_replacements(old: &[String], new: &[String]) -> Vec<(String, String)> {
    let mut table = vec![vec![0u16; new.len() + 1]; old.len() + 1];
    for i in 0..old.len() {
        for j in 0..new.len() {
            table[i + 1][j + 1] = if old[i].eq_ignore_ascii_case(&new[j]) {
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
        if old[i - 1].eq_ignore_ascii_case(&new[j - 1]) && table[i][j] == table[i - 1][j - 1] + 1 {
            if old[i - 1] == new[j - 1] {
                ops.push(Op::Keep);
            } else {
                ops.push(Op::KeepFix {
                    from: old[i - 1].clone(),
                    to: new[j - 1].clone(),
                });
            }
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
    let mut pending_fix: Option<(String, String)> = None;
    for op in ops {
        match op {
            Op::Keep => {
                flush_fix(&mut pending_fix, &mut pairs);
                flush(&mut dels, &mut inss, &mut pairs);
            }
            Op::KeepFix { from, to } => {
                flush(&mut dels, &mut inss, &mut pairs);
                flush_fix(&mut pending_fix, &mut pairs);
                pending_fix = Some((from, to));
            }
            Op::Del(word) => {
                flush_fix(&mut pending_fix, &mut pairs);
                dels.push(word);
            }
            Op::Ins(word) => {
                if let Some((_, meant)) = pending_fix.as_mut() {
                    meant.push(' ');
                    meant.push_str(&word);
                } else {
                    inss.push(word);
                }
            }
        }
    }
    flush(&mut dels, &mut inss, &mut pairs);
    flush_fix(&mut pending_fix, &mut pairs);
    pairs
}

fn flush_fix(pending: &mut Option<(String, String)>, pairs: &mut Vec<(String, String)>) {
    if let Some((heard, meant)) = pending.take() {
        pairs.push((heard, meant));
    }
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
    KeepFix { from: String, to: String },
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

    #[test]
    fn picks_out_alloy_codes_and_mid_sentence_names() {
        let terms = glossary_candidates("Please inspect weld W12 on the Inconel 625 coupon.");
        assert!(terms.iter().any(|term| term == "W12"));
        assert!(terms.iter().any(|term| term == "Inconel 625"));
        assert!(!terms.iter().any(|term| term == "Please"));
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
