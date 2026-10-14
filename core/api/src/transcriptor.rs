//! Transcripción en vivo durante la grabación.
//!
//! Publica hipótesis reemplazables mientras un segmento VAD sigue abierto y
//! una pasada final de mayor calidad al detener. Cada hipótesis vuelve a
//! procesar el audio acumulado del segmento; por eso el RTF de una pasada por
//! segmentos cerrados no demuestra la latencia ni el ritmo del flujo en vivo.
//! Esas métricas se registran por separado y deben verificarse en captura real.

use crate::eventos::Evento;
use crate::limpieza_transcripcion::{conservar_historial_reciente, deduplicar_solape};
use dictar_domain::{SessionId, Track};
use dictar_storage::transcript::NewUtterance;
use dictar_stt::modelos::{self, Modelo};
use dictar_stt::nivel::{interpretar_interruptor, Nivelador};
use dictar_stt::whisper::Transcriptor;
use dictar_stt::SttOpciones;
use dictar_vad::segmentador::{Segmento, SOLAPE_MS};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

/// Trabajo pendiente para el transcriptor.
pub enum Trabajo {
    /// Bloque de audio para el reconocedor de estado continuo.
    Bloque(Box<dictar_audio::AudioFrame>, Instant),
    Segmento(Box<Segmento>, Instant),
    /// Hipótesis reemplazable de un segmento que sigue abierto.
    Hipotesis(Box<Segmento>, Instant),
    /// No llegarán más segmentos: vacía la cola y termina.
    Terminar,
}

