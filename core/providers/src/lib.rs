//! Capa de proveedores de IA.
//!
//! Requisito de producto: **debe funcionar con cualquiera** —Gemini, DeepSeek,
//! OpenAI o un modelo local— sin tocar código, solo configuración.
//!
//! La observación que lo simplifica todo: DeepSeek, Groq, OpenRouter, Together,
//! vLLM, LM Studio y Ollama exponen la **misma API que OpenAI**. Así que no
//! hacen falta cuatro adaptadores, sino dos: uno compatible-OpenAI
//! parametrizado por URL, y uno nativo de Gemini.
//!
//! La salida estructurada sí varía entre proveedores. El contrato de este
//! crate es que el resto del sistema **nunca** tiene que saberlo: se valida
//! siempre en el cliente contra el esquema y, si falla, se reintenta
//! inyectando el error de validación en el prompt.

pub mod config;
pub mod gemini;
pub mod openai_compat;
pub mod router;
pub mod schema;
pub mod secretos;

use async_trait::async_trait;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("error de red: {0}")]
    Red(#[from] reqwest::Error),

    #[error("el proveedor «{proveedor}» devolvió {estado}: {cuerpo}")]
    Http {
        proveedor: String,
        estado: u16,
        cuerpo: String,
    },

    #[error("límite de peticiones en «{0}»")]
    Cuota(String),

    #[error("respuesta ilegible de «{proveedor}»: {motivo}")]
    RespuestaInvalida { proveedor: String, motivo: String },

    #[error("la salida no cumple el esquema tras {intentos} intentos: {ultimo_error}")]
    EsquemaIncumplido { intentos: u32, ultimo_error: String },

    #[error("no hay ningún proveedor configurado para la tarea «{0}»")]
    SinProveedor(String),

    #[error("todos los proveedores fallaron: {0}")]
    CadenaAgotada(String),

    #[error("configuración inválida: {0}")]
    Config(String),
}

impl ProviderError {
    /// Si merece la pena pasar al siguiente proveedor de la cadena.
    ///
    /// Un 429 o un 503 son transitorios y ajenos a nosotros. Un 400 significa
    /// que la petición está mal construida: reintentarla en otro proveedor
    /// daría el mismo error y solo gastaría tiempo y dinero.
    pub fn merece_respaldo(&self) -> bool {
        match self {
            ProviderError::Red(_) => true,
            ProviderError::Cuota(_) => true,
            ProviderError::Http { estado, .. } => *estado == 408 || *estado >= 500,
            ProviderError::RespuestaInvalida { .. } => true,
            ProviderError::EsquemaIncumplido { .. } => true,
            _ => false,
        }
    }
}

pub type Result<T> = std::result::Result<T, ProviderError>;

// ---------------------------------------------------------------------------
// Contrato
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructuredMode {
    /// El proveedor valida el esquema por su cuenta (OpenAI, Gemini).
    Native,
    /// "Devuelve JSON válido", sin garantía de esquema (DeepSeek y la mayoría).
    JsonMode,
    /// Solo instrucciones en el prompt (modelos locales pequeños).
    PromptOnly,
}

#[derive(Debug, Clone)]
pub struct Capabilities {
    pub context_window: usize,
    pub max_output: usize,
    pub structured_output: StructuredMode,
    pub accepts_audio: bool,
    pub accepts_images: bool,
    pub price_in_per_mtok: f64,
    pub price_out_per_mtok: f64,
}

impl Capabilities {
    pub fn coste(&self, tokens_in: u32, tokens_out: u32) -> f64 {
        (tokens_in as f64 / 1e6) * self.price_in_per_mtok
            + (tokens_out as f64 / 1e6) * self.price_out_per_mtok
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn sistema(c: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: c.into(),
        }
    }
    pub fn usuario(c: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: c.into(),
        }
    }
    pub fn asistente(c: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: c.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub messages: Vec<Message>,
    /// Esquema ya endurecido. Lo rellena [`completar_estructurado`].
    pub schema: Option<Value>,
    pub schema_name: String,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
}

