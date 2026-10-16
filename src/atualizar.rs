//! `nasus atualizar`: baixa o binário do último release do GitHub, confere
//! o SHA256 e troca o executável em uso. Também guarda, uma vez por dia, qual
//! é a última versão, para o app e o bot avisarem que há novidade.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::Local;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const VERSAO: &str = env!("CARGO_PKG_VERSION");

const REPO: &str = "jpbarbosa44/Nasus";
const USER_AGENT: &str = concat!("nasus/", env!("CARGO_PKG_VERSION"));

/// binários maiores que isso não são de verdade
const MAX_BINARIO: u64 = 200 * 1024 * 1024;

pub const USO: &str = "uso: nasus atualizar     baixa e instala a última versão (confere o SHA256)
     nasus versao        mostra a versão instalada";

/// versao lê "v1.2.3" ou "1.2.3" (ignora sufixos como "-beta").
fn versao(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches('v');
    let s = s.split(['-', '+']).next()?;
    let mut partes = s.split('.').map(|p| p.parse::<u64>().ok());
    let v = (partes.next()??, partes.next()??, partes.next()??);
    partes.next().is_none().then_some(v)
}

/// mais_nova diz se `tag` é uma versão acima da `atual`.
fn mais_nova(tag: &str, atual: &str) -> bool {
    matches!((versao(tag), versao(atual)), (Some(t), Some(a)) if t > a)
}

/// nome_binario é o nome do binário solto no release para este sistema.
fn nome_binario(programa: &str) -> Option<String> {
    let plataforma = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-amd64",
        ("macos", "aarch64") => "mac-arm64",
        ("macos", "x86_64") => "mac-intel",
        ("windows", "x86_64") => "windows-amd64",
        _ => return None,
    };
    let ext = if cfg!(windows) { ".exe" } else { "" };
    Some(format!("{programa}-{plataforma}{ext}"))
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
}

fn ultima_release(espera: Duration) -> Result<Release> {
    let r: Release = ureq::get(&format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/vnd.github+json")
        .timeout(espera)
        .call()
        .context("consultando o último release no GitHub")?
        .into_json()?;
    Ok(r)
}

fn baixar(url: &str) -> Result<Vec<u8>> {
    let resp = ureq::get(url).set("User-Agent", USER_AGENT).timeout(Duration::from_secs(300)).call()?;
    let mut v = Vec::new();
    resp.into_reader().take(MAX_BINARIO + 1).read_to_end(&mut v)?;
    if v.len() as u64 > MAX_BINARIO {
        bail!("download grande demais: {url}");
    }
    Ok(v)
}

/// conferir procura o arquivo no SHA256SUMS (formato do sha256sum) e compara o hash.
fn conferir(dados: &[u8], sums: &str, nome: &str) -> Result<()> {
    let esperado = sums
        .lines()
        .filter_map(|l| l.split_once(char::is_whitespace))
        .find(|(_, arq)| arq.trim().trim_start_matches('*') == nome)
        .map(|(hash, _)| hash.to_lowercase())
        .with_context(|| format!("{nome} não está no SHA256SUMS"))?;
    let calculado: String = Sha256::digest(dados).iter().map(|b| format!("{b:02x}")).collect();
    if calculado != esperado {
        bail!("o SHA256 de {nome} não confere: o download pode estar corrompido; nada foi trocado");
    }
    Ok(())
}

/// trocar põe o binário novo no lugar do `alvo`. No Windows um .exe em uso
/// não pode ser sobrescrito, mas pode ser renomeado: o antigo vira `.old` e
/// é apagado na próxima execução.
fn trocar(alvo: &Path, dados: &[u8]) -> Result<()> {
    let dir = alvo.parent().context("executável sem pasta")?;
    let temp = dir.join(format!(".nasus-novo-{}", std::process::id()));
    std::fs::write(&temp, dados).with_context(|| format!("gravando em {} (precisa de permissão?)", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o755))?;
    }
    if cfg!(windows) && alvo.exists() {
        let velho = antigo(alvo);
        let _ = std::fs::remove_file(&velho);
        if let Err(e) = std::fs::rename(alvo, &velho) {
            let _ = std::fs::remove_file(&temp);
            return Err(e).context("renomeando o executável atual");
        }
    }
    if let Err(e) = std::fs::rename(&temp, alvo) {
        let _ = std::fs::remove_file(&temp);
        return Err(e).with_context(|| format!("trocando {}", alvo.display()));
    }
    Ok(())
}

