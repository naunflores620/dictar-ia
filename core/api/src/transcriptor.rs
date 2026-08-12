//! Transcripción en vivo durante la grabación.
//!
//! Antes todo el trabajo se hacía al pulsar detener: una clase de dos horas
//! dejaba al usuario media hora esperando. Y es innecesario, porque la máquina
//! va sobrada: medimos **4,6× tiempo real**, o sea que transcribe 4,6 segundos
//! de audio por cada segundo de CPU. Siguiendo la clase según ocurre, al
//! terminar solo queda generar el resumen —unos segundos— en vez de todo.
//!
//! Es lo que hace que se sienta como Notion: al detener, los apuntes salen
//! casi enseguida porque la transcripción ya estaba hecha.

use crate::eventos::Evento;
use dictar_domain::{SessionId, Track};
use dictar_storage::transcript::NewUtterance;
use dictar_stt::modelos::{self, Modelo};
use dictar_stt::whisper::Transcriptor;
use dictar_stt::SttOpciones;
use dictar_vad::segmentador::Segmento;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;

/// Trabajo pendiente para el transcriptor.
pub enum Trabajo {
    Segmento(Box<Segmento>),
    /// No llegarán más segmentos: vacía la cola y termina.
    Terminar,
}

/// Transcriptor que corre en paralelo a la grabación.
pub struct TranscriptorVivo {
    tx: Sender<Trabajo>,
    hilo: Option<JoinHandle<()>>,
    pendientes: Arc<AtomicUsize>,
    /// El hilo la marca al salir, por cualquier camino. Es lo que permite a
    /// `esperar_drenaje` saber que ya no queda nada sin bloquearse en un join.
    terminado: Arc<AtomicBool>,
}

impl TranscriptorVivo {
    /// Arranca el hilo de transcripción.
    ///
    /// El modelo se carga aquí, en cuanto empieza la grabación: tarda unos
    /// segundos y hacerlo al final volvería a meter esa espera justo donde se
    /// quería quitar.
    pub fn iniciar(
        session_id: SessionId,
        db: PathBuf,
        modelo: Modelo,
        glosario: String,
        idioma: Option<String>,
        quien_sistema: String,
        eventos: Sender<Evento>,
    ) -> crate::Result<Self> {
        let ruta_modelo = modelos::localizar(modelo)?;
        let (tx, rx) = std::sync::mpsc::channel::<Trabajo>();
        let pendientes = Arc::new(AtomicUsize::new(0));
        let terminado = Arc::new(AtomicBool::new(false));

        let hilo = {
            let pendientes = pendientes.clone();
            let terminado = terminado.clone();
            std::thread::Builder::new()
                .name("dictar-transcripcion".into())
                .spawn(move || {
                    bucle(
                        rx,
                        pendientes,
                        session_id,
                        db,
                        ruta_modelo,
                        glosario,
                        idioma,
                        quien_sistema,
                        eventos,
                    );
                    // Por cualquier camino de salida —normal, modelo que no
                    // carga, base inaccesible—: quien espere el drenaje tiene
                    // que enterarse de que ya no vendrá nada más.
                    terminado.store(true, Ordering::SeqCst);
                })
                .map_err(|e| crate::ApiError::Config(e.to_string()))?
        };

        Ok(Self {
            tx,
            hilo: Some(hilo),
            pendientes,
            terminado,
        })
    }

    /// Pide el cierre **sin esperar**: el hilo vaciará su cola y marcará el
    /// fin por su cuenta.
    pub fn pedir_fin(&self) {
        let _ = self.tx.send(Trabajo::Terminar);
    }

    /// Si el hilo ya vació su cola y salió.
    pub fn esta_terminado(&self) -> bool {
        self.terminado.load(Ordering::SeqCst)
    }

    /// Encola un segmento cerrado.
    pub fn encolar(&self, seg: Segmento) {
        self.pendientes.fetch_add(1, Ordering::SeqCst);
        let _ = self.tx.send(Trabajo::Segmento(Box::new(seg)));
    }

    /// Segmentos aún sin transcribir. La interfaz lo enseña al detener, para
    /// que la espera —normalmente de segundos— no parezca un cuelgue.
    pub fn pendientes(&self) -> usize {
        self.pendientes.load(Ordering::SeqCst)
    }

    /// Espera al hilo. Debe llamarse cuando [`Self::esta_terminado`] ya es
    /// cierto; si no, bloquea hasta que la cola se vacíe.
    pub fn esperar(mut self) {
        let _ = self.tx.send(Trabajo::Terminar);
        if let Some(h) = self.hilo.take() {
            let _ = h.join();
        }
    }
}

