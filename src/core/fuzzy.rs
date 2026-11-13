//! Case-insensitive subsequence ("fuzzy") matching with a small scoring model.
//!
//! Used by the spotlight command palette to rank commands by how well their
//! label / description / id match the typed query, replacing the old
//! substring-`contains` filter.  Mirrors the feel of `VSCode`'s Command Palette
//! without pulling in a dependency.

/// Returns a match score (`Some(score)`, higher = better) if every character of
/// `query` appears in `target` in order (case-insensitive), or `None` if the
/// query is not a subsequence.
///
/// Scoring bonuses:
/// - base `1` per matched char
/// - `+8` when a char matches at the very start of `target`
/// - `+6` when a char matches at a word boundary (separator, or camelCase)
/// - `+4` when a matched char is contiguous with the previous match
pub fn fuzzy_match(query: &str, target: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }

    let q: Vec<char> = query.to_lowercase().chars().collect();
    let t: Vec<char> = target.chars().collect();
    let t_lower: Vec<char> = target.to_lowercase().chars().collect();

    if q.len() > t.len() {
        return None;
    }

    let mut score: i32 = 0;
    let mut qi = 0usize;
    let mut prev_matched: Option<usize> = None;

    for (ti, &tc) in t_lower.iter().enumerate() {
        if qi < q.len() && tc == q[qi] {
            let mut bonus = 1;

            if ti == 0 {
                bonus += 8;
            } else if is_boundary(t[ti - 1], t[ti]) {
                bonus += 6;
            }

            if let Some(p) = prev_matched
                && ti == p + 1
            {
                bonus += 4;
            }

            score += bonus;
            qi += 1;
            prev_matched = Some(ti);
        }
    }

    (qi == q.len()).then_some(score)
}

/// Whether `cur` starts a new word when preceded by `prev`: either a separator
/// character, or a lower→upper camelCase transition.
fn is_boundary(prev: char, cur: char) -> bool {
    if matches!(
        prev,
        ' ' | '-' | '_' | '/' | '\\' | '.' | ':' | '(' | '[' | '{' | ','
    ) {
        return true;
    }
    prev.is_lowercase() && cur.is_uppercase()
}

#[cfg(test)]
mod tests {
    use super::fuzzy_match;

    #[test]
    fn exact_match_scores_highest() {
        let exact = fuzzy_match("hosts", "Hosts").unwrap();
        let partial = fuzzy_match("ho", "Hosts").unwrap();
        assert!(exact > partial);
    }

    #[test]
    fn subsequence_across_words() {
        assert!(fuzzy_match("ct", "Close current tab").is_some());
        assert!(fuzzy_match("clotab", "Close current tab").is_some());
    }

    #[test]
    fn non_match_returns_none() {
        assert!(fuzzy_match("xyz", "Hosts").is_none());
    }

    #[test]
    fn case_insensitive() {
        assert!(fuzzy_match("SFTP", "Open in SFTP").is_some());
    }
}
