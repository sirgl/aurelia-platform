//! Language tags: normalising, the fallback chain and picking a bundle from what the
//! operating system asks for. Pure string work; the platform layers read the OS settings
//! (environment variables, the browser's `navigator.languages`, Android's `Locale`) and
//! hand the tags here.

/// The tag of the base language: the one every other bundle falls back to.
pub const BASE: &str = "en";
/// The pseudo-locale: English expanded with accents and brackets, to find text that
/// overflows or is not localised.
pub const PSEUDO: &str = "en-XA";

/// A tag in canonical case (`ru`, `zh-Hans`, `pt-BR`), or None for the C/POSIX locale and
/// text that is no tag. Accepts `ru_RU.UTF-8`, `de_DE@euro` and `zh-hans-cn`.
pub fn normalize(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let raw = raw.split(['.', '@']).next().unwrap_or("");
    if raw.is_empty() || raw.eq_ignore_ascii_case("c") || raw.eq_ignore_ascii_case("posix") {
        return None;
    }
    let mut parts = raw.split(['-', '_']);
    let language = parts.next()?;
    if !(2..=3).contains(&language.len()) || !language.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut out = language.to_ascii_lowercase();
    for part in parts {
        if part.is_empty() || !part.chars().all(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        out.push('-');
        match part.len() {
            4 if part.chars().all(|c| c.is_ascii_alphabetic()) => {
                out.push_str(&part[..1].to_ascii_uppercase());
                out.push_str(&part[1..].to_ascii_lowercase());
            }
            2 | 3 => out.push_str(&part.to_ascii_uppercase()),
            _ => out.push_str(part),
        }
    }
    Some(out)
}

/// The tags to try for `tag`, most specific first, ending with the base language:
/// `pt-BR` -> `pt-BR`, `pt`, `en`. Chinese maps by script: `zh-CN`/`zh` -> `zh-Hans`;
/// `zh-TW`/`zh-HK` -> `zh-Hant`, then `zh-Hans`.
pub fn chain(tag: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |t: String| {
        if !out.contains(&t) {
            out.push(t);
        }
    };
    let tag = normalize(tag).unwrap_or_else(|| BASE.to_owned());
    if tag.eq_ignore_ascii_case(PSEUDO) {
        push(PSEUDO.to_owned());
        push(BASE.to_owned());
        return out;
    }
    let language = tag.split('-').next().unwrap_or("").to_owned();
    if language == "zh" {
        let traditional =
            tag.contains("Hant") || ["-TW", "-HK", "-MO"].iter().any(|r| tag.ends_with(r));
        if traditional {
            push(tag.clone());
            push("zh-Hant".to_owned());
        } else {
            push(tag.clone());
        }
        push("zh-Hans".to_owned());
    } else {
        push(tag.clone());
        let mut cut = tag.as_str();
        while let Some(at) = cut.rfind('-') {
            cut = &cut[..at];
            push(cut.to_owned());
        }
        if language == "no" {
            push("nb".to_owned());
        }
    }
    push(BASE.to_owned());
    out
}

/// The tags the operating system asks for, from the usual environment variables
/// (`LANGUAGE` first: a colon-separated list; then `LC_ALL`, `LC_MESSAGES`, `LANG`), most
/// wanted first. `get` reads one variable.
pub fn from_env(get: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"] {
        let Some(value) = get(name) else { continue };
        for item in value.split(':') {
            if let Some(tag) = normalize(item) {
                if !out.contains(&tag) {
                    out.push(tag);
                }
            }
        }
    }
    out
}

/// The first of `requested` that has a bundle in `available` (itself or a fallback of it
/// other than the base language), or the base language.
pub fn negotiate(requested: &[String], available: &[String]) -> String {
    for tag in requested {
        for candidate in chain(tag) {
            if candidate == BASE {
                // The base language is the answer only when it is what was asked for.
                if normalize(tag)
                    .as_deref()
                    .map(|t| t == BASE || t.starts_with("en-"))
                    == Some(true)
                {
                    return BASE.to_owned();
                }
                continue;
            }
            if available.iter().any(|a| *a == candidate) {
                return candidate;
            }
        }
    }
    BASE.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_are_normalised() {
        assert_eq!(normalize("ru_RU.UTF-8").as_deref(), Some("ru-RU"));
        assert_eq!(normalize("DE_de@euro").as_deref(), Some("de-DE"));
        assert_eq!(normalize("zh-hans-cn").as_deref(), Some("zh-Hans-CN"));
        assert_eq!(normalize("C"), None);
        assert_eq!(normalize("POSIX"), None);
        assert_eq!(normalize(""), None);
        assert_eq!(normalize("x"), None);
        assert_eq!(normalize("../etc"), None);
    }

    #[test]
    fn the_chain_ends_in_english() {
        assert_eq!(chain("pt-BR"), ["pt-BR", "pt", "en"]);
        assert_eq!(chain("de"), ["de", "en"]);
        assert_eq!(chain("en"), ["en"]);
        assert_eq!(chain("en-XA"), ["en-XA", "en"]);
        assert_eq!(chain("zh-CN"), ["zh-CN", "zh-Hans", "en"]);
        assert_eq!(chain("zh"), ["zh", "zh-Hans", "en"]);
        assert_eq!(chain("zh-TW"), ["zh-TW", "zh-Hant", "zh-Hans", "en"]);
        assert_eq!(chain("garbage!"), ["en"]);
    }

    #[test]
    fn the_environment_is_read_in_order() {
        let env = |name: &str| match name {
            "LANGUAGE" => Some("fr_CA:de".to_owned()),
            "LANG" => Some("ru_RU.UTF-8".to_owned()),
            _ => None,
        };
        assert_eq!(from_env(env), ["fr-CA", "de", "ru-RU"]);
        assert!(from_env(|_| None).is_empty());
    }

    #[test]
    fn negotiation_picks_the_first_available() {
        let available: Vec<String> = ["en", "de", "ru", "zh-Hans"].map(String::from).to_vec();
        let ask = |tags: &[&str]| {
            negotiate(
                &tags.iter().map(|t| t.to_string()).collect::<Vec<_>>(),
                &available,
            )
        };
        assert_eq!(ask(&["de-AT", "fr"]), "de");
        assert_eq!(ask(&["fr", "ru-RU"]), "ru");
        assert_eq!(ask(&["zh-CN"]), "zh-Hans");
        assert_eq!(ask(&["zh-TW"]), "zh-Hans");
        assert_eq!(ask(&["fr"]), "en");
        assert_eq!(ask(&["en-GB", "de"]), "en");
        assert_eq!(ask(&[]), "en");
    }
}
