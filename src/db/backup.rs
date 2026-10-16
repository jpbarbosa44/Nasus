//! Cópias de segurança: uma por dia em `backups/` (as 7 mais recentes),
//! cópias avulsas antes de importar ou de o MCP gravar, e a restauração.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{Local, NaiveDate};
use rusqlite::{Connection, OpenFlags};

use super::Db;

/// quantas cópias diárias ficam guardadas
const MAX_DIARIOS: usize = 7;

/// pasta_backups é a pasta `backups/` ao lado do banco.
pub fn pasta_backups(caminho_db: &Path) -> PathBuf {
    caminho_db.parent().unwrap_or(Path::new(".")).join("backups")
}

/// backups lista as cópias da pasta, da mais nova para a mais antiga.
pub fn backups(caminho_db: &Path) -> Result<Vec<PathBuf>> {
    let pasta = pasta_backups(caminho_db);
    if !pasta.exists() {
        return Ok(Vec::new());
    }
    let mut v: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&pasta)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "db"))
        .map(|p| (p.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), p))
        .collect();
    v.sort_by(|a, b| b.cmp(a));
    Ok(v.into_iter().map(|(_, p)| p).collect())
}

impl Db {
    /// copiar grava uma cópia consistente e compactada do banco em `destino`.
    pub fn copiar(&self, destino: &Path) -> Result<()> {
        if let Some(dir) = destino.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("criando {}", dir.display()))?;
        }
        if destino.exists() {
            std::fs::remove_file(destino)?;
        }
        self.conn
            .execute("VACUUM INTO ?1", [destino.to_string_lossy()])
            .with_context(|| format!("copiando o banco para {}", destino.display()))?;
        Ok(())
    }

    /// backup_diario faz a cópia do dia (uma só por dia) e apaga as mais
    /// antigas, deixando as últimas 7. Devolve o arquivo criado.
    pub fn backup_diario(&self, hoje: NaiveDate) -> Result<Option<PathBuf>> {
        let Some(caminho) = &self.caminho else { return Ok(None) };
        let pasta = pasta_backups(caminho);
        let destino = pasta.join(format!("nasus-{}.db", hoje.format("%Y-%m-%d")));
        if destino.exists() {
            return Ok(None);
        }
        self.copiar(&destino)?;
        let mut diarios: Vec<PathBuf> = std::fs::read_dir(&pasta)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(diario))
            .collect();
        diarios.sort();
        let sobra = diarios.len().saturating_sub(MAX_DIARIOS);
        for velho in &diarios[..sobra] {
            std::fs::remove_file(velho).with_context(|| format!("apagando {}", velho.display()))?;
        }
        Ok(Some(destino))
    }

    /// copia_avulsa guarda o estado atual antes de uma operação grande
    /// (`backups/nasus.<motivo>-AAAAMMDD-HHMMSS.db`), deixando as 10 últimas de cada motivo.
    pub fn copia_avulsa(&self, motivo: &str) -> Result<Option<PathBuf>> {
        let Some(caminho) = &self.caminho else { return Ok(None) };
        let pasta = pasta_backups(caminho);
        let prefixo = format!("nasus.{motivo}-");
        let destino = pasta.join(format!("{prefixo}{}.db", Local::now().format("%Y%m%d-%H%M%S")));
        self.copiar(&destino)?;
        let mut antigas: Vec<PathBuf> = std::fs::read_dir(&pasta)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&prefixo)))
            .collect();
        antigas.sort();
        for velha in &antigas[..antigas.len().saturating_sub(10)] {
            let _ = std::fs::remove_file(velha);
        }
        Ok(Some(destino))
    }
}

/// diario diz se o nome é de uma cópia diária (`nasus-AAAA-MM-DD.db`).
fn diario(nome: &str) -> bool {
    nome.strip_prefix("nasus-")
        .and_then(|n| n.strip_suffix(".db"))
        .is_some_and(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok())
}

