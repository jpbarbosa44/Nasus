use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::{art, cores, rodape};
use crate::app::{App, MENU, aba_do_menu};

/// Menu inicial: cabeçalho NASUS e as telas numeradas, como no Prisma.
pub fn desenhar(f: &mut Frame, app: &App) {
    let area = f.area();
    let cab = art::cabecalho(area.width);
    let [topo, _, lista, baixo] = Layout::vertical([
        Constraint::Length(cab.len() as u16 + 1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    f.render_widget(Paragraph::new(cab), topo);

    let mut linhas: Vec<Line> = MENU
        .iter()
        .enumerate()
        .map(|(i, (titulo, resumo))| {
            let resumo = match aba_do_menu(i) {
                Some(aba) => format!("{resumo} · {}", app.contagem(aba)),
                None => resumo.to_string(),
            };
            let rotulo = format!(" {}  {:<14}", i + 1, titulo);
            let estilo = if i == app.menu_sel { cores::SELEC } else { Default::default() };
            Line::from(vec![Span::styled(rotulo, estilo), Span::raw(" "), Span::styled(resumo, cores::APAGADA)])
        })
        .collect();
    if let Some(v) = &app.atualizacao {
        linhas.push(Line::default());
        linhas.push(Line::styled(format!(" ✦ nova versão {v}: rode nasus atualizar"), cores::AMAR));
    }
    f.render_widget(Paragraph::new(linhas), lista);
    rodape(f, app, baixo, "↑↓ navegar · Enter abrir · 1-7 atalho · ? ajuda · q sair");
}
