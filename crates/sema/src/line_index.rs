//! Byte offsets to lines and columns and back, in UTF-8 bytes or UTF-16 code
//! units. LSP clients ask for one or the other (docs/design.md#language-server).

/// How columns are counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf16,
}

/// A zero-based line and column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LineCol {
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset where each line starts. The first is 0.
    starts: Vec<usize>,
    len: usize,
}

impl LineIndex {
    pub fn new(text: &str) -> LineIndex {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        LineIndex {
            starts,
            len: text.len(),
        }
    }

    /// The line and column of a byte offset. Offsets past the end clamp to it;
    /// offsets inside a character count as its start.
    pub fn line_col(&self, text: &str, offset: usize, encoding: Encoding) -> LineCol {
        let offset = floor_char_boundary(text, offset.min(self.len));
        let line = self.starts.partition_point(|&s| s <= offset) - 1;
        let start = self.starts[line];
        let col = match encoding {
            Encoding::Utf8 => offset - start,
            Encoding::Utf16 => text[start..offset].encode_utf16().count(),
        };
        LineCol {
            line: to_u32(line),
            col: to_u32(col),
        }
    }

    /// The byte offset of a line and column. A column past the end of its line
    /// clamps to the end of the line; a line past the end clamps to the end of
    /// the text.
    pub fn offset(&self, text: &str, at: LineCol, encoding: Encoding) -> usize {
        let Some(&start) = self.starts.get(at.line as usize) else {
            return self.len;
        };
        let end = self
            .starts
            .get(at.line as usize + 1)
            .map_or(self.len, |&next| next - 1);
        let line = &text[start..end];
        let col = at.col as usize;
        let within = match encoding {
            Encoding::Utf8 => floor_char_boundary(line, col.min(line.len())),
            Encoding::Utf16 => {
                let mut units = 0;
                line.char_indices()
                    .find(|(_, c)| {
                        units += c.len_utf16();
                        units > col
                    })
                    .map_or(line.len(), |(i, _)| i)
            }
        };
        start + within
    }
}

fn floor_char_boundary(text: &str, mut offset: usize) -> usize {
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "mov x0, #1\n_é: b 🦀\n\nret";

    fn lc(line: u32, col: u32) -> LineCol {
        LineCol { line, col }
    }

    #[test]
    fn utf8_round_trip() {
        let index = LineIndex::new(TEXT);
        for offset in (0..=TEXT.len()).filter(|&o| TEXT.is_char_boundary(o)) {
            let at = index.line_col(TEXT, offset, Encoding::Utf8);
            assert_eq!(index.offset(TEXT, at, Encoding::Utf8), offset, "{offset}");
        }
    }

    #[test]
    fn utf16_round_trip() {
        let index = LineIndex::new(TEXT);
        for offset in (0..=TEXT.len()).filter(|&o| TEXT.is_char_boundary(o)) {
            let at = index.line_col(TEXT, offset, Encoding::Utf16);
            assert_eq!(index.offset(TEXT, at, Encoding::Utf16), offset, "{offset}");
        }
    }

    #[test]
    fn columns_differ_by_encoding() {
        let index = LineIndex::new(TEXT);
        let colon = TEXT.find(':').expect("colon");
        assert_eq!(index.line_col(TEXT, colon, Encoding::Utf8), lc(1, 3));
        assert_eq!(index.line_col(TEXT, colon, Encoding::Utf16), lc(1, 2));
        // The crab is two UTF-16 units and four bytes.
        let end_of_line = TEXT.find("\n\n").expect("blank line");
        assert_eq!(index.line_col(TEXT, end_of_line, Encoding::Utf16), lc(1, 8));
        assert_eq!(index.line_col(TEXT, end_of_line, Encoding::Utf8), lc(1, 11));
    }

    #[test]
    fn out_of_range_clamps() {
        let index = LineIndex::new(TEXT);
        assert_eq!(index.offset(TEXT, lc(0, 999), Encoding::Utf16), 10);
        assert_eq!(index.offset(TEXT, lc(99, 0), Encoding::Utf8), TEXT.len());
        assert_eq!(index.line_col(TEXT, 9999, Encoding::Utf8), lc(3, 3));
    }

    #[test]
    fn inside_a_character_counts_as_its_start() {
        let index = LineIndex::new(TEXT);
        let crab = TEXT.find('🦀').expect("crab");
        assert_eq!(
            index.line_col(TEXT, crab + 1, Encoding::Utf8),
            index.line_col(TEXT, crab, Encoding::Utf8)
        );
    }
}
