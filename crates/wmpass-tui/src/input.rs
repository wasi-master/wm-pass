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
            KeyCode::Char(ch) if !ctrl => {
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

    /// The line and column of the cursor, for placing the terminal's cursor.
    pub fn cursor_line_col(&self) -> (usize, usize) {
        let before: String = self.text.chars().take(self.cursor).collect();
        let line = before.matches('\n').count();
        let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count());
        (line, col)
    }
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
