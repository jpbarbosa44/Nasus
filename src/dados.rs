//! Exportar e importar a biblioteca, e restaurar uma cópia de segurança.
//!
//! O JSON leva tudo (livros, notas, planos, histórico e metas) e volta
//! inteiro pelo `nasus importar`; o CSV é para abrir em planilha. A
//! importação também entende o CSV do Goodreads ("Exportar biblioteca").

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::db::{self, Db};
use crate::model::{Book, Note, NoteKind, Plano, Status, ler_categorias};

pub const USO: &str = "uso: nasus exportar [--saida ARQUIVO]
     nasus importar ARQUIVO [--sim]
     nasus restaurar [--arquivo COPIA]

  exportar    grava a biblioteca em JSON (completo: notas, planos, histórico)
              ou em CSV (para planilhas), conforme a extensão; o padrão é
              nasus-AAAA-MM-DD.json na pasta atual
  importar    lê um JSON do Nasus ou o CSV do Goodreads (Meus livros ›
              Importar e exportar › Exportar biblioteca). Livros que já existem
              (mesmo ISBN, ou mesmo título e autor) são pulados. --sim não pergunta.
  restaurar   volta o banco para uma cópia de segurança. Sem --arquivo, lista
              as cópias de backups/ (uma por dia, as 7 últimas) para escolher.";

