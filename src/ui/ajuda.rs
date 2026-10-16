use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::{cores, moldura};
use crate::app::App;

const SECOES: [(&str, &str); 10] = [
    ("Menu", "↑↓ ou 1-7 escolher · Enter abrir · q sair"),
    ("Biblioteca", "Tab/←→ trocar aba · ↑↓ mover · Enter detalhes · a adicionar · e editar · d remover · / filtrar · c filtrar por categoria · o buscar online"),
    ("Detalhes", "p página atual · s status · r nota (0-5) · l plano de leitura · L recalcular plano · n nota · c citação · ↑↓ + x remover nota · m emprestar/devolvido · e editar · d remover livro"),
    ("Estatísticas", "←→ trocar ano · g definir meta (0 remove) · r retrospectiva do ano"),
    ("Buscar online", "digite ISBN ou título e Enter · ↑↓ escolher · Enter de novo abre o formulário preenchido"),
    ("Formulário", "Tab/↑↓ trocar campo · ←→ ou espaço mudam o status · Enter salvar · Esc cancelar"),
    ("Campos de texto", "←→ Home End movem o cursor · Ctrl+U limpa"),
    ("Em qualquer lugar", "Esc voltar · ? ajuda · Ctrl+C sair"),
    ("Bot do Telegram", "nasus bot --token TOKEN, depois nasus bot --chat ID · --instalar-servico deixa sempre ligado"),
    ("No terminal", "nasus exportar · nasus importar (JSON ou Goodreads) · nasus restaurar · nasus mcp · nasus atualizar"),
];

pub fn desenhar(f: &mut Frame, app: &App) {
    let meio = moldura(f, app, Line::styled(" Como usar", cores::TITULO), "Esc voltar");
    let mut linhas = vec![
        Line::raw(" Todo livro adicionado vai para a Estante; mude o status (s) para \"Quero ler\" para pô-lo na fila."),
        Line::raw(" Categorias (romance, filosofia…) vão no formulário (e), separadas por vírgula."),
        Line::raw(" Registrar a página atual (p) num livro da estante ou da fila já o move para \"Lendo\"; ao chegar na última"),
        Line::raw(" página, o Nasus oferece marcar como lido. As datas de início e fim são preenchidas sozinhas."),
        Line::raw(" Plano de leitura (l): informe o início e as páginas por dia; o Nasus calcula a data de fim e mostra"),
        Line::raw(" a meta de hoje e se você está à frente ou atrás. L refaz o plano a partir de hoje (0 págs/dia remove)."),
        Line::raw(" Cada página registrada entra no histórico: as estatísticas mostram a sequência de dias lendo e o mapa"),
        Line::raw(" de calor, e o detalhe do livro mostra quando ele termina no seu ritmo. Série e volume vão no formulário."),
        Line::default(),
    ];
    for (secao, teclas) in SECOES {
        linhas.push(Line::from(vec![Span::styled(format!(" {secao:<18}"), cores::TITULO), Span::raw(teclas)]));
    }
    linhas.push(Line::default());
    linhas.push(Line::styled(
        format!(" Dados em {}  (troque com NASUS_DB) · uma cópia por dia em backups/, ao lado", app.caminho_db),
        cores::APAGADA,
    ));
    f.render_widget(Paragraph::new(linhas).wrap(Wrap { trim: false }), meio);
}
