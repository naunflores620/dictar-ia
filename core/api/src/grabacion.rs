//! Sesión de grabación en curso.
//!
//! El hilo de escritura hace una cosa por encima de todas: **volcar el audio a
//! disco antes de procesarlo**. Si la aplicación muere en el minuto 90 de una
//! clase de dos horas, el archivo tiene que estar completo. Todo lo demás
//! —niveles, transcripción en vivo— es secundario y puede fallar sin
//! consecuencias.

use crate::eventos::Evento;
use crate::{ApiError, Nucleo, Result};
use dictar_audio::wav::EscritorPistas;
use dictar_audio::{CaptureConfig, CaptureSession};
use dictar_domain::{EpochMs, Session, SessionId, SessionKind, SessionSource, TopicId, Track};
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
    /// Guardar una captura cada vez que cambia la diapositiva compartida.
    pub capturar_diapositivas: bool,
}

impl Default for OpcionesGrabacion {
    fn default() -> Self {
        Self {
            kind: SessionKind::Class,
            topic_id: None,
            titulo: None,
            capturar_sistema: true,
            solo_local: false,
            capturar_diapositivas: true,
        }
    }
}

/// Grabación viva. Al detenerla, cierra los archivos y deja la sesión lista
/// para procesar.
pub struct Grabacion {
    pub session_id: SessionId,
    /// Captura de diapositivas, si se pidió. Vive aparte del audio: si falla,
    /// la grabación sigue. Nunca al revés.
    pantalla: Option<dictar_screen::SesionCaptura>,
    laminas: Option<std::sync::mpsc::Receiver<dictar_screen::SlideCapturada>>,
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

        let dir_audio = self.dir_datos().join("audio").join(sesion.id.as_str());
        sesion.audio_path = Some(dir_audio.to_string_lossy().into_owned());

        self.con_db(|db| db.crear_sesion(&sesion))?;

        let cfg = CaptureConfig {
            capturar_sistema: opts.capturar_sistema,
            capturar_microfono: true,
            ..Default::default()
        };

        let (rx_audio, captura) = dictar_audio::iniciar(cfg)?;
        let (tx_ev, rx_ev) = mpsc::channel::<Evento>();

        let parar = Arc::new(AtomicBool::new(false));
        let inicio = Instant::now();

        let hilo = {
            let dir = dir_audio.clone();
            let parar = parar.clone();
            std::thread::Builder::new()
                .name("dictar-escritura".into())
                .spawn(move || {
                    if let Err(e) = bucle_escritura(dir, rx_audio, tx_ev.clone(), parar) {
                        tracing::error!(error = %e, "el hilo de escritura falló");
                        let _ = tx_ev.send(Evento::Error {
                            mensaje: e.to_string(),
                            // El audio ya escrito sigue en disco: la sesión se
                            // puede procesar igualmente.
                            recuperable: true,
                        });
                    }
                })
                .map_err(|e| ApiError::Config(e.to_string()))?
        };

        // La captura de pantalla es opcional y no puede tumbar la grabación:
        // si falla —no hay servidor gráfico, el portal deniega el permiso— se
        // avisa y se sigue grabando el audio, que es lo importante.
        let (laminas, pantalla) = if opts.capturar_diapositivas {
            match dictar_screen::iniciar(
                dir_audio.join("diapositivas"),
                dictar_screen::ConfigCaptura::default(),
            ) {
                Ok((rx, ses)) => (Some(rx), Some(ses)),
                Err(e) => {
                    tracing::warn!(error = %e, "sin captura de diapositivas; se graba solo el audio");
                    (None, None)
                }
            }
        } else {
            (None, None)
        };

        let id = sesion.id.clone();
        *self.grabacion.lock().unwrap() = Some(Grabacion {
            session_id: id.clone(),
            pantalla,
            laminas,
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
        if let Some(p) = grab.pantalla.take() {
            p.detener();
        }

        // Las diapositivas se vuelcan a la base al cerrar, no según llegan:
        // durante la clase el hilo de escritura no debe tocar SQLite, que es
        // donde la interfaz está leyendo.
        let mut laminas = 0;
        if let Some(rx) = grab.laminas.take() {
            while let Ok(s) = rx.try_recv() {
                let ruta = s.ruta.to_string_lossy().into_owned();
                if self
                    .con_db(|db| {
                        db.insertar_diapositiva(&grab.session_id, &ruta, s.ts_ms, &s.phash)
                    })
                    .is_ok()
                {
                    laminas += 1;
                }
            }
        }
        if laminas > 0 {
            tracing::info!(laminas, "diapositivas guardadas");
        }
        grab.parar.store(true, Ordering::SeqCst);
        if let Some(h) = grab.hilo.take() {
            let _ = h.join();
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
    /// Es la vía manual: el usuario decide el momento, así que el encuadre y
    /// la lámina son los correctos por definición. La detección automática
    /// nunca acertará siempre —hay clases sin diapositivas y presentaciones
    /// con vídeo dentro—, y esta opción no depende de ella.
    pub fn capturar_diapositiva(&self) -> Result<PathBuf> {
        let g = self.grabacion.lock().unwrap();
        let Some(grab) = g.as_ref() else {
            return Err(ApiError::NoGrabando);
        };

        let id = grab.session_id.clone();
        let ts = grab.duracion().as_millis() as i64;
        drop(g);

        let s = dictar_screen::capturar_ahora(self.dir_audio(&id).join("diapositivas"), ts, None)?;

        let ruta = s.ruta.to_string_lossy().into_owned();
        self.con_db(|db| db.insertar_diapositiva(&id, &ruta, s.ts_ms, &s.phash))?;

        Ok(s.ruta)
    }

    /// Carpeta de audio de una sesión.
    pub fn dir_audio(&self, id: &SessionId) -> PathBuf {
        self.dir_datos().join("audio").join(id.as_str())
    }
}

fn bucle_escritura(
    dir: PathBuf,
    rx: Receiver<dictar_audio::AudioFrame>,
    tx: Sender<Evento>,
    parar: Arc<AtomicBool>,
) -> Result<()> {
    let mut escritor = EscritorPistas::nuevo(&dir)?;

    let mut nivel_mic = 0.0f32;
    let mut nivel_sis = 0.0f32;
    let mut ultimo_evento = Instant::now();
    let mut ultimo_flush = Instant::now();

    loop {
        // Espera acotada: permite comprobar la señal de parada aunque una pista
        // deje de entregar bloques, en vez de quedarse colgado.
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(frame) => {
                // Lo primero, siempre: a disco.
                escritor.escribir(&frame)?;

                // Suavizado exponencial de los niveles, para que el indicador
                // no parpadee con cada bloque.
                let nivel = frame.nivel();
                match frame.track {
                    Track::Mic => nivel_mic = nivel_mic * 0.7 + nivel * 0.3,
                    Track::System => nivel_sis = nivel_sis * 0.7 + nivel * 0.3,
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
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
            ultimo_flush = Instant::now();
        }

        if parar.load(Ordering::SeqCst) {
            // Vaciar lo que quede en el canal antes de cerrar: son los últimos
            // segundos de la sesión, que suelen ser el resumen final.
            while let Ok(frame) = rx.try_recv() {
                escritor.escribir(&frame)?;
            }
            break;
        }
    }

    let rutas = escritor.cerrar()?;
    tracing::info!(archivos = rutas.len(), dir = %dir.display(), "audio cerrado");
    Ok(())
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