/// Um livro como vai para o JSON, com tudo o que pertence a ele.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct LivroJson {
    titulo: String,
    autor: String,
    isbn: String,
    paginas: i64,
    status: String,
    pagina_atual: i64,
    nota: u8,
    resenha: String,
    inicio: Option<NaiveDate>,
    fim: Option<NaiveDate>,
    categorias: Vec<String>,
    notas: Vec<NotaJson>,
    plano: Option<PlanoJson>,
    /// (dia, de, até)
    historico: Vec<(NaiveDate, i64, i64)>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct NotaJson {
    tipo: String,
    pagina: Option<i64>,
    texto: String,
    data: Option<NaiveDate>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct PlanoJson {
    inicio: NaiveDate,
    pagina_inicial: i64,
    paginas_dia: i64,
}

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Arquivo {
    formato: String,
    versao: u32,
    exportado_em: String,
    livros: Vec<LivroJson>,
    /// (ano, livros)
    metas: Vec<(i32, i64)>,
}

const FORMATO: &str = "nasus";

/// exportar_json monta o arquivo completo da biblioteca.
pub fn exportar_json(db: &Db) -> Result<String> {
    let mut a = Arquivo {
        formato: FORMATO.into(),
        versao: 1,
        exportado_em: Local::now().format("%Y-%m-%d %H:%M").to_string(),
        metas: db.metas()?,
        ..Default::default()
    };
    for b in db.livros()? {
        let notas = db
            .notas(b.id)?
            .into_iter()
            .map(|n| NotaJson {
                tipo: if n.kind == NoteKind::Citacao { "citacao" } else { "nota" }.into(),
                pagina: n.page,
                texto: n.body,
                data: Some(n.created_at),
            })
            .collect();
        let plano = db.plano(b.id)?.map(|p| PlanoJson {
            inicio: p.inicio,
            pagina_inicial: p.pagina_inicial,
            paginas_dia: p.paginas_dia,
        });
        let historico = db.leituras(b.id)?;
        a.livros.push(LivroJson {
            titulo: b.title,
            autor: b.author,
            isbn: b.isbn,
            paginas: b.pages,
            status: b.status.chave().into(),
            pagina_atual: b.current_page,
            nota: b.rating,
            resenha: b.review,
            inicio: b.started_at,
            fim: b.finished_at,
            categorias: b.categorias,
            notas,
            plano,
            historico,
        });
    }
    Ok(serde_json::to_string_pretty(&a)? + "\n")
}

/// exportar_csv faz uma linha por livro, separada por ";" (abre direto no
/// Excel e no LibreOffice em português).
pub fn exportar_csv(db: &Db) -> Result<String> {
    let mut w = csv::WriterBuilder::new().delimiter(b';').from_writer(Vec::new());
    w.write_record([
        "titulo", "autor", "isbn", "paginas", "status", "pagina_atual", "nota", "inicio", "fim", "categorias", "resenha",
    ])?;
    let data = |d: Option<NaiveDate>| d.map(|d| d.format("%d/%m/%Y").to_string()).unwrap_or_default();
    for b in db.livros()? {
        w.write_record([
            b.title.clone(),
            b.author.clone(),
            b.isbn.clone(),
            b.pages.to_string(),
            b.status.nome().to_string(),
            b.current_page.to_string(),
            b.rating.to_string(),
            data(b.started_at),
            data(b.finished_at),
            b.categorias.join(", "),
            b.review.clone(),
        ])?;
    }
    Ok(String::from_utf8(w.into_inner()?)?)
}

/// Um livro lido do arquivo, pronto para gravar.
#[derive(Debug)]
struct Importado {
    livro: Book,
    notas: Vec<Note>,
    plano: Option<Plano>,
    historico: Vec<(NaiveDate, i64, i64)>,
}

/// O que a importação vai fazer, para mostrar antes de gravar.
#[derive(Debug, Default)]
pub struct Importacao {
    novos: Vec<Importado>,
    /// títulos já presentes na biblioteca (ou repetidos no próprio arquivo)
    pub repetidos: Vec<String>,
    metas: Vec<(i32, i64)>,
    pub formato: &'static str,
}

impl Importacao {
    pub fn novos(&self) -> usize {
        self.novos.len()
    }
}

/// ler_importacao interpreta o arquivo e separa os livros novos dos que já existem.
pub fn ler_importacao(db: &Db, texto: &str) -> Result<Importacao> {
    let texto = texto.trim_start_matches('\u{feff}');
    let (formato, itens, metas) = if texto.trim_start().starts_with('{') {
        let (itens, metas) = ler_json(texto)?;
        ("Nasus (JSON)", itens, metas)
    } else if texto.lines().next().is_some_and(|l| l.contains("Exclusive Shelf")) {
        ("Goodreads (CSV)", ler_goodreads(texto)?, Vec::new())
    } else {
        bail!("formato não reconhecido: use um JSON exportado pelo Nasus ou o CSV do Goodreads");
    };
    let mut existentes: Vec<(String, String)> =
        db.livros()?.iter().map(|b| (b.title.to_lowercase(), b.author.to_lowercase())).collect();
    let mut isbns: Vec<String> = Vec::new();
    let mut imp = Importacao { metas, formato, ..Default::default() };
    for item in itens {
        let b = &item.livro;
        let chave = (b.title.to_lowercase(), b.author.to_lowercase());
        let isbn = b.isbn.replace(['-', ' '], "").to_uppercase();
        let repetido = existentes.contains(&chave)
            || (!isbn.is_empty() && (isbns.contains(&isbn) || db.livro_por_isbn(&isbn)?.is_some()));
        if repetido {
            imp.repetidos.push(b.title.clone());
            continue;
        }
        existentes.push(chave);
        if !isbn.is_empty() {
            isbns.push(isbn);
        }
        imp.novos.push(item);
    }
    Ok(imp)
}

/// gravar_importacao grava os livros novos e as metas que ainda não existem.
pub fn gravar_importacao(db: &Db, imp: Importacao) -> Result<usize> {
    let n = imp.novos.len();
    for item in imp.novos {
        let id = db.inserir_livro(&item.livro)?;
        for mut nota in item.notas {
            nota.book_id = id;
            db.inserir_nota(&nota)?;
        }
        if let Some(mut p) = item.plano {
            p.book_id = id;
            db.salvar_plano(&p)?;
        }
        for (dia, de, ate) in item.historico {
            db.inserir_leitura(id, dia, de, ate)?;
        }
    }
    for (ano, livros) in imp.metas {
        if db.meta(ano)?.is_none() {
            db.definir_meta(ano, livros)?;
        }
    }
    Ok(n)
}

/// metas por ano: (ano, livros)
type Metas = Vec<(i32, i64)>;

fn ler_json(texto: &str) -> Result<(Vec<Importado>, Metas)> {
    let a: Arquivo = serde_json::from_str(texto).context("JSON inválido")?;
    if a.formato != FORMATO {
        bail!("este JSON não foi exportado pelo Nasus");
    }
    let hoje = Local::now().date_naive();
    let itens = a
        .livros
        .into_iter()
        .filter(|l| !l.titulo.trim().is_empty())
        .map(|l| Importado {
            livro: Book {
                title: l.titulo.trim().into(),
                author: l.autor,
                isbn: l.isbn,
                pages: l.paginas.max(0),
                status: Status::de_chave(&l.status).unwrap_or(Status::Estante),
                current_page: l.pagina_atual.max(0),
                rating: l.nota.min(5),
                review: l.resenha,
                started_at: l.inicio,
                finished_at: l.fim,
                categorias: ler_categorias(&l.categorias.join(",")),
                ..Book::novo()
            },
            notas: l
                .notas
                .into_iter()
                .map(|n| Note {
                    id: 0,
                    book_id: 0,
                    kind: if n.tipo == "citacao" { NoteKind::Citacao } else { NoteKind::Nota },
                    page: n.pagina,
                    body: n.texto,
                    created_at: n.data.unwrap_or(hoje),
                })
                .collect(),
            plano: l.plano.filter(|p| p.paginas_dia > 0).map(|p| Plano {
                book_id: 0,
                inicio: p.inicio,
                pagina_inicial: p.pagina_inicial,
                paginas_dia: p.paginas_dia,
            }),
            historico: l.historico,
        })
        .collect();
    Ok((itens, a.metas))
}

/// ler_goodreads interpreta o CSV de "Exportar biblioteca" do Goodreads.
fn ler_goodreads(texto: &str) -> Result<Vec<Importado>> {
    let mut r = csv::ReaderBuilder::new().flexible(true).from_reader(texto.as_bytes());
    let cab = r.headers()?.clone();
    let col = |nome: &str| cab.iter().position(|c| c.trim() == nome);
    let titulo = col("Title").context("CSV do Goodreads sem a coluna Title")?;
    let (autor, isbn, isbn13, nota, paginas, lido_em, estantes, estante, resenha) = (
        col("Author"),
        col("ISBN"),
        col("ISBN13"),
        col("My Rating"),
        col("Number of Pages"),
        col("Date Read"),
        col("Bookshelves"),
        col("Exclusive Shelf"),
        col("My Review"),
    );
    let mut v = Vec::new();
    for linha in r.records() {
        let linha = linha?;
        let campo = |i: Option<usize>| i.and_then(|i| linha.get(i)).unwrap_or_default().trim().to_string();
        let t = campo(Some(titulo));
        if t.is_empty() {
            continue;
        }
        // o Goodreads escreve o ISBN como ="9788535914849" para a planilha não estragar
        let limpa = |s: String| s.trim_start_matches('=').trim_matches('"').to_string();
        let isbn = Some(limpa(campo(isbn13))).filter(|s| !s.is_empty()).unwrap_or_else(|| limpa(campo(isbn)));
        let pages: i64 = campo(paginas).parse().unwrap_or(0);
        let data = |s: String| NaiveDate::parse_from_str(&s, "%Y/%m/%d").ok();
        let status = match campo(estante).as_str() {
            "read" => Status::Lido,
            "currently-reading" => Status::Lendo,
            "to-read" => Status::QueroLer,
            _ => Status::Estante,
        };
        let cats: Vec<String> = campo(estantes)
            .split(',')
            .map(str::trim)
            .filter(|e| !matches!(*e, "" | "read" | "currently-reading" | "to-read"))
            .map(|e| e.replace('-', " "))
            .collect();
        let mut b = Book {
            title: t,
            author: campo(autor),
            isbn,
            pages,
            status,
            rating: campo(nota).parse::<u8>().unwrap_or(0).min(5),
            review: campo(resenha).replace("<br/>", "\n").replace("<br />", "\n"),
            categorias: ler_categorias(&cats.join(",")),
            ..Book::novo()
        };
        if status == Status::Lido {
            b.finished_at = data(campo(lido_em));
            b.current_page = pages;
        }
        v.push(Importado { livro: b, notas: Vec::new(), plano: None, historico: Vec::new() });
    }
    Ok(v)
}

// --- linha de comando ---

fn opcao(args: &[String], nome: &str) -> Result<Option<String>> {
    match args.iter().position(|a| a == nome) {
        Some(i) => Ok(Some(args.get(i + 1).with_context(|| format!("{nome} precisa de um valor"))?.clone())),
        None => Ok(None),
    }
}

fn ajuda(args: &[String]) -> Result<()> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        bail!("{USO}");
    }
    Ok(())
}

