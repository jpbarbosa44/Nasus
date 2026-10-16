//! Desenho das telas. Só lê o estado de `App`.

mod ajuda;
mod art;
mod biblioteca;
mod busca;
pub mod cores;
mod detalhe;
mod menu;
mod modal;
mod retro;
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
        Tela::Retrospectiva => retro::desenhar(f, app),
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

    use chrono::{Datelike, Days, Local};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::db::Db;
    use crate::model::{Book, Note, NoteKind, Status};

    fn tela(app: &App) -> String {
        let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
        t.draw(|f| desenhar(f, app)).unwrap();
        let buf = t.backend().buffer();
        (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn tecla(app: &mut App, c: KeyCode) {
        app.tecla(KeyEvent::new(c, KeyModifiers::NONE));
        assert!(app.aviso.as_ref().is_none_or(|a| !a.erro), "erro: {}", app.aviso.as_ref().unwrap().texto);
    }

    /// percorre as telas novas com dados de verdade, só para garantir que
    /// desenham sem pânico e mostram o essencial (NASUS_TELAS=1 imprime).
    #[test]
    fn telas_desenham() {
        let hoje = Local::now().date_naive();
        let db = Db::em_memoria().unwrap();
        let mut b = Book { title: "A Montanha Mágica".into(), author: "Thomas Mann".into(), pages: 1160, serie: "Clássicos".into(), volume: 2, ..Book::novo() };
        b.id = db.inserir_livro(&b).unwrap();
        for (i, p) in [(20u64, 30), (5, 60), (2, 90), (1, 120), (0, 160)] {
            let d = hoje - Days::new(i);
            b.registrar_pagina(p, d);
            db.salvar_progresso(&b, d).unwrap();
        }
        db.emprestar(b.id, "Maria", hoje).unwrap();
        let mut lido = Book { title: "Dom Casmurro".into(), author: "Machado".into(), pages: 256, rating: 5, ..Book::novo() };
        lido.mudar_status(Status::Lendo, hoje.with_day(1).unwrap());
        lido.mudar_status(Status::Lido, hoje);
        lido.id = db.inserir_livro(&lido).unwrap();
        db.inserir_nota(&Note { id: 0, book_id: lido.id, kind: NoteKind::Citacao, page: None, body: "Uma frase.".into(), created_at: hoje }).unwrap();

        let mut app = App::new(db, "teste".into()).unwrap();
        let mostra = std::env::var("NASUS_TELAS").is_ok();
        let confere = |app: &App, deve: &[&str]| {
            let t = tela(app);
            if mostra {
                println!("{t}\n{}", "─".repeat(120));
            }
            for d in deve {
                assert!(t.contains(d), "faltou {d:?} em:\n{t}");
            }
        };
        tecla(&mut app, KeyCode::Char('1'));
        confere(&app, &["A Montanha Mágica ↗"]);
        tecla(&mut app, KeyCode::Enter);
        confere(&app, &["Clássicos · vol. 2", "com Maria", "pág/dia", "m emprestar"]);
        tecla(&mut app, KeyCode::Char('e'));
        confere(&app, &["Série", "Volume"]);
        tecla(&mut app, KeyCode::Esc);
        tecla(&mut app, KeyCode::Esc);
        tecla(&mut app, KeyCode::Esc);
        tecla(&mut app, KeyCode::Char('5'));
        confere(&app, &["seguidos", "Livros terminados por mês"]);
        tecla(&mut app, KeyCode::Char('r'));
        confere(&app, &[&format!("Retrospectiva {}", hoje.year()), "Dom Casmurro", "“Uma frase.”"]);
    }

    #[test]
    fn formatos() {
        assert_eq!(barra(0.5, 10), "█████░░░░░  50%");
        assert_eq!(barra(1.0, 4), "████ 100%");
        assert_eq!(milhar(0), "0");
        assert_eq!(milhar(2340), "2.340");
        assert_eq!(milhar(1234567), "1.234.567");
    }
}