fn antigo(exe: &Path) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(".old");
    PathBuf::from(s)
}

/// limpar_antigo apaga o `.old` que uma atualização no Windows deixou.
pub fn limpar_antigo() {
    if let Ok(exe) = std::env::current_exe() {
        let velho = antigo(&exe);
        if velho.exists() {
            let _ = std::fs::remove_file(velho);
        }
    }
}

/// atualizar trata `nasus atualizar`.
pub fn atualizar(caminho_db: &Path, args: &[String]) -> Result<()> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        bail!("{USO}");
    }
    println!("Versão instalada: v{VERSAO}. Procurando a última…");
    let rel = ultima_release(Duration::from_secs(20))?;
    guardar_cache(caminho_db, &rel.tag_name);
    if !mais_nova(&rel.tag_name, VERSAO) {
        println!("✔ Você já está na última versão ({}).", rel.tag_name);
        return Ok(());
    }
    let exe = std::env::current_exe().context("não achei o executável em uso")?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let base = format!("https://github.com/{REPO}/releases/download/{}", rel.tag_name);
    let sums = String::from_utf8(baixar(&format!("{base}/SHA256SUMS"))?).context("SHA256SUMS inválido")?;

    // o app desktop, quando instalado ao lado, vai junto
    let mut programas = vec![("nasus", exe.clone())];
    let app = exe.with_file_name(if cfg!(windows) { "nasus-app.exe" } else { "nasus-app" });
    if app.exists() {
        programas.push(("nasus-app", app));
    }
    let mut baixados = Vec::new();
    for (programa, alvo) in programas {
        let nome = nome_binario(programa).context("não há binário pronto para este sistema; compile com cargo install")?;
        println!("Baixando {nome} {}…", rel.tag_name);
        let dados = baixar(&format!("{base}/{nome}"))?;
        conferir(&dados, &sums, &nome)?;
        baixados.push((alvo, dados));
    }
    // só troca depois de baixar e conferir tudo
    for (alvo, dados) in baixados {
        trocar(&alvo, &dados)?;
        println!("✔ {} atualizado.", alvo.display());
    }
    println!("Nasus {} instalado. Novidades: {}", rel.tag_name, rel.html_url);
    if cfg!(target_os = "linux") && bot_usa(&exe) {
        println!("O bot está rodando com a versão antiga: systemctl --user restart nasus-bot");
    }
    Ok(())
}

/// bot_usa diz se o serviço nasus-bot está ativo e roda este mesmo binário.
fn bot_usa(exe: &Path) -> bool {
    let ativo = std::process::Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", "nasus-bot"])
        .status()
        .is_ok_and(|s| s.success());
    if !ativo {
        return false;
    }
    let Ok(saida) = std::process::Command::new("systemctl").args(["--user", "show", "-p", "ExecStart", "--value", "nasus-bot"]).output()
    else {
        return false;
    };
    caminho_exec_start(&String::from_utf8_lossy(&saida.stdout))
        .is_some_and(|p| Path::new(p).canonicalize().is_ok_and(|p| p == exe))
}

/// caminho_exec_start tira o binário de "{ path=/x/nasus ; argv[]=... }".
fn caminho_exec_start(s: &str) -> Option<&str> {
    let resto = s.split("path=").nth(1)?;
    Some(resto.split(" ;").next()?.trim()).filter(|p| !p.is_empty())
}

// --- aviso de versão nova ---

#[derive(Default, Serialize, Deserialize)]
struct Cache {
    /// dia (AAAA-MM-DD) da última consulta ao GitHub
    verificado_em: String,
    ultima: String,
}

fn caminho_cache(caminho_db: &Path) -> PathBuf {
    caminho_db.with_file_name("update.json")
}

