use anyhow::Result;
use rusqlite::OptionalExtension;

use super::Db;

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
    fn meta_atualiza_e_remove() {
        let db = Db::em_memoria().unwrap();
        db.definir_meta(2026, 10).unwrap();
        db.definir_meta(2026, 20).unwrap();
        assert_eq!(db.meta(2026).unwrap(), Some(20));
        db.definir_meta(2026, 0).unwrap();
        assert_eq!(db.meta(2026).unwrap(), None);
    }
}
