use anyhow::{Context, Result};
use rusqlite::{Row, params};

use super::Db;
use crate::model::{Book, Status};

const COLUNAS: &str =
    "id, title, author, isbn, pages, status, current_page, rating, review, started_at, finished_at";

fn de_linha(r: &Row) -> rusqlite::Result<Book> {
    let status: String = r.get(5)?;
    Ok(Book {
        id: r.get(0)?,
        title: r.get(1)?,
        author: r.get(2)?,
        isbn: r.get(3)?,
        pages: r.get(4)?,
        status: Status::de_chave(&status).unwrap_or(Status::QueroLer),
        current_page: r.get(6)?,
        rating: r.get::<_, i64>(7)?.clamp(0, 5) as u8,
        review: r.get(8)?,
        started_at: r.get(9)?,
        finished_at: r.get(10)?,
    })
}

impl Db {
    /// livros devolve todos os livros: os em leitura primeiro, depois a
    /// fila, os lidos e os abandonados, cada grupo em ordem alfabética.
    pub fn livros(&self) -> Result<Vec<Book>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUNAS} FROM books ORDER BY
               CASE status WHEN 'reading' THEN 0 WHEN 'want' THEN 1 WHEN 'read' THEN 2 ELSE 3 END,
               title COLLATE NOCASE"
        ))?;
        let livros = stmt.query_map([], de_linha)?.collect::<rusqlite::Result<_>>()?;
        Ok(livros)
    }

    pub fn livro(&self, id: i64) -> Result<Book> {
        self.conn
            .query_row(&format!("SELECT {COLUNAS} FROM books WHERE id = ?1"), [id], de_linha)
            .with_context(|| format!("livro {id} não encontrado"))
    }

    pub fn inserir_livro(&self, b: &Book) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO books (title, author, isbn, pages, status, current_page, rating, review, started_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                b.title, b.author, b.isbn, b.pages, b.status.chave(), b.current_page, b.rating,
                b.review, b.started_at, b.finished_at
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn salvar_livro(&self, b: &Book) -> Result<()> {
        self.conn.execute(
            "UPDATE books SET title = ?2, author = ?3, isbn = ?4, pages = ?5, status = ?6, current_page = ?7,
               rating = ?8, review = ?9, started_at = ?10, finished_at = ?11
             WHERE id = ?1",
            params![
                b.id, b.title, b.author, b.isbn, b.pages, b.status.chave(), b.current_page, b.rating,
                b.review, b.started_at, b.finished_at
            ],
        )?;
        Ok(())
    }

    pub fn remover_livro(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM books WHERE id = ?1", [id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    #[test]
    fn crud_de_livros() {
        let db = Db::em_memoria().unwrap();
        let mut b = Book { title: "Dom Casmurro".into(), author: "Machado de Assis".into(), pages: 256, ..Book::novo() };
        b.mudar_status(Status::Lendo, NaiveDate::from_ymd_opt(2026, 10, 1).unwrap());
        b.id = db.inserir_livro(&b).unwrap();
        assert_eq!(db.livro(b.id).unwrap(), b);

        b.current_page = 120;
        b.rating = 4;
        db.salvar_livro(&b).unwrap();
        assert_eq!(db.livro(b.id).unwrap(), b);

        db.remover_livro(b.id).unwrap();
        assert!(db.livro(b.id).is_err());
    }

    #[test]
    fn livros_em_leitura_vem_primeiro() {
        let db = Db::em_memoria().unwrap();
        for (titulo, status) in [("B", Status::Lido), ("A", Status::QueroLer), ("C", Status::Lendo)] {
            db.inserir_livro(&Book { title: titulo.into(), status, ..Book::novo() }).unwrap();
        }
        let titulos: Vec<_> = db.livros().unwrap().into_iter().map(|b| b.title).collect();
        assert_eq!(titulos, ["C", "A", "B"]);
    }
}
