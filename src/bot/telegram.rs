//! Cliente mínimo da API HTTP do Telegram (https://core.telegram.org/bots/api).

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde::de::{DeserializeOwned, IgnoredAny};
use serde::{Deserialize, Serialize};

use super::comandos::Resposta;

/// limite de texto de uma mensagem no Telegram (4096) com folga
const MAX_TEXTO: usize = 4000;

pub struct Telegram {
    agente: ureq::Agent,
    token: String,
}

#[derive(Deserialize)]
pub struct Usuario {
    pub username: Option<String>,
}

#[derive(Deserialize)]
pub struct Chat {
    pub id: i64,
}

#[derive(Deserialize)]
pub struct Mensagem {
    pub message_id: i64,
    pub chat: Chat,
    pub text: Option<String>,
}

#[derive(Deserialize)]
pub struct Callback {
    pub id: String,
    pub message: Option<Mensagem>,
    pub data: Option<String>,
}

#[derive(Deserialize)]
pub struct Update {
    pub update_id: i64,
    pub message: Option<Mensagem>,
    pub callback_query: Option<Callback>,
}

#[derive(Deserialize)]
struct Envelope<T> {
    ok: bool,
    description: Option<String>,
    result: Option<T>,
}

#[derive(Serialize)]
struct Botao<'a> {
    text: &'a str,
    callback_data: &'a str,
}

#[derive(Serialize)]
struct Teclado<'a> {
    inline_keyboard: Vec<Vec<Botao<'a>>>,
}

impl Telegram {
    pub fn new(token: &str) -> Self {
        // o timeout precisa ser maior que o long polling de getUpdates (50s)
        let agente = ureq::AgentBuilder::new().timeout(Duration::from_secs(70)).build();
        Telegram { agente, token: token.to_string() }
    }

    /// chamar faz um POST em /bot<token>/<metodo> e devolve o `result`.
    fn chamar<T: DeserializeOwned>(&self, metodo: &str, params: &[(&str, &str)]) -> Result<T> {
        let url = format!("https://api.telegram.org/bot{}/{metodo}", self.token);
        let resp = match self.agente.post(&url).send_form(params) {
            Ok(r) => r,
            // erros da API (400, 403...) trazem a descrição no corpo
            Err(ureq::Error::Status(_, r)) => r,
            // a URL contém o token: nunca deixar ele ir parar num log
            Err(e) => return Err(anyhow!("{metodo}: {}", e.to_string().replace(&self.token, "***"))),
        };
        let env: Envelope<T> = resp.into_json().with_context(|| format!("{metodo}: resposta inválida"))?;
        if !env.ok {
            bail!("{metodo}: {}", env.description.unwrap_or_default());
        }
        env.result.with_context(|| format!("{metodo}: resposta sem resultado"))
    }

    pub fn get_me(&self) -> Result<Usuario> {
        self.chamar("getMe", &[])
    }

    /// get_updates espera até 50s por novidades (long polling).
    pub fn get_updates(&self, offset: i64) -> Result<Vec<Update>> {
        self.chamar(
            "getUpdates",
            &[
                ("offset", &offset.to_string()),
                ("timeout", "50"),
                ("allowed_updates", r#"["message","callback_query"]"#),
            ],
        )
    }

    /// enviar manda a resposta, quebrando textos longos em várias mensagens;
    /// os botões vão na última.
    pub fn enviar(&self, chat_id: i64, r: &Resposta) -> Result<()> {
        let partes = fatiar(&r.texto, MAX_TEXTO);
        let teclado = Teclado {
            inline_keyboard: r
                .botoes
                .iter()
                .map(|linha| linha.iter().map(|(t, d)| Botao { text: t, callback_data: d }).collect())
                .collect(),
        };
        let teclado = serde_json::to_string(&teclado)?;
        let chat = chat_id.to_string();
        for (i, parte) in partes.iter().enumerate() {
            let mut params = vec![("chat_id", chat.as_str()), ("text", parte.as_str())];
            if i + 1 == partes.len() && !r.botoes.is_empty() {
                params.push(("reply_markup", &teclado));
            }
            self.chamar::<IgnoredAny>("sendMessage", &params)?;
        }
        Ok(())
    }

    /// tirar_botoes remove o teclado inline de uma mensagem já enviada.
    pub fn tirar_botoes(&self, chat_id: i64, msg_id: i64) -> Result<()> {
        self.chamar::<IgnoredAny>(
            "editMessageReplyMarkup",
            &[
                ("chat_id", &chat_id.to_string()),
                ("message_id", &msg_id.to_string()),
                ("reply_markup", r#"{"inline_keyboard":[]}"#),
            ],
        )?;
        Ok(())
    }

    /// responder_callback fecha o "relógio" que o Telegram mostra ao tocar num botão.
    pub fn responder_callback(&self, id: &str, texto: &str) -> Result<()> {
        let mut params = vec![("callback_query_id", id)];
        if !texto.is_empty() {
            params.push(("text", texto));
        }
        self.chamar::<IgnoredAny>("answerCallbackQuery", &params)?;
        Ok(())
    }
}

/// fatiar quebra o texto em pedaços de até `max` bytes, nas quebras de linha.
fn fatiar(texto: &str, max: usize) -> Vec<String> {
    let mut partes = Vec::new();
    let mut atual = String::new();
    for linha in texto.lines() {
        if !atual.is_empty() && atual.len() + linha.len() + 1 > max {
            partes.push(std::mem::take(&mut atual));
        }
        if !atual.is_empty() {
            atual.push('\n');
        }
        atual.push_str(linha);
    }
    if !atual.is_empty() || partes.is_empty() {
        partes.push(atual);
    }
    partes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fatia_nas_quebras_de_linha() {
        assert_eq!(fatiar("abc", 10), ["abc"]);
        assert_eq!(fatiar("aaaa\nbbbb\ncccc", 10), ["aaaa\nbbbb", "cccc"]);
    }
}
