//! Adaptador nativo de Gemini.
//!
//! Existe además del compatible-OpenAI porque la API nativa da acceso a lo que
//! aquí importa: contexto muy largo, `responseSchema` estricto y entrada
//! directa de imágenes para el OCR de diapositivas.

use crate::schema::response_schema_gemini;
use crate::{
    Capabilities, CompletionRequest, LlmProvider, ProviderError, RawCompletion, Result, Role,
    StructuredMode, Usage,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const BASE: &str = "https://generativelanguage.googleapis.com/v1beta";

pub struct Gemini {
    id: String,
    model: String,
    api_key: String,
    caps: Capabilities,
    client: reqwest::Client,
    base_url: String,
}

impl Gemini {
    pub fn nuevo(
        id: impl Into<String>,
        model: impl Into<String>,
        api_key: impl Into<String>,
        caps: Capabilities,
    ) -> Self {
        Self {
            id: id.into(),
            model: model.into(),
            api_key: api_key.into(),
            caps,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(180))
                .build()
                .unwrap_or_default(),
            base_url: BASE.to_owned(),
        }
    }

    fn cuerpo(&self, req: &CompletionRequest) -> Value {
        // Gemini separa la instrucción de sistema del historial, y llama
        // "model" a lo que OpenAI llama "assistant".
        let sistema: Vec<&str> = req
            .messages
            .iter()
            .filter(|m| m.role == Role::System)
            .map(|m| m.content.as_str())
            .collect();

        let contents: Vec<Value> = req
            .messages
            .iter()
            .filter(|m| m.role != Role::System)
            .map(|m| {
                json!({
                    "role": if m.role == Role::Assistant { "model" } else { "user" },
                    "parts": [{"text": m.content}],
                })
            })
            .collect();

        let mut generation = json!({ "temperature": req.temperature });
        if let Some(max) = req.max_tokens {
            generation["maxOutputTokens"] = json!(max);
        }

        if let Some(esquema) = &req.schema {
            generation["responseMimeType"] = json!("application/json");
            if self.caps.structured_output == StructuredMode::Native {
                // Gemini no entiende `$ref` ni `$defs`: hay que aplanar.
                generation["responseSchema"] = response_schema_gemini(esquema.clone());
            }
        }

        let mut cuerpo = json!({
            "contents": contents,
            "generationConfig": generation,
        });

        if !sistema.is_empty() {
            cuerpo["systemInstruction"] = json!({
                "parts": [{"text": sistema.join("\n\n")}]
            });
        }

        cuerpo
    }
}

#[async_trait]
impl LlmProvider for Gemini {
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
        let inicio = Instant::now();
        let url = format!("{}/models/{}:generateContent", self.base_url, self.model);

        let resp = self
            .client
            .post(&url)
            // En cabecera y no en la query: así la clave no acaba en los logs
            // de acceso de ningún intermediario.
            .header("x-goog-api-key", &self.api_key)
            .json(&self.cuerpo(req))
            .send()
            .await?;

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

pub fn parsear(json: &Value, proveedor: &str) -> Result<(String, u32, u32)> {
    // Un corte por límite de tokens devuelve contenido truncado que parece
    // válido: hay que detectarlo aquí o el JSON incompleto se interpretaría
    // como un fallo de esquema y se reintentaría tres veces en balde.
    if let Some(razon) = json
        .pointer("/candidates/0/finishReason")
        .and_then(Value::as_str)
    {
        if razon != "STOP" {
            return Err(ProviderError::RespuestaInvalida {
                proveedor: proveedor.to_owned(),
                motivo: format!("la generación terminó por «{razon}», no por STOP"),
            });
        }
    }

    // Las partes pueden venir troceadas; se concatenan todas.
    let texto = json
        .pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
        .map(|partes| {
            partes
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("")
        })
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ProviderError::RespuestaInvalida {
            proveedor: proveedor.to_owned(),
            motivo: "falta candidates[0].content.parts[].text".to_owned(),
        })?;

