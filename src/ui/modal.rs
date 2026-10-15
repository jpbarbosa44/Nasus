use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::{centro, cores, desenhar_input};
use crate::app::{CAMPO_STATUS, CAMPOS, CAMPOS_PLANO, Form, FormPlano, Modal};
use crate::model::{Plano, Status};

fn bloco(titulo: &str) -> Block<'static> {
    Block::bordered().title(format!(" {titulo} ")).title_style(cores::TITULO).border_style(cores::LIVRO)
}

/// abrir limpa a área do modal, desenha a moldura e devolve o interior.
fn abrir(f: &mut Frame, area: Rect, titulo: &str) -> Rect {
    f.render_widget(Clear, area);
    let b = bloco(titulo);
    let dentro = b.inner(area);
    f.render_widget(b, area);
    dentro
}

pub fn desenhar(f: &mut Frame, m: &Modal) {
    let tela = f.area();
    match m {
        Modal::Form(form) => formulario(f, form),
        Modal::Plano(form) => plano(f, form),
        Modal::Prompt { titulo, input, .. } => {
            let dentro = abrir(f, centro(tela, 60, 3), titulo);
            desenhar_input(f, input, dentro, true);
        }
        Modal::Confirmar { texto, .. } => {
            let largura = (texto.chars().count() as u16 + 6).max(40);
            let dentro = abrir(f, centro(tela, largura, 5), "Confirmar");
            f.render_widget(
                Paragraph::new(vec![
                    Line::raw(format!(" {texto}")),
                    Line::default(),
                    Line::from(vec![
                        Span::styled(" s ", cores::SELEC),
                        Span::raw(" sim   "),
                        Span::styled(" n ", cores::SELEC),
                        Span::raw(" não"),
                    ]),
                ]),
                dentro,
            );
        }
        Modal::Status { sel } => {
            let dentro = abrir(f, centro(tela, 30, Status::TODOS.len() as u16 + 2), "Status");
            let linhas: Vec<Line> = Status::TODOS
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let texto = format!(" {}  {} {:<14}", i + 1, s.simbolo(), s.nome());
                    if i == *sel { Line::styled(texto, cores::SELEC) } else { Line::styled(texto, cores::status(*s)) }
                })
                .collect();
            f.render_widget(Paragraph::new(linhas), dentro);
        }
    }
}

/// rotulo desenha o nome do campo (destacado quando em foco) e devolve a
/// área onde vai o valor.
fn rotulo(f: &mut Frame, linha: Rect, nome: &str, foco: bool) -> Rect {
    let [rotulo, valor] = Layout::horizontal([Constraint::Length(11), Constraint::Min(0)]).areas(linha);
    f.render_widget(
        Paragraph::new(Span::styled(format!(" {nome:<9}"), if foco { cores::SELEC } else { cores::APAGADA })),
        rotulo,
    );
    Rect { x: valor.x + 1, width: valor.width.saturating_sub(2), ..valor }
}

fn formulario(f: &mut Frame, form: &Form) {
    let titulo = if form.id.is_some() { "Editar livro" } else { "Novo livro" };
    let dentro = abrir(f, centro(f.area(), 72, CAMPOS.len() as u16 + 4), titulo);
    let mut restricoes = vec![Constraint::Length(1); CAMPOS.len()];
    restricoes.extend([Constraint::Length(1), Constraint::Length(1)]);
    let linhas = Layout::vertical(restricoes).split(dentro);

    for (i, nome) in CAMPOS.iter().enumerate() {
        let foco = form.foco == i;
        let valor = rotulo(f, linhas[i], nome, foco);
        match form.entrada(i) {
            Some(inp) => desenhar_input(f, inp, valor, foco),
            None if i == CAMPO_STATUS => f.render_widget(
                Paragraph::new(Span::styled(
                    format!("◀ {} {} ▶", form.status.simbolo(), form.status.nome()),
                    cores::status(form.status),
                )),
                valor,
            ),
            None => {}
        }
    }
    f.render_widget(
        Paragraph::new(Line::styled(" Tab próximo campo · ←→ status · Enter salvar · Esc cancelar", cores::APAGADA)),
        linhas[CAMPOS.len() + 1],
    );
}

fn plano(f: &mut Frame, form: &FormPlano) {
    let dentro = abrir(f, centro(f.area(), 74, 7), "Plano de leitura");
    let linhas = Layout::vertical([Constraint::Length(1); 5]).split(dentro);
    for (i, nome) in CAMPOS_PLANO.iter().enumerate() {
        let foco = form.foco == i;
        let valor = rotulo(f, linhas[i], nome, foco);
        desenhar_input(f, &form.entradas[i], valor, foco);
    }

    // prévia ao vivo do que o plano vai ser
    let previa = match form.ler() {
        Ok((_, 0)) => Line::styled(" 0 páginas por dia remove o plano", cores::AMAR),
        Ok((inicio, paginas_dia)) => {
            let p = Plano { book_id: 0, inicio, pagina_inicial: form.pagina_inicial, paginas_dia };
            let restantes = (form.total - form.pagina_inicial).max(0);
            let desde = if form.pagina_inicial > 0 { format!(" da pág. {}:", form.pagina_inicial) } else { String::new() };
            Line::from(vec![
                Span::styled(format!("{desde} {restantes} páginas a {paginas_dia}/dia → termina em "), cores::APAGADA),
                Span::styled(p.fim(form.total).format("%d/%m/%Y").to_string(), cores::TITULO),
                Span::styled(format!(" ({} dias)", p.dias(form.total)), cores::APAGADA),
            ])
        }
        Err(e) => Line::styled(format!(" {e}"), cores::AMAR),
    };
    f.render_widget(Paragraph::new(previa), linhas[3]);
    f.render_widget(
        Paragraph::new(Line::styled(" Tab troca o campo · Enter salvar · Esc cancelar", cores::APAGADA)),
        linhas[4],
    );
}
