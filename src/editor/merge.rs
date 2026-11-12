//! Line merge for a file that changed on disk while the buffer was open.
//!
//! The baseline is the text this buffer last loaded or saved. One side moving
//! is not a merge. Both sides moving is a line diff: edits that do not cover
//! the same baseline lines combine, and edits that do are left marked so the
//! buffer is not written back over the other copy.

/// Start of a conflict from this computer. The tab treats this prefix as its own
/// marker, so the wording has to stay stable.
pub const OURS_MARK: &str = "<<<<<<< subtext (this computer)";

const OURS: &str = "<<<<<<< subtext (this computer)\n";
const DIVIDER: &str = "=======\n";
const THEIRS: &str = ">>>>>>> subtext (other copy)\n";

/// Give up the line diff past this many changed lines and mark the whole file.
/// The trace is small for a notes buffer and would grow with the square of a
/// rewrite, which a scratch pad does not need to spend memory on.
const MAX_DISTANCE: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Merge {
    Clean(String),
    Conflict(String),
}

/// `base` is the shared ancestor, `local` is this buffer, `remote` is the file.
pub fn merge(base: &str, local: &str, remote: &str) -> Merge {
    if local == remote || remote == base {
        return Merge::Clean(local.to_string());
    }
    if local == base {
        return Merge::Clean(remote.to_string());
    }
    let base_lines = split_lines(base);
    let local_lines = split_lines(local);
    let remote_lines = split_lines(remote);
    let (Some(local_hunks), Some(remote_hunks)) = (
        hunks(&base_lines, &local_lines),
        hunks(&base_lines, &remote_lines),
    ) else {
        return Merge::Conflict(marked(local, remote));
    };
    apply(&base_lines, &local_hunks, &remote_hunks)
}

fn marked(local: &str, remote: &str) -> String {
    let mut out = String::new();
    out.push_str(OURS);
    push_side(&mut out, local);
    out.push_str(DIVIDER);
    push_side(&mut out, remote);
    out.push_str(THEIRS);
    out
}

fn push_side(out: &mut String, text: &str) {
    out.push_str(text);
    if !text.is_empty() && !text.ends_with('\n') {
        out.push('\n');
    }
}

fn split_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            lines.push(text[start..=index].to_string());
            start = index + 1;
        }
    }
    if start < text.len() {
        lines.push(text[start..].to_string());
    }
    lines
}

#[derive(Clone)]
struct Hunk {
    base_start: usize,
    base_end: usize,
    lines: Vec<String>,
}

enum Op {
    Equal,
    Delete,
    Insert,
}

fn hunks(base: &[String], other: &[String]) -> Option<Vec<Hunk>> {
    let ops = diff(base, other)?;
    let mut hunks = Vec::new();
    let mut base_index = 0;
    let mut other_index = 0;
    let mut op_index = 0;
    while op_index < ops.len() {
        if matches!(ops[op_index], Op::Equal) {
            base_index += 1;
            other_index += 1;
            op_index += 1;
            continue;
        }
        let base_start = base_index;
        let mut lines = Vec::new();
        while op_index < ops.len() && !matches!(ops[op_index], Op::Equal) {
            match ops[op_index] {
                Op::Delete => base_index += 1,
                Op::Insert => {
                    lines.push(other[other_index].clone());
                    other_index += 1;
                }
                Op::Equal => unreachable!(),
            }
            op_index += 1;
        }
        hunks.push(Hunk {
            base_start,
            base_end: base_index,
            lines,
        });
    }
    Some(hunks)
}

/// Myers edit script. `None` when the two sides rewrote too much to trace.
fn diff(a: &[String], b: &[String]) -> Option<Vec<Op>> {
    let n = a.len();
    let m = b.len();
    let max = n + m;
    if max == 0 {
        return Some(Vec::new());
    }
    let offset = max as i32;
    let mut frontier = vec![0i32; 2 * max + 1];
    let mut trace = Vec::new();
    for distance in 0..=max {
        if distance > MAX_DISTANCE {
            return None;
        }
        trace.push(frontier.clone());
        let distance_i = distance as i32;
        for diagonal in (-distance_i..=distance_i).step_by(2) {
            let mut x = if diagonal == -distance_i
                || (diagonal != distance_i
                    && frontier[(offset + diagonal - 1) as usize]
                        < frontier[(offset + diagonal + 1) as usize])
            {
                frontier[(offset + diagonal + 1) as usize]
            } else {
                frontier[(offset + diagonal - 1) as usize] + 1
            };
            let mut y = x - diagonal;
            while x < n as i32 && y < m as i32 && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            frontier[(offset + diagonal) as usize] = x;
            if x >= n as i32 && y >= m as i32 {
                return Some(backtrack(&trace, offset, n, m));
            }
        }
    }
    None
}

