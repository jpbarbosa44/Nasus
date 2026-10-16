//! Bot do Telegram: `nasus bot` registra o progresso e consulta as leituras
//! pelo celular, num loop de long polling, e manda os lembretes do plano.

mod comandos;
mod servico;
mod telegram;

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::{Datelike, Local, Timelike};
use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::openlibrary;
use comandos::{Nucleo, Resposta};
use telegram::{Callback, Mensagem, Telegram};

/// a partir desta hora, manda a meta de leitura do dia
const HORA_MANHA: u32 = 8;
/// a partir desta hora, lembra das páginas que faltam hoje
const HORA_NOITE: u32 = 21;

pub const USO: &str = "uso: nasus bot [--token TOKEN] [--chat ID] [--instalar-servico | --remover-servico]

  --token TOKEN        token do bot (crie com o @BotFather no Telegram); fica salvo
  --chat ID            único chat autorizado a usar o bot; fica salvo
  --instalar-servico   mantém o bot sempre rodando via systemd
  --remover-servico    remove o serviço systemd do bot";

/// Configuração do bot, num JSON ao lado do banco.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Config {
    token: String,
    chat_id: i64,
    /// datas (AAAA-MM-DD) do último lembrete enviado, para não repetir no dia
    #[serde(default, skip_serializing_if = "String::is_empty")]
    ultimo_lembrete: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    ultimo_resumo: String,
    /// última versão nova já avisada, para não repetir
    #[serde(default, skip_serializing_if = "String::is_empty")]
    versao_avisada: String,
    /// ano da última retrospectiva enviada (em 31/12)
    #[serde(default, skip_serializing_if = "is_zero")]
    ultima_retrospectiva: i32,
}

fn is_zero(n: &i32) -> bool {
    *n == 0
}

impl Config {
    fn caminho(caminho_db: &Path) -> PathBuf {
        caminho_db.with_file_name("telegram.json")
    }

    fn carregar(caminho: &Path) -> Result<Self> {
        match std::fs::read_to_string(caminho) {
            Ok(s) => serde_json::from_str(&s).with_context(|| format!("lendo {}", caminho.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e).with_context(|| format!("lendo {}", caminho.display())),
        }
    }

    fn salvar(&self, caminho: &Path) -> Result<()> {
        let dados = serde_json::to_string_pretty(self)?;
        // 0600: o token dá controle total do bot, não pode vazar para outros usuários
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(caminho)?;
            f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            f.write_all(dados.as_bytes())?;
        }
        #[cfg(not(unix))]
        std::fs::write(caminho, dados)?;
        Ok(())
    }
}

#[derive(Debug, Default, PartialEq)]
struct Opcoes {
    token: Option<String>,
    chat: Option<i64>,
    instalar: bool,
    remover: bool,
}

fn ler_opcoes(args: &[String]) -> Result<Opcoes> {
    let mut o = Opcoes::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let (nome, valor) = match a.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (a.as_str(), None),
        };
        let mut valor = || valor.clone().or_else(|| it.next().cloned()).with_context(|| format!("{nome} precisa de um valor"));
        match nome {
            "--token" => o.token = Some(valor()?),
            "--chat" => o.chat = Some(valor()?.parse().context("--chat precisa ser o número do chat")?),
            "--instalar-servico" => o.instalar = true,
            "--remover-servico" => o.remover = true,
            "-h" | "--help" => bail!("{USO}"),
            _ => bail!("opção desconhecida: {a}\n\n{USO}"),
        }
    }
    Ok(o)
}

