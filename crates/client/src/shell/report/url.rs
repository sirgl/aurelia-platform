//! The link to GitHub's "new issue" page: the repository, the template, the title and the body,
//! percent-encoded. Nothing is sent: the player sees the page and presses Submit.

/// The repository the "Report a bug" button opens an issue in, as `owner/name`. This is the ONE
/// place to change when the repository moves or becomes public under another name.
pub const ISSUE_REPO: &str = "sirgl/aurelia-classic";
/// The issue template file in `.github/ISSUE_TEMPLATE/` of that repository.
pub const ISSUE_TEMPLATE: &str = "bug_report.md";
/// The body is cut to this many characters before it is encoded.
pub const BODY_CAP: usize = 6000;
/// GitHub refuses a request line longer than about 8 KB. The whole link stays under this many
/// bytes, so a body full of newlines and non-ASCII text (three bytes per escaped byte) still opens.
pub const LINK_BUDGET: usize = 7500;
/// One line of the log is cut to this many characters.
const LINE_CAP: usize = 300;

/// `text` as a URL component: letters, digits and `-._~` stay, every other byte of the UTF-8
/// text becomes `%XX`.
pub fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The "new issue" link of `repo` (`owner/name`).
pub fn issue_url_of(repo: &str, title: &str, body: &str) -> String {
    format!(
        "https://github.com/{repo}/issues/new?template={}&title={}&body={}",
        percent_encode(ISSUE_TEMPLATE),
        percent_encode(title),
        percent_encode(body)
    )
}

/// The "new issue" link of `ISSUE_REPO`.
pub fn issue_url(title: &str, body: &str) -> String {
    issue_url_of(ISSUE_REPO, title, body)
}

/// Lines of the panic trace the issue shows (the report file has all of it).
pub const TRACE_SHOWN: usize = 40;

/// The body of the issue: `head` (the template and the facts, never cut), `trace` (the panic
/// trace, first frames first) and `log` (the lines of the game's log, oldest first). The log gives
/// way first: whole lines go from its old end until the body is within `BODY_CAP` characters and
/// the link within `LINK_BUDGET` bytes for `title` and `repo`; only then does the trace shrink
/// from its far end. The result is exactly what the issue form is filled with.
pub fn fit_body(repo: &str, title: &str, head: &str, trace: &[String], log: &[String]) -> String {
    let cut_line = |line: &String| -> String { line.chars().take(LINE_CAP).collect() };
    let lines: Vec<String> = log.iter().map(cut_line).collect();
    let trace: Vec<String> = trace.iter().take(TRACE_SHOWN).map(cut_line).collect();
    let fits = |body: &str| body.chars().count() <= BODY_CAP && issue_url_of(repo, title, body).len() <= LINK_BUDGET;
    for skipped in 0..=lines.len() {
        let body = with_log(&with_trace(head, &trace), &lines[skipped..], skipped);
        if fits(&body) {
            return body;
        }
    }
    for shown in (0..trace.len()).rev() {
        let body = with_trace(head, &trace[..shown]);
        if fits(&body) {
            return body;
        }
    }
    // Even the header alone is too long: it stays (it is the report), the rest goes.
    head.to_owned()
}

/// `head` and a fenced block of the trace's `lines`.
fn with_trace(head: &str, lines: &[String]) -> String {
    if lines.is_empty() {
        return head.to_owned();
    }
    let mut body = head.trim_end().to_owned();
    body.push_str("\n\n### Trace\n```\n"); // i18n-ok: the public issue is in English
    for line in lines {
        body.push_str(line);
        body.push('\n');
    }
    body.push_str("```\n");
    body
}

/// `head` and a fenced log block of `lines`; `cut` earlier lines are said to be missing.
fn with_log(head: &str, lines: &[String], cut: usize) -> String {
    if lines.is_empty() && cut == 0 {
        return head.to_owned();
    }
    let mut body = head.trim_end().to_owned();
    body.push_str("\n\n### Log\n```\n"); // i18n-ok: the public issue is in English
    if cut > 0 {
        body.push_str(&format!("({cut} earlier lines cut)\n")); // i18n-ok: the public issue is in English
    }
    for line in lines {
        body.push_str(line);
        body.push('\n');
    }
    body.push_str("```\n");
    body
}