/// Transcriptor que corre en paralelo a la grabación.
pub struct TranscriptorVivo {
    tx: Sender<Trabajo>,
    hilo: Option<JoinHandle<()>>,
    pendientes: Arc<AtomicUsize>,
    audio_en_cola_ms: Arc<AtomicUsize>,
    ultima_perdida_por_pista: Arc<Mutex<HashMap<Track, i64>>>,
    hipotesis_descartadas: Arc<AtomicUsize>,
    inicio_captura: Arc<std::sync::Mutex<Instant>>,
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
        inicio_captura: Instant,
        ruta_metricas: PathBuf,
    ) -> crate::Result<Self> {
        // En español, Vosk puede arrancar aunque el modelo Whisper de la
        // pasada final aún no esté instalado. Se conserva su ruta opcional
        // como respaldo; las demás lenguas siguen exigiendo Whisper.
        let ruta_modelo = if idioma.as_deref() == Some("es") {
            modelos::localizar(modelo).ok()
        } else {
            Some(modelos::localizar(modelo)?)
        };
        let (tx, rx) = std::sync::mpsc::channel::<Trabajo>();
        let (listo_tx, listo_rx) = std::sync::mpsc::sync_channel(1);
        let esperar_modelo_es = cfg!(target_os = "windows") && idioma.as_deref() == Some("es");
        let inicio_captura = Arc::new(std::sync::Mutex::new(inicio_captura));
        let pendientes = Arc::new(AtomicUsize::new(0));
        let audio_en_cola_ms = Arc::new(AtomicUsize::new(0));
        let ultima_perdida_por_pista = Arc::new(Mutex::new(HashMap::new()));
        let hipotesis_descartadas = Arc::new(AtomicUsize::new(0));
        let terminado = Arc::new(AtomicBool::new(false));

        let hilo = {
            let pendientes = pendientes.clone();
            let audio_en_cola_ms = audio_en_cola_ms.clone();
            let ultima_perdida_por_pista = ultima_perdida_por_pista.clone();
            let hipotesis_descartadas = hipotesis_descartadas.clone();
            let terminado = terminado.clone();
            let inicio_captura = inicio_captura.clone();
            std::thread::Builder::new()
                .name("dictar-transcripcion".into())
                .spawn(move || {
                    bucle(
                        rx,
                        pendientes,
                        audio_en_cola_ms,
                        ultima_perdida_por_pista,
                        hipotesis_descartadas,
                        session_id,
                        db,
                        ruta_modelo,
                        glosario,
                        idioma,
                        quien_sistema,
                        eventos,
                        inicio_captura,
                        listo_tx,
                        ruta_metricas,
                    );
                    // Por cualquier camino de salida —normal, modelo que no
                    // carga, base inaccesible—: quien espere el drenaje tiene
                    // que enterarse de que ya no vendrá nada más.
                    terminado.store(true, Ordering::SeqCst);
                })
                .map_err(|e| crate::ApiError::Config(e.to_string()))?
        };

        // En Windows Español, la carga del modelo puede tardar decenas de
        // segundos. La API espera a que esté listo antes de abrir la captura,
        // para que la primera parte de la clase no quede como audio atrasado.
        if esperar_modelo_es {
            let _ = listo_rx.recv();
        }

        Ok(Self {
            tx,
            hilo: Some(hilo),
            pendientes,
            audio_en_cola_ms,
            ultima_perdida_por_pista,
            hipotesis_descartadas,
            inicio_captura,
            terminado,
        })
    }

    /// Restablece el reloj cuando la captura realmente empieza, después de
    /// haber preparado el modelo local.
    pub fn marcar_inicio_captura(&self, inicio: Instant) {
        *self.inicio_captura.lock().unwrap() = inicio;
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
        let _ = self
            .tx
            .send(Trabajo::Segmento(Box::new(seg), Instant::now()));
    }

    /// Encola audio en vivo con límite: si el motor se atrasa, se omite audio
    /// solo en la transcripción; la escritura WAV ya ocurrió y nunca espera.
    pub fn encolar_bloque(&self, frame: dictar_audio::AudioFrame) {
        // El modelo ya está cargado antes de iniciar WASAPI. Acotamos el atraso
        // por tiempo de audio, no por cantidad de paquetes: el tamaño de paquete
        // depende del dispositivo y 1.000 paquetes podían retener decenas de
        // segundos de audio en equipos lentos.
        const MAX_AUDIO_EN_COLA_MS: usize = 2_000;
        let duracion_ms = frame.duracion_ms().max(0) as usize;
        let previo = self
            .audio_en_cola_ms
            .fetch_add(duracion_ms, Ordering::SeqCst);
        if previo.saturating_add(duracion_ms) > MAX_AUDIO_EN_COLA_MS {
            self.audio_en_cola_ms
                .fetch_sub(duracion_ms, Ordering::SeqCst);
            let fin_perdido = frame.timestamp_ms.saturating_add(duracion_ms as i64);
            let mut perdidas = self.ultima_perdida_por_pista.lock().unwrap();
            perdidas
                .entry(frame.track)
                .and_modify(|fin| *fin = (*fin).max(fin_perdido))
                .or_insert(fin_perdido);
            drop(perdidas);
            self.hipotesis_descartadas.fetch_add(1, Ordering::Relaxed);
            return;
        }
        self.pendientes.fetch_add(1, Ordering::SeqCst);
        if self
            .tx
            .send(Trabajo::Bloque(Box::new(frame), Instant::now()))
            .is_err()
        {
            self.pendientes.fetch_sub(1, Ordering::SeqCst);
            self.audio_en_cola_ms
                .fetch_sub(duracion_ms, Ordering::SeqCst);
        }
    }

    /// Encola una vista previa solo si el motor no tiene otro trabajo pendiente.
    /// Las vistas son descartables; impedir que se acumulen es esencial para
    /// que la transcripción no quede cada vez más atrás del audio.
    pub fn encolar_hipotesis(&self, seg: Segmento) {
        let reservada = self
            .pendientes
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok();
        if !reservada {
            self.hipotesis_descartadas.fetch_add(1, Ordering::Relaxed);
            return;
        }

        if self
            .tx
            .send(Trabajo::Hipotesis(Box::new(seg), Instant::now()))
            .is_err()
        {
            self.pendientes.fetch_sub(1, Ordering::SeqCst);
            self.hipotesis_descartadas.fetch_add(1, Ordering::Relaxed);
        }
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
    audio_en_cola_ms: Arc<AtomicUsize>,
    ultima_perdida_por_pista: Arc<Mutex<HashMap<Track, i64>>>,
    hipotesis_descartadas: Arc<AtomicUsize>,
    session_id: SessionId,
    ruta_db: PathBuf,
    ruta_modelo: Option<PathBuf>,
    glosario: String,
    idioma: Option<String>,
    quien_sistema: String,
    eventos: Sender<Evento>,
    inicio_captura: Arc<std::sync::Mutex<Instant>>,
    listo: std::sync::mpsc::SyncSender<()>,
    ruta_metricas: PathBuf,
) {
    let idioma_en_vivo = idioma
        .as_deref()
        .filter(|idioma| matches!(*idioma, "es" | "en"))
        .map(str::to_owned);
    let mut reconocedores = None;
    let mut motor_en_vivo = "whisper";

    #[cfg(target_os = "windows")]
    if idioma_en_vivo.as_deref() == Some("es") {
        match crear_reconocedores_sherpa(&glosario) {
            Ok(resultado) => {
                reconocedores = Some(resultado);
                motor_en_vivo = "sherpa-onnx-streaming-zipformer-es-kroko-2025-08-06";
            }
            Err(error) => tracing::warn!(error = %error, "Sherpa no disponible; se intenta Vosk"),
        }
    }

    if reconocedores.is_none() {
        if let Some(idioma) = idioma_en_vivo.as_deref() {
            match crear_reconocedores_vosk(idioma) {
                Ok(resultado) => {
                    reconocedores = Some(resultado);
                    motor_en_vivo = if idioma == "en" {
                        "vosk-model-small-en-us-0.15"
                    } else {
                        "vosk-model-small-es-0.42"
                    };
                }
                Err(error) => {
                    tracing::warn!(error = %error, idioma, "Vosk no disponible; se mantiene el respaldo Whisper");
                    let _ = eventos.send(Evento::Error {
                        mensaje: format!("No se pudo iniciar el reconocedor local ({idioma}): {error}; se usara Whisper"),
                        recuperable: true,
                    });
                }
            }
        }
    }

    // Solo la pista de sistema y solo con Sherpa: el micrófono recoge además el
    // eco de los altavoces (correlación 0,71–0,74 medida) y amplificarlo lo
    // subiría tanto como a la voz, y para Vosk no hay medición que lo respalde.
    let mut nivelador_sistema = if reconocedores.is_some()
        && motor_en_vivo.starts_with("sherpa")
        && nivelar_desde_entorno(&eventos)
    {
        Some(Nivelador::nuevo())
    } else {
        None
    };

    let mut trans = if reconocedores.is_none() {
        let Some(ruta) = ruta_modelo.as_ref() else {
            let _ = listo.send(());
            notificar_sin_motor(
                "no hay reconocedor en vivo ni modelo Whisper instalado",
                &eventos,
                &rx,
                &pendientes,
            );
            return;
        };
        match Transcriptor::cargar(ruta) {
            Ok(t) => Some(t),
            Err(error) => {
                let _ = listo.send(());
                notificar_sin_motor(error, &eventos, &rx, &pendientes);
                return;
            }
        }
    } else {
        None
    };
    // Conexión propia a la base: SQLite no admite compartir una conexión entre
    // hilos, y el modo WAL permite escribir aquí mientras la interfaz lee.
    let mut db = match dictar_storage::Db::abrir(&ruta_db) {
        Ok(d) => d,
        Err(e) => {
            let _ = listo.send(());
            tracing::error!(error = %e, "el transcriptor no pudo abrir la base");
            let _ = eventos.send(Evento::Error {
                mensaje: format!("No se pudo abrir la base para transcribir en vivo: {e}"),
                // El WAV sigue guardándose y el procesamiento posterior puede
                // volver a abrir la base cuando termine la captura.
                recuperable: true,
            });
            // No se puede persistir el resultado en vivo. Vaciar lo ya
            // encolado evita dejar el contador de pendientes bloqueado; los
            // WAV quedan disponibles para el procesamiento final.
            while let Ok(trabajo) = rx.recv() {
                if matches!(trabajo, Trabajo::Terminar) {
                    break;
                }
                pendientes.fetch_sub(1, Ordering::SeqCst);
            }
            return;
        }
    };
    let _ = listo.send(());

    // Se limita a dos hilos para dejar CPU disponible al video y a la captura.
    // El compromiso es más tiempo de inferencia; stt_metrics.jsonl registra
    // ese costo junto con la edad de cada hipótesis.
    let mut opciones = SttOpciones::en_vivo();
    opciones.tiempos_por_palabra = false;
    if !glosario.is_empty() {
        opciones.glosario = Some(glosario);
    }
    if let Some(l) = idioma {
        opciones.idioma = Some(l);
    }

    let mut acuerdo_mic = dictar_stt::acuerdo::AcuerdoLocal::nuevo();
    let mut acuerdo_sistema = dictar_stt::acuerdo::AcuerdoLocal::nuevo();
    let mut ultimo_texto_mic: Option<String> = None;
    let mut ultimo_texto_sistema: Option<String> = None;
    let mut frases_recientes: Vec<NewUtterance> = Vec::new();
    let modelo = if reconocedores.is_some() {
        motor_en_vivo.to_owned()
    } else {
        ruta_modelo
            .as_ref()
            .and_then(|ruta| ruta.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("desconocido")
            .to_owned()
    };

    let mut estado_vivo: HashMap<Track, EstadoIncremental> = Track::TODAS
        .iter()
        .copied()
        .map(|track| (track, EstadoIncremental::nuevo()))
        .collect();
    let mut metricas_vivo = HashMap::<Track, ResumenBloquesVivo>::new();
    let mut ultimo_flush_vivo = Instant::now();
    let mut eco = ContextoEco::default();
    let mut bloques_mic: u64 = 0;
    let mut mic_con_huecos = false;
    while let Ok(trabajo) = rx.recv() {
        let inicio_captura = *inicio_captura.lock().unwrap();
        let (seg, es_hipotesis, encolado_en) = match trabajo {
            Trabajo::Terminar => break,
            Trabajo::Bloque(frame, encolado_en) => {
                audio_en_cola_ms.fetch_sub(frame.duracion_ms().max(0) as usize, Ordering::SeqCst);
                let espera_cola_ms = encolado_en.elapsed().as_millis() as u64;
                let inicio_inferencia = Instant::now();
                let resultado = if let Some(reconocedores) = reconocedores.as_mut() {
                    let motor = reconocedores
                        .get_mut(&frame.track)
                        .expect("hay un reconocedor por pista");
                    let estado = estado_vivo
                        .get_mut(&frame.track)
                        .expect("hay estado incremental por pista");
                    let reiniciar_por_hueco = {
                        let mut perdidas = ultima_perdida_por_pista.lock().unwrap();
                        if perdidas
                            .get(&frame.track)
                            .is_some_and(|fin_perdido| frame.timestamp_ms > *fin_perdido)
                        {
                            perdidas.remove(&frame.track);
                            true
                        } else {
                            false
                        }
                    };
                    if reiniciar_por_hueco {
                        mic_con_huecos |= frame.track == Track::Mic;
                        motor.reiniciar();
                        estado.reiniciar();
                        let _ = eventos.send(Evento::frase_incremental(
                            &session_id,
                            frame.track,
                            frame.timestamp_ms,
                            frame.timestamp_ms,
                            "",
                            "",
                        ));
                    }
                    procesar_bloque_incremental(
                        motor,
                        &frame,
                        &session_id,
                        &mut db,
                        &eventos,
                        &quien_sistema,
                        estado,
                        nivelador_para(frame.track, &mut nivelador_sistema),
                        &mut eco,
                    )
                } else {
                    Ok(())
                };
                if frame.track == Track::Mic && reconocedores.is_some() && resultado.is_ok() {
                    bloques_mic += 1;
                }
                let inferencia_ms = inicio_inferencia.elapsed().as_millis() as u64;
                if reconocedores.is_some() || resultado.is_err() {
                    let fin_audio_ms = frame
                        .timestamp_ms
                        .saturating_add(frame.duracion_ms())
                        .max(0) as u64;
                    let edad_audio_ms =
                        (inicio_captura.elapsed().as_millis() as u64).saturating_sub(fin_audio_ms);
                    metricas_vivo.entry(frame.track).or_default().registrar(
                        frame.duracion_ms().max(0) as u64,
                        espera_cola_ms,
                        inferencia_ms,
                        edad_audio_ms,
                        resultado.is_err(),
                    );
                    if ultimo_flush_vivo.elapsed() >= std::time::Duration::from_secs(5) {
                        volcar_metricas_vivo(
                            &ruta_metricas,
                            &mut metricas_vivo,
                            pendientes_en_cola(&pendientes),
                            hipotesis_descartadas.load(Ordering::Relaxed),
                            motor_en_vivo,
                            idioma_en_vivo.as_deref().unwrap_or("auto"),
                        );
                        ultimo_flush_vivo = Instant::now();
                    }
                }
                if let Err(e) = resultado {
                    tracing::warn!(error = %e, "Vosk falló durante la sesión; se cambia al respaldo Whisper");
                    let _ = eventos.send(Evento::Error {
                        mensaje: format!(
                            "Vosk falló durante la transcripción; se continuará con Whisper: {e}"
                        ),
                        recuperable: true,
                    });
                    reconocedores = None;
                    motor_en_vivo = "whisper";
                    trans = ruta_modelo
                        .as_ref()
                        .and_then(|ruta| match Transcriptor::cargar(ruta) {
                            Ok(t) => Some(t),
                            Err(error) => {
                                tracing::error!(error = %error, "no se pudo cargar Whisper como respaldo");
                                None
                            }
                        });
                }
                pendientes.fetch_sub(1, Ordering::SeqCst);
                continue;
            }
            Trabajo::Segmento(s, encolado) => (s, false, encolado),
            Trabajo::Hipotesis(s, encolado) => (s, true, encolado),
        };

        if reconocedores.is_some() {
            pendientes.fetch_sub(1, Ordering::SeqCst);
            continue;
        }

        let Some(trans) = trans.as_ref() else {
            pendientes.fetch_sub(1, Ordering::SeqCst);
            continue;
        };

        // El contexto previo mejora la continuidad, pero se aísla por pista
        // para que un ruido o alucinación del micrófono no contamine el audio
        // del sistema (ni viceversa). Además, solo se toma si está libre de alucinaciones
        // y se acota a las últimas palabras.
        let previo = match seg.track {
            Track::Mic => ultimo_texto_mic.as_deref(),
            Track::System => ultimo_texto_sistema.as_deref(),
        };

        opciones.contexto_previo = previo
            .filter(|t| !dictar_stt::limpieza::es_alucinacion(t))
            .map(|t| {
                let palabras: Vec<&str> = t.split_whitespace().collect();
                if palabras.len() > 20 {
                    palabras[palabras.len() - 20..].join(" ")
                } else {
                    t.to_string()
                }
            });

        let inicio_inferencia = Instant::now();
        let resultado = trans.transcribir(&seg.pcm, &opciones);
        let fin_inferencia = Instant::now();
        let duracion_audio_ms = seg.duracion_ms().max(0) as u64;
        let duracion_inferencia_ms = fin_inferencia
            .saturating_duration_since(inicio_inferencia)
            .as_millis() as u64;
        let edad_hipotesis_ms = fin_inferencia
            .saturating_duration_since(inicio_captura)
            .as_millis() as i64
            - seg.fin_ms;
        registrar_metrica(
            &ruta_metricas,
            serde_json::json!({
                "modelo": &modelo,
                "idioma": opciones.idioma.as_deref().unwrap_or("auto"),
                "hilos": opciones.hilos,
                "sin_marcas_de_tiempo": opciones.sin_marcas_de_tiempo,
                "pista": seg.track.as_str(),
                "tipo": if es_hipotesis { "hipotesis" } else { "segmento" },
                "inicio_audio_ms": seg.inicio_ms,
                "fin_audio_ms": seg.fin_ms,
                "duracion_audio_ms": duracion_audio_ms,
                "espera_cola_ms": inicio_inferencia
                    .saturating_duration_since(encolado_en)
                    .as_millis() as u64,
                "inferencia_ms": duracion_inferencia_ms,
                "edad_al_final_inferencia_ms": edad_hipotesis_ms.max(0),
                "rtf": if duracion_audio_ms == 0 {
                    0.0
                } else {
                    duracion_inferencia_ms as f64 / duracion_audio_ms as f64
                },
                "cola_pendiente": pendientes_en_cola(&pendientes),
                "hipotesis_descartadas_total": hipotesis_descartadas.load(Ordering::Relaxed),
                "exito": resultado.is_ok(),
            }),
        );

        match resultado {
            Ok(fragmentos) => {
                let textos: Vec<&str> = fragmentos.iter().map(|f| f.texto.as_str()).collect();
                let (indices, _) = dictar_stt::limpieza::colapsar_bucles(&textos);

                let texto_hipotesis = indices
                    .iter()
                    .filter(|&&i| !dictar_stt::limpieza::es_alucinacion(&fragmentos[i].texto))
                    .map(|&i| fragmentos[i].texto.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");

                if es_hipotesis {
                    let acuerdo = match seg.track {
                        Track::Mic => &mut acuerdo_mic,
                        Track::System => &mut acuerdo_sistema,
                    };
                    let (_, provisional) = acuerdo.actualizar(&texto_hipotesis);
                    let estable = acuerdo.texto_confirmado();

                    let _ = eventos.send(Evento::frase_incremental(
                        &session_id,
                        seg.track,
                        seg.inicio_ms,
                        seg.fin_ms,
                        &estable,
                        &provisional,
                    ));
                    pendientes.fetch_sub(1, Ordering::SeqCst);
                    continue;
                }

                // También se manda un marcador vacío para retirar el borrador
                // cuando el segmento definitivo no contiene voz útil.
                if !seg.merece_transcribirse() {
                    let _ = eventos.send(Evento::frase(
                        &session_id,
                        seg.track,
                        seg.inicio_ms,
                        seg.fin_ms,
                        "",
                        true,
                    ));
                    match seg.track {
                        Track::Mic => acuerdo_mic = dictar_stt::acuerdo::AcuerdoLocal::nuevo(),
                        Track::System => {
                            acuerdo_sistema = dictar_stt::acuerdo::AcuerdoLocal::nuevo()
                        }
                    }
                    pendientes.fetch_sub(1, Ordering::SeqCst);
                    continue;
                }

                let _ = eventos.send(Evento::frase(
                    &session_id,
                    seg.track,
                    seg.inicio_ms,
                    seg.fin_ms,
                    "",
                    true,
                ));

                // El segmento acaba de cerrarse: el texto definitivo
                // reemplazará el borrador y el siguiente segmento empieza un
                // nuevo acuerdo independiente.
                match seg.track {
                    Track::Mic => acuerdo_mic = dictar_stt::acuerdo::AcuerdoLocal::nuevo(),
                    Track::System => acuerdo_sistema = dictar_stt::acuerdo::AcuerdoLocal::nuevo(),
                }

                let mut frases = Vec::new();
                if opciones.sin_marcas_de_tiempo {
                    // En borradores, whisper.cpp no entrega timestamps de
                    // fragmento fiables. Guardar cada fragmento en el mismo
                    // rango también haría que la UI los reemplazara entre sí
                    // por compartir la clave (pista, inicio). Se conserva el
                    // texto del segmento VAD como una sola frase temporal.
                    let validos: Vec<_> = indices
                        .iter()
                        .map(|&i| &fragmentos[i])
                        .filter(|f| !dictar_stt::limpieza::es_alucinacion(&f.texto))
                        .collect();
                    let texto = validos
                        .iter()
                        .map(|f| f.texto.as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    if !texto.is_empty() {
                        let confianza =
                            validos.iter().map(|f| f.confianza).sum::<f32>() / validos.len() as f32;
                        frases.push(NewUtterance {
                            track: seg.track,
                            speaker: Some(match seg.track {
                                Track::Mic => "Yo".to_owned(),
                                Track::System => quien_sistema.clone(),
                            }),
                            start_ms: seg.inicio_ms,
                            end_ms: seg.fin_ms,
                            text: texto,
                            confidence: Some(confianza),
                            is_final: false,
                        });
                    }
                } else {
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
                            // Es una frase cerrada para la interfaz, pero su texto
                            // procede del modelo pequeño y se reemplaza al procesar
                            // la sesión con la pasada local definitiva.
                            is_final: false,
                        };

                        frases.push(u);
                    }
                }

                let desde_historial_ms = seg.inicio_ms - SOLAPE_MS - 4_000;
                conservar_historial_reciente(&mut frases_recientes, desde_historial_ms);
                let descartadas =
                    deduplicar_solape(&frases_recientes, &mut frases, seg.inicio_ms, SOLAPE_MS);
                if descartadas > 0 {
                    tracing::info!(descartadas, "frases duplicadas en el solape en vivo");
                }

                for u in &frases {
                    let _ = eventos.send(Evento::frase(
                        &session_id,
                        seg.track,
                        u.start_ms,
                        u.end_ms,
                        &u.text,
                        true,
                    ));

                    // Solo se actualiza el contexto previo si la frase es limpia.
                    match seg.track {
                        Track::Mic => ultimo_texto_mic = Some(u.text.clone()),
                        Track::System => ultimo_texto_sistema = Some(u.text.clone()),
                    }
                }

                if !frases.is_empty() {
                    if let Err(e) = db.insertar_frases(&session_id, &frases) {
                        tracing::warn!(error = %e, "no se pudieron guardar las frases");
                    }
                    frases_recientes.extend(frases);
                }
            }
            Err(e) => {
                // Un segmento fallido no detiene la clase: se pierde un tramo
                // de transcripción, no la grabación.
                tracing::warn!(error = %e, "segmento sin transcribir");
                if !es_hipotesis {
                    let _ = eventos.send(Evento::frase(
                        &session_id,
                        seg.track,
                        seg.inicio_ms,
                        seg.fin_ms,
                        "",
                        true,
                    ));
                    match seg.track {
                        Track::Mic => acuerdo_mic = dictar_stt::acuerdo::AcuerdoLocal::nuevo(),
                        Track::System => {
                            acuerdo_sistema = dictar_stt::acuerdo::AcuerdoLocal::nuevo()
                        }
                    }
                }
            }
        }

        pendientes.fetch_sub(1, Ordering::SeqCst);
    }

    volcar_metricas_vivo(
        &ruta_metricas,
        &mut metricas_vivo,
        pendientes_en_cola(&pendientes),
        hipotesis_descartadas.load(Ordering::Relaxed),
        motor_en_vivo,
        idioma_en_vivo.as_deref().unwrap_or("auto"),
    );

    if let Some(reconocedores) = reconocedores.as_mut() {
        // Primero el sistema: su última frase tiene que estar anotada antes de
        // decidir si la del micrófono es su eco. El orden de un HashMap no lo
        // garantiza.
        for track in [Track::System, Track::Mic] {
            let Some(motor) = reconocedores.get_mut(&track) else {
                continue;
            };
            let estado = estado_vivo
                .get_mut(&track)
                .expect("hay estado incremental por pista");
            if let Some(inicio_ms) = estado.inicio_ms.take() {
                if let Ok(texto) = motor.finalizar() {
                    let fin = estado.fin_ms_actual.take().unwrap_or(inicio_ms + 1_000);
                    cerrar_frase_incremental(
                        texto,
                        track,
                        inicio_ms,
                        fin,
                        &session_id,
                        &mut db,
                        &eventos,
                        &quien_sistema,
                        &mut eco,
                    );
                }
            }
        }
    }

    // El marcador se escribe al final y solo si todo salió bien: su ausencia
    // es el estado seguro (el cierre transcribe el micrófono por lotes).
    let tramo_perdido_al_final = ultima_perdida_por_pista
        .lock()
        .unwrap()
        .contains_key(&Track::Mic);
    if mic_cubierto_en_vivo(
        reconocedores.is_some(),
        bloques_mic,
        mic_con_huecos || tramo_perdido_al_final,
    ) {
        if let Some(carpeta) = ruta_metricas.parent() {
            escribir_marcador_mic(carpeta);
        }
    }

    tracing::info!(sesion = %session_id, "transcripción en vivo terminada");
}

fn notificar_sin_motor(
    error: impl std::fmt::Display,
    eventos: &Sender<Evento>,
    rx: &Receiver<Trabajo>,
    pendientes: &AtomicUsize,
) {
    tracing::error!(error = %error, "sin transcripción en vivo");
    let _ = eventos.send(Evento::Error {
        mensaje: format!("No se pudo cargar el modelo: {error}"),
        recuperable: true,
    });
    while let Ok(trabajo) = rx.recv() {
        if matches!(trabajo, Trabajo::Terminar) {
            break;
        }
        pendientes.fetch_sub(1, Ordering::SeqCst);
    }
}

enum ReconocedorVivo {
    Vosk(dictar_stt::vosk::VoskVivo),
    #[cfg(target_os = "windows")]
    Sherpa(dictar_stt::sherpa::SherpaVivo),
}

impl ReconocedorVivo {
    fn aceptar(&mut self, pcm: &[f32]) -> dictar_stt::Result<(String, bool)> {
        match self {
            Self::Vosk(motor) => motor.aceptar(pcm),
            #[cfg(target_os = "windows")]
            Self::Sherpa(motor) => motor.aceptar(pcm),
        }
    }

    fn finalizar(&mut self) -> dictar_stt::Result<String> {
        match self {
            Self::Vosk(motor) => motor.finalizar(),
            #[cfg(target_os = "windows")]
            Self::Sherpa(motor) => motor.finalizar(),
        }
    }

    fn reiniciar(&mut self) {
        match self {
            Self::Vosk(motor) => motor.reiniciar(),
            #[cfg(target_os = "windows")]
            Self::Sherpa(motor) => motor.reiniciar(),
        }
    }
}

fn crear_reconocedores_vosk(idioma: &str) -> dictar_stt::Result<HashMap<Track, ReconocedorVivo>> {
    let ruta = dictar_stt::vosk::modelo_para_idioma(idioma)?;
    let modelo = Arc::new(dictar_stt::vosk::VoskModelo::cargar(&ruta)?);
    let mic = dictar_stt::vosk::VoskModelo::nuevo_reconocedor(&modelo)?;
    let sistema = dictar_stt::vosk::VoskModelo::nuevo_reconocedor(&modelo)?;
    Ok(HashMap::from([
        (Track::Mic, ReconocedorVivo::Vosk(mic)),
        (Track::System, ReconocedorVivo::Vosk(sistema)),
    ]))
}

/// Valor de producción de la nivelación de la pista de sistema con Sherpa.
///
/// Sigue en `false` hasta que `sherpa_live_eval` demuestre, con las reglas de
/// decisión de AV-3, que mejora; ver el HANDOFF de esa tarea.
const NIVELAR_SISTEMA_POR_DEFECTO: bool = false;
const VAR_NIVELAR: &str = "DICTAR_STT_NIVELAR";

/// Único sitio donde se lee `DICTAR_STT_NIVELAR`. Un valor que no sea 0 o 1 se
/// avisa a la interfaz y se usa el de producción, en vez de medir sin saberlo
/// una configuración distinta de la pedida.
fn nivelar_desde_entorno(eventos: &Sender<Evento>) -> bool {
    let valor = std::env::var(VAR_NIVELAR).ok();
    match interpretar_interruptor(VAR_NIVELAR, valor.as_deref(), NIVELAR_SISTEMA_POR_DEFECTO) {
        Ok(nivelar) => nivelar,
        Err(mensaje) => {
            tracing::warn!(%mensaje, "interruptor de nivelación ignorado");
            let _ = eventos.send(Evento::Error {
                mensaje,
                recuperable: true,
            });
            NIVELAR_SISTEMA_POR_DEFECTO
        }
    }
}

/// El nivelador que corresponde a una pista: solo la de sistema tiene uno.
fn nivelador_para(track: Track, sistema: &mut Option<Nivelador>) -> Option<&mut Nivelador> {
    match track {
        Track::System => sistema.as_mut(),
        Track::Mic => None,
    }
}

/// Audio que se entrega al motor: una copia nivelada si hay nivelador, o el
/// original sin copiar. El WAV ya se escribió antes y no pasa por aquí.
fn audio_para_motor<'a>(
    pcm: &'a [f32],
    nivelador: Option<&mut Nivelador>,
) -> std::borrow::Cow<'a, [f32]> {
    match nivelador {
        Some(nivelador) => {
            let mut copia = pcm.to_vec();
            nivelador.procesar(&mut copia);
            std::borrow::Cow::Owned(copia)
        }
        None => std::borrow::Cow::Borrowed(pcm),
    }
}

