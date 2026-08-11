//! Configuración de proveedores: `config/providers.toml`.
//!
//! Los identificadores de modelo cambian con frecuencia y cada proveedor usa su
//! nomenclatura. Por eso viven en un TOML y no en el código: añadir un
//! proveedor compatible es una entrada más en el archivo, no una versión nueva
//! de la aplicación.

use crate::{Capabilities, ProviderError, Result, StructuredMode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// Cubre OpenAI, DeepSeek, Groq, OpenRouter, Ollama, LM Studio y vLLM.
    OpenaiCompat,
    Gemini,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub kind: ProviderKind,
    #[serde(default)]
    pub base_url: Option<String>,
    pub model: String,
    /// Referencia al llavero del SO: "keyring:gemini". Nunca la clave en claro.
    #[serde(default)]
    pub api_key_ref: Option<String>,

    #[serde(default = "ctx_por_defecto")]
    pub context: usize,
    #[serde(default = "salida_por_defecto")]
    pub max_output: usize,
    #[serde(default = "estructurado_por_defecto")]
    pub structured: StructuredMode,
    #[serde(default)]
    pub accepts_audio: bool,
    #[serde(default)]
    pub accepts_images: bool,
    #[serde(default)]
    pub price_in: f64,
    #[serde(default)]
    pub price_out: f64,
}

fn ctx_por_defecto() -> usize {
    128_000
}
fn salida_por_defecto() -> usize {
    8_192
}
fn estructurado_por_defecto() -> StructuredMode {
    // Suponer el modo menos capaz es lo seguro: si el proveedor resulta ser
    // más listo, la validación en cliente no estorba. Al revés, se rompe.
    StructuredMode::JsonMode
}

impl ProviderConfig {
    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            context_window: self.context,
            max_output: self.max_output,
            structured_output: self.structured,
            accepts_audio: self.accepts_audio,
            accepts_images: self.accepts_images,
            price_in_per_mtok: self.price_in,
            price_out_per_mtok: self.price_out,
        }
    }

    pub fn es_local(&self) -> bool {
        self.base_url
            .as_deref()
            .map(|u| u.contains("localhost") || u.contains("127.0.0.1"))
            .unwrap_or(false)
    }
}

/// Tareas con enrutado propio. Cada una tiene una cadena de respaldo ordenada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Task {
    /// Se ejecuta cada 10 minutos durante la sesión: conviene barato.
    RollingSummary,
    /// Los apuntes de la clase.
    ClassNotes,
    /// El acta que puedes enviar a un cliente: conviene el mejor modelo.
    ClientMinutes,
    /// Actas de reuniones internas.
    TeamNotes,
    /// Chat con la asignatura.
    Chat,
    /// OCR de diapositivas: requiere modelo con visión.
    OcrSlides,
}

