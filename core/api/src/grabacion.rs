//! Sesión de grabación en curso.
//!
//! El hilo de escritura hace una cosa por encima de todas: **volcar el audio a
//! disco antes de procesarlo**. Si la aplicación muere en el minuto 90 de una
//! clase de dos horas, el archivo tiene que estar completo. Todo lo demás
//! —niveles, transcripción en vivo— es secundario y puede fallar sin
//! consecuencias.

use crate::eventos::Evento;
use crate::salud_audio::SaludAudio;
use crate::transcriptor::TranscriptorVivo;
use crate::{ApiError, Nucleo, Result};
use dictar_audio::wav::EscritorPistas;
use dictar_audio::{CaptureConfig, CaptureSession};
use dictar_domain::{EpochMs, Session, SessionId, SessionKind, SessionSource, TopicId, Track};
use serde_json::json;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct OpcionesGrabacion {
    pub kind: SessionKind,
    pub topic_id: Option<TopicId>,
    pub titulo: Option<String>,
    /// Capturar la salida del sistema además del micrófono.
    pub capturar_sistema: bool,
    /// Marca la sesión como confidencial: nunca se enviará a un proveedor
    /// externo, aunque la configuración global lo permita.
    pub solo_local: bool,
}

impl Default for OpcionesGrabacion {
    fn default() -> Self {
        Self {
            kind: SessionKind::Class,
            topic_id: None,
            titulo: None,
            capturar_sistema: true,
            solo_local: false,
        }
    }
}

/// Grabación viva. Al detenerla, cierra los archivos y deja la sesión lista
/// para procesar.
pub struct Grabacion {
    pub session_id: SessionId,
    /// Transcripción en vivo. Al terminar la clase la transcripción ya está
    /// hecha, así que detener solo cuesta el resumen.
    transcriptor: Option<Arc<TranscriptorVivo>>,
    // La ruta del audio no se guarda aquí: se deriva del id con
    // `Nucleo::dir_audio`, y tener dos fuentes de verdad para lo mismo es
    // justo como acaban desincronizándose.
    captura: Option<Box<dyn CaptureSession>>,
    parar: Arc<AtomicBool>,
    hilo: Option<JoinHandle<()>>,
    inicio: Instant,
}

impl Grabacion {
    pub fn duracion(&self) -> Duration {
        self.inicio.elapsed()
    }
}

