//! Empréstimos: com quem está cada livro emprestado, e desde quando.

use anyhow::{Result, bail};
use chrono::NaiveDate;
use rusqlite::params;

use super::Db;
use crate::model::Book;

/// Um empréstimo, aberto ou já devolvido.
#[derive(Clone, Debug, PartialEq)]
pub struct Emprestimo {
    pub pessoa: String,
    pub desde: NaiveDate,
    pub devolvido: Option<NaiveDate>,
}

impl Db {
    /// emprestar registra que o livro foi para `pessoa`; falha se ele já está emprestado.
    pub fn emprestar(&self, book_id: i64, pessoa: &str, dia: NaiveDate) -> Result<()> {
        let pessoa = pessoa.trim();
        if pessoa.is_empty() {
            bail!("emprestado para quem?");
        }
        let b = self.livro(book_id)?;
        if let Some((outra, desde)) = b.emprestado {
            bail!("{} já está com {outra} desde {}", b.title, desde.format("%d/%m"));
        }
        self.inserir_emprestimo(book_id, &Emprestimo { pessoa: pessoa.into(), desde: dia, devolvido: None })
    }

    /// devolver fecha o empréstimo aberto do livro.
    pub fn devolver(&self, book_id: i64, dia: NaiveDate) -> Result<String> {
        let b = self.livro(book_id)?;
        let Some((pessoa, _)) = b.emprestado else { bail!("{} não está emprestado", b.title) };
        self.conn.execute(
            "UPDATE loans SET returned_at = ?2 WHERE book_id = ?1 AND returned_at IS NULL",
            params![book_id, dia],
        )?;
        Ok(pessoa)
    }

    /// emprestados devolve os livros que estão emprestados agora, do mais antigo para o mais novo.
    pub fn emprestados(&self) -> Result<Vec<Book>> {
        let mut v: Vec<Book> = self.livros()?.into_iter().filter(|b| b.emprestado.is_some()).collect();
        v.sort_by_key(|b| b.emprestado.as_ref().map(|(_, d)| *d));
        Ok(v)
    }

    /// emprestimos devolve todos os empréstimos de um livro, em ordem.
    pub fn emprestimos(&self, book_id: i64) -> Result<Vec<Emprestimo>> {
        let mut stmt =
            self.conn.prepare("SELECT person, lent_at, returned_at FROM loans WHERE book_id = ?1 ORDER BY id")?;
        let v = stmt
            .query_map([book_id], |r| Ok(Emprestimo { pessoa: r.get(0)?, desde: r.get(1)?, devolvido: r.get(2)? }))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    pub fn inserir_emprestimo(&self, book_id: i64, e: &Emprestimo) -> Result<()> {
        self.conn.execute(
            "INSERT INTO loans (book_id, person, lent_at, returned_at) VALUES (?1, ?2, ?3, ?4)",
            params![book_id, e.pessoa, e.desde, e.devolvido],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dia(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
    }

    #[test]
    fn emprestar_e_devolver() {
        let db = Db::em_memoria().unwrap();
        let id = db.inserir_livro(&Book { title: "Duna".into(), ..Book::novo() }).unwrap();
        let outro = db.inserir_livro(&Book { title: "Outro".into(), ..Book::novo() }).unwrap();
        assert!(db.devolver(id, dia(1)).is_err(), "não está emprestado");
        assert!(db.emprestar(id, "  ", dia(1)).is_err());

        db.emprestar(id, "Maria", dia(2)).unwrap();
        assert_eq!(db.livro(id).unwrap().emprestado, Some(("Maria".into(), dia(2))));
        let erro = db.emprestar(id, "João", dia(3)).unwrap_err().to_string();
        assert!(erro.contains("já está com Maria desde 02/10"), "{erro}");
        db.emprestar(outro, "Ana", dia(1)).unwrap();
        let titulos: Vec<String> = db.emprestados().unwrap().into_iter().map(|b| b.title).collect();
        assert_eq!(titulos, ["Outro", "Duna"], "o mais antigo primeiro");

        assert_eq!(db.devolver(id, dia(9)).unwrap(), "Maria");
        assert_eq!(db.livro(id).unwrap().emprestado, None);
        db.emprestar(id, "João", dia(10)).unwrap();
        assert_eq!(
            db.emprestimos(id).unwrap(),
            [
                Emprestimo { pessoa: "Maria".into(), desde: dia(2), devolvido: Some(dia(9)) },
                Emprestimo { pessoa: "João".into(), desde: dia(10), devolvido: None },
            ]
        );
    }
}