fn perguntar(texto: &str) -> Result<String> {
    print!("{texto}");
    std::io::stdout().flush()?;
    let mut linha = String::new();
    std::io::stdin().lock().read_line(&mut linha)?;
    Ok(linha.trim().to_string())
}

/// exportar trata `nasus exportar [--saida ARQUIVO]`.
pub fn exportar(caminho_db: &Path, args: &[String]) -> Result<()> {
    ajuda(args)?;
    let saida = opcao(args, "--saida")?
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("nasus-{}.json", Local::now().format("%Y-%m-%d"))));
    let db = Db::abrir(caminho_db)?;
    let csv = saida.extension().is_some_and(|e| e.eq_ignore_ascii_case("csv"));
    let conteudo = if csv { exportar_csv(&db)? } else { exportar_json(&db)? };
    std::fs::write(&saida, conteudo).with_context(|| format!("gravando {}", saida.display()))?;
    println!("✔ {} livros exportados para {}", db.livros()?.len(), saida.display());
    Ok(())
}

/// importar trata `nasus importar ARQUIVO [--sim]`.
pub fn importar(caminho_db: &Path, args: &[String]) -> Result<()> {
    ajuda(args)?;
    let arquivo = args.iter().find(|a| !a.starts_with("--")).with_context(|| format!("qual arquivo?\n\n{USO}"))?;
    let texto = std::fs::read_to_string(arquivo).with_context(|| format!("lendo {arquivo}"))?;
    let db = Db::abrir(caminho_db)?;
    let imp = ler_importacao(&db, &texto)?;
    println!("Formato: {}", imp.formato);
    println!("{} livros novos, {} já existentes.", imp.novos(), imp.repetidos.len());
    if imp.novos() == 0 {
        return Ok(());
    }
    if !args.iter().any(|a| a == "--sim") {
        let r = perguntar("Importar? [s/N] ")?;
        if !matches!(r.to_lowercase().as_str(), "s" | "sim" | "y" | "yes") {
            println!("Nada foi importado.");
            return Ok(());
        }
    }
    if let Some(copia) = db.copia_avulsa("pre-importacao")? {
        println!("Cópia do estado atual: {}", copia.display());
    }
    let n = gravar_importacao(&db, imp)?;
    println!("✔ {n} livros importados.");
    Ok(())
}