impl Task {
    pub fn clave(&self) -> &'static str {
        match self {
            Task::RollingSummary => "rolling_summary",
            Task::ClassNotes => "class_notes",
            Task::ClientMinutes => "client_minutes",
            Task::TeamNotes => "team_notes",
            Task::Chat => "chat",
            Task::OcrSlides => "ocr_slides",
        }
    }

    /// Si la tarea exige un modelo capaz de leer imágenes.
    pub fn necesita_vision(&self) -> bool {
        matches!(self, Task::OcrSlides)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoutingConfig {
    #[serde(flatten)]
    pub cadenas: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvidersFile {
    #[serde(default, rename = "provider")]
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub routing: RoutingConfig,
}

impl ProvidersFile {
    pub fn desde_toml(texto: &str) -> Result<Self> {
        let f: ProvidersFile =
            toml::from_str(texto).map_err(|e| ProviderError::Config(e.to_string()))?;
        f.validar()?;
        Ok(f)
    }

    fn validar(&self) -> Result<()> {
        if self.providers.is_empty() {
            return Err(ProviderError::Config(
                "no hay ningún proveedor definido".into(),
            ));
        }

        let mut vistos = std::collections::HashSet::new();
        for p in &self.providers {
            if !vistos.insert(&p.id) {
                return Err(ProviderError::Config(format!(
                    "el identificador de proveedor «{}» está repetido",
                    p.id
                )));
            }
            if p.kind == ProviderKind::OpenaiCompat && p.base_url.is_none() {
                return Err(ProviderError::Config(format!(
                    "«{}» es compatible-OpenAI y necesita `base_url`",
                    p.id
                )));
            }
        }

        // Una cadena que menciona un proveedor inexistente es un error de
        // configuración que, si no se detecta aquí, se manifestaría a mitad de
        // una clase de dos horas.
        for (tarea, cadena) in &self.routing.cadenas {
            for id in cadena {
                if !self.providers.iter().any(|p| &p.id == id) {
                    return Err(ProviderError::Config(format!(
                        "la tarea «{tarea}» enruta a «{id}», que no está definido"
                    )));
                }
            }
        }

        Ok(())
    }

    pub fn proveedor(&self, id: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// Cadena de respaldo de una tarea. Si no hay entrada explícita, se usan
    /// todos los proveedores en el orden en que aparecen en el archivo.
    pub fn cadena(&self, tarea: Task) -> Vec<&ProviderConfig> {
        let ids = self.routing.cadenas.get(tarea.clave());

        let candidatos: Vec<&ProviderConfig> = match ids {
            Some(lista) => lista.iter().filter_map(|id| self.proveedor(id)).collect(),
            None => self.providers.iter().collect(),
        };

        // Un modelo sin visión en la cadena de OCR no fallaría con un error
        // claro: describiría la imagen sin verla. Mejor descartarlo.
        if tarea.necesita_vision() {
            candidatos
                .into_iter()
                .filter(|p| p.accepts_images)
                .collect()
        } else {
            candidatos
        }
    }
}

/// Configuración por defecto, para el primer arranque.
///
/// Deja Ollama en local al final de cada cadena: si el usuario no ha puesto
/// ninguna clave, la aplicación sigue funcionando sin conexión ni coste.
pub const PLANTILLA_TOML: &str = include_str!("../../../config/providers.toml");

#[cfg(test)]
mod tests {
    use super::*;

    const EJEMPLO: &str = r#"
[[provider]]
id = "gemini-flash"
kind = "gemini"
model = "gemini-2.5-flash"
api_key_ref = "keyring:gemini"
context = 1000000
structured = "native"
accepts_audio = true
accepts_images = true
price_in = 0.30
price_out = 2.50

[[provider]]
id = "deepseek"
kind = "openai_compat"
base_url = "https://api.deepseek.com/v1"
model = "deepseek-chat"
api_key_ref = "keyring:deepseek"
context = 128000
structured = "json_mode"
price_in = 0.27
price_out = 1.10

[[provider]]
id = "local"
kind = "openai_compat"
base_url = "http://localhost:11434/v1"
model = "qwen3:14b"

[routing]
class_notes = ["gemini-flash", "deepseek", "local"]
ocr_slides = ["gemini-flash", "deepseek"]
"#;

    #[test]
    fn se_lee_la_configuracion_y_se_calculan_las_capacidades() {
        let f = ProvidersFile::desde_toml(EJEMPLO).unwrap();
        assert_eq!(f.providers.len(), 3);

        let g = f.proveedor("gemini-flash").unwrap();
        assert_eq!(g.capabilities().structured_output, StructuredMode::Native);
        assert!(g.capabilities().accepts_images);
    }

    #[test]
    fn un_proveedor_sin_modo_declarado_se_supone_el_menos_capaz() {
        // Suponer que soporta esquema estricto cuando no lo hace rompería en
        // ejecución; al revés solo sobra una validación en cliente.
        let f = ProvidersFile::desde_toml(EJEMPLO).unwrap();
        let local = f.proveedor("local").unwrap();
        assert_eq!(local.structured, StructuredMode::JsonMode);
        assert!(local.es_local());
    }

    #[test]
    fn la_cadena_de_respaldo_respeta_el_orden_configurado() {
        let f = ProvidersFile::desde_toml(EJEMPLO).unwrap();
        let ids: Vec<&str> = f
            .cadena(Task::ClassNotes)
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        assert_eq!(ids, vec!["gemini-flash", "deepseek", "local"]);
    }

    #[test]
    fn sin_cadena_explicita_se_usan_todos_en_orden_de_archivo() {
        let f = ProvidersFile::desde_toml(EJEMPLO).unwrap();
        assert_eq!(f.cadena(Task::Chat).len(), 3);
    }

    #[test]
    fn el_ocr_descarta_los_modelos_sin_vision() {
        // Un modelo sin visión no fallaría con un error claro: describiría la
        // diapositiva sin haberla visto.
        let f = ProvidersFile::desde_toml(EJEMPLO).unwrap();
        let ids: Vec<&str> = f
            .cadena(Task::OcrSlides)
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        assert_eq!(ids, vec!["gemini-flash"], "deepseek no tiene visión");
    }

    #[test]
    fn una_cadena_que_apunta_a_un_proveedor_inexistente_se_rechaza_al_cargar() {
        // Este error debe salir al guardar la configuración, no a mitad de una
        // clase de dos horas.
        let malo = format!("{EJEMPLO}\nchat = [\"no-existe\"]");
        let err = ProvidersFile::desde_toml(&malo).unwrap_err();
        assert!(err.to_string().contains("no-existe"));
    }

    #[test]
    fn un_compatible_openai_sin_base_url_se_rechaza() {
        let malo = r#"
[[provider]]
id = "roto"
kind = "openai_compat"
model = "x"
"#;
        assert!(ProvidersFile::desde_toml(malo).is_err());
    }

    #[test]
    fn los_identificadores_repetidos_se_rechazan() {
        let malo = r#"
[[provider]]
id = "dup"
kind = "openai_compat"
base_url = "http://a"
model = "x"

[[provider]]
id = "dup"
kind = "openai_compat"
base_url = "http://b"
model = "y"
"#;
        assert!(ProvidersFile::desde_toml(malo).is_err());
    }

    #[test]
    fn la_plantilla_que_se_distribuye_es_valida() {
        // Si la plantilla por defecto no carga, la aplicación no arranca en un
        // equipo nuevo.
        let f = ProvidersFile::desde_toml(PLANTILLA_TOML).unwrap();
        assert!(!f.providers.is_empty());
        assert!(
            f.providers.iter().any(|p| p.es_local()),
            "debe haber una opción local para funcionar sin claves ni conexión"
        );
    }
}
