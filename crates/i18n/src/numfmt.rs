//! Numbers and dates in a language's conventions. A small table, not a locale database:
//! the grouping and decimal separators of the base language and a numeric date order.

/// How a language writes numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberFormat {
    pub group: &'static str,
    pub decimal: &'static str,
    /// Digits before the first group separator appears: 4 in most languages (1,234), 5 in
    /// Spanish and Polish (1234 but 12.345).
    pub min_group_digits: u8,
}

/// The order of a numeric date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateOrder {
    /// 10/20/2026
    MonthDayYear,
    /// 20.10.2026
    DayMonthYear,
    /// 2026/10/20
    YearMonthDay,
}

const NNBSP: &str = "\u{202f}";
const NBSP: &str = "\u{a0}";

impl NumberFormat {
    pub fn for_language(tag: &str) -> NumberFormat {
        let base = tag
            .split(['-', '_'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let (group, decimal, min) = match base.as_str() {
            "de" | "it" | "nl" | "id" | "tr" | "da" | "el" | "pt" | "ro" | "vi" => (".", ",", 4),
            "es" => (".", ",", 5),
            "fr" => (NNBSP, ",", 4),
            "ru" | "uk" | "be" | "cs" | "sk" | "sv" | "nb" | "no" | "fi" | "bg" | "hu" | "pl" => {
                (NBSP, ",", if base == "pl" { 5 } else { 4 })
            }
            _ => (",", ".", 4),
        };
        NumberFormat {
            group,
            decimal,
            min_group_digits: min,
        }
    }

    /// `value` with at least `min_fraction` and at most `max_fraction` decimals.
    pub fn format(&self, value: f64, min_fraction: u8, max_fraction: u8) -> String {
        if !value.is_finite() {
            return value.to_string();
        }
        let max_fraction = max_fraction.max(min_fraction) as usize;
        let negative = value < 0.0;
        let mut text = format!("{:.*}", max_fraction, value.abs());
        if max_fraction > min_fraction as usize && text.contains('.') {
            while text.ends_with('0')
                && text.len() - text.find('.').unwrap() - 1 > min_fraction as usize
            {
                text.pop();
            }
            if text.ends_with('.') {
                text.pop();
            }
        }
        let (int_part, frac) = match text.split_once('.') {
            Some((i, f)) => (i.to_owned(), Some(f.to_owned())),
            None => (text.clone(), None),
        };
        let mut out = String::new();
        if negative
            && (int_part.chars().any(|c| c != '0')
                || frac.as_deref().is_some_and(|f| f.chars().any(|c| c != '0')))
        {
            out.push('\u{2212}');
        }
        out.push_str(&self.group_digits(&int_part));
        if let Some(f) = frac {
            out.push_str(self.decimal);
            out.push_str(&f);
        }
        out
    }

    pub fn format_int(&self, value: i64) -> String {
        self.format(value as f64, 0, 0)
    }

    fn group_digits(&self, digits: &str) -> String {
        if digits.len() < self.min_group_digits as usize {
            return digits.to_owned();
        }
        let mut out = String::new();
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i) % 3 == 0 {
                out.push_str(self.group);
            }
            out.push(c);
        }
        out
    }
}

impl DateOrder {
    pub fn for_language(tag: &str) -> (DateOrder, &'static str) {
        let base = tag
            .split(['-', '_'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        match base.as_str() {
            "ja" | "zh" | "ko" | "hu" | "lt" => (DateOrder::YearMonthDay, "/"),
            "de" | "ru" | "uk" | "pl" | "cs" | "sk" | "tr" | "fi" | "nb" | "no" | "da" | "ro"
            | "bg" => (DateOrder::DayMonthYear, "."),
            "fr" | "es" | "it" | "pt" | "el" | "ca" | "vi" | "id" | "ar" | "he" => {
                (DateOrder::DayMonthYear, "/")
            }
            "nl" => (DateOrder::DayMonthYear, "-"),
            "sv" => (DateOrder::YearMonthDay, "-"),
            _ => (DateOrder::MonthDayYear, "/"),
        }
    }

    pub fn format(tag: &str, year: i32, month: u8, day: u8) -> String {
        let (order, sep) = DateOrder::for_language(tag);
        // English writes 10/20/2026; the others pad: 20.10.2026, 2026/10/20.
        let (m, d) = if order == DateOrder::MonthDayYear {
            (month.to_string(), day.to_string())
        } else {
            (format!("{month:02}"), format!("{day:02}"))
        };
        match order {
            DateOrder::MonthDayYear => format!("{m}{sep}{d}{sep}{year}"),
            DateOrder::DayMonthYear => format!("{d}{sep}{m}{sep}{year}"),
            DateOrder::YearMonthDay => format!("{year}{sep}{m}{sep}{d}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_follow_the_language() {
        let en = NumberFormat::for_language("en");
        assert_eq!(en.format_int(1234567), "1,234,567");
        assert_eq!(en.format(1234.5, 0, 2), "1,234.5");
        assert_eq!(en.format(2.0, 1, 2), "2.0");
        assert_eq!(en.format(-12.0, 0, 0), "\u{2212}12");
        let de = NumberFormat::for_language("de-DE");
        assert_eq!(de.format(1234.5, 0, 2), "1.234,5");
        let fr = NumberFormat::for_language("fr");
        assert_eq!(fr.format_int(1234), "1\u{202f}234");
        let ru = NumberFormat::for_language("ru");
        assert_eq!(ru.format(12345.25, 0, 2), "12\u{a0}345,25");
        assert_eq!(ru.format_int(999), "999");
        let es = NumberFormat::for_language("es");
        assert_eq!(
            es.format_int(1234),
            "1234",
            "Spanish groups from five digits"
        );
        assert_eq!(es.format_int(12345), "12.345");
        assert_eq!(NumberFormat::for_language("ja").format_int(10000), "10,000");
        assert_eq!(
            NumberFormat::for_language("zh-Hans").format(0.5, 0, 1),
            "0.5"
        );
    }

    #[test]
    fn dates_follow_the_language() {
        assert_eq!(DateOrder::format("en", 2026, 10, 20), "10/20/2026");
        assert_eq!(DateOrder::format("de", 2026, 10, 5), "05.10.2026");
        assert_eq!(DateOrder::format("fr", 2026, 10, 5), "05/10/2026");
        assert_eq!(DateOrder::format("ja", 2026, 10, 5), "2026/10/05");
        assert_eq!(DateOrder::format("ru", 2026, 1, 2), "02.01.2026");
    }
}