/// restaurar trata `nasus restaurar [--arquivo COPIA]`.
pub fn restaurar(caminho_db: &Path, args: &[String]) -> Result<()> {
    ajuda(args)?;
    let copia = match opcao(args, "--arquivo")? {
        Some(a) => PathBuf::from(a),
        None => {
            let lista = db::backups(caminho_db)?;
            if lista.is_empty() {
                bail!("nenhuma cópia em {}", db::pasta_backups(caminho_db).display());
            }
            println!("Cópias de segurança (da mais nova para a mais antiga):");
            for (i, p) in lista.iter().enumerate() {
                println!("  {:>2}  {}", i + 1, p.file_name().unwrap_or_default().to_string_lossy());
            }
            let r = perguntar("Restaurar qual? (número, Enter cancela) ")?;
            if r.is_empty() {
                return Ok(());
            }
            let i: usize = r.parse().ok().filter(|i| (1..=lista.len()).contains(i)).context("número inválido")?;
            lista[i - 1].clone()
        }
    };
    println!("Se o bot ou a interface web estiverem rodando, pare-os antes (systemctl --user stop nasus-bot).");
    let r = perguntar(&format!("Trocar o banco atual por {}? [s/N] ", copia.display()))?;
    if !matches!(r.to_lowercase().as_str(), "s" | "sim" | "y" | "yes") {
        println!("Nada mudou.");
        return Ok(());
    }
    let anterior = db::restaurar(caminho_db, &copia)?;
    println!("✔ Banco restaurado.");
    if let Some(a) = anterior {
        println!("O estado anterior ficou em {}", a.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dia(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
    }

    fn biblioteca() -> Db {
        let db = Db::em_memoria().unwrap();
        let mut b = Book { title: "Duna".into(), author: "Frank Herbert".into(), isbn: "9788576573135".into(), pages: 680, ..Book::novo() };
        b.categorias = vec!["ficção científica".into()];
        b.id = db.inserir_livro(&b).unwrap();
        b.registrar_pagina(100, dia(1));
        db.salvar_progresso(&b, dia(1)).unwrap();
        db.inserir_nota(&Note { id: 0, book_id: b.id, kind: NoteKind::Citacao, page: Some(10), body: "O medo mata a mente.".into(), created_at: dia(1) }).unwrap();
        db.aplicar_plano(&b, dia(1), 0, 20).unwrap();
        let mut lido = Book { title: "Dom Casmurro".into(), author: "Machado de Assis".into(), pages: 256, rating: 5, ..Book::novo() };
        lido.mudar_status(Status::Lido, dia(5));
        db.inserir_livro(&lido).unwrap();
        db.definir_meta(2026, 12).unwrap();
        db
    }

    #[test]
    fn json_vai_e_volta_inteiro() {
        let origem = biblioteca();
        let json = exportar_json(&origem).unwrap();
        let destino = Db::em_memoria().unwrap();
        let imp = ler_importacao(&destino, &json).unwrap();
        assert_eq!((imp.novos(), imp.repetidos.len()), (2, 0));
        gravar_importacao(&destino, imp).unwrap();

        let sem_id = |db: &Db| {
            db.livros().unwrap().into_iter().map(|b| (Book { id: 0, ..b.clone() }, db.notas(b.id).unwrap().len(), db.plano(b.id).unwrap().map(|p| p.paginas_dia), db.leituras(b.id).unwrap())).collect::<Vec<_>>()
        };
        assert_eq!(sem_id(&origem), sem_id(&destino));
        assert_eq!(destino.meta(2026).unwrap(), Some(12));
        // o JSON de novo bate (fora a hora da exportação)
        let corta = |s: String| s.lines().filter(|l| !l.contains("exportado_em")).collect::<Vec<_>>().join("\n");
        assert_eq!(corta(exportar_json(&destino).unwrap()), corta(json));

        let de_novo = ler_importacao(&destino, &exportar_json(&origem).unwrap()).unwrap();
        assert_eq!((de_novo.novos(), de_novo.repetidos.len()), (0, 2), "importar duas vezes não duplica");
    }

    #[test]
    fn csv_para_planilha() {
        let csv = exportar_csv(&biblioteca()).unwrap();
        let linhas: Vec<&str> = csv.lines().collect();
        assert!(linhas[0].starts_with("titulo;autor;isbn;paginas;status"));
        assert!(linhas[1].starts_with("Duna;Frank Herbert;9788576573135;680;Lendo;100;0;01/10/2026;;ficção científica"), "{}", linhas[1]);
        assert!(linhas[2].contains("Lido"));
    }

    const GOODREADS: &str = "\
Book Id,Title,Author,Author l-f,Additional Authors,ISBN,ISBN13,My Rating,Average Rating,Publisher,Binding,Number of Pages,Year Published,Original Publication Year,Date Read,Date Added,Bookshelves,Bookshelves with positions,Exclusive Shelf,My Review,Spoiler,Private Notes,Read Count,Owned Copies
1,\"Dune\",Frank Herbert,\"Herbert, Frank\",,\"=\"\"0441013597\"\"\",\"=\"\"9780441013593\"\"\",5,4.25,Ace,Paperback,688,2005,1965,2024/03/10,2024/01/02,\"sci-fi, favorites\",\"sci-fi (#1)\",read,Muito bom.<br/>Recomendo,,,1,0
2,\"Dom Casmurro\",Machado de Assis,,,\"=\"\"\"\"\",\"=\"\"\"\"\",0,4.0,,,256,,,,2024/05/01,currently-reading,,currently-reading,,,,0,0
3,\"Duna\",Frank Herbert,,,\"=\"\"\"\"\",\"=\"\"9788576573135\"\"\",0,4.3,,,680,,,,2024/05/01,to-read,,to-read,,,,0,0
";

    #[test]
    fn importa_o_csv_do_goodreads() {
        let db = biblioteca();
        let imp = ler_importacao(&db, GOODREADS).unwrap();
        assert_eq!(imp.formato, "Goodreads (CSV)");
        assert_eq!(imp.repetidos, ["Dom Casmurro", "Duna"], "mesmo título e autor, ou mesmo ISBN");
        let n = gravar_importacao(&db, imp).unwrap();
        assert_eq!(n, 1);
        let b = db.livro_por_isbn("9780441013593").unwrap().expect("Dune importado");
        assert_eq!((b.title.as_str(), b.status, b.rating, b.pages, b.current_page), ("Dune", Status::Lido, 5, 688, 688));
        assert_eq!(b.finished_at, NaiveDate::from_ymd_opt(2024, 3, 10));
        assert_eq!(b.categorias, ["favorites", "sci fi"]);
        assert_eq!(b.review, "Muito bom.\nRecomendo");
    }

    #[test]
    fn formato_desconhecido() {
        let db = Db::em_memoria().unwrap();
        assert!(ler_importacao(&db, "a;b;c\n1;2;3").is_err());
        assert!(ler_importacao(&db, "{\"formato\": \"outro\"}").is_err());
    }
}
