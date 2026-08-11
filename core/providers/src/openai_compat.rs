//! Adaptador compatible con la API de OpenAI.
//!
//! Un solo adaptador cubre OpenAI, DeepSeek, Groq, OpenRouter, Together, vLLM,
//! LM Studio y Ollama: todos hablan `/v1/chat/completions`. Añadir uno nuevo es
//! una entrada en `providers.toml`, no código.

use crate::schema::response_format_openai;
use crate::{
    Capabilities, CompletionRequest, LlmProvider, Message, ProviderError, RawCompletion, Result,
    Role, StructuredMode, Usage,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub struct OpenAiCompat {
    id: String,
    model: String,
    base_url: String,
    api_key: Option<String>,
    caps: Capabilities,
    client: reqwest::Client,
}

impl OpenAiCompat {
    pub fn nuevo(
        id: impl Into<String>,
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: Option<String>,
        caps: Capabilities,
    ) -> Self {
        Self {
            id: id.into(),
            model: model.into(),
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            api_key,
            caps,
            client: reqwest::Client::builder()
                // Una transcripción de dos horas troceada puede tardar; pero
                // colgarse indefinidamente bloquearía la cola de proceso.
                .timeout(Duration::from_secs(180))
                .build()
                .unwrap_or_default(),
        }
    }

    fn cuerpo(&self, req: &CompletionRequest) -> Value {
        let mensajes: Vec<Value> = req
            .messages
            .iter()
            .map(|m| {
                json!({
                    "role": match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                    },
                    "content": m.content,
                })
            })
            .collect();

        let mut cuerpo = json!({
            "model": self.model,
            "messages": mensajes,
            "temperature": req.temperature,
        });

        if let Some(max) = req.max_tokens {
            cuerpo["max_tokens"] = json!(max);
        }

        // Cada modo pide la salida estructurada de una forma distinta. Es la
        // única diferencia real entre proveedores «compatibles».
        if let Some(esquema) = &req.schema {
            match self.caps.structured_output {
                StructuredMode::Native => {
                    cuerpo["response_format"] =
                        response_format_openai(&req.schema_name, esquema.clone());
                }
                StructuredMode::JsonMode => {
                    cuerpo["response_format"] = json!({"type": "json_object"});
                }
                StructuredMode::PromptOnly => {}
            }
        }

        cuerpo
    }

    /// Instrucción de esquema para los proveedores que no lo aceptan como
    /// parámetro. Va en el mensaje de sistema porque es donde más caso le
    /// hacen los modelos pequeños.
    fn instruccion_esquema(req: &CompletionRequest) -> Option<Message> {
        let esquema = req.schema.as_ref()?;
        Some(Message::sistema(format!(
            "Responde ÚNICAMENTE con un JSON que cumpla este esquema, sin texto \
             alrededor y sin vallas de código:\n{}",
            serde_json::to_string(esquema).ok()?
        )))
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompat {
    fn id(&self) -> &str {
        &self.id
    }

    fn modelo(&self) -> &str {
        &self.model
    }

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    async fn completar(&self, req: &CompletionRequest) -> Result<RawCompletion> {
        let mut req = req.clone();

        // En `json_mode` la API de OpenAI exige que la palabra "json" aparezca
        // en la conversación, y los modelos locales necesitan ver el esquema.
        if matches!(
            self.caps.structured_output,
            StructuredMode::JsonMode | StructuredMode::PromptOnly
        ) {
            if let Some(instr) = Self::instruccion_esquema(&req) {
                req.messages.insert(0, instr);
            }
        }

        let inicio = Instant::now();
        let mut peticion = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .json(&self.cuerpo(&req));

        if let Some(clave) = &self.api_key {
            peticion = peticion.bearer_auth(clave);
        }

        let resp = peticion.send().await?;
        let estado = resp.status();

        if !estado.is_success() {
            let cuerpo = resp.text().await.unwrap_or_default();
            return Err(if estado.as_u16() == 429 {
                ProviderError::Cuota(self.id.clone())
            } else {
                ProviderError::Http {
                    proveedor: self.id.clone(),
                    estado: estado.as_u16(),
                    cuerpo: cuerpo.chars().take(500).collect(),
                }
            });
        }

        let json: Value = resp.json().await?;
        let (texto, tin, tout) = parsear(&json, &self.id)?;

        Ok(RawCompletion {
            texto,
            usage: Usage {
                tokens_in: tin,
                tokens_out: tout,
                cost_usd: self.caps.coste(tin, tout),
                latency_ms: inicio.elapsed().as_millis() as u32,
            },
        })
    }
}

/// Extrae texto y uso de una respuesta. Función aparte para poder probarla con
/// respuestas reales guardadas, sin necesidad de red ni de claves.
pub fn parsear(json: &Value, proveedor: &str) -> Result<(String, u32, u32)> {
    let texto = json
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError::RespuestaInvalida {
            proveedor: proveedor.to_owned(),
            motivo: "falta choices[0].message.content".to_owned(),
        })?
        .to_owned();

    // El bloque `usage` es opcional en varios proveedores compatibles; su
    // ausencia no debe tumbar la petición, solo dejar el coste a cero.
    let tin = json
        .pointer("/usage/prompt_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let tout = json
        .pointer("/usage/completion_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;

    Ok((texto, tin, tout))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(modo: StructuredMode) -> Capabilities {
        Capabilities {
            context_window: 128_000,
            max_output: 8192,
            structured_output: modo,
            accepts_audio: false,
            accepts_images: false,
            price_in_per_mtok: 0.27,
            price_out_per_mtok: 1.10,
        }
    }

    fn peticion_con_esquema() -> CompletionRequest {
        let mut r = CompletionRequest::nueva(vec![Message::usuario("resume esto")]);
        r.schema = Some(json!({"type": "object", "properties": {"tldr": {"type": "string"}}}));
        r.schema_name = "Notas".into();
        r
    }

    #[test]
    fn el_modo_estricto_manda_el_esquema_como_response_format() {
        let p = OpenAiCompat::nuevo(
            "openai",
            "https://api.openai.com/v1",
            "gpt-5",
            None,
            caps(StructuredMode::Native),
        );
        let cuerpo = p.cuerpo(&peticion_con_esquema());

        assert_eq!(cuerpo["response_format"]["type"], "json_schema");
        assert_eq!(cuerpo["response_format"]["json_schema"]["strict"], true);
        assert_eq!(cuerpo["response_format"]["json_schema"]["name"], "Notas");
    }

    #[test]
    fn json_mode_no_manda_esquema_pero_si_activa_json() {
        // DeepSeek y la mayoría de compatibles: garantizan JSON válido, no que
        // cumpla el esquema. De ahí que se valide siempre en el cliente.
        let p = OpenAiCompat::nuevo(
            "deepseek",
            "https://api.deepseek.com/v1",
            "deepseek-chat",
            None,
            caps(StructuredMode::JsonMode),
        );
        let cuerpo = p.cuerpo(&peticion_con_esquema());

        assert_eq!(cuerpo["response_format"]["type"], "json_object");
        assert!(cuerpo["response_format"].get("json_schema").is_none());
    }

    #[test]
    fn sin_soporte_de_json_el_esquema_va_en_el_mensaje_de_sistema() {
        let instr = OpenAiCompat::instruccion_esquema(&peticion_con_esquema()).unwrap();
        assert_eq!(instr.role, Role::System);
        assert!(instr.content.contains("tldr"));
        assert!(instr.content.contains("sin vallas de código"));
    }

    #[test]
    fn la_url_base_tolera_barra_final() {
        let p = OpenAiCompat::nuevo(
            "x",
            "http://localhost:11434/v1/",
            "m",
            None,
            caps(StructuredMode::JsonMode),
        );
        assert_eq!(p.base_url, "http://localhost:11434/v1");
    }

    #[test]
    fn se_parsea_una_respuesta_tipica() {
        let resp = json!({
            "choices": [{"message": {"role": "assistant", "content": "{\"tldr\":\"ok\"}"}}],
            "usage": {"prompt_tokens": 1200, "completion_tokens": 300}
        });
        let (texto, tin, tout) = parsear(&resp, "deepseek").unwrap();
        assert_eq!(texto, "{\"tldr\":\"ok\"}");
        assert_eq!((tin, tout), (1200, 300));
    }

    #[test]
    fn una_respuesta_sin_bloque_de_uso_no_falla() {
        // Varios compatibles (Ollama entre ellos) omiten `usage`. Perder el
        // dato de coste es aceptable; perder la respuesta, no.
        let resp = json!({"choices": [{"message": {"content": "{}"}}]});
        let (_, tin, tout) = parsear(&resp, "local").unwrap();
        assert_eq!((tin, tout), (0, 0));
    }

    #[test]
    fn una_respuesta_sin_contenido_da_error_identificando_al_proveedor() {
        let resp = json!({"choices": []});
        let err = parsear(&resp, "groq").unwrap_err();
        assert!(err.to_string().contains("groq"));
        // Merece intentar con el siguiente de la cadena.
        assert!(err.merece_respaldo());
    }

    #[test]
    fn la_temperatura_por_defecto_es_baja() {
        // Extraer hechos de una transcripción no es tarea creativa.
        let r = CompletionRequest::nueva(vec![]);
        assert!(r.temperature <= 0.3);
    }
}
