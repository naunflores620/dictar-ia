//! Generación de apuntes y actas: el map-reduce sobre la transcripción.
//!
//! Tres niveles, según el momento:
//!
//! 1. **Rodante** — cada ~10 min durante la sesión. Coste marginal, permite ver
//!    de qué se ha hablado sin parar la clase.
//! 2. **Map** — al terminar, cada bloque se extrae por separado y en paralelo.
//! 3. **Reduce** — los extractos se consolidan en el documento final.
//!
//! Mandar dos horas de transcripción en una sola llamada degradaría el
//! resultado aunque cupiera en el contexto: el modelo pierde el detalle del
//! medio y devuelve algo genérico.

pub mod chunking;
pub mod fecha;
pub mod markdown;
pub mod prompts;

use chunking::Bloque;
use dictar_domain::notes::NotesPayload;
use dictar_domain::{
    ClassNotes, ClientMinutes, NoteTemplate, RollingSummary, SessionKind, Slide, TeamNotes,
    Utterance,
};
use dictar_providers::config::Task;
use dictar_providers::router::Router;
use dictar_providers::{CompletionRequest, Message, ProviderError, Usage};
use futures::stream::{self, StreamExt};
use serde::de::DeserializeOwned;
use serde::Serialize;

/// Peticiones de *map* simultáneas.
///
/// Tres es un equilibrio: aprovecha el paralelismo sin provocar el 429 que
/// obligaría a recorrer la cadena de respaldo entera.
const MAP_CONCURRENTE: usize = 3;

#[derive(Debug, thiserror::Error)]
pub enum NotesError {
    #[error(transparent)]
    Proveedor(#[from] ProviderError),

    #[error("no hay transcripción que resumir")]
    SinTranscripcion,

    #[error("error de serialización: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, NotesError>;

/// Todo lo que el modelo necesita saber de la sesión, aparte del texto.
#[derive(Debug, Clone, Default)]
pub struct SessionContext {
    pub kind: SessionKindWrapper,
    /// "Cálculo II — Dra. Pérez" o "Acme S.A. — Juan Ruiz".
    pub contexto: String,
    pub duracion_ms: i64,
    /// Fecha de la sesión en ISO-8601.
    ///
    /// Imprescindible: sin ella el modelo resuelve «el parcial es el 15 de
    /// septiembre» inventándose el año, y una fecha de examen equivocada es
    /// peor que ninguna fecha porque el usuario se fía de ella.
    pub fecha: String,
    /// Vocabulario acumulado del `topic`.
    pub glosario: String,
    pub diapositivas: Vec<Slide>,
}

/// Envoltorio para poder derivar `Default` sobre `SessionContext`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionKindWrapper(pub SessionKind);

impl Default for SessionKindWrapper {
    fn default() -> Self {
        Self(SessionKind::TeamMeeting)
    }
}

impl From<SessionKind> for SessionKindWrapper {
    fn from(k: SessionKind) -> Self {
        Self(k)
    }
}

impl SessionContext {
    pub fn plantilla(&self) -> NoteTemplate {
        self.kind.0.plantilla()
    }

    fn tarea(&self) -> Task {
        match self.plantilla() {
            NoteTemplate::Class => Task::ClassNotes,
            NoteTemplate::Client => Task::ClientMinutes,
            NoteTemplate::Team => Task::TeamNotes,
        }
    }

    /// Fecha de la sesión, o la de hoy si no se indicó.
    fn fecha_o_hoy(&self) -> String {
        if self.fecha.is_empty() {
            fecha::hoy()
        } else {
            self.fecha.clone()
        }
    }

    fn duracion_humana(&self) -> String {
        let m = self.duracion_ms / 60_000;
        if m >= 60 {
            format!("{}h {}min", m / 60, m % 60)
        } else {
            format!("{m} min")
        }
    }

    /// Texto OCR de las diapositivas, para que el modelo pueda corregir la
    /// terminología: lo escrito manda sobre lo oído.
    fn bloque_diapositivas(&self, inicio_ms: i64, fin_ms: i64) -> String {
        let visibles: Vec<String> = self
            .diapositivas
            .iter()
            .filter(|s| s.ts_ms >= inicio_ms && s.ts_ms <= fin_ms)
            .filter_map(|s| {
                s.ocr_text
                    .as_ref()
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| format!("- slide_id {} (ts {}): {}", s.id, s.ts_ms, t.trim()))
            })
            .collect();

        if visibles.is_empty() {
            String::new()
        } else {
            format!(
                "\nDIAPOSITIVAS VISIBLES EN ESTE TRAMO (el texto OCR es la fuente MÁS \
                 FIABLE de la terminología exacta: si la transcripción dice «de la \
                 place» y la diapositiva dice «Laplace», usa la diapositiva):\n{}\n",
                visibles.join("\n")
            )
        }
    }
}

#[derive(Debug, Clone)]
pub struct Generadas {
    pub payload: NotesPayload,
    pub usage: Usage,
    /// Proveedor que acabó respondiendo. Se guarda con las notas: saber con qué
    /// modelo se generó un documento es imprescindible para comparar calidad.
    pub proveedor: String,
    pub prompt_ver: &'static str,
}

pub struct NotesGenerator<'a> {
    router: &'a Router,
}

