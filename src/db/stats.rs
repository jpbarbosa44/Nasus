use anyhow::Result;
use chrono::{Datelike, NaiveDate};
use rusqlite::OptionalExtension;

use super::Db;
use crate::model::Status;

/// O ano de leitura em destaques, para a retrospectiva.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Retrospectiva {
    pub ano: i32,
    pub lidos: i64,
    pub paginas: i64,
    /// (título, páginas)
    pub mais_longo: Option<(String, i64)>,
    pub mais_curto: Option<(String, i64)>,
    /// (título, dias do início ao fim)
    pub mais_rapido: Option<(String, i64)>,
    /// (autor, livros)
    pub autor: Option<(String, i64)>,
    /// (categoria, livros)
    pub categoria: Option<(String, i64)>,
    /// (mês de 1 a 12, livros)
    pub melhor_mes: Option<(u32, u64)>,
    pub nota_media: Option<f64>,
    pub cinco_estrelas: Vec<String>,
    pub notas: i64,
    pub citacoes: i64,
    /// (texto, título do livro), sorteada entre as citações do ano
    pub citacao: Option<(String, String)>,
    /// dias com alguma página registrada e a maior sequência dentro do ano
    pub dias_lendo: i64,
    pub maior_sequencia: i64,
}

/// Números de um ano de leitura.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Estatisticas {
    pub ano: i32,
    pub meta: Option<i64>,
    pub lidos: i64,
    pub por_mes: [u64; 12],
    pub paginas: i64,
    pub nota_media: Option<f64>,
    pub lendo: i64,
    /// livros lidos no ano por categoria, da mais lida para a menos
    pub por_categoria: Vec<(String, i64)>,
}

impl Db {
    /// definir_meta grava a meta de livros do ano; zero remove a meta.
    pub fn definir_meta(&self, ano: i32, livros: i64) -> Result<()> {
        if livros <= 0 {
            self.conn.execute("DELETE FROM goals WHERE year = ?1", [ano])?;
        } else {
            self.conn.execute(
                "INSERT INTO goals (year, target_books) VALUES (?1, ?2)
                 ON CONFLICT(year) DO UPDATE SET target_books = excluded.target_books",
                (ano, livros),
            )?;
        }
        Ok(())
    }

    /// metas devolve todas as metas, por ano.
    pub fn metas(&self) -> Result<Vec<(i32, i64)>> {
        let mut stmt = self.conn.prepare("SELECT year, target_books FROM goals ORDER BY year")?;
        let v = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    pub fn meta(&self, ano: i32) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row("SELECT target_books FROM goals WHERE year = ?1", [ano], |r| r.get(0))
            .optional()?)
    }

    /// estatisticas conta os livros terminados no ano (pela data de fim).
    pub fn estatisticas(&self, ano: i32) -> Result<Estatisticas> {
        let mut e = Estatisticas { ano, meta: self.meta(ano)?, ..Default::default() };
        let mut stmt = self.conn.prepare(
            "SELECT CAST(strftime('%m', finished_at) AS INTEGER), pages, rating FROM books
             WHERE status = 'read' AND strftime('%Y', finished_at) = ?1",
        )?;
        let mut notas = Vec::new();
        let linhas = stmt.query_map([format!("{ano:04}")], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
        })?;
        for linha in linhas {
            let (mes, paginas, nota) = linha?;
            e.lidos += 1;
            e.paginas += paginas;
            if (1..=12).contains(&mes) {
                e.por_mes[mes as usize - 1] += 1;
            }
            if nota > 0 {
                notas.push(nota as f64);
            }
        }
        if !notas.is_empty() {
            e.nota_media = Some(notas.iter().sum::<f64>() / notas.len() as f64);
        }
        e.lendo = self.conn.query_row("SELECT COUNT(*) FROM books WHERE status = 'reading'", [], |r| r.get(0))?;
        let mut stmt = self.conn.prepare(
            "SELECT lower(c.name), COUNT(*) FROM book_categories c JOIN books b ON b.id = c.book_id
             WHERE b.status = 'read' AND strftime('%Y', b.finished_at) = ?1
             GROUP BY lower(c.name) ORDER BY COUNT(*) DESC, lower(c.name)",
        )?;
        e.por_categoria = stmt
            .query_map([format!("{ano:04}")], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(e)
    }
}

