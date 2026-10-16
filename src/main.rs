//! Nasus: curador das suas leituras, no terminal.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEventKind};

use nasus::app::App;
use nasus::db::Db;
use nasus::{atualizar, bot, dados, desktop, mcp, ui, web};

const USO: &str = "uso: nasus              abre o app no terminal
     nasus app          abre o Nasus numa janela própria
     nasus web ...      abre a interface no navegador (nasus web --help)
     nasus bot ...      bot do Telegram (nasus bot --help)
     nasus exportar     grava a biblioteca em JSON ou CSV
     nasus importar     traz livros de um JSON do Nasus ou do Goodreads
     nasus restaurar    volta o banco para uma cópia de segurança
     nasus mcp          servidor MCP para o Claude consultar a biblioteca
     nasus instalar-atalho   põe o Nasus no menu de aplicativos
     nasus atualizar    instala a última versão
     nasus versao       mostra a versão";

fn main() -> Result<()> {
    atualizar::limpar_antigo();
    let caminho = Db::caminho_padrao()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => {}
        Some("bot") => return bot::rodar(&caminho, &args[1..]),
        Some("web" | "--web") => return web::rodar(&caminho, &args[1..]),
        Some("exportar" | "export") => return dados::exportar(&caminho, &args[1..]),
        Some("importar" | "import") => return dados::importar(&caminho, &args[1..]),
        Some("restaurar" | "restore") => return dados::restaurar(&caminho, &args[1..]),
        Some("mcp") => return mcp::rodar(&caminho, &args[1..]),
        Some("app" | "--app") => return desktop::abrir(&args[1..]),
        Some("instalar-atalho") => return desktop::instalar_atalho(),
        Some("remover-atalho") => return desktop::remover_atalho(),
        Some("atualizar" | "update") => return atualizar::atualizar(&caminho, &args[1..]),
        Some("versao" | "versão" | "--version" | "-V") => {
            atualizar::mostrar_versao();
            return Ok(());
        }
        Some("-h" | "--help" | "ajuda") => {
            println!("{USO}\n\n{}\n\n{}\n\n{}\n\n{}\n\n{}", desktop::USO, web::USO, bot::USO, dados::USO, mcp::USO);
            return Ok(());
        }
        Some(outro) => bail!("comando desconhecido: {outro}\n\n{USO}"),
    }

    let db = Db::abrir(&caminho).with_context(|| format!("abrindo {}", caminho.display()))?;
    // a consulta ao GitHub não segura a abertura: o aviso vale da próxima vez
    let para_verificar = caminho.clone();
    std::thread::spawn(move || atualizar::verificar(&para_verificar));
    let mut app = App::new(db, caminho.display().to_string())?;

    // ratatui::init também instala um hook que restaura o terminal num panic
    let mut terminal = ratatui::init();
    let r = rodar(&mut terminal, &mut app);
    ratatui::restore();
    r
}

fn rodar(terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
    while !app.sair {
        terminal.draw(|f| ui::desenhar(f, app))?;
        // espera curta para recolher a resposta da busca online sem travar
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(k) = event::read()?
            && k.kind == KeyEventKind::Press
        {
            app.tecla(k);
        }
        app.verificar_busca();
    }
    Ok(())
}