impl CompletionRequest {
    pub fn nueva(messages: Vec<Message>) -> Self {
        Self {
            messages,
            schema: None,
            schema_name: "respuesta".to_owned(),
            // Extraer hechos de una transcripción no es una tarea creativa:
            // una temperatura alta solo añade invención.
            temperature: 0.2,
            max_tokens: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub tokens_in: u32,
    pub tokens_out: u32,
    pub cost_usd: f64,
    pub latency_ms: u32,
}

#[derive(Debug, Clone)]
pub struct RawCompletion {
    pub texto: String,
    pub usage: Usage,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn id(&self) -> &str;
    fn modelo(&self) -> &str;
    fn capabilities(&self) -> &Capabilities;

    /// Una sola llamada, sin validación ni reintentos. Los adaptadores solo se
    /// ocupan de HTTP; la lógica de esquema es común y vive fuera.
    async fn completar(&self, req: &CompletionRequest) -> Result<RawCompletion>;
}

// ---------------------------------------------------------------------------
// Salida estructurada con validación y reintento
// ---------------------------------------------------------------------------

/// Número de intentos antes de rendirse. Dos reintentos: uno suele bastar para
/// que el modelo corrija un campo mal puesto; a partir del tercero, el problema
/// es del prompt o del esquema y reintentar solo quema dinero.
const MAX_INTENTOS: u32 = 3;

/// Pide una respuesta que cumpla el tipo `T`, cueste lo que cueste.
///
/// Genera el esquema desde el propio tipo, se lo pasa al proveedor en el
/// formato que ese proveedor entienda, valida la respuesta y —si no cuadra—
/// reintenta **inyectando el error de deserialización en la conversación**.
/// El modelo ve exactamente qué campo estropeó y suele arreglarlo al primer
/// intento.
///
/// Es la función que hace real el "funciona con cualquier proveedor": arriba de
/// aquí nadie sabe si el modelo tenía modo estricto o no.
pub async fn completar_estructurado<T>(
    proveedor: &dyn LlmProvider,
    mut req: CompletionRequest,
) -> Result<(T, Usage)>
where
    T: DeserializeOwned + schemars::JsonSchema,
{
    let bruto = serde_json::to_value(schemars::schema_for!(T))
        .map_err(|e| ProviderError::Config(e.to_string()))?;
    req.schema = Some(schema::a_estricto(bruto));
    req.schema_name = nombre_corto::<T>();

    let mut acumulado = Usage::default();
    let mut ultimo_error = String::new();

    for intento in 1..=MAX_INTENTOS {
        let respuesta = proveedor.completar(&req).await?;

        acumulado.tokens_in += respuesta.usage.tokens_in;
        acumulado.tokens_out += respuesta.usage.tokens_out;
        acumulado.cost_usd += respuesta.usage.cost_usd;
        acumulado.latency_ms += respuesta.usage.latency_ms;

        let json = extraer_json(&respuesta.texto);

        match serde_json::from_str::<T>(&json) {
            Ok(valor) => return Ok((valor, acumulado)),
            Err(e) => {
                ultimo_error = e.to_string();
                tracing::warn!(
                    proveedor = proveedor.id(),
                    intento,
                    error = %ultimo_error,
                    "la salida no cumple el esquema, reintentando"
                );

                if intento < MAX_INTENTOS {
                    req.messages.push(Message::asistente(respuesta.texto));
                    req.messages.push(Message::usuario(format!(
                        "Ese JSON no cumple el esquema: {ultimo_error}\n\n\
                         Devuelve ÚNICAMENTE el JSON corregido, sin texto \
                         alrededor y sin vallas de código."
                    )));
                }
            }
        }
    }

    Err(ProviderError::EsquemaIncumplido {
        intentos: MAX_INTENTOS,
        ultimo_error,
    })
}

fn nombre_corto<T: schemars::JsonSchema>() -> String {
    // `schema_name` puede traer genéricos o rutas; los nombres de esquema solo
    // admiten [a-zA-Z0-9_-].
    T::schema_name()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Recupera el JSON de una respuesta que puede venir envuelta.
///
/// Los modelos en `json_mode` —y sobre todo los locales— tienden a rodear la
/// respuesta con vallas de código o con una frase amable. Rescatarlo aquí
/// evita un reintento completo, que costaría una llamada entera.
pub fn extraer_json(texto: &str) -> String {
    let t = texto.trim();

    // ```json ... ```
    if let Some(inicio) = t.find("```") {
        let resto = &t[inicio + 3..];
        let resto = resto.strip_prefix("json").unwrap_or(resto);
        if let Some(fin) = resto.find("```") {
            return resto[..fin].trim().to_owned();
        }
    }

    // Primer objeto o array equilibrado del texto.
    let apertura = t.find(['{', '[']);
    if let Some(i) = apertura {
        let abre = t.as_bytes()[i];
        let cierra = if abre == b'{' { b'}' } else { b']' };
        let mut nivel = 0i32;
        let mut en_cadena = false;
        let mut escape = false;

        for (j, b) in t.bytes().enumerate().skip(i) {
            if escape {
                escape = false;
                continue;
            }
            match b {
                b'\\' if en_cadena => escape = true,
                b'"' => en_cadena = !en_cadena,
                _ if en_cadena => {}
                b if b == abre => nivel += 1,
                b if b == cierra => {
                    nivel -= 1;
                    if nivel == 0 {
                        return t[i..=j].to_owned();
                    }
                }
                _ => {}
            }
        }
    }

    t.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_coste_se_calcula_por_millon_de_tokens() {
        let c = Capabilities {
            context_window: 1_000_000,
            max_output: 8192,
            structured_output: StructuredMode::Native,
            accepts_audio: true,
            accepts_images: true,
            price_in_per_mtok: 0.30,
            price_out_per_mtok: 2.50,
        };
        // Una sesión típica: 20k de entrada, 5k de salida.
        let coste = c.coste(20_000, 5_000);
        assert!((coste - 0.0185).abs() < 1e-9, "coste calculado: {coste}");
    }

    #[test]
    fn se_rescata_el_json_envuelto_en_vallas_de_codigo() {
        let respuesta =
            "Claro, aquí tienes:\n```json\n{\"tldr\": \"hola\"}\n```\n¡Espero que sirva!";
        assert_eq!(extraer_json(respuesta), r#"{"tldr": "hola"}"#);
    }

    #[test]
    fn se_rescata_el_json_sin_vallas_pero_con_texto_alrededor() {
        let respuesta = "Aquí va: {\"a\": 1, \"b\": {\"c\": 2}} y ya está.";
        assert_eq!(extraer_json(respuesta), r#"{"a": 1, "b": {"c": 2}}"#);
    }

    #[test]
    fn las_llaves_dentro_de_una_cadena_no_confunden_al_extractor() {
        // Un texto transcrito puede contener llaves; contarlas ingenuamente
        // cortaría el JSON por la mitad.
        let respuesta = r#"{"cita": "dijo } y luego {", "ok": true}"#;
        let extraido = extraer_json(respuesta);
        let v: serde_json::Value = serde_json::from_str(&extraido).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["cita"], "dijo } y luego {");
    }

    #[test]
    fn un_json_limpio_pasa_intacto() {
        let limpio = r#"{"tldr":"x"}"#;
        assert_eq!(extraer_json(limpio), limpio);
    }

    #[test]
    fn solo_se_pasa_al_siguiente_proveedor_cuando_tiene_sentido() {
        // 429 y 5xx son ajenos a nosotros: probar otro proveedor puede salvar
        // la sesión.
        assert!(ProviderError::Cuota("x".into()).merece_respaldo());
        assert!(ProviderError::Http {
            proveedor: "x".into(),
            estado: 503,
            cuerpo: String::new()
        }
        .merece_respaldo());

        // Un 400 es culpa de la petición: reintentarla en otro proveedor daría
        // el mismo error y solo gastaría tiempo y dinero.
        assert!(!ProviderError::Http {
            proveedor: "x".into(),
            estado: 400,
            cuerpo: String::new()
        }
        .merece_respaldo());
        assert!(!ProviderError::Config("mal".into()).merece_respaldo());
    }
}
