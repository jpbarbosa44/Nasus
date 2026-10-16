//! Servidor MCP: `nasus mcp` deixa o Claude (Desktop ou Code) consultar a
//! biblioteca e, com `--escrita`, registrar leituras. Fala JSON-RPC 2.0 pela
//! entrada e saída padrão, uma mensagem por linha, como o `prisma mcp`.

use std::io::{BufRead, Write};
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{Datelike, Days, Local, NaiveDate};
use serde_json::{Map, Value, json};

use crate::db::{Db, citacoes};
use crate::model::{Book, Note, NoteKind, Status, ler_categorias, plural, separar_pagina};

pub const USO: &str = "uso: nasus mcp [--escrita]

Servidor MCP (Model Context Protocol) pela entrada e saída padrão, para o
Claude consultar a sua biblioteca. Sem --escrita, só lê; com --escrita, também
registra páginas, notas, status e empréstimos (com uma cópia do banco antes da
primeira gravação de cada sessão).

  claude mcp add nasus -- nasus mcp
  claude mcp add nasus -- nasus mcp --escrita";

/// versão do protocolo que este servidor conhece; se o cliente pedir outra, ecoa a dele
const VERSAO_PROTOCOLO: &str = "2025-06-18";

/// textos maiores que isso são cortados, com um aviso para refinar a consulta
const LIMITE_SAIDA: usize = 60_000;

const INSTRUCOES: &str = "Nasus é o curador de leituras do usuário: livros, páginas lidas, notas, \
citações, planos de leitura, categorias, séries e empréstimos. Os status são estante (tem o livro), \
quero_ler (fila), lendo, lido e abandonado. Use `livros` para achar o #id e `livro` para os detalhes \
de um; `buscar_notas` procura nas notas e citações de todos os livros. Responda em português.";

const INSTRUCOES_ESCRITA: &str = " Esta sessão pode gravar: confirme com o usuário antes de \
mudar algo que ele não pediu. Em `adicionar_nota`, o texto pode começar com a página (p.115).";

#[derive(Clone, Copy)]
enum Tipo {
    Texto,
    Inteiro,
    Logico,
}

struct Param {
    nome: &'static str,
    tipo: Tipo,
    descricao: &'static str,
    obrigatorio: bool,
    opcoes: &'static [&'static str],
}

struct Ferramenta {
    nome: &'static str,
    descricao: &'static str,
    params: &'static [Param],
    escreve: bool,
}

const fn p(nome: &'static str, tipo: Tipo, descricao: &'static str) -> Param {
    Param { nome, tipo, descricao, obrigatorio: false, opcoes: &[] }
}

const fn obrig(nome: &'static str, tipo: Tipo, descricao: &'static str) -> Param {
    Param { nome, tipo, descricao, obrigatorio: true, opcoes: &[] }
}

const STATUS: &[&str] = &["estante", "quero_ler", "lendo", "lido", "abandonado"];

const ID: Param = obrig("id", Tipo::Inteiro, "número do livro (#id), visto em `livros`");
const ANO: Param = p("ano", Tipo::Inteiro, "ano; o padrão é o atual");

