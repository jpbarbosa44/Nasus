//! Busca de metadados de livros: BrasilAPI (ISBNs brasileiros) e Open Library.

use std::time::Duration;

use anyhow::Result;
use serde::Deserialize;

const URL: &str = "https://openlibrary.org/search.json";
const CAMPOS: &str = "title,author_name,isbn,number_of_pages_median,first_publish_year,subject";

#[derive(Clone, Debug, PartialEq)]
pub struct Resultado {
    pub title: String,
    pub author: String,
    pub isbn: String,
    pub pages: i64,
    pub ano: Option<i64>,
    /// categorias sugeridas a partir dos assuntos da Open Library
    pub categorias: Vec<String>,
}

#[derive(Deserialize)]
struct Resposta {
    docs: Vec<Doc>,
}

#[derive(Deserialize)]
struct Doc {
    title: Option<String>,
    #[serde(default)]
    author_name: Vec<String>,
    #[serde(default)]
    isbn: Vec<String>,
    number_of_pages_median: Option<i64>,
    first_publish_year: Option<i64>,
    #[serde(default)]
    subject: Vec<String>,
}

/// Assuntos da Open Library (em inglês, às vezes em português) → categoria.
/// Os mais específicos vêm antes: "science fiction" não pode virar "ciência".
const ASSUNTOS: &[(&[&str], &str)] = &[
    (&["science fiction", "ficção científica"], "ficção científica"),
    (&["fantasy", "fantasia"], "fantasia"),
    (&["horror", "terror"], "terror"),
    (&["detective", "mystery", "crime", "policia"], "mistério"),
    (&["love stories", "romance"], "romance"),
    (&["juvenile", "young adult", "children", "infanto"], "juvenil"),
    (&["poetry", "poesia"], "poesia"),
    (&["biography", "autobiography", "biografia", "memoir"], "biografia"),
    (&["philosophy", "filosofia"], "filosofia"),
    (&["psychology", "psicologia"], "psicologia"),
    (&["self-help", "autoajuda", "self-actualization"], "autoajuda"),
    (&["economics", "economia"], "economia"),
    (&["politic", "política"], "política"),
    (&["religion", "theology", "religião", "christian"], "religião"),
    (&["history", "história"], "história"),
    (&["business", "management", "negócios"], "negócios"),
    (&["classic", "clássico"], "clássico"),
    (&["science", "ciência"], "ciência"),
    (&["fiction", "ficção", "novel"], "ficção"),
];

/// gêneros que já dizem que o livro é ficção, tornando "ficção" redundante
const GENEROS_DE_FICCAO: [&str; 5] = ["ficção científica", "fantasia", "terror", "mistério", "romance"];

/// categorias_de traduz os assuntos em até 3 categorias, na ordem em que aparecem.
pub fn categorias_de(assuntos: &[String]) -> Vec<String> {
    let mut v: Vec<&str> = Vec::new();
    for a in assuntos {
        let a = a.to_lowercase();
        if let Some((_, cat)) = ASSUNTOS.iter().find(|(chaves, _)| chaves.iter().any(|c| a.contains(c)))
            && !v.contains(cat)
        {
            v.push(cat);
        }
    }
    if v.iter().any(|c| GENEROS_DE_FICCAO.contains(c)) {
        v.retain(|c| *c != "ficção");
    }
    v.into_iter().take(3).map(String::from).collect()
}

/// normalizar_isbn devolve o ISBN sem hífens/espaços quando `q` parece um
/// ISBN-10 ou ISBN-13.
pub fn normalizar_isbn(q: &str) -> Option<String> {
    let s: String = q.chars().filter(|c| !matches!(c, '-' | ' ')).collect::<String>().to_uppercase();
    let digitos = s.chars().filter(char::is_ascii_digit).count();
    let ok = match s.len() {
        13 => digitos == 13,
        10 => digitos == 10 || (digitos == 9 && s.ends_with('X')),
        _ => false,
    };
    ok.then_some(s)
}