impl Nucleo {
    /// Empieza a grabar y devuelve el canal de eventos.
    pub fn iniciar_grabacion(
        &self,
        opts: OpcionesGrabacion,
        ahora: EpochMs,
    ) -> Result<(SessionId, Receiver<Evento>)> {
        {
            let g = self.grabacion.lock().unwrap();
            if g.is_some() {
                return Err(ApiError::YaGrabando);
            }
        }

        let source = if opts.capturar_sistema {
            SessionSource::DesktopLoopback
        } else {
            SessionSource::DesktopMic
        };

        let mut sesion = Session::nueva(opts.kind, source, ahora);
        sesion.topic_id = opts.topic_id.clone();
        sesion.title = opts.titulo.clone();
        sesion.solo_local = opts.solo_local;
        sesion.language = self.ajustes().idioma_efectivo();

        let dir_audio = self.dir_datos().join("audio").join(sesion.id.as_str());
        sesion.audio_path = Some(dir_audio.to_string_lossy().into_owned());

        self.con_db(|db| db.crear_sesion(&sesion))?;

        let cfg = CaptureConfig {
            capturar_sistema: opts.capturar_sistema,
            capturar_microfono: true,
            ..Default::default()
        };

        let inicio_captura = Instant::now();
        let usar_vosk = matches!(sesion.language.as_deref(), Some("es" | "en"));
        let (tx_ev, rx_ev) = mpsc::channel::<Evento>();

        // Transcripción en vivo. Si el modelo no está descargado se avisa y se
        // sigue grabando: al detener se transcribirá por lotes, que es como
        // funcionaba antes. Perder la clase por eso sería absurdo.
        let glosario = opts
            .topic_id
            .as_ref()
            .and_then(|t| self.glosario(t).ok())
            .unwrap_or_default();

        let transcriptor = match TranscriptorVivo::iniciar(
            sesion.id.clone(),
            self.dir_datos().join("dictar.db"),
            self.modelo_en_vivo(),
            glosario,
            sesion.language.clone(),
            self.nombre_interlocutor(&sesion),
            tx_ev.clone(),
            inicio_captura,
            dir_audio.join("stt_metrics.jsonl"),
        ) {
            Ok(t) => Some(Arc::new(t)),
            Err(e) => {
                tracing::warn!(error = %e, "sin transcripción en vivo; se hará al terminar");
                let _ = tx_ev.send(Evento::Error {
                    mensaje: format!("Sin transcripción en vivo: {e}"),
                    recuperable: true,
                });
                None
            }
        };

        // En Windows Español, TranscriptorVivo espera a que el modelo streaming
        // esté cargado. Abrimos WASAPI después de ese punto, para que la clase
        // no empiece con decenas de segundos de transcripción atrasada.
        let (rx_audio, captura) = dictar_audio::iniciar(cfg)?;
        let inicio_captura = Instant::now();
        if let Some(t) = &transcriptor {
            t.marcar_inicio_captura(inicio_captura);
        }
        sesion.started_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(i64::MAX as u128) as i64;
        self.con_db(|db| db.actualizar_sesion(&sesion))?;

        let parar = Arc::new(AtomicBool::new(false));
        let inicio = Instant::now();

        let hilo = {
            let dir = dir_audio.clone();
            let parar = parar.clone();
            let trans = transcriptor.clone();
            let tx_hilo = tx_ev.clone();
            let priorizar_sistema = opts.capturar_sistema;
            let capturar_sistema = opts.capturar_sistema;
            std::thread::Builder::new()
                .name("dictar-escritura".into())
                .spawn(move || {
                    if let Err(e) = bucle_escritura(
                        dir,
                        rx_audio,
                        tx_hilo.clone(),
                        parar,
                        trans,
                        priorizar_sistema,
                        usar_vosk,
                        SaludAudio::nueva(inicio, capturar_sistema),
                    ) {
                        tracing::error!(error = %e, "el hilo de escritura falló");
                        let _ = tx_hilo.send(Evento::Error {
                            mensaje: e.to_string(),
                            // El audio ya escrito sigue en disco: la sesión se
                            // puede procesar igualmente.
                            recuperable: true,
                        });
                    }
                })
                .map_err(|e| ApiError::Config(e.to_string()))?
        };

        let id = sesion.id.clone();
        *self.grabacion.lock().unwrap() = Some(Grabacion {
            session_id: id.clone(),
            transcriptor,
            captura: Some(captura),
            parar,
            hilo: Some(hilo),
            inicio,
        });

        tracing::info!(sesion = %id, "grabación iniciada");
        Ok((id, rx_ev))
    }

    /// Detiene la grabación y deja la sesión en cola de proceso.
    pub fn detener_grabacion(&self, ahora: EpochMs) -> Result<SessionId> {
        let mut g = self.grabacion.lock().unwrap();
        let Some(mut grab) = g.take() else {
            return Err(ApiError::NoGrabando);
        };

        // El orden importa: primero se corta la captura, para que no lleguen
        // más bloques, y solo después se cierra el hilo de escritura. Al revés,
        // los últimos segundos se perderían.
        if let Some(c) = grab.captura.take() {
            let _ = c.detener();
        }
        grab.parar.store(true, Ordering::SeqCst);
        if let Some(h) = grab.hilo.take() {
            let _ = h.join();
        }

        // La cola de transcripción que quede se termina en segundo plano.
        // Detener tiene que volver AL INSTANTE: la versión anterior esperaba
        // aquí a que se vaciara, y en una clase real —con Meet comiéndose
        // media CPU— la cola se atrasa, así que el botón se quedaba minutos
        // clavado. Y sin un solo aviso, porque la interfaz dejaba de leer
        // eventos justo antes de llamar. Ahora quien espera es el procesado,
        // con la barra de progreso delante.
        if let Some(t) = grab.transcriptor.take() {
            let pendientes = t.pendientes();
            if pendientes > 0 {
                tracing::info!(pendientes, "la cola se termina en segundo plano");
            }
            t.pedir_fin();
            *self.drenaje.lock().unwrap() = Some((grab.session_id.clone(), t));
        }

        let id = grab.session_id.clone();
        self.con_db(|db| db.cerrar_sesion(&id, ahora))?;

        tracing::info!(sesion = %id, duracion_s = grab.inicio.elapsed().as_secs(), "grabación detenida");
        Ok(id)
    }