impl<'a> NotesGenerator<'a> {
    pub fn nuevo(router: &'a Router) -> Self {
        Self { router }
    }

    /// Documento final: trocea, extrae en paralelo y consolida.
    pub async fn generar(&self, ctx: &SessionContext, frases: &[Utterance]) -> Result<Generadas> {
        if frases.is_empty() {
            return Err(NotesError::SinTranscripcion);
        }

        let bloques = chunking::trocear(frases, chunking::TOKENS_POR_BLOQUE);
        tracing::info!(bloques = bloques.len(), "iniciando map-reduce de notas");

        match ctx.plantilla() {
            NoteTemplate::Class => self
                .map_reduce::<ClassNotes>(ctx, &bloques, prompts::REDUCE_CLASE)
                .await
                .map(|(p, u, q)| Generadas {
                    payload: NotesPayload::Class(p),
                    usage: u,
                    proveedor: q,
                    prompt_ver: prompts::VERSION,
                }),
            NoteTemplate::Client => self
                .map_reduce::<ClientMinutes>(ctx, &bloques, prompts::REDUCE_CLIENTE)
                .await
                .map(|(p, u, q)| Generadas {
                    payload: NotesPayload::Client(p),
                    usage: u,
                    proveedor: q,
                    prompt_ver: prompts::VERSION,
                }),
            NoteTemplate::Team => self
                .map_reduce::<TeamNotes>(ctx, &bloques, prompts::REDUCE_EQUIPO)
                .await
                .map(|(p, u, q)| Generadas {
                    payload: NotesPayload::Team(p),
                    usage: u,
                    proveedor: q,
                    prompt_ver: prompts::VERSION,
                }),
        }
    }