const USER_AGENT: &str = concat!("nasus/", env!("CARGO_PKG_VERSION"));

/// buscar consulta por ISBN (quando `q` parece um) ou por texto livre.
///
/// A Open Library tem poucos ISBNs brasileiros e devolve o título da obra
/// original; por isso o ISBN vai primeiro à BrasilAPI (base da CBL, a agência
/// do ISBN no Brasil), e a Open Library só completa o que faltar.
pub fn buscar(q: &str) -> Result<Vec<Resultado>> {
    let Some(isbn) = normalizar_isbn(q) else {
        return buscar_openlibrary("q", q.trim(), None);
    };
    let br = buscar_brasilapi(&isbn);
    let ol = buscar_openlibrary("isbn", &isbn, Some(&isbn));
    match br {
        Ok(Some(livro)) => Ok(vec![completar(livro, ol.ok().and_then(|v| v.into_iter().next()))]),
        Ok(None) => ol,
        // BrasilAPI fora do ar: vale o que a Open Library achar
        Err(e) => match ol {
            Ok(v) if !v.is_empty() => Ok(v),
            _ => Err(e),
        },
    }
}

fn buscar_openlibrary(campo: &str, valor: &str, isbn: Option<&str>) -> Result<Vec<Resultado>> {
    let corpo = ureq::get(URL)
        .set("User-Agent", USER_AGENT)
        .timeout(Duration::from_secs(20))
        .query("fields", CAMPOS)
        .query("limit", "20")
        .query(campo, valor)
        .call()?
        .into_string()?;
    interpretar(&corpo, isbn)
}

