//! Enrutado con cadena de respaldo.
//!
//! Cada tarea tiene una lista ordenada de proveedores. Si el primero devuelve
//! 429, 5xx o agota el tiempo, se pasa al siguiente automáticamente: los
//! apuntes de una clase de dos horas no se pierden porque un proveedor tuviera
//! un mal minuto.

use crate::config::{ProviderConfig, ProviderKind, ProvidersFile, Task};
use crate::gemini::Gemini;
use crate::openai_compat::OpenAiCompat;
use crate::{completar_estructurado, CompletionRequest, LlmProvider, ProviderError, Result, Usage};
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::sync::Arc;

// La resolución de claves vive en `secretos`, que conoce las variables de
// entorno, el archivo `.env` y —cuando entre— el llavero del sistema. Aquí se
// reexporta para no romper a quien importe desde `router`.
pub use crate::secretos::{
    resolver_por_defecto, CadenaResolvers, DotEnvResolver, EnvResolver, KeyResolver, MapResolver,
    Origen,
};

pub struct Router {
    proveedores: HashMap<String, Arc<dyn LlmProvider>>,
    cadenas: HashMap<Task, Vec<String>>,
}

impl Router {
    pub fn desde_config(cfg: &ProvidersFile, claves: &dyn KeyResolver) -> Result<Self> {
        let mut proveedores: HashMap<String, Arc<dyn LlmProvider>> = HashMap::new();

        for p in &cfg.providers {
            match construir(p, claves) {
                Some(prov) => {
                    proveedores.insert(p.id.clone(), prov);
                }
                None => {
                    // Un proveedor sin clave no es un error: el usuario puede
                    // haber configurado solo uno. Se omite y la cadena sigue
                    // hasta el respaldo local, que no necesita clave.
                    tracing::info!(
                        proveedor = %p.id,
                        "omitido: no hay clave de API configurada"
                    );
                }
            }
        }

        if proveedores.is_empty() {
            return Err(ProviderError::Config(
                "ningún proveedor utilizable: falta configurar al menos una clave \
                 de API, o tener Ollama en local"
                    .into(),
            ));
        }

        let mut cadenas = HashMap::new();
        for tarea in [
            Task::RollingSummary,
            Task::ClassNotes,
            Task::ClientMinutes,
            Task::TeamNotes,
            Task::Chat,
            Task::OcrSlides,
        ] {
            let ids: Vec<String> = cfg
                .cadena(tarea)
                .iter()
                .map(|p| p.id.clone())
                .filter(|id| proveedores.contains_key(id))
                .collect();
            cadenas.insert(tarea, ids);
        }

        Ok(Self {
            proveedores,
            cadenas,
        })
    }

    /// Router con un único proveedor, para probarlo aislado.
    ///
    /// Sin esto, comprobar una clave usaría la cadena de respaldo normal y un
    /// fallo del proveedor quedaría tapado por el siguiente: el usuario vería
    /// «funciona» con una clave que no vale.
    pub fn desde_config_filtrado(
        cfg: &ProvidersFile,
        claves: &dyn KeyResolver,
        solo: &str,
    ) -> Result<Self> {
        let p = cfg
            .proveedor(solo)
            .ok_or_else(|| ProviderError::Config(format!("no existe el proveedor «{solo}»")))?;

        let prov = construir(p, claves).ok_or_else(|| {
            ProviderError::Config(format!(
                "«{solo}» no tiene clave configurada, así que no hay nada que probar"
            ))
        })?;

        let mut cadenas = HashMap::new();
        for tarea in [
            Task::RollingSummary,
            Task::ClassNotes,
            Task::ClientMinutes,
            Task::TeamNotes,
            Task::Chat,
            Task::OcrSlides,
        ] {
            cadenas.insert(tarea, vec![solo.to_owned()]);
        }

        Ok(Self::con_proveedores(vec![prov], cadenas))
    }

