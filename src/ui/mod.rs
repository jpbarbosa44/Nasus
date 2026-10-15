//! Desenho das telas. Só lê o estado de `App`.

mod ajuda;
mod art;
mod biblioteca;
mod busca;
pub mod cores;
mod detalhe;
mod menu;
mod modal;
mod stats;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, Tela};
use crate::input::Input;

pub fn desenhar(f: &mut Frame, app: &App) {
    match app.tela {
        Tela::Menu => menu::desenhar(f, app),
        Tela::Biblioteca => biblioteca::desenhar(f, app),
        Tela::Detalhe => detalhe::desenhar(f, app),
        Tela::Estatisticas => stats::desenhar(f, app),
        Tela::Busca => busca::desenhar(f, app),
        Tela::Ajuda => ajuda::desenhar(f, app),
    }
    if let Some(m) = &app.modal {
        modal::desenhar(f, m);
    }
}

/// moldura desenha o título no topo e o rodapé, e devolve a área do meio.
fn moldura(f: &mut Frame, app: &App, titulo: Line, atalhos: &str) -> Rect {
    let [topo, meio, baixo] =
        Layout::vertical([Constraint::Length(2), Constraint::Min(0), Constraint::Length(1)]).areas(f.area());
    f.render_widget(Paragraph::new(titulo), topo);
    rodape(f, app, baixo, atalhos);
    meio
}

/// rodape mostra o aviso pendente ou, sem aviso, os atalhos da tela.
fn rodape(f: &mut Frame, app: &App, area: Rect, atalhos: &str) {
    let linha = match &app.aviso {
        Some(a) => Span::styled(format!(" {}", a.texto), if a.erro { cores::ERRO } else { cores::OK }),
        None => Span::styled(format!(" {atalhos}"), cores::APAGADA),
    };
    f.render_widget(Paragraph::new(linha), area);
}

/// desenhar_input mostra o campo rolando para manter o cursor visível.
fn desenhar_input(f: &mut Frame, inp: &Input, area: Rect, foco: bool) {
    let largura = area.width as usize;
    if largura == 0 {
        return;
    }
    let inicio = inp.cursor().saturating_sub(largura - 1);
    let visivel: String = inp.valor().chars().skip(inicio).take(largura).collect();
    f.render_widget(Paragraph::new(visivel), area);
    if foco {
        f.set_cursor_position(Position::new(area.x + (inp.cursor() - inicio) as u16, area.y));
    }
}

fn centro(area: Rect, w: u16, h: u16) -> Rect {
    let (w, h) = (w.min(area.width), h.min(area.height));
    Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h)
}

/// barra de progresso em texto: "█████░░░░░  50%".
fn barra(p: f64, largura: usize) -> String {
    let cheio = ((p * largura as f64).round() as usize).min(largura);
    format!("{}{} {:>3.0}%", "█".repeat(cheio), "░".repeat(largura - cheio), p * 100.0)
}

/// milhar formata 12345 como "12.345".
pub(crate) fn milhar(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatos() {
        assert_eq!(barra(0.5, 10), "█████░░░░░  50%");
        assert_eq!(barra(1.0, 4), "████ 100%");
        assert_eq!(milhar(0), "0");
        assert_eq!(milhar(2340), "2.340");
        assert_eq!(milhar(1234567), "1.234.567");
    }
}
