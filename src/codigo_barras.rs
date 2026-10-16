//! Leitura do ISBN pelo código de barras (EAN-13) da contracapa.

use anyhow::{Context, Result};
use image::DynamicImage;
use rxing::{BarcodeFormat, DecodeHints};

/// acima disso a foto é reduzida: o código continua legível e a leitura fica rápida
const LADO_MAX: u32 = 2000;

/// ler_isbn procura um EAN-13 de livro (978/979) na imagem; None quando não
/// há código ou ele é de outro tipo de produto.
pub fn ler_isbn(bytes: &[u8]) -> Result<Option<String>> {
    let mut img = image::load_from_memory(bytes).context("não consegui abrir a imagem")?;
    if img.width().max(img.height()) > LADO_MAX {
        img = img.thumbnail(LADO_MAX, LADO_MAX);
    }
    // fotos em pé deixam o código na vertical
    Ok(decodificar(&img).or_else(|| decodificar(&img.rotate90())))
}

fn decodificar(img: &DynamicImage) -> Option<String> {
    let luma = img.to_luma8();
    let (w, h) = luma.dimensions();
    let mut hints = DecodeHints { TryHarder: Some(true), ..Default::default() };
    let r = rxing::helpers::detect_in_luma_with_hints(luma.into_raw(), w, h, Some(BarcodeFormat::EAN_13), &mut hints)
        .ok()?;
    let codigo = r.getText();
    isbn13_valido(codigo).then(|| codigo.to_string())
}

/// isbn13_valido confere o prefixo de livro e o dígito verificador do EAN-13.
pub fn isbn13_valido(s: &str) -> bool {
    if s.len() != 13 || !s.bytes().all(|b| b.is_ascii_digit()) || !(s.starts_with("978") || s.starts_with("979")) {
        return false;
    }
    let soma: u32 = s.bytes().enumerate().map(|(i, b)| u32::from(b - b'0') * if i % 2 == 0 { 1 } else { 3 }).sum();
    soma.is_multiple_of(10)
}

/// png_de_teste gera a imagem PNG de um EAN-13, para os testes.
#[cfg(test)]
pub fn png_de_teste(codigo: &str) -> Vec<u8> {
    use rxing::Writer;
    let matriz = rxing::MultiFormatWriter.encode(codigo, &BarcodeFormat::EAN_13, 400, 150).unwrap();
    let img = DynamicImage::from(&matriz);
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    png
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valida_isbn13() {
        assert!(isbn13_valido("9788535914849"));
        assert!(isbn13_valido("9791032305690"));
        assert!(!isbn13_valido("9788535914848"), "dígito verificador errado");
        assert!(!isbn13_valido("7891000315507"), "EAN de outro produto");
        assert!(!isbn13_valido("978853591484"));
    }

    #[test]
    fn le_o_codigo_de_barras() {
        assert_eq!(ler_isbn(&png_de_teste("9788535914849")).unwrap().as_deref(), Some("9788535914849"));

        let girada = image::load_from_memory(&png_de_teste("9788535914849")).unwrap().rotate270();
        let mut png = Vec::new();
        girada.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        assert_eq!(ler_isbn(&png).unwrap().as_deref(), Some("9788535914849"), "foto em pé");

        assert_eq!(ler_isbn(&png_de_teste("7891000315507")).unwrap(), None, "não é livro");
        assert!(ler_isbn(b"isto nao e imagem").is_err());
    }
}
