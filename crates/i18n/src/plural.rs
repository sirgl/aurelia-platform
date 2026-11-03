//! CLDR plural rules for whole numbers (and a fallback for decimals).
//!
//! A language has up to six categories; a message that depends on a number picks the
//! variant of the category the language puts the number in: `[one]`, `[few]`, `[many]`...
//! The table covers the languages the game ships and the common ones community bundles are
//! likely to bring; an unknown language uses the English rule (`one`/`other`) and the
//! bundle linter says so.

/// A CLDR plural category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Zero,
    One,
    Two,
    Few,
    Many,
    Other,
}

impl Category {
    pub const ALL: [Category; 6] = [
        Category::Zero,
        Category::One,
        Category::Two,
        Category::Few,
        Category::Many,
        Category::Other,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Category::Zero => "zero",
            Category::One => "one",
            Category::Two => "two",
            Category::Few => "few",
            Category::Many => "many",
            Category::Other => "other",
        }
    }

    pub fn from_name(name: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|c| c.name() == name)
    }
}

/// The plural rule of a language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluralRule {
    /// No plural forms: `other` (Chinese, Japanese, Korean, Vietnamese, Thai, Indonesian).
    Other,
    /// `one` for 1, `other` for the rest (English, German, Dutch, Swedish, Italian, ...).
    OneOther,
    /// `one` for 0 and 1, `other` for the rest (French, Portuguese).
    OneOtherFrench,
    /// Spanish: `one` for 1, `many` for whole millions, `other`.
    Spanish,
    /// East Slavic (Russian, Ukrainian, Belarusian): `one`, `few`, `many`.
    EastSlavic,
    /// Polish: `one`, `few`, `many`.
    Polish,
    /// Czech and Slovak: `one`, `few`, `other` (`many` for fractions).
    Czech,
    /// Arabic: all six.
    Arabic,
    /// Turkish, Hungarian, Greek, Finnish... `one` for 1 and `other` (same as OneOther).
    Turkish,
}

impl PluralRule {
    /// The rule of the base language of `tag` ("ru", "ru-RU", "zh-Hans" -> "zh").
    pub fn for_language(tag: &str) -> PluralRule {
        PluralRule::known(tag).unwrap_or(PluralRule::OneOther)
    }

    /// The rule of `tag`, None when the table does not know the language.
    pub fn known(tag: &str) -> Option<PluralRule> {
        let base = tag
            .split(['-', '_'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        Some(match base.as_str() {
            "zh" | "ja" | "ko" | "vi" | "th" | "id" | "ms" | "my" | "lo" | "km" => {
                PluralRule::Other
            }
            "en" | "de" | "nl" | "sv" | "da" | "nb" | "no" | "nn" | "it" | "fi" | "et" | "ca"
            | "gl" | "bg" | "el" | "hu" | "af" | "sq" | "eu" | "ka" | "ur" | "he" | "ro" => {
                PluralRule::OneOther
            }
            "fr" | "pt" | "hy" | "hi" | "fa" | "bn" => PluralRule::OneOtherFrench,
            "es" => PluralRule::Spanish,
            "ru" | "uk" | "be" => PluralRule::EastSlavic,
            "pl" => PluralRule::Polish,
            "cs" | "sk" => PluralRule::Czech,
            "ar" => PluralRule::Arabic,
            "tr" | "az" | "kk" | "uz" => PluralRule::Turkish,
            _ => return None,
        })
    }

    /// The categories the language uses for whole numbers, `other` always last.
    pub fn categories(self) -> &'static [Category] {
        use Category::*;
        match self {
            PluralRule::Other => &[Other],
            PluralRule::OneOther | PluralRule::OneOtherFrench | PluralRule::Turkish => {
                &[One, Other]
            }
            PluralRule::Spanish => &[One, Many, Other],
            PluralRule::EastSlavic | PluralRule::Polish => &[One, Few, Many, Other],
            PluralRule::Czech => &[One, Few, Other],
            PluralRule::Arabic => &[Zero, One, Two, Few, Many, Other],
        }
    }

    /// The categories a translation must provide. The same as [`categories`](Self::categories)
    /// except `many` in French and Spanish, which only whole millions use: a message may leave
    /// it out and `other` is shown.
    pub fn required(self) -> &'static [Category] {
        use Category::*;
        match self {
            PluralRule::Spanish | PluralRule::OneOtherFrench => &[One, Other],
            other => other.categories(),
        }
    }

