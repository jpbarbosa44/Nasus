//! Cabeçalho do Nasus, no mesmo espírito do cabeçalho do Prisma: um livro
//! aberto em ASCII ao lado do nome em letras de bloco, encolhendo em telas
//! estreitas.

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::cores;

pub const SUBTITULO: &str = "— curador das suas leituras";

/// folga à esquerda da arte
const MARGEM: usize = 8;

/// arte é o livro aberto: `═` são as linhas de texto das páginas e `▐▌▝▘` a
/// fita do marcador pendurada abaixo da lombada. Montada por código para que
/// todas as linhas tenham exatamente a mesma largura.
fn arte() -> Vec<String> {
    let pagina = |n: usize| format!("  {:<16}", "═".repeat(n));
    let mut v = vec![
        format!("  {} {}  ", "_".repeat(17), "_".repeat(17)),
        format!(" /{}V{}\\ ", " ".repeat(17), " ".repeat(17)),
    ];
    for (esq, dir) in [(12, 12), (14, 11), (11, 12), (14, 10), (12, 13)] {
        v.push(format!("|{}|{}|", pagina(esq), pagina(dir)));
    }
    v.push(format!("|{}|{}|", "_".repeat(18), "_".repeat(18)));
    v.push(format!("{:<39}", format!("{}▐▌", " ".repeat(19))));
    v.push(format!("{:<39}", format!("{}▝▘", " ".repeat(19))));
    v
}

// Letras no estilo "ANSI Shadow", as mesmas do PRISMA.
const N: [&str; 6] = ["███╗   ██╗", "████╗  ██║", "██╔██╗ ██║", "██║╚██╗██║", "██║ ╚████║", "╚═╝  ╚═══╝"];
const A: [&str; 6] = [" █████╗ ", "██╔══██╗", "███████║", "██╔══██║", "██║  ██║", "╚═╝  ╚═╝"];
const S: [&str; 6] = ["███████╗", "██╔════╝", "███████╗", "╚════██║", "███████║", "╚══════╝"];
const U: [&str; 6] = ["██╗   ██╗", "██║   ██║", "██║   ██║", "██║   ██║", "╚██████╔╝", " ╚═════╝ "];

fn letras() -> Vec<String> {
    (0..6).map(|i| [N, A, S, U, S].iter().map(|l| l[i]).collect()).collect()
}

fn estilo_arte(c: char) -> Style {
    match c {
        '═' => cores::TEXTO,
        '▐' | '▌' | '▝' | '▘' => cores::FITA,
        _ => cores::LIVRO,
    }
}

/// colorir_arte quebra a linha em trechos de mesma cor.
fn colorir_arte(linha: &str) -> Vec<Span<'static>> {
    let mut spans: Vec<Span> = Vec::new();
    let mut atual = String::new();
    let mut estilo = None;
    for c in linha.chars() {
        let e = estilo_arte(c);
        if estilo.is_some_and(|x| x != e) {
            spans.push(Span::styled(std::mem::take(&mut atual), estilo.unwrap_or_default()));
        }
        estilo = Some(e);
        atual.push(c);
    }
    if !atual.is_empty() {
        spans.push(Span::styled(atual, estilo.unwrap_or_default()));
    }
    spans
}

/// cabecalho monta o livro ao lado de NASUS em letras grandes. Abaixo de 110
/// colunas o nome vai espaçado sob o livro; abaixo de 60, sobra uma linha só.
pub fn cabecalho(largura: u16) -> Vec<Line<'static>> {
    if largura < 60 {
        return vec![Line::from(vec![
            Span::styled(" ◆ N A S U S ", cores::TITULO),
            Span::styled(SUBTITULO, cores::APAGADA),
        ])];
    }

    let arte = arte();
    let mut linhas: Vec<Vec<Span<'static>>> = arte
        .iter()
        .map(|l| {
            let mut s = vec![Span::raw(" ".repeat(MARGEM))];
            s.extend(colorir_arte(l));
            s
        })
        .collect();

    if largura < 110 {
        let mut v: Vec<Line> = linhas.into_iter().map(Line::from).collect();
        v.push(Line::default());
        v.push(Line::from(vec![
            Span::raw(" ".repeat(MARGEM)),
            Span::styled("N A S U S ", cores::TITULO),
            Span::styled(SUBTITULO, cores::APAGADA),
        ]));
        return v;
    }

    let letras = letras();
    let topo = (arte.len() - letras.len() - 1) / 2;
    for (i, l) in letras.into_iter().enumerate() {
        linhas[topo + i].extend([Span::raw("   "), Span::styled(l, cores::TITULO)]);
    }
    linhas[topo + 6].extend([Span::raw("   "), Span::styled(SUBTITULO, cores::APAGADA)]);
    linhas.into_iter().map(Line::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn largura(l: &str) -> usize {
        l.chars().count()
    }

    #[test]
    fn arte_e_letras_alinhadas() {
        let a = arte();
        assert!(a.iter().all(|l| largura(l) == largura(&a[0])), "{a:#?}");
        let l = letras();
        assert!(l.iter().all(|x| largura(x) == largura(&l[0])), "{l:#?}");
    }

    #[test]
    fn cabecalho_cabe_em_cada_faixa() {
        for (w, altura) in [(40, 1), (59, 1), (60, 12), (109, 12), (110, 10), (200, 10)] {
            let c = cabecalho(w);
            assert_eq!(c.len(), altura, "largura {w}");
            for l in &c {
                assert!(l.width() <= w as usize, "largura {w}: linha com {} colunas", l.width());
            }
        }
    }

    #[test]
    fn letras_grandes_so_na_faixa_larga() {
        let tem_bloco = |w| cabecalho(w).iter().any(|l| l.to_string().contains("███"));
        assert!(tem_bloco(110));
        assert!(!tem_bloco(109));
    }
}
