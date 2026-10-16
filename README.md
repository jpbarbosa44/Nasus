# Nasus

**Curador das suas leituras.** App de terminal (TUI), app desktop e bot de Telegram para acompanhar os livros que você lê: fila, progresso, planos de leitura, notas, citações e meta anual.

```
          _________________ _________________
         /                 V                 \    ███╗   ██╗ █████╗ ███████╗██╗   ██╗███████╗
        |  ════════════    |  ════════════    |   ████╗  ██║██╔══██╗██╔════╝██║   ██║██╔════╝
        |  ══════════════  |  ═══════════     |   ██╔██╗ ██║███████║███████╗██║   ██║███████╗
        |  ═══════════     |  ════════════    |   ██║╚██╗██║██╔══██║╚════██║██║   ██║╚════██║
        |  ══════════════  |  ══════════      |   ██║ ╚████║██║  ██║███████║╚██████╔╝███████║
        |  ════════════    |  ═════════════   |   ╚═╝  ╚═══╝╚═╝  ╚═╝╚══════╝ ╚═════╝ ╚══════╝
        |__________________|__________________|   — curador das suas leituras
                           ▐▌
                           ▝▘
```

## O que faz

- **Biblioteca** por status: estante (todo livro novo entra nela), quero ler (a fila), lendo, lidos e abandonados, com filtro e barra de progresso.
- **Categorias** livres por livro (romance, filosofia, clássico…), com filtro por categoria.
- **Progresso**: registre a página atual; o livro passa a "lendo" sozinho e, na última página, o Nasus oferece marcar como lido. Datas de início e fim são preenchidas automaticamente.
- **Plano de leitura**: informe o início e quantas páginas por dia (ex.: 800 páginas, 25/dia) e o Nasus calcula a data de fim, mostra a meta do dia e se você está à frente ou atrás.
- **Busca por ISBN ou título**: ISBNs brasileiros na [BrasilAPI](https://brasilapi.com.br) (base da CBL), complementados pela [Open Library](https://openlibrary.org), que também sugere as categorias. Ou mande a **foto do código de barras** da contracapa.
- **Notas, citações e avaliação** de 0 a 5 estrelas; notas e citações aceitam a página (`p.115 texto`).
- **Histórico de leitura**: cada página registrada conta no dia; as estatísticas mostram a sequência de dias lendo e um mapa de calor, e cada livro mostra quando termina no seu ritmo.
- **Estatísticas**: meta anual de livros, páginas lidas, nota média, livros por mês e categorias mais lidas.
- **Retrospectiva do ano**: livro mais longo, mais rápido, autor e categoria do ano, melhor mês, cinco estrelas e uma citação.
- **Séries e coleções** com volume; ao terminar um, o Nasus sugere o próximo.
- **Empréstimos**: com quem está cada livro e desde quando.
- **Seus dados**: backup diário automático, exportar em JSON ou CSV e importar do Goodreads.
- **App desktop** numa janela própria, com ícone no menu de aplicativos.
- **Interface web** no navegador, com as mesmas telas e atalhos do app.
- **Bot de Telegram** para usar pelo celular, com lembretes diários do plano.
- **Servidor MCP** para o Claude consultar a sua biblioteca.

## Instalação

Baixe o pacote da sua plataforma na página de [Releases](https://github.com/jpbarbosa44/Nasus/releases/latest):

| Plataforma | Arquivo |
|---|---|
| Linux (x86_64) | `nasus-linux-amd64.tar.gz` |
| macOS (Apple Silicon) | `nasus-mac-arm64.tar.gz` |
| macOS (Intel) | `nasus-mac-intel.tar.gz` |
| Windows (x86_64) | `nasus-windows-amd64.zip` |

```sh
tar xzf nasus-linux-amd64.tar.gz
mv nasus nasus-app ~/.local/bin/   # ou outra pasta no PATH
nasus instalar-atalho              # opcional: põe o Nasus no menu de aplicativos
```

O pacote traz dois programas: `nasus` (terminal, web, bot e MCP) e `nasus-app` (o app desktop). No macOS, se o sistema bloquear os binários baixados: `xattr -d com.apple.quarantine nasus nasus-app`.

Para atualizar depois: `nasus atualizar` baixa a última versão e confere o SHA256 antes de trocar o binário. O app e o bot avisam quando sai uma versão nova.

Ou compile a partir do código (Rust 1.88+):

```sh
cargo install --git https://github.com/jpbarbosa44/Nasus
cargo install --git https://github.com/jpbarbosa44/Nasus --features app --bin nasus-app   # app desktop
```

## Uso

```sh
nasus             # abre o app no terminal
nasus app         # abre o app desktop
nasus web         # abre a interface no navegador (nasus web --help)
nasus bot         # bot do Telegram (nasus bot --help)
nasus mcp         # servidor MCP para o Claude
nasus atualizar   # instala a última versão
```

Dentro do app, tecle `?` para ver todos os atalhos. Os dados ficam num SQLite em `~/.local/share/nasus/nasus.db` (Linux; no macOS/Windows, na pasta de dados do usuário). Use a variável `NASUS_DB` para escolher outro arquivo.

## Seus dados

O Nasus guarda sozinho uma cópia do banco por dia em `backups/`, na mesma pasta (as 7 mais recentes).

```sh
nasus exportar                       # nasus-AAAA-MM-DD.json: tudo, inclusive notas, planos e histórico
nasus exportar --saida livros.csv    # CSV com ";" para abrir em planilha
nasus importar arquivo.json          # traz de volta um JSON exportado
nasus importar goodreads_library_export.csv
nasus restaurar                      # lista as cópias e volta para uma delas
```

Do **Goodreads**: Meus livros › Importar e exportar › Exportar biblioteca. O Nasus traz o status de cada estante, a data de leitura, a nota, a resenha, as estantes como categorias e a série do título ("Duna (Crônicas de Duna, #1)"). Livros que já existem (mesmo ISBN, ou mesmo título e autor) são pulados, e uma cópia do banco é guardada antes de importar. A interface web também exporta (Como usar › Seus dados).

## Interface web

```sh
nasus web                    # sobe o servidor e abre o navegador
nasus web --porta 8080       # outra porta (padrão: 6278)
nasus web --sem-abrir        # só sobe o servidor
```

As mesmas telas do app de terminal no navegador: biblioteca com abas e categorias, detalhes com capa, plano e notas, estatísticas com mapa de calor e retrospectiva, e busca online (dá para enviar a foto do código de barras). Os atalhos são os mesmos (`a` adiciona, `e` edita, `p` registra a página, `1`-`7` trocam de tela, `?` ajuda).

O servidor escuta **somente em `127.0.0.1`**: nada fica acessível pela rede, e requisições de outros sites são recusadas. A página vai embutida no binário, sem arquivos extras. O app, a web e o bot podem ficar abertos ao mesmo tempo sobre o mesmo banco. Ctrl+C encerra.

## App desktop

```sh
nasus app                # abre a janela (ou chame nasus-app direto)
nasus instalar-atalho    # Nasus no menu de aplicativos, com ícone
nasus remover-atalho
```

A mesma interface web, numa janela própria em vez do navegador. O servidor sobe numa porta livre de `127.0.0.1` e fecha junto com a janela. Abrir o app de novo traz a janela que já está aberta para a frente, links externos abrem no navegador e o exportar salva na pasta Downloads.

O atalho fica em `~/.local/share/applications` no Linux, no menu Iniciar no Windows e em `~/Applications/Nasus.app` no macOS.

Requisitos: no Linux, o WebKitGTK (`libwebkit2gtk-4.1-0` no Debian/Ubuntu, `webkit2gtk4.1` no Fedora); no Windows, o WebView2, que já vem no Windows 10 e 11; no macOS 11 ou mais novo, nada além do sistema. Na primeira abertura no macOS, o Gatekeeper pode bloquear o app, que não é assinado: abra com o botão direito › Abrir, ou rode o `xattr` acima.

## Bot do Telegram

1. Crie um bot com o [@BotFather](https://t.me/BotFather) (`/newbot`) e copie o token.
2. `nasus bot --token SEU_TOKEN`, mande uma mensagem ao bot e ele responde com o seu chat id.
3. Pare com Ctrl+C e autorize o chat: `nasus bot --chat SEU_ID`.
4. Para deixar o bot sempre ligado (Linux/systemd): `nasus bot --instalar-servico`.

O bot só responde ao chat autorizado. Pelo celular:

| Mensagem | O que faz |
|---|---|
| `120` / `+25` | registra a página atual / soma páginas lidas |
| `/lendo`, `/estante`, `/fila`, `/livro #3` | consulta os livros (`/livro duna` procura pelo título) |
| `/notas #3` | todas as notas e citações do livro |
| `/buscar duna` | busca e põe na estante com um toque |
| 📷 foto do código de barras | lê o ISBN da contracapa e busca o livro |
| `/categoria #3 romance, filosofia` | define as categorias do livro (`/categorias` lista todas) |
| `/paginas #3 320` | corrige o total de páginas |
| `/remover #3`, `/remover nota 12` | remove um livro (pede confirmação) ou uma nota |
| `/plano 25` | plano de 25 páginas por dia a partir de hoje |
| `/nota texto`, `/citacao p.115 texto` | anota no livro atual (p.115 marca a página; sem ela, vale a atual) |
| `/meta 12` | define a meta de livros do ano |
| `/serie #3 Duna 1` | série e volume (`/serie Duna` lista os volumes) |
| `/emprestar #3 Maria`, `/devolvido #3` | anota empréstimos (`/emprestados` lista) |
| `/retrospectiva` | os destaques do ano |

Às 8h ele manda a meta de leitura do dia e às 21h lembra do que ainda falta, inclusive a sequência de dias lendo que está em risco. Em 31/12 chega a retrospectiva do ano. Mande `/ajuda` para a lista completa.

## Claude (MCP)

`nasus mcp` é um servidor [MCP](https://modelcontextprotocol.io): com ele, o Claude consulta a sua biblioteca ("o que eu anotei sobre memória em Proust?", "qual o próximo da fila?", "como foi meu ano de leitura?").

```sh
claude mcp add nasus -- nasus mcp             # Claude Code, só leitura
claude mcp add nasus -- nasus mcp --escrita   # também registra páginas, notas, status e empréstimos
```

No Claude Desktop, em `claude_desktop_config.json` (use o caminho completo do binário se ele não estiver no PATH do app):

```json
{ "mcpServers": { "nasus": { "command": "nasus", "args": ["mcp"] } } }
```

Ferramentas de leitura: `livros`, `livro`, `buscar_notas`, `estatisticas`, `retrospectiva`, `historico`, `categorias` e `emprestimos`. Com `--escrita`: `adicionar_livro`, `registrar_pagina`, `mudar_status`, `avaliar`, `adicionar_nota`, `definir_categorias`, `emprestar` e `devolver`, com uma cópia do banco antes da primeira gravação de cada sessão.

## Licença

[GPL-3.0](LICENSE)