#[cfg(target_os = "windows")]
fn crear_reconocedores_sherpa(
    glosario: &str,
) -> dictar_stt::Result<HashMap<Track, ReconocedorVivo>> {
    let modelo = dictar_stt::sherpa::modelo_es_compartido()?;
    let terminos = modelo.preparar_terminos(glosario);
    if modelo.opciones().palabras_clave {
        tracing::info!(
            cargados = terminos.aceptados.len(),
            rechazados = terminos.rechazados.len(),
            "palabras clave de Sherpa preparadas"
        );
    }
    let mic = dictar_stt::sherpa::SherpaModelo::nuevo_flujo_con_terminos(&modelo, &terminos);
    let sistema = dictar_stt::sherpa::SherpaModelo::nuevo_flujo_con_terminos(&modelo, &terminos);
    Ok(HashMap::from([
        (Track::Mic, ReconocedorVivo::Sherpa(mic)),
        (Track::System, ReconocedorVivo::Sherpa(sistema)),
    ]))
}

#[allow(clippy::too_many_arguments)]
struct EstadoIncremental {
    inicio_ms: Option<i64>,
    fin_ms_actual: Option<i64>,
    ultimo_muestreo_acuerdo_ms: Option<i64>,
    acuerdo: dictar_stt::acuerdo::AcuerdoLocal,
    visible: (String, usize),
}