const FERRAMENTAS: &[Ferramenta] = &[
    Ferramenta {
        nome: "livros",
        descricao: "Lista os livros da biblioteca com #id, status, progresso, nota, categorias e série. \
                    Todos os filtros são opcionais e se combinam.",
        params: &[
            Param { opcoes: STATUS, ..p("status", Tipo::Texto, "só os livros com este status") },
            p("categoria", Tipo::Texto, "só os livros com esta categoria"),
            p("serie", Tipo::Texto, "só os livros desta série, em ordem de volume"),
            p("texto", Tipo::Texto, "procura no título, autor, série ou com quem está emprestado"),
        ],
        escreve: false,
    },
    Ferramenta {
        nome: "livro",
        descricao: "Detalhes de um livro: datas, páginas, plano de leitura, ritmo, empréstimo, resenha e todas as notas e citações.",
        params: &[ID],
        escreve: false,
    },
    Ferramenta {
        nome: "buscar_notas",
        descricao: "Procura um texto nas notas e citações de todos os livros (sem diferenciar maiúsculas).",
        params: &[obrig("texto", Tipo::Texto, "o que procurar")],
        escreve: false,
    },
    Ferramenta {
        nome: "estatisticas",
        descricao: "Números do ano: livros lidos, meta, páginas, nota média, livros por mês e por categoria, sequência de dias lendo.",
        params: &[ANO],
        escreve: false,
    },
    Ferramenta {
        nome: "retrospectiva",
        descricao: "Destaques do ano: mais longo, mais rápido, autor e categoria favoritos, melhor mês, cinco estrelas, uma citação.",
        params: &[ANO],
        escreve: false,
    },
    Ferramenta {
        nome: "historico",
        descricao: "Páginas lidas em cada dia (só os dias com leitura) e a sequência atual de dias lendo.",
        params: &[p("dias", Tipo::Inteiro, "quantos dias para trás, contando hoje (padrão 30)")],
        escreve: false,
    },
    Ferramenta { nome: "categorias", descricao: "Categorias em uso e quantos livros cada uma tem.", params: &[], escreve: false },
    Ferramenta {
        nome: "emprestimos",
        descricao: "Livros emprestados agora: com quem e desde quando.",
        params: &[],
        escreve: false,
    },
    Ferramenta {
        nome: "adicionar_livro",
        descricao: "Cadastra um livro. Recusa um ISBN que já está na biblioteca.",
        params: &[
            obrig("titulo", Tipo::Texto, "título"),
            p("autor", Tipo::Texto, "autor"),
            p("isbn", Tipo::Texto, "ISBN-10 ou ISBN-13"),
            p("paginas", Tipo::Inteiro, "total de páginas"),
            Param { opcoes: STATUS, ..p("status", Tipo::Texto, "padrão: estante") },
            p("categorias", Tipo::Texto, "separadas por vírgula: romance, filosofia"),
            p("serie", Tipo::Texto, "série ou coleção"),
            p("volume", Tipo::Inteiro, "número do volume na série"),
        ],
        escreve: true,
    },
    Ferramenta {
        nome: "registrar_pagina",
        descricao: "Registra a página em que o usuário está (conta no histórico de hoje). Tirar da estante ou da fila passa o livro para lendo.",
        params: &[ID, obrig("pagina", Tipo::Inteiro, "página atual")],
        escreve: true,
    },
    Ferramenta {
        nome: "mudar_status",
        descricao: "Muda o status do livro; as datas de início e fim se ajustam sozinhas.",
        params: &[ID, Param { opcoes: STATUS, ..obrig("status", Tipo::Texto, "novo status") }],
        escreve: true,
    },
    Ferramenta {
        nome: "avaliar",
        descricao: "Dá uma nota de 0 a 5 estrelas (0 tira a nota).",
        params: &[ID, obrig("nota", Tipo::Inteiro, "0 a 5")],
        escreve: true,
    },
    Ferramenta {
        nome: "adicionar_nota",
        descricao: "Guarda uma nota ou citação no livro. Comece o texto com p.115 para marcar a página.",
        params: &[ID, obrig("texto", Tipo::Texto, "o texto"), p("citacao", Tipo::Logico, "true para citação")],
        escreve: true,
    },
    Ferramenta {
        nome: "definir_categorias",
        descricao: "Troca as categorias do livro (vazio tira todas).",
        params: &[ID, obrig("categorias", Tipo::Texto, "separadas por vírgula")],
        escreve: true,
    },
    Ferramenta {
        nome: "emprestar",
        descricao: "Registra que o livro foi emprestado para alguém hoje.",
        params: &[ID, obrig("pessoa", Tipo::Texto, "com quem ficou")],
        escreve: true,
    },
    Ferramenta { nome: "devolver", descricao: "Registra que o livro emprestado voltou hoje.", params: &[ID], escreve: true },
];

impl Param {
    fn esquema(&self) -> Value {
        let tipo = match self.tipo {
            Tipo::Texto => "string",
            Tipo::Inteiro => "integer",
            Tipo::Logico => "boolean",
        };
        let mut v = json!({ "type": tipo, "description": self.descricao });
        if !self.opcoes.is_empty() {
            v["enum"] = json!(self.opcoes);
        }
        v
    }
}

