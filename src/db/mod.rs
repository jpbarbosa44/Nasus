//! Persistência em SQLite. Cada migração em MIGRACOES roda uma vez, controlada
//! pelo `PRAGMA user_version`.

mod backup;
mod books;
mod emprestimos;
mod historico;
mod notes;
mod plans;
mod stats;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use rusqlite::Connection;

pub use backup::{backups, pasta_backups, restaurar};
pub use emprestimos::Emprestimo;
pub use historico::Sequencia;
pub use stats::Estatisticas;

const MIGRACOES: &[&str] = &[r#"
CREATE TABLE books (
    id           INTEGER PRIMARY KEY,
    title        TEXT    NOT NULL,
    author       TEXT    NOT NULL DEFAULT '',
    isbn         TEXT    NOT NULL DEFAULT '',
    pages        INTEGER NOT NULL DEFAULT 0,
    status       TEXT    NOT NULL DEFAULT 'want',
    current_page INTEGER NOT NULL DEFAULT 0,
    rating       INTEGER NOT NULL DEFAULT 0,
    review       TEXT    NOT NULL DEFAULT '',
    started_at   TEXT,
    finished_at  TEXT,
    created_at   TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE notes (
    id         INTEGER PRIMARY KEY,
    book_id    INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    kind       TEXT    NOT NULL DEFAULT 'note',
    page       INTEGER,
    body       TEXT    NOT NULL,
    created_at TEXT    NOT NULL
);
CREATE INDEX notes_book ON notes(book_id);
CREATE TABLE goals (
    year         INTEGER PRIMARY KEY,
    target_books INTEGER NOT NULL
);
"#, r#"
CREATE TABLE plans (
    book_id       INTEGER PRIMARY KEY REFERENCES books(id) ON DELETE CASCADE,
    start_date    TEXT    NOT NULL,
    start_page    INTEGER NOT NULL,
    pages_per_day INTEGER NOT NULL
);
"#, r#"
CREATE TABLE book_categories (
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    name    TEXT    NOT NULL COLLATE NOCASE,
    PRIMARY KEY (book_id, name)
);
"#, r#"
CREATE TABLE reading_log (
    book_id   INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    day       TEXT    NOT NULL,
    from_page INTEGER NOT NULL,
    to_page   INTEGER NOT NULL,
    PRIMARY KEY (book_id, day)
);
"#, r#"
CREATE TABLE loans (
    id          INTEGER PRIMARY KEY,
    book_id     INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    person      TEXT    NOT NULL,
    lent_at     TEXT    NOT NULL,
    returned_at TEXT
);
CREATE INDEX loans_book ON loans(book_id);
"#, r#"
ALTER TABLE books ADD COLUMN series TEXT NOT NULL DEFAULT '';
ALTER TABLE books ADD COLUMN series_index INTEGER NOT NULL DEFAULT 0;
"#];

pub struct Db {
    conn: Connection,
    /// arquivo do banco; None no banco em memória dos testes
    caminho: Option<PathBuf>,
}

impl Db {
    pub fn abrir(caminho: &Path) -> Result<Self> {
        if let Some(dir) = caminho.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("criando {}", dir.display()))?;
        }
        Self::preparar(Connection::open(caminho)?, Some(caminho.to_path_buf()))
    }

    #[cfg(test)]
    pub fn em_memoria() -> Result<Self> {
        Self::preparar(Connection::open_in_memory()?, None)
    }

    fn preparar(conn: Connection, caminho: Option<PathBuf>) -> Result<Self> {
        // o bot e a TUI podem usar o banco ao mesmo tempo: WAL deixa um ler
        // enquanto o outro grava, e o busy_timeout espera em vez de falhar
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        let versao: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let db = Db { conn, caminho };
        // a cópia do dia sai antes das migrações: se uma der errado, dá para voltar
        if versao > 0
            && let Err(e) = db.backup_diario(chrono::Local::now().date_naive())
        {
            eprintln!("aviso: backup diário: {e:#}");
        }
        let conn = &db.conn;
        for (i, sql) in MIGRACOES.iter().enumerate().skip(versao as usize) {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(sql).with_context(|| format!("migração {}", i + 1))?;
            tx.execute_batch(&format!("PRAGMA user_version = {}", i + 1))?;
            tx.commit()?;
        }
        Ok(db)
    }

    /// caminho_padrao é `$NASUS_DB` ou `~/.local/share/nasus/nasus.db`.
    pub fn caminho_padrao() -> Result<PathBuf> {
        if let Ok(p) = std::env::var("NASUS_DB")
            && !p.is_empty()
        {
            return Ok(p.into());
        }
        let dirs = ProjectDirs::from("", "", "nasus").context("não achei o diretório de dados do usuário")?;
        Ok(dirs.data_dir().join("nasus.db"))
    }
}