    async fn map_reduce<T>(
        &self,
        ctx: &SessionContext,
        bloques: &[Bloque],
        plantilla_reduce: &str,
    ) -> Result<(T, Usage, String)>
    where
        T: DeserializeOwned + Serialize + schemars::JsonSchema + Send + 'static,
    {
        let total = bloques.len();

        // -- Map: cada bloque por separado, en paralelo y acotado ------------
        let extractos: Vec<(usize, T, Usage)> = stream::iter(bloques.iter())
            .map(|b| async move {
                let prompt = prompts::render(
                    prompts::MAP,
                    &[
                        ("tipo", ctx.plantilla().nombre_humano()),
                        ("contexto", &ctx.contexto),
                        ("fecha", &ctx.fecha_o_hoy()),
                        (
                            "glosario",
                            if ctx.glosario.is_empty() {
                                "(vacío)"
                            } else {
                                &ctx.glosario
                            },
                        ),
                        (
                            "diapositivas",
                            &ctx.bloque_diapositivas(b.inicio_ms, b.fin_ms),
                        ),
                        ("bloque", &(b.indice + 1).to_string()),
                        ("total", &total.to_string()),
                        ("inicio", &b.inicio_ms.to_string()),
                        ("fin", &b.fin_ms.to_string()),
                        ("transcripcion", &b.texto),
                    ],
                );

                let req = CompletionRequest::nueva(vec![
                    Message::sistema(prompts::SISTEMA),
                    Message::usuario(prompt),
                ]);

                let r = self.router.estructurado::<T>(ctx.tarea(), req).await;
                (b.indice, r)
            })
            .buffer_unordered(MAP_CONCURRENTE)
            .filter_map(|(idx, r)| async move {
                match r {
                    Ok((v, u, _)) => Some((idx, v, u)),
                    Err(e) => {
                        // Perder un bloque de seis degrada las notas; perder la
                        // sesión entera por un 429 en un bloque sería peor.
                        tracing::warn!(bloque = idx, error = %e, "bloque descartado");
                        None
                    }
                }
            })
            .collect()
            .await;

        if extractos.is_empty() {
            return Err(NotesError::Proveedor(ProviderError::CadenaAgotada(
                "ningún bloque pudo extraerse".into(),
            )));
        }

        if extractos.len() < total {
            tracing::warn!(
                obtenidos = extractos.len(),
                total,
                "faltan bloques en el documento final"
            );
        }

        // Reordenar: `buffer_unordered` los devuelve según terminan, y el orden
        // cronológico importa para que el reduce entienda la progresión.
        let mut extractos = extractos;
        extractos.sort_by_key(|(i, _, _)| *i);

        let mut usage = Usage::default();
        for (_, _, u) in &extractos {
            usage.tokens_in += u.tokens_in;
            usage.tokens_out += u.tokens_out;
            usage.cost_usd += u.cost_usd;
            usage.latency_ms += u.latency_ms;
        }

        // -- Reduce ----------------------------------------------------------
        let json_extractos =
            serde_json::to_string_pretty(&extractos.iter().map(|(_, v, _)| v).collect::<Vec<_>>())?;

        let prompt = prompts::render(
            plantilla_reduce,
            &[
                ("duracion", &ctx.duracion_humana()),
                ("contexto", &ctx.contexto),
                ("fecha", &ctx.fecha_o_hoy()),
                ("extractos", &json_extractos),
                ("diapositivas", &ctx.bloque_diapositivas(0, i64::MAX)),
            ],
        );

        let req = CompletionRequest::nueva(vec![
            Message::sistema(prompts::SISTEMA),
            Message::usuario(prompt),
        ]);

        let (final_, uso_reduce, proveedor) =
            self.router.estructurado::<T>(ctx.tarea(), req).await?;

        usage.tokens_in += uso_reduce.tokens_in;
        usage.tokens_out += uso_reduce.tokens_out;
        usage.cost_usd += uso_reduce.cost_usd;
        usage.latency_ms += uso_reduce.latency_ms;

        Ok((final_, usage, proveedor))
    }