impl EstadoIncremental {
    fn nuevo() -> Self {
        Self {
            inicio_ms: None,
            fin_ms_actual: None,
            ultimo_muestreo_acuerdo_ms: None,
            acuerdo: dictar_stt::acuerdo::AcuerdoLocal::nuevo(),
            visible: (String::new(), 0),
        }
    }

    fn reiniciar(&mut self) {
        self.inicio_ms = None;
        self.fin_ms_actual = None;
        self.ultimo_muestreo_acuerdo_ms = None;
        self.acuerdo = dictar_stt::acuerdo::AcuerdoLocal::nuevo();
        self.visible = (String::new(), 0);
    }
}

#[allow(clippy::too_many_arguments)]
fn procesar_bloque_incremental(
    motor: &mut ReconocedorVivo,
    frame: &dictar_audio::AudioFrame,
    session_id: &SessionId,
    db: &mut dictar_storage::Db,
    eventos: &Sender<Evento>,
    quien_sistema: &str,
    estado: &mut EstadoIncremental,
    nivelador: Option<&mut Nivelador>,
    eco: &mut ContextoEco,
) -> dictar_stt::Result<()> {
    const INTERVALO_ACUERDO_MS: i64 = 500;

    let (texto, cerrado) = motor.aceptar(&audio_para_motor(&frame.pcm, nivelador))?;
    let fin = frame.timestamp_ms + frame.duracion_ms();
    if cerrado {
        let inicio = estado.inicio_ms.unwrap_or(frame.timestamp_ms);
        cerrar_frase_incremental(
            texto,
            frame.track,
            inicio,
            fin,
            session_id,
            db,
            eventos,
            quien_sistema,
            eco,
        );
        estado.reiniciar();
    } else if !texto.is_empty() {
        // El reconocedor emite hipótesis a intervalos cortos (~64 ms). Dos
        // resultados consecutivos pueden ser casi idénticos y confirmar
        // palabras demasiado pronto, antes de que llegue el contexto acústico
        // siguiente. Conservamos la decodificación continua, pero comparamos
        // hipótesis separadas al menos medio segundo para marcar el prefijo.
        if estado
            .ultimo_muestreo_acuerdo_ms
            .is_some_and(|ultimo| fin.saturating_sub(ultimo) < INTERVALO_ACUERDO_MS)
        {
            return Ok(());
        }
        estado.ultimo_muestreo_acuerdo_ms = Some(fin);

        let inicio = *estado
            .inicio_ms
            .get_or_insert_with(|| frame.timestamp_ms.saturating_sub(800).max(0));
        estado.fin_ms_actual = Some(fin);
        let (_, provisional) = estado.acuerdo.actualizar(&texto);
        let estable = estado.acuerdo.texto_confirmado();
        let actual = if estable.is_empty() {
            provisional.clone()
        } else if provisional.is_empty() {
            estable.clone()
        } else {
            format!("{estable} {provisional}")
        };
        if frame.track == Track::System {
            eco.registrar_hipotesis(inicio, fin, &actual);
        }
        let estable_utf16 = estable.encode_utf16().count();
        if actual != estado.visible.0 || estable_utf16 != estado.visible.1 {
            let _ = eventos.send(Evento::frase_incremental(
                session_id,
                frame.track,
                inicio,
                fin,
                &estable,
                &provisional,
            ));
            estado.visible = (actual, estable_utf16);
        }
    }
    Ok(())
}

