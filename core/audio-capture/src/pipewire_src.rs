//! Captura en Linux mediante PipeWire.
//!
//! Se abren dos flujos a la vez: el micrófono y el *monitor* de la salida del
//! sistema. El segundo es el que recoge al profesor o al cliente, salga el
//! audio de Meet, Zoom, Teams o del navegador.
//!
//! PipeWire tiene una propiedad pensada exactamente para esto,
//! `stream.capture.sink`, que conecta el flujo a la monitorización del destino
//! predeterminado. Evita tener que localizar el nodo `.monitor` a mano y, sobre
//! todo, sigue funcionando cuando el usuario cambia de altavoces a auriculares
//! a mitad de clase.

use crate::mezcla::{a_mono, Remuestreador};
use crate::{
    AudioError, AudioFrame, CaptureConfig, CaptureSession, DeviceInfo, Result, SAMPLE_RATE,
};
use dictar_domain::Track;
use libspa as spa;
use pipewire as pw;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Instant;

/// Estado de un flujo, vivo dentro del bucle de PipeWire.
struct EstadoFlujo {
    track: Track,
    tx: Sender<AudioFrame>,
    remuestreador: Option<Remuestreador>,
    canales: usize,
    frecuencia: u32,
    /// Muestras ya emitidas a 16 kHz. La marca de tiempo se deriva de aquí y no
    /// del reloj de pared: así las dos pistas no se desalinean en una sesión
    /// larga, que es lo que haría que las notas atribuyeran frases al
    /// interlocutor equivocado.
    muestras_emitidas: u64,
    /// Momento en que llegó el primer bloque de esta pista, por si un flujo
    /// arranca más tarde que el otro.
    desfase_ms: Option<i64>,
    inicio: Instant,
}

impl EstadoFlujo {
    fn nuevo(track: Track, tx: Sender<AudioFrame>, inicio: Instant) -> Self {
        Self {
            track,
            tx,
            remuestreador: None,
            canales: 2,
            frecuencia: 48_000,
            muestras_emitidas: 0,
            desfase_ms: None,
            inicio,
        }
    }

    fn timestamp_ms(&self) -> i64 {
        let desde_muestras = (self.muestras_emitidas as i64 * 1000) / SAMPLE_RATE as i64;
        self.desfase_ms.unwrap_or(0) + desde_muestras
    }
}

pub fn iniciar(cfg: CaptureConfig) -> Result<(Receiver<AudioFrame>, Box<dyn CaptureSession>)> {
    if !cfg.capturar_microfono && !cfg.capturar_sistema {
        return Err(AudioError::Inicio(
            "hay que capturar al menos una pista".into(),
        ));
    }

    let (tx_frames, rx_frames) = mpsc::channel::<AudioFrame>();
    let (tx_parar, rx_parar) = pw::channel::channel::<()>();
    let (tx_listo, rx_listo) = mpsc::channel::<Result<()>>();

    let inicio = Instant::now();

    let hilo = std::thread::Builder::new()
        .name("dictar-pipewire".into())
        .spawn(move || {
            let r = bucle(cfg, tx_frames, rx_parar, inicio, &tx_listo);
            if let Err(e) = &r {
                tracing::error!(error = %e, "el bucle de PipeWire terminó con error");
                // Si falla después de haber arrancado, el receptor ya no espera
                // confirmación; el envío puede fallar sin consecuencias.
                let _ = tx_listo.send(Err(AudioError::Inicio(e.to_string())));
            }
        })
        .map_err(|e| AudioError::Inicio(e.to_string()))?;

    // Esperar a saber si los flujos se han podido crear. Sin esto, un fallo de
    // conexión se manifestaría como «no llega audio» sin ningún error, que es
    // lo peor que puede pasar al empezar a grabar una clase.
    match rx_listo.recv() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            return Err(AudioError::Inicio(
                "el hilo de captura murió durante el arranque".into(),
            ))
        }
    }

    Ok((
        rx_frames,
        Box::new(SesionPipewire {
            tx_parar: Some(tx_parar),
            hilo: Some(hilo),
        }),
    ))
}

struct SesionPipewire {
    tx_parar: Option<pw::channel::Sender<()>>,
    hilo: Option<JoinHandle<()>>,
}

