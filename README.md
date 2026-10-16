# Nasus

**Curador das suas leituras.** App de terminal (TUI) e bot de Telegram para acompanhar os livros que você lê: fila, progresso, planos de leitura, notas, citações e meta anual.

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
- **Busca por ISBN ou título**: ISBNs brasileiros na [BrasilAPI](https://brasilapi.com.br) (base da CBL), complementados pela [Open Library](https://openlibrary.org).
- **Notas, citações e avaliação** de 0 a 5 estrelas.
- **Estatísticas**: meta anual de livros, páginas lidas, nota média e livros por mês.
- **Bot de Telegram** para usar pelo celular, com lembretes diários do plano.

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
mv nasus ~/.local/bin/   # ou outra pasta no PATH
```

No macOS, se o sistema bloquear o binário baixado: `xattr -d com.apple.quarantine nasus`.

Ou compile a partir do código (Rust 1.88+):

```sh
cargo install --git https://github.com/jpbarbosa44/Nasus
```

## Uso

```sh
nasus          # abre o app no terminal
nasus bot      # bot do Telegram (nasus bot --help)
```

Dentro do app, tecle `?` para ver todos os atalhos. Os dados ficam num SQLite em `~/.local/share/nasus/nasus.db` (Linux; no macOS/Windows, na pasta de dados do usuário). Use a variável `NASUS_DB` para escolher outro arquivo.

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

Às 8h ele manda a meta de leitura do dia e às 21h lembra do que ainda falta. Mande `/ajuda` para a lista completa.

## Licença

[GPL-3.0](LICENSE)