/// Marcador que se deja en la carpeta de audio cuando el micrófono quedó
/// cubierto en vivo; ver [`mic_cubierto_en_vivo`]. El cierre de la sesión
/// (`proceso::falta_microfono_tras_vivo`) lo lee para no re-transcribir.
pub(crate) const MARCADOR_MIC_EN_VIVO: &str = "mic_en_vivo.ok";

/// Margen a cada lado de una frase del micrófono dentro del cual se busca su
/// eco en el sistema. El retorno acústico medido va de 100 a 500 ms; el resto
/// absorbe la diferencia de cierre entre los dos reconocedores.
const VENTANA_ECO_MS: i64 = 1_500;
/// Cuánto texto de sistema se conserva para comparar.
const HISTORIAL_ECO_MS: i64 = 30_000;
/// Porcentaje de palabras de contenido del micrófono que deben aparecer en el
/// sistema para tomar la frase por eco (si tiene tres o más).
const PORCENTAJE_ECO: usize = 60;

/// Palabras gramaticales: las comparten dos frases distintas cualquiera, así
/// que no pueden servir de prueba de eco.
const PALABRAS_VACIAS: &[&str] = &[
    "el", "la", "los", "las", "un", "una", "unos", "unas", "de", "del", "y", "e", "o", "a", "al",
    "en", "que", "es", "se", "lo", "por", "con", "para", "su", "sus",
];

