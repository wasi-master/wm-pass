//! A text box. Its text is wiped when it is dropped or cleared, because it may hold a
//! password, and a terminal's key events arrive one character at a time with no buffer of
//! their own to clean.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use zeroize::{Zeroize, Zeroizing};

/// One line, or several when `multiline` is set.
#[derive(Debug, Default)]
pub struct Input {
    text: Zeroizing<String>,
    /// Cursor, in characters.
    cursor: usize,
    /// Whether Enter adds a line rather than finishing.
    pub multiline: bool,
}

/// What a key did.
#[derive(Debug, PartialEq, Eq)]
pub enum Edit {
    /// The text changed.
    Changed,
    /// The cursor moved.
    Moved,
    /// Not a key for a text box.
    Ignored,
}

impl Input {
    /// An empty box.
    pub fn new() -> Self {
        Self::default()
    }

    /// A box holding `text`, with the cursor at the end.
    pub fn with(text: &str) -> Self {
        let mut input = Self::new();
        input.set(text);
        input
    }

    /// The text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The cursor, in characters.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Whether it is empty.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Replaces the text.
    pub fn set(&mut self, text: &str) {
        self.text.zeroize();
        self.text.push_str(text);
        self.cursor = self.text.chars().count();
    }

    /// Empties it, wiping what it held.
    pub fn clear(&mut self) {
        self.text.zeroize();
        self.cursor = 0;
    }

    fn byte_at(&self, chars: usize) -> usize {
        self.text.char_indices().nth(chars).map_or(self.text.len(), |(i, _)| i)
    }

    fn insert(&mut self, ch: char) {
        let at = self.byte_at(self.cursor);
        // Growing a String may move it and leave the old bytes behind, so a box that is
        // about to grow moves its text into a bigger one it owns, wiping the old.
        if self.text.len() + ch.len_utf8() > self.text.capacity() {
            let mut bigger = Zeroizing::new(String::with_capacity((self.text.capacity() * 2).max(32)));
            bigger.push_str(&self.text);
            self.text = bigger;
        }
        self.text.insert(at, ch);
        self.cursor += 1;
    }

    fn line_bounds(&self) -> (usize, usize) {
        let chars: Vec<char> = self.text.chars().collect();
        let mut start = self.cursor;
        while start > 0 && chars[start - 1] != '\n' {
            start -= 1;
        }
        let mut end = self.cursor;
        while end < chars.len() && chars[end] != '\n' {
            end += 1;
        }
        (start, end)
    }

    fn word_right(&self) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let mut at = self.cursor;
        while at < chars.len() && chars[at].is_whitespace() {
            at += 1;
        }
        while at < chars.len() && !chars[at].is_whitespace() {
            at += 1;
        }
        at
    }

    fn word_left(&self) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let mut at = self.cursor;
        while at > 0 && chars[at - 1].is_whitespace() {
            at -= 1;
        }
        while at > 0 && !chars[at - 1].is_whitespace() {
            at -= 1;
        }
        at
    }

    fn remove_range(&mut self, from: usize, to: usize) {
        let (a, b) = (self.byte_at(from), self.byte_at(to));
        // `drain` shifts the tail down, and the bytes left past the new end are wiped by
        // the next zeroize; a removal never reallocates, so nothing escapes.
        self.text.drain(a..b);
        self.cursor = from;
    }

    /// Applies a key.
    pub fn key(&mut self, key: KeyEvent) -> Edit {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let len = self.text.chars().count();
        match key.code {
            KeyCode::Char('a') if ctrl => {
                self.cursor = self.line_bounds().0;
                Edit::Moved
            }
            KeyCode::Char('e') if ctrl => {
                self.cursor = self.line_bounds().1;
                Edit::Moved
            }
            KeyCode::Char('u') if ctrl => {
                let start = self.line_bounds().0;
                self.remove_range(start, self.cursor);
                Edit::Changed
            }
            KeyCode::Char('k') if ctrl => {
                let end = self.line_bounds().1;
                let at = self.cursor;
                self.remove_range(at, end);
                Edit::Changed
            }
            KeyCode::Char('w') if ctrl => {
                let from = self.word_left();
                let at = self.cursor;
                self.remove_range(from, at);
                Edit::Changed
            }
            KeyCode::Backspace if alt || ctrl => {
                let from = self.word_left();
                let at = self.cursor;
                self.remove_range(from, at);
                Edit::Changed
            }
            KeyCode::Delete if alt || ctrl => {
                let to = self.word_right();
                let at = self.cursor;
                self.remove_range(at, to);
                Edit::Changed
            }
            // Alt with b, f and d moves and deletes by word, as in a shell; it is what a Mac
            // terminal sends for Option and an arrow when Option is set to act as Meta.
            KeyCode::Char('b') if alt && !ctrl => {
                self.cursor = self.word_left();
                Edit::Moved
            }
            KeyCode::Char('f') if alt && !ctrl => {
                self.cursor = self.word_right();
                Edit::Moved
            }
            KeyCode::Char('d') if alt && !ctrl => {
                let to = self.word_right();
                let at = self.cursor;
                self.remove_range(at, to);
                Edit::Changed
            }
            // Windows reports AltGr as Ctrl and Alt together: `@` on a German keyboard arrives
            // that way, and a password typed with it must not lose the letter.
            KeyCode::Char(ch) if !ctrl || alt => {
                self.insert(ch);
                Edit::Changed
            }
            KeyCode::Enter if self.multiline => {
                self.insert('\n');
                Edit::Changed
            }
            KeyCode::Backspace if self.cursor > 0 => {
                let at = self.cursor;
                self.remove_range(at - 1, at);
                Edit::Changed
            }
            KeyCode::Delete if self.cursor < len => {
                let at = self.cursor;
                self.remove_range(at, at + 1);
                Edit::Changed
            }
            KeyCode::Left if alt || ctrl => {
                self.cursor = self.word_left();
                Edit::Moved
            }
            KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                Edit::Moved
            }
            KeyCode::Right if alt || ctrl => {
                self.cursor = self.word_right();
                Edit::Moved
            }
            KeyCode::Right => {
                self.cursor = (self.cursor + 1).min(len);
                Edit::Moved
            }
            KeyCode::Home => {
                self.cursor = self.line_bounds().0;
                Edit::Moved
            }
            KeyCode::End => {
                self.cursor = self.line_bounds().1;
                Edit::Moved
            }
            _ => Edit::Ignored,
        }
    }

    /// Takes the text out, leaving the box empty.
    pub fn take(&mut self) -> Zeroizing<String> {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    /// The line and column of the cursor, for placing the terminal's cursor. The column counts
    /// terminal cells, so a wide letter such as 漢 or an emoji counts two.
    pub fn cursor_line_col(&self) -> (usize, usize) {
        let before = &self.text[..self.byte_at(self.cursor)];
        let line = before.matches('\n').count();
        let col = before.rsplit('\n').next().map_or(0, cells);
        (line, col)
    }

    /// The cells the text before the cursor takes on one line, for a one-line box.
    pub fn cursor_cells(&self) -> usize {
        self.cursor_line_col().1
    }
}

