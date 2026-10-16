//! Interface web: `nasus web` sobe um servidor em 127.0.0.1 e abre o
//! navegador. Uma página única embutida no binário (HTML, CSS e JS puros)
//! consome a API JSON abaixo, que reaproveita o mesmo modelo e banco da TUI.

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{Datelike, Local, NaiveDate};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::codigo_barras;
use crate::db::Db;
use crate::model::{Book, Note, NoteKind, Plano, Status, ler_categorias, separar_pagina};
use crate::openlibrary::{self, Resultado};

const PAGINA: &str = include_str!("pagina.html");

/// N-A-S-U num teclado de telefone
const PORTA: u16 = 6278;

/// a foto do código de barras é o maior corpo que a API recebe
const MAX_CORPO: u64 = 12 * 1024 * 1024;

pub const USO: &str = "uso: nasus web [--porta N] [--sem-abrir]

  --porta N     porta local (padrão 6278)
  --sem-abrir   só sobe o servidor, sem abrir o navegador

O servidor escuta só em 127.0.0.1: nada fica acessível pela rede. Ctrl+C encerra.";

pub type Busca = fn(&str) -> Result<Vec<Resultado>>;

#[derive(Debug, PartialEq)]
struct Opcoes {
    porta: u16,
    abrir: bool,
}

fn ler_opcoes(args: &[String]) -> Result<Opcoes> {
    let mut o = Opcoes { porta: PORTA, abrir: true };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--porta" => {
                let p = it.next().context("--porta precisa de um número")?;
                o.porta = p.parse().ok().filter(|p| *p > 0).with_context(|| format!("porta inválida: {p}"))?;
            }
            "--sem-abrir" => o.abrir = false,
            "-h" | "--help" => bail!("{USO}"),
            _ => bail!("opção desconhecida: {a}\n\n{USO}"),
        }
    }
    Ok(o)
}

/// rodar trata `nasus web ...`: atende uma requisição por vez até o Ctrl+C.
pub fn rodar(caminho_db: &Path, args: &[String]) -> Result<()> {
    let op = ler_opcoes(args)?;
    let db = Db::abrir(caminho_db).with_context(|| format!("abrindo {}", caminho_db.display()))?;
    let mut api = Api::new(db, openlibrary::buscar);
    let servidor = tiny_http::Server::http(("127.0.0.1", op.porta))
        .map_err(|e| anyhow!("não deu para escutar em 127.0.0.1:{}: {e}", op.porta))?;
    let url = format!("http://127.0.0.1:{}", op.porta);
    println!("Nasus web em {url} — Ctrl+C encerra");
    if op.abrir {
        abrir_navegador(&url);
    }
    for mut req in servidor.incoming_requests() {
        let (status, tipo, corpo) = atender(&mut api, &mut req);
        let resp = tiny_http::Response::from_data(corpo)
            .with_status_code(status)
            .with_header(cabecalho("Content-Type", tipo))
            .with_header(cabecalho("Cache-Control", "no-store"))
            .with_header(cabecalho("X-Content-Type-Options", "nosniff"))
            .with_header(cabecalho("X-Frame-Options", "DENY"))
            .with_header(cabecalho(
                "Content-Security-Policy",
                "default-src 'self'; img-src 'self' data: https://covers.openlibrary.org https://*.archive.org; \
                 style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline'",
            ));
        if let Err(e) = req.respond(resp) {
            eprintln!("aviso: respondendo: {e}");
        }
    }
    Ok(())
}

fn cabecalho(nome: &str, valor: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(nome, valor).expect("cabeçalho ASCII")
}