impl CaptureSession for SesionPipewire {
    fn detener(mut self: Box<Self>) -> Result<()> {
        if let Some(tx) = self.tx_parar.take() {
            let _ = tx.send(());
        }
        if let Some(h) = self.hilo.take() {
            let _ = h.join();
        }
        Ok(())
    }
}

impl Drop for SesionPipewire {
    fn drop(&mut self) {
        // Soltar la sesión sin llamar a `detener` no debe dejar el hilo de
        // PipeWire girando en segundo plano con el micrófono abierto.
        if let Some(tx) = self.tx_parar.take() {
            let _ = tx.send(());
        }
        if let Some(h) = self.hilo.take() {
            let _ = h.join();
        }
    }
}

fn bucle(
    cfg: CaptureConfig,
    tx: Sender<AudioFrame>,
    rx_parar: pw::channel::Receiver<()>,
    inicio: Instant,
    tx_listo: &Sender<Result<()>>,
) -> Result<()> {
    pw::init();

    let mainloop = pw::main_loop::MainLoop::new(None).map_err(err)?;
    let context = pw::context::Context::new(&mainloop).map_err(err)?;
    let core = context.connect(None).map_err(err)?;

    let _recv = rx_parar.attach(mainloop.loop_(), {
        let ml = mainloop.clone();
        move |_| ml.quit()
    });

    // Los flujos y sus escuchas tienen que seguir vivos mientras corre el
    // bucle: si se sueltan aquí, la captura se cierra en silencio.
    let mut vivos = Vec::new();

    if cfg.capturar_microfono {
        vivos.push(crear_flujo(
            &core,
            Track::Mic,
            tx.clone(),
            inicio,
            false,
            cfg.nodo_microfono.as_deref(),
        )?);
    }

    if cfg.capturar_sistema {
        vivos.push(crear_flujo(
            &core,
            Track::System,
            tx.clone(),
            inicio,
            true,
            cfg.nodo_sistema.as_deref(),
        )?);
    }

    tracing::info!(flujos = vivos.len(), "captura de PipeWire iniciada");
    let _ = tx_listo.send(Ok(()));

    mainloop.run();

    tracing::info!("captura de PipeWire detenida");
    Ok(())
}

type Vivo = (pw::stream::Stream, pw::stream::StreamListener<EstadoFlujo>);

fn crear_flujo(
    core: &pw::core::Core,
    track: Track,
    tx: Sender<AudioFrame>,
    inicio: Instant,
    monitor_de_salida: bool,
    nodo: Option<&str>,
) -> Result<Vivo> {
    let mut props = pw::properties::properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        // "Communication" le dice a PipeWire que esto es voz: evita que el
        // gestor de sesión aplique procesado pensado para música.
        *pw::keys::MEDIA_ROLE => "Communication",
        *pw::keys::NODE_NAME => if track == Track::Mic { "dictar_ia_mic" } else { "dictar_ia_sistema" },
    };

    if monitor_de_salida {
        // La propiedad que hace todo el trabajo: conecta a la monitorización
        // del destino predeterminado, y sigue valiendo si el usuario cambia de
        // altavoces a auriculares a mitad de clase.
        props.insert(*pw::keys::STREAM_CAPTURE_SINK, "true");
    }

    if let Some(n) = nodo {
        // El nombre de la propiedad se escribe literal: la constante
        // `TARGET_OBJECT` no existe en todas las versiones del binding, y el
        // valor que espera PipeWire es este.
        props.insert("target.object", n);
    }

    let estado = EstadoFlujo::nuevo(track, tx, inicio);
    let stream = pw::stream::Stream::new(core, "dictar_ia", props).map_err(err)?;

    let listener = stream
        .add_local_listener_with_user_data(estado)
        .param_changed(|_, estado, id, pod| {
            if id != pw::spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Some(pod) = pod else { return };

            let Ok((tipo, subtipo)) = spa::param::format_utils::parse_format(pod) else {
                return;
            };
            if tipo != spa::param::format::MediaType::Audio
                || subtipo != spa::param::format::MediaSubtype::Raw
            {
                return;
            }

            let mut info = spa::param::audio::AudioInfoRaw::default();
            if info.parse(pod).is_err() {
                return;
            }

            estado.canales = info.channels().max(1) as usize;
            estado.frecuencia = info.rate().max(1);

            match Remuestreador::nuevo(estado.frecuencia) {
                Ok(r) => {
                    tracing::info!(
                        pista = estado.track.as_str(),
                        canales = estado.canales,
                        frecuencia = estado.frecuencia,
                        "formato negociado"
                    );
                    estado.remuestreador = Some(r);
                }
                Err(e) => tracing::error!(error = %e, "no se pudo crear el remuestreador"),
            }
        })
        .process(|stream, estado| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datos = buffer.datas_mut();
            let Some(d) = datos.first_mut() else { return };

            let tam = d.chunk().size() as usize;
            let Some(bytes) = d.data() else { return };
            if tam == 0 || bytes.len() < tam {
                return;
            }

            // El formato negociado es F32LE, que es lo que pedimos.
            let muestras: Vec<f32> = bytes[..tam]
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect();

            let mono = a_mono(&muestras, estado.canales);

            let pcm = match estado.remuestreador.as_mut() {
                Some(r) => match r.procesar(&mono) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!(error = %e, "fallo de remuestreo, bloque descartado");
                        return;
                    }
                },
                // Todavía no se ha negociado el formato: descartar es correcto,
                // llegan uno o dos bloques como mucho.
                None => return,
            };

            if pcm.is_empty() {
                return;
            }

            if estado.desfase_ms.is_none() {
                estado.desfase_ms = Some(estado.inicio.elapsed().as_millis() as i64);
            }

            let frame = AudioFrame {
                track: estado.track,
                timestamp_ms: estado.timestamp_ms(),
                pcm,
            };
            estado.muestras_emitidas += frame.pcm.len() as u64;

            // Si el consumidor ha desaparecido, no hay nada que hacer: el bucle
            // se detendrá al recibir la señal de parada.
            let _ = estado.tx.send(frame);
        })
        .register()
        .map_err(err)?;

    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);

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
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(err)?;

    Ok((stream, listener))
}

