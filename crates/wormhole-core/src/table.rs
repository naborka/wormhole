//! The one column-aligned table wormhole prints. `ps` and the box list
//! both draw one, so one body owns the widths — a second copy would drift
//! the moment either grew a column.

/// Rows, header first, padded into columns. Empty rows render as nothing;
/// a caller with nothing to show says so in its own words.
pub fn render(rows: &[Vec<String>]) -> String {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    // Measured in the terminal's own units. `{cell:<width$}` pads by
    // character count, which is right for Latin text and wrong for
    // everything a terminal draws double-wide or not at all: a CJK box
    // name or an emoji in a path pushes its column out of line with the
    // rest of the table. The padding is computed here for the same reason.
    let mut widths = vec![0usize; columns];
    for row in rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(display_width(cell));
        }
    }
    let mut table = String::new();
    for row in rows {
        let mut line = String::new();
        for (width, cell) in widths.iter().zip(row) {
            line.push_str(cell);
            line.push_str(&" ".repeat(width.saturating_sub(display_width(cell)) + 2));
        }
        table.push_str(line.trim_end());
        table.push('\n');
    }
    table
}

/// How many columns a cell takes on a terminal. Not its length in bytes,
/// and not its length in characters: a CJK character occupies two columns
/// and a combining mark occupies none.
pub fn display_width(cell: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(cell)
}

/// Seconds into a short human age: the two largest units that matter.
pub fn age(seconds: u64) -> String {
    let (days, hours, minutes) = (seconds / 86400, (seconds / 3600) % 24, (seconds / 60) % 60);
    match (days, hours, minutes) {
        (0, 0, 0) => format!("{seconds}s"),
        (0, 0, m) => format!("{m}m"),
        (0, h, m) => format!("{h}h{m}m"),
        (d, h, _) => format!("{d}d{h}h"),
    }
}

/// What a cut cell ends — or begins — with. One column wide, so the
/// arithmetic below can reserve exactly one for it.
pub const CUT: char = '…';

/// How many columns one character takes. A control character the terminal
/// draws as nothing counts as nothing, which is what keeps the cuts below
/// from ever handing back more columns than they were given.
fn char_width(c: char) -> usize {
    unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)
}

/// `text` cut to `width` terminal columns, losing its tail.
///
/// By columns rather than by characters, for the reason [`render`] pads by
/// columns: a CJK character occupies two and a combining mark none, so a
/// cut counted in characters overflows the width it was given — which on a
/// table is every column after it out of line, and on a full screen is a
/// wrapped line the cursor no longer marks.
///
/// Fills greedily: whatever is handed back is as wide as it can be without
/// exceeding `width`, never wider.
#[must_use]
pub fn truncate_tail(text: &str, width: usize) -> String {
    if display_width(text) <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut kept = String::new();
    let mut used = 0usize;
    for c in text.chars() {
        // One column is the cut mark's, and it is always drawn: this
        // branch is only reached when something had to go.
        if used + char_width(c) + 1 > width {
            break;
        }
        used += char_width(c);
        kept.push(c);
    }
    kept.push(CUT);
    kept
}

/// The same, losing its head. What a path wants: `/home/me/rust/a` is
/// told apart from `/home/me/rust/b` by its last character, so a path too
/// wide for its column keeps the end and loses the root.
#[must_use]
pub fn truncate_head(text: &str, width: usize) -> String {
    if display_width(text) <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut tail: Vec<char> = Vec::new();
    let mut used = 0usize;
    for c in text.chars().rev() {
        if used + char_width(c) + 1 > width {
            break;
        }
        used += char_width(c);
        tail.push(c);
    }
    std::iter::once(CUT).chain(tail.into_iter().rev()).collect()
}

