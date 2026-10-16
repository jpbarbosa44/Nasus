//! Paleta do Nasus: as mesmas cores ANSI do Prisma.

use ratatui::style::{Color, Modifier, Style};

use crate::model::Status;

pub const LIVRO: Style = Style::new().fg(Color::LightCyan); // ciano (14)
pub const TEXTO: Style = Style::new().fg(Color::White); // linhas das páginas (15)
pub const FITA: Style = Style::new().fg(Color::LightRed); // marcador de página
pub const TITULO: Style = Style::new().fg(Color::LightCyan).add_modifier(Modifier::BOLD);
pub const SELEC: Style = Style::new().fg(Color::Black).bg(Color::LightCyan).add_modifier(Modifier::BOLD);
pub const APAGADA: Style = Style::new().fg(Color::DarkGray);
pub const ERRO: Style = Style::new().fg(Color::LightRed);
pub const OK: Style = Style::new().fg(Color::LightGreen);
pub const AMAR: Style = Style::new().fg(Color::LightYellow);

pub fn status(s: Status) -> Style {
    match s {
        Status::Estante => Style::new().fg(Color::Gray),
        Status::Lendo => LIVRO,
        Status::QueroLer => Style::new().fg(Color::White),
        Status::Lido => OK,
        Status::Abandonado => ERRO,
    }
}