impl Ferramenta {
    fn esquema(&self) -> Value {
        let props: Map<String, Value> = self.params.iter().map(|p| (p.nome.to_string(), p.esquema())).collect();
        let obrigatorios: Vec<&str> = self.params.iter().filter(|p| p.obrigatorio).map(|p| p.nome).collect();
        json!({ "type": "object", "properties": props, "required": obrigatorios, "additionalProperties": false })
    }

    /// validar confere os argumentos contra o esquema antes de rodar.
    fn validar(&self, args: &Map<String, Value>) -> Result<()> {
        for nome in args.keys() {
            if !self.params.iter().any(|p| p.nome == nome) {
                bail!("argumento desconhecido: {nome}");
            }
        }
        for p in self.params {
            match args.get(p.nome) {
                None | Some(Value::Null) if p.obrigatorio => bail!("falta o argumento {}", p.nome),
                None | Some(Value::Null) => {}
                Some(v) => {
                    let ok = match p.tipo {
                        Tipo::Texto => v.is_string(),
                        Tipo::Inteiro => v.is_i64(),
                        Tipo::Logico => v.is_boolean(),
                    };
                    if !ok {
                        bail!("{} tem o tipo errado: {}", p.nome, p.esquema()["type"]);
                    }
                    if !p.opcoes.is_empty() && !p.opcoes.contains(&v.as_str().unwrap_or_default()) {
                        bail!("{} deve ser um de: {}", p.nome, p.opcoes.join(", "));
                    }
                }
            }
        }
        Ok(())
    }
}

/// Uma conversa com o cliente MCP.
pub struct Sessao {
    db: Db,
    escrita: bool,
    /// a cópia de segurança sai uma vez, antes da primeira gravação
    copia_feita: bool,
}

/// rodar trata `nasus mcp [--escrita]`: lê pedidos da entrada até ela fechar.
pub fn rodar(caminho_db: &Path, args: &[String]) -> Result<()> {
    let mut escrita = false;
    for a in args {
        match a.as_str() {
            "--escrita" => escrita = true,
            "-h" | "--help" => bail!("{USO}"),
            _ => bail!("opção desconhecida: {a}\n\n{USO}"),
        }
    }
    let db = Db::abrir(caminho_db).with_context(|| format!("abrindo {}", caminho_db.display()))?;
    let mut s = Sessao::new(db, escrita);
    let mut saida = std::io::stdout().lock();
    for linha in std::io::stdin().lock().lines() {
        let linha = linha?;
        if linha.trim().is_empty() {
            continue;
        }
        if let Some(resposta) = s.tratar(&linha) {
            writeln!(saida, "{resposta}")?;
            saida.flush()?;
        }
    }
    Ok(())
}

fn erro(id: Value, codigo: i64, mensagem: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": codigo, "message": mensagem } })
}

fn texto_arg<'a>(args: &'a Map<String, Value>, nome: &str) -> Option<&'a str> {
    args.get(nome).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn int_arg(args: &Map<String, Value>, nome: &str) -> Option<i64> {
    args.get(nome).and_then(Value::as_i64)
}

fn status_de(s: &str) -> Status {
    match s {
        "quero_ler" => Status::QueroLer,
        "lendo" => Status::Lendo,
        "lido" => Status::Lido,
        "abandonado" => Status::Abandonado,
        _ => Status::Estante,
    }
}

fn data(d: NaiveDate) -> String {
    d.format("%d/%m/%Y").to_string()
}

/// linha_livro resume um livro numa linha.
fn linha_livro(b: &Book) -> String {
    let mut t = format!("#{} {} {}", b.id, b.status.simbolo(), b.title);
    if !b.author.is_empty() {
        t += &format!(" — {}", b.author);
    }
    t += &format!(" · {}", b.status.nome());
    match (b.pages, b.current_page) {
        (0, 0) => {}
        (0, p) => t += &format!(" · pág. {p}"),
        (total, p) if p > 0 && b.status != Status::Lido => t += &format!(" · pág. {p}/{total}"),
        (total, _) => t += &format!(" · {total} págs"),
    }
    if b.rating > 0 {
        t += &format!(" · {}", b.estrelas());
    }
    if !b.categorias.is_empty() {
        t += &format!(" · [{}]", b.categorias.join(", "));
    }
    if !b.serie.is_empty() {
        t += &format!(" · série {}{}", b.serie, if b.volume > 0 { format!(" #{}", b.volume) } else { String::new() });
    }
    if let Some((pessoa, _)) = &b.emprestado {
        t += &format!(" · ↗ com {pessoa}");
    }
    t
}