/// `text` broken into lines of at most `width` columns, at spaces.
///
/// For the one thing on a screen that may not be cut: the line answering
/// the key just pressed. A word wider than the whole line is cut, because
/// the alternative is a line that overflows and takes the layout with it.
#[must_use]
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let space = usize::from(!line.is_empty());
        if display_width(&line) + space + display_width(word) > width {
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            if display_width(word) > width {
                lines.push(truncate_tail(word, width));
                continue;
            }
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// What an empty cell looks like in every listing wormhole prints. One
/// body, so the box table and the `ps` table cannot disagree about it.
pub fn or_dash(value: Option<&str>) -> String {
    value.unwrap_or("-").to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(cells: &[[&str; 2]]) -> Vec<Vec<String>> {
        cells
            .iter()
            .map(|row| row.iter().map(|c| (*c).to_owned()).collect())
            .collect()
    }

    #[test]
    fn columns_line_up_and_trailing_padding_is_trimmed() {
        let table = render(&rows(&[
            ["ID", "WORKSPACE"],
            ["a3f9", "/w"],
            ["7d2e04ab", "/other"],
        ]));
        let lines: Vec<&str> = table.lines().collect();
        assert_eq!(lines[0], "ID        WORKSPACE");
        assert_eq!(lines[1], "a3f9      /w");
        assert_eq!(lines[2], "7d2e04ab  /other");
        assert!(!table.contains(" \n"), "{table:?}");
    }

    /// A terminal draws a CJK cell double-wide and a combining mark not at
    /// all. Padding by character count puts every column after it out of
    /// line, so the width is the one the terminal will actually draw.
    #[test]
    fn a_cell_is_padded_by_terminal_columns_not_characters() {
        for (name, drawn) in [("名前", 4), ("héfé", 4), ("ab", 2)] {
            let table = render(&rows(&[["NAME", "W"], [name, "/w"]]));
            // Where the second column begins, in the units a terminal uses.
            let begins: Vec<usize> = table
                .lines()
                .map(|line| {
                    let at = line.find(['W', '/']).expect("second column");
                    display_width(&line[..at])
                })
                .collect();
            assert_eq!(begins[0], begins[1], "{name}: {table:?}");
            assert_eq!(begins[0], drawn.max("NAME".len()) + 2, "{name}: {table:?}");
        }
    }

    #[test]
    fn nothing_renders_as_nothing() {
        assert_eq!(render(&[]), "");
    }

    /// A cell that does not fit is cut where the terminal would cut it —
    /// by columns, not by characters — and says that it was cut.
    #[test]
    fn a_cell_too_wide_is_cut_and_marked() {
        assert_eq!(truncate_tail("alphaca-java", 12), "alphaca-java");
        assert_eq!(truncate_tail("alphaca-java", 8), "alphaca…");
        assert_eq!(truncate_tail("名前名前", 5), "名前…");
        assert_eq!(truncate_tail("abc", 1), "…");
        assert_eq!(truncate_tail("abc", 0), "");
    }

    /// A path is identified by its tail, so a path that does not fit
    /// keeps its tail and loses its root.
    #[test]
    fn a_path_too_wide_keeps_its_tail() {
        assert_eq!(truncate_head("/home/me/rust/a", 15), "/home/me/rust/a");
        assert_eq!(truncate_head("/home/me/rust/a", 9), "…e/rust/a");
        assert_eq!(truncate_head("名前名前", 5), "…名前");
        assert_eq!(truncate_head("abc", 1), "…");
        assert_eq!(truncate_head("abc", 0), "");
    }

    /// The one thing on a screen that may not be cut is the line that
    /// answers the key just pressed, so it wraps instead.
    #[test]
    fn a_line_that_may_not_be_cut_wraps_at_spaces() {
        assert_eq!(
            wrap("remove box a3f9? y confirm", 12),
            ["remove box", "a3f9? y", "confirm"]
        );
        assert_eq!(wrap("", 10), Vec::<String>::new());
        assert_eq!(wrap("short", 10), ["short"]);
        // A word with no space to break at is cut rather than overflowed.
        assert_eq!(
            wrap("/very/long/path/that/never/breaks", 10),
            ["/very/lon…"]
        );
        assert_eq!(wrap("anything", 0), Vec::<String>::new());
    }

    /// Wrapping that handed back a line wider than it was given would
    /// take the whole layout with it.
    #[test]
    fn no_wrapped_line_is_wider_than_the_width_it_was_given() {
        let text = "remove box a3f9c1e40b2d? its home goes with it — history, \
                    logins, and whatever the agent installed. 名前名前名前名前名前";
        for width in 1..60 {
            for line in wrap(text, width) {
                assert!(
                    display_width(&line) <= width,
                    "{width}: {line:?} is {} wide",
                    display_width(&line)
                );
            }
        }
    }

    /// Both cuts must leave something a terminal can draw in the width
    /// asked for — never one column more, whatever the characters are.
    #[test]
    fn neither_cut_ever_exceeds_the_width_it_was_given() {
        for text in ["", "a", "abc", "名前名前", "héfé", "a名b前c"] {
            for width in 0..10 {
                for cut in [truncate_tail, truncate_head] {
                    let cut = cut(text, width);
                    assert!(
                        display_width(&cut) <= width,
                        "{text:?} at {width}: {cut:?} is {} wide",
                        display_width(&cut)
                    );
                }
            }
        }
    }

    #[test]
    fn an_age_shows_the_two_units_that_matter() {
        assert_eq!(age(5), "5s");
        assert_eq!(age(90), "1m");
        assert_eq!(age(3700), "1h1m");
        assert_eq!(age(90_000), "1d1h");
    }
}
