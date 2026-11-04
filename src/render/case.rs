//! Every mapping is one character to one: the sweep counts characters, so a `ß`
//! that became `SS` would put every later highlight one letter out.

use serde::Deserialize;
use unicode_normalization::char::{compose, decompose_canonical};
use unicode_script::{Script, UnicodeScript};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Case {
    #[default]
    Upper,
    Lower,
}

impl Case {
    pub fn apply(self, text: &str) -> String {
        match self {
            Case::Upper => text.chars().map(upper).collect(),
            Case::Lower => {
                let chars: Vec<char> = text.chars().collect();
                let letter = |i: Option<usize>| {
                    i.and_then(|i| chars.get(i))
                        .is_some_and(|c| c.is_alphabetic())
                };
                (0..chars.len())
                    .map(|i| match chars[i] {
                        'Σ' if letter(i.checked_sub(1)) && !letter(Some(i + 1)) => 'ς',
                        c => lower(c),
                    })
                    .collect()
            }
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Case::Upper => "upper",
            Case::Lower => "lower",
        }
    }

    /// The other one, for the `c` key.
    pub fn next(self) -> Case {
        match self {
            Case::Upper => Case::Lower,
            Case::Lower => Case::Upper,
        }
    }
}

impl std::str::FromStr for Case {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "upper" => Ok(Case::Upper),
            "lower" => Ok(Case::Lower),
            _ => Err(format!("unknown case `{s}`; expected upper or lower")),
        }
    }
}

fn upper(c: char) -> char {
    // The capital eszett exists, and is what a sign painter would use.
    if c == 'ß' {
        return 'ẞ';
    }
    // Greek in capitals drops its accents but keeps the diaeresis: ά is Α and
    // ΐ is Ϊ.
    let c = match c.script() {
        Script::Greek => without_accents(c),
        _ => c,
    };
    single(c.to_uppercase()).unwrap_or(c)
}

fn without_accents(c: char) -> char {
    let mut parts = Vec::new();
    decompose_canonical(c, |d| parts.push(d));
    parts[1..]
        .iter()
        .filter(|m| **m == '\u{308}')
        .fold(parts[0], |base, m| compose(base, *m).unwrap_or(base))
}

fn lower(c: char) -> char {
    single(c.to_lowercase()).unwrap_or(c)
}

fn single(mut it: impl Iterator<Item = char>) -> Option<char> {
    match (it.next(), it.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIXED: &str = "Straße İstanbul ŉ ǅ Ǆ ﬁ Señor ΐΰ";

    #[test]
    fn every_case_keeps_the_character_count() {
        for case in [Case::Upper, Case::Lower] {
            assert_eq!(
                case.apply(MIXED).chars().count(),
                MIXED.chars().count(),
                "{case:?}"
            );
        }
    }

    #[test]
    fn the_cases_do_what_they_say() {
        assert_eq!(Case::Upper.apply("Señor ß"), "SEÑOR ẞ");
        assert_eq!(Case::Lower.apply("Señor ẞ"), "señor ß");
    }

    #[test]
    fn greek_capitals_lose_their_accents_but_not_the_diaeresis() {
        assert_eq!(Case::Upper.apply("Σ' αγαπώ, ΐ ϋ"), "Σ' ΑΓΑΠΩ, Ϊ Ϋ");
    }

    #[test]
    fn a_capital_sigma_ending_a_word_lowers_to_final_sigma() {
        assert_eq!(Case::Lower.apply("ΟΔΟΣ ΣΑΣ, Σ'"), "οδος σας, σ'");
    }

    #[test]
    fn the_key_toggles_between_the_two() {
        assert_eq!(Case::Upper.next(), Case::Lower);
        assert_eq!(Case::Lower.next(), Case::Upper);
    }

    #[test]
    fn labels_parse_back() {
        for case in [Case::Upper, Case::Lower] {
            assert_eq!(case.label().parse::<Case>(), Ok(case));
        }
        assert!("original".parse::<Case>().is_err());
    }
}
