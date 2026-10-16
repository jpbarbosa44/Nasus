//! Integração com a área de trabalho: `nasus app` abre a janela (o binário
//! `nasus-app`, ao lado deste) e `nasus instalar-atalho` põe o Nasus no menu
//! de aplicativos do sistema, com ícone.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

pub const ICONE_PNG: &[u8] = include_bytes!("../packaging/nasus.png");
pub const ICONE_SVG: &str = include_str!("../packaging/nasus.svg");
#[cfg(any(windows, test))]
const ICONE_ICO: &[u8] = include_bytes!("../packaging/nasus.ico");
#[cfg(any(target_os = "macos", test))]
const ICONE_ICNS: &[u8] = include_bytes!("../packaging/nasus.icns");

pub const USO: &str = "uso: nasus app               abre o Nasus numa janela própria (precisa do nasus-app)
     nasus instalar-atalho     põe o Nasus no menu de aplicativos, com ícone
     nasus remover-atalho      tira o atalho do menu";

/// id do app: nome do .desktop no Linux e app_id da janela (o Wayland liga
/// a janela ao atalho, e ao ícone, por ele)
pub const APP_ID: &str = "io.github.jpbarbosa44.Nasus";

const NOME_APP: &str = if cfg!(windows) { "nasus-app.exe" } else { "nasus-app" };

/// binario_app acha o `nasus-app` instalado ao lado deste executável.
pub fn binario_app() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe = exe.canonicalize().unwrap_or(exe);
    Some(exe.with_file_name(NOME_APP)).filter(|p| p.exists())
}

fn este_binario() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("não achei o executável em uso")?;
    Ok(exe.canonicalize().unwrap_or(exe))
}

const SEM_APP: &str = "o nasus-app não está instalado ao lado do nasus.
Ele vem nos pacotes do release (https://github.com/jpbarbosa44/Nasus/releases/latest) ou sai com
  cargo install --git https://github.com/jpbarbosa44/Nasus --features app --bin nasus-app
(no Linux, a compilação precisa de libwebkit2gtk-4.1-dev). Enquanto isso, nasus web abre no navegador.";

/// abrir trata `nasus app`: lança a janela e volta para o terminal.
pub fn abrir(args: &[String]) -> Result<()> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        bail!("{USO}");
    }
    let app = binario_app().context(SEM_APP)?;
    Command::new(&app).args(args).spawn().with_context(|| format!("abrindo {}", app.display()))?;
    Ok(())
}

/// pasta de dados do usuário (~/.local/share no Linux)
fn dados_usuario() -> Result<PathBuf> {
    directories::BaseDirs::new().map(|d| d.data_dir().to_path_buf()).context("não achei a pasta de dados do usuário")
}

fn gravar(caminho: &Path, conteudo: &[u8]) -> Result<()> {
    if let Some(dir) = caminho.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("criando {}", dir.display()))?;
    }
    std::fs::write(caminho, conteudo).with_context(|| format!("gravando {}", caminho.display()))
}

/// entrada_desktop monta o arquivo .desktop (padrão freedesktop) de um atalho.
fn entrada_desktop(nome: &str, comentario: &str, exec: &Path, terminal: bool) -> String {
    // a janela do app se identifica pelo APP_ID; a do terminal é do emulador
    let classe = if terminal { String::new() } else { format!("StartupWMClass={APP_ID}\n") };
    // o Exec do freedesktop usa aspas duplas, com \ e " escapados
    let exec = exec.display().to_string().replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "[Desktop Entry]
Type=Application
Name={nome}
GenericName=Curador de leituras
Comment={comentario}
Exec=\"{exec}\"
Icon=nasus
Terminal={terminal}
Categories=Office;
Keywords=livros;leitura;books;reading;
{classe}"
    )
}

/// instalar_atalho trata `nasus instalar-atalho`.
pub fn instalar_atalho() -> Result<()> {
    let nasus = este_binario()?;
    let app = binario_app();
    if cfg!(target_os = "macos") {
        let app = app.context(SEM_APP)?;
        return instalar_mac(&app);
    }
    if cfg!(windows) {
        let app = app.context(SEM_APP)?;
        return instalar_windows(&app);
    }
    let dados = dados_usuario()?;
    let aplicativos = dados.join("applications");
    let icones = dados.join("icons/hicolor");
    gravar(&icones.join("scalable/apps/nasus.svg"), ICONE_SVG.as_bytes())?;
    gravar(&icones.join("256x256/apps/nasus.png"), ICONE_PNG)?;
    let terminal = entrada_desktop("Nasus (terminal)", "O Nasus no terminal", &nasus, true);
    gravar(&aplicativos.join("nasus-terminal.desktop"), terminal.as_bytes())?;
    match &app {
        Some(app) => {
            let janela = entrada_desktop("Nasus", "Seus livros, planos de leitura e notas", app, false);
            gravar(&aplicativos.join(format!("{APP_ID}.desktop")), janela.as_bytes())?;
        }
        None => println!("aviso: {SEM_APP}\nSó o atalho do terminal foi criado.\n"),
    }
    // atualizam o menu na hora; se não existirem, o menu se atualiza sozinho depois
    let _ = Command::new("update-desktop-database").arg(&aplicativos).status();
    let _ = Command::new("gtk-update-icon-cache").args(["-f", "-t"]).arg(&icones).status();
    println!("✔ Nasus no menu de aplicativos ({}).", aplicativos.display());
    Ok(())
}