impl Drop for TranscriptorVivo {
    fn drop(&mut self) {
        // Solo se pide el fin, **sin esperar**: Drop puede ejecutarse en la
        // llamada de la interfaz, y un join aquí es exactamente el cuelgue que
        // tuvo el botón de detener. El hilo termina su cola por su cuenta; el
        // audio ya está en disco pase lo que pase, así que soltar el mango no
        // pierde nada.
        let _ = self.tx.send(Trabajo::Terminar);
        if let Some(h) = self.hilo.take() {
            drop(h);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn bucle(
    rx: Receiver<Trabajo>,
    pendientes: Arc<AtomicUsize>,
    session_id: SessionId,
    ruta_db: PathBuf,
    ruta_modelo: PathBuf,
    glosario: String,
    idioma: Option<String>,
    quien_sistema: String,
    eventos: Sender<Evento>,
) {
    let trans = match Transcriptor::cargar(&ruta_modelo) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(error = %e, "sin transcripción en vivo");
            let _ = eventos.send(Evento::Error {
                mensaje: format!("No se pudo cargar el modelo: {e}"),
                // La grabación sigue: al detener se transcribirá por lotes,
                // que es como funcionaba antes.
                recuperable: true,
            });
            // Se drena la cola para no bloquear a quien encola.
            while let Ok(t) = rx.recv() {
                if matches!(t, Trabajo::Terminar) {
                    break;
                }
                pendientes.fetch_sub(1, Ordering::SeqCst);
            }
            return;
        }
    };

    // Conexión propia a la base: SQLite no admite compartir una conexión entre
    // hilos, y el modo WAL permite escribir aquí mientras la interfaz lee.
    let mut db = match dictar_storage::Db::abrir(&ruta_db) {
        Ok(d) => d,
        Err(e) => {
            tracing::error!(error = %e, "el transcriptor no pudo abrir la base");
            return;
        }
    };

    // Menos hilos que en la pasada por lotes: durante la clase el equipo está
    // reproduciendo vídeo, y quedarse sin CPU cortaría el audio, que es lo
    // único que no se puede recuperar.
    let mut opciones = SttOpciones::en_vivo();
    opciones.tiempos_por_palabra = false;
    if !glosario.is_empty() {
        opciones.glosario = Some(glosario);
    }
    if let Some(l) = idioma {
        opciones.idioma = Some(l);
    }

    let mut ultimo_texto: Option<String> = None;

    while let Ok(trabajo) = rx.recv() {
        let seg = match trabajo {
            Trabajo::Terminar => break,
            Trabajo::Segmento(s) => s,
        };

        // El contexto previo mejora la continuidad: Whisper acierta más con
        // los nombres propios si acaba de verlos.
        opciones.contexto_previo = ultimo_texto.clone();

        match trans.transcribir(&seg.pcm, &opciones) {
            Ok(fragmentos) => {
                let textos: Vec<&str> = fragmentos.iter().map(|f| f.texto.as_str()).collect();
                let (indices, _) = dictar_stt::limpieza::colapsar_bucles(&textos);

                let mut frases = Vec::new();
                for i in indices {
                    let f = &fragmentos[i];
                    if dictar_stt::limpieza::es_alucinacion(&f.texto) {
                        continue;
                    }

                    let u = NewUtterance {
                        track: seg.track,
                        speaker: Some(match seg.track {
                            Track::Mic => "Yo".to_owned(),
                            Track::System => quien_sistema.clone(),
                        }),
                        start_ms: seg.inicio_ms + f.inicio_ms,
                        end_ms: seg.inicio_ms + f.fin_ms,
                        text: f.texto.clone(),
                        confidence: Some(f.confianza),
                        // Definitiva: se transcribe con el modelo bueno, así
                        // que no hay una segunda pasada que la sustituya.
                        is_final: true,
                    };

                    let _ = eventos.send(Evento::frase(
                        &session_id,
                        seg.track,
                        u.start_ms,
                        u.end_ms,
                        &u.text,
                        true,
                    ));

                    ultimo_texto = Some(u.text.clone());
                    frases.push(u);
                }

                if !frases.is_empty() {
                    if let Err(e) = db.insertar_frases(&session_id, &frases) {
                        tracing::warn!(error = %e, "no se pudieron guardar las frases");
                    }
                }
            }
            Err(e) => {
                // Un segmento fallido no detiene la clase: se pierde un tramo
                // de transcripción, no la grabación.
                tracing::warn!(error = %e, "segmento sin transcribir");
            }
        }

        pendientes.fetch_sub(1, Ordering::SeqCst);
    }

    tracing::info!(sesion = %session_id, "transcripción en vivo terminada");
}
