use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{BarChart, Block, Gauge, Paragraph};

use super::{cores, milhar, moldura};
use crate::app::App;

const MESES: [&str; 12] = ["Jan", "Fev", "Mar", "Abr", "Mai", "Jun", "Jul", "Ago", "Set", "Out", "Nov", "Dez"];

fn numero<'a>(rotulo: &str, valor: String) -> Line<'a> {
    Line::from(vec![Span::styled(format!(" {rotulo:<16}"), cores::APAGADA), Span::styled(valor, cores::TITULO)])
}

pub fn desenhar(f: &mut Frame, app: &App) {
    let Some(s) = &app.stats else { return };
    let meio = moldura(
        f,
        app,
        Line::styled(format!(" Estatísticas › {}", s.ano), cores::TITULO),
        "←→ ano · g definir meta · Esc voltar · ? ajuda",
    );
    let [meta, nums, grafico] =
        Layout::vertical([Constraint::Length(3), Constraint::Length(6), Constraint::Min(6)]).areas(meio);

    let bloco = Block::bordered().title(format!(" Meta de {} ", s.ano)).border_style(cores::APAGADA);
    match s.meta {
        Some(m) if m > 0 => {
            let rotulo = if s.lidos >= m {
                format!("{} de {} livros: meta batida!", s.lidos, m)
            } else {
                format!("{} de {} livros", s.lidos, m)
            };
            f.render_widget(
                Gauge::default()
                    .block(bloco)
                    .gauge_style(Style::new().fg(Color::LightCyan).bg(Color::Black))
                    .use_unicode(true)
                    .ratio((s.lidos as f64 / m as f64).min(1.0))
                    .label(rotulo),
                meta,
            );
        }
        _ => f.render_widget(
            Paragraph::new(Line::styled(format!("Sem meta para {}: tecle g para definir.", s.ano), cores::APAGADA))
                .block(bloco),
            meta,
        ),
    }

    let mut linhas = vec![
        Line::default(),
        numero("Livros lidos", s.lidos.to_string()),
        numero("Páginas lidas", milhar(s.paginas)),
        numero("Nota média", s.nota_media.map_or("—".into(), |n| format!("{n:.1} ★").replace('.', ","))),
        numero("Lendo agora", s.lendo.to_string()),
    ];
    if let Some(m) = s.meta.filter(|m| *m > s.lidos) {
        linhas.push(numero("Faltam", format!("{} livros para a meta", m - s.lidos)));
    }
    let [nums, cats] = Layout::horizontal([Constraint::Min(40), Constraint::Percentage(45)]).areas(nums);
    f.render_widget(Paragraph::new(linhas), nums);

    // categorias mais lidas no ano, com uma barrinha proporcional
    let mut linhas = vec![Line::styled(" Mais lidas por categoria", cores::APAGADA)];
    let maior = s.por_categoria.first().map_or(1, |(_, n)| *n).max(1);
    for (cat, n) in s.por_categoria.iter().take(5) {
        let barra = "█".repeat(((*n * 12) / maior).max(1) as usize);
        linhas.push(Line::from(vec![
            Span::raw(format!(" {cat:<18.18} ")),
            Span::styled(barra, cores::LIVRO),
            Span::styled(format!(" {n}"), cores::APAGADA),
        ]));
    }
    if s.por_categoria.is_empty() {
        linhas.push(Line::styled(" nenhum livro lido com categoria neste ano", cores::APAGADA));
    }
    f.render_widget(Paragraph::new(linhas), cats);

    let dados: Vec<(&str, u64)> = MESES.iter().copied().zip(s.por_mes).collect();
    let largura_barra = ((grafico.width.saturating_sub(2)) / 12).saturating_sub(1).clamp(1, 6);
    f.render_widget(
        BarChart::default()
            .block(Block::bordered().title(" Livros terminados por mês ").border_style(cores::APAGADA))
            .data(&dados)
            .bar_width(largura_barra)
            .bar_gap(1)
            .bar_style(cores::LIVRO)
            .value_style(cores::SELEC)
            .label_style(cores::APAGADA),
        grafico,
    );
}