/// remover_atalho trata `nasus remover-atalho`.
pub fn remover_atalho() -> Result<()> {
    let mut removidos = Vec::new();
    let mut remover = |p: PathBuf| {
        let ok = if p.is_dir() { std::fs::remove_dir_all(&p).is_ok() } else { std::fs::remove_file(&p).is_ok() };
        if ok {
            removidos.push(p);
        }
    };
    if cfg!(target_os = "macos") {
        if let Some(casa) = directories::BaseDirs::new() {
            remover(casa.home_dir().join("Applications/Nasus.app"));
        }
    } else if cfg!(windows) {
        remover(menu_iniciar()?.join("Nasus.lnk"));
        remover(dados_usuario()?.join("nasus").join("nasus.ico"));
    } else {
        let dados = dados_usuario()?;
        remover(dados.join(format!("applications/{APP_ID}.desktop")));
        remover(dados.join("applications/nasus-terminal.desktop"));
        remover(dados.join("icons/hicolor/scalable/apps/nasus.svg"));
        remover(dados.join("icons/hicolor/256x256/apps/nasus.png"));
        let _ = Command::new("update-desktop-database").arg(dados.join("applications")).status();
    }
    if removidos.is_empty() {
        println!("Nenhum atalho do Nasus para remover.");
    }
    for p in removidos {
        println!("removido: {}", p.display());
    }
    Ok(())
}

fn menu_iniciar() -> Result<PathBuf> {
    // %APPDATA%\Microsoft\Windows\Start Menu\Programs
    let roaming = directories::BaseDirs::new().context("não achei o AppData")?.config_dir().to_path_buf();
    Ok(roaming.join("Microsoft").join("Windows").join("Start Menu").join("Programs"))
}

/// instalar_windows cria Nasus.lnk no menu Iniciar (via PowerShell, que já vem no Windows).
fn instalar_windows(app: &Path) -> Result<()> {
    let icone = dados_usuario()?.join("nasus").join("nasus.ico");
    #[cfg(windows)]
    gravar(&icone, ICONE_ICO)?;
    let atalho = menu_iniciar()?.join("Nasus.lnk");
    // aspas simples no PowerShell: só a própria aspa precisa ser dobrada
    let q = |p: &Path| p.display().to_string().replace('\'', "''");
    let script = format!(
        "$s = (New-Object -ComObject WScript.Shell).CreateShortcut('{}'); $s.TargetPath = '{}'; \
         $s.WorkingDirectory = '{}'; $s.IconLocation = '{}'; $s.Description = 'Nasus: curador das suas leituras'; $s.Save()",
        q(&atalho),
        q(app),
        q(app.parent().unwrap_or(Path::new("."))),
        q(&icone)
    );
    let ok = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status()
        .context("rodando o PowerShell")?
        .success();
    if !ok {
        bail!("o PowerShell não conseguiu criar o atalho");
    }
    println!("✔ Nasus no menu Iniciar ({}).", atalho.display());
    Ok(())
}

/// info_plist descreve o pacote Nasus.app do macOS.
fn info_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Nasus</string>
  <key>CFBundleDisplayName</key><string>Nasus</string>
  <key>CFBundleIdentifier</key><string>io.github.jpbarbosa44.nasus</string>
  <key>CFBundleVersion</key><string>{v}</string>
  <key>CFBundleShortVersionString</key><string>{v}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>Nasus</string>
  <key>CFBundleIconFile</key><string>nasus</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#,
        v = env!("CARGO_PKG_VERSION")
    )
}

/// instalar_mac monta ~/Applications/Nasus.app, que chama o nasus-app instalado.
fn instalar_mac(app: &Path) -> Result<()> {
    let casa = directories::BaseDirs::new().context("não achei a pasta do usuário")?.home_dir().to_path_buf();
    let pacote = casa.join("Applications/Nasus.app/Contents");
    gravar(&pacote.join("Info.plist"), info_plist().as_bytes())?;
    #[cfg(target_os = "macos")]
    gravar(&pacote.join("Resources/nasus.icns"), ICONE_ICNS)?;
    let lancador = pacote.join("MacOS/Nasus");
    let aspas = app.display().to_string().replace('\'', "'\\''");
    gravar(&lancador, format!("#!/bin/sh\nexec '{aspas}' \"$@\"\n").as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&lancador, std::fs::Permissions::from_mode(0o755))?;
    }
    println!("✔ Nasus.app em {} (abra pelo Launchpad ou pelo Spotlight).", casa.join("Applications").display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry() {
        let e = entrada_desktop("Nasus", "x", Path::new("/opt/meu \"app\"/nasus-app"), false);
        assert!(e.contains("Exec=\"/opt/meu \\\"app\\\"/nasus-app\"\n"), "{e}");
        assert!(e.contains("Terminal=false") && e.contains("Icon=nasus"));
        assert!(e.contains(&format!("StartupWMClass={APP_ID}\n")));
        assert!(!entrada_desktop("Nasus", "x", Path::new("/n"), true).contains("StartupWMClass"));
    }

    #[test]
    fn icones_embutidos() {
        assert!(ICONE_PNG.starts_with(b"\x89PNG"));
        assert!(ICONE_ICO.starts_with(&[0, 0, 1, 0]));
        assert!(ICONE_ICNS.starts_with(b"icns"));
        assert!(ICONE_SVG.contains("<svg"));
        assert!(info_plist().contains(&format!("<string>{}</string>", env!("CARGO_PKG_VERSION"))));
    }
}
