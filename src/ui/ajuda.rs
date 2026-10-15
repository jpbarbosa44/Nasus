use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::{cores, moldura};
use crate::app::App;

const SECOES: [(&str, &str); 9] = [
    ("Menu", "↑↓ ou 1-5 escolher · Enter abrir · q sair"),
    ("Biblioteca", "Tab/←→ trocar aba · ↑↓ mover · Enter detalhes · a adicionar · e editar · d remover · / filtrar · o buscar online"),
    ("Detalhes", "p página atual · s status · r nota (0-5) · l plano de leitura · L recalcular plano · n nota · c citação · ↑↓ + x remover nota · e editar · d remover livro"),
    ("Estatísticas", "←→ trocar ano · g definir meta (0 remove)"),
    ("Buscar online", "digite ISBN ou título e Enter · ↑↓ escolher · Enter de novo abre o formulário preenchido"),
    ("Formulário", "Tab/↑↓ trocar campo · ←→ ou espaço mudam o status · Enter salvar · Esc cancelar"),
    ("Campos de texto", "←→ Home End movem o cursor · Ctrl+U limpa"),
    ("Em qualquer lugar", "Esc voltar · ? ajuda · Ctrl+C sair"),
    ("Bot do Telegram", "nasus bot --token TOKEN, depois nasus bot --chat ID · --instalar-servico deixa sempre ligado"),
];

pub fn desenhar(f: &mut Frame, app: &App) {
    let meio = moldura(f, app, Line::styled(" Como usar", cores::TITULO), "Esc voltar");
    let mut linhas = vec![
        Line::raw(" Registrar a página atual (p) num livro da fila já o move para \"Lendo\"; ao chegar na última"),
        Line::raw(" página, o Nasus oferece marcar como lido. As datas de início e fim são preenchidas sozinhas."),
        Line::raw(" Plano de leitura (l): informe o início e as páginas por dia; o Nasus calcula a data de fim e mostra"),
        Line::raw(" a meta de hoje e se você está à frente ou atrás. L refaz o plano a partir de hoje (0 págs/dia remove)."),
        Line::default(),
    ];
    for (secao, teclas) in SECOES {
        linhas.push(Line::from(vec![Span::styled(format!(" {secao:<18}"), cores::TITULO), Span::raw(teclas)]));
    }
    linhas.push(Line::default());
    linhas.push(Line::styled(format!(" Dados em {}  (troque com NASUS_DB)", app.caminho_db), cores::APAGADA));
    f.render_widget(Paragraph::new(linhas).wrap(Wrap { trim: false }), meio);
}