impl Sessao {
    pub fn new(db: Db, escrita: bool) -> Self {
        Sessao { db, escrita, copia_feita: false }
    }

    fn disponiveis(&self) -> impl Iterator<Item = &'static Ferramenta> + '_ {
        FERRAMENTAS.iter().filter(|f| self.escrita || !f.escreve)
    }

    /// tratar responde a uma linha do cliente; notificações (sem id) não têm resposta.
    pub fn tratar(&mut self, linha: &str) -> Option<Value> {
        let msg: Value = match serde_json::from_str(linha) {
            Ok(v) => v,
            Err(_) => return Some(erro(Value::Null, -32700, "JSON inválido")),
        };
        let id = msg.get("id").cloned()?;
        let metodo = msg.get("method").and_then(Value::as_str).unwrap_or_default();
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let resultado = match metodo {
            "initialize" => {
                let versao = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(VERSAO_PROTOCOLO);
                let instrucoes = if self.escrita { format!("{INSTRUCOES}{INSTRUCOES_ESCRITA}") } else { INSTRUCOES.into() };
                json!({
                    "protocolVersion": versao,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "nasus", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": instrucoes,
                })
            }
            "ping" => json!({}),
            "tools/list" => {
                let lista: Vec<Value> = self
                    .disponiveis()
                    .map(|f| json!({ "name": f.nome, "description": f.descricao, "inputSchema": f.esquema() }))
                    .collect();
                json!({ "tools": lista })
            }
            "tools/call" => {
                let nome = params.get("name").and_then(Value::as_str).unwrap_or_default();
                let Some(f) = self.disponiveis().find(|f| f.nome == nome) else {
                    return Some(erro(id, -32602, &format!("ferramenta desconhecida: {nome}")));
                };
                let args = params.get("arguments").and_then(Value::as_object).cloned().unwrap_or_default();
                let r = f.validar(&args).and_then(|_| self.chamar(f, &args));
                let (texto, falhou) = match r {
                    Ok(t) => (t, false),
                    Err(e) => (format!("erro: {e:#}"), true),
                };
                let texto = if texto.len() > LIMITE_SAIDA {
                    let mut corte = LIMITE_SAIDA;
                    while !texto.is_char_boundary(corte) {
                        corte -= 1;
                    }
                    format!("{}\n\n[saída cortada: refine a consulta com os filtros]", &texto[..corte])
                } else {
                    texto
                };
                json!({ "content": [{ "type": "text", "text": texto }], "isError": falhou })
            }
            _ => return Some(erro(id, -32601, &format!("método desconhecido: {metodo}"))),
        };
        Some(json!({ "jsonrpc": "2.0", "id": id, "result": resultado }))
    }