    /// Resumen rodante, durante la sesión.
    ///
    /// Usa la cadena de la tarea `RollingSummary`, que apunta a lo barato: se
    /// ejecuta cada diez minutos y su resultado es provisional.
    pub async fn rodante(
        &self,
        previo: Option<&str>,
        frases: &[Utterance],
    ) -> Result<(RollingSummary, Usage, String)> {
        if frases.is_empty() {
            return Err(NotesError::SinTranscripcion);
        }

        let inicio = frases.first().map(|f| f.start_ms).unwrap_or(0);
        let fin = frases.last().map(|f| f.end_ms).unwrap_or(0);

        let prompt = prompts::render(
            prompts::RODANTE,
            &[
                ("previo", previo.unwrap_or("(todavía no hay notas)")),
                ("inicio", &inicio.to_string()),
                ("fin", &fin.to_string()),
                ("transcripcion", &chunking::formatear(frases)),
            ],
        );

        let req = CompletionRequest::nueva(vec![
            Message::sistema(prompts::SISTEMA),
            Message::usuario(prompt),
        ]);

        Ok(self
            .router
            .estructurado::<RollingSummary>(Task::RollingSummary, req)
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use dictar_domain::{SessionId, Track};
    use dictar_providers::{
        Capabilities, CompletionRequest, LlmProvider, RawCompletion, StructuredMode,
    };
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    /// Proveedor de prueba que devuelve JSON fijo y registra los prompts que
    /// recibe, para poder comprobar qué se le pidió exactamente.
    struct Espia {
        respuesta: String,
        prompts: Mutex<Vec<String>>,
        llamadas: AtomicU32,
        caps: Capabilities,
    }

    impl Espia {
        fn nuevo(respuesta: &str) -> Arc<Self> {
            Arc::new(Self {
                respuesta: respuesta.to_owned(),
                prompts: Mutex::new(Vec::new()),
                llamadas: AtomicU32::new(0),
                caps: Capabilities {
                    context_window: 128_000,
                    max_output: 8192,
                    structured_output: StructuredMode::Native,
                    accepts_audio: false,
                    accepts_images: false,
                    price_in_per_mtok: 0.30,
                    price_out_per_mtok: 2.50,
                },
            })
        }
        fn prompts(&self) -> Vec<String> {
            self.prompts.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl LlmProvider for Espia {
        fn id(&self) -> &str {
            "espia"
        }
        fn modelo(&self) -> &str {
            "espia-1"
        }
        fn capabilities(&self) -> &Capabilities {
            &self.caps
        }

        async fn completar(
            &self,
            req: &CompletionRequest,
        ) -> dictar_providers::Result<RawCompletion> {
            self.llamadas.fetch_add(1, Ordering::SeqCst);
            self.prompts
                .lock()
                .unwrap()
                .push(req.messages.last().unwrap().content.clone());
            Ok(RawCompletion {
                texto: self.respuesta.clone(),
                usage: Usage {
                    tokens_in: 1000,
                    tokens_out: 200,
                    cost_usd: 0.0005,
                    latency_ms: 10,
                },
            })
        }
    }

    fn router_con(espia: Arc<Espia>) -> Router {
        let mut cadenas = HashMap::new();
        for t in [
            Task::ClassNotes,
            Task::ClientMinutes,
            Task::TeamNotes,
            Task::RollingSummary,
        ] {
            cadenas.insert(t, vec!["espia".to_owned()]);
        }
        Router::con_proveedores(vec![espia], cadenas)
    }

    fn frases(n: usize, palabras: usize) -> Vec<Utterance> {
        (0..n)
            .map(|i| Utterance {
                id: i as i64,
                session_id: SessionId::from("s"),
                track: if i % 2 == 0 {
                    Track::System
                } else {
                    Track::Mic
                },
                speaker: None,
                start_ms: (i as i64) * 5_000,
                end_ms: (i as i64) * 5_000 + 4_500,
                text: vec!["palabra"; palabras].join(" "),
                confidence: Some(0.9),
                is_final: true,
                revision: 1,
            })
            .collect()
    }

    const APUNTES_MINIMOS: &str = r#"{
        "tldr": "Clase sobre transformadas de Laplace.",
        "temas": [], "conceptos": [], "avisos": [], "senales_examen": []
    }"#;

    #[tokio::test]
    async fn genera_apuntes_de_clase_con_map_y_reduce() {
        let espia = Espia::nuevo(APUNTES_MINIMOS);
        let router = router_con(espia.clone());
        let gen = NotesGenerator::nuevo(&router);

        let ctx = SessionContext {
            kind: SessionKind::Class.into(),
            contexto: "Cálculo II — Dra. Pérez".into(),
            duracion_ms: 5_400_000,
            fecha: "2026-08-11".into(),
            glosario: "Laplace, Fourier".into(),
            diapositivas: vec![],
        };

        let g = gen.generar(&ctx, &frases(200, 40)).await.unwrap();

        assert_eq!(g.payload.plantilla(), NoteTemplate::Class);
        assert_eq!(g.payload.tldr(), "Clase sobre transformadas de Laplace.");
        assert_eq!(g.prompt_ver, prompts::VERSION);

        // Varios bloques de map más una llamada de reduce.
        let n = espia.llamadas.load(Ordering::SeqCst);
        assert!(
            n >= 3,
            "esperaba varias llamadas de map más el reduce, hubo {n}"
        );

        // El coste acumula todas las llamadas, no solo la última.
        assert!(g.usage.tokens_in >= 3000);
    }

    #[tokio::test]
    async fn el_glosario_llega_a_los_prompts_de_map() {
        let espia = Espia::nuevo(APUNTES_MINIMOS);
        let router = router_con(espia.clone());
        let gen = NotesGenerator::nuevo(&router);

        let ctx = SessionContext {
            kind: SessionKind::Class.into(),
            contexto: "Cálculo II".into(),
            duracion_ms: 600_000,
            fecha: "2026-08-11".into(),
            glosario: "transformada de Laplace, región de convergencia".into(),
            diapositivas: vec![],
        };
        gen.generar(&ctx, &frases(20, 30)).await.unwrap();

        assert!(espia.prompts()[0].contains("región de convergencia"));
    }

    #[tokio::test]
    async fn el_ocr_de_las_diapositivas_entra_en_el_prompt_del_tramo() {
        let espia = Espia::nuevo(APUNTES_MINIMOS);
        let router = router_con(espia.clone());
        let gen = NotesGenerator::nuevo(&router);

        let ctx = SessionContext {
            kind: SessionKind::Class.into(),
            contexto: "Cálculo II".into(),
            duracion_ms: 600_000,
            fecha: "2026-08-11".into(),
            glosario: String::new(),
            diapositivas: vec![Slide {
                id: 7,
                session_id: SessionId::from("s"),
                path: "/s/7.webp".into(),
                ts_ms: 20_000,
                phash: "aaaa".into(),
                ocr_text: Some("Transformada de Laplace  F(s) = ∫f(t)e^{-st}dt".into()),
                caption: None,
            }],
        };
        gen.generar(&ctx, &frases(20, 30)).await.unwrap();

        let primero = &espia.prompts()[0];
        assert!(primero.contains("slide_id 7"));
        assert!(primero.contains("F(s)"));
        // Y la instrucción de que el OCR manda sobre lo oído.
        assert!(primero.contains("MÁS \n                 FIABLE") || primero.contains("FIABLE"));
    }

    #[tokio::test]
    async fn una_transcripcion_vacia_da_error_en_lugar_de_llamar_al_modelo() {
        let espia = Espia::nuevo(APUNTES_MINIMOS);
        let router = router_con(espia.clone());
        let gen = NotesGenerator::nuevo(&router);

        let res = gen.generar(&SessionContext::default(), &[]).await;
        assert!(matches!(res, Err(NotesError::SinTranscripcion)));
        assert_eq!(
            espia.llamadas.load(Ordering::SeqCst),
            0,
            "no debe gastar una llamada"
        );
    }

    #[tokio::test]
    async fn la_reunion_con_cliente_usa_la_plantilla_comercial() {
        const ACTA: &str = r#"{
            "tldr": "Quedamos en enviar propuesta.",
            "contexto_cliente": {"empresa": "Acme", "sector": null, "tamano": null,
                                 "herramientas": []},
            "necesidades": [], "acuerdos": [],
            "presupuesto": null, "proximo_paso": null,
            "compromisos": {"mios": [], "del_cliente": []}
        }"#;

        let espia = Espia::nuevo(ACTA);
        let router = router_con(espia.clone());
        let gen = NotesGenerator::nuevo(&router);

        let ctx = SessionContext {
            kind: SessionKind::ClientMeeting.into(),
            contexto: "Acme S.A.".into(),
            duracion_ms: 3_600_000,
            fecha: "2026-08-11".into(),
            glosario: String::new(),
            diapositivas: vec![],
        };
        let g = gen.generar(&ctx, &frases(20, 30)).await.unwrap();

        assert_eq!(g.payload.plantilla(), NoteTemplate::Client);
        // Y el prompt de reduce debe llevar la prohibición clave.
        let reduce = espia.prompts().last().unwrap().clone();
        assert!(reduce.contains("NUNCA inventes un compromiso"));
    }

    #[tokio::test]
    async fn la_duracion_se_expresa_en_horas_y_minutos() {
        let ctx = SessionContext {
            duracion_ms: 5_400_000,
            ..Default::default()
        };
        assert_eq!(ctx.duracion_humana(), "1h 30min");

        let corta = SessionContext {
            duracion_ms: 900_000,
            ..Default::default()
        };
        assert_eq!(corta.duracion_humana(), "15 min");
    }

    #[tokio::test]
    async fn el_resumen_rodante_arrastra_las_notas_previas() {
        const RODANTE: &str = r#"{"texto":"acumulado","temas_hasta_ahora":["a"]}"#;
        let espia = Espia::nuevo(RODANTE);
        let router = router_con(espia.clone());
        let gen = NotesGenerator::nuevo(&router);

        let (r, _, _) = gen
            .rodante(Some("lo dicho hasta el minuto 10"), &frases(10, 20))
            .await
            .unwrap();

        assert_eq!(r.texto, "acumulado");
        assert!(espia.prompts()[0].contains("lo dicho hasta el minuto 10"));
    }
}