/// rodar trata `nasus bot ...`: salva token/chat e entra no loop de long
/// polling até ser interrompido (Ctrl+C).
pub fn rodar(caminho_db: &Path, args: &[String]) -> Result<()> {
    let op = ler_opcoes(args)?;
    if op.remover {
        return servico::remover();
    }
    let caminho_cfg = Config::caminho(caminho_db);
    let mut cfg = Config::carregar(&caminho_cfg)?;
    let mudou = op.token.is_some() || op.chat.is_some();
    if let Some(t) = op.token {
        cfg.token = t.trim().to_string();
    }
    if let Some(c) = op.chat {
        cfg.chat_id = c;
    }
    if mudou {
        cfg.salvar(&caminho_cfg).context("salvando a configuração do bot")?;
    }
    if cfg.token.is_empty() {
        bail!("bot sem token: crie um bot com o @BotFather no Telegram e rode `nasus bot --token SEU_TOKEN`");
    }
    if op.instalar {
        servico::instalar()?;
        if cfg.chat_id == 0 {
            println!("\nFalta parear o chat: mande uma mensagem ao bot e rode `nasus bot --chat SEU_ID`.");
        }
        return Ok(());
    }

    // O Telegram só admite um getUpdates por bot. Se o serviço já está no ar,
    // quem roda no terminal só grava a configuração e reinicia o serviço.
    if !servico::sou_o_servico() && servico::ativo() {
        if mudou {
            servico::reiniciar()?;
            println!("Configuração salva e serviço {} reiniciado.", servico::NOME);
            return Ok(());
        }
        bail!(
            "o serviço {0} já está rodando (um bot só admite um poller).\n\
             veja com `systemctl --user status {0}`; pare com `nasus bot --remover-servico` para rodar no terminal",
            servico::NOME
        );
    }

    let tg = Telegram::new(&cfg.token);
    let eu = tg.get_me().context("conectando ao Telegram")?;
    println!("Bot @{} conectado.", eu.username.unwrap_or_default());
    if cfg.chat_id == 0 {
        println!("Nenhum chat autorizado ainda: mande qualquer mensagem ao bot para descobrir seu chat id,");
        println!("depois rode `nasus bot --chat SEU_CHAT_ID`. Até lá, nada é registrado.");
    }

    let mut nucleo = Nucleo::new(Db::abrir(caminho_db)?, openlibrary::buscar);
    let mut offset = 0;
    loop {
        match tg.get_updates(offset) {
            Ok(ups) => {
                for up in ups {
                    offset = up.update_id + 1;
                    if let Some(m) = up.message {
                        tratar_mensagem(&tg, &cfg, &mut nucleo, m);
                    } else if let Some(cb) = up.callback_query {
                        tratar_callback(&tg, &cfg, &mut nucleo, cb);
                    }
                }
            }
            Err(e) => {
                eprintln!("aviso: {e:#} (tentando de novo em 5s)");
                thread::sleep(Duration::from_secs(5));
            }
        }
        verificar_agenda(&tg, &mut cfg, &caminho_cfg, &nucleo);
    }
}

fn enviar(tg: &Telegram, chat_id: i64, r: &Resposta) {
    if let Err(e) = tg.enviar(chat_id, r) {
        eprintln!("aviso: enviando mensagem: {e:#}");
    }
}

fn tratar_mensagem(tg: &Telegram, cfg: &Config, nucleo: &mut Nucleo, m: Mensagem) {
    let chat = m.chat.id;
    if cfg.chat_id == 0 {
        println!("Mensagem do chat {chat} (ainda não autorizado).");
        let texto = format!(
            "Este bot ainda não está pareado. Seu chat id é {chat}.\nSe este Nasus é seu, rode: nasus bot --chat {chat}"
        );
        enviar(tg, chat, &Resposta { foto: None, texto, botoes: Vec::new() });
        return;
    }
    if chat != cfg.chat_id {
        println!("Ignorando mensagem do chat {chat} (autorizado: {}).", cfg.chat_id);
        return;
    }
    let hoje = Local::now().date_naive();
    let r = if let Some(id) = m.imagem() {
        match tg.baixar(id) {
            Ok(bytes) => nucleo.foto(&bytes),
            Err(e) => {
                eprintln!("aviso: baixando foto: {e:#}");
                Resposta { foto: None, texto: "❌ não consegui baixar a foto; tente de novo".into(), botoes: Vec::new() }
            }
        }
    } else if let Some(texto) = m.text {
        nucleo.mensagem(&texto, hoje)
    } else {
        return;
    };
    enviar(tg, chat, &r);
}

fn tratar_callback(tg: &Telegram, cfg: &Config, nucleo: &mut Nucleo, cb: Callback) {
    let Some(msg) = cb.message else { return };
    if cfg.chat_id == 0 || msg.chat.id != cfg.chat_id {
        let _ = tg.responder_callback(&cb.id, "não autorizado");
        return;
    }
    let dados = cb.data.unwrap_or_default();
    let r = nucleo.callback(&dados, Local::now().date_naive());
    if let Err(e) = tg.responder_callback(&cb.id, "") {
        eprintln!("aviso: respondendo callback: {e:#}");
    }
    if Nucleo::fecha_teclado(&dados)
        && let Err(e) = tg.tirar_botoes(msg.chat.id, msg.message_id)
    {
        eprintln!("aviso: tirando botões: {e:#}");
    }
    enviar(tg, msg.chat.id, &r);
}