struct FraseSistema {
    inicio_ms: i64,
    fin_ms: i64,
    texto: String,
}

/// Lo último que dijo la pista de sistema, para reconocer el eco de los
/// altavoces en el micrófono.
#[derive(Default)]
struct ContextoEco {
    cerradas: Vec<FraseSistema>,
    hipotesis: Option<FraseSistema>,
}

impl ContextoEco {
    fn registrar_hipotesis(&mut self, inicio_ms: i64, fin_ms: i64, texto: &str) {
        self.hipotesis = Some(FraseSistema {
            inicio_ms,
            fin_ms,
            texto: texto.to_owned(),
        });
    }

    /// Una frase cerrada sustituye a la hipótesis que la precedía.
    fn registrar_cerrada(&mut self, inicio_ms: i64, fin_ms: i64, texto: &str) {
        self.hipotesis = None;
        if texto.trim().is_empty() {
            return;
        }
        self.cerradas.push(FraseSistema {
            inicio_ms,
            fin_ms,
            texto: texto.to_owned(),
        });
        let corte = fin_ms - HISTORIAL_ECO_MS;
        self.cerradas.retain(|f| f.fin_ms >= corte);
    }

    /// Si una frase cerrada del micrófono es el eco de lo que sonaba por los
    /// altavoces en esos mismos instantes.
    fn es_eco(&self, inicio_ms: i64, fin_ms: i64, texto_mic: &str) -> bool {
        let desde = inicio_ms - VENTANA_ECO_MS;
        let hasta = fin_ms + VENTANA_ECO_MS;
        let sistema = self
            .cerradas
            .iter()
            .chain(self.hipotesis.iter())
            .filter(|f| f.fin_ms >= desde && f.inicio_ms <= hasta)
            .map(|f| f.texto.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        texto_es_eco(texto_mic, &sistema)
    }
}

/// Minúsculas, sin acentos ni puntuación, separado en palabras.
fn palabras_normalizadas(texto: &str) -> Vec<String> {
    let limpio: String = texto
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect();
    limpio.split_whitespace().map(str::to_owned).collect()
}

/// Si el texto del micrófono repite al del sistema.
///
/// Se cuentan solo las palabras de contenido: «el» o «de» aparecen en
/// cualquier frase y harían pasar por eco la voz propia. Con tres o más, basta
/// el 60 %; con una o dos se exige coincidencia total, porque con tan poco
/// texto el azar pesa demasiado. Una frase hecha solo de palabras vacías
/// nunca es eco: es mejor conservar de más que borrar voz del usuario.
fn texto_es_eco(texto_mic: &str, texto_sistema: &str) -> bool {
    let sistema = palabras_normalizadas(texto_sistema);
    if sistema.is_empty() {
        return false;
    }
    let contenido: Vec<String> = palabras_normalizadas(texto_mic)
        .into_iter()
        .filter(|p| !PALABRAS_VACIAS.contains(&p.as_str()))
        .collect();
    if contenido.is_empty() {
        return false;
    }
    let coincidentes = contenido.iter().filter(|p| sistema.contains(p)).count();
    if contenido.len() >= 3 {
        coincidentes * 100 >= contenido.len() * PORCENTAJE_ECO
    } else {
        coincidentes == contenido.len()
    }
}

/// Si el motor incremental dejó cubierto el micrófono durante toda la sesión:
/// activo hasta el final (no cayó al respaldo Whisper), con audio procesado y
/// sin tramos descartados por cola llena.
fn mic_cubierto_en_vivo(motor_activo: bool, bloques_mic: u64, con_huecos: bool) -> bool {
    motor_activo && bloques_mic > 0 && !con_huecos
}

fn escribir_marcador_mic(carpeta: &Path) {
    let ruta = carpeta.join(MARCADOR_MIC_EN_VIVO);
    // Si no se puede escribir no hay daño: el cierre vuelve a transcribir el
    // micrófono por lotes. Pero se dice, para no perder la mejora en silencio.
    if let Err(error) = std::fs::write(&ruta, "micrófono transcrito en vivo\n") {
        tracing::warn!(error = %error, ruta = %ruta.display(), "no se pudo escribir el marcador del micrófono en vivo");
    }
}

/// Cierra una frase incremental: el sistema se anota para comparar y la del
/// micrófono se descarta si es eco de los altavoces.
#[allow(clippy::too_many_arguments)]
fn cerrar_frase_incremental(
    texto: String,
    track: Track,
    inicio_ms: i64,
    fin_ms: i64,
    session_id: &SessionId,
    db: &mut dictar_storage::Db,
    eventos: &Sender<Evento>,
    quien_sistema: &str,
    eco: &mut ContextoEco,
) {
    let fin_ms = fin_ms.max(inicio_ms);
    match track {
        Track::System => eco.registrar_cerrada(inicio_ms, fin_ms, &texto),
        Track::Mic => {
            if eco.es_eco(inicio_ms, fin_ms, &texto) {
                tracing::info!(inicio_ms, fin_ms, "frase del micrófono descartada por eco");
                // Un incremental vacío retira el borrador de esa pista y rango
                // en la interfaz; no hay frase definitiva que sustituirlo.
                let _ = eventos.send(Evento::frase_incremental(
                    session_id,
                    Track::Mic,
                    inicio_ms,
                    fin_ms,
                    "",
                    "",
                ));
                return;
            }
        }
    }
    guardar_frase_incremental(
        texto,
        track,
        inicio_ms,
        fin_ms,
        session_id,
        db,
        eventos,
        quien_sistema,
    );
}

fn guardar_frase_incremental(
    texto: String,
    track: Track,
    inicio_ms: i64,
    fin_ms: i64,
    session_id: &SessionId,
    db: &mut dictar_storage::Db,
    eventos: &Sender<Evento>,
    quien_sistema: &str,
) {
    let texto = texto.trim().to_owned();
    let _ = eventos.send(Evento::frase(
        session_id,
        track,
        inicio_ms,
        fin_ms.max(inicio_ms),
        &texto,
        true,
    ));
    if texto.is_empty() {
        return;
    }
    let frase = NewUtterance {
        track,
        speaker: Some(match track {
            Track::Mic => "Yo".to_owned(),
            Track::System => quien_sistema.to_owned(),
        }),
        start_ms: inicio_ms,
        end_ms: fin_ms.max(inicio_ms),
        text: texto,
        confidence: None,
        is_final: false,
    };
    if let Err(e) = db.insertar_frases(session_id, &[frase]) {
        tracing::warn!(error = %e, "no se pudo guardar frase incremental");
    }
}

#[derive(Default)]
struct ResumenBloquesVivo {
    bloques: u64,
    audio_ms: u64,
    espera_cola_total_ms: u128,
    espera_cola_max_ms: u64,
    inferencia_total_ms: u128,
    inferencia_max_ms: u64,
    edad_audio_total_ms: u128,
    edad_audio_max_ms: u64,
    fallos: u64,
}

impl ResumenBloquesVivo {
    fn registrar(
        &mut self,
        audio_ms: u64,
        espera_cola_ms: u64,
        inferencia_ms: u64,
        edad_audio_ms: u64,
        fallo: bool,
    ) {
        self.bloques += 1;
        self.audio_ms += audio_ms;
        self.espera_cola_total_ms += espera_cola_ms as u128;
        self.espera_cola_max_ms = self.espera_cola_max_ms.max(espera_cola_ms);
        self.inferencia_total_ms += inferencia_ms as u128;
        self.inferencia_max_ms = self.inferencia_max_ms.max(inferencia_ms);
        self.edad_audio_total_ms += edad_audio_ms as u128;
        self.edad_audio_max_ms = self.edad_audio_max_ms.max(edad_audio_ms);
        self.fallos += u64::from(fallo);
    }
}

fn volcar_metricas_vivo(
    ruta: &Path,
    metricas: &mut HashMap<Track, ResumenBloquesVivo>,
    cola_pendiente: usize,
    trabajos_descartados_total: usize,
    motor: &str,
    idioma: &str,
) {
    for (pista, m) in std::mem::take(metricas) {
        if m.bloques == 0 {
            continue;
        }
        let bloques = m.bloques as u128;
        registrar_metrica(
            ruta,
            serde_json::json!({
                "modelo": motor,
                "idioma": idioma,
                "pista": pista.as_str(),
                "tipo": format!("{}_bloques", if motor.starts_with("vosk") { "vosk" } else { "streaming" }),
                "bloques": m.bloques,
                "audio_ms": m.audio_ms,
                "espera_cola_ms": m.espera_cola_total_ms / bloques,
                "espera_cola_max_ms": m.espera_cola_max_ms,
                "inferencia_ms": m.inferencia_total_ms / bloques,
                "inferencia_max_ms": m.inferencia_max_ms,
                "edad_al_final_inferencia_ms": m.edad_audio_total_ms / bloques,
                "edad_audio_max_ms": m.edad_audio_max_ms,
                "rtf": if m.audio_ms == 0 {
                    0.0
                } else {
                    m.inferencia_total_ms as f64 / m.audio_ms as f64
                },
                "fallos": m.fallos,
                "exito": m.fallos == 0,
                "cola_pendiente": cola_pendiente,
                "hipotesis_descartadas_total": trabajos_descartados_total,
                "trabajos_descartados_total": trabajos_descartados_total,
            }),
        );
    }
}

fn pendientes_en_cola(pendientes: &AtomicUsize) -> usize {
    pendientes.load(Ordering::SeqCst).saturating_sub(1)
}

/// Guarda métricas de rendimiento sin texto ni audio de la sesión.
fn registrar_metrica(ruta: &Path, dato: serde_json::Value) {
    if let Some(parent) = ruta.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            tracing::warn!(error = %error, "no se pudo crear la carpeta de métricas STT");
            return;
        }
    }

    let resultado = OpenOptions::new()
        .create(true)
        .append(true)
        .open(ruta)
        .and_then(|mut archivo| writeln!(archivo, "{dato}"));
    if let Err(error) = resultado {
        tracing::warn!(error = %error, "no se pudo guardar la métrica STT");
    }
}

