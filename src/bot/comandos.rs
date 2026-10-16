//! Núcleo do bot: interpreta mensagens e toques em botões e monta as
//! respostas. Não fala com o Telegram (a busca online é injetada), então dá
//! para testar tudo com um banco em memória.

use anyhow::{Context, Result, bail};
use chrono::{Datelike, NaiveDate};

use crate::codigo_barras;
use crate::db::Db;
use crate::model::{Book, Note, NoteKind, Plano, Situacao, Status, ler_categorias, plural, separar_pagina};
use crate::openlibrary::Resultado;
use crate::ui::milhar;

pub const AJUDA: &str = "📚 Nasus — curador das suas leituras

Para registrar o progresso, me mande:
120 — estou na página 120
+25 — li mais 25 páginas
Com mais de um livro em andamento, eu pergunto qual (ou use 120 #3).

Consultar:
/lendo — livros em andamento e o plano de hoje
/estante — livros que você tem
/fila — livros que você quer ler (a seguir)
/livro #3 — detalhes de um livro
/meta — meta de livros do ano (/meta 12 define)

Organizar:
/buscar duna — busca na Open Library por título ou ISBN e põe na estante com um toque
📷 foto do código de barras do livro — eu leio o ISBN e busco
/novo Título | Autor | páginas — adiciona à mão
/status #3 lendo — estante, fila, lendo, lido ou abandonado
/categoria #3 romance, filosofia — categorias do livro (/categorias lista todas)
/avaliar #3 5 — nota de 0 a 5
/nota texto — anota algo no livro atual (/nota #3 texto)
/citacao texto — guarda uma citação
Comece com p.115 para marcar a página (/citacao p.115 texto); sem isso, vale a página atual.

Plano de leitura:
/plano 25 — 25 páginas por dia a partir de hoje (/plano #3 25 15/10 escolhe livro e início)
/plano — mostra o plano; /plano 0 remove
/recalcular — refaz o plano a partir de hoje e da página atual

Todo dia às 8h eu mando a meta de leitura do dia e, às 21h, lembro do que ainda falta.";

/// Resposta a uma mensagem ou toque: texto e, opcionalmente, botões inline
/// (rótulo, dados do callback) organizados em linhas.
#[derive(Debug, Default, PartialEq)]
pub struct Resposta {
    pub texto: String,
    pub botoes: Vec<Vec<(String, String)>>,
}

impl Resposta {
    fn texto(t: impl Into<String>) -> Self {
        Resposta { texto: t.into(), botoes: Vec::new() }
    }
}

pub type Busca = fn(&str) -> Result<Vec<Resultado>>;

pub struct Nucleo {
    db: Db,
    buscar: Busca,
    /// resultados da última /buscar, alvo dos botões "add:N"
    resultados: Vec<Resultado>,
}

/// separar_id tira um "#12" do começo ou do fim do texto.
fn separar_id(s: &str) -> (Option<i64>, String) {
    let mut partes: Vec<&str> = s.split_whitespace().collect();
    let id_de = |p: &str| p.strip_prefix('#').and_then(|n| n.parse::<i64>().ok());
    if let Some(id) = partes.first().and_then(|p| id_de(p)) {
        partes.remove(0);
        return (Some(id), partes.join(" "));
    }
    if let Some(id) = partes.last().and_then(|p| id_de(p)) {
        partes.pop();
        return (Some(id), partes.join(" "));
    }
    (None, partes.join(" "))
}

/// ler_pagina reconhece "120", "+25", "120 #3": (relativo, número, livro).
fn ler_pagina(texto: &str) -> Option<(bool, i64, Option<i64>)> {
    let (id, resto) = separar_id(texto);
    let (relativo, num) = match resto.strip_prefix('+') {
        Some(n) => (true, n.trim()),
        None => (false, resto.as_str()),
    };
    if num.is_empty() || !num.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((relativo, num.parse().ok()?, id))
}

/// ler_data aceita dd/mm (ano corrente) e dd/mm/aaaa.
fn ler_data(s: &str, hoje: NaiveDate) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%d/%m/%Y")
        .ok()
        .or_else(|| NaiveDate::parse_from_str(&format!("{s}/{}", hoje.year()), "%d/%m/%Y").ok())
}

fn ler_status(s: &str) -> Option<Status> {
    match s.to_lowercase().as_str() {
        "estante" | "tenho" => Some(Status::Estante),
        "fila" | "quero" | "quero ler" => Some(Status::QueroLer),
        "lendo" | "comecar" | "começar" => Some(Status::Lendo),
        "lido" | "terminei" | "terminado" => Some(Status::Lido),
        "abandonado" | "abandonar" | "parei" => Some(Status::Abandonado),
        _ => None,
    }
}

fn data_curta(d: NaiveDate) -> String {
    d.format("%d/%m").to_string()
}

fn progresso(b: &Book) -> String {
    match b.progresso() {
        Some(p) => format!("pág. {} de {} ({:.0}%)", b.current_page, b.pages, p * 100.0),
        None if b.current_page > 0 => format!("pág. {}", b.current_page),
        None => "não começado".into(),
    }
}

fn descricao_plano(b: &Book, p: &Plano, hoje: NaiveDate) -> String {
    let mut s = format!("🗓 {} pág/dia · {} → {}", p.paginas_dia, data_curta(p.inicio), data_curta(p.fim(b.pages)));
    match p.situacao(b, hoje) {
        sit @ (Situacao::Concluido | Situacao::NaoComecou { .. }) => s += &format!("\n   {}", sit.texto()),
        sit => s += &format!("\n   hoje: até a pág. {} · {}", p.meta_ate(hoje, b.pages), sit.texto()),
    }
    s
}

impl Nucleo {
    pub fn new(db: Db, buscar: Busca) -> Self {
        Nucleo { db, buscar, resultados: Vec::new() }
    }

    pub fn mensagem(&mut self, texto: &str, hoje: NaiveDate) -> Resposta {
        self.interpretar(texto.trim(), hoje).unwrap_or_else(|e| Resposta::texto(format!("❌ {e:#}")))
    }

    /// foto lê o ISBN no código de barras da imagem e busca o livro.
    pub fn foto(&mut self, imagem: &[u8]) -> Resposta {
        self.ler_foto(imagem).unwrap_or_else(|e| Resposta::texto(format!("❌ {e:#}")))
    }

    fn ler_foto(&mut self, imagem: &[u8]) -> Result<Resposta> {
        let Some(isbn) = codigo_barras::ler_isbn(imagem)? else {
            return Ok(Resposta::texto(
                "Não achei o código de barras de um livro na foto. Tente mais perto, com boa luz e o código reto \
                 (mandar como arquivo evita a compressão) — ou mande o número com /buscar 978…",
            ));
        };
        let mut r = self.buscar(&isbn)?;
        r.texto = format!("📷 ISBN {isbn}\n{}", r.texto);
        Ok(r)
    }

    pub fn callback(&mut self, dados: &str, hoje: NaiveDate) -> Resposta {
        self.tocar(dados, hoje).unwrap_or_else(|e| Resposta::texto(format!("❌ {e:#}")))
    }

    /// fecha_teclado diz se os botões da mensagem somem depois do toque: os
    /// de escolha sim; os da lista de busca ficam, para adicionar vários.
    pub fn fecha_teclado(dados: &str) -> bool {
        !dados.starts_with("add:")
    }

    fn interpretar(&mut self, texto: &str, hoje: NaiveDate) -> Result<Resposta> {
        if let Some(cmd) = texto.strip_prefix('/') {
            let (nome, resto) = cmd.split_once(char::is_whitespace).unwrap_or((cmd, ""));
            // em grupos o Telegram manda /comando@nome_do_bot
            let nome = nome.split('@').next().unwrap_or_default().to_lowercase();
            let resto = resto.trim();
            return match nome.as_str() {
                "start" | "ajuda" | "help" => Ok(Resposta::texto(AJUDA)),
                "lendo" => self.lendo(hoje),
                "estante" => self.estante(),
                "fila" => self.fila(),
                "livro" => {
                    let b = self.alvo(separar_id(resto).0)?;
                    self.detalhe(&b, hoje)
                }
                "buscar" => self.buscar(resto),
                "novo" => self.novo(resto, hoje),
                "status" => self.status(resto, hoje),
                "avaliar" => self.avaliar(resto),
                "categoria" => self.categoria(resto),
                "categorias" => self.categorias(),
                "nota" => self.nota(resto, NoteKind::Nota, hoje),
                "citacao" | "citação" => self.nota(resto, NoteKind::Citacao, hoje),
                "plano" => self.plano(resto, hoje),
                "recalcular" => self.recalcular(resto, hoje),
                "meta" => self.meta(resto, hoje),
                _ => bail!("comando desconhecido; mande /ajuda"),
            };
        }
        match ler_pagina(texto) {
            Some((relativo, n, id)) => self.pagina(relativo, n, id, hoje),
            None => bail!("não entendi. Mande a página (120) ou quantas leu (+25), ou /ajuda"),
        }
    }

    fn tocar(&mut self, dados: &str, hoje: NaiveDate) -> Result<Resposta> {
        let mut partes = dados.split(':');
        let acao = partes.next().unwrap_or_default();
        let a = partes.next().and_then(|x| x.parse::<i64>().ok());
        let b = partes.next().and_then(|x| x.parse::<i64>().ok());
        match (acao, a, b) {
            ("pag", Some(id), Some(n)) => {
                let livro = self.db.livro(id)?;
                self.registrar(livro, n, hoje)
            }
            ("rel", Some(id), Some(n)) => {
                let livro = self.db.livro(id)?;
                let pagina = livro.current_page + n;
                self.registrar(livro, pagina, hoje)
            }
            ("lido", Some(id), _) => self.mudar_status(id, Status::Lido, hoje),
            ("lendo", Some(id), _) => self.mudar_status(id, Status::Lendo, hoje),
            ("fila", Some(id), _) => self.mudar_status(id, Status::QueroLer, hoje),
            ("add", Some(i), _) => self.adicionar(i as usize, hoje),
            ("x", ..) => Ok(Resposta::texto("👍")),
            _ => bail!("botão desconhecido"),
        }
    }

    fn em_andamento(&self) -> Result<Vec<Book>> {
        Ok(self.db.livros()?.into_iter().filter(|b| b.status == Status::Lendo).collect())
    }

    /// alvo é o livro indicado por #id ou, sem id, o único em andamento.
    fn alvo(&self, id: Option<i64>) -> Result<Book> {
        if let Some(id) = id {
            return self.db.livro(id).map_err(|_| anyhow::anyhow!("não achei o livro #{id}"));
        }
        let mut lendo = self.em_andamento()?;
        match lendo.len() {
            1 => Ok(lendo.remove(0)),
            0 => bail!("nenhum livro em andamento; indique qual com #id (veja /fila)"),
            _ => bail!("você está lendo mais de um livro; indique qual com #id (veja /lendo)"),
        }
    }

    fn pagina(&mut self, relativo: bool, n: i64, id: Option<i64>, hoje: NaiveDate) -> Result<Resposta> {
        if id.is_none() {
            let lendo = self.em_andamento()?;
            if lendo.len() > 1 {
                let acao = if relativo { "rel" } else { "pag" };
                return Ok(Resposta {
                    texto: if relativo { format!("Mais {n} páginas em qual livro?") } else { format!("Página {n} de qual livro?") },
                    botoes: lendo.iter().map(|b| vec![(b.title.clone(), format!("{acao}:{}:{n}", b.id))]).collect(),
                });
            }
        }
        let livro = self.alvo(id)?;
        let pagina = if relativo { livro.current_page + n } else { n };
        self.registrar(livro, pagina, hoje)
    }

    fn registrar(&mut self, mut b: Book, pagina: i64, hoje: NaiveDate) -> Result<Resposta> {
        let chegou_ao_fim = b.registrar_pagina(pagina, hoje);
        self.db.salvar_livro(&b)?;
        let mut r = Resposta::texto(format!("📖 {}\n{}", b.title, progresso(&b)));
        if let Some(p) = self.db.plano(b.id)? {
            r.texto += &format!("\n{}", p.situacao(&b, hoje).texto());
        }
        if chegou_ao_fim {
            r.texto += "\n\n🏁 Chegou na última página!";
            r.botoes = vec![vec![
                ("✔ Marcar como lido".into(), format!("lido:{}", b.id)),
                ("Ainda não".into(), "x".into()),
            ]];
        }
        Ok(r)
    }

    fn mudar_status(&mut self, id: i64, novo: Status, hoje: NaiveDate) -> Result<Resposta> {
        let mut b = self.db.livro(id)?;
        b.mudar_status(novo, hoje);
        self.db.salvar_livro(&b)?;
        let extra = match novo {
            Status::Lido => {
                let e = self.db.estatisticas(hoje.year())?;
                match e.meta {
                    Some(m) if m > 0 => format!("\n🎯 {} de {m} livros na meta de {}.", e.lidos, hoje.year()),
                    _ => format!("\n{} lidos em {}.", plural(e.lidos, "livro"), hoje.year()),
                }
            }
            Status::Lendo => "\nBoa leitura! Mande a página quando avançar, ou /plano 25 para um ritmo.".into(),
            Status::QueroLer => "\nEstá na /fila.".into(),
            _ => String::new(),
        };
        Ok(Resposta::texto(format!("{} {}: {}{extra}", novo.simbolo(), b.title, novo.nome())))
    }

    fn lendo(&self, hoje: NaiveDate) -> Result<Resposta> {
        let lendo = self.em_andamento()?;
        if lendo.is_empty() {
            return Ok(Resposta::texto("Nenhum livro em andamento. Veja a /fila ou use /buscar."));
        }
        let mut t = String::from("📚 Lendo agora");
        for b in &lendo {
            t += &format!("\n\n#{} {}\n   {}", b.id, b.title, progresso(b));
            if let Some(p) = self.db.plano(b.id)? {
                t += &format!("\n   {}", descricao_plano(b, &p, hoje).replace('\n', "\n   "));
            }
        }
        Ok(Resposta::texto(t))
    }

    fn fila(&self) -> Result<Resposta> {
        let fila: Vec<Book> = self.db.livros()?.into_iter().filter(|b| b.status == Status::QueroLer).collect();
        if fila.is_empty() {
            return Ok(Resposta::texto("A fila está vazia. Escolha um livro da /estante ou use /buscar."));
        }
        let mut t = String::from("📋 Quero ler");
        for b in &fila {
            let autor = if b.author.is_empty() { String::new() } else { format!(" — {}", b.author) };
            t += &format!("\n#{} {}{autor}", b.id, b.title);
        }
        let botoes = fila
            .iter()
            .take(8)
            .map(|b| vec![(format!("▶ Começar: {}", b.title), format!("lendo:{}", b.id))])
            .collect();
        Ok(Resposta { texto: t, botoes })
    }

    fn estante(&self) -> Result<Resposta> {
        let estante: Vec<Book> = self.db.livros()?.into_iter().filter(|b| b.status == Status::Estante).collect();
        if estante.is_empty() {
            return Ok(Resposta::texto("A estante está vazia. Use /buscar ou mande a foto do código de barras."));
        }
        let mut t = String::from("📚 Na estante");
        for b in &estante {
            let autor = if b.author.is_empty() { String::new() } else { format!(" — {}", b.author) };
            let cats = if b.categorias.is_empty() { String::new() } else { format!(" [{}]", b.categorias.join(", ")) };
            t += &format!("\n#{} {}{autor}{cats}", b.id, b.title);
        }
        let botoes = estante
            .iter()
            .take(8)
            .map(|b| vec![(format!("📋 Quero ler: {}", b.title), format!("fila:{}", b.id))])
            .collect();
        Ok(Resposta { texto: t, botoes })
    }

    fn detalhe(&self, b: &Book, hoje: NaiveDate) -> Result<Resposta> {
        let mut t = format!("{} #{} {}", b.status.simbolo(), b.id, b.title);
        if !b.author.is_empty() {
            t += &format!("\npor {}", b.author);
        }
        t += &format!("\n{} · {}", b.status.nome(), progresso(b));
        if let Some(i) = b.started_at {
            let fim = b.finished_at.map_or("…".into(), data_curta);
            t += &format!("\n{} → {fim}", data_curta(i));
        }
        if b.rating > 0 {
            t += &format!("\n{}", b.estrelas());
        }
        if !b.categorias.is_empty() {
            t += &format!("\n🏷 {}", b.categorias.join(" · "));
        }
        if let Some(p) = self.db.plano(b.id)? {
            t += &format!("\n{}", descricao_plano(b, &p, hoje));
        }
        let notas = self.db.notas(b.id)?;
        for n in notas.iter().rev().take(3) {
            let pagina = n.page.map_or(String::new(), |p| format!("p. {p} "));
            let corpo = match n.kind {
                NoteKind::Nota => format!("• {}", n.body),
                NoteKind::Citacao => format!("“{}”", n.body),
            };
            t += &format!("\n{pagina}{corpo}");
        }
        Ok(Resposta::texto(t))
    }

    fn buscar(&mut self, q: &str) -> Result<Resposta> {
        if q.is_empty() {
            bail!("use /buscar seguido do título ou ISBN");
        }
        let mut r = (self.buscar)(q).context("falha na busca")?;
        r.truncate(5);
        self.resultados = r;
        if self.resultados.is_empty() {
            return Ok(Resposta::texto("Nada encontrado na Open Library."));
        }
        let mut t = String::from("🔎 Toque para pôr na estante:");
        for (i, r) in self.resultados.iter().enumerate() {
            let mut extra = Vec::new();
            if let Some(a) = r.ano {
                extra.push(a.to_string());
            }
            if r.pages > 0 {
                extra.push(format!("{} p.", r.pages));
            }
            let autor = if r.author.is_empty() { String::new() } else { format!(" — {}", r.author) };
            let extra = if extra.is_empty() { String::new() } else { format!(" ({})", extra.join(", ")) };
            t += &format!("\n{}. {}{autor}{extra}", i + 1, r.title);
        }
        let botoes =
            vec![(0..self.resultados.len()).map(|i| (format!("➕ {}", i + 1), format!("add:{i}"))).collect()];
        Ok(Resposta { texto: t, botoes })
    }

    fn adicionar(&mut self, i: usize, hoje: NaiveDate) -> Result<Resposta> {
        let r = self.resultados.get(i).cloned().context("essa busca expirou; busque de novo")?;
        let mut b = Book { title: r.title, author: r.author, isbn: r.isbn, pages: r.pages, ..Book::novo() };
        b.mudar_status(Status::Estante, hoje);
        self.inserir(b)
    }

    fn inserir(&mut self, b: Book) -> Result<Resposta> {
        let id = self.db.inserir_livro(&b)?;
        let paginas = if b.pages > 0 { format!(" ({} páginas)", b.pages) } else { String::new() };
        Ok(Resposta {
            texto: format!("➕ Na estante: #{id} {}{paginas}\nPara pôr categorias: /categoria #{id} romance, filosofia", b.title),
            botoes: vec![vec![
                ("📋 Quero ler".into(), format!("fila:{id}")),
                ("▶ Começar a ler".into(), format!("lendo:{id}")),
            ]],
        })
    }

    fn novo(&mut self, resto: &str, hoje: NaiveDate) -> Result<Resposta> {
        let campos: Vec<&str> = resto.split('|').map(str::trim).collect();
        let titulo = campos.first().copied().unwrap_or_default();
        if titulo.is_empty() {
            bail!("use /novo Título | Autor | páginas");
        }
        let pages = match campos.get(2).copied().unwrap_or_default() {
            "" => 0,
            p => p.parse().ok().filter(|p: &i64| *p >= 0).context("páginas deve ser um número")?,
        };
        let mut b = Book {
            title: titulo.into(),
            author: campos.get(1).copied().unwrap_or_default().into(),
            pages,
            ..Book::novo()
        };
        b.mudar_status(Status::Estante, hoje);
        self.inserir(b)
    }

    /// categoria define as categorias do livro; sem texto, apaga.
    fn categoria(&mut self, resto: &str) -> Result<Resposta> {
        let (id, texto) = separar_id(resto);
        if id.is_none() && texto.is_empty() {
            return self.categorias();
        }
        let mut b = self.alvo(id)?;
        b.categorias = ler_categorias(&texto);
        self.db.salvar_livro(&b)?;
        Ok(Resposta::texto(if b.categorias.is_empty() {
            format!("🏷 {}: sem categorias", b.title)
        } else {
            format!("🏷 {}: {}", b.title, b.categorias.join(" · "))
        }))
    }

    fn categorias(&self) -> Result<Resposta> {
        let lista = self.db.categorias()?;
        if lista.is_empty() {
            return Ok(Resposta::texto("Nenhuma categoria ainda. Use /categoria #3 romance, filosofia"));
        }
        let mut t = String::from("🏷 Categorias");
        for (nome, n) in lista {
            t += &format!("\n{nome} ({n})");
        }
        Ok(Resposta::texto(t))
    }

    fn status(&mut self, resto: &str, hoje: NaiveDate) -> Result<Resposta> {
        let (id, nome) = separar_id(resto);
        let novo = ler_status(&nome).context("use /status #id estante|fila|lendo|lido|abandonado")?;
        let b = self.alvo(id)?;
        self.mudar_status(b.id, novo, hoje)
    }

    fn avaliar(&mut self, resto: &str) -> Result<Resposta> {
        let (id, nota) = separar_id(resto);
        let nota: u8 = nota.parse().ok().filter(|n| *n <= 5).context("use /avaliar #id 0 a 5")?;
        let mut b = self.alvo(id)?;
        b.rating = nota;
        self.db.salvar_livro(&b)?;
        let estrelas = if nota == 0 { "sem nota".into() } else { b.estrelas() };
        Ok(Resposta::texto(format!("⭐ {}: {estrelas}", b.title)))
    }

    fn nota(&mut self, resto: &str, kind: NoteKind, hoje: NaiveDate) -> Result<Resposta> {
        let (id, texto) = separar_id(resto);
        let (pagina, texto) = separar_pagina(&texto);
        if texto.is_empty() {
            bail!("escreva o texto depois do comando");
        }
        let b = self.alvo(id)?;
        let page = pagina.or((b.current_page > 0).then_some(b.current_page));
        self.db.inserir_nota(&Note { id: 0, book_id: b.id, kind, page, body: texto.into(), created_at: hoje })?;
        let onde = page.map_or(String::new(), |p| format!(" (pág. {p})"));
        let o_que = if kind == NoteKind::Citacao { "💬 Citação salva" } else { "📝 Nota salva" };
        Ok(Resposta::texto(format!("{o_que} em {}{onde}.", b.title)))
    }

    fn plano(&mut self, resto: &str, hoje: NaiveDate) -> Result<Resposta> {
        let (id, args) = separar_id(resto);
        let b = self.alvo(id)?;
        let mut args = args.split_whitespace();
        let Some(ritmo) = args.next() else {
            return match self.db.plano(b.id)? {
                Some(p) => Ok(Resposta::texto(format!("{}\n{}", b.title, descricao_plano(&b, &p, hoje)))),
                None => Ok(Resposta::texto(format!("{} não tem plano. Use /plano 25 para criar.", b.title))),
            };
        };
        let paginas_dia: i64 = ritmo.parse().ok().filter(|n| *n >= 0).context("use /plano <páginas por dia> [dd/mm]")?;
        let inicio = match args.next() {
            Some(d) => ler_data(d, hoje).context("data de início no formato dd/mm ou dd/mm/aaaa")?,
            None => hoje,
        };
        match self.db.aplicar_plano(&b, inicio, b.current_page, paginas_dia)? {
            None => Ok(Resposta::texto(format!("🗑 Plano de {} removido.", b.title))),
            Some(p) => {
                let b = self.db.livro(b.id)?;
                Ok(Resposta::texto(format!(
                    "✔ Plano criado para {}\n{} páginas em {} dias\n{}",
                    b.title,
                    b.pages - p.pagina_inicial,
                    p.dias(b.pages),
                    descricao_plano(&b, &p, hoje)
                )))
            }
        }
    }

    fn recalcular(&mut self, resto: &str, hoje: NaiveDate) -> Result<Resposta> {
        let b = self.alvo(separar_id(resto).0)?;
        let atual = self.db.plano(b.id)?.with_context(|| format!("{} não tem plano; use /plano 25", b.title))?;
        let p = self
            .db
            .aplicar_plano(&b, hoje, b.current_page, atual.paginas_dia)?
            .context("plano não foi recriado")?;
        Ok(Resposta::texto(format!("🔄 Plano recalculado para {}\n{}", b.title, descricao_plano(&b, &p, hoje))))
    }

    fn meta(&mut self, resto: &str, hoje: NaiveDate) -> Result<Resposta> {
        let ano = hoje.year();
        if !resto.is_empty() {
            let n: i64 = resto.parse().ok().filter(|n| *n >= 0).context("use /meta <número de livros>")?;
            self.db.definir_meta(ano, n)?;
        }
        let e = self.db.estatisticas(ano)?;
        let mut t = match e.meta {
            Some(m) if m > 0 => {
                let pct = (e.lidos * 100 / m).min(100);
                let cheio = (pct / 10) as usize;
                format!(
                    "🎯 Meta de {ano}: {} de {m} livros ({pct}%)\n{}{}",
                    e.lidos,
                    "▰".repeat(cheio),
                    "▱".repeat(10 - cheio)
                )
            }
            _ => format!("🎯 Sem meta para {ano} (use /meta 12).\n{} lidos este ano.", plural(e.lidos, "livro")),
        };
        t += &format!("\nPáginas lidas: {} · Lendo agora: {}", milhar(e.paginas), e.lendo);
        if let Some(n) = e.nota_media {
            t += &format!("\nNota média: {}", format!("{n:.1}").replace('.', ","));
        }
        Ok(Resposta::texto(t))
    }

    /// livros em andamento com plano já começado, e o plano de cada um.
    fn planos_ativos(&self, hoje: NaiveDate) -> Result<Vec<(Book, Plano)>> {
        let mut v = Vec::new();
        for b in self.em_andamento()? {
            if let Some(p) = self.db.plano(b.id)?
                && hoje >= p.inicio
            {
                v.push((b, p));
            }
        }
        Ok(v)
    }

    /// lembrete_manha lista a meta de hoje de cada plano ativo.
    pub fn lembrete_manha(&self, hoje: NaiveDate) -> Result<Option<Resposta>> {
        let planos = self.planos_ativos(hoje)?;
        if planos.is_empty() {
            return Ok(None);
        }
        let mut t = String::from("☀️ Leitura de hoje");
        for (b, p) in &planos {
            t += &format!(
                "\n\n{}\nler até a pág. {} · {}",
                b.title,
                p.meta_ate(hoje, b.pages),
                p.situacao(b, hoje).texto()
            );
        }
        Ok(Some(Resposta::texto(t)))
    }

    /// lembrete_noite avisa os planos com páginas faltando hoje, com um botão
    /// para registrar a meta cumprida de uma vez.
    pub fn lembrete_noite(&self, hoje: NaiveDate) -> Result<Option<Resposta>> {
        let mut r = Resposta::texto("🌙 Ainda dá tempo hoje:");
        for (b, p) in self.planos_ativos(hoje)? {
            let faltam = match p.situacao(&b, hoje) {
                Situacao::FaltamHoje(n) | Situacao::Atrasado { para_hoje: n, .. } => n,
                _ => continue,
            };
            let meta = p.meta_ate(hoje, b.pages);
            r.texto += &format!("\n\n{}\nfaltam {} (até a pág. {meta})", b.title, plural(faltam, "página"));
            r.botoes.push(vec![(format!("✔ Li até a pág. {meta}: {}", b.title), format!("pag:{}:{meta}", b.id))]);
        }
        Ok((!r.botoes.is_empty()).then_some(r))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dia(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
    }

    fn busca_falsa(q: &str) -> Result<Vec<Resultado>> {
        Ok(vec![Resultado { title: format!("{q} (resultado)"), author: "Autor".into(), isbn: String::new(), pages: 300, ano: Some(2000) }])
    }

    fn nucleo() -> Nucleo {
        Nucleo::new(Db::em_memoria().unwrap(), busca_falsa)
    }

    #[test]
    fn interpreta_paginas_e_ids() {
        assert_eq!(ler_pagina("120"), Some((false, 120, None)));
        assert_eq!(ler_pagina("+25"), Some((true, 25, None)));
        assert_eq!(ler_pagina("+ 25 #3"), Some((true, 25, Some(3))));
        assert_eq!(ler_pagina("#3 120"), Some((false, 120, Some(3))));
        assert_eq!(ler_pagina("oi"), None);
        assert_eq!(ler_pagina("12a"), None);
        assert_eq!(separar_id("#2 gostei muito"), (Some(2), "gostei muito".into()));
        assert_eq!(ler_data("15/10", dia(1)), Some(dia(15)));
        assert_eq!(ler_data("15/10/2026", dia(1)), Some(dia(15)));
        assert_eq!(ler_data("amanhã", dia(1)), None);
    }

    #[test]
    fn fluxo_novo_livro_plano_e_progresso() {
        let mut n = nucleo();
        let r = n.mensagem("/novo Livro de testes | Fulano | 800", dia(14));
        assert!(r.texto.contains("Na estante: #1 Livro de testes"), "{}", r.texto);
        assert_eq!(r.botoes[0][0].1, "fila:1");
        assert_eq!(r.botoes[0][1].1, "lendo:1");

        assert!(n.mensagem("120", dia(14)).texto.starts_with("❌"), "nada em andamento ainda");

        let r = n.mensagem("/plano #1 25", dia(14));
        assert!(r.texto.contains("800 páginas em 32 dias"), "{}", r.texto);
        assert!(r.texto.contains("14/10 → 14/11"), "{}", r.texto);
        assert_eq!(n.db.livro(1).unwrap().status, Status::Lendo, "plano começa a leitura");

        let r = n.mensagem("40", dia(14));
        assert!(r.texto.contains("pág. 40 de 800 (5%)"), "{}", r.texto);
        assert!(r.texto.contains("15 páginas à frente"), "{}", r.texto);
        let r = n.mensagem("+10", dia(14));
        assert!(r.texto.contains("pág. 50 de 800"), "{}", r.texto);

        let manha = n.lembrete_manha(dia(15)).unwrap().unwrap();
        assert!(manha.texto.contains("ler até a pág. 50"), "{}", manha.texto);
        let noite = n.lembrete_noite(dia(16)).unwrap().unwrap();
        assert_eq!(noite.botoes[0][0].1, "pag:1:75");
        assert!(n.lembrete_noite(dia(14)).unwrap().is_none(), "meta de hoje já cumprida");

        let r = n.callback("pag:1:800", dia(16));
        assert!(r.texto.contains("Chegou na última página"), "{}", r.texto);
        assert_eq!(r.botoes[0][0].1, "lido:1");
        let r = n.callback("lido:1", dia(16));
        assert!(r.texto.contains("Lido"), "{}", r.texto);
        assert_eq!(n.db.livro(1).unwrap().finished_at, Some(dia(16)));
    }

    #[test]
    fn varios_livros_pergunta_qual() {
        let mut n = nucleo();
        n.mensagem("/novo A | | 100", dia(1));
        n.mensagem("/novo B | | 100", dia(1));
        n.callback("lendo:1", dia(1));
        n.callback("lendo:2", dia(1));
        let r = n.mensagem("+5", dia(2));
        assert_eq!(r.botoes, vec![vec![("A".to_string(), "rel:1:5".to_string())], vec![("B".into(), "rel:2:5".into())]]);
        n.callback("rel:2:5", dia(2));
        assert_eq!(n.db.livro(2).unwrap().current_page, 5);
        assert!(n.mensagem("/nota gostei", dia(2)).texto.contains("mais de um livro"));
        assert!(n.mensagem("/nota #2 gostei", dia(2)).texto.contains("Nota salva em B (pág. 5)"));
        let r = n.mensagem("/citacao #2 p.3 uma frase", dia(2));
        assert!(r.texto.contains("Citação salva em B (pág. 3)"), "{}", r.texto);
        assert_eq!(n.db.notas(2).unwrap().last().unwrap().body, "uma frase");
    }

    #[test]
    fn busca_adiciona_e_comandos_diversos() {
        let mut n = nucleo();
        let r = n.mensagem("/buscar duna", dia(1));
        assert_eq!(r.botoes[0][0].1, "add:0");
        assert!(!Nucleo::fecha_teclado("add:0"));
        let r = n.callback("add:0", dia(1));
        assert!(r.texto.contains("#1 duna (resultado) (300 páginas)"), "{}", r.texto);
        assert!(n.callback("add:9", dia(1)).texto.contains("expirou"));

        assert!(n.mensagem("/fila", dia(1)).texto.contains("vazia"), "livro novo vai para a estante");
        let r = n.mensagem("/estante", dia(1));
        assert!(r.texto.contains("#1 duna"), "{}", r.texto);
        assert_eq!(r.botoes[0][0].1, "fila:1");
        assert!(n.callback("fila:1", dia(1)).texto.contains("Quero ler"));
        assert!(n.mensagem("/fila", dia(1)).texto.contains("#1 duna"));
        assert!(n.mensagem("/estante", dia(1)).texto.contains("vazia"));

        let r = n.mensagem("/categoria #1 Ficção científica, clássico", dia(1));
        assert!(r.texto.contains("clássico · ficção científica"), "{}", r.texto);
        assert!(n.mensagem("/livro #1", dia(1)).texto.contains("🏷 clássico · ficção científica"));
        assert!(n.mensagem("/categorias", dia(1)).texto.contains("clássico (1)"));
        assert!(n.mensagem("/categoria #1", dia(1)).texto.contains("sem categorias"));
        assert!(n.mensagem("/status #1 lendo", dia(1)).texto.contains("Lendo"));
        assert!(n.mensagem("/avaliar 4", dia(1)).texto.contains("★★★★☆"));
        assert!(n.mensagem("/meta 12", dia(1)).texto.contains("0 de 12 livros"));
        assert!(n.mensagem("/lendo", dia(1)).texto.contains("#1 duna"));
        assert!(n.mensagem("/xyz", dia(1)).texto.contains("desconhecido"));
        assert!(n.mensagem("/start@nasus_bot", dia(1)).texto.starts_with("📚 Nasus"));
    }

    #[test]
    fn foto_do_codigo_de_barras_busca_o_isbn() {
        let mut n = nucleo();
        let r = n.foto(&codigo_barras::png_de_teste("9788535914849"));
        assert!(r.texto.starts_with("📷 ISBN 9788535914849\n"), "{}", r.texto);
        assert!(r.texto.contains("9788535914849 (resultado)"), "{}", r.texto);
        assert_eq!(r.botoes[0][0].1, "add:0");
        assert!(n.foto(&codigo_barras::png_de_teste("7891000315507")).texto.starts_with("Não achei"));
        assert!(n.foto(b"lixo").texto.starts_with("❌"));
    }
}
