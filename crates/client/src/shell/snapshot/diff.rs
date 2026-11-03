//! A line diff for people: the changed lines with two lines of context around each run,
//! `-` for the kept snapshot and `+` for the frame now, under `@@ line N` headers.

/// Lines of unchanged context shown around a change.
const CONTEXT: usize = 2;
/// Changed lines printed at most (the rest is counted).
const LIMIT: usize = 200;
/// Cells of the longest-common-subsequence table at most; a larger middle shows as one
/// replaced block.
const TABLE_LIMIT: usize = 4_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edit {
    Same,
    Removed,
    Added,
}

/// The difference of `old` and `new` (both line texts), readable in a test failure.
pub(super) fn lines(old: &str, new: &str) -> String {
    let old: Vec<&str> = old.lines().collect();
    let new: Vec<&str> = new.lines().collect();
    let edits = edits(&old, &new);
    render(&edits, &old, &new)
}

/// The edit script: common head and tail, then the longest common subsequence between.
fn edits(old: &[&str], new: &[&str]) -> Vec<Edit> {
    let head = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let tail = old[head..].iter().rev().zip(new[head..].iter().rev()).take_while(|(a, b)| a == b).count();
    let middle = middle_edits(&old[head..old.len() - tail], &new[head..new.len() - tail]);
    let mut edits = vec![Edit::Same; head];
    edits.extend(middle);
    edits.extend(std::iter::repeat_n(Edit::Same, tail));
    edits
}

fn middle_edits(old: &[&str], new: &[&str]) -> Vec<Edit> {
    if old.len().saturating_mul(new.len()) > TABLE_LIMIT {
        return replaced(old.len(), new.len());
    }
    // longest[i][j]: the common subsequence of old[i..] and new[j..].
    let width = new.len() + 1;
    let mut longest = vec![0u32; (old.len() + 1) * width];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            longest[i * width + j] = if old[i] == new[j] {
                longest[(i + 1) * width + j + 1] + 1
            } else {
                longest[(i + 1) * width + j].max(longest[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j, mut edits) = (0, 0, Vec::new());
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && old[i] == new[j] {
            edits.push(Edit::Same);
            (i, j) = (i + 1, j + 1);
        } else if i < old.len() && (j == new.len() || longest[(i + 1) * width + j] >= longest[i * width + j + 1]) {
            edits.push(Edit::Removed);
            i += 1;
        } else {
            edits.push(Edit::Added);
            j += 1;
        }
    }
    edits
}

fn replaced(removed: usize, added: usize) -> Vec<Edit> {
    let mut edits = vec![Edit::Removed; removed];
    edits.extend(std::iter::repeat_n(Edit::Added, added));
    edits
}

/// Hunks of changed lines with their context; numbers are the new file's lines.
fn render(edits: &[Edit], old: &[&str], new: &[&str]) -> String {
    let shown = context_mask(edits);
    let (mut out, mut printed, mut hidden) = (String::new(), 0, 0);
    let (mut i, mut j, mut gap) = (0, 0, true);
    for (at, edit) in edits.iter().enumerate() {
        if !shown[at] {
            gap = true;
        } else if *edit != Edit::Same && printed >= LIMIT {
            hidden += 1;
        } else {
            if gap {
                out.push_str(&format!("@@ line {}\n", j + 1));
                gap = false;
            }
            let (mark, text) = match edit {
                Edit::Same => (' ', new[j]),
                Edit::Removed => ('-', old[i]),
                Edit::Added => ('+', new[j]),
            };
            out.push_str(&format!("{mark} {text}\n"));
            printed += usize::from(*edit != Edit::Same);
        }
        match edit {
            Edit::Same => (i, j) = (i + 1, j + 1),
            Edit::Removed => i += 1,
            Edit::Added => j += 1,
        }
    }
    if hidden > 0 {
        out.push_str(&format!("… and {hidden} more changed lines\n"));
    }
    out
}

/// Which edits print: every change and `CONTEXT` unchanged lines on each side of it.
fn context_mask(edits: &[Edit]) -> Vec<bool> {
    let mut shown = vec![false; edits.len()];
    for (at, edit) in edits.iter().enumerate() {
        if *edit != Edit::Same {
            let from = at.saturating_sub(CONTEXT);
            let to = (at + CONTEXT + 1).min(edits.len());
            shown[from..to].iter_mut().for_each(|line| *line = true);
        }
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_line_shows_with_its_context_and_line_number() {
        let old = "a\nb\nc\nd\ne\nf\ng\n";
        let new = "a\nb\nc\nD\ne\nf\ng\n";
        assert_eq!(lines(old, new), "@@ line 2\n  b\n  c\n- d\n+ D\n  e\n  f\n");
    }

    #[test]
    fn inserted_and_removed_lines_keep_the_common_ones() {
        let old = "head\nkept\ngone\ntail\n";
        let new = "head\nnew\nkept\ntail\n";
        assert_eq!(lines(old, new), "@@ line 1\n  head\n+ new\n  kept\n- gone\n  tail\n");
    }

    #[test]
    fn distant_changes_print_as_separate_hunks() {
        let old: String = (0..20).map(|n| format!("{n}\n")).collect();
        let new = old.replace("3\n", "three\n").replace("17\n", "seventeen\n");
        let diff = lines(&old, &new);
        assert_eq!(diff.matches("@@").count(), 2, "{diff}");
    }
}
