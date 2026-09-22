//! Voice snippets: a spoken cue expands to a saved block of text.
//!
//! Say "my address" and the full address is what gets typed. Longest trigger
//! wins so "my email signature" is not eaten by "my email".

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    pub trigger: String,
    pub expansion: String,
}

/// Replace snippet triggers in `text` with their expansions.
pub fn expand(text: &str, snippets: &[Snippet]) -> String {
    if snippets.is_empty() || text.is_empty() {
        return text.to_string();
    }

    let mut ordered: Vec<&Snippet> = snippets
        .iter()
        .filter(|snippet| !snippet.trigger.trim().is_empty())
        .collect();
    ordered.sort_by_key(|a| std::cmp::Reverse(a.trigger.trim().len()));

    let mut output = text.to_string();
    for snippet in ordered {
        output = replace_phrase(&output, snippet.trigger.trim(), &snippet.expansion);
    }
    output
}

fn replace_phrase(haystack: &str, trigger: &str, expansion: &str) -> String {
    let lower = haystack.to_lowercase();
    let needle = trigger.to_lowercase();
    let mut result = String::with_capacity(haystack.len());
    let mut rest = haystack;
    let mut rest_lower = lower.as_str();

    while let Some(found) = rest_lower.find(&needle) {
        let before = &rest[..found];
        let after_start = found + needle.len();
        let left_ok = found == 0 || !rest.as_bytes()[found - 1].is_ascii_alphanumeric();
        let right_ok = after_start == rest.len()
            || !rest
                .as_bytes()
                .get(after_start)
                .is_some_and(|b| b.is_ascii_alphanumeric());
        if left_ok && right_ok {
            result.push_str(before);
            result.push_str(expansion);
            rest = &rest[after_start..];
            rest_lower = &rest_lower[after_start..];
        } else {
            result.push_str(&rest[..after_start]);
            rest = &rest[after_start..];
            rest_lower = &rest_lower[after_start..];
        }
    }
    result.push_str(rest);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snippet(trigger: &str, expansion: &str) -> Snippet {
        Snippet {
            trigger: trigger.into(),
            expansion: expansion.into(),
        }
    }

    #[test]
    fn expands_a_spoken_cue() {
        let text = expand(
            "send it to my address please",
            &[snippet("my address", "12 Weld Lane")],
        );
        assert_eq!(text, "send it to 12 Weld Lane please");
    }

    #[test]
    fn prefers_the_longer_trigger() {
        let text = expand(
            "paste my email signature",
            &[
                snippet("my email", "a@b.com"),
                snippet("my email signature", "Kind regards,\nGert"),
            ],
        );
        assert_eq!(text, "paste Kind regards,\nGert");
    }

    #[test]
    fn ignores_a_partial_word() {
        let text = expand("emailing the client", &[snippet("email", "NO")]);
        assert_eq!(text, "emailing the client");
    }
}
