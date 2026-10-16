//! Retrospectiva do ano: os destaques em cartões.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Wrap};

use super::{cores, milhar, moldura};
use crate::app::App;
use crate::db::{MESES, citacoes};
use crate::model::plural;

/// cartao desenha um destaque: o rótulo na borda, o valor em destaque e um detalhe.
fn cartao(f: &mut Frame, area: Rect, rotulo: &str, valor: Option<String>, detalhe: String) {
    let bloco = Block::bordered().title(format!(" {rotulo} ")).border_style(cores::APAGADA);
    let linhas = match valor {
        Some(v) => vec![Line::styled(format!(" {v}"), cores::TITULO), Line::styled(format!(" {detalhe}"), cores::APAGADA)],
        None => vec![Line::styled(" —", cores::APAGADA)],
    };
    f.render_widget(Paragraph::new(linhas).block(bloco), area);
}

pub fn desenhar(f: &mut Frame, app: &App) {
    let Some(r) = &app.retro else { return };
    let meio = moldura(
        f,
        app,
        Line::styled(format!(" Estatísticas › Retrospectiva {}", r.ano), cores::TITULO),
        "←→ ano · Esc voltar · ? ajuda",
    );
    if r.lidos == 0 && r.dias_lendo == 0 {
        let msg = format!("  Nenhum livro terminado em {} ainda.", r.ano);
        f.render_widget(Paragraph::new(vec![Line::default(), Line::styled(msg, cores::APAGADA)]), meio);
        return;
    }
    let [grade, citacao] = Layout::vertical([Constraint::Length(12), Constraint::Min(0)]).areas(meio);
    let linhas = Layout::vertical([Constraint::Length(4); 3]).split(grade);
    let celulas: Vec<Rect> =
        linhas.iter().flat_map(|l| Layout::horizontal([Constraint::Ratio(1, 3); 3]).split(*l).to_vec()).collect();

    let par = |o: &Option<(String, i64)>| o.clone();
    cartao(f, celulas[0], "Livros lidos", Some(plural(r.lidos, "livro")), format!("{} páginas", milhar(r.paginas)));
    let (titulo, p) = par(&r.mais_longo).unzip();
    cartao(f, celulas[1], "Mais longo", titulo, p.map_or(String::new(), |p| format!("{} páginas", milhar(p))));
    let (titulo, d) = par(&r.mais_rapido).unzip();
    cartao(f, celulas[2], "Mais rápido", titulo, d.map_or(String::new(), |d| format!("em {}", plural(d, "dia"))));
    let (autor, n) = par(&r.autor).unzip();
    cartao(f, celulas[3], "Autor do ano", autor, n.map_or(String::new(), |n| plural(n, "livro")));
    let (cat, n) = par(&r.categoria).unzip();
    cartao(f, celulas[4], "Categoria favorita", cat, n.map_or(String::new(), |n| plural(n, "livro")));
    cartao(
        f,
        celulas[5],
        "Melhor mês",
        r.melhor_mes.map(|(m, _)| MESES[m as usize - 1].to_string()),
        r.melhor_mes.map_or(String::new(), |(_, n)| plural(n as i64, "livro")),
    );
    cartao(
        f,
        celulas[6],
        "Nota média",
        r.nota_media.map(|m| format!("{m:.1} ★").replace('.', ",")),
        if r.cinco_estrelas.is_empty() { String::new() } else { format!("{} com 5 estrelas", plural(r.cinco_estrelas.len() as i64, "livro")) },
    );
    cartao(
        f,
        celulas[7],
        "Dias lendo",
        (r.dias_lendo > 0).then(|| plural(r.dias_lendo, "dia")),
        format!("maior sequência: {}", plural(r.maior_sequencia, "dia")),
    );
    cartao(
        f,
        celulas[8],
        "Notas e citações",
        (r.notas + r.citacoes > 0).then(|| (r.notas + r.citacoes).to_string()),
        format!("{} · {}", plural(r.notas, "nota"), citacoes(r.citacoes)),
    );

    if let Some((texto, livro)) = &r.citacao {
        let linhas = vec![
            Line::default(),
            Line::styled(format!("  “{texto}”"), cores::TEXTO),
            Line::styled(format!("  — {livro}"), cores::APAGADA),
        ];
        f.render_widget(Paragraph::new(linhas).wrap(Wrap { trim: false }), citacao);
    }
}
