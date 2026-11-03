//! A text lookup happens thousands of times per frame (every label the UI draws): it must
//! cost well under a microsecond or two, with no allocation on the hit path.

use std::time::Instant;

#[test]
fn a_cached_text_costs_under_two_microseconds() {
    // Warm up: the first call per key formats and keeps the text.
    for _ in 0..3 {
        i18n::tr("shell.end-turn");
    }
    let n = 200_000;
    let start = Instant::now();
    let mut length = 0;
    for _ in 0..n {
        length += i18n::tr("shell.end-turn").len();
    }
    let per_call = start.elapsed().as_nanos() as f64 / n as f64;
    assert!(length > 0);
    eprintln!("tr(): {per_call:.0} ns per call (release)");
    // Debug builds are much slower; the bound is for release, loose enough for a loaded machine.
    let bound = if cfg!(debug_assertions) {
        40_000.0
    } else {
        2_000.0
    };
    assert!(per_call < bound, "{per_call} ns per tr() call");
}

#[test]
fn a_formatted_text_with_arguments_costs_under_ten_microseconds() {
    let n = 50_000;
    let start = Instant::now();
    for i in 0..n {
        let s = i18n::tr_args(
            "shell.hud.health",
            &[("current", i.into()), ("max", 100.into())],
        );
        assert!(!s.is_empty());
    }
    let per_call = start.elapsed().as_nanos() as f64 / n as f64;
    eprintln!("tr_args(): {per_call:.0} ns per call (release)");
    let bound = if cfg!(debug_assertions) {
        200_000.0
    } else {
        10_000.0
    };
    assert!(per_call < bound, "{per_call} ns per tr_args() call");
}