    pub fn grabando(&self) -> Option<SessionId> {
        self.grabacion
            .lock()
            .unwrap()
            .as_ref()
            .map(|g| g.session_id.clone())
    }

    pub fn duracion_grabacion(&self) -> Option<Duration> {
        self.grabacion
            .lock()
            .unwrap()
            .as_ref()
            .map(|g| g.duracion())
    }

    /// Guarda una captura de la pantalla en la sesión que se está grabando.
    ///
    /// Es la vía manual: el usuario decide el momento. No se toman capturas
    /// automáticas durante la grabación.
    pub fn capturar_diapositiva(&self) -> Result<PathBuf> {
        let g = self.grabacion.lock().unwrap();
        let Some(grab) = g.as_ref() else {
            return Err(ApiError::NoGrabando);
        };

        let id = grab.session_id.clone();
        let ts = grab.duracion().as_millis() as i64;
        drop(g);

        let s = dictar_screen::capturar_ahora(
            self.dir_audio(&id).join("diapositivas"),
            ts,
            None,
            self.region_captura(),
        )?;

        let ruta = s.ruta.to_string_lossy().into_owned();
        self.con_db(|db| db.insertar_diapositiva(&id, &ruta, s.ts_ms, &s.phash))?;

        Ok(s.ruta)
    }

    /// Carpeta de audio de una sesión.
    pub fn dir_audio(&self, id: &SessionId) -> PathBuf {
        self.dir_datos().join("audio").join(id.as_str())
    }
}

