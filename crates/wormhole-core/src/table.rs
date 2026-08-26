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

    #[test]
    fn an_age_shows_the_two_units_that_matter() {
        assert_eq!(age(5), "5s");
        assert_eq!(age(90), "1m");
        assert_eq!(age(3700), "1h1m");
        assert_eq!(age(90_000), "1d1h");
    }
}
