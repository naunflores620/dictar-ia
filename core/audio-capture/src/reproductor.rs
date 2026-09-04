//! Reproducción de las sesiones grabadas.
//!
//! Sale por PipeWire, igual que el audio entró: añadir GStreamer o mpv al
//! paquete solo para oír un WAV sería pagar una dependencia pesada por algo
//! que el sistema ya sabe hacer. Las dos pistas se mezclan al vuelo, porque al
//! repasar una clase se quiere oír la conversación completa —profesor y
//! preguntas—, no una mitad.

use crate::{AudioError, Result, SAMPLE_RATE};
use libspa as spa;
use pipewire as pw;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;

// `mezclar` no depende de PipeWire —solo de `wav::leer_wav`—, así que vive en
// `lib.rs` como función independiente de plataforma, y este archivo la
// reexporta en vez de reimplementarla. `reproductor_stub.rs`, la versión de
// este módulo fuera de Linux, hace exactamente lo mismo: así no hay dos
// copias de la lógica de mezcla que puedan divergir con el tiempo.
pub use crate::mezclar;

struct EstadoSalida {
    pcm: Arc<Vec<f32>>,
    posicion: Arc<AtomicUsize>,
    pausado: Arc<AtomicBool>,
    terminado: Arc<AtomicBool>,
}

/// Una reproducción en curso. Al soltarla, para.
pub struct Reproductor {
    posicion: Arc<AtomicUsize>,
    pausado: Arc<AtomicBool>,
    terminado: Arc<AtomicBool>,
    total: usize,
    tx_parar: Option<pw::channel::Sender<()>>,
    hilo: Option<JoinHandle<()>>,
}

impl Reproductor {
    /// Empieza a reproducir la sesión guardada en `dir`, desde `desde_ms`.
    pub fn iniciar(dir: &Path, desde_ms: i64) -> Result<Self> {
        let pcm = Arc::new(mezclar(dir)?);
        if pcm.is_empty() {
            return Err(AudioError::Inicio(
                "esta sesión no tiene audio guardado".into(),
            ));
        }

        let total = pcm.len();
        let inicio = ms_a_muestras(desde_ms).min(total);

        let posicion = Arc::new(AtomicUsize::new(inicio));
        let pausado = Arc::new(AtomicBool::new(false));
        let terminado = Arc::new(AtomicBool::new(false));

        let (tx_parar, rx_parar) = pw::channel::channel::<()>();
        let (tx_listo, rx_listo) = mpsc::channel::<Result<()>>();

        let hilo = {
            let estado = EstadoSalida {
                pcm,
                posicion: posicion.clone(),
                pausado: pausado.clone(),
                terminado: terminado.clone(),
            };
            std::thread::Builder::new()
                .name("dictar-reproduccion".into())
                .spawn(move || {
                    if let Err(e) = bucle(estado, rx_parar, &tx_listo) {
                        tracing::error!(error = %e, "la reproducción falló");
                        let _ = tx_listo.send(Err(AudioError::Inicio(e.to_string())));
                    }
                })
                .map_err(|e| AudioError::Inicio(e.to_string()))?
        };

        // Igual que en la captura: esperar a saber si el flujo arrancó, para
        // que un fallo llegue como error y no como silencio inexplicable.
        match rx_listo.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                return Err(AudioError::Inicio(
                    "el hilo de reproducción murió al arrancar".into(),
                ))
            }
        }

        Ok(Self {
            posicion,
            pausado,
            terminado,
            total,
            tx_parar: Some(tx_parar),
            hilo: Some(hilo),
        })
    }

    pub fn posicion_ms(&self) -> i64 {
        muestras_a_ms(self.posicion.load(Ordering::SeqCst))
    }

    pub fn duracion_ms(&self) -> i64 {
        muestras_a_ms(self.total)
    }

    pub fn pausar(&self, pausar: bool) {
        self.pausado.store(pausar, Ordering::SeqCst);
    }

    pub fn pausado(&self) -> bool {
        self.pausado.load(Ordering::SeqCst)
    }

    pub fn terminado(&self) -> bool {
        self.terminado.load(Ordering::SeqCst)
    }

    /// Salta a un instante. Saltar hacia atrás desde el final reanuda.
    pub fn saltar(&self, ms: i64) {
        let m = ms_a_muestras(ms).min(self.total);
        self.posicion.store(m, Ordering::SeqCst);
        if m < self.total {
            self.terminado.store(false, Ordering::SeqCst);
        }
    }
}

