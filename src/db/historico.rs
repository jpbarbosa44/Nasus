//! Histórico de leitura: uma linha por livro e por dia, com a página onde o
//! dia começou e onde terminou. É a base da sequência de dias, do mapa de
//! calor e do ritmo de cada livro.

use anyhow::Result;
use chrono::{Days, NaiveDate};
use rusqlite::{Connection, params};

use super::Db;
use crate::model::Book;

/// Dias seguidos com leitura.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sequencia {
    /// conta até hoje, ou até ontem se hoje ainda não teve leitura
    pub atual: i64,
    pub recorde: i64,
    pub leu_hoje: bool,
}

/// anotar registra que o livro foi da página `de` para `ate` no dia. A
/// primeira anotação do dia guarda o começo; as seguintes só movem o fim.
pub(super) fn anotar(conn: &Connection, book_id: i64, dia: NaiveDate, de: i64, ate: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO reading_log (book_id, day, from_page, to_page) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(book_id, day) DO UPDATE SET to_page = excluded.to_page",
        params![book_id, dia, de, ate],
    )?;
    Ok(())
}

impl Db {
    /// paginas_por_dia soma as páginas lidas em cada dia do intervalo (só os
    /// dias com leitura). Voltar páginas não conta como leitura negativa.
    pub fn paginas_por_dia(&self, de: NaiveDate, ate: NaiveDate) -> Result<Vec<(NaiveDate, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT day, SUM(max(0, to_page - from_page)) FROM reading_log
             WHERE day BETWEEN ?1 AND ?2 GROUP BY day HAVING SUM(max(0, to_page - from_page)) > 0 ORDER BY day",
        )?;
        let v = stmt.query_map(params![de, ate], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    /// sequencia conta os dias seguidos com leitura. A sequência de ontem
    /// ainda vale hoje, enquanto o dia não acaba.
    pub fn sequencia(&self, hoje: NaiveDate) -> Result<Sequencia> {
        let dias: Vec<NaiveDate> = self.paginas_por_dia(NaiveDate::from_ymd_opt(1900, 1, 1).unwrap_or(hoje), hoje)?.into_iter().map(|(d, _)| d).collect();
        let mut s = Sequencia { leu_hoje: dias.last() == Some(&hoje), ..Default::default() };
        let mut corrida = 0;
        let mut anterior: Option<NaiveDate> = None;
        for &d in &dias {
            corrida = if anterior.and_then(|a| a.succ_opt()) == Some(d) { corrida + 1 } else { 1 };
            s.recorde = s.recorde.max(corrida);
            anterior = Some(d);
        }
        let ontem = hoje.pred_opt().unwrap_or(hoje);
        if anterior == Some(hoje) || anterior == Some(ontem) {
            s.atual = corrida;
        }
        Ok(s)
    }

    /// ritmo é a média de páginas por dia do livro nos últimos 14 dias,
    /// contando do primeiro dia com leitura dentro dessa janela.
    pub fn ritmo(&self, book_id: i64, hoje: NaiveDate) -> Result<Option<f64>> {
        let desde = hoje - Days::new(13);
        let (primeiro, paginas): (Option<NaiveDate>, i64) = self.conn.query_row(
            "SELECT MIN(day), COALESCE(SUM(max(0, to_page - from_page)), 0) FROM reading_log
             WHERE book_id = ?1 AND day BETWEEN ?2 AND ?3",
            params![book_id, desde, hoje],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok(primeiro.filter(|_| paginas > 0).map(|p| paginas as f64 / ((hoje - p).num_days() + 1) as f64))
    }

    /// previsao estima o dia em que o livro termina, no ritmo atual.
    pub fn previsao(&self, b: &Book, hoje: NaiveDate) -> Result<Option<NaiveDate>> {
        let restantes = b.pages - b.current_page;
        if b.pages <= 0 || restantes <= 0 {
            return Ok(None);
        }
        Ok(self.ritmo(b.id, hoje)?.map(|r| {
            // o dia de hoje conta se ainda não teve leitura; arredonda para cima
            hoje + Days::new(((restantes as f64 / r).ceil() as u64).saturating_sub(1))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Status;

    fn dia(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
    }

    fn lendo(db: &Db, paginas: i64) -> Book {
        let mut b = Book { title: "Livro".into(), pages: paginas, ..Book::novo() };
        b.id = db.inserir_livro(&b).unwrap();
        b
    }

    fn ler(db: &Db, b: &mut Book, pagina: i64, d: NaiveDate) {
        b.registrar_pagina(pagina, d);
        db.salvar_progresso(b, d).unwrap();
    }

    #[test]
    fn uma_linha_por_dia_e_voltar_nao_conta() {
        let db = Db::em_memoria().unwrap();
        let mut b = lendo(&db, 300);
        ler(&db, &mut b, 20, dia(1));
        ler(&db, &mut b, 50, dia(1));
        ler(&db, &mut b, 40, dia(2));
        assert_eq!(db.paginas_por_dia(dia(1), dia(31)).unwrap(), [(dia(1), 50)], "o dia 2 voltou 10 páginas");
        ler(&db, &mut b, 70, dia(2));
        assert_eq!(db.paginas_por_dia(dia(1), dia(31)).unwrap(), [(dia(1), 50), (dia(2), 20)]);
    }

    #[test]
    fn terminar_conta_as_ultimas_paginas_mas_marcar_antigo_nao() {
        let db = Db::em_memoria().unwrap();
        let mut b = lendo(&db, 300);
        ler(&db, &mut b, 250, dia(1));
        b.mudar_status(Status::Lido, dia(2));
        db.salvar_progresso(&b, dia(2)).unwrap();
        assert_eq!(db.paginas_por_dia(dia(2), dia(2)).unwrap(), [(dia(2), 50)]);

        let mut antigo = lendo(&db, 500);
        antigo.mudar_status(Status::Lido, dia(3));
        db.salvar_progresso(&antigo, dia(3)).unwrap();
        assert!(db.paginas_por_dia(dia(3), dia(3)).unwrap().is_empty(), "lido de outra época não é leitura de hoje");
    }

    #[test]
    fn sequencia_atual_e_recorde() {
        let db = Db::em_memoria().unwrap();
        let mut b = lendo(&db, 1000);
        for (d, p) in [(1, 10), (2, 20), (3, 30), (4, 40), (7, 50), (8, 60)] {
            ler(&db, &mut b, p, dia(d));
        }
        assert_eq!(db.sequencia(dia(8)).unwrap(), Sequencia { atual: 2, recorde: 4, leu_hoje: true });
        assert_eq!(db.sequencia(dia(9)).unwrap(), Sequencia { atual: 2, recorde: 4, leu_hoje: false }, "ontem ainda vale");
        assert_eq!(db.sequencia(dia(10)).unwrap().atual, 0, "dois dias sem ler zera");
    }

    #[test]
    fn ritmo_e_previsao() {
        let db = Db::em_memoria().unwrap();
        let mut b = lendo(&db, 300);
        assert_eq!(db.ritmo(b.id, dia(10)).unwrap(), None);
        ler(&db, &mut b, 30, dia(1));
        ler(&db, &mut b, 60, dia(3));
        assert_eq!(db.ritmo(b.id, dia(3)).unwrap(), Some(20.0), "60 páginas em 3 dias");
        // faltam 240 a 20 por dia: 12 dias, contando hoje
        assert_eq!(db.previsao(&b, dia(3)).unwrap(), Some(dia(14)));
    }
}