/// buscar_brasilapi devolve None quando o ISBN não está cadastrado.
fn buscar_brasilapi(isbn: &str) -> Result<Option<Resultado>> {
    let resp = ureq::get(&format!("https://brasilapi.com.br/api/isbn/v1/{isbn}"))
        .set("User-Agent", USER_AGENT)
        .timeout(Duration::from_secs(20))
        .call();
    match resp {
        Ok(r) => interpretar_brasilapi(&r.into_string()?, isbn),
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

#[derive(Deserialize)]
struct LivroBr {
    title: Option<String>,
    #[serde(default)]
    authors: Vec<String>,
    year: Option<i64>,
    page_count: Option<i64>,
}

pub fn interpretar_brasilapi(json: &str, isbn: &str) -> Result<Option<Resultado>> {
    let l: LivroBr = serde_json::from_str(json)?;
    Ok(l.title.filter(|t| !t.trim().is_empty()).map(|title| Resultado {
        title,
        author: l.authors.join(", "),
        isbn: isbn.to_string(),
        pages: l.page_count.unwrap_or(0),
        ano: l.year,
        categorias: Vec::new(),
    }))
}

/// completar mantém o título brasileiro e preenche as lacunas com a Open
/// Library. O autor de lá tem preferência: a lista da BrasilAPI costuma
/// misturar os tradutores.
fn completar(br: Resultado, ol: Option<Resultado>) -> Resultado {
    let Some(ol) = ol else { return br };
    Resultado {
        author: if ol.author.is_empty() { br.author } else { ol.author },
        pages: if br.pages > 0 { br.pages } else { ol.pages },
        ano: br.ano.or(ol.ano),
        categorias: if br.categorias.is_empty() { ol.categorias } else { br.categorias },
        ..br
    }
}

/// interpretar converte a resposta JSON; quando a busca foi por ISBN, é esse
/// o ISBN que fica no resultado (a obra lista os de todas as edições).
pub fn interpretar(json: &str, isbn_buscado: Option<&str>) -> Result<Vec<Resultado>> {
    let r: Resposta = serde_json::from_str(json)?;
    Ok(r.docs
        .into_iter()
        .filter_map(|d| {
            let isbn = match isbn_buscado {
                Some(i) => i.to_string(),
                None => d
                    .isbn
                    .iter()
                    .find(|i| i.len() == 13 && (i.starts_with("978") || i.starts_with("979")))
                    .or(d.isbn.first())
                    .cloned()
                    .unwrap_or_default(),
            };
            Some(Resultado {
                title: d.title?,
                author: d.author_name.join(", "),
                isbn,
                pages: d.number_of_pages_median.unwrap_or(0),
                ano: d.first_publish_year,
                categorias: categorias_de(&d.subject),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconhece_isbn() {
        assert_eq!(normalizar_isbn("978-85-359-1484-9").as_deref(), Some("9788535914849"));
        assert_eq!(normalizar_isbn("0-306-40615-x").as_deref(), Some("030640615X"));
        assert_eq!(normalizar_isbn("Dom Casmurro"), None);
        assert_eq!(normalizar_isbn("12345"), None);
    }

    #[test]
    fn traduz_assuntos_em_categorias() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            categorias_de(&s(&["Dune (Imaginary place)", "Fiction", "Science fiction"])),
            ["ficção científica"],
            "o gênero específico dispensa o genérico"
        );
        assert_eq!(
            categorias_de(&s(&["Philosophy", "Stoics", "Ethics", "History of philosophy", "Classic Literature", "Fiction"])),
            ["filosofia", "clássico", "ficção"],
            "no máximo 3, sem repetir"
        );
        assert!(categorias_de(&s(&["Accessible book", "Protected DAISY"])).is_empty());
    }

    #[test]
    fn interpreta_resposta() {
        let json = r#"{"numFound": 2, "docs": [
            {"title": "Ensaio sobre a cegueira", "author_name": ["José Saramago"],
             "isbn": ["0156007754", "9789722110947"], "number_of_pages_median": 310,
             "first_publish_year": 1995, "subject": ["Blindness", "Portuguese fiction", "Fiction, general"]},
            {"author_name": ["Sem título"]},
            {"title": "Anônimo"}
        ]}"#;
        let r = interpretar(json, None).unwrap();
        assert_eq!(r.len(), 2, "docs sem título são descartados");
        assert_eq!(
            r[0],
            Resultado {
                title: "Ensaio sobre a cegueira".into(),
                author: "José Saramago".into(),
                isbn: "9789722110947".into(),
                pages: 310,
                ano: Some(1995),
                categorias: vec!["ficção".into()],
            }
        );
        assert_eq!((r[1].author.as_str(), r[1].pages, r[1].ano), ("", 0, None));

        let r = interpretar(json, Some("0156007754")).unwrap();
        assert_eq!(r[0].isbn, "0156007754");
    }

    #[test]
    fn brasilapi_e_complemento_da_open_library() {
        let json = r#"{"isbn":"9788532530783","title":"Harry Potter e a pedra filosofal","subtitle":null,
            "authors":[],"publisher":"Editora Rocco","year":null,"page_count":null,"provider":"cbl"}"#;
        let br = interpretar_brasilapi(json, "9788532530783").unwrap().unwrap();
        assert_eq!((br.title.as_str(), br.pages), ("Harry Potter e a pedra filosofal", 0));
        assert_eq!(interpretar_brasilapi(r#"{"title":null}"#, "1").unwrap(), None);

        let ol = Resultado {
            title: "Harry Potter and the Philosopher's Stone".into(),
            author: "J. K. Rowling".into(),
            isbn: "9788532530783".into(),
            pages: 302,
            ano: Some(1997),
            categorias: vec!["fantasia".into()],
        };
        let r = completar(br.clone(), Some(ol));
        assert_eq!(r.title, "Harry Potter e a pedra filosofal", "título brasileiro fica");
        assert_eq!((r.author.as_str(), r.pages, r.ano), ("J. K. Rowling", 302, Some(1997)));
        assert_eq!(r.categorias, ["fantasia"], "categorias vêm da Open Library");
        assert_eq!(completar(br.clone(), None), br);
    }
}