    fn chamar(&mut self, f: &Ferramenta, a: &Map<String, Value>) -> Result<String> {
        let hoje = Local::now().date_naive();
        if f.escreve && !self.copia_feita {
            self.db.copia_avulsa("mcp-antes")?;
            self.copia_feita = true;
        }
        let ano = int_arg(a, "ano").map(|a| a as i32).unwrap_or(hoje.year());
        let id = int_arg(a, "id");
        let livro = |db: &Db| -> Result<Book> {
            let id = id.context("falta o id")?;
            db.livro(id).map_err(|_| anyhow!("não existe o livro #{id}"))
        };
        match f.nome {
            "livros" => self.livros(a),
            "livro" => self.livro(livro(&self.db)?, hoje),
            "buscar_notas" => self.buscar_notas(texto_arg(a, "texto").unwrap_or_default()),
            "estatisticas" => self.estatisticas(ano, hoje),
            "retrospectiva" => Ok(self.db.retrospectiva(ano)?.texto()),
            "historico" => {
                let dias = int_arg(a, "dias").unwrap_or(30).clamp(1, 3660) as u64;
                let s = self.db.sequencia(hoje)?;
                let lista = self.db.paginas_por_dia(hoje - Days::new(dias - 1), hoje)?;
                let total: i64 = lista.iter().map(|(_, n)| n).sum();
                let mut t = format!(
                    "Sequência: {} seguidos (recorde {}); {} hoje.\n{} páginas em {} dias com leitura nos últimos {dias} dias.",
                    plural(s.atual, "dia"),
                    plural(s.recorde, "dia"),
                    if s.leu_hoje { "já leu" } else { "ainda não leu" },
                    total,
                    lista.len()
                );
                for (d, n) in lista {
                    t += &format!("\n{}: {}", data(d), plural(n, "página"));
                }
                Ok(t)
            }
            "categorias" => {
                let lista = self.db.categorias()?;
                if lista.is_empty() {
                    return Ok("Nenhuma categoria ainda.".into());
                }
                Ok(lista.into_iter().map(|(c, n)| format!("{c}: {}", plural(n, "livro"))).collect::<Vec<_>>().join("\n"))
            }
            "emprestimos" => {
                let lista = self.db.emprestados()?;
                if lista.is_empty() {
                    return Ok("Nenhum livro emprestado.".into());
                }
                Ok(lista
                    .iter()
                    .filter_map(|b| {
                        let (pessoa, desde) = b.emprestado.as_ref()?;
                        Some(format!("#{} {} — com {pessoa} desde {}", b.id, b.title, data(*desde)))
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            "adicionar_livro" => {
                let isbn = texto_arg(a, "isbn").unwrap_or_default().to_string();
                if let Some(outro) = self.db.livro_por_isbn(&isbn)? {
                    bail!("esse ISBN já está na biblioteca: {}", linha_livro(&outro));
                }
                let serie = texto_arg(a, "serie").unwrap_or_default().to_string();
                let mut b = Book {
                    title: texto_arg(a, "titulo").context("o título não pode ser vazio")?.into(),
                    author: texto_arg(a, "autor").unwrap_or_default().into(),
                    isbn,
                    pages: int_arg(a, "paginas").unwrap_or(0).max(0),
                    categorias: ler_categorias(texto_arg(a, "categorias").unwrap_or_default()),
                    volume: if serie.is_empty() { 0 } else { int_arg(a, "volume").unwrap_or(0).max(0) },
                    serie,
                    ..Book::novo()
                };
                b.mudar_status(status_de(texto_arg(a, "status").unwrap_or("estante")), hoje);
                b.id = self.db.inserir_livro(&b)?;
                Ok(format!("Adicionado: {}", linha_livro(&b)))
            }
            "registrar_pagina" => {
                let mut b = livro(&self.db)?;
                let fim = b.registrar_pagina(int_arg(a, "pagina").unwrap_or(0), hoje);
                self.db.salvar_progresso(&b, hoje)?;
                let mut t = format!("Registrado: {}", linha_livro(&b));
                if fim {
                    t += "\nChegou na última página: pergunte se o usuário terminou (mudar_status lido).";
                }
                Ok(t)
            }
            "mudar_status" => {
                let mut b = livro(&self.db)?;
                b.mudar_status(status_de(texto_arg(a, "status").unwrap_or_default()), hoje);
                self.db.salvar_progresso(&b, hoje)?;
                let mut t = format!("Status mudado: {}", linha_livro(&b));
                if b.status == Status::Lido
                    && let Some(p) = self.db.proximo_da_serie(&b)?
                {
                    t += &format!("\nPróximo da série: {}", linha_livro(&p));
                }
                Ok(t)
            }
            "avaliar" => {
                let mut b = livro(&self.db)?;
                let nota = int_arg(a, "nota").unwrap_or(-1);
                if !(0..=5).contains(&nota) {
                    bail!("a nota vai de 0 a 5");
                }
                b.rating = nota as u8;
                self.db.salvar_livro(&b)?;
                Ok(format!("Avaliado: {}", linha_livro(&b)))
            }
            "adicionar_nota" => {
                let b = livro(&self.db)?;
                let (pagina, corpo) = separar_pagina(texto_arg(a, "texto").unwrap_or_default());
                if corpo.is_empty() {
                    bail!("a nota está vazia");
                }
                let citacao = a.get("citacao").and_then(Value::as_bool).unwrap_or(false);
                let n = Note {
                    id: 0,
                    book_id: b.id,
                    kind: if citacao { NoteKind::Citacao } else { NoteKind::Nota },
                    page: pagina.or((b.current_page > 0).then_some(b.current_page)),
                    body: corpo.to_string(),
                    created_at: hoje,
                };
                let nid = self.db.inserir_nota(&n)?;
                Ok(format!("{} [{nid}] guardada em #{} {}", if citacao { "Citação" } else { "Nota" }, b.id, b.title))
            }
            "definir_categorias" => {
                let mut b = livro(&self.db)?;
                b.categorias = ler_categorias(texto_arg(a, "categorias").unwrap_or_default());
                self.db.salvar_livro(&b)?;
                Ok(format!("Categorias: {}", linha_livro(&b)))
            }
            "emprestar" => {
                let b = livro(&self.db)?;
                self.db.emprestar(b.id, texto_arg(a, "pessoa").unwrap_or_default(), hoje)?;
                Ok(format!("Emprestado: {}", linha_livro(&self.db.livro(b.id)?)))
            }
            "devolver" => {
                let b = livro(&self.db)?;
                let pessoa = self.db.devolver(b.id, hoje)?;
                Ok(format!("{pessoa} devolveu #{} {}", b.id, b.title))
            }
            outro => bail!("ferramenta sem implementação: {outro}"),
        }
    }

    fn livros(&self, a: &Map<String, Value>) -> Result<String> {
        let status = texto_arg(a, "status").map(status_de);
        let categoria = texto_arg(a, "categoria").map(str::to_lowercase);
        let texto = texto_arg(a, "texto").map(str::to_lowercase);
        let lista = match texto_arg(a, "serie") {
            Some(s) => self.db.serie(s)?,
            None => self.db.livros()?,
        };
        let linhas: Vec<String> = lista
            .iter()
            .filter(|b| status.is_none_or(|s| b.status == s))
            .filter(|b| categoria.as_ref().is_none_or(|c| b.categorias.contains(c)))
            .filter(|b| {
                texto.as_ref().is_none_or(|t| {
                    [&b.title, &b.author, &b.serie].iter().any(|c| c.to_lowercase().contains(t))
                        || b.emprestado.as_ref().is_some_and(|(p, _)| p.to_lowercase().contains(t))
                })
            })
            .map(linha_livro)
            .collect();
        if linhas.is_empty() {
            return Ok("Nenhum livro com esses filtros.".into());
        }
        Ok(format!("{}:\n{}", plural(linhas.len() as i64, "livro"), linhas.join("\n")))
    }

    fn livro(&self, b: Book, hoje: NaiveDate) -> Result<String> {
        let mut t = linha_livro(&b);
        if !b.isbn.is_empty() {
            t += &format!("\nISBN {}", b.isbn);
        }
        if let Some(i) = b.started_at {
            t += &format!("\nPeríodo: {} → {}", data(i), b.finished_at.map_or("…".into(), data));
        }
        if let Some(p) = self.db.plano(b.id)? {
            t += &format!(
                "\nPlano: {} pág/dia de {} a {}; {}",
                p.paginas_dia,
                data(p.inicio),
                data(p.fim(b.pages)),
                p.situacao(&b, hoje).texto()
            );
        }
        if b.status == Status::Lendo
            && let Some(r) = self.db.ritmo(b.id, hoje)?
        {
            t += &format!("\nRitmo: {r:.0} pág/dia nas últimas duas semanas");
            if let Some(fim) = self.db.previsao(&b, hoje)? {
                t += &format!("; termina por volta de {}", data(fim));
            }
        }
        if let Some((pessoa, desde)) = &b.emprestado {
            t += &format!("\nEmprestado para {pessoa} desde {}", data(*desde));
        }
        if !b.review.is_empty() {
            t += &format!("\nResenha: {}", b.review);
        }
        let notas = self.db.notas(b.id)?;
        if !notas.is_empty() {
            let n_cit = notas.iter().filter(|n| n.kind == NoteKind::Citacao).count() as i64;
            t += &format!("\n\n{} e {}:", plural(notas.len() as i64 - n_cit, "nota"), citacoes(n_cit));
            for n in notas {
                t += &format!("\n{}", linha_nota(&n));
            }
        }
        Ok(t)
    }

    fn buscar_notas(&self, texto: &str) -> Result<String> {
        let busca = texto.to_lowercase();
        let mut achadas = Vec::new();
        for b in self.db.livros()? {
            for n in self.db.notas(b.id)? {
                if n.body.to_lowercase().contains(&busca) {
                    achadas.push(format!("#{} {} · {}", b.id, b.title, linha_nota(&n)));
                }
            }
        }
        if achadas.is_empty() {
            return Ok(format!("Nenhuma nota ou citação com \"{texto}\"."));
        }
        Ok(achadas.join("\n"))
    }

    fn estatisticas(&self, ano: i32, hoje: NaiveDate) -> Result<String> {
        let e = self.db.estatisticas(ano)?;
        let mut t = format!("{ano}: {} lidos, {} páginas", plural(e.lidos, "livro"), e.paginas);
        if let Some(m) = e.meta {
            t += &format!(" (meta: {m})");
        }
        if let Some(m) = e.nota_media {
            t += &format!("; nota média {m:.1}");
        }
        t += &format!("\nLendo agora: {}", e.lendo);
        let meses: Vec<String> = crate::db::MESES
            .iter()
            .zip(e.por_mes)
            .filter(|(_, n)| *n > 0)
            .map(|(m, n)| format!("{m} {n}"))
            .collect();
        if !meses.is_empty() {
            t += &format!("\nPor mês: {}", meses.join(", "));
        }
        if !e.por_categoria.is_empty() {
            let cats: Vec<String> = e.por_categoria.iter().map(|(c, n)| format!("{c} {n}")).collect();
            t += &format!("\nPor categoria: {}", cats.join(", "));
        }
        let s = self.db.sequencia(hoje)?;
        t += &format!("\nSequência de leitura: {} (recorde {})", plural(s.atual, "dia"), plural(s.recorde, "dia"));
        Ok(t)
    }
}

fn linha_nota(n: &Note) -> String {
    let pagina = n.page.map_or(String::new(), |p| format!("p. {p} "));
    match n.kind {
        NoteKind::Nota => format!("[{}] {pagina}nota: {}", n.id, n.body),
        NoteKind::Citacao => format!("[{}] {pagina}citação: “{}”", n.id, n.body),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sessao(escrita: bool) -> Sessao {
        let db = Db::em_memoria().unwrap();
        let mut b = Book { title: "Duna".into(), author: "Frank Herbert".into(), pages: 680, ..Book::novo() };
        b.categorias = vec!["ficção científica".into()];
        b.id = db.inserir_livro(&b).unwrap();
        let hoje = Local::now().date_naive();
        db.inserir_nota(&Note { id: 0, book_id: b.id, kind: NoteKind::Citacao, page: Some(10), body: "O medo mata a mente.".into(), created_at: hoje }).unwrap();
        Sessao::new(db, escrita)
    }

    fn pedir(s: &mut Sessao, metodo: &str, params: Value) -> Value {
        let linha = json!({ "jsonrpc": "2.0", "id": 1, "method": metodo, "params": params }).to_string();
        s.tratar(&linha).expect("tem resposta")
    }

    fn chamar(s: &mut Sessao, nome: &str, args: Value) -> (String, bool) {
        let r = pedir(s, "tools/call", json!({ "name": nome, "arguments": args }));
        let res = &r["result"];
        (res["content"][0]["text"].as_str().unwrap_or_default().to_string(), res["isError"].as_bool().unwrap_or(true))
    }

    #[test]
    fn inicializa_e_lista_ferramentas() {
        let mut s = sessao(false);
        let r = pedir(&mut s, "initialize", json!({ "protocolVersion": "2025-03-26" }));
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26", "ecoa a versão do cliente");
        assert_eq!(r["result"]["serverInfo"]["name"], "nasus");
        assert!(s.tratar(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());

        let nomes = |s: &mut Sessao| -> Vec<String> {
            pedir(s, "tools/list", json!({}))["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["name"].as_str().unwrap().to_string())
                .collect()
        };
        let leitura = nomes(&mut s);
        assert!(leitura.contains(&"livros".to_string()) && !leitura.contains(&"registrar_pagina".to_string()));
        let todas = nomes(&mut sessao(true));
        assert!(todas.contains(&"registrar_pagina".to_string()));
        let t = &pedir(&mut s, "tools/list", json!({}))["result"]["tools"][0];
        assert_eq!(t["inputSchema"]["additionalProperties"], false);
        assert_eq!(t["inputSchema"]["properties"]["status"]["enum"][1], "quero_ler");

        assert_eq!(s.tratar("{lixo").unwrap()["error"]["code"], -32700);
        assert_eq!(pedir(&mut s, "outro/metodo", json!({}))["error"]["code"], -32601);
        let r = pedir(&mut s, "tools/call", json!({ "name": "registrar_pagina", "arguments": { "id": 1, "pagina": 2 } }));
        assert_eq!(r["error"]["code"], -32602, "sem --escrita a ferramenta nem existe");
    }

    #[test]
    fn consultas() {
        let mut s = sessao(false);
        let (t, erro) = chamar(&mut s, "livros", json!({ "categoria": "ficção científica" }));
        assert!(!erro && t.contains("#1 □ Duna — Frank Herbert · Na estante"), "{t}");
        assert!(chamar(&mut s, "livros", json!({ "status": "lido" })).0.contains("Nenhum"));
        let (t, _) = chamar(&mut s, "livro", json!({ "id": 1 }));
        assert!(t.contains("[1] p. 10 citação: “O medo mata a mente.”"), "{t}");
        assert!(chamar(&mut s, "buscar_notas", json!({ "texto": "MEDO" })).0.contains("#1 Duna"));
        assert!(chamar(&mut s, "estatisticas", json!({})).0.contains("Lendo agora: 0"));
        assert!(chamar(&mut s, "retrospectiva", json!({ "ano": 2026 })).0.starts_with("🎉"));

        let (t, erro) = chamar(&mut s, "livro", json!({ "id": 99 }));
        assert!(erro && t.contains("não existe o livro #99"));
        let (t, erro) = chamar(&mut s, "livros", json!({ "outro": 1 }));
        assert!(erro && t.contains("argumento desconhecido"));
        let (t, erro) = chamar(&mut s, "livro", json!({ "id": "1" }));
        assert!(erro && t.contains("tipo errado"), "{t}");
        let (_, erro) = chamar(&mut s, "livros", json!({ "status": "relendo" }));
        assert!(erro);
    }

    #[test]
    fn escrita() {
        let mut s = sessao(true);
        let (t, erro) = chamar(&mut s, "registrar_pagina", json!({ "id": 1, "pagina": 120 }));
        assert!(!erro && t.contains("Lendo · pág. 120/680"), "{t}");
        assert!(chamar(&mut s, "historico", json!({ "dias": 7 })).0.contains("120 páginas"));
        let (t, _) = chamar(&mut s, "adicionar_nota", json!({ "id": 1, "texto": "p.115 Arrakis", "citacao": true }));
        assert!(t.starts_with("Citação"), "{t}");
        assert!(chamar(&mut s, "livro", json!({ "id": 1 })).0.contains("p. 115 citação: “Arrakis”"));
        assert!(chamar(&mut s, "emprestar", json!({ "id": 1, "pessoa": "Maria" })).0.contains("com Maria"));
        assert!(chamar(&mut s, "emprestimos", json!({})).0.contains("#1 Duna — com Maria"));
        assert!(chamar(&mut s, "devolver", json!({ "id": 1 })).0.contains("Maria devolveu"));
        let (t, erro) = chamar(&mut s, "adicionar_livro", json!({ "titulo": "Messias", "serie": "Duna", "volume": 2, "status": "quero_ler" }));
        assert!(!erro && t.contains("série Duna #2"), "{t}");
        assert!(chamar(&mut s, "avaliar", json!({ "id": 1, "nota": 9 })).1);
        assert!(chamar(&mut s, "mudar_status", json!({ "id": 1, "status": "lido" })).0.contains("Lido"));
    }
}