fn backtrack(trace: &[Vec<i32>], offset: i32, n: usize, m: usize) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut x = n as i32;
    let mut y = m as i32;
    for (distance, frontier) in trace.iter().enumerate().rev() {
        let distance_i = distance as i32;
        let diagonal = x - y;
        let prev_diagonal = if diagonal == -distance_i
            || (diagonal != distance_i
                && frontier[(offset + diagonal - 1) as usize]
                    < frontier[(offset + diagonal + 1) as usize])
        {
            diagonal + 1
        } else {
            diagonal - 1
        };
        let prev_x = frontier[(offset + prev_diagonal) as usize];
        let prev_y = prev_x - prev_diagonal;
        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
            ops.push(Op::Equal);
        }
        if distance > 0 {
            if x == prev_x {
                y -= 1;
                ops.push(Op::Insert);
            } else {
                x -= 1;
                ops.push(Op::Delete);
            }
        }
    }
    ops.reverse();
    ops
}

fn apply(base: &[String], local: &[Hunk], remote: &[Hunk]) -> Merge {
    let mut out = String::new();
    let mut index = 0;
    let mut local_index = 0;
    let mut remote_index = 0;
    let mut conflict = false;
    let len = base.len();
    while index < len || local_index < local.len() || remote_index < remote.len() {
        let next = match (
            local.get(local_index).map(|hunk| hunk.base_start),
            remote.get(remote_index).map(|hunk| hunk.base_start),
        ) {
            (None, None) => len,
            (Some(start), None) | (None, Some(start)) => start,
            (Some(left), Some(right)) => left.min(right),
        };
        while index < next.min(len) {
            out.push_str(&base[index]);
            index += 1;
        }
        if local_index >= local.len() && remote_index >= remote.len() {
            break;
        }
        let local_here = local
            .get(local_index)
            .is_some_and(|hunk| hunk.base_start == index);
        let remote_here = remote
            .get(remote_index)
            .is_some_and(|hunk| hunk.base_start == index);
        match (local_here, remote_here) {
            (true, false) => {
                let hunk = &local[local_index];
                if remote
                    .get(remote_index)
                    .is_some_and(|other| overlaps_hunk(other, index, hunk.base_end.max(index)))
                {
                    conflict |= emit_conflict(
                        &mut out,
                        base,
                        local,
                        remote,
                        &mut index,
                        &mut local_index,
                        &mut remote_index,
                    );
                } else {
                    for line in &hunk.lines {
                        out.push_str(line);
                    }
                    index = hunk.base_end;
                    local_index += 1;
                }
            }
            (false, true) => {
                let hunk = &remote[remote_index];
                if local
                    .get(local_index)
                    .is_some_and(|other| overlaps_hunk(other, index, hunk.base_end.max(index)))
                {
                    conflict |= emit_conflict(
                        &mut out,
                        base,
                        local,
                        remote,
                        &mut index,
                        &mut local_index,
                        &mut remote_index,
                    );
                } else {
                    for line in &hunk.lines {
                        out.push_str(line);
                    }
                    index = hunk.base_end;
                    remote_index += 1;
                }
            }
            (true, true) => {
                let left = &local[local_index];
                let right = &remote[remote_index];
                if left.base_end == right.base_end && left.lines == right.lines {
                    for line in &left.lines {
                        out.push_str(line);
                    }
                    index = left.base_end;
                    local_index += 1;
                    remote_index += 1;
                } else {
                    conflict |= emit_conflict(
                        &mut out,
                        base,
                        local,
                        remote,
                        &mut index,
                        &mut local_index,
                        &mut remote_index,
                    );
                }
            }
            (false, false) => break,
        }
    }
    if conflict {
        Merge::Conflict(out)
    } else {
        Merge::Clean(out)
    }
}

fn emit_conflict(
    out: &mut String,
    base: &[String],
    local: &[Hunk],
    remote: &[Hunk],
    index: &mut usize,
    local_index: &mut usize,
    remote_index: &mut usize,
) -> bool {
    let start = *index;
    let mut end = start;
    let mut local_end = *local_index;
    let mut remote_end = *remote_index;
    loop {
        let mut grew = false;
        while local
            .get(local_end)
            .is_some_and(|hunk| overlaps_hunk(hunk, start, end))
        {
            end = end.max(local[local_end].base_end);
            local_end += 1;
            grew = true;
        }
        while remote
            .get(remote_end)
            .is_some_and(|hunk| overlaps_hunk(hunk, start, end))
        {
            end = end.max(remote[remote_end].base_end);
            remote_end += 1;
            grew = true;
        }
        if !grew {
            break;
        }
    }
    let left = project(base, &local[*local_index..local_end], start, end);
    let right = project(base, &remote[*remote_index..remote_end], start, end);
    if left == right {
        out.push_str(&left);
        *index = end;
        *local_index = local_end;
        *remote_index = remote_end;
        return false;
    }
    out.push_str(OURS);
    push_side(out, &left);
    out.push_str(DIVIDER);
    push_side(out, &right);
    out.push_str(THEIRS);
    *index = end;
    *local_index = local_end;
    *remote_index = remote_end;
    true
}