#[cfg(test)]
mod tests_nivelacion {
    use super::*;

    fn tono_suave(muestras: usize) -> Vec<f32> {
        (0..muestras)
            .map(|i| 0.01 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 16_000.0).sin())
            .collect()
    }

    #[test]
    fn el_microfono_nunca_recibe_nivelador_aunque_haya_uno_para_el_sistema() {
        // Nivelar el micrófono amplificaría el eco de los altavoces (correlación
        // 0,71–0,74 medida) tanto como la voz.
        let mut sistema = Some(Nivelador::nuevo());
        assert!(nivelador_para(Track::Mic, &mut sistema).is_none());
        assert!(nivelador_para(Track::System, &mut sistema).is_some());
        let mut ninguno = None;
        assert!(nivelador_para(Track::System, &mut ninguno).is_none());
    }

    #[test]
    fn sin_nivelador_el_motor_recibe_el_audio_original_sin_copiarlo() {
        let pcm = tono_suave(1_024);
        let entregado = audio_para_motor(&pcm, None);
        assert!(matches!(entregado, std::borrow::Cow::Borrowed(_)));
        assert_eq!(&*entregado, &pcm[..]);
    }

    #[test]
    fn con_nivelador_el_motor_recibe_una_copia_mas_fuerte_y_el_original_no_cambia() {
        let original = tono_suave(16_000 * 8);
        let guardado = original.clone();
        let mut nivelador = Nivelador::nuevo();
        let mut ultimo = Vec::new();
        // Bloques de 1024 como los de la captura real.
        for bloque in original.chunks(1_024) {
            ultimo = audio_para_motor(bloque, Some(&mut nivelador)).into_owned();
        }
        assert_eq!(original, guardado, "el audio de origen no debe tocarse");
        let pico = |v: &[f32]| v.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(pico(&ultimo) > pico(&original[original.len() - ultimo.len()..]) * 1.5);
    }
}