fn atender(api: &mut Api, req: &mut tiny_http::Request) -> (u16, &'static str, Vec<u8>) {
    const JSON: &str = "application/json; charset=utf-8";
    let header = |nome: &str| {
        req.headers()
            .iter()
            .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(nome))
            .map(|h| h.value.as_str().to_string())
    };
    let metodo = req.method().as_str().to_string();
    if !protegido(header("Host").as_deref(), header("Origin").as_deref(), &metodo) {
        return (403, "text/plain; charset=utf-8", "acesso negado".into());
    }
    let url = req.url().to_string();
    let (caminho, query) = url.split_once('?').unwrap_or((&url, ""));
    if caminho == "/" && metodo == "GET" {
        return (200, "text/html; charset=utf-8", PAGINA.into());
    }
    if !caminho.starts_with("/api/") {
        return (404, "text/plain; charset=utf-8", "não encontrado".into());
    }
    let mut corpo = Vec::new();
    if req.as_reader().take(MAX_CORPO + 1).read_to_end(&mut corpo).is_err() || corpo.len() as u64 > MAX_CORPO {
        return (413, JSON, json!({ "erro": "corpo grande demais" }).to_string().into());
    }
    let hoje = Local::now().date_naive();
    let (status, v) = api.tratar(&metodo, caminho, query, &corpo, hoje);
    (status, JSON, v.to_string().into())
}

fn abrir_navegador(url: &str) {
    let r = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else if cfg!(windows) {
        std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if r.is_err() {
        println!("Abra {url} no navegador.");
    }
}

/// host_local diz se o nome é o próprio computador.
fn host_local(h: &str) -> bool {
    matches!(h, "127.0.0.1" | "localhost" | "::1")
}

/// tirar_porta separa o host de "host:porta", "[::1]:porta" ou "host".
fn tirar_porta(s: &str) -> &str {
    if let Some(resto) = s.strip_prefix('[') {
        return resto.split(']').next().unwrap_or_default();
    }
    match s.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => h,
        _ => s,
    }
}

/// protegido barra o que não veio do navegador local, mesmo escutando só em
/// 127.0.0.1 (como no `prisma --web`):
///   - Host de fora: DNS rebinding (um domínio que resolve para 127.0.0.1);
///   - Origin de outro site em métodos que não são GET: CSRF.
fn protegido(host: Option<&str>, origin: Option<&str>, metodo: &str) -> bool {
    let Some(host) = host else { return false };
    if !host_local(tirar_porta(host)) {
        return false;
    }
    match origin {
        Some(o) if metodo != "GET" => {
            let sem_esquema = o.strip_prefix("http://").or_else(|| o.strip_prefix("https://"));
            sem_esquema.is_some_and(|s| host_local(tirar_porta(s.split('/').next().unwrap_or_default())))
        }
        _ => true,
    }
}

/// parametro lê um valor da query string, decodificando %XX e +.
fn parametro(query: &str, nome: &str) -> Option<String> {
    query.split('&').filter_map(|p| p.split_once('=')).find(|(k, _)| *k == nome).map(|(_, v)| decodificar(v))
}

