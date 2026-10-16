use anyhow::Result;
use rusqlite::params;

use super::Db;
use crate::model::{Note, NoteKind};

impl Db {
    pub fn notas(&self, book_id: i64) -> Result<Vec<Note>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, book_id, kind, page, body, created_at FROM notes WHERE book_id = ?1 ORDER BY id",
        )?;
        let notas = stmt
            .query_map([book_id], |r| {
                let kind: String = r.get(2)?;
                Ok(Note {
                    id: r.get(0)?,
                    book_id: r.get(1)?,
                    kind: NoteKind::de_chave(&kind),
                    page: r.get(3)?,
                    body: r.get(4)?,
                    created_at: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(notas)
    }

    pub fn inserir_nota(&self, n: &Note) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO notes (book_id, kind, page, body, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![n.book_id, n.kind.chave(), n.page, n.body, n.created_at],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn remover_nota(&self, id: i64) -> Result<()> {
        if self.conn.execute("DELETE FROM notes WHERE id = ?1", [id])? == 0 {
            anyhow::bail!("nota {id} não encontrada");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;
    use crate::model::Book;

    #[test]
    fn notas_somem_junto_com_o_livro() {
        let db = Db::em_memoria().unwrap();
        let livro = db.inserir_livro(&Book { title: "Duna".into(), ..Book::novo() }).unwrap();
        let nota = Note {
            id: 0,
            book_id: livro,
            kind: NoteKind::Citacao,
            page: Some(42),
            body: "O medo é o assassino da mente.".into(),
            created_at: NaiveDate::from_ymd_opt(2026, 10, 14).unwrap(),
        };
        let id = db.inserir_nota(&nota).unwrap();
        assert_eq!(db.notas(livro).unwrap(), vec![Note { id, ..nota }]);

        db.remover_livro(livro).unwrap();
        assert!(db.notas(livro).unwrap().is_empty());
    }
}