#[cfg(test)]
mod tests_eco {
    use super::*;

    #[test]
    fn el_eco_exacto_del_sistema_se_reconoce() {
        assert!(texto_es_eco(
            "La derivada de la función es la pendiente",
            "la derivada de la función es la pendiente"
        ));
    }

    #[test]
    fn el_eco_con_acentos_y_puntuacion_distintos_se_reconoce() {
        // Los dos reconocedores no coinciden en tildes ni en comas.
        assert!(texto_es_eco(
            "Segunda derivada, función cóncava",
            "segunda derivada funcion concava"
        ));
    }

    #[test]
    fn una_frase_de_microfono_con_retraso_dentro_de_la_ventana_es_eco() {
        // Antes no había filtro: el mismo texto salía en las dos pistas.
        let mut eco = ContextoEco::default();
        eco.registrar_cerrada(
            10_000,
            14_000,
            "el teorema de Bayes relaciona probabilidades",
        );
        // El retorno llega ~400 ms tarde y cierra después que el sistema.
        assert!(eco.es_eco(
            10_400,
            14_500,
            "el teorema de bayes relaciona probabilidades"
        ));
    }

    #[test]
    fn el_eco_se_reconoce_contra_la_hipotesis_aun_sin_cerrar() {
        let mut eco = ContextoEco::default();
        eco.registrar_hipotesis(20_000, 22_000, "vamos a integrar por partes");
        assert!(eco.es_eco(20_300, 22_300, "vamos a integrar por partes"));
        // Al cerrarse la frase del sistema, la hipótesis deja de contar sola.
        eco.registrar_cerrada(20_000, 22_500, "vamos a integrar por partes");
        assert!(eco.hipotesis.is_none());
        assert!(eco.es_eco(20_300, 22_300, "vamos a integrar por partes"));
    }

    #[test]
    fn la_voz_propia_que_solo_comparte_palabras_vacias_no_es_eco() {
        // «el» y «de» están en casi cualquier frase: no pueden delatar eco.
        assert!(!texto_es_eco(
            "el examen de mañana será difícil",
            "el profesor de cálculo explica la derivada"
        ));
        assert!(!texto_es_eco("el de", "el profesor de cálculo"));
    }

    #[test]
    fn una_frase_corta_solo_es_eco_si_coincide_entera() {
        assert!(texto_es_eco("derivada", "hablemos de la derivada"));
        assert!(!texto_es_eco("derivada parcial", "hablemos de la derivada"));
        assert!(!texto_es_eco("sí", "hablemos de la derivada"));
    }

    #[test]
    fn con_tres_palabras_o_mas_basta_el_sesenta_por_ciento() {
        let sistema = "integral definida area";
        // 3 de 5 palabras de contenido = 60 %: justo en el umbral.
        assert!(texto_es_eco("integral definida area curva unidad", sistema));
        // 2 de 5 = 40 %: voz propia que menciona el mismo tema.
        assert!(!texto_es_eco("integral curva unidad limite serie", sistema));
    }

    #[test]
    fn el_sistema_que_hablo_mucho_antes_no_cuenta_como_eco() {
        let mut eco = ContextoEco::default();
        eco.registrar_cerrada(1_000, 3_000, "la matriz inversa existe");
        // El profesor lo dijo 20 s antes: el usuario lo repite por su cuenta.
        assert!(!eco.es_eco(23_000, 25_000, "la matriz inversa existe"));
    }

    #[test]
    fn el_historial_de_sistema_queda_acotado() {
        let mut eco = ContextoEco::default();
        for i in 0..100 {
            eco.registrar_cerrada(i * 2_000, i * 2_000 + 1_500, "texto cualquiera");
        }
        // 30 s de historial a 2 s por frase: unas 16 frases, no 100.
        assert!(eco.cerradas.len() <= 17, "{}", eco.cerradas.len());
    }

    #[test]
    fn sin_texto_de_sistema_nada_es_eco() {
        assert!(!ContextoEco::default().es_eco(0, 1_000, "hola a todos"));
        assert!(!texto_es_eco("", "algo"));
    }

    #[test]
    fn el_microfono_solo_cuenta_como_cubierto_si_el_motor_no_se_cayo_ni_perdio_audio() {
        assert!(mic_cubierto_en_vivo(true, 500, false));
        // Caída al respaldo Whisper: el micrófono no estuvo todo el tiempo.
        assert!(!mic_cubierto_en_vivo(false, 500, false));
        // Cola llena: hubo tramos sin procesar.
        assert!(!mic_cubierto_en_vivo(true, 500, true));
        // Nunca llegó audio del micrófono.
        assert!(!mic_cubierto_en_vivo(true, 0, false));
    }

    #[test]
    fn el_marcador_se_escribe_con_el_nombre_que_lee_el_cierre() {
        let dir = std::env::temp_dir().join(format!("dictar_marcador_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        escribir_marcador_mic(&dir);
        assert!(dir.join(MARCADOR_MIC_EN_VIVO).is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