fn decodificar(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn data(d: Option<NaiveDate>) -> Value {
    d.map_or(Value::Null, |d| d.format("%d/%m/%Y").to_string().into())
}

fn ler_data(s: &str) -> Result<NaiveDate> {
    let s = s.trim();
    NaiveDate::parse_from_str(s, "%d/%m/%Y")
        .or_else(|_| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
        .ok()
        .context("data no formato dd/mm/aaaa")
}

fn status_json(s: Status) -> Value {
    json!({ "chave": s.chave(), "nome": s.nome(), "rotulo": s.rotulo(), "simbolo": s.simbolo() })
}

fn livro_json(b: &Book) -> Value {
    json!({
        "id": b.id,
        "titulo": b.title,
        "autor": b.author,
        "isbn": b.isbn,
        "paginas": b.pages,
        "pagina_atual": b.current_page,
        "progresso": b.progresso(),
        "status": b.status.chave(),
        "nota": b.rating,
        "resenha": b.review,
        "inicio": data(b.started_at),
        "fim": data(b.finished_at),
        "categorias": b.categorias,
    })
}

fn nota_json(n: &Note) -> Value {
    json!({
        "id": n.id,
        "tipo": if n.kind == NoteKind::Citacao { "citacao" } else { "nota" },
        "pagina": n.page,
        "texto": n.body,
        "data": data(Some(n.created_at)),
    })
}

fn plano_json(p: &Plano, b: &Book, hoje: NaiveDate) -> Value {
    json!({
        "inicio": data(Some(p.inicio)),
        "paginas_dia": p.paginas_dia,
        "pagina_inicial": p.pagina_inicial,
        "fim": data(Some(p.fim(b.pages))),
        "dias": p.dias(b.pages),
        "meta_hoje": p.meta_ate(hoje, b.pages),
        "situacao": p.situacao(b, hoje).texto(),
    })
}

fn resultado_json(r: &Resultado, ja_tenho: Option<i64>) -> Value {
    json!({
        "titulo": r.title,
        "autor": r.author,
        "isbn": r.isbn,
        "paginas": r.pages,
        "ano": r.ano,
        "categorias": r.categorias,
        "ja_tenho": ja_tenho,
    })
}

#[derive(Deserialize)]
struct Id {
    id: i64,
}

#[derive(Deserialize)]
struct LivroForm {
    id: Option<i64>,
    titulo: String,
    #[serde(default)]
    autor: String,
    #[serde(default)]
    isbn: String,
    #[serde(default)]
    paginas: i64,
    /// texto separado por vírgulas, como no formulário da TUI
    #[serde(default)]
    categorias: String,
    status: Option<String>,
    #[serde(default)]
    resenha: String,
}

#[derive(Deserialize)]
struct MudarStatus {
    id: i64,
    status: String,
}

#[derive(Deserialize)]
struct Pagina {
    id: i64,
    pagina: i64,
}

#[derive(Deserialize)]
struct Avaliar {
    id: i64,
    nota: u8,
}

#[derive(Deserialize)]
struct NovaNota {
    livro: i64,
    tipo: String,
    texto: String,
}

#[derive(Deserialize)]
struct PlanoForm {
    livro: i64,
    inicio: String,
    paginas_dia: i64,
}

#[derive(Deserialize)]
struct Meta {
    ano: i32,
    livros: i64,
}

/// Erro da API: o status HTTP e a mensagem que a página mostra.
struct Falha(u16, String);

impl From<anyhow::Error> for Falha {
    fn from(e: anyhow::Error) -> Self {
        Falha(400, format!("{e:#}"))
    }
}

type Saida = std::result::Result<Value, Falha>;

fn ler<T: DeserializeOwned>(corpo: &[u8]) -> std::result::Result<T, Falha> {
    serde_json::from_slice(corpo).map_err(|e| Falha(400, format!("JSON inválido: {e}")))
}

pub struct Api {
    db: Db,
    buscar: Busca,
}

impl Api {
    pub fn new(db: Db, buscar: Busca) -> Self {
        Api { db, buscar }
    }

    /// tratar atende uma chamada da API e devolve o status e o JSON. Não
    /// conhece HTTP nem rede, então os testes chamam direto.
    pub fn tratar(&mut self, metodo: &str, caminho: &str, query: &str, corpo: &[u8], hoje: NaiveDate) -> (u16, Value) {
        match self.rota(metodo, caminho, query, corpo, hoje) {
            Ok(v) => (200, v),
            Err(Falha(status, msg)) => (status, json!({ "erro": msg })),
        }
    }

    fn rota(&mut self, metodo: &str, caminho: &str, query: &str, corpo: &[u8], hoje: NaiveDate) -> Saida {
        let id_da_query = || -> std::result::Result<i64, Falha> {
            parametro(query, "id").and_then(|v| v.parse().ok()).ok_or(Falha(400, "falta o id".into()))
        };
        match (metodo, caminho) {
            ("GET", "/api/livros") => self.livros(),
            ("GET", "/api/livro") => self.livro(id_da_query()?, hoje),
            ("POST", "/api/livro") => self.salvar(ler(corpo)?, hoje),
            ("POST", "/api/livro/status") => {
                let m: MudarStatus = ler(corpo)?;
                let novo = Status::de_chave(&m.status).ok_or(Falha(400, "status desconhecido".into()))?;
                let mut b = self.livro_existente(m.id)?;
                b.mudar_status(novo, hoje);
                self.db.salvar_livro(&b)?;
                Ok(json!({ "livro": livro_json(&b) }))
            }
            ("POST", "/api/livro/pagina") => {
                let p: Pagina = ler(corpo)?;
                let mut b = self.livro_existente(p.id)?;
                let chegou_ao_fim = b.registrar_pagina(p.pagina, hoje);
                self.db.salvar_livro(&b)?;
                Ok(json!({ "livro": livro_json(&b), "chegou_ao_fim": chegou_ao_fim }))
            }
            ("POST", "/api/livro/avaliar") => {
                let a: Avaliar = ler(corpo)?;
                if a.nota > 5 {
                    return Err(Falha(400, "a nota vai de 0 a 5".into()));
                }
                let mut b = self.livro_existente(a.id)?;
                b.rating = a.nota;
                self.db.salvar_livro(&b)?;
                Ok(json!({ "livro": livro_json(&b) }))
            }
            ("POST", "/api/livro/remover") => {
                let Id { id } = ler(corpo)?;
                self.livro_existente(id)?;
                self.db.remover_livro(id)?;
                Ok(json!({ "ok": true }))
            }
            ("POST", "/api/nota") => {
                let n: NovaNota = ler(corpo)?;
                let b = self.livro_existente(n.livro)?;
                let (pagina, texto) = separar_pagina(&n.texto);
                if texto.is_empty() {
                    return Err(Falha(400, "escreva o texto da nota".into()));
                }
                let kind = if n.tipo == "citacao" { NoteKind::Citacao } else { NoteKind::Nota };
                let page = pagina.or((b.current_page > 0).then_some(b.current_page));
                let nota = Note { id: 0, book_id: b.id, kind, page, body: texto.into(), created_at: hoje };
                let id = self.db.inserir_nota(&nota)?;
                Ok(json!({ "nota": nota_json(&Note { id, ..nota }) }))
            }
            ("POST", "/api/nota/remover") => {
                let Id { id } = ler(corpo)?;
                self.db.remover_nota(id)?;
                Ok(json!({ "ok": true }))
            }
            ("POST", "/api/plano") => {
                let f: PlanoForm = ler(corpo)?;
                let b = self.livro_existente(f.livro)?;
                let inicio = ler_data(&f.inicio)?;
                // editar um plano mantém a página de onde ele partiu, como na TUI
                let pagina_inicial = self.db.plano(b.id)?.map_or(b.current_page, |p| p.pagina_inicial);
                self.db.aplicar_plano(&b, inicio, pagina_inicial, f.paginas_dia)?;
                self.livro(b.id, hoje)
            }
            ("POST", "/api/plano/recalcular") => {
                let Id { id } = ler(corpo)?;
                let b = self.livro_existente(id)?;
                let mut p = self.db.plano(id)?.ok_or(Falha(400, "este livro não tem plano".into()))?;
                p.inicio = hoje;
                p.pagina_inicial = b.current_page;
                self.db.salvar_plano(&p)?;
                self.livro(id, hoje)
            }
            ("GET", "/api/categorias") => {
                let lista: Vec<Value> =
                    self.db.categorias()?.into_iter().map(|(nome, n)| json!({ "nome": nome, "livros": n })).collect();
                Ok(json!({ "categorias": lista }))
            }
            ("GET", "/api/estatisticas") => {
                let ano = parametro(query, "ano").and_then(|a| a.parse().ok()).unwrap_or(hoje.year());
                let e = self.db.estatisticas(ano)?;
                let cats: Vec<Value> = e.por_categoria.iter().map(|(c, n)| json!({ "nome": c, "livros": n })).collect();
                Ok(json!({
                    "ano": e.ano,
                    "meta": e.meta,
                    "lidos": e.lidos,
                    "por_mes": e.por_mes,
                    "paginas": e.paginas,
                    "nota_media": e.nota_media,
                    "lendo": e.lendo,
                    "por_categoria": cats,
                }))
            }
            ("POST", "/api/meta") => {
                let m: Meta = ler(corpo)?;
                self.db.definir_meta(m.ano, m.livros.max(0))?;
                Ok(json!({ "ok": true }))
            }
            ("GET", "/api/buscar") => {
                let q = parametro(query, "q").unwrap_or_default();
                if q.trim().is_empty() {
                    return Err(Falha(400, "digite um título ou ISBN".into()));
                }
                self.resultados(&q)
            }
            ("POST", "/api/foto") => {
                let Some(isbn) = codigo_barras::ler_isbn(corpo)? else {
                    return Ok(json!({ "isbn": null, "resultados": [] }));
                };
                let mut v = self.resultados(&isbn)?;
                v["isbn"] = isbn.into();
                Ok(v)
            }
            (_, c) if c.starts_with("/api/") => Err(Falha(404, format!("rota desconhecida: {metodo} {c}"))),
            _ => Err(Falha(404, "não encontrado".into())),
        }
    }

    fn livro_existente(&self, id: i64) -> std::result::Result<Book, Falha> {
        self.db.livro(id).map_err(|_| Falha(404, format!("livro {id} não encontrado")))
    }

    fn livros(&self) -> Saida {
        let livros: Vec<Value> = self.db.livros()?.iter().map(livro_json).collect();
        let status: Vec<Value> = Status::TODOS.into_iter().map(status_json).collect();
        Ok(json!({ "livros": livros, "status": status }))
    }

    fn livro(&self, id: i64, hoje: NaiveDate) -> Saida {
        let b = self.livro_existente(id)?;
        let notas: Vec<Value> = self.db.notas(id)?.iter().map(nota_json).collect();
        let plano = self.db.plano(id)?.map(|p| plano_json(&p, &b, hoje));
        Ok(json!({ "livro": livro_json(&b), "notas": notas, "plano": plano }))
    }

    fn salvar(&mut self, f: LivroForm, hoje: NaiveDate) -> Saida {
        let titulo = f.titulo.trim();
        if titulo.is_empty() {
            return Err(Falha(400, "o título é obrigatório".into()));
        }
        if f.paginas < 0 {
            return Err(Falha(400, "páginas deve ser um número".into()));
        }
        let mut b = match f.id {
            Some(id) => self.livro_existente(id)?,
            None => Book::novo(),
        };
        b.title = titulo.into();
        b.author = f.autor.trim().into();
        b.isbn = f.isbn.trim().into();
        b.pages = f.paginas;
        b.categorias = ler_categorias(&f.categorias);
        b.review = f.resenha.trim().into();
        if b.pages > 0 {
            b.current_page = b.current_page.min(b.pages);
        }
        if let Some(outro) = self.db.livro_por_isbn(&b.isbn)?
            && Some(outro.id) != f.id
        {
            return Err(Falha(409, format!("esse ISBN já é de #{} {}", outro.id, outro.title)));
        }
        let status = match f.status.as_deref() {
            Some(s) => Status::de_chave(s).ok_or(Falha(400, "status desconhecido".into()))?,
            None => b.status,
        };
        if f.id.is_none() || b.status != status {
            b.mudar_status(status, hoje);
        }
        let id = match f.id {
            Some(id) => {
                self.db.salvar_livro(&b)?;
                id
            }
            None => self.db.inserir_livro(&b)?,
        };
        self.livro(id, hoje)
    }

    fn resultados(&self, q: &str) -> Saida {
        let mut v = Vec::new();
        for r in (self.buscar)(q).context("falha na busca")?.iter().take(10) {
            let ja = self.db.livro_por_isbn(&r.isbn)?.map(|b| b.id);
            v.push(resultado_json(r, ja));
        }
        Ok(json!({ "resultados": v }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dia(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
    }

    fn busca_falsa(q: &str) -> Result<Vec<Resultado>> {
        Ok(vec![Resultado {
            title: format!("{q} (resultado)"),
            author: "Autor".into(),
            isbn: "9788535914849".into(),
            pages: 300,
            ano: Some(2000),
            categorias: vec!["ficção".into()],
        }])
    }

    fn api() -> Api {
        Api::new(Db::em_memoria().unwrap(), busca_falsa)
    }

    fn post(a: &mut Api, caminho: &str, corpo: Value) -> (u16, Value) {
        a.tratar("POST", caminho, "", corpo.to_string().as_bytes(), dia(14))
    }

    fn get(a: &mut Api, caminho: &str, query: &str) -> (u16, Value) {
        a.tratar("GET", caminho, query, b"", dia(14))
    }

    #[test]
    fn so_aceita_o_navegador_local() {
        assert!(protegido(Some("127.0.0.1:6278"), None, "GET"));
        assert!(protegido(Some("localhost:6278"), Some("http://localhost:6278"), "POST"));
        assert!(protegido(Some("[::1]:6278"), Some("http://[::1]:6278"), "POST"));
        assert!(!protegido(Some("evil.com:6278"), None, "GET"), "DNS rebinding");
        assert!(!protegido(None, None, "GET"));
        assert!(!protegido(Some("127.0.0.1:6278"), Some("https://evil.com"), "POST"), "CSRF");
        assert!(protegido(Some("127.0.0.1:6278"), Some("https://evil.com"), "GET"), "GET não altera nada");
        assert!(!protegido(Some("127.0.0.1.evil.com"), None, "GET"));
    }

    #[test]
    fn le_parametros_e_opcoes() {
        assert_eq!(parametro("q=dom+casmurro&x=1", "q").as_deref(), Some("dom casmurro"));
        assert_eq!(parametro("q=fic%C3%A7%C3%A3o%20cient%C3%ADfica", "q").as_deref(), Some("ficção científica"));
        assert_eq!(parametro("q=100%", "q").as_deref(), Some("100%"));
        assert_eq!(parametro("a=1", "q"), None);
        assert_eq!(ler_opcoes(&[]).unwrap(), Opcoes { porta: 6278, abrir: true });
        let args: Vec<String> = ["--porta", "8080", "--sem-abrir"].iter().map(|s| s.to_string()).collect();
        assert_eq!(ler_opcoes(&args).unwrap(), Opcoes { porta: 8080, abrir: false });
        assert!(ler_opcoes(&["--porta".to_string(), "x".to_string()]).is_err());
    }

    #[test]
    fn fluxo_completo_pela_api() {
        let mut a = api();
        let (s, v) = post(
            &mut a,
            "/api/livro",
            json!({ "titulo": "Duna", "autor": "Frank Herbert", "isbn": "978-85-359-1484-9", "paginas": 680,
                    "categorias": "Ficção científica, clássico" }),
        );
        assert_eq!(s, 200, "{v}");
        assert_eq!(v["livro"]["status"], "shelf", "livro novo vai para a estante");
        assert_eq!(v["livro"]["categorias"], json!(["clássico", "ficção científica"]));
        let id = v["livro"]["id"].as_i64().unwrap();

        let (s, v) = post(&mut a, "/api/livro", json!({ "titulo": "Outro", "isbn": "9788535914849" }));
        assert_eq!(s, 409, "ISBN repetido: {v}");

        let (_, v) = get(&mut a, "/api/livros", "");
        assert_eq!(v["livros"].as_array().unwrap().len(), 1);
        assert_eq!(v["status"][0]["chave"], "shelf");

        let (_, v) = post(&mut a, "/api/livro/pagina", json!({ "id": id, "pagina": 50 }));
        assert_eq!((v["livro"]["status"].as_str(), v["chegou_ao_fim"].as_bool()), (Some("reading"), Some(false)));

        let (_, v) = post(&mut a, "/api/nota", json!({ "livro": id, "tipo": "citacao", "texto": "p.10 O medo mata a mente." }));
        assert_eq!((v["nota"]["pagina"].as_i64(), v["nota"]["texto"].as_str()), (Some(10), Some("O medo mata a mente.")));
        let nota = v["nota"]["id"].as_i64().unwrap();

        let (s, v) = post(&mut a, "/api/plano", json!({ "livro": id, "inicio": "14/10/2026", "paginas_dia": 30 }));
        assert_eq!(s, 200, "{v}");
        assert_eq!(v["plano"]["pagina_inicial"], 50);
        assert_eq!(v["plano"]["meta_hoje"], 80);

        let (_, v) = get(&mut a, "/api/livro", &format!("id={id}"));
        assert_eq!(v["notas"].as_array().unwrap().len(), 1);
        assert!(v["plano"]["situacao"].as_str().unwrap().contains("faltam 30"), "{v}");

        assert_eq!(post(&mut a, "/api/livro/avaliar", json!({ "id": id, "nota": 5 })).1["livro"]["nota"], 5);
        assert_eq!(post(&mut a, "/api/livro/avaliar", json!({ "id": id, "nota": 9 })).0, 400);
        let (_, v) = post(&mut a, "/api/livro/status", json!({ "id": id, "status": "read" }));
        assert_eq!(v["livro"]["pagina_atual"], 680);

        let (_, v) = get(&mut a, "/api/estatisticas", "ano=2026");
        assert_eq!(v["lidos"], 1);
        assert_eq!(v["por_categoria"][0]["livros"], 1);
        assert_eq!(post(&mut a, "/api/meta", json!({ "ano": 2026, "livros": 12 })).0, 200);
        assert_eq!(get(&mut a, "/api/estatisticas", "").1["meta"], 12);
        assert_eq!(get(&mut a, "/api/categorias", "").1["categorias"].as_array().unwrap().len(), 2);

        let (_, v) = get(&mut a, "/api/buscar", "q=duna");
        assert_eq!(v["resultados"][0]["ja_tenho"], id, "a busca avisa o que já está na biblioteca");

        assert_eq!(post(&mut a, "/api/nota/remover", json!({ "id": nota })).0, 200);
        assert_eq!(post(&mut a, "/api/livro/remover", json!({ "id": id })).0, 200);
        assert_eq!(get(&mut a, "/api/livro", &format!("id={id}")).0, 404);
    }

    #[test]
    fn foto_e_erros() {
        let mut a = api();
        let png = codigo_barras::png_de_teste("9788535914849");
        let (s, v) = a.tratar("POST", "/api/foto", "", &png, dia(1));
        assert_eq!(s, 200, "{v}");
        assert_eq!(v["isbn"], "9788535914849");
        assert_eq!(v["resultados"][0]["titulo"], "9788535914849 (resultado)");
        assert_eq!(a.tratar("POST", "/api/foto", "", b"lixo", dia(1)).0, 400);

        assert_eq!(get(&mut a, "/api/nada", "").0, 404);
        assert_eq!(a.tratar("POST", "/api/livro", "", b"{", dia(1)).0, 400);
        assert_eq!(post(&mut a, "/api/livro", json!({ "titulo": " " })).0, 400);
        assert_eq!(get(&mut a, "/api/livro", "").0, 400);
        assert_eq!(get(&mut a, "/api/buscar", "q=").0, 400);
    }
}