    /// Completado de texto libre, sin esquema.
    ///
    /// Para el chat y para la comprobación de proveedores, donde no hace falta
    /// una estructura y forzarla solo complicaría la prueba.
    pub async fn texto(&self, tarea: Task, req: CompletionRequest) -> Result<String> {
        let cadena = self.cadena(tarea);
        let mut fallos = Vec::new();

        for id in cadena {
            let Some(prov) = self.proveedores.get(id) else {
                continue;
            };
            match prov.completar(&req).await {
                Ok(r) => return Ok(r.texto),
                Err(e) => {
                    let seguir = e.merece_respaldo();
                    fallos.push(format!("{id}: {e}"));
                    if !seguir {
                        return Err(ProviderError::CadenaAgotada(fallos.join(" | ")));
                    }
                }
            }
        }

        Err(ProviderError::CadenaAgotada(fallos.join(" | ")))
    }

    /// Construye un router sobre proveedores ya instanciados. Para tests y para
    /// el modo "solo local" de una sesión confidencial.
    pub fn con_proveedores(
        proveedores: Vec<Arc<dyn LlmProvider>>,
        cadenas: HashMap<Task, Vec<String>>,
    ) -> Self {
        Self {
            proveedores: proveedores
                .into_iter()
                .map(|p| (p.id().to_owned(), p))
                .collect(),
            cadenas,
        }
    }