/// Enumera micrófonos y monitores de salida.
pub fn dispositivos() -> Result<Vec<DeviceInfo>> {
    // El registro de PipeWire se consulta de forma asíncrona; para el listado
    // basta con lo que ya sabemos: el predeterminado del sistema, que es lo que
    // usa el 95 % de las veces. La enumeración completa llegará con el selector
    // de dispositivo de la interfaz.
    Ok(vec![
        DeviceInfo {
            id: "default_source".into(),
            nombre: "Micrófono predeterminado".into(),
            descripcion: "La entrada configurada en el sistema".into(),
            es_monitor: false,
            por_defecto: true,
        },
        DeviceInfo {
            id: "default_sink_monitor".into(),
            nombre: "Salida del sistema".into(),
            descripcion: "Lo que oyes: Meet, Zoom, Teams o el navegador".into(),
            es_monitor: true,
            por_defecto: true,
        },
    ])
}

fn err<E: std::fmt::Display>(e: E) -> AudioError {
    AudioError::Inicio(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_marca_de_tiempo_avanza_con_las_muestras_no_con_el_reloj() {
        // Derivarla del reloj de pared haría que las dos pistas se separaran
        // en una sesión de dos horas.
        let (tx, _rx) = mpsc::channel();
        let mut e = EstadoFlujo::nuevo(Track::Mic, tx, Instant::now());
        e.desfase_ms = Some(0);

        assert_eq!(e.timestamp_ms(), 0);

        e.muestras_emitidas = 16_000;
        assert_eq!(e.timestamp_ms(), 1000);

        e.muestras_emitidas = 16_000 * 3600;
        assert_eq!(e.timestamp_ms(), 3_600_000);
    }

    #[test]
    fn un_flujo_que_arranca_tarde_conserva_su_desfase() {
        let (tx, _rx) = mpsc::channel();
        let mut e = EstadoFlujo::nuevo(Track::System, tx, Instant::now());

        // El monitor de salida tardó 250 ms en dar su primer bloque.
        e.desfase_ms = Some(250);
        e.muestras_emitidas = 16_000;

        assert_eq!(e.timestamp_ms(), 1250);
    }

    #[test]
    fn capturar_ninguna_pista_es_un_error_explicito() {
        let cfg = CaptureConfig {
            capturar_sistema: false,
            capturar_microfono: false,
            ..Default::default()
        };
        assert!(iniciar(cfg).is_err());
    }

    #[test]
    fn se_ofrecen_las_dos_fuentes_habituales() {
        let d = dispositivos().unwrap();
        assert!(d.iter().any(|x| x.es_monitor));
        assert!(d.iter().any(|x| !x.es_monitor));
    }
}
