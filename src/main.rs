//! Nasus: curador das suas leituras, no terminal.

mod app;
mod bot;
mod codigo_barras;
mod dados;
mod db;
mod input;
mod mcp;
mod model;
mod openlibrary;
mod ui;
mod web;

use std::time::Duration;

use anyhow::{Context, Result, bail};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEventKind};

use app::App;
use db::Db;

const USO: &str = "uso: nasus              abre o app no terminal
     nasus web ...      abre a interface no navegador (nasus web --help)
     nasus bot ...      bot do Telegram (nasus bot --help)
     nasus exportar     grava a biblioteca em JSON ou CSV
     nasus importar     traz livros de um JSON do Nasus ou do Goodreads
     nasus restaurar    volta o banco para uma cópia de segurança
     nasus mcp          servidor MCP para o Claude consultar a biblioteca";

fn main() -> Result<()> {
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
        Some("-h" | "--help" | "ajuda") => {
            println!("{USO}\n\n{}\n\n{}\n\n{}\n\n{}", web::USO, bot::USO, dados::USO, mcp::USO);
            return Ok(());
        }
        Some(outro) => bail!("comando desconhecido: {outro}\n\n{USO}"),
    }

    let db = Db::abrir(&caminho).with_context(|| format!("abrindo {}", caminho.display()))?;
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
