use chrono::{Local, NaiveDate};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, LineGauge, List, ListItem, ListState, Paragraph, Wrap};

use super::{cores, moldura};
use crate::app::App;
use crate::model::{Book, NoteKind, Plano, Situacao, plural};

fn data(d: Option<NaiveDate>) -> String {
    d.map_or("—".into(), |d| d.format("%d/%m/%Y").to_string())
}

fn campo<'a>(rotulo: &str, valor: Span<'a>) -> Line<'a> {
    Line::from(vec![Span::styled(format!(" {rotulo:<9}"), cores::APAGADA), valor])
}

/// linhas_plano resume o plano: período, prazo e a situação em relação à
/// meta de hoje.
fn linhas_plano(b: &Book, p: &Plano) -> Vec<Line<'static>> {
    let hoje = Local::now().date_naive();
    let fim = p.fim(b.pages);
    let prazo = if hoje < p.inicio {
        format!("começa em {}", plural((p.inicio - hoje).num_days(), "dia"))
    } else if hoje <= fim {
        let n = (fim - hoje).num_days() + 1;
        if n == 1 { "último dia".to_string() } else { format!("faltam {n} dias") }
    } else {
        "prazo encerrado".to_string()
    };
    let periodo = Line::from(vec![
        Span::styled(format!("{} pág/dia", p.paginas_dia), cores::TITULO),
        Span::styled(format!(" · {} → {} · {prazo}", data(Some(p.inicio)), data(Some(fim))), cores::APAGADA),
    ]);

    let situacao = match p.situacao(b, hoje) {
        Situacao::Concluido => Line::styled("✔ Plano concluído", cores::OK),
        Situacao::NaoComecou { primeira_meta } => Line::from(vec![
            Span::styled("Primeira meta: ", cores::APAGADA),
            Span::raw(format!("até a pág. {primeira_meta} em {}", data(Some(p.inicio)))),
        ]),
        s => {
            let cor = match s {
                Situacao::Cumprida { .. } => cores::OK,
                Situacao::Atrasado { .. } => cores::AMAR,
                _ => cores::LIVRO,
            };
            Line::from(vec![
                Span::styled("Meta de hoje: ", cores::APAGADA),
                Span::raw(format!("até a pág. {}", p.meta_ate(hoje, b.pages))),
                Span::styled(" · ", cores::APAGADA),
                Span::styled(s.texto(), cor),
            ])
        }
    };
    vec![periodo, situacao]
}

pub fn desenhar(f: &mut Frame, app: &App) {
    let Some(b) = &app.livro else { return };
    let meio = moldura(
        f,
        app,
        Line::styled(" Biblioteca › Detalhes", cores::TITULO),
        "p página · s status · r nota · l plano · L recalcular · n nota · c citação · x remover nota · e editar · d remover · Esc voltar",
    );
    let altura_resenha = if b.review.is_empty() { 0 } else { 4 };
    let altura_plano = if app.plano.is_some() { 4 } else { 0 };
    let [info, progresso, plano, resenha, notas] = Layout::vertical([
        Constraint::Length(8),
        Constraint::Length(2),
        Constraint::Length(altura_plano),
        Constraint::Length(altura_resenha),
        Constraint::Min(3),
    ])
    .areas(meio);

    let autor = if b.author.is_empty() { "autor desconhecido" } else { &b.author };
    let paginas = match (b.pages, b.current_page) {
        (0, 0) => "—".to_string(),
        (0, atual) => format!("{atual} (total desconhecido)"),
        (total, atual) => format!("{atual} de {total}"),
    };
    let isbn = if b.isbn.is_empty() { String::new() } else { format!("    ISBN {}", b.isbn) };
    let nota = if b.rating == 0 { Span::styled("sem nota", cores::APAGADA) } else { Span::styled(b.estrelas(), cores::AMAR) };
    let linhas = vec![
        Line::styled(format!(" {}", b.title), cores::TITULO),
        Line::styled(format!(" por {autor}"), cores::APAGADA),
        Line::default(),
        campo("Status", Span::styled(format!("{} {}", b.status.simbolo(), b.status.nome()), cores::status(b.status))),
        campo("Páginas", Span::raw(paginas + &isbn)),
        campo("Período", Span::raw(format!("{} → {}", data(b.started_at), data(b.finished_at)))),
        campo("Nota", nota),
    ];
    f.render_widget(Paragraph::new(linhas), info);

    let linha = progresso.inner(Margin::new(1, 0));
    match b.progresso() {
        Some(p) => f.render_widget(
            LineGauge::default()
                .filled_style(cores::LIVRO)
                .unfilled_style(cores::APAGADA)
                .label(format!("{:>3.0}% ", p * 100.0))
                .ratio(p),
            linha,
        ),
        None => f.render_widget(
            Paragraph::new(Line::styled("informe o total de páginas (e) para ver o progresso", cores::APAGADA)),
            linha,
        ),
    }

    if let Some(p) = &app.plano {
        f.render_widget(
            Paragraph::new(linhas_plano(b, p))
                .block(Block::bordered().title(" Plano de leitura ").border_style(cores::APAGADA)),
            plano,
        );
    }

    if altura_resenha > 0 {
        f.render_widget(
            Paragraph::new(b.review.as_str())
                .wrap(Wrap { trim: true })
                .block(Block::bordered().title(" Resenha ").border_style(cores::APAGADA)),
            resenha,
        );
    }

    let bloco = Block::bordered()
        .title(format!(" Notas e citações ({}) ", app.notas.len()))
        .border_style(cores::APAGADA);
    if app.notas.is_empty() {
        f.render_widget(
            Paragraph::new(Line::styled("Nenhuma nota ainda: n para nota, c para citação.", cores::APAGADA))
                .block(bloco),
            notas,
        );
        return;
    }
    let itens: Vec<ListItem> = app
        .notas
        .iter()
        .map(|n| {
            let pagina = n.page.map_or(String::new(), |p| format!("p. {p:<4} "));
            let corpo = match n.kind {
                NoteKind::Nota => Span::raw(format!("• {}", n.body)),
                NoteKind::Citacao => {
                    Span::styled(format!("“{}”", n.body), Style::new().add_modifier(Modifier::ITALIC))
                }
            };
            ListItem::new(Line::from(vec![Span::styled(pagina, cores::APAGADA), corpo]))
        })
        .collect();
    let mut estado = ListState::default().with_selected(Some(app.nota_sel));
    f.render_stateful_widget(List::new(itens).block(bloco).highlight_style(cores::SELEC), notas, &mut estado);
}
