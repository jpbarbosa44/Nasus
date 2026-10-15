use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};

use super::{cores, desenhar_input, moldura};
use crate::app::App;

pub fn desenhar(f: &mut Frame, app: &App) {
    let meio = moldura(
        f,
        app,
        Line::styled(" Buscar online › Open Library", cores::TITULO),
        "Enter buscar · ↑↓ escolher · Enter de novo adiciona o selecionado · Esc voltar",
    );
    let [entrada, info, lista] =
        Layout::vertical([Constraint::Length(3), Constraint::Length(1), Constraint::Min(0)]).areas(meio);

    let bloco = Block::bordered().title(" ISBN ou título ").border_style(cores::LIVRO);
    let dentro = bloco.inner(entrada);
    f.render_widget(bloco, entrada);
    desenhar_input(f, &app.busca, dentro, app.modal.is_none());

    let estado = if app.buscando {
        Line::styled("  buscando…", cores::AMAR)
    } else if !app.resultados.is_empty() {
        Line::styled(match app.resultados.len() {
            1 => "  1 resultado".to_string(),
            n => format!("  {n} resultados"),
        }, cores::APAGADA)
    } else {
        Line::default()
    };
    f.render_widget(Paragraph::new(estado), info);

    let itens: Vec<ListItem> = app
        .resultados
        .iter()
        .map(|r| {
            let mut extra = Vec::new();
            if let Some(a) = r.ano {
                extra.push(a.to_string());
            }
            if r.pages > 0 {
                extra.push(format!("{} p.", r.pages));
            }
            if !r.isbn.is_empty() {
                extra.push(format!("ISBN {}", r.isbn));
            }
            let autor = if r.author.is_empty() { String::new() } else { format!(" — {}", r.author) };
            ListItem::new(Line::from(vec![
                Span::raw(format!(" {}", r.title)),
                Span::styled(autor, cores::LIVRO),
                Span::styled(format!("  {}", extra.join(" · ")), cores::APAGADA),
            ]))
        })
        .collect();
    let mut st = ListState::default().with_selected((!itens.is_empty()).then_some(app.busca_sel));
    f.render_stateful_widget(List::new(itens).highlight_style(cores::SELEC), lista, &mut st);
}
