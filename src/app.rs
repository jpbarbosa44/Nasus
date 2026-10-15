//! Estado da aplicação e tratamento de teclas. A interface só lê daqui
//! (ver `ui`); toda mudança passa por `App::tecla`.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use anyhow::{Context, Result, bail};
use chrono::{Datelike, Local, NaiveDate};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::db::{Db, Estatisticas};
use crate::input::Input;
use crate::model::{Book, Note, NoteKind, Plano, Status};
use crate::openlibrary::{self, Resultado};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tela {
    Menu,
    Biblioteca,
    Detalhe,
    Estatisticas,
    Busca,
    Ajuda,
}

/// Itens do menu inicial: título e resumo.
pub const MENU: [(&str, &str); 5] = [
    ("Biblioteca", "todos os seus livros, por status"),
    ("Lendo agora", "livros em andamento"),
    ("Estatísticas", "meta do ano, páginas e notas"),
    ("Buscar online", "Open Library por ISBN ou título"),
    ("Como usar", "atalhos e dicas"),
];

/// Abas da biblioteca; `None` é "Todos".
pub const ABAS: [Option<Status>; 5] =
    [None, Some(Status::Lendo), Some(Status::QueroLer), Some(Status::Lido), Some(Status::Abandonado)];

pub fn rotulo_aba(aba: Option<Status>) -> &'static str {
    aba.map_or("Todos", Status::rotulo)
}

pub const CAMPOS: [&str; 6] = ["Título", "Autor", "ISBN", "Páginas", "Status", "Resenha"];
pub const CAMPO_STATUS: usize = 4;

/// Formulário de livro (novo ou edição).
pub struct Form {
    pub id: Option<i64>,
    /// título, autor, ISBN, páginas, resenha (o status não é texto)
    entradas: [Input; 5],
    pub status: Status,
    pub foco: usize,
}

impl Form {
    fn novo(status: Status) -> Self {
        Form { id: None, entradas: Default::default(), status, foco: 0 }
    }

    fn de_livro(b: &Book) -> Self {
        Form {
            id: Some(b.id),
            entradas: [
                Input::com(&b.title),
                Input::com(&b.author),
                Input::com(&b.isbn),
                Input::com(paginas_texto(b.pages)),
                Input::com(&b.review),
            ],
            status: b.status,
            foco: 0,
        }
    }

    fn de_resultado(r: &Resultado) -> Self {
        Form {
            id: None,
            entradas: [
                Input::com(&r.title),
                Input::com(&r.author),
                Input::com(&r.isbn),
                Input::com(paginas_texto(r.pages)),
                Input::default(),
            ],
            status: Status::QueroLer,
            foco: 0,
        }
    }

    /// entrada do campo `i` de CAMPOS; o campo de status não tem.
    pub fn entrada(&self, i: usize) -> Option<&Input> {
        match i {
            CAMPO_STATUS => None,
            i if i < CAMPO_STATUS => self.entradas.get(i),
            i => self.entradas.get(i - 1),
        }
    }

    fn entrada_mut(&mut self, i: usize) -> Option<&mut Input> {
        match i {
            CAMPO_STATUS => None,
            i if i < CAMPO_STATUS => self.entradas.get_mut(i),
            i => self.entradas.get_mut(i - 1),
        }
    }
}

fn paginas_texto(p: i64) -> String {
    if p > 0 { p.to_string() } else { String::new() }
}

pub const CAMPOS_PLANO: [&str; 2] = ["Início", "Págs/dia"];

/// Formulário do plano de leitura de um livro.
pub struct FormPlano {
    /// início (dd/mm/aaaa) e páginas por dia
    pub entradas: [Input; 2],
    pub foco: usize,
    pub total: i64,
    pub pagina_inicial: i64,
}