/// conferir abre a cópia só para leitura e checa se é um banco do Nasus íntegro.
pub fn conferir(arquivo: &Path) -> Result<()> {
    let conn = Connection::open_with_flags(arquivo, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("abrindo {}", arquivo.display()))?;
    let r: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .with_context(|| format!("{} não é um banco SQLite", arquivo.display()))?;
    if r != "ok" {
        bail!("{} está corrompido: {r}", arquivo.display());
    }
    conn.query_row("SELECT COUNT(*) FROM books", [], |r| r.get::<_, i64>(0))
        .with_context(|| format!("{} não é um banco do Nasus", arquivo.display()))?;
    Ok(())
}

/// restaurar troca o banco pela cópia, guardando antes o estado atual em
/// `backups/nasus.pre-restauracao-*.db`. Devolve onde ficou essa cópia.
pub fn restaurar(caminho_db: &Path, copia: &Path) -> Result<Option<PathBuf>> {
    conferir(copia)?;
    let anterior = if caminho_db.exists() {
        let db = Db::abrir(caminho_db)?;
        let p = db.copia_avulsa("pre-restauracao")?;
        drop(db);
        p
    } else {
        None
    };
    let temp = caminho_db.with_extension("db.restaurando");
    std::fs::copy(copia, &temp).with_context(|| format!("copiando {}", copia.display()))?;
    for sufixo in ["-wal", "-shm"] {
        let extra = PathBuf::from(format!("{}{sufixo}", caminho_db.display()));
        if extra.exists() {
            std::fs::remove_file(&extra).with_context(|| format!("apagando {}", extra.display()))?;
        }
    }
    std::fs::rename(&temp, caminho_db).with_context(|| format!("trocando {}", caminho_db.display()))?;
    Ok(anterior)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Book;

    fn pasta_temp(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("nasus-teste-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn um_backup_por_dia_e_guarda_sete() {
        let pasta = pasta_temp("backup");
        let caminho = pasta.join("nasus.db");
        let db = Db::abrir(&caminho).unwrap();
        db.inserir_livro(&Book { title: "A".into(), ..Book::novo() }).unwrap();
        for d in 1..=9 {
            let hoje = NaiveDate::from_ymd_opt(2026, 10, d).unwrap();
            assert!(db.backup_diario(hoje).unwrap().is_some());
            assert!(db.backup_diario(hoje).unwrap().is_none(), "só uma por dia");
        }
        let mut nomes: Vec<String> = std::fs::read_dir(pasta_backups(&caminho))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| diario(n))
            .collect();
        nomes.sort();
        assert_eq!(nomes.len(), 7);
        assert_eq!(nomes[0], "nasus-2026-10-03.db", "as duas mais antigas saíram");
        conferir(&pasta_backups(&caminho).join("nasus-2026-10-09.db")).unwrap();
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    fn restaurar_troca_o_banco_e_guarda_o_anterior() {
        let pasta = pasta_temp("restaurar");
        let caminho = pasta.join("nasus.db");
        let db = Db::abrir(&caminho).unwrap();
        db.inserir_livro(&Book { title: "Antes".into(), ..Book::novo() }).unwrap();
        let copia = pasta.join("copia.db");
        db.copiar(&copia).unwrap();
        db.inserir_livro(&Book { title: "Depois".into(), ..Book::novo() }).unwrap();
        drop(db);

        let anterior = restaurar(&caminho, &copia).unwrap().expect("guardou o estado atual");
        let titulos = |p: &Path| Db::abrir(p).unwrap().livros().unwrap().into_iter().map(|b| b.title).collect::<Vec<_>>();
        assert_eq!(titulos(&caminho), ["Antes"]);
        assert_eq!(titulos(&anterior), ["Antes", "Depois"]);

        std::fs::write(pasta.join("lixo.db"), b"isto nao e um banco").unwrap();
        assert!(restaurar(&caminho, &pasta.join("lixo.db")).is_err());
        assert_eq!(titulos(&caminho), ["Antes"], "cópia ruim não mexe no banco");
        let _ = std::fs::remove_dir_all(&pasta);
    }
}
