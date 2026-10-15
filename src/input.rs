//! Campo de texto de uma linha com cursor.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Debug, Default)]
pub struct Input {
    valor: String,
    /// posição do cursor, em caracteres
    cursor: usize,
}

impl Input {
    pub fn com(valor: impl Into<String>) -> Self {
        let valor = valor.into();
        let cursor = valor.chars().count();
        Input { valor, cursor }
    }

    pub fn valor(&self) -> &str {
        &self.valor
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    fn tamanho(&self) -> usize {
        self.valor.chars().count()
    }

    fn byte(&self, c: usize) -> usize {
        self.valor.char_indices().nth(c).map_or(self.valor.len(), |(i, _)| i)
    }

    /// tratar aplica a tecla ao campo; devolve false se ela não é de edição.
    pub fn tratar(&mut self, k: KeyEvent) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Char('u') if ctrl => *self = Input::default(),
            KeyCode::Char(c) if !ctrl => {
                let i = self.byte(self.cursor);
                self.valor.insert(i, c);
                self.cursor += 1;
            }
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                let i = self.byte(self.cursor);
                self.valor.remove(i);
            }
            KeyCode::Delete if self.cursor < self.tamanho() => {
                let i = self.byte(self.cursor);
                self.valor.remove(i);
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.tamanho()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.tamanho(),
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tecla(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    #[test]
    fn edita_com_acentos() {
        let mut i = Input::com("Saramgo");
        for c in [KeyCode::Left, KeyCode::Left] {
            i.tratar(tecla(c));
        }
        i.tratar(tecla(KeyCode::Char('a')));
        assert_eq!(i.valor(), "Saramago");

        let mut i = Input::com("ção");
        i.tratar(tecla(KeyCode::Home));
        i.tratar(tecla(KeyCode::Delete));
        i.tratar(tecla(KeyCode::End));
        i.tratar(tecla(KeyCode::Backspace));
        assert_eq!(i.valor(), "ã");
        assert_eq!(i.cursor(), 1);
    }
}