pub const MESES: [&str; 12] = [
    "janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro",
    "dezembro",
];

/// citacoes é o plural de citação, que o `plural` comum não acerta.
pub fn citacoes(n: i64) -> String {
    if n == 1 { "1 citação".into() } else { format!("{n} citações") }
}

impl Retrospectiva {
    /// texto é a retrospectiva em poucas linhas, para o bot e o MCP.
    pub fn texto(&self) -> String {
        use crate::model::plural;
        use crate::ui::milhar;
        let mut t = format!("🎉 Retrospectiva {}", self.ano);
        if self.lidos == 0 && self.dias_lendo == 0 {
            return t + "\nNenhum livro terminado neste ano ainda.";
        }
        t += &format!("\n\n📚 {} · {} páginas", plural(self.lidos, "livro"), milhar(self.paginas));
        if let Some((titulo, p)) = &self.mais_longo {
            t += &format!("\n📖 Mais longo: {titulo} ({} págs)", milhar(*p));
        }
        if let Some((titulo, p)) = self.mais_curto.as_ref().filter(|_| self.lidos > 1) {
            t += &format!("\n🪶 Mais curto: {titulo} ({p} págs)");
        }
        if let Some((titulo, d)) = &self.mais_rapido {
            t += &format!("\n⚡ Mais rápido: {titulo} ({})", plural(*d, "dia"));
        }
        if let Some((autor, n)) = self.autor.as_ref().filter(|(_, n)| *n > 1) {
            t += &format!("\n✍️ Autor do ano: {autor} ({})", plural(*n, "livro"));
        }
        if let Some((cat, n)) = &self.categoria {
            t += &format!("\n🏷 Categoria favorita: {cat} ({})", plural(*n, "livro"));
        }
        if let Some((m, n)) = self.melhor_mes {
            t += &format!("\n📅 Melhor mês: {} ({})", MESES[m as usize - 1], plural(n as i64, "livro"));
        }
        if let Some(m) = self.nota_media {
            t += &format!("\n⭐ Nota média {}", format!("{m:.1}").replace('.', ","));
        }
        if !self.cinco_estrelas.is_empty() {
            t += &format!("\n🌟 Cinco estrelas: {}", self.cinco_estrelas.join(", "));
        }
        if self.dias_lendo > 0 {
            t += &format!(
                "\n🔥 {} lendo · maior sequência: {}",
                plural(self.dias_lendo, "dia"),
                plural(self.maior_sequencia, "dia")
            );
        }
        if self.notas + self.citacoes > 0 {
            t += &format!("\n📝 {} e {}", plural(self.notas, "nota"), citacoes(self.citacoes));
        }
        if let Some((c, livro)) = &self.citacao {
            t += &format!("\n\n“{c}”\n— {livro}");
        }
        t
    }
}

