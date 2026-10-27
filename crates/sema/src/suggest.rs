//! "Did you mean" suggestions by edit distance.

/// The closest candidate to `word`, if one is close enough to be a likely typo:
/// at most one edit for short words, two for longer ones. Ties go to the first
/// candidate.
pub fn suggest<'a>(word: &str, candidates: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let word = word.to_ascii_lowercase();
    let limit = if word.chars().count() <= 4 { 1 } else { 2 };
    let mut best: Option<(usize, &str)> = None;
    for candidate in candidates {
        if candidate == word {
            continue;
        }
        let distance = levenshtein(&word, candidate);
        if distance <= limit && best.is_none_or(|(d, _)| distance < d) {
            best = Some((distance, candidate));
        }
    }
    best.map(|(_, c)| c)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(ca != *cb))
                .min(row[j] + 1)
                .min(above + 1);
            diagonal = above;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("add", "add"), 0);
    }

    #[test]
    fn suggests_close_words_only() {
        let words = ["mov", "movk", "movz", "ldr", "ldp"];
        assert_eq!(suggest("mvo", words.iter().copied()), None);
        assert_eq!(suggest("movv", words.iter().copied()), Some("mov"));
        assert_eq!(suggest("ldrr", words.iter().copied()), Some("ldr"));
        assert_eq!(suggest("frobnicate", words.iter().copied()), None);
    }
}