/// An insert occupies the point in front of a baseline line. It meets a
/// replacement that starts on that line, and it does not meet the replacement
/// that ends there.
fn overlaps_hunk(hunk: &Hunk, start: usize, end: usize) -> bool {
    if hunk.base_start == hunk.base_end {
        if end == start {
            hunk.base_start == start
        } else {
            hunk.base_start >= start && hunk.base_start < end
        }
    } else if end == start {
        hunk.base_start == start
    } else {
        hunk.base_start < end && hunk.base_end > start
    }
}

fn project(base: &[String], hunks: &[Hunk], start: usize, end: usize) -> String {
    let mut out = String::new();
    let mut index = start;
    for hunk in hunks {
        if hunk.base_start == hunk.base_end {
            if end == start {
                if hunk.base_start == start {
                    for line in &hunk.lines {
                        out.push_str(line);
                    }
                }
                continue;
            }
            if hunk.base_start < start || hunk.base_start >= end {
                continue;
            }
            while index < hunk.base_start {
                out.push_str(&base[index]);
                index += 1;
            }
            for line in &hunk.lines {
                out.push_str(line);
            }
            continue;
        }
        if hunk.base_end <= start || hunk.base_start >= end {
            continue;
        }
        while index < hunk.base_start {
            out.push_str(&base[index]);
            index += 1;
        }
        for line in &hunk.lines {
            out.push_str(line);
        }
        index = hunk.base_end;
    }
    while index < end {
        out.push_str(&base[index]);
        index += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        split_lines(text)
    }

    fn script(base: &str, other: &str) -> String {
        let ops = diff(&lines(base), &lines(other)).unwrap();
        let mut out = String::new();
        let mut base_index = 0;
        let mut other_index = 0;
        let other_lines = lines(other);
        let base_lines = lines(base);
        for op in ops {
            match op {
                Op::Equal => {
                    out.push('=');
                    out.push_str(&base_lines[base_index]);
                    base_index += 1;
                    other_index += 1;
                }
                Op::Delete => {
                    out.push('-');
                    out.push_str(&base_lines[base_index]);
                    base_index += 1;
                }
                Op::Insert => {
                    out.push('+');
                    out.push_str(&other_lines[other_index]);
                    other_index += 1;
                }
            }
        }
        out
    }

    #[test]
    fn diff_covers_the_small_edits() {
        assert_eq!(script("", ""), "");
        assert_eq!(script("a\n", "a\n"), "=a\n");
        assert_eq!(script("a\n", "b\na\n"), "+b\n=a\n");
        assert_eq!(script("a\nc\n", "a\nb\nc\n"), "=a\n+b\n=c\n");
        assert_eq!(script("a\nb\n", "a\nb\nc\n"), "=a\n=b\n+c\n");
        assert_eq!(script("a\nb\nc\n", "a\nc\n"), "=a\n-b\n=c\n");
        assert_eq!(script("a\nb\nc\n", "a\nB\nc\n"), "=a\n-b\n+B\n=c\n");
        assert_eq!(script("a", "a\n"), "-a+a\n");
    }

    #[test]
    fn one_side_passes_through() {
        assert_eq!(merge("a\n", "a\nb\n", "a\n"), Merge::Clean("a\nb\n".into()));
        assert_eq!(merge("a\n", "a\n", "a\nb\n"), Merge::Clean("a\nb\n".into()));
        assert_eq!(
            merge("a\n", "same\n", "same\n"),
            Merge::Clean("same\n".into())
        );
    }

    #[test]
    fn apart_edits_combine() {
        assert_eq!(
            merge("a\n", "a\nc\n", "z\na\n"),
            Merge::Clean("z\na\nc\n".into())
        );
        assert_eq!(
            merge(
                "a\nb\nc\nd\ne\n",
                "a\nb\nC\nd\ne\nz\n",
                "0\na\nb\nc\nd\nE\n"
            ),
            Merge::Clean("0\na\nb\nC\nd\nE\nz\n".into())
        );
        assert_eq!(
            merge("a\nb\nc\n", "a\nc\n", "a\nb\nc\nz\n"),
            Merge::Clean("a\nc\nz\n".into())
        );
    }

    #[test]
    fn the_same_edit_on_both_sides_is_clean() {
        assert_eq!(
            merge("a\nb\n", "A\nb\n", "A\nb\n"),
            Merge::Clean("A\nb\n".into())
        );
    }

    #[test]
    fn overlapping_lines_are_marked() {
        match merge("a\nb\n", "a\nB\n", "a\nX\n") {
            Merge::Conflict(text) => {
                assert!(text.contains(OURS_MARK));
                assert!(text.contains("B\n"));
                assert!(text.contains("X\n"));
                assert!(text.contains(">>>>>>> subtext (other copy)"));
            }
            Merge::Clean(text) => panic!("merged overlapping lines into {text}"),
        }
        assert!(matches!(
            merge("a\n", "a\nb\n", "a\nc\n"),
            Merge::Conflict(_)
        ));
    }
}
