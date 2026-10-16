//! nasus-app: o Nasus numa janela própria. Sobe a interface web numa porta
//! livre de 127.0.0.1 e a mostra num WebView do sistema (WebKitGTK no Linux,
//! WebView2 no Windows, WKWebView no macOS). Fecha junto com a janela.

// no Windows, sem janela de console por trás
#![windows_subsystem = "windows"]

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy};
use tao::window::{Icon, Window, WindowBuilder};
use wry::{NewWindowResponse, WebView, WebViewBuilder};

use nasus::db::Db;
use nasus::{desktop, openlibrary, web};

enum Evento {
    /// outra cópia do app foi aberta: trazer esta janela para a frente
    Focar,
    /// um arquivo exportado terminou de baixar (ou falhou)
    Baixado(Option<PathBuf>),
}

fn main() {
    #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
    let mut construtor = EventLoopBuilder::<Evento>::with_user_event();
    #[cfg(target_os = "linux")]
    {
        use tao::platform::unix::EventLoopBuilderExtUnix;
        construtor.with_app_id(desktop::APP_ID);
    }
    let event_loop = construtor.build();
    let proxy = event_loop.create_proxy();
    let pagina = match preparar(proxy.clone()) {
        Ok(Some(url)) => Pagina::Url(url),
        // já havia uma janela aberta, e ela veio para a frente
        Ok(None) => return,
        Err(e) => Pagina::Erro(format!("{e:#}")),
    };
    if let Err(e) = abrir_janela(event_loop, proxy, pagina) {
        eprintln!("nasus-app: {e:#}");
        std::process::exit(1);
    }
}

enum Pagina {
    Url(String),
    Erro(String),
}

/// trava guarda a porta da janela aberta, para a próxima cópia achá-la.
fn trava(caminho_db: &Path) -> PathBuf {
    caminho_db.with_file_name("app.lock")
}

/// preparar sobe o servidor e devolve a URL da página; None quando outra
/// janela já estava aberta (e recebeu o pedido para aparecer).
fn preparar(proxy: EventLoopProxy<Evento>) -> Result<Option<String>> {
    let caminho = Db::caminho_padrao()?;
    if let Ok(porta) = std::fs::read_to_string(trava(&caminho))
        && focar_outra(porta.trim())
    {
        return Ok(None);
    }
    let db = Db::abrir(&caminho).with_context(|| format!("abrindo {}", caminho.display()))?;
    let (servidor, porta) = web::iniciar(0)?;
    let api = web::Api::new(db, openlibrary::buscar).ao_focar(move || {
        let _ = proxy.send_event(Evento::Focar);
    });
    std::thread::spawn(move || web::servir(&servidor, api));
    let _ = std::fs::write(trava(&caminho), porta.to_string());
    Ok(Some(format!("http://127.0.0.1:{porta}/")))
}

/// focar_outra pede à janela que já está aberta que venha para a frente.
fn focar_outra(porta: &str) -> bool {
    let Ok(porta) = porta.parse::<u16>() else { return false };
    ureq::post(&format!("http://127.0.0.1:{porta}/api/focar"))
        .timeout(Duration::from_secs(2))
        .call()
        .is_ok()
}

fn icone() -> Option<Icon> {
    let img = image::load_from_memory(desktop::ICONE_PNG).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).ok()
}

/// abrir_externo manda os links de fora (capas, Open Library) para o navegador.
fn abrir_externo(url: &str) {
    let r = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else if cfg!(windows) {
        std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if let Err(e) = r {
        eprintln!("nasus-app: abrindo {url}: {e}");
    }
}

fn pagina_de_erro(erro: &str) -> String {
    let erro = erro.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        "<!doctype html><meta charset=utf-8><body style=\"background:#0e1014;color:#d8dee4;font:15px monospace;padding:32px\">\
         <h2 style=\"color:#ee6a6a\">O Nasus não conseguiu abrir</h2><pre style=\"white-space:pre-wrap\">{erro}</pre>\
         <p style=\"color:#6c7380\">O app de terminal (nasus) mostra o mesmo erro com mais detalhes.</p></body>"
    )
}

fn abrir_janela(event_loop: EventLoop<Evento>, proxy: EventLoopProxy<Evento>, pagina: Pagina) -> Result<()> {
    let janela = WindowBuilder::new()
        .with_title("Nasus")
        .with_inner_size(LogicalSize::new(1200.0, 800.0))
        .with_min_inner_size(LogicalSize::new(420.0, 480.0))
        .with_window_icon(icone())
        .build(&event_loop)
        .context("criando a janela")?;

    let construtor = match &pagina {
        Pagina::Url(url) => {
            let origem = url.clone();
            WebViewBuilder::new()
                .with_url(url)
                // só a própria página navega na janela; o resto vai para o navegador
                .with_navigation_handler(move |u| {
                    let dentro = u.starts_with(&origem) || u.starts_with("about:") || u.starts_with("data:");
                    if !dentro {
                        abrir_externo(&u);
                    }
                    dentro
                })
                .with_new_window_req_handler(|u, _| {
                    abrir_externo(&u);
                    NewWindowResponse::Deny
                })
                // o destino já vem como Downloads/<nome sugerido>
                .with_download_started_handler(|_, _| true)
                .with_download_completed_handler(move |_, caminho, ok| {
                    let _ = proxy.send_event(Evento::Baixado(caminho.filter(|_| ok)));
                })
        }
        Pagina::Erro(e) => WebViewBuilder::new().with_html(pagina_de_erro(e)),
    };
    let webview = construir(construtor, &janela)?;

    event_loop.run(move |evento, _, controle| {
        *controle = ControlFlow::Wait;
        match evento {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                if let Pagina::Url(_) = pagina
                    && let Ok(caminho) = Db::caminho_padrao()
                {
                    let _ = std::fs::remove_file(trava(&caminho));
                }
                *controle = ControlFlow::Exit;
            }
            Event::UserEvent(Evento::Focar) => {
                janela.set_minimized(false);
                janela.set_visible(true);
                janela.set_focus();
            }
            Event::UserEvent(Evento::Baixado(caminho)) => {
                let (texto, erro) = match caminho {
                    Some(c) => (format!("Salvo em {}", c.display()), false),
                    None => ("O download falhou.".to_string(), true),
                };
                let js = format!("avisar({}, {erro})", serde_json::to_string(&texto).unwrap_or_default());
                let _ = webview.evaluate_script(&js);
            }
            _ => {}
        }
    });
}

fn construir(construtor: WebViewBuilder, janela: &Window) -> Result<WebView> {
    #[cfg(target_os = "linux")]
    {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = janela.default_vbox().context("janela sem vbox do GTK")?;
        construtor.build_gtk(vbox).context("criando o WebView (o WebKitGTK está instalado?)")
    }
    #[cfg(not(target_os = "linux"))]
    {
        construtor.build(janela).context("criando o WebView")
    }
}