impl FormPlano {
    fn novo(inicio: NaiveDate, paginas_dia: String, total: i64, pagina_inicial: i64) -> Self {
        FormPlano {
            entradas: [Input::com(inicio.format("%d/%m/%Y").to_string()), Input::com(paginas_dia)],
            foco: 1,
            total,
            pagina_inicial,
        }
    }

    /// ler interpreta os campos: data de início e páginas por dia (0 remove).
    pub fn ler(&self) -> Result<(NaiveDate, i64)> {
        let inicio = ler_data(self.entradas[0].valor())?;
        let paginas_dia = self.entradas[1]
            .valor()
            .trim()
            .parse::<i64>()
            .ok()
            .filter(|n| *n >= 0)
            .context("páginas por dia deve ser um número")?;
        Ok((inicio, paginas_dia))
    }
}

/// ler_data aceita dd/mm/aaaa (ou aaaa-mm-dd).
fn ler_data(s: &str) -> Result<NaiveDate> {
    let s = s.trim();
    NaiveDate::parse_from_str(s, "%d/%m/%Y")
        .or_else(|_| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
        .ok()
        .context("use o formato dd/mm/aaaa")
}

pub enum Modal {
    Form(Form),
    Plano(FormPlano),
    Prompt { titulo: String, input: Input, acao: Acao },
    Confirmar { texto: String, acao: Confirmacao },
    Status { sel: usize },
}

#[derive(Clone, Copy)]
pub enum Acao {
    Pagina,
    Nota,
    NovaNota(NoteKind),
    Meta,
    Filtro,
}

#[derive(Clone, Copy)]
pub enum Confirmacao {
    RemoverLivro(i64),
    RemoverNota(i64),
    MarcarLido,
    RecalcularPlano,
}

pub struct Aviso {
    pub texto: String,
    pub erro: bool,
}

type RespostaBusca = Result<Vec<Resultado>, String>;

pub struct App {
    db: Db,
    pub caminho_db: String,
    pub tela: Tela,
    /// para onde Esc volta a partir da ajuda e da busca
    anterior: Tela,
    pub sair: bool,
    pub menu_sel: usize,

    pub livros: Vec<Book>,
    pub aba: usize,
    pub filtro: String,
    pub sel: usize,

    pub livro: Option<Book>,
    pub notas: Vec<Note>,
    pub nota_sel: usize,
    pub plano: Option<Plano>,

    pub ano: i32,
    pub stats: Option<Estatisticas>,

    pub busca: Input,
    pub resultados: Vec<Resultado>,
    pub busca_sel: usize,
    pub buscando: bool,
    ultima_busca: String,
    tx: Sender<RespostaBusca>,
    rx: Receiver<RespostaBusca>,

    pub modal: Option<Modal>,
    pub aviso: Option<Aviso>,
}

fn hoje() -> NaiveDate {
    Local::now().date_naive()
}

impl App {
    pub fn new(db: Db, caminho_db: String) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let mut app = App {
            db,
            caminho_db,
            tela: Tela::Menu,
            anterior: Tela::Menu,
            sair: false,
            menu_sel: 0,
            livros: Vec::new(),
            aba: 0,
            filtro: String::new(),
            sel: 0,
            livro: None,
            notas: Vec::new(),
            nota_sel: 0,
            plano: None,
            ano: hoje().year(),
            stats: None,
            busca: Input::default(),
            resultados: Vec::new(),
            busca_sel: 0,
            buscando: false,
            ultima_busca: String::new(),
            tx,
            rx,
            modal: None,
            aviso: None,
        };
        app.recarregar()?;
        Ok(app)
    }

    /// visiveis são os livros da aba atual que passam no filtro.
    pub fn visiveis(&self) -> Vec<&Book> {
        let filtro = self.filtro.to_lowercase();
        self.livros
            .iter()
            .filter(|b| ABAS[self.aba].is_none_or(|s| b.status == s))
            .filter(|b| {
                filtro.is_empty()
                    || b.title.to_lowercase().contains(&filtro)
                    || b.author.to_lowercase().contains(&filtro)
            })
            .collect()
    }

    pub fn contagem(&self, aba: usize) -> usize {
        self.livros.iter().filter(|b| ABAS[aba].is_none_or(|s| b.status == s)).count()
    }

    fn selecionado(&self) -> Option<&Book> {
        self.visiveis().get(self.sel).copied()
    }

    fn recarregar(&mut self) -> Result<()> {
        self.livros = self.db.livros()?;
        self.sel = self.sel.min(self.visiveis().len().saturating_sub(1));
        Ok(())
    }

    fn abrir_detalhe(&mut self, id: i64) -> Result<()> {
        self.livro = Some(self.db.livro(id)?);
        self.notas = self.db.notas(id)?;
        self.plano = self.db.plano(id)?;
        self.nota_sel = 0;
        self.tela = Tela::Detalhe;
        Ok(())
    }

    fn recarregar_detalhe(&mut self) -> Result<()> {
        if let Some(id) = self.livro.as_ref().map(|b| b.id) {
            self.livro = Some(self.db.livro(id)?);
            self.notas = self.db.notas(id)?;
            self.plano = self.db.plano(id)?;
            self.nota_sel = self.nota_sel.min(self.notas.len().saturating_sub(1));
        }
        self.recarregar()
    }

    fn livro_atual(&self) -> Result<Book> {
        self.livro.clone().context("nenhum livro aberto")
    }

    fn abrir_stats(&mut self) -> Result<()> {
        self.stats = Some(self.db.estatisticas(self.ano)?);
        self.tela = Tela::Estatisticas;
        Ok(())
    }

    fn ir_para(&mut self, tela: Tela) {
        self.anterior = self.tela;
        self.tela = tela;
    }

    fn ok(&mut self, texto: impl Into<String>) {
        self.aviso = Some(Aviso { texto: texto.into(), erro: false });
    }

    fn prompt(&mut self, titulo: impl Into<String>, valor: impl Into<String>, acao: Acao) {
        self.modal = Some(Modal::Prompt { titulo: titulo.into(), input: Input::com(valor), acao });
    }

    fn confirmar(&mut self, texto: impl Into<String>, acao: Confirmacao) {
        self.modal = Some(Modal::Confirmar { texto: texto.into(), acao });
    }

    /// verificar_busca recolhe a resposta da thread da Open Library, se chegou.
    pub fn verificar_busca(&mut self) {
        while let Ok(r) = self.rx.try_recv() {
            self.buscando = false;
            match r {
                Ok(v) => {
                    if v.is_empty() {
                        self.ok("Nada encontrado.");
                    }
                    self.resultados = v;
                    self.busca_sel = 0;
                }
                Err(e) => self.aviso = Some(Aviso { texto: format!("Falha na busca: {e}"), erro: true }),
            }
        }
    }

    fn iniciar_busca(&mut self, q: String) {
        self.buscando = true;
        self.ultima_busca = q.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let _ = tx.send(openlibrary::buscar(&q).map_err(|e| format!("{e:#}")));
        });
    }

    pub fn tecla(&mut self, k: KeyEvent) {
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            self.sair = true;
            return;
        }
        self.aviso = None;
        let r = if self.modal.is_some() {
            self.tecla_modal(k)
        } else {
            match self.tela {
                Tela::Menu => self.tecla_menu(k),
                Tela::Biblioteca => self.tecla_biblioteca(k),
                Tela::Detalhe => self.tecla_detalhe(k),
                Tela::Estatisticas => self.tecla_stats(k),
                Tela::Busca => self.tecla_busca(k),
                Tela::Ajuda => {
                    if matches!(k.code, KeyCode::Esc | KeyCode::Char('q' | '?')) {
                        self.tela = self.anterior;
                    }
                    Ok(())
                }
            }
        };
        if let Err(e) = r {
            self.aviso = Some(Aviso { texto: format!("{e:#}"), erro: true });
        }
    }

    fn tecla_menu(&mut self, k: KeyEvent) -> Result<()> {
        let n = MENU.len();
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => self.menu_sel = (self.menu_sel + n - 1) % n,
            KeyCode::Down | KeyCode::Char('j') => self.menu_sel = (self.menu_sel + 1) % n,
            KeyCode::Char(c @ '1'..='5') => {
                self.menu_sel = c as usize - '1' as usize;
                return self.entrar_menu();
            }
            KeyCode::Enter => return self.entrar_menu(),
            KeyCode::Char('?') => self.ir_para(Tela::Ajuda),
            KeyCode::Esc | KeyCode::Char('q') => self.sair = true,
            _ => {}
        }
        Ok(())
    }

    fn entrar_menu(&mut self) -> Result<()> {
        match self.menu_sel {
            0 | 1 => {
                self.aba = self.menu_sel;
                self.sel = 0;
                self.recarregar()?;
                self.tela = Tela::Biblioteca;
            }
            2 => self.abrir_stats()?,
            3 => self.ir_para(Tela::Busca),
            _ => self.ir_para(Tela::Ajuda),
        }
        Ok(())
    }

    fn tecla_biblioteca(&mut self, k: KeyEvent) -> Result<()> {
        let n = self.visiveis().len();
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => self.sel = self.sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') if self.sel + 1 < n => self.sel += 1,
            KeyCode::Home | KeyCode::Char('g') => self.sel = 0,
            KeyCode::End | KeyCode::Char('G') => self.sel = n.saturating_sub(1),
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                self.aba = (self.aba + 1) % ABAS.len();
                self.sel = 0;
            }
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                self.aba = (self.aba + ABAS.len() - 1) % ABAS.len();
                self.sel = 0;
            }
            KeyCode::Enter => {
                if let Some(id) = self.selecionado().map(|b| b.id) {
                    self.abrir_detalhe(id)?;
                }
            }
            KeyCode::Char('a') => {
                self.modal = Some(Modal::Form(Form::novo(ABAS[self.aba].unwrap_or(Status::QueroLer))));
            }
            KeyCode::Char('e') => {
                if let Some(b) = self.selecionado() {
                    self.modal = Some(Modal::Form(Form::de_livro(b)));
                }
            }
            KeyCode::Char('d') => {
                if let Some((id, titulo)) = self.selecionado().map(|b| (b.id, b.title.clone())) {
                    self.confirmar(format!("Remover “{titulo}” e suas notas?"), Confirmacao::RemoverLivro(id));
                }
            }
            KeyCode::Char('/') => self.prompt("Filtrar por título ou autor", self.filtro.clone(), Acao::Filtro),
            KeyCode::Char('o') => self.ir_para(Tela::Busca),
            KeyCode::Char('?') => self.ir_para(Tela::Ajuda),
            KeyCode::Esc if !self.filtro.is_empty() => {
                self.filtro.clear();
                self.sel = 0;
            }
            KeyCode::Esc | KeyCode::Char('q') => self.tela = Tela::Menu,
            _ => {}
        }
        Ok(())
    }

    fn tecla_detalhe(&mut self, k: KeyEvent) -> Result<()> {
        let b = self.livro_atual()?;
        match k.code {
            KeyCode::Esc | KeyCode::Backspace => {
                self.recarregar()?;
                self.tela = Tela::Biblioteca;
            }
            KeyCode::Up | KeyCode::Char('k') => self.nota_sel = self.nota_sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') if self.nota_sel + 1 < self.notas.len() => self.nota_sel += 1,
            KeyCode::Char('p') => {
                let titulo = if b.pages > 0 {
                    format!("Página atual (de {})", b.pages)
                } else {
                    "Página atual".to_string()
                };
                self.prompt(titulo, b.current_page.to_string(), Acao::Pagina);
            }
            KeyCode::Char('s') => {
                let sel = Status::TODOS.iter().position(|s| *s == b.status).unwrap_or(0);
                self.modal = Some(Modal::Status { sel });
            }
            KeyCode::Char('r') => self.prompt("Nota (0 a 5)", b.rating.to_string(), Acao::Nota),
            KeyCode::Char('n') => self.prompt("Nova nota", "", Acao::NovaNota(NoteKind::Nota)),
            KeyCode::Char('c') => self.prompt("Nova citação", "", Acao::NovaNota(NoteKind::Citacao)),
            KeyCode::Char('x') => {
                if let Some(id) = self.notas.get(self.nota_sel).map(|n| n.id) {
                    self.confirmar("Remover a nota selecionada?", Confirmacao::RemoverNota(id));
                }
            }
            KeyCode::Char('l') => {
                if b.pages <= 0 {
                    bail!("informe o total de páginas (e) antes de criar um plano");
                }
                let form = match &self.plano {
                    Some(p) => FormPlano::novo(p.inicio, p.paginas_dia.to_string(), b.pages, p.pagina_inicial),
                    None => FormPlano::novo(hoje(), String::new(), b.pages, b.current_page),
                };
                self.modal = Some(Modal::Plano(form));
            }
            KeyCode::Char('L') => {
                if self.plano.is_none() {
                    bail!("este livro ainda não tem plano: l cria um");
                }
                self.confirmar(
                    format!("Recalcular o plano a partir de hoje, da página {}?", b.current_page),
                    Confirmacao::RecalcularPlano,
                );
            }
            KeyCode::Char('e') => self.modal = Some(Modal::Form(Form::de_livro(&b))),
            KeyCode::Char('d') => {
                self.confirmar(format!("Remover “{}” e suas notas?", b.title), Confirmacao::RemoverLivro(b.id));
            }
            KeyCode::Char('?') => self.ir_para(Tela::Ajuda),
            _ => {}
        }
        Ok(())
    }

    fn tecla_stats(&mut self, k: KeyEvent) -> Result<()> {
        match k.code {
            KeyCode::Left | KeyCode::Char('h') => {
                self.ano -= 1;
                self.abrir_stats()?;
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.ano += 1;
                self.abrir_stats()?;
            }
            KeyCode::Char('g') => {
                let atual = self.stats.as_ref().and_then(|s| s.meta).map(|m| m.to_string()).unwrap_or_default();
                self.prompt(format!("Meta de livros para {} (0 remove)", self.ano), atual, Acao::Meta);
            }
            KeyCode::Char('?') => self.ir_para(Tela::Ajuda),
            KeyCode::Esc | KeyCode::Char('q') => self.tela = Tela::Menu,
            _ => {}
        }
        Ok(())
    }

    fn tecla_busca(&mut self, k: KeyEvent) -> Result<()> {
        match k.code {
            KeyCode::Esc => {
                self.recarregar()?;
                self.tela = self.anterior;
            }
            KeyCode::Up => self.busca_sel = self.busca_sel.saturating_sub(1),
            KeyCode::Down if self.busca_sel + 1 < self.resultados.len() => self.busca_sel += 1,
            KeyCode::Enter => {
                let q = self.busca.valor().trim().to_string();
                if let Some(r) = self.resultados.get(self.busca_sel)
                    && q == self.ultima_busca
                {
                    self.modal = Some(Modal::Form(Form::de_resultado(r)));
                } else if !q.is_empty() && !self.buscando {
                    self.iniciar_busca(q);
                }
            }
            _ => {
                self.busca.tratar(k);
            }
        }
        Ok(())
    }

    fn tecla_modal(&mut self, k: KeyEvent) -> Result<()> {
        let Some(modal) = self.modal.take() else { return Ok(()) };
        match modal {
            Modal::Form(mut f) => {
                let n = CAMPOS.len();
                match k.code {
                    KeyCode::Esc => return Ok(()),
                    KeyCode::Enter => {
                        if let Err(e) = self.salvar_form(&f) {
                            self.modal = Some(Modal::Form(f));
                            return Err(e);
                        }
                        return Ok(());
                    }
                    KeyCode::Tab | KeyCode::Down => f.foco = (f.foco + 1) % n,
                    KeyCode::BackTab | KeyCode::Up => f.foco = (f.foco + n - 1) % n,
                    KeyCode::Left if f.foco == CAMPO_STATUS => f.status = f.status.anterior(),
                    KeyCode::Right | KeyCode::Char(' ') if f.foco == CAMPO_STATUS => f.status = f.status.proximo(),
                    _ => {
                        let foco = f.foco;
                        if let Some(i) = f.entrada_mut(foco) {
                            i.tratar(k);
                        }
                    }
                }
                self.modal = Some(Modal::Form(f));
            }
            Modal::Plano(mut f) => {
                match k.code {
                    KeyCode::Esc => return Ok(()),
                    KeyCode::Enter => {
                        if let Err(e) = self.salvar_plano(&f) {
                            self.modal = Some(Modal::Plano(f));
                            return Err(e);
                        }
                        return Ok(());
                    }
                    KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => f.foco = 1 - f.foco,
                    _ => {
                        let foco = f.foco;
                        f.entradas[foco].tratar(k);
                    }
                }
                self.modal = Some(Modal::Plano(f));
            }
            Modal::Prompt { titulo, mut input, acao } => match k.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    if let Err(e) = self.executar_prompt(acao, input.valor()) {
                        self.modal = Some(Modal::Prompt { titulo, input, acao });
                        return Err(e);
                    }
                }
                _ => {
                    input.tratar(k);
                    self.modal = Some(Modal::Prompt { titulo, input, acao });
                }
            },
            Modal::Confirmar { texto, acao } => match k.code {
                KeyCode::Char('s' | 'S' | 'y' | 'Y') | KeyCode::Enter => self.executar_confirmacao(acao)?,
                KeyCode::Char('n' | 'N') | KeyCode::Esc => {}
                _ => self.modal = Some(Modal::Confirmar { texto, acao }),
            },
            Modal::Status { mut sel } => {
                let n = Status::TODOS.len();
                match k.code {
                    KeyCode::Esc => return Ok(()),
                    KeyCode::Up | KeyCode::Char('k') => sel = (sel + n - 1) % n,
                    KeyCode::Down | KeyCode::Char('j') => sel = (sel + 1) % n,
                    KeyCode::Char(c @ '1'..='4') => return self.mudar_status(Status::TODOS[c as usize - '1' as usize]),
                    KeyCode::Enter => return self.mudar_status(Status::TODOS[sel]),
                    _ => {}
                }
                self.modal = Some(Modal::Status { sel });
            }
        }
        Ok(())
    }

    fn salvar_form(&mut self, f: &Form) -> Result<()> {
        let texto = |i: usize| f.entrada(i).map_or("", |e| e.valor().trim()).to_string();
        let titulo = texto(0);
        if titulo.is_empty() {
            bail!("o título é obrigatório");
        }
        let paginas = match texto(3).as_str() {
            "" => 0,
            s => s.parse::<i64>().ok().filter(|p| *p >= 0).context("páginas deve ser um número")?,
        };
        let mut b = match f.id {
            Some(id) => self.db.livro(id)?,
            None => Book::novo(),
        };
        b.title = titulo;
        b.author = texto(1);
        b.isbn = texto(2);
        b.pages = paginas;
        b.review = texto(5);
        if b.pages > 0 {
            b.current_page = b.current_page.min(b.pages);
        }
        if f.id.is_none() || b.status != f.status {
            b.mudar_status(f.status, hoje());
        }
        match f.id {
            Some(_) => {
                self.db.salvar_livro(&b)?;
                self.recarregar_detalhe()?;
                self.ok("Livro atualizado.");
            }
            None => {
                let id = self.db.inserir_livro(&b)?;
                self.recarregar()?;
                self.abrir_detalhe(id)?;
                self.ok("Livro adicionado.");
            }
        }
        Ok(())
    }

    fn executar_prompt(&mut self, acao: Acao, valor: &str) -> Result<()> {
        let valor = valor.trim();
        match acao {
            Acao::Filtro => {
                self.filtro = valor.to_string();
                self.sel = 0;
            }
            Acao::Pagina => {
                let p: i64 = valor.parse().context("digite um número de página")?;
                let mut b = self.livro_atual()?;
                let chegou_ao_fim = b.registrar_pagina(p, hoje());
                self.db.salvar_livro(&b)?;
                self.recarregar_detalhe()?;
                if chegou_ao_fim {
                    self.confirmar("Chegou na última página! Marcar como lido?", Confirmacao::MarcarLido);
                } else {
                    self.ok("Progresso salvo.");
                }
            }
            Acao::Nota => {
                let n: u8 = valor.parse().ok().filter(|n| *n <= 5).context("a nota vai de 0 a 5")?;
                let mut b = self.livro_atual()?;
                b.rating = n;
                self.db.salvar_livro(&b)?;
                self.recarregar_detalhe()?;
            }
            Acao::NovaNota(kind) => {
                if valor.is_empty() {
                    bail!("o texto está vazio");
                }
                let b = self.livro_atual()?;
                self.db.inserir_nota(&Note {
                    id: 0,
                    book_id: b.id,
                    kind,
                    page: (b.current_page > 0).then_some(b.current_page),
                    body: valor.to_string(),
                    created_at: hoje(),
                })?;
                self.recarregar_detalhe()?;
                self.nota_sel = self.notas.len().saturating_sub(1);
                self.ok(if kind == NoteKind::Citacao { "Citação salva." } else { "Nota salva." });
            }
            Acao::Meta => {
                let n: i64 = if valor.is_empty() { 0 } else { valor.parse().context("digite um número de livros")? };
                self.db.definir_meta(self.ano, n)?;
                self.abrir_stats()?;
            }
        }
        Ok(())
    }

    fn executar_confirmacao(&mut self, acao: Confirmacao) -> Result<()> {
        match acao {
            Confirmacao::RemoverLivro(id) => {
                self.db.remover_livro(id)?;
                self.livro = None;
                self.recarregar()?;
                self.tela = Tela::Biblioteca;
                self.ok("Livro removido.");
            }
            Confirmacao::RemoverNota(id) => {
                self.db.remover_nota(id)?;
                self.recarregar_detalhe()?;
            }
            Confirmacao::MarcarLido => self.mudar_status(Status::Lido)?,
            Confirmacao::RecalcularPlano => {
                let b = self.livro_atual()?;
                let mut p = self.plano.clone().context("este livro não tem plano")?;
                p.inicio = hoje();
                p.pagina_inicial = b.current_page;
                self.db.salvar_plano(&p)?;
                self.recarregar_detalhe()?;
                self.ok(format!("Plano recalculado: termina em {}.", p.fim(b.pages).format("%d/%m/%Y")));
            }
        }
        Ok(())
    }

    fn salvar_plano(&mut self, f: &FormPlano) -> Result<()> {
        let (inicio, paginas_dia) = f.ler()?;
        let b = self.livro_atual()?;
        let plano = self.db.aplicar_plano(&b, inicio, f.pagina_inicial, paginas_dia)?;
        self.recarregar_detalhe()?;
        match plano {
            Some(p) => self.ok(format!("Plano salvo: termina em {}.", p.fim(b.pages).format("%d/%m/%Y"))),
            None => self.ok("Plano removido."),
        }
        Ok(())
    }

    fn mudar_status(&mut self, novo: Status) -> Result<()> {
        let mut b = self.livro_atual()?;
        b.mudar_status(novo, hoje());
        self.db.salvar_livro(&b)?;
        self.recarregar_detalhe()?;
        self.ok(format!("Status: {} {}", novo.simbolo(), novo.nome()));
        Ok(())
    }
}
