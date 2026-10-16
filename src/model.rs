//! Tipos do domínio: livros, status de leitura e notas.

use chrono::NaiveDate;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// livro que você tem, sem planos de ler ainda
    Estante,
    QueroLer,
    Lendo,
    Lido,
    Abandonado,
}

impl Status {
    /// Ordem usada nas abas e no seletor de status.
    pub const TODOS: [Status; 5] = [Status::Estante, Status::Lendo, Status::QueroLer, Status::Lido, Status::Abandonado];

    /// chave é o valor gravado no banco.
    pub fn chave(self) -> &'static str {
        match self {
            Status::Estante => "shelf",
            Status::QueroLer => "want",
            Status::Lendo => "reading",
            Status::Lido => "read",
            Status::Abandonado => "abandoned",
        }
    }

    pub fn de_chave(s: &str) -> Option<Self> {
        Self::TODOS.into_iter().find(|st| st.chave() == s)
    }

    /// nome é o status de um livro ("Lido"); rotulo é o da aba ("Lidos").
    pub fn nome(self) -> &'static str {
        match self {
            Status::Estante => "Na estante",
            Status::QueroLer => "Quero ler",
            Status::Lendo => "Lendo",
            Status::Lido => "Lido",
            Status::Abandonado => "Abandonado",
        }
    }

    pub fn rotulo(self) -> &'static str {
        match self {
            Status::Estante => "Estante",
            Status::Lido => "Lidos",
            Status::Abandonado => "Abandonados",
            s => s.nome(),
        }
    }

    pub fn simbolo(self) -> &'static str {
        match self {
            Status::Estante => "□",
            Status::QueroLer => "○",
            Status::Lendo => "▶",
            Status::Lido => "✔",
            Status::Abandonado => "✖",
        }
    }

    fn indice(self) -> usize {
        Self::TODOS.iter().position(|s| *s == self).unwrap_or(0)
    }

    pub fn proximo(self) -> Self {
        Self::TODOS[(self.indice() + 1) % Self::TODOS.len()]
    }

    pub fn anterior(self) -> Self {
        Self::TODOS[(self.indice() + Self::TODOS.len() - 1) % Self::TODOS.len()]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Book {
    pub id: i64,
    pub title: String,
    pub author: String,
    pub isbn: String,
    /// total de páginas; 0 quando desconhecido
    pub pages: i64,
    pub status: Status,
    pub current_page: i64,
    /// 0 = sem nota; 1 a 5 estrelas
    pub rating: u8,
    pub review: String,
    pub started_at: Option<NaiveDate>,
    pub finished_at: Option<NaiveDate>,
    /// em minúsculas, ordem alfabética
    pub categorias: Vec<String>,
    /// com quem está e desde quando, se estiver emprestado
    pub emprestado: Option<(String, NaiveDate)>,
    /// série ou coleção ("Duna"); vazio quando o livro é avulso
    pub serie: String,
    /// número do volume na série; 0 quando não se sabe
    pub volume: i64,
}

impl Book {
    pub fn novo() -> Self {
        Book {
            id: 0,
            title: String::new(),
            author: String::new(),
            isbn: String::new(),
            pages: 0,
            status: Status::Estante,
            current_page: 0,
            rating: 0,
            review: String::new(),
            started_at: None,
            finished_at: None,
            categorias: Vec::new(),
            emprestado: None,
            serie: String::new(),
            volume: 0,
        }
    }

    /// progresso de 0 a 1, quando o total de páginas é conhecido.
    pub fn progresso(&self) -> Option<f64> {
        (self.pages > 0).then(|| (self.current_page as f64 / self.pages as f64).clamp(0.0, 1.0))
    }

    /// mudar_status troca o status e mantém as datas coerentes: começar a ler
    /// marca o início, terminar marca o fim (e completa as páginas), voltar
    /// para a estante ou "quero ler" zera tudo.
    pub fn mudar_status(&mut self, novo: Status, hoje: NaiveDate) {
        match novo {
            Status::Estante | Status::QueroLer => {
                self.started_at = None;
                self.finished_at = None;
                self.current_page = 0;
            }
            Status::Lendo => {
                self.started_at.get_or_insert(hoje);
                self.finished_at = None;
            }
            Status::Lido => {
                self.started_at.get_or_insert(hoje);
                self.finished_at = Some(hoje);
                if self.pages > 0 {
                    self.current_page = self.pages;
                }
            }
            Status::Abandonado => self.finished_at = None,
        }
        self.status = novo;
    }

    /// registrar_pagina atualiza o progresso; sair da página 0 de um livro
    /// na estante ou em "quero ler" passa ele para "lendo". Devolve true quando a leitura chegou
    /// na última página de um livro que ainda não está marcado como lido.
    pub fn registrar_pagina(&mut self, pagina: i64, hoje: NaiveDate) -> bool {
        self.current_page = if self.pages > 0 { pagina.clamp(0, self.pages) } else { pagina.max(0) };
        if self.current_page > 0 && matches!(self.status, Status::Estante | Status::QueroLer) {
            self.mudar_status(Status::Lendo, hoje);
        }
        self.pages > 0 && self.current_page >= self.pages && self.status != Status::Lido
    }

    pub fn estrelas(&self) -> String {
        if self.rating == 0 {
            return String::new();
        }
        let n = self.rating.min(5) as usize;
        "★".repeat(n) + &"☆".repeat(5 - n)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteKind {
    Nota,
    Citacao,
}

impl NoteKind {
    pub fn chave(self) -> &'static str {
        match self {
            NoteKind::Nota => "note",
            NoteKind::Citacao => "quote",
        }
    }

    pub fn de_chave(s: &str) -> Self {
        if s == "quote" { NoteKind::Citacao } else { NoteKind::Nota }
    }
}

/// ler_categorias interpreta "Romance, filosofia": separa por vírgula, em
/// minúsculas, sem vazios nem repetições, em ordem alfabética.
pub fn ler_categorias(texto: &str) -> Vec<String> {
    let mut v: Vec<String> =
        texto.split(',').map(|c| c.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()).filter(|c| !c.is_empty()).collect();
    v.sort();
    v.dedup();
    v
}

/// separar_pagina tira a página do começo de uma nota ("p.115 texto",
/// "pág. 115 texto", "p115 texto"); sem ela, devolve o texto inteiro.
pub fn separar_pagina(texto: &str) -> (Option<i64>, &str) {
    fn primeira(s: &str) -> (&str, &str) {
        let s = s.trim_start();
        match s.find(char::is_whitespace) {
            Some(i) => (&s[..i], s[i..].trim_start()),
            None => (s, ""),
        }
    }
    let numero = |n: &str| {
        (!n.is_empty() && n.len() <= 6 && n.bytes().all(|b| b.is_ascii_digit()))
            .then(|| n.parse::<i64>().ok())
            .flatten()
            .filter(|n| *n > 0)
    };
    let texto = texto.trim();
    let (palavra, resto) = primeira(texto);
    let palavra = palavra.to_lowercase();
    let Some(num) = ["pág.", "pag.", "pág", "pag", "p.", "p"].iter().find_map(|p| palavra.strip_prefix(p)) else {
        return (None, texto);
    };
    let (num, resto) = if num.is_empty() { primeira(resto) } else { (num, resto) };
    match numero(num) {
        Some(n) => (Some(n), resto),
        None => (None, texto),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub id: i64,
    pub book_id: i64,
    pub kind: NoteKind,
    pub page: Option<i64>,
    pub body: String,
    pub created_at: NaiveDate,
}

/// Plano de leitura: a partir de `inicio`, ler `paginas_dia` páginas por dia
/// desde `pagina_inicial` (onde o livro estava quando o plano foi feito) até o
/// fim do livro. O plano é fixo; atrasos aparecem como saldo negativo.
#[derive(Clone, Debug, PartialEq)]
pub struct Plano {
    pub book_id: i64,
    pub inicio: NaiveDate,
    pub pagina_inicial: i64,
    pub paginas_dia: i64,
}

impl Plano {
    /// dias que o plano leva para cobrir as páginas restantes.
    pub fn dias(&self, total: i64) -> i64 {
        let restantes = (total - self.pagina_inicial).max(0);
        ((restantes + self.paginas_dia - 1) / self.paginas_dia.max(1)).max(1)
    }

    /// fim é o último dia do plano.
    pub fn fim(&self, total: i64) -> NaiveDate {
        self.inicio + chrono::Days::new((self.dias(total) - 1) as u64)
    }

    /// meta_ate é a página que deveria estar lida ao fim de `dia`.
    pub fn meta_ate(&self, dia: NaiveDate, total: i64) -> i64 {
        let decorridos = (dia - self.inicio).num_days();
        if decorridos < 0 {
            return self.pagina_inicial.min(total);
        }
        (self.pagina_inicial + self.paginas_dia * (decorridos + 1)).min(total)
    }

    /// saldo de páginas em relação à meta de hoje: positivo é adiantado.
    pub fn saldo(&self, atual: i64, hoje: NaiveDate, total: i64) -> i64 {
        atual - self.meta_ate(hoje, total)
    }

    /// situacao compara o livro com o plano. Atrasado é não ter cumprido a
    /// meta até ontem; o dia de hoje ainda está em andamento.
    pub fn situacao(&self, b: &Book, hoje: NaiveDate) -> Situacao {
        if b.status == Status::Lido {
            return Situacao::Concluido;
        }
        if hoje < self.inicio {
            return Situacao::NaoComecou { primeira_meta: self.meta_ate(self.inicio, b.pages) };
        }
        let saldo = self.saldo(b.current_page, hoje, b.pages);
        let atras = self.meta_ate(hoje.pred_opt().unwrap_or(hoje), b.pages) - b.current_page;
        match saldo {
            s if s >= 0 => Situacao::Cumprida { a_frente: s },
            s if atras <= 0 => Situacao::FaltamHoje(-s),
            s => Situacao::Atrasado { atras, para_hoje: -s },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Situacao {
    Concluido,
    NaoComecou { primeira_meta: i64 },
    Cumprida { a_frente: i64 },
    FaltamHoje(i64),
    Atrasado { atras: i64, para_hoje: i64 },
}

impl Situacao {
    pub fn texto(self) -> String {
        match self {
            Situacao::Concluido => "✔ plano concluído".into(),
            Situacao::NaoComecou { primeira_meta } => {
                format!("o plano ainda não começou (primeira meta: pág. {primeira_meta})")
            }
            Situacao::Cumprida { a_frente: 0 } => "✔ meta de hoje cumprida".into(),
            Situacao::Cumprida { a_frente } => format!("✔ meta cumprida, {} à frente", plural(a_frente, "página")),
            Situacao::FaltamHoje(n) => format!("faltam {} hoje", plural(n, "página")),
            Situacao::Atrasado { atras, para_hoje } => {
                format!("⚠ {} atrás · {} para fechar hoje", plural(atras, "página"), plural(para_hoje, "página"))
            }
        }
    }
}

pub fn plural(n: i64, palavra: &str) -> String {
    if n == 1 { format!("1 {palavra}") } else { format!("{n} {palavra}s") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_categorias() {
        assert_eq!(ler_categorias("Romance, filosofia ,romance"), ["filosofia", "romance"]);
        assert_eq!(ler_categorias("  Ficção   Científica ,, "), ["ficção científica"]);
        assert!(ler_categorias("").is_empty());
    }

    #[test]
    fn separa_a_pagina_da_nota() {
        assert_eq!(separar_pagina("p.115 Tudo é mais simples"), (Some(115), "Tudo é mais simples"));
        assert_eq!(separar_pagina("pág. 42 linha 1\nlinha 2"), (Some(42), "linha 1\nlinha 2"));
        assert_eq!(separar_pagina("P7 curto"), (Some(7), "curto"));
        assert_eq!(separar_pagina("p 10"), (Some(10), ""));
        assert_eq!(separar_pagina("para pensar"), (None, "para pensar"));
        assert_eq!(separar_pagina("p. depois"), (None, "p. depois"));
        assert_eq!(separar_pagina("p0 nada"), (None, "p0 nada"));
        assert_eq!(separar_pagina("115 sem prefixo"), (None, "115 sem prefixo"));
    }

    fn plano(pagina_inicial: i64, paginas_dia: i64) -> Plano {
        Plano { book_id: 1, inicio: dia(14), pagina_inicial, paginas_dia }
    }

    #[test]
    fn plano_do_exemplo_800_paginas_25_por_dia() {
        let p = plano(0, 25);
        assert_eq!(p.dias(800), 32);
        assert_eq!(p.fim(800), NaiveDate::from_ymd_opt(2026, 11, 14).unwrap());
        assert_eq!(p.dias(810), 33, "sobra de 10 páginas ganha mais um dia");
        assert_eq!(plano(200, 25).dias(800), 24, "começa de onde o livro estava");
        assert_eq!(plano(800, 25).dias(800), 1);
    }

    #[test]
    fn meta_e_saldo_do_plano() {
        let p = plano(100, 25);
        assert_eq!(p.meta_ate(dia(13), 800), 100, "antes do início");
        assert_eq!(p.meta_ate(dia(14), 800), 125, "primeiro dia");
        assert_eq!(p.meta_ate(dia(20), 800), 275);
        assert_eq!(p.meta_ate(NaiveDate::from_ymd_opt(2027, 1, 1).unwrap(), 800), 800, "limita ao total");
        assert_eq!(p.saldo(290, dia(20), 800), 15);
        assert_eq!(p.saldo(260, dia(20), 800), -15);
    }

    #[test]
    fn situacao_do_plano() {
        let p = plano(0, 25);
        let livro = |pagina| Book { pages: 800, current_page: pagina, status: Status::Lendo, ..Book::novo() };
        assert_eq!(p.situacao(&livro(0), dia(13)), Situacao::NaoComecou { primeira_meta: 25 });
        assert_eq!(p.situacao(&livro(0), dia(14)), Situacao::FaltamHoje(25), "primeiro dia não é atraso");
        assert_eq!(p.situacao(&livro(25), dia(14)), Situacao::Cumprida { a_frente: 0 });
        assert_eq!(p.situacao(&livro(40), dia(14)), Situacao::Cumprida { a_frente: 15 });
        assert_eq!(p.situacao(&livro(40), dia(15)), Situacao::FaltamHoje(10));
        assert_eq!(p.situacao(&livro(10), dia(15)), Situacao::Atrasado { atras: 15, para_hoje: 40 });
        let lido = Book { status: Status::Lido, ..livro(800) };
        assert_eq!(p.situacao(&lido, dia(20)), Situacao::Concluido);
    }

    fn dia(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
    }

    #[test]
    fn status_ida_e_volta_pela_chave() {
        for s in Status::TODOS {
            assert_eq!(Status::de_chave(s.chave()), Some(s));
        }
        assert_eq!(Status::Abandonado.proximo(), Status::Estante);
        assert_eq!(Status::Estante.anterior(), Status::Abandonado);
        assert_eq!(Book::novo().status, Status::Estante, "livro novo vai para a estante");
    }

    #[test]
    fn transicoes_de_status_preenchem_datas() {
        let mut b = Book { pages: 300, ..Book::novo() };
        b.mudar_status(Status::Lendo, dia(1));
        assert_eq!(b.started_at, Some(dia(1)));
        assert_eq!(b.finished_at, None);

        b.mudar_status(Status::Lido, dia(9));
        assert_eq!(b.started_at, Some(dia(1)), "início não é sobrescrito");
        assert_eq!(b.finished_at, Some(dia(9)));
        assert_eq!(b.current_page, 300);

        b.mudar_status(Status::QueroLer, dia(10));
        assert_eq!((b.started_at, b.finished_at, b.current_page), (None, None, 0));

        b.mudar_status(Status::Lendo, dia(11));
        b.current_page = 40;
        b.mudar_status(Status::Estante, dia(12));
        assert_eq!((b.started_at, b.current_page), (None, 0), "voltar à estante zera");
    }

    #[test]
    fn registrar_pagina_comeca_leitura_e_avisa_fim() {
        let mut b = Book { pages: 200, ..Book::novo() };
        assert_eq!(b.status, Status::Estante);
        assert!(!b.registrar_pagina(50, dia(2)));
        assert_eq!(b.status, Status::Lendo);
        assert_eq!(b.started_at, Some(dia(2)));

        assert!(b.registrar_pagina(999, dia(5)), "passou do fim: avisa");
        assert_eq!(b.current_page, 200, "limita ao total");

        b.mudar_status(Status::Lido, dia(5));
        assert!(!b.registrar_pagina(200, dia(6)), "já lido: não avisa de novo");
    }

    #[test]
    fn progresso_e_estrelas() {
        let b = Book { pages: 400, current_page: 100, rating: 3, ..Book::novo() };
        assert_eq!(b.progresso(), Some(0.25));
        assert_eq!(b.estrelas(), "★★★☆☆");
        assert_eq!(Book::novo().progresso(), None);
        assert_eq!(Book::novo().estrelas(), "");
    }
}