/// `text` with the player's home directory written as `~`: `home` itself (as the system names it,
/// with either slash) and any `/Users/<name>/`, `/home/<name>/` or `C:\Users\<name>\` prefix.
/// Nothing else is changed.
pub fn scrub(text: &str, home: Option<&str>) -> String {
    let mut out = text.to_owned();
    if let Some(home) = home.map(|h| h.trim_end_matches(['/', '\\'])).filter(|h| h.len() > 1) {
        for variant in [home.to_owned(), home.replace('\\', "/"), home.replace('/', "\\")] {
            out = out.replace(&variant, "~");
        }
    }
    scrub_user_folders(&out)
}

/// `/Users/name`, `/home/name` and `X:\Users\name` (any case of `Users`, either slash) become `~`.
fn scrub_user_folders(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < text.len() {
        let end = if starts_a_path(text, at) { user_folder_end(&text[at..]).map(|len| at + len) } else { None };
        match end {
            Some(end) => {
                out.push('~');
                at = end;
            }
            None => {
                let next = at + text[at..].chars().next().map_or(1, char::len_utf8);
                out.push_str(&text[at..next]);
                at = next;
            }
        }
    }
    out
}

/// A path may start at `at`: not in the middle of a word or of a longer path (`/opt/home/x`).
fn starts_a_path(text: &str, at: usize) -> bool {
    !text[..at].ends_with(|c: char| c.is_alphanumeric() || matches!(c, '/' | '\\' | '.' | '_' | '-'))
}