impl Drop for Reproductor {
    fn drop(&mut self) {
        if let Some(tx) = self.tx_parar.take() {
            let _ = tx.send(());
        }
        if let Some(h) = self.hilo.take() {
            let _ = h.join();
        }
    }
}

fn ms_a_muestras(ms: i64) -> usize {
    ((ms.max(0) as u64 * SAMPLE_RATE as u64) / 1000) as usize
}

fn muestras_a_ms(m: usize) -> i64 {
    (m as i64 * 1000) / SAMPLE_RATE as i64
}

fn bucle(
    estado: EstadoSalida,
    rx_parar: pw::channel::Receiver<()>,
    tx_listo: &mpsc::Sender<Result<()>>,
) -> Result<()> {
    pw::init();

    let mainloop = pw::main_loop::MainLoop::new(None).map_err(err)?;
    let context = pw::context::Context::new(&mainloop).map_err(err)?;
    let core = context.connect(None).map_err(err)?;

    let _recv = rx_parar.attach(mainloop.loop_(), {
        let ml = mainloop.clone();
        move |_| ml.quit()
    });

    let props = pw::properties::properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Playback",
        *pw::keys::MEDIA_ROLE => "Music",
        *pw::keys::NODE_NAME => "dictar_ia_reproduccion",
    };

    let stream = pw::stream::Stream::new(&core, "dictar_ia", props).map_err(err)?;

    let _listener = stream
        .add_local_listener_with_user_data(estado)
        .process(|stream, estado| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();
            let Some(d) = datas.first_mut() else { return };

            let pausado = estado.pausado.load(Ordering::SeqCst);
            let pos = estado.posicion.load(Ordering::SeqCst);
            let total = estado.pcm.len();

            let frames;
            {
                let Some(destino) = d.data() else { return };
                // Bloques cortos: mantienen el salto y la pausa con respuesta
                // inmediata sin encarecer nada.
                frames = (destino.len() / 4).min(2048);

                for i in 0..frames {
                    // En pausa o al acabar se emite silencio en vez de cortar
                    // el flujo: así reanudar y saltar atrás son instantáneos.
                    let v = if pausado || pos + i >= total {
                        0.0
                    } else {
                        estado.pcm[pos + i]
                    };
                    destino[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
                }
            }

            if !pausado && pos < total {
                let nueva = (pos + frames).min(total);
                estado.posicion.store(nueva, Ordering::SeqCst);
                if nueva >= total {
                    estado.terminado.store(true, Ordering::SeqCst);
                }
            }

            let chunk = d.chunk_mut();
            *chunk.offset_mut() = 0;
            *chunk.stride_mut() = 4;
            *chunk.size_mut() = (frames * 4) as u32;
        })
        .register()
        .map_err(err)?;

    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    info.set_rate(SAMPLE_RATE);
    info.set_channels(1);

    let valores: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(spa::pod::Object {
            type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
            id: spa::param::ParamType::EnumFormat.as_raw(),
            properties: info.into(),
        }),
    )
    .map_err(|e| AudioError::Inicio(format!("no se pudo construir el formato: {e:?}")))?
    .0
    .into_inner();

    let mut params = [spa::pod::Pod::from_bytes(&valores)
        .ok_or_else(|| AudioError::Inicio("formato inválido".into()))?];

    stream
        .connect(
            spa::utils::Direction::Output,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(err)?;

    let _ = tx_listo.send(Ok(()));
    mainloop.run();
    Ok(())
}

fn err<E: std::fmt::Display>(e: E) -> AudioError {
    AudioError::Inicio(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Los tests de `mezclar` viven ahora junto a la función, en
    // `lib.rs::tests`: ese módulo se compila en todas las plataformas, y este
    // archivo (con PipeWire) solo en Linux.

    #[test]
    fn la_conversion_de_tiempo_va_y_vuelve() {
        assert_eq!(ms_a_muestras(1000), 16_000);
        assert_eq!(muestras_a_ms(16_000), 1000);
        assert_eq!(ms_a_muestras(-5), 0, "un ms negativo no debe reventar");
    }
}