    pub fn cadena(&self, tarea: Task) -> &[String] {
        self.cadenas
            .get(&tarea)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Ejecuta la tarea recorriendo la cadena hasta que alguien responda.
    ///
    /// Devuelve también el identificador del proveedor que acabó respondiendo,
    /// porque se guarda junto a las notas: saber con qué modelo se generó un
    /// documento es imprescindible para comparar calidad entre versiones.
    pub async fn estructurado<T>(
        &self,
        tarea: Task,
        req: CompletionRequest,
    ) -> Result<(T, Usage, String)>
    where
        T: DeserializeOwned + schemars::JsonSchema,
    {
        let cadena = self.cadena(tarea);
        if cadena.is_empty() {
            return Err(ProviderError::SinProveedor(tarea.clave().to_owned()));
        }

        let mut fallos = Vec::new();

        for id in cadena {
            let Some(prov) = self.proveedores.get(id) else {
                continue;
            };

            match completar_estructurado::<T>(prov.as_ref(), req.clone()).await {
                Ok((valor, uso)) => {
                    if !fallos.is_empty() {
                        tracing::info!(
                            proveedor = %id,
                            tras_fallar = fallos.len(),
                            "la cadena de respaldo salvó la petición"
                        );
                    }
                    return Ok((valor, uso, id.clone()));
                }
                Err(e) => {
                    let seguir = e.merece_respaldo();
                    tracing::warn!(proveedor = %id, error = %e, seguir, "fallo del proveedor");
                    fallos.push(format!("{id}: {e}"));

                    // Un 400 o una configuración mala darían el mismo error en
                    // todos: seguir la cadena solo gastaría tiempo y dinero.
                    if !seguir {
                        return Err(ProviderError::CadenaAgotada(fallos.join(" | ")));
                    }
                }
            }
        }

        Err(ProviderError::CadenaAgotada(fallos.join(" | ")))
    }
}

fn construir(p: &ProviderConfig, claves: &dyn KeyResolver) -> Option<Arc<dyn LlmProvider>> {
    let clave = p.api_key_ref.as_deref().and_then(|r| claves.resolver(r));

    match p.kind {
        ProviderKind::Gemini => {
            // Gemini siempre necesita clave; sin ella no se instancia.
            Some(Arc::new(Gemini::nuevo(
                &p.id,
                &p.model,
                clave?,
                p.capabilities(),
            )))
        }
        ProviderKind::OpenaiCompat => {
            // Ollama y vLLM en local no usan clave: exigirla dejaría al usuario
            // sin el respaldo que funciona sin conexión.
            if clave.is_none() && !p.es_local() {
                return None;
            }
            Some(Arc::new(OpenAiCompat::nuevo(
                &p.id,
                p.base_url.as_deref()?,
                &p.model,
                clave,
                p.capabilities(),
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Capabilities, LlmProvider, Message, RawCompletion, StructuredMode};
    use async_trait::async_trait;
    use schemars::JsonSchema;
    use serde::Deserialize;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[derive(Debug, Deserialize, JsonSchema, PartialEq)]
    struct Respuesta {
        tldr: String,
    }

    /// Proveedor de prueba con comportamiento programable.
    struct Falso {
        id: String,
        resultado: Box<dyn Fn(u32) -> Result<String> + Send + Sync>,
        llamadas: AtomicU32,
        caps: Capabilities,
    }

    impl Falso {
        fn nuevo(id: &str, f: impl Fn(u32) -> Result<String> + Send + Sync + 'static) -> Arc<Self> {
            Arc::new(Self {
                id: id.to_owned(),
                resultado: Box::new(f),
                llamadas: AtomicU32::new(0),
                caps: Capabilities {
                    context_window: 128_000,
                    max_output: 4096,
                    structured_output: StructuredMode::Native,
                    accepts_audio: false,
                    accepts_images: false,
                    price_in_per_mtok: 1.0,
                    price_out_per_mtok: 1.0,
                },
            })
        }
        fn llamadas(&self) -> u32 {
            self.llamadas.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl LlmProvider for Falso {
        fn id(&self) -> &str {
            &self.id
        }
        fn modelo(&self) -> &str {
            "falso"
        }
        fn capabilities(&self) -> &Capabilities {
            &self.caps
        }

        async fn completar(&self, _: &CompletionRequest) -> Result<RawCompletion> {
            let n = self.llamadas.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(RawCompletion {
                texto: (self.resultado)(n)?,
                usage: Usage {
                    tokens_in: 100,
                    tokens_out: 50,
                    cost_usd: 0.0,
                    latency_ms: 1,
                },
            })
        }
    }

    fn router(provs: Vec<Arc<dyn LlmProvider>>) -> Router {
        let ids: Vec<String> = provs.iter().map(|p| p.id().to_owned()).collect();
        let mut cadenas = HashMap::new();
        cadenas.insert(Task::ClassNotes, ids);
        Router::con_proveedores(provs, cadenas)
    }

    fn peticion() -> CompletionRequest {
        CompletionRequest::nueva(vec![Message::usuario("resume")])
    }

    #[tokio::test]
    async fn el_camino_feliz_usa_el_primero_de_la_cadena() {
        let a = Falso::nuevo("a", |_| Ok(r#"{"tldr":"desde a"}"#.to_owned()));
        let b = Falso::nuevo("b", |_| Ok(r#"{"tldr":"desde b"}"#.to_owned()));
        let r = router(vec![a.clone(), b.clone()]);

        let (v, _, quien): (Respuesta, _, _) =
            r.estructurado(Task::ClassNotes, peticion()).await.unwrap();

        assert_eq!(v.tldr, "desde a");
        assert_eq!(quien, "a");
        assert_eq!(
            b.llamadas(),
            0,
            "no debe molestar al segundo si el primero responde"
        );
    }

    #[tokio::test]
    async fn ante_un_429_se_pasa_al_siguiente_proveedor() {
        let a = Falso::nuevo("a", |_| Err(ProviderError::Cuota("a".into())));
        let b = Falso::nuevo("b", |_| Ok(r#"{"tldr":"rescatado"}"#.to_owned()));
        let r = router(vec![a.clone(), b.clone()]);

        let (v, _, quien): (Respuesta, _, _) =
            r.estructurado(Task::ClassNotes, peticion()).await.unwrap();

        assert_eq!(v.tldr, "rescatado");
        assert_eq!(quien, "b");
    }

    #[tokio::test]
    async fn un_400_corta_la_cadena_en_lugar_de_gastar_en_todos() {
        // La petición está mal construida: daría el mismo error en todos.
        let a = Falso::nuevo("a", |_| {
            Err(ProviderError::Http {
                proveedor: "a".into(),
                estado: 400,
                cuerpo: "mal".into(),
            })
        });
        let b = Falso::nuevo("b", |_| Ok(r#"{"tldr":"x"}"#.to_owned()));
        let r = router(vec![a.clone(), b.clone()]);

        let res: Result<(Respuesta, _, _)> = r.estructurado(Task::ClassNotes, peticion()).await;
        assert!(res.is_err());
        assert_eq!(b.llamadas(), 0, "no debe intentarlo con el siguiente");
    }

    #[tokio::test]
    async fn una_salida_mal_formada_se_reintenta_con_el_error_inyectado() {
        // El primer intento devuelve un JSON que no cumple el tipo; el segundo,
        // ya con el error de validación en la conversación, acierta.
        let a = Falso::nuevo("a", |n| {
            Ok(if n == 1 {
                r#"{"resumen":"campo equivocado"}"#.to_owned()
            } else {
                r#"{"tldr":"corregido"}"#.to_owned()
            })
        });
        let r = router(vec![a.clone()]);

        let (v, uso, _): (Respuesta, _, _) =
            r.estructurado(Task::ClassNotes, peticion()).await.unwrap();

        assert_eq!(v.tldr, "corregido");
        assert_eq!(a.llamadas(), 2);
        // El uso de ambos intentos se acumula: el coste real incluye el fallo.
        assert_eq!(uso.tokens_in, 200);
    }

    #[tokio::test]
    async fn se_rescata_una_respuesta_envuelta_en_vallas_de_codigo() {
        let a = Falso::nuevo("a", |_| {
            Ok("```json\n{\"tldr\":\"con vallas\"}\n```".to_owned())
        });
        let r = router(vec![a.clone()]);

        let (v, _, _): (Respuesta, _, _) =
            r.estructurado(Task::ClassNotes, peticion()).await.unwrap();

        assert_eq!(v.tldr, "con vallas");
        assert_eq!(a.llamadas(), 1, "no debe hacer falta reintentar");
    }

    #[tokio::test]
    async fn tras_agotar_los_reintentos_se_pasa_al_siguiente_proveedor() {
        let a = Falso::nuevo("a", |_| Ok(r#"{"mal":"siempre"}"#.to_owned()));
        let b = Falso::nuevo("b", |_| Ok(r#"{"tldr":"ok"}"#.to_owned()));
        let r = router(vec![a.clone(), b.clone()]);

        let (v, _, quien): (Respuesta, _, _) =
            r.estructurado(Task::ClassNotes, peticion()).await.unwrap();

        assert_eq!(quien, "b");
        assert_eq!(v.tldr, "ok");
        assert_eq!(a.llamadas(), 3, "tres intentos antes de rendirse");
    }

    #[tokio::test]
    async fn sin_proveedores_para_la_tarea_el_error_lo_dice() {
        let r = Router::con_proveedores(vec![], HashMap::new());
        let res: Result<(Respuesta, _, _)> = r.estructurado(Task::Chat, peticion()).await;
        assert!(matches!(res, Err(ProviderError::SinProveedor(_))));
    }

    #[test]
    fn un_proveedor_local_se_construye_sin_clave() {
        // Es el respaldo que funciona sin conexión ni coste: exigirle clave
        // dejaría al usuario sin nada cuando no ha configurado ninguna.
        let cfg = ProvidersFile::desde_toml(
            r#"
[[provider]]
id = "local"
kind = "openai_compat"
base_url = "http://localhost:11434/v1"
model = "qwen3:14b"
"#,
        )
        .unwrap();

        let router = Router::desde_config(&cfg, &MapResolver::default()).unwrap();
        assert_eq!(router.cadena(Task::ClassNotes), &["local".to_owned()]);
    }

    #[test]
    fn los_proveedores_sin_clave_se_omiten_de_las_cadenas() {
        let cfg = ProvidersFile::desde_toml(
            r#"
[[provider]]
id = "gemini-flash"
kind = "gemini"
model = "gemini-2.5-flash"
api_key_ref = "keyring:gemini"

[[provider]]
id = "local"
kind = "openai_compat"
base_url = "http://localhost:11434/v1"
model = "qwen3:14b"

[routing]
class_notes = ["gemini-flash", "local"]
"#,
        )
        .unwrap();

        // Sin clave de Gemini configurada, la cadena cae al respaldo local en
        // lugar de fallar.
        let router = Router::desde_config(&cfg, &MapResolver::default()).unwrap();
        assert_eq!(router.cadena(Task::ClassNotes), &["local".to_owned()]);

        // Con la clave puesta, vuelve a estar disponible.
        let mut mapa = HashMap::new();
        mapa.insert("keyring:gemini".to_owned(), "clave".to_owned());
        let router = Router::desde_config(&cfg, &MapResolver(mapa)).unwrap();
        assert_eq!(router.cadena(Task::ClassNotes).len(), 2);
    }
}