    let tin = json
        .pointer("/usageMetadata/promptTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let tout = json
        .pointer("/usageMetadata/candidatesTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;

    Ok((texto, tin, tout))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Message;

    fn caps() -> Capabilities {
        Capabilities {
            context_window: 1_000_000,
            max_output: 8192,
            structured_output: StructuredMode::Native,
            accepts_audio: true,
            accepts_images: true,
            price_in_per_mtok: 0.30,
            price_out_per_mtok: 2.50,
        }
    }

    fn provider() -> Gemini {
        Gemini::nuevo("gemini-flash", "gemini-2.5-flash", "clave", caps())
    }

    #[test]
    fn la_instruccion_de_sistema_va_en_su_propio_campo() {
        // Gemini no acepta mensajes con rol "system" dentro de `contents`.
        let req = CompletionRequest::nueva(vec![
            Message::sistema("eres un asistente de apuntes"),
            Message::usuario("resume la clase"),
        ]);
        let cuerpo = provider().cuerpo(&req);

        assert_eq!(
            cuerpo["systemInstruction"]["parts"][0]["text"],
            "eres un asistente de apuntes"
        );
        assert_eq!(cuerpo["contents"].as_array().unwrap().len(), 1);
        assert_eq!(cuerpo["contents"][0]["role"], "user");
    }

    #[test]
    fn el_rol_del_asistente_se_traduce_a_model() {
        let req = CompletionRequest::nueva(vec![
            Message::usuario("hola"),
            Message::asistente("{}"),
            Message::usuario("corrige"),
        ]);
        let cuerpo = provider().cuerpo(&req);
        assert_eq!(cuerpo["contents"][1]["role"], "model");
    }

    #[test]
    fn el_esquema_se_manda_aplanado_y_activa_salida_json() {
        let mut req = CompletionRequest::nueva(vec![Message::usuario("x")]);
        req.schema = Some(json!({
            "type": "object",
            "properties": {"a": {"$ref": "#/$defs/A"}},
            "$defs": {"A": {"type": "string"}}
        }));

        let cuerpo = provider().cuerpo(&req);
        assert_eq!(
            cuerpo["generationConfig"]["responseMimeType"],
            "application/json"
        );

        let esquema = &cuerpo["generationConfig"]["responseSchema"];
        assert_eq!(esquema["properties"]["a"]["type"], "string");
        assert!(!serde_json::to_string(esquema).unwrap().contains("$ref"));
    }

    #[test]
    fn se_parsea_una_respuesta_tipica() {
        let resp = json!({
            "candidates": [{
                "content": {"parts": [{"text": "{\"tldr\":\"ok\"}"}], "role": "model"},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 20000, "candidatesTokenCount": 5000}
        });
        let (texto, tin, tout) = parsear(&resp, "gemini-flash").unwrap();
        assert_eq!(texto, "{\"tldr\":\"ok\"}");
        assert_eq!((tin, tout), (20000, 5000));
    }

    #[test]
    fn las_partes_troceadas_se_concatenan() {
        let resp = json!({
            "candidates": [{
                "content": {"parts": [{"text": "{\"a\":"}, {"text": "1}"}]},
                "finishReason": "STOP"
            }]
        });
        let (texto, _, _) = parsear(&resp, "gemini-flash").unwrap();
        assert_eq!(texto, "{\"a\":1}");
    }

    #[test]
    fn una_generacion_truncada_se_detecta_en_lugar_de_reintentar_a_ciegas() {
        // Sin esta comprobación, el JSON incompleto parecería un fallo de
        // esquema y se reintentaría tres veces con el mismo resultado.
        let resp = json!({
            "candidates": [{
                "content": {"parts": [{"text": "{\"tldr\": \"a med"}]},
                "finishReason": "MAX_TOKENS"
            }]
        });
        let err = parsear(&resp, "gemini-flash").unwrap_err();
        assert!(err.to_string().contains("MAX_TOKENS"));
    }

    #[test]
    fn una_respuesta_bloqueada_por_seguridad_da_error_claro() {
        let resp = json!({"candidates": [{"finishReason": "SAFETY"}]});
        let err = parsear(&resp, "gemini-flash").unwrap_err();
        assert!(err.to_string().contains("SAFETY"));
    }
}