    /// The category of the whole number `n` (the sign is ignored).
    pub fn category(self, n: i64) -> Category {
        use Category::*;
        let i = n.unsigned_abs();
        let (m10, m100) = (i % 10, i % 100);
        match self {
            PluralRule::Other => Other,
            PluralRule::OneOther | PluralRule::Turkish => {
                if i == 1 {
                    One
                } else {
                    Other
                }
            }
            PluralRule::OneOtherFrench => {
                if i <= 1 {
                    One
                } else if i != 0 && i % 1_000_000 == 0 {
                    Many
                } else {
                    Other
                }
            }
            PluralRule::Spanish => {
                if i == 1 {
                    One
                } else if i != 0 && i % 1_000_000 == 0 {
                    Many
                } else {
                    Other
                }
            }
            PluralRule::EastSlavic => {
                if m10 == 1 && m100 != 11 {
                    One
                } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                    Few
                } else {
                    Many
                }
            }
            PluralRule::Polish => {
                if i == 1 {
                    One
                } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                    Few
                } else {
                    Many
                }
            }
            PluralRule::Czech => {
                if i == 1 {
                    One
                } else if (2..=4).contains(&i) {
                    Few
                } else {
                    Other
                }
            }
            PluralRule::Arabic => {
                if i == 0 {
                    Zero
                } else if i == 1 {
                    One
                } else if i == 2 {
                    Two
                } else if (3..=10).contains(&m100) {
                    Few
                } else if (11..=99).contains(&m100) {
                    Many
                } else {
                    Other
                }
            }
        }
    }

    /// The category of a number with a fraction (`1.5`): `other` in most languages, `many`
    /// in Czech, `one` in French for 0 <= n < 2.
    pub fn category_fraction(self, x: f64) -> Category {
        use Category::*;
        match self {
            PluralRule::Other => Other,
            PluralRule::OneOtherFrench => {
                if (0.0..2.0).contains(&x) {
                    One
                } else {
                    Other
                }
            }
            PluralRule::Czech => Many,
            _ => Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Category::*;

    fn cats(rule: PluralRule, ns: &[i64]) -> Vec<Category> {
        ns.iter().map(|&n| rule.category(n)).collect()
    }

    #[test]
    fn english_and_germanic_have_one_and_other() {
        assert_eq!(
            cats(PluralRule::for_language("en"), &[0, 1, 2, 21, 100]),
            [Other, One, Other, Other, Other]
        );
        assert_eq!(PluralRule::for_language("de-AT").category(1), One);
    }

    #[test]
    fn russian_has_one_few_many() {
        let ru = PluralRule::for_language("ru");
        assert_eq!(
            cats(
                ru,
                &[1, 2, 4, 5, 11, 12, 14, 21, 22, 25, 101, 111, 112, 104, 0]
            ),
            [One, Few, Few, Many, Many, Many, Many, One, Few, Many, One, Many, Many, Few, Many]
        );
        assert_eq!(ru.categories(), &[One, Few, Many, Other]);
    }

    #[test]
    fn french_counts_zero_as_one_and_spanish_does_not() {
        assert_eq!(
            cats(PluralRule::for_language("fr"), &[0, 1, 2]),
            [One, One, Other]
        );
        assert_eq!(
            cats(PluralRule::for_language("es"), &[0, 1, 2]),
            [Other, One, Other]
        );
        assert_eq!(PluralRule::for_language("fr").category(1_000_000), Many);
    }

    #[test]
    fn chinese_and_japanese_have_no_forms() {
        for tag in ["zh", "zh-Hans", "ja", "ko"] {
            assert_eq!(
                cats(PluralRule::for_language(tag), &[0, 1, 2, 5]),
                [Other; 4],
                "{tag}"
            );
        }
        assert_eq!(PluralRule::for_language("ja").categories(), &[Other]);
    }

    #[test]
    fn polish_czech_arabic() {
        let pl = PluralRule::for_language("pl");
        assert_eq!(
            cats(pl, &[1, 2, 5, 12, 22, 25]),
            [One, Few, Many, Many, Few, Many]
        );
        let cs = PluralRule::for_language("cs");
        assert_eq!(cats(cs, &[1, 3, 5]), [One, Few, Other]);
        let ar = PluralRule::for_language("ar");
        assert_eq!(
            cats(ar, &[0, 1, 2, 3, 11, 100]),
            [Zero, One, Two, Few, Many, Other]
        );
    }

    #[test]
    fn unknown_language_falls_back_to_english_rule() {
        assert_eq!(PluralRule::known("xx"), None);
        assert_eq!(PluralRule::for_language("xx").category(1), One);
    }
}