/// verificar_agenda manda os lembretes do dia uma vez, depois da hora marcada.
fn verificar_agenda(tg: &Telegram, cfg: &mut Config, caminho_cfg: &Path, nucleo: &Nucleo) {
    if cfg.chat_id == 0 {
        return;
    }
    let agora = Local::now();
    let hoje = agora.date_naive();
    let chave = hoje.to_string();
    let mut mudou = false;
    // o bot fica dias com o banco aberto: a cópia do dia sai daqui
    if let Err(e) = nucleo.backup_diario(hoje) {
        eprintln!("aviso: backup diário: {e:#}");
    }
    // o cache de versão fica na mesma pasta do telegram.json (a do banco)
    crate::atualizar::verificar(caminho_cfg);
    if let Some(v) = crate::atualizar::nova_versao(caminho_cfg)
        && cfg.versao_avisada != v
    {
        let texto = format!("✦ Saiu o Nasus {v}. No computador: nasus atualizar e depois systemctl --user restart nasus-bot");
        enviar(tg, cfg.chat_id, &Resposta { texto, botoes: Vec::new(), foto: None });
        cfg.versao_avisada = v;
        mudou = true;
    }
    if agora.hour() >= HORA_MANHA && cfg.ultimo_lembrete != chave {
        match nucleo.lembrete_manha(hoje) {
            Ok(Some(r)) => enviar(tg, cfg.chat_id, &r),
            Ok(None) => {}
            Err(e) => eprintln!("aviso: lembrete da manhã: {e:#}"),
        }
        cfg.ultimo_lembrete = chave.clone();
        mudou = true;
    }
    if agora.hour() >= HORA_NOITE && cfg.ultimo_resumo != chave {
        match nucleo.lembrete_noite(hoje) {
            Ok(Some(r)) => enviar(tg, cfg.chat_id, &r),
            Ok(None) => {}
            Err(e) => eprintln!("aviso: lembrete da noite: {e:#}"),
        }
        cfg.ultimo_resumo = chave;
        mudou = true;
    }
    // no último dia do ano, junto com o lembrete da noite, vai a retrospectiva
    let ultimo_dia = hoje.month() == 12 && hoje.day() == 31;
    if agora.hour() >= HORA_NOITE && ultimo_dia && cfg.ultima_retrospectiva != hoje.year() {
        match nucleo.retrospectiva(hoje.year()) {
            Ok(r) => enviar(tg, cfg.chat_id, &r),
            Err(e) => eprintln!("aviso: retrospectiva: {e:#}"),
        }
        cfg.ultima_retrospectiva = hoje.year();
        mudou = true;
    }
    if mudou && let Err(e) = cfg.salvar(caminho_cfg) {
        eprintln!("aviso: salvando config: {e:#}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn le_opcoes() {
        assert_eq!(ler_opcoes(&[]).unwrap(), Opcoes::default());
        let o = ler_opcoes(&args("--token abc:123 --chat=42 --instalar-servico")).unwrap();
        assert_eq!(o, Opcoes { token: Some("abc:123".into()), chat: Some(42), instalar: true, remover: false });
        assert!(ler_opcoes(&args("--chat")).is_err());
        assert!(ler_opcoes(&args("--chat x")).is_err());
        assert!(ler_opcoes(&args("--outra")).is_err());
    }

    #[test]
    fn config_fica_ao_lado_do_banco_e_privada() {
        let dir = std::env::temp_dir().join(format!("nasus-teste-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let caminho = Config::caminho(&dir.join("nasus.db"));
        assert_eq!(caminho, dir.join("telegram.json"));
        assert_eq!(Config::carregar(&caminho).unwrap().token, "", "sem arquivo, config vazia");

        let cfg = Config { token: "t".into(), chat_id: 7, ..Default::default() };
        cfg.salvar(&caminho).unwrap();
        let lida = Config::carregar(&caminho).unwrap();
        assert_eq!((lida.token.as_str(), lida.chat_id), ("t", 7));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&caminho).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
