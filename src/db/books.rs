use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::Db;
use crate::model::{Book, Status, ler_categorias};

const COLUNAS: &str = "id, title, author, isbn, pages, status, current_page, rating, review, started_at, finished_at,
    (SELECT group_concat(name, ',') FROM book_categories c WHERE c.book_id = books.id)";

fn de_linha(r: &Row) -> rusqlite::Result<Book> {
    let status: String = r.get(5)?;
    Ok(Book {
        id: r.get(0)?,
        title: r.get(1)?,
        author: r.get(2)?,
        isbn: r.get(3)?,
        pages: r.get(4)?,
        status: Status::de_chave(&status).unwrap_or(Status::Estante),
        current_page: r.get(6)?,
        rating: r.get::<_, i64>(7)?.clamp(0, 5) as u8,
        review: r.get(8)?,
        started_at: r.get(9)?,
        finished_at: r.get(10)?,
        categorias: ler_categorias(&r.get::<_, Option<String>>(11)?.unwrap_or_default()),
    })
}

impl Db {
    /// livros devolve todos os livros: os em leitura primeiro, depois a
    /// fila, a estante, os lidos e os abandonados, cada grupo em ordem alfabética.
    pub fn livros(&self) -> Result<Vec<Book>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUNAS} FROM books ORDER BY
               CASE status WHEN 'reading' THEN 0 WHEN 'want' THEN 1 WHEN 'shelf' THEN 2 WHEN 'read' THEN 3 ELSE 4 END,
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

    /// livro_por_isbn acha o livro com esse ISBN (ignorando hífens e espaços).
    pub fn livro_por_isbn(&self, isbn: &str) -> Result<Option<Book>> {
        let isbn: String = isbn.chars().filter(|c| !matches!(c, '-' | ' ')).collect::<String>().to_uppercase();
        if isbn.is_empty() {
            return Ok(None);
        }
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLUNAS} FROM books WHERE upper(replace(replace(isbn, '-', ''), ' ', '')) = ?1 LIMIT 1"),
                [isbn],
                de_linha,
            )
            .optional()?)
    }

    pub fn inserir_livro(&self, b: &Book) -> Result<i64> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO books (title, author, isbn, pages, status, current_page, rating, review, started_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                b.title, b.author, b.isbn, b.pages, b.status.chave(), b.current_page, b.rating,
                b.review, b.started_at, b.finished_at
            ],
        )?;
        let id = tx.last_insert_rowid();
        gravar_categorias(&tx, id, &b.categorias)?;
        tx.commit()?;
        Ok(id)
    }

    pub fn salvar_livro(&self, b: &Book) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE books SET title = ?2, author = ?3, isbn = ?4, pages = ?5, status = ?6, current_page = ?7,
               rating = ?8, review = ?9, started_at = ?10, finished_at = ?11
             WHERE id = ?1",
            params![
                b.id, b.title, b.author, b.isbn, b.pages, b.status.chave(), b.current_page, b.rating,
                b.review, b.started_at, b.finished_at
            ],
        )?;
        gravar_categorias(&tx, b.id, &b.categorias)?;
        tx.commit()?;
        Ok(())
    }

    /// categorias devolve todas as categorias em uso e quantos livros têm cada uma.
    pub fn categorias(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT lower(name), COUNT(*) FROM book_categories GROUP BY lower(name) ORDER BY lower(name)")?;
        let v = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    pub fn remover_livro(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM books WHERE id = ?1", [id])?;
        Ok(())
    }
}

/// gravar_categorias substitui as categorias do livro.
fn gravar_categorias(conn: &Connection, id: i64, categorias: &[String]) -> Result<()> {
    conn.execute("DELETE FROM book_categories WHERE book_id = ?1", [id])?;
    let mut stmt = conn.prepare("INSERT OR IGNORE INTO book_categories (book_id, name) VALUES (?1, ?2)")?;
    for c in categorias {
        stmt.execute(params![id, c])?;
    }
    Ok(())
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

        b.isbn = "978-85-359-1484-9".into();
        db.salvar_livro(&b).unwrap();
        assert_eq!(db.livro_por_isbn("9788535914849").unwrap().map(|l| l.id), Some(b.id));
        assert_eq!(db.livro_por_isbn("9780000000000").unwrap(), None);
        assert_eq!(db.livro_por_isbn("").unwrap(), None);

        db.remover_livro(b.id).unwrap();
        assert!(db.livro(b.id).is_err());
    }

    #[test]
    fn livros_em_leitura_vem_primeiro() {
        let db = Db::em_memoria().unwrap();
        for (titulo, status) in
            [("B", Status::Lido), ("D", Status::Estante), ("A", Status::QueroLer), ("C", Status::Lendo)]
        {
            db.inserir_livro(&Book { title: titulo.into(), status, ..Book::novo() }).unwrap();
        }
        let titulos: Vec<_> = db.livros().unwrap().into_iter().map(|b| b.title).collect();
        assert_eq!(titulos, ["C", "A", "D", "B"]);
    }

    #[test]
    fn categorias_dos_livros() {
        let db = Db::em_memoria().unwrap();
        let mut a = Book { title: "A".into(), categorias: vec!["filosofia".into(), "romance".into()], ..Book::novo() };
        a.id = db.inserir_livro(&a).unwrap();
        let b = Book { title: "B".into(), categorias: vec!["romance".into()], ..Book::novo() };
        let b_id = db.inserir_livro(&b).unwrap();
        assert_eq!(db.livro(a.id).unwrap().categorias, ["filosofia", "romance"]);
        assert_eq!(db.categorias().unwrap(), [("filosofia".to_string(), 1), ("romance".to_string(), 2)]);

        a.categorias = vec!["clássico".into()];
        db.salvar_livro(&a).unwrap();
        assert_eq!(db.livro(a.id).unwrap().categorias, ["clássico"]);
        db.remover_livro(b_id).unwrap();
        assert_eq!(db.categorias().unwrap(), [("clássico".to_string(), 1)], "somem com o livro");
        assert!(db.livros().unwrap()[0].categorias == ["clássico"]);
    }
}
