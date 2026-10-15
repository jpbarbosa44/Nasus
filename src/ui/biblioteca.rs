use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState, Tabs};

use super::{barra, cores, moldura};
use crate::app::{ABAS, App, rotulo_aba};
use crate::model::Book;

pub fn desenhar(f: &mut Frame, app: &App) {
    let mut titulo = vec![Span::styled(" Biblioteca", cores::TITULO)];
    if !app.filtro.is_empty() {
        titulo.push(Span::styled(format!("   filtro: {}  (Esc limpa)", app.filtro), cores::AMAR));
    }
    let meio = moldura(
        f,
        app,
        Line::from(titulo),
        "Tab abas · ↑↓ mover · Enter abrir · a adicionar · e editar · d remover · / filtrar · o buscar online · ? ajuda",
    );
    let [abas, lista] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(meio);

    let nomes: Vec<String> =
        ABAS.iter().enumerate().map(|(i, a)| format!("{} ({})", rotulo_aba(*a), app.contagem(i))).collect();
    f.render_widget(
        Tabs::new(nomes).select(app.aba).style(cores::APAGADA).highlight_style(cores::SELEC).divider("│"),
        abas,
    );

    let livros = app.visiveis();
    if livros.is_empty() {
        let msg = if app.filtro.is_empty() {
            "  Nenhum livro aqui. Tecle a para adicionar ou o para buscar online."
        } else {
            "  Nenhum livro bate com o filtro."
        };
        f.render_widget(Paragraph::new(Line::styled(msg, cores::APAGADA)), lista);
        return;
    }

    let linhas = livros.iter().map(|b| {
        Row::new(vec![
            Cell::from(Span::styled(b.status.simbolo(), cores::status(b.status))),
            Cell::from(b.title.clone()),
            Cell::from(Span::styled(b.author.clone(), cores::APAGADA)),
            Cell::from(progresso(b)),
            Cell::from(Span::styled(b.estrelas(), cores::AMAR)),
        ])
    });
    let tabela = Table::new(
        linhas,
        [
            Constraint::Length(1),
            Constraint::Fill(3),
            Constraint::Fill(2),
            Constraint::Length(15),
            Constraint::Length(5),
        ],
    )
    .header(Row::new(["", "TÍTULO", "AUTOR", "PROGRESSO", "NOTA"]).style(cores::APAGADA))
    .row_highlight_style(cores::SELEC);
    let mut estado = TableState::default().with_selected(Some(app.sel));
    f.render_stateful_widget(tabela, lista, &mut estado);
}

fn progresso(b: &Book) -> String {
    match b.progresso() {
        Some(p) => barra(p, 10),
        None if b.current_page > 0 => format!("p. {}", b.current_page),
        None => "—".into(),
    }
}