#[allow(clippy::too_many_arguments)]
fn bucle_escritura(
    dir: PathBuf,
    rx: Receiver<dictar_audio::AudioFrame>,
    tx: Sender<Evento>,
    parar: Arc<AtomicBool>,
    transcriptor: Option<Arc<TranscriptorVivo>>,
    priorizar_sistema: bool,
    usar_vosk: bool,
    mut salud: SaludAudio,
) -> Result<()> {
    use dictar_vad::segmentador::Segmentador;
    use std::collections::HashMap;

    let mut escritor = EscritorPistas::nuevo(&dir)?;

    // Un segmentador por pista: cada una lleva su propio detector de voz y sus
    // propias fronteras de frase.
    let mut segmentadores: HashMap<Track, Segmentador> = HashMap::new();

    let mut nivel_mic = 0.0f32;
    let mut nivel_sis = 0.0f32;
    let mut ultimo_evento = Instant::now();
    let mut ultimo_flush = Instant::now();
    let mut ultimo_borrador: HashMap<Track, Instant> = HashMap::new();
    let inicio_transcripcion = Instant::now();
    let mut ultima_trama_sistema: Option<Instant> = None;
    let mut metricas_captura = HashMap::<Track, VentanaCaptura>::new();

    loop {
        // Espera acotada: permite comprobar la señal de parada aunque una pista
        // deje de entregar bloques, en vez de quedarse colgado.
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(frame) => {
                // Lo primero, siempre: a disco. La transcripción va después y
                // puede fallar; el audio no.
                escritor.escribir(&frame)?;
                metricas_captura
                    .entry(frame.track)
                    .or_default()
                    .registrar(&frame);

                // Con el audio ya en disco. Los avisos son un lujo de la
                // interfaz: nunca pueden ir antes que la escritura.
                for cambio in salud.trama(&frame, Instant::now()) {
                    let _ = tx.send(cambio.a_evento());
                }

                if frame.track == Track::System {
                    ultima_trama_sistema = Some(Instant::now());
                }

                // Cuando hay audio de escritorio, esa pista es la referencia
                // limpia del profesor. El micrófono puede traer la misma voz
                // retrasada por los altavoces: se conserva completo en WAV,
                // pero no se transcribe en vivo mientras exista la pista de
                // sistema. La pasada final procesa ambas y elimina la
                // diafonía por contenido. Si el loopback no aparece al
                // arrancar o deja de entregar tramas, el micrófono queda
                // como respaldo.
                let transcribir_pista = match frame.track {
                    Track::System => true,
                    Track::Mic => {
                        !priorizar_sistema
                            || (ultima_trama_sistema
                                .map(|t| t.elapsed() >= Duration::from_secs(1))
                                .unwrap_or(true)
                                && inicio_transcripcion.elapsed() >= Duration::from_secs(1))
                    }
                };

                // El motor incremental (Sherpa/Vosk) recibe el micrófono
                // siempre: la compuerta de arriba era para no pagar un segundo
                // Whisper sobre el eco, y el filtro de eco del transcriptor
                // (`ContextoEco`) ya descarta las frases que repiten al sistema.
                // Sin esto, el micrófono no tenía ni una frase en vivo y el
                // cierre lo transcribía entero por lotes. Los segmentos VAD
                // (camino Whisper) conservan la compuerta.
                if usar_vosk && !transcribir_pista {
                    if let Some(t) = &transcriptor {
                        t.encolar_bloque(frame.clone());
                    }
                }

                if transcribir_pista {
                    if let Some(t) = &transcriptor {
                        if usar_vosk {
                            t.encolar_bloque(frame.clone());
                        }
                        let seg = segmentadores
                            .entry(frame.track)
                            .or_insert_with(|| Segmentador::nuevo(frame.track));

                        let cerrados = seg.empujar(&frame.pcm, frame.timestamp_ms);
                        let cerro_segmento = !cerrados.is_empty();
                        for cerrado in cerrados {
                            ultimo_borrador.remove(&frame.track);
                            t.encolar(cerrado);
                        }

                        let reloj = Instant::now();
                        let vencio = ultimo_borrador
                            .get(&frame.track)
                            .map(|t| t.elapsed() >= Duration::from_secs(3))
                            .unwrap_or(true);
                        // La vista previa clona el buffer acumulado. Si Whisper
                        // sigue ocupado, la hipótesis se descartaría de todos
                        // modos; comprobar pendientes primero evita esa copia.
                        if vencio && !cerro_segmento && t.pendientes() == 0 {
                            if let Some(vista) = seg.vista_previa() {
                                if vista.duracion_ms() >= 2_000 && vista.merece_transcribirse() {
                                    t.encolar_hipotesis(vista);
                                    ultimo_borrador.insert(frame.track, reloj);
                                }
                            }
                        }
                    }
                }

                // Suavizado exponencial de los niveles, para que el indicador
                // no parpadee con cada bloque.
                let nivel = frame.nivel();
                match frame.track {
                    Track::Mic => nivel_mic = nivel_mic * 0.7 + nivel * 0.3,
                    Track::System => nivel_sis = nivel_sis * 0.7 + nivel * 0.3,
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            // La captura soltó el canal: se detuvo antes que nosotros. NO se
            // hace el cierre aquí sino tras el bucle, común a las dos salidas.
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        // En cada vuelta, también cuando `recv_timeout` venció sin trama:
        // «el micrófono dejó de enviar» y «el loopback está mudo» son, por
        // definición, avisos de que no llega nada. El vaciado de después del
        // bucle no alimenta esto: ya no hay interfaz a la que avisar.
        for cambio in salud.tick(Instant::now()) {
            let _ = tx.send(cambio.a_evento());
        }

        if ultimo_evento.elapsed() >= Duration::from_millis(150) {
            let _ = tx.send(Evento::Niveles {
                mic: nivel_mic,
                sistema: nivel_sis,
                transcurrido_ms: escritor
                    .duracion_ms(Track::Mic)
                    .max(escritor.duracion_ms(Track::System)),
            });
            ultimo_evento = Instant::now();
        }

        // Volcado periódico: un corte de luz cuesta unos segundos de audio, no
        // la clase entera.
        if ultimo_flush.elapsed() >= Duration::from_secs(5) {
            escritor.sincronizar()?;
            volcar_metricas_captura(&dir, &mut metricas_captura);
            ultimo_flush = Instant::now();
        }

        if parar.load(Ordering::SeqCst) {
            break;
        }
    }

    // El cierre va DESPUÉS del bucle, común a las dos formas de salir: la
    // señal de parar y la desconexión del canal. Antes solo lo hacía el brazo
    // de parar, y detener corta la captura primero —el canal se desconecta y
    // el bucle salía por el otro brazo—, así que los segmentos a medias nunca
    // se cerraban: los últimos ~25 s de cada clase se quedaban sin transcribir
    // en vivo, y una sesión corta no transcribía nada y se rehacía entera por
    // lotes, cargando el modelo por segunda vez.

    // Primero, lo que quede en el canal: son los últimos segundos, que suelen
    // ser el resumen del profesor o el acuerdo final con el cliente.
    while let Ok(frame) = rx.try_recv() {
        escritor.escribir(&frame)?;
        metricas_captura
            .entry(frame.track)
            .or_default()
            .registrar(&frame);
        if frame.track == Track::System {
            ultima_trama_sistema = Some(Instant::now());
        }
        let transcribir_pista = match frame.track {
            Track::System => true,
            Track::Mic => {
                !priorizar_sistema
                    || (ultima_trama_sistema
                        .map(|t| t.elapsed() >= Duration::from_secs(1))
                        .unwrap_or(true)
                        && inicio_transcripcion.elapsed() >= Duration::from_secs(1))
            }
        };
        if transcribir_pista {
            if let Some(t) = &transcriptor {
                let seg = segmentadores
                    .entry(frame.track)
                    .or_insert_with(|| Segmentador::nuevo(frame.track));
                let cerrados = seg.empujar(&frame.pcm, frame.timestamp_ms);
                let cerro_segmento = !cerrados.is_empty();
                for cerrado in cerrados {
                    ultimo_borrador.remove(&frame.track);
                    t.encolar(cerrado);
                }
                let reloj = Instant::now();
                let vencio = ultimo_borrador
                    .get(&frame.track)
                    .map(|t| t.elapsed() >= Duration::from_secs(3))
                    .unwrap_or(true);
                // Mismo control que en el bucle principal: no copiar una vista
                // que se descartaría porque hay inferencia pendiente.
                if vencio && !cerro_segmento && t.pendientes() == 0 {
                    if let Some(vista) = seg.vista_previa() {
                        if vista.duracion_ms() >= 2_000 && vista.merece_transcribirse() {
                            t.encolar_hipotesis(vista);
                            ultimo_borrador.insert(frame.track, reloj);
                        }
                    }
                }
            }
        }
    }

    volcar_metricas_captura(&dir, &mut metricas_captura);

    // Y los segmentos a medias.
    if let Some(t) = &transcriptor {
        for seg in segmentadores.values_mut() {
            if let Some(ultimo) = seg.finalizar() {
                t.encolar(ultimo);
            }
        }
    }

    let rutas = escritor.cerrar()?;
    tracing::info!(archivos = rutas.len(), dir = %dir.display(), "audio cerrado");
    Ok(())
}

/// Aggregate capture health for one window. It stores no audio or transcript.
#[derive(Default)]
struct VentanaCaptura {
    tramas: u64,
    muestras: u64,
    suma_cuadrados: f64,
    pico: f32,
    muestras_recortadas: u64,
    tramas_silenciosas: u64,
    duracion_audio_ms: u64,
    inicio_audio_ms: Option<i64>,
    fin_audio_ms: i64,
}

impl VentanaCaptura {
    fn registrar(&mut self, frame: &dictar_audio::AudioFrame) {
        const UMBRAL_SILENCIO_RMS: f64 = 0.003;
        self.tramas += 1;
        self.duracion_audio_ms += frame.duracion_ms().max(0) as u64;
        self.inicio_audio_ms.get_or_insert(frame.timestamp_ms);
        self.fin_audio_ms = self
            .fin_audio_ms
            .max(frame.timestamp_ms.saturating_add(frame.duracion_ms()));

        let mut energia = 0.0f64;
        let mut pico = 0.0f32;
        let mut recortadas = 0u64;
        for muestra in &frame.pcm {
            let absoluta = if muestra.is_finite() {
                muestra.abs()
            } else {
                0.0
            };
            energia += (absoluta as f64).powi(2);
            pico = pico.max(absoluta);
            recortadas += u64::from(absoluta >= 0.999);
        }
        if !frame.pcm.is_empty() && (energia / frame.pcm.len() as f64).sqrt() < UMBRAL_SILENCIO_RMS
        {
            self.tramas_silenciosas += 1;
        }
        self.muestras += frame.pcm.len() as u64;
        self.suma_cuadrados += energia;
        self.pico = self.pico.max(pico);
        self.muestras_recortadas += recortadas;
    }

    fn a_json(&self, pista: Track) -> Option<serde_json::Value> {
        if self.tramas == 0 {
            return None;
        }
        let muestras = self.muestras.max(1);
        let dato = json!({
            "tipo": "captura",
            "pista": pista.as_str(),
            "inicio_audio_ms": self.inicio_audio_ms.unwrap_or(0),
            "fin_audio_ms": self.fin_audio_ms,
            "tramas": self.tramas,
            "muestras": self.muestras,
            "duracion_audio_ms": self.duracion_audio_ms,
            "nivel_rms": (self.suma_cuadrados / muestras as f64).sqrt(),
            "pico_abs": self.pico,
            "muestras_recortadas": self.muestras_recortadas,
            "proporcion_muestras_recortadas": self.muestras_recortadas as f64 / muestras as f64,
            "tramas_casi_silencio": self.tramas_silenciosas,
            "proporcion_tramas_casi_silencio": self.tramas_silenciosas as f64 / self.tramas as f64,
        });
        Some(dato)
    }
}

/// Appends per-track signal metrics every five seconds. Failures are logged
/// but never stop writing the source WAV files.
fn volcar_metricas_captura(dir: &std::path::Path, metricas: &mut HashMap<Track, VentanaCaptura>) {
    let ruta = dir.join("capture_metrics.jsonl");
    for (pista, ventana) in metricas.iter_mut() {
        let Some(dato) = ventana.a_json(*pista) else {
            continue;
        };
        let resultado = (|| -> std::io::Result<()> {
            let mut linea = serde_json::to_vec(&dato).map_err(std::io::Error::other)?;
            linea.push(b'\n');
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&ruta)?
                .write_all(&linea)?;
            Ok(())
        })();
        match resultado {
            Ok(()) => *ventana = VentanaCaptura::default(),
            Err(error) => {
                tracing::warn!(error = %error, ruta = %ruta.display(), "failed to write capture metrics")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn las_opciones_por_defecto_graban_las_dos_pistas() {
        let o = OpcionesGrabacion::default();
        assert!(o.capturar_sistema);
        assert!(!o.solo_local);
    }

    #[test]
    fn detener_sin_grabar_da_un_error_claro() {
        let dir = tempfile::tempdir().unwrap();
        let n = Nucleo::abrir(dir.path()).unwrap();

        assert!(matches!(n.detener_grabacion(0), Err(ApiError::NoGrabando)));
        assert!(n.grabando().is_none());
    }

    #[test]
    fn el_directorio_de_audio_cuelga_del_id_de_sesion() {
        let dir = tempfile::tempdir().unwrap();
        let n = Nucleo::abrir(dir.path()).unwrap();

        let id = SessionId::from("ses_prueba");
        let d = n.dir_audio(&id);
        assert!(d.ends_with("audio/ses_prueba"));
    }
}
