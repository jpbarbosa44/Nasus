use anyhow::{Result, bail};
use chrono::NaiveDate;
use rusqlite::{OptionalExtension, params};

use super::Db;
use crate::model::{Book, Plano, Status};

impl Db {
    /// aplicar_plano grava o plano do livro (ou o remove, com 0 páginas por
    /// dia). Um livro que estava na estante ou na fila passa a "lendo" no início do plano.
    pub fn aplicar_plano(
        &self,
        livro: &Book,
        inicio: NaiveDate,
        pagina_inicial: i64,
        paginas_dia: i64,
    ) -> Result<Option<Plano>> {
        if livro.pages <= 0 {
            bail!("informe o total de páginas do livro antes de criar um plano");
        }
        if paginas_dia <= 0 {
            self.remover_plano(livro.id)?;
            return Ok(None);
        }
        let p = Plano { book_id: livro.id, inicio, pagina_inicial, paginas_dia };
        self.salvar_plano(&p)?;
        if matches!(livro.status, Status::Estante | Status::QueroLer) {
            let mut b = livro.clone();
            b.mudar_status(Status::Lendo, inicio);
            self.salvar_livro(&b)?;
        }
        Ok(Some(p))
    }

    pub fn plano(&self, book_id: i64) -> Result<Option<Plano>> {
        Ok(self
            .conn
            .query_row(
                "SELECT book_id, start_date, start_page, pages_per_day FROM plans WHERE book_id = ?1",
                [book_id],
                |r| Ok(Plano { book_id: r.get(0)?, inicio: r.get(1)?, pagina_inicial: r.get(2)?, paginas_dia: r.get(3)? }),
            )
            .optional()?)
    }

    pub fn salvar_plano(&self, p: &Plano) -> Result<()> {
        self.conn.execute(
            "INSERT INTO plans (book_id, start_date, start_page, pages_per_day) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(book_id) DO UPDATE SET start_date = excluded.start_date,
               start_page = excluded.start_page, pages_per_day = excluded.pages_per_day",
            params![p.book_id, p.inicio, p.pagina_inicial, p.paginas_dia],
        )?;
        Ok(())
    }

    pub fn remover_plano(&self, book_id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM plans WHERE book_id = ?1", [book_id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;
    use crate::model::Book;

    #[test]
    fn crud_de_planos() {
        let db = Db::em_memoria().unwrap();
        let livro = db.inserir_livro(&Book { title: "Livro de testes".into(), pages: 800, ..Book::novo() }).unwrap();
        assert_eq!(db.plano(livro).unwrap(), None);

        let mut p = Plano {
            book_id: livro,
            inicio: NaiveDate::from_ymd_opt(2026, 10, 14).unwrap(),
            pagina_inicial: 0,
            paginas_dia: 25,
        };
        db.salvar_plano(&p).unwrap();
        p.paginas_dia = 40;
        db.salvar_plano(&p).unwrap();
        assert_eq!(db.plano(livro).unwrap(), Some(p.clone()));

        db.remover_plano(livro).unwrap();
        assert_eq!(db.plano(livro).unwrap(), None);

        db.salvar_plano(&p).unwrap();
        db.remover_livro(livro).unwrap();
        assert_eq!(db.plano(livro).unwrap(), None, "some junto com o livro");
    }

    #[test]
    fn aplicar_plano_comeca_a_leitura() {
        let db = Db::em_memoria().unwrap();
        let inicio = NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        let sem_paginas = db.inserir_livro(&Book { title: "?".into(), ..Book::novo() }).unwrap();
        assert!(db.aplicar_plano(&db.livro(sem_paginas).unwrap(), inicio, 0, 25).is_err());

        let id = db.inserir_livro(&Book { title: "Livro de testes".into(), pages: 800, ..Book::novo() }).unwrap();
        let p = db.aplicar_plano(&db.livro(id).unwrap(), inicio, 0, 25).unwrap().unwrap();
        assert_eq!(db.plano(id).unwrap(), Some(p));
        let b = db.livro(id).unwrap();
        assert_eq!((b.status, b.started_at), (Status::Lendo, Some(inicio)));

        assert_eq!(db.aplicar_plano(&b, inicio, 0, 0).unwrap(), None);
        assert_eq!(db.plano(id).unwrap(), None);
    }
}