fn ler_cache(caminho_db: &Path) -> Cache {
    std::fs::read_to_string(caminho_cache(caminho_db))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn guardar_cache(caminho_db: &Path, ultima: &str) {
    let c = Cache { verificado_em: Local::now().date_naive().to_string(), ultima: ultima.into() };
    if let Ok(s) = serde_json::to_string(&c) {
        let _ = std::fs::write(caminho_cache(caminho_db), s);
    }
}

/// verificar consulta o GitHub no máximo uma vez por dia (4 s de espera, sem
/// reclamar se falhar) e guarda a última versão ao lado do banco.
pub fn verificar(caminho_db: &Path) {
    if ler_cache(caminho_db).verificado_em == Local::now().date_naive().to_string() {
        return;
    }
    // sem rede, anota o dia mesmo assim: tenta de novo só amanhã
    let ultima = match ultima_release(Duration::from_secs(4)) {
        Ok(rel) => rel.tag_name,
        Err(_) => ler_cache(caminho_db).ultima,
    };
    guardar_cache(caminho_db, &ultima);
}

/// nova_versao devolve a versão mais nova que a instalada, segundo a última verificação.
pub fn nova_versao(caminho_db: &Path) -> Option<String> {
    let c = ler_cache(caminho_db);
    mais_nova(&c.ultima, VERSAO).then_some(c.ultima)
}

/// versao trata `nasus versao`.
pub fn mostrar_versao() {
    println!("nasus v{VERSAO}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_start_do_systemd() {
        let s = "{ path=/home/x/.cargo/bin/nasus ; argv[]=/home/x/.cargo/bin/nasus bot ; ignore_errors=no }\n";
        assert_eq!(caminho_exec_start(s), Some("/home/x/.cargo/bin/nasus"));
        assert_eq!(caminho_exec_start(""), None);
    }

    #[test]
    fn compara_versoes() {
        assert_eq!(versao("v0.3.1"), Some((0, 3, 1)));
        assert_eq!(versao("1.2.3-beta"), Some((1, 2, 3)));
        assert_eq!(versao("1.2"), None);
        assert_eq!(versao("dev"), None);
        assert!(mais_nova("v0.10.0", "0.9.9"));
        assert!(!mais_nova("v0.3.0", "0.3.0"));
        assert!(!mais_nova("v0.2.9", "0.3.0"));
        assert!(!mais_nova("lixo", "0.3.0"));
    }

    #[test]
    fn binario_do_sistema() {
        let n = nome_binario("nasus");
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert_eq!(n.as_deref(), Some("nasus-linux-amd64"));
        }
        if cfg!(windows) {
            assert!(n.unwrap().ends_with(".exe"));
        }
    }

    #[test]
    fn confere_o_sha256() {
        let dados = b"binario de teste";
        // sha256 de "binario de teste"
        let hash: String = Sha256::digest(dados).iter().map(|b| format!("{b:02x}")).collect();
        let sums = format!("{}  nasus-linux-amd64.tar.gz\n{hash}  nasus-linux-amd64\n", "0".repeat(64));
        assert!(conferir(dados, &sums, "nasus-linux-amd64").is_ok());
        assert!(conferir(b"outro", &sums, "nasus-linux-amd64").is_err(), "hash diferente");
        assert!(conferir(dados, &sums, "nasus-mac-arm64").is_err(), "fora da lista");
        assert!(conferir(dados, &format!("{hash} *nasus-linux-amd64"), "nasus-linux-amd64").is_ok(), "modo binário");
    }

    #[test]
    fn troca_o_executavel() {
        let dir = std::env::temp_dir().join(format!("nasus-teste-troca-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let alvo = dir.join("nasus");
        std::fs::write(&alvo, b"velho").unwrap();
        trocar(&alvo, b"novo").unwrap();
        assert_eq!(std::fs::read(&alvo).unwrap(), b"novo");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&alvo).unwrap().permissions().mode() & 0o777, 0o755);
        }
        let sobras: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name() != "nasus" && e.file_name() != "nasus.old").collect();
        assert!(sobras.is_empty(), "não deixa temporários");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_avisa_versao_nova() {
        let dir = std::env::temp_dir().join(format!("nasus-teste-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("nasus.db");
        assert_eq!(nova_versao(&db), None, "sem cache, sem aviso");
        guardar_cache(&db, "v999.0.0");
        assert_eq!(nova_versao(&db).as_deref(), Some("v999.0.0"));
        guardar_cache(&db, &format!("v{VERSAO}"));
        assert_eq!(nova_versao(&db), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