impl Db {
    /// retrospectiva junta os destaques do ano: livros terminados nele, notas
    /// e citações escritas nele e os dias de leitura do histórico.
    pub fn retrospectiva(&self, ano: i32) -> Result<Retrospectiva> {
        let e = self.estatisticas(ano)?;
        let lidos: Vec<_> = self
            .livros()?
            .into_iter()
            .filter(|b| b.status == Status::Lido && b.finished_at.is_some_and(|f| f.year() == ano))
            .collect();
        let mut r = Retrospectiva {
            ano,
            lidos: e.lidos,
            paginas: e.paginas,
            nota_media: e.nota_media,
            categoria: e.por_categoria.first().cloned(),
            ..Default::default()
        };
        let com_paginas = || lidos.iter().filter(|b| b.pages > 0);
        r.mais_longo = com_paginas().max_by_key(|b| b.pages).map(|b| (b.title.clone(), b.pages));
        r.mais_curto = com_paginas().min_by_key(|b| b.pages).map(|b| (b.title.clone(), b.pages));
        r.mais_rapido = lidos
            .iter()
            .filter_map(|b| Some((b, (b.finished_at? - b.started_at?).num_days() + 1)))
            .min_by_key(|(_, d)| *d)
            .map(|(b, d)| (b.title.clone(), d));
        let mut autores: Vec<(String, i64)> = Vec::new();
        for b in lidos.iter().filter(|b| !b.author.is_empty()) {
            match autores.iter_mut().find(|(a, _)| a.eq_ignore_ascii_case(&b.author)) {
                Some((_, n)) => *n += 1,
                None => autores.push((b.author.clone(), 1)),
            }
        }
        // empate fica com quem apareceu primeiro (ordem da biblioteca)
        r.autor = autores.into_iter().fold(None, |m: Option<(String, i64)>, a| match &m {
            Some((_, n)) if *n >= a.1 => m,
            _ => Some(a),
        });
        r.melhor_mes = (1..=12u32)
            .map(|m| (m, e.por_mes[m as usize - 1]))
            .filter(|(_, n)| *n > 0)
            .fold(None, |m: Option<(u32, u64)>, x| match m {
                Some(a) if a.1 >= x.1 => Some(a),
                _ => Some(x),
            });
        r.cinco_estrelas = lidos.iter().filter(|b| b.rating == 5).map(|b| b.title.clone()).collect();

        let ano_txt = format!("{ano:04}");
        (r.notas, r.citacoes) = self.conn.query_row(
            "SELECT COALESCE(SUM(kind = 'note'), 0), COALESCE(SUM(kind = 'quote'), 0) FROM notes
             WHERE strftime('%Y', created_at) = ?1",
            [&ano_txt],
            |l| Ok((l.get(0)?, l.get(1)?)),
        )?;
        r.citacao = self
            .conn
            .query_row(
                "SELECT n.body, b.title FROM notes n JOIN books b ON b.id = n.book_id
                 WHERE n.kind = 'quote' AND strftime('%Y', n.created_at) = ?1 ORDER BY random() LIMIT 1",
                [&ano_txt],
                |l| Ok((l.get(0)?, l.get(1)?)),
            )
            .optional()?;

        let (Some(jan), Some(dez)) = (NaiveDate::from_ymd_opt(ano, 1, 1), NaiveDate::from_ymd_opt(ano, 12, 31)) else {
            return Ok(r);
        };
        let dias = self.paginas_por_dia(jan, dez)?;
        r.dias_lendo = dias.len() as i64;
        let mut corrida = 0;
        let mut anterior: Option<NaiveDate> = None;
        for (d, _) in &dias {
            corrida = if anterior.and_then(|a| a.succ_opt()) == Some(*d) { corrida + 1 } else { 1 };
            r.maior_sequencia = r.maior_sequencia.max(corrida);
            anterior = Some(*d);
        }
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;
    use crate::model::{Book, Status};

    #[test]
    fn estatisticas_do_ano() {
        let db = Db::em_memoria().unwrap();
        let lido = |titulo: &str, paginas, nota, fim: NaiveDate, cats: &[&str]| {
            let categorias = cats.iter().map(|c| c.to_string()).collect();
            let mut b = Book { title: titulo.into(), pages: paginas, rating: nota, categorias, ..Book::novo() };
            b.mudar_status(Status::Lido, fim);
            db.inserir_livro(&b).unwrap();
        };
        lido("A", 100, 4, NaiveDate::from_ymd_opt(2026, 3, 5).unwrap(), &["romance", "clássico"]);
        lido("B", 300, 0, NaiveDate::from_ymd_opt(2026, 3, 20).unwrap(), &["romance"]);
        lido("C", 200, 5, NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(), &[]);
        lido("Ano passado", 999, 1, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(), &["filosofia"]);
        let d = Book { title: "D".into(), status: Status::Lendo, categorias: vec!["romance".into()], ..Book::novo() };
        db.inserir_livro(&d).unwrap();
        db.definir_meta(2026, 12).unwrap();

        let e = db.estatisticas(2026).unwrap();
        assert_eq!(e.meta, Some(12));
        assert_eq!(e.lidos, 3);
        assert_eq!(e.paginas, 600);
        assert_eq!(e.por_mes[2], 2);
        assert_eq!(e.por_mes[10], 1);
        assert_eq!(e.nota_media, Some(4.5), "livro sem nota não entra na média");
        assert_eq!(e.lendo, 1);
        assert_eq!(
            e.por_categoria,
            [("romance".to_string(), 2), ("clássico".to_string(), 1)],
            "só os lidos no ano, do mais lido para o menos"
        );
    }

    #[test]
    fn retrospectiva_do_ano() {
        use crate::model::{Note, NoteKind};
        let db = Db::em_memoria().unwrap();
        let dia = |m, d| NaiveDate::from_ymd_opt(2026, m, d).unwrap();
        let lido = |titulo: &str, autor: &str, paginas, nota, inicio: NaiveDate, fim: NaiveDate| {
            let mut b = Book { title: titulo.into(), author: autor.into(), pages: paginas, rating: nota, ..Book::novo() };
            b.mudar_status(Status::Lendo, inicio);
            b.mudar_status(Status::Lido, fim);
            db.inserir_livro(&b).unwrap()
        };
        let a = lido("Curto", "Clarice", 90, 5, dia(3, 1), dia(3, 2));
        lido("Longo", "Tolstói", 1200, 4, dia(1, 1), dia(3, 20));
        lido("Médio", "Clarice", 300, 5, dia(6, 1), dia(6, 30));
        lido("Ano passado", "Outro", 5000, 5, NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(), NaiveDate::from_ymd_opt(2025, 2, 1).unwrap());
        let nota = |kind, body: &str, d| Note { id: 0, book_id: a, kind, page: None, body: body.into(), created_at: d };
        db.inserir_nota(&nota(NoteKind::Citacao, "Liberdade é pouco.", dia(3, 1))).unwrap();
        db.inserir_nota(&nota(NoteKind::Nota, "gostei", dia(3, 1))).unwrap();
        let mut c = Book { title: "Em andamento".into(), pages: 500, ..Book::novo() };
        c.id = db.inserir_livro(&c).unwrap();
        for (d, p) in [(10, 10), (11, 20), (12, 30), (20, 40)] {
            c.registrar_pagina(p, dia(4, d));
            db.salvar_progresso(&c, dia(4, d)).unwrap();
        }

        let r = db.retrospectiva(2026).unwrap();
        assert_eq!((r.lidos, r.paginas), (3, 1590));
        assert_eq!(r.mais_longo, Some(("Longo".into(), 1200)));
        assert_eq!(r.mais_curto, Some(("Curto".into(), 90)));
        assert_eq!(r.mais_rapido, Some(("Curto".into(), 2)));
        assert_eq!(r.autor, Some(("Clarice".into(), 2)));
        assert_eq!(r.melhor_mes, Some((3, 2)));
        assert_eq!(r.cinco_estrelas, ["Curto", "Médio"]);
        assert_eq!((r.notas, r.citacoes), (1, 1));
        assert_eq!(r.citacao, Some(("Liberdade é pouco.".into(), "Curto".into())));
        assert_eq!((r.dias_lendo, r.maior_sequencia), (4, 3));

        let vazio = db.retrospectiva(2030).unwrap();
        assert_eq!((vazio.lidos, vazio.mais_longo, vazio.citacao), (0, None, None));
    }

    #[test]
    fn meta_atualiza_e_remove() {
        let db = Db::em_memoria().unwrap();
        db.definir_meta(2026, 10).unwrap();
        db.definir_meta(2026, 20).unwrap();
        assert_eq!(db.meta(2026).unwrap(), Some(20));
        db.definir_meta(2026, 0).unwrap();
        assert_eq!(db.meta(2026).unwrap(), None);
    }
}