/// How many terminal cells `text` takes.
pub fn cells(text: &str) -> usize {
    ratatui::text::Span::raw(text).width()
}

/// The end of `text` that fits in `width` cells with the cursor `col` cells in, and where the
/// cursor lands in it: a long line scrolls so the cursor stays in sight.
pub fn scrolled(text: &str, col: usize, width: usize) -> (String, usize) {
    let width = width.max(1);
    // Leave a cell for the cursor itself past the last letter.
    let skip = (col + 1).saturating_sub(width);
    let mut skipped = 0;
    let mut out = String::new();
    for ch in text.chars() {
        let w = cells(ch.encode_utf8(&mut [0; 4]));
        if skipped < skip {
            skipped += w;
            continue;
        }
        out.push(ch);
    }
    (out, col.saturating_sub(skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(input: &mut Input, code: KeyCode) {
        input.key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn ctrl(input: &mut Input, ch: char) {
        input.key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL));
    }

    #[test]
    fn typing_and_moving() {
        let mut input = Input::new();
        for ch in "héllo wörld".chars() {
            press(&mut input, KeyCode::Char(ch));
        }
        assert_eq!(input.text(), "héllo wörld");
        ctrl(&mut input, 'w');
        assert_eq!(input.text(), "héllo ");
        press(&mut input, KeyCode::Home);
        press(&mut input, KeyCode::Delete);
        assert_eq!(input.text(), "éllo ");
        press(&mut input, KeyCode::End);
        press(&mut input, KeyCode::Backspace);
        assert_eq!(input.text(), "éllo");
        ctrl(&mut input, 'a');
        ctrl(&mut input, 'k');
        assert!(input.is_empty());
    }

    #[test]
    fn many_lines() {
        let mut input = Input::new();
        input.multiline = true;
        for ch in "ab".chars() {
            press(&mut input, KeyCode::Char(ch));
        }
        press(&mut input, KeyCode::Enter);
        press(&mut input, KeyCode::Char('c'));
        assert_eq!(input.text(), "ab\nc");
        assert_eq!(input.cursor_line_col(), (1, 1));
        ctrl(&mut input, 'u');
        assert_eq!(input.text(), "ab\n");
    }

    #[test]
    fn altgr_letters_are_typed() {
        let mut input = Input::new();
        for ch in "@{€\\".chars() {
            input.key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL | KeyModifiers::ALT));
        }
        assert_eq!(input.text(), "@{€\\");
    }

    #[test]
    fn words_go_both_ways() {
        let mut input = Input::with("one two three");
        press(&mut input, KeyCode::Home);
        input.key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL));
        assert_eq!(input.cursor(), 3, "ctrl+right ends the word, as ctrl+left starts one");
        input.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::ALT));
        assert_eq!(input.cursor(), 7, "alt+f, as a shell has it");
        input.key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT));
        assert_eq!(input.cursor(), 4);
        input.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::ALT));
        assert_eq!(input.text(), "one  three", "alt+d deletes the next word, typing nothing");
        input.key(KeyEvent::new(KeyCode::Delete, KeyModifiers::CONTROL));
        assert_eq!(input.text(), "one ");
    }

    #[test]
    fn the_cursor_counts_cells_and_long_lines_scroll() {
        let input = Input::with("漢字ab");
        assert_eq!(input.cursor_line_col(), (0, 6), "a wide letter takes two cells");
        let (shown, at) = scrolled("abcdefghij", 10, 5);
        assert_eq!((shown.as_str(), at), ("ghij", 4), "the end stays in sight, with a cell for the cursor");
        let (shown, at) = scrolled("abc", 1, 5);
        assert_eq!((shown.as_str(), at), ("abc", 1));
        let (shown, at) = scrolled("漢字漢字", 8, 5);
        assert_eq!((shown.as_str(), at), ("漢字", 4));
    }

    #[test]
    fn growing_past_capacity_keeps_the_text() {
        let mut input = Input::new();
        let long: String = std::iter::repeat_n('x', 200).collect();
        for ch in long.chars() {
            press(&mut input, KeyCode::Char(ch));
        }
        assert_eq!(input.text(), long);
        let taken = input.take();
        assert_eq!(taken.as_str(), long);
        assert!(input.is_empty());
    }
}