/// The length of the user folder at the start of `rest` (`/Users/name`), when there is one.
fn user_folder_end(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let prefix = if rest.starts_with("/Users/") {
        7
    } else if rest.starts_with("/home/") {
        6
    } else if bytes.len() > 9
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
        && rest.get(3..8).is_some_and(|word| word.eq_ignore_ascii_case("users"))
        && matches!(bytes[8], b'\\' | b'/')
    {
        9
    } else {
        return None;
    };
    let name = rest[prefix..].find(|c: char| c.is_whitespace() || matches!(c, '/' | '\\' | '"' | '\'' | ')' | ']' | '>' | ',' | ';')).unwrap_or(rest.len() - prefix);
    (name > 0).then_some(prefix + name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encoding_keeps_unreserved_bytes_and_escapes_the_rest_as_utf8() {
        assert_eq!(percent_encode("Abc-1._~"), "Abc-1._~");
        assert_eq!(percent_encode("a b&c=d#e?f/g\n"), "a%20b%26c%3Dd%23e%3Ff%2Fg%0A");
        assert_eq!(percent_encode("é"), "%C3%A9");
        assert_eq!(percent_encode("日"), "%E6%97%A5");
        assert_eq!(percent_encode("100%"), "100%25");
        assert_eq!(percent_encode("+"), "%2B", "a plus must not read as a space");
        assert_eq!(percent_encode(""), "");
    }

    #[test]
    fn the_link_names_the_repository_the_template_the_title_and_the_body() {
        let link = issue_url_of("owner/repo", "A title", "line 1\nline 2");
        assert_eq!(link, "https://github.com/owner/repo/issues/new?template=bug_report.md&title=A%20title&body=line%201%0Aline%202");
        assert!(issue_url("t", "b").starts_with(&format!("https://github.com/{ISSUE_REPO}/issues/new?template=")));
    }

    #[test]
    fn a_long_log_gives_way_oldest_lines_first_and_the_head_stays_whole() {
        let head = "## What happened\n\n### Info\n- Game: Aurelia 0.2.0\n- Match: seed 7, round 3\n";
        let log: Vec<String> = (1..=20).map(|n| format!("line {n:02} {}", "x".repeat(400))).collect();
        let body = fit_body("owner/repo", "T", head, &[], &log);
        assert!(body.starts_with(head.trim_end()), "{body}");
        assert!(body.chars().count() <= BODY_CAP, "{} chars", body.chars().count());
        assert!(issue_url_of("owner/repo", "T", &body).len() <= LINK_BUDGET);
        assert!(body.contains("line 20") && !body.contains("line 01"), "the newest lines stay");
        assert!(body.contains("earlier lines cut"));
        // A line is cut to 300 characters.
        assert!(!body.contains(&"x".repeat(301)));
        // A short log is kept whole.
        let short = fit_body("owner/repo", "T", head, &[], &["one".into(), "two".into()]);
        assert!(short.contains("one\ntwo\n```") && !short.contains("cut"));
        // No log, no log section.
        assert_eq!(fit_body("owner/repo", "T", head, &[], &[]), head);
    }

    #[test]
    fn the_cap_counts_characters_and_the_link_budget_counts_encoded_bytes() {
        // Newlines and Cyrillic text triple or sextuple in size when encoded: the link budget bites
        // long before 6000 characters.
        let head = "head\n";
        let log: Vec<String> = (0..100).map(|n| format!("строка {n} с кириллицей")).collect();
        let body = fit_body("o/r", "T", head, &[], &log);
        assert!(issue_url_of("o/r", "T", &body).len() <= LINK_BUDGET);
        assert!(body.chars().count() < BODY_CAP);
        assert!(body.contains("кириллицей") && body.starts_with("head"));
        // ASCII text with few escapes meets the 6000-character cap first.
        let ascii: Vec<String> = (0..100).map(|n| format!("lineslineslineslines{n:03}")).collect();
        let body = fit_body("o/r", "T", head, &[], &ascii);
        assert!(body.chars().count() <= BODY_CAP);
        assert!(body.contains("lines099"));
    }

    #[test]
    fn a_head_longer_than_the_cap_is_kept_and_the_log_goes() {
        let head = "h".repeat(BODY_CAP + 500);
        let body = fit_body("o/r", "T", &head, &["frame".into()], &["log line".into()]);
        assert_eq!(body.matches('h').count(), BODY_CAP + 500);
        assert!(!body.contains("log line") && !body.contains("frame"));
    }

    #[test]
    fn the_trace_comes_before_the_log_and_shrinks_only_after_the_log_is_gone() {
        let head = "head\n";
        let trace: Vec<String> = (0..60).map(|n| format!("  {n}: frame_{n}")).collect();
        let log: Vec<String> = (0..5).map(|n| format!("log {n}")).collect();
        let body = fit_body("o/r", "T", head, &trace, &log);
        assert!(body.contains("frame_39") && !body.contains("frame_40"), "the first 40 lines");
        assert!(body.find("### Trace").unwrap() < body.find("### Log").unwrap());
        assert!(body.contains("log 4"));
        // A huge trace line budget: the log is dropped first, then the far frames.
        let wide: Vec<String> = (0..40).map(|n| format!("{n}: {}", "t".repeat(280))).collect();
        let body = fit_body("o/r", "T", head, &wide, &log);
        assert!(body.chars().count() <= BODY_CAP && issue_url_of("o/r", "T", &body).len() <= LINK_BUDGET);
        assert!(!body.contains("log 4") && body.contains("### Trace") && body.starts_with("head"), "{}", body.len());
    }

    #[test]
    fn the_home_directory_becomes_a_tilde() {
        assert_eq!(scrub("/Users/jane/work/saves/a.aurelia", Some("/Users/jane")), "~/work/saves/a.aurelia");
        assert_eq!(scrub("cannot read C:\\Users\\jane\\AppData\\x.cfg", Some("C:\\Users\\jane")), "cannot read ~\\AppData\\x.cfg");
        assert_eq!(scrub("C:/Users/jane/AppData", Some("C:\\Users\\jane")), "~/AppData", "either slash");
        // Without knowing the home, the usual places are still found.
        assert_eq!(scrub("at /Users/bob/.cargo/registry/src/x.rs:1", None), "at ~/.cargo/registry/src/x.rs:1");
        assert_eq!(scrub("(/home/bob/.local/share/aurelia)", None), "(~/.local/share/aurelia)");
        assert_eq!(scrub("d:\\users\\Bob Smith\\x", None), "~ Smith\\x", "a name ends at a space");
        // Other text and other paths stay.
        assert_eq!(scrub("/opt/home/x /usr/lib/y /Users/ /home/", None), "/opt/home/x /usr/lib/y /Users/ /home/");
        assert_eq!(scrub("no paths here: ünïcode ok", Some("/home/z")), "no paths here: ünïcode ok");
        assert_eq!(scrub("/home/jane and /home/jane/x", Some("/home/jane/")), "~ and ~/x");
    }
}
