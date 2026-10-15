//! Serviço systemd de usuário para o bot.
//!
//! O bot fala com o Telegram por long polling: abre uma conexão DE SAÍDA para
//! api.telegram.org e as mensagens chegam por ali. Ou seja, ele responde de
//! qualquer rede, sem IP fixo nem porta aberta; só precisa estar rodando. O
//! serviço mantém o `nasus bot` no ar sempre que o computador estiver ligado,
//! inclusive sem ninguém logado e depois de reiniciar.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};

pub const NOME: &str = "nasus-bot";
const ARQUIVO: &str = "nasus-bot.service";
/// marca a execução que é o próprio serviço (definida no unit)
const VAR_SERVICO: &str = "NASUS_BOT_SERVICE";

fn caminho_unit() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME não definido")?;
    Ok(PathBuf::from(home).join(".config/systemd/user").join(ARQUIVO))
}

fn systemctl(args: &[&str]) -> Result<()> {
    let out = Command::new("systemctl").arg("--user").args(args).output().context("rodando systemctl")?;
    if !out.status.success() {
        bail!("systemctl --user {} falhou:\n{}", args.join(" "), String::from_utf8_lossy(&out.stderr));
    }
    Ok(())
}

/// unit monta o arquivo do serviço; fixa o banco em uso quando NASUS_DB está
/// definido, para o serviço não depender do ambiente.
fn unit(exe: &str, nasus_db: Option<&str>) -> String {
    let db = nasus_db.map_or(String::new(), |p| format!("Environment=NASUS_DB={p}\n"));
    format!(
        "[Unit]
Description=Nasus — bot de Telegram
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
Environment={VAR_SERVICO}=1
{db}ExecStart={exe} bot
Restart=always
RestartSec=10

[Install]
WantedBy=default.target
"
    )
}

pub fn instalar() -> Result<()> {
    if !cfg!(target_os = "linux") {
        bail!(
            "instalação automática do serviço só no Linux (systemd).\n\
             No macOS use um launchd agent; no Windows, o Agendador de Tarefas para rodar `nasus bot`"
        );
    }
    let exe = std::env::current_exe()?.canonicalize()?;
    let caminho = caminho_unit()?;
    std::fs::create_dir_all(caminho.parent().context("caminho do unit")?)?;
    let nasus_db = std::env::var("NASUS_DB").ok().filter(|p| !p.is_empty());
    std::fs::write(&caminho, unit(&exe.to_string_lossy(), nasus_db.as_deref()))?;

    // linger mantém os serviços do usuário no ar mesmo sem sessão aberta
    let linger = Command::new("loginctl").arg("enable-linger").output();
    if !linger.is_ok_and(|o| o.status.success()) {
        eprintln!("aviso: não consegui habilitar o linger; sem ele o bot só roda enquanto você está logado.");
        eprintln!("       rode: sudo loginctl enable-linger {}", std::env::var("USER").unwrap_or_default());
    }
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "--now", ARQUIVO])?;

    println!("Serviço instalado e ativo: {}\n", caminho.display());
    println!("O bot agora sobe sozinho com o computador e responde de qualquer rede,");
    println!("enquanto o PC estiver ligado e com internet.\n");
    println!("  estado:    systemctl --user status {NOME}");
    println!("  logs:      journalctl --user -u {NOME} -f");
    println!("  desligar:  nasus bot --remover-servico");
    Ok(())
}

pub fn sou_o_servico() -> bool {
    std::env::var(VAR_SERVICO).is_ok_and(|v| v == "1")
}

/// ativo diz se o serviço está rodando agora.
pub fn ativo() -> bool {
    cfg!(target_os = "linux")
        && Command::new("systemctl")
            .args(["--user", "is-active", "--quiet", ARQUIVO])
            .status()
            .is_ok_and(|s| s.success())
}

/// reiniciar faz o serviço recarregar a configuração nova.
pub fn reiniciar() -> Result<()> {
    systemctl(&["restart", ARQUIVO])
}

pub fn remover() -> Result<()> {
    if !cfg!(target_os = "linux") {
        bail!("serviço automático só no Linux (systemd)");
    }
    let _ = systemctl(&["disable", "--now", ARQUIVO]);
    match std::fs::remove_file(caminho_unit()?) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    let _ = systemctl(&["daemon-reload"]);
    println!("Serviço do bot removido. (O `nasus bot` no terminal continua funcionando.)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_do_servico() {
        let u = unit("/usr/local/bin/nasus", None);
        assert!(u.contains("ExecStart=/usr/local/bin/nasus bot\n"));
        assert!(u.contains("Environment=NASUS_BOT_SERVICE=1\n"));
        assert!(!u.contains("NASUS_DB"));
        let u = unit("/x/nasus", Some("/dados/nasus.db"));
        assert!(u.contains("Environment=NASUS_DB=/dados/nasus.db\nExecStart=/x/nasus bot"));
    }
}
