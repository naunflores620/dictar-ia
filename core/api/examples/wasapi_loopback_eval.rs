//! Prueba corta de captura WASAPI y transcripción local con una pista de clase.
//!
//! Reproduce `system.wav` por la salida predeterminada y captura solo el
//! loopback del sistema. No guarda el audio capturado ni el texto reconocido:
//! conserva únicamente `capture_summary.json` y `stt_metrics.jsonl`.
//!
//! Uso (Windows):
//! cargo run --release -p dictar-api --example wasapi_loopback_eval -- \
//!   <system.wav> <directorio-salida> [segundos]
//!   wasapi_loopback_eval --devices
//!
//! Modo AV-1, sin modelo ni transcripción: graba las dos pistas durante N
//! segundos, deja `system.wav` y `mic.wav` en el directorio y cuenta, segundo
//! a segundo, cuánta señal llegó. Sirve para comprobar que la grabación sigue
//! al dispositivo por defecto cuando cambia o desaparece.
//! cargo run --release -p dictar-api --example wasapi_loopback_eval -- \
//!   --seguir-dispositivo <directorio-salida> [segundos] [--tono]
//! Con `--tono` el propio ejemplo reproduce un tono por la salida por defecto,
//! para que la prueba no dependa de abrir un navegador.

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("Este harness requiere Windows y captura WASAPI.");
    std::process::exit(2);
}

#[cfg(target_os = "windows")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use dictar_api::transcriptor::TranscriptorVivo;
    use dictar_audio::CaptureConfig;
    use dictar_domain::{Session, SessionKind, SessionSource};
    use dictar_storage::Db;
    use dictar_stt::modelos::Modelo;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let mut args = std::env::args_os().skip(1);
    let primer_arg = args.next().ok_or("falta ruta a system.wav")?;
    if primer_arg == "--seguir-dispositivo" {
        let resto: Vec<String> = args.map(|a| a.to_string_lossy().into_owned()).collect();
        return seguir_dispositivo(&resto);
    }
    if primer_arg == "--devices" {
        let dispositivos = dictar_audio::dispositivos()?;
        println!(
            "{}",
            serde_json::to_string_pretty(&dispositivos_json(&dispositivos))?
        );
        return Ok(());
    }

    let wav = PathBuf::from(primer_arg);
    let salida = PathBuf::from(args.next().ok_or("falta directorio de salida")?);
    let segundos = args
        .next()
        .map(|s| s.to_string_lossy().parse::<u64>())
        .transpose()?
        .unwrap_or(30)
        .clamp(5, 120);

    if !wav.is_file() {
        return Err(format!("no existe el WAV: {}", wav.display()).into());
    }
    let dispositivos = dictar_audio::dispositivos()?;
    let salidas: Vec<_> = dispositivos
        .iter()
        .filter(|d| d.es_monitor)
        .map(|d| {
            serde_json::json!({
                "nombre": d.nombre,
                "predeterminado": d.por_defecto,
            })
        })
        .collect();
    if !dispositivos.iter().any(|d| d.es_monitor && d.por_defecto) {
        return Err("WASAPI no encontró una salida de audio predeterminada".into());
    }
    std::fs::create_dir_all(&salida)?;
    let metricas = salida.join("stt_metrics.jsonl");
    std::fs::write(&metricas, b"")?;

    // El reproductor espera una carpeta de sesión. Solo se copia system.wav;
    // así no se suma ni reproduce la pista del micrófono de la sesión fuente.
    let reproduccion = tempfile::tempdir()?;
    std::fs::copy(&wav, reproduccion.path().join("system.wav"))?;

    let datos = tempfile::tempdir()?;
    let db_path = datos.path().join("bench.db");
    let db = Db::abrir(&db_path)?;
    let sesion = Session::nueva(SessionKind::Class, SessionSource::DesktopLoopback, 0);
    let session_id = sesion.id.clone();
    db.crear_sesion(&sesion)?;
    drop(db);

    // Simula el tiempo entre seleccionar Español y pulsar Grabar: el usuario
    // puede preparar la sesión mientras el modelo se carga en segundo plano.
    let inicio_preparacion_modelo = Instant::now();
    dictar_stt::sherpa::precalentar_modelo_es()?;
    let preparacion_modelo_ms = inicio_preparacion_modelo.elapsed().as_millis();

    let (tx_eventos, rx_eventos) = mpsc::channel();
    // Carga el reconocedor antes de abrir WASAPI, igual que producción. Así,
    // el tiempo de inicialización del modelo no se interpreta como silencio
    // capturado ni deja el comienzo de la reproducción esperando en la cola.
    let inicio_captura = Instant::now();
    let transcriptor = TranscriptorVivo::iniciar(
        session_id.clone(),
        db_path.clone(),
        Modelo::Small,
        String::new(),
        Some("es".into()),
        "Profesor".into(),
        tx_eventos,
        inicio_captura,
        metricas.clone(),
    )?;

    // Preabre la salida en pausa para que su inicialización no aparezca como
    // silencio dentro del intervalo de captura.
    let reproductor =
        dictar_audio::reproductor::Reproductor::iniciar_pausado(reproduccion.path(), 0)?;
    let (rx_audio, captura) = dictar_audio::iniciar(CaptureConfig {
        capturar_sistema: true,
        capturar_microfono: false,
        ..CaptureConfig::default()
    })?;
    let inicio_captura_real = Instant::now();
    transcriptor.marcar_inicio_captura(inicio_captura_real);
    reproductor.pausar(false);

    let inicio_prueba = Instant::now();
    let limite = Duration::from_secs(segundos);
    let mut frames = 0u64;
    let mut muestras = 0u64;
    let mut silenciosos = 0u64;
    let mut recortados = 0u64;
    let mut suma_cuadrados = 0.0f64;
    let mut pico = 0.0f32;
    let mut ultimo_ts_ms = 0i64;
    let mut parciales = 0u64;
    let mut cierres = 0u64;
    let mut errores = Vec::new();
    let mut primera_parcial_ms = None;
    let mut primera_parcial_audio_ms = None;

    while inicio_prueba.elapsed() < limite && !reproductor.terminado() {
        if let Ok(frame) = rx_audio.recv_timeout(Duration::from_millis(100)) {
            acumular(
                &frame.pcm,
                &mut frames,
                &mut muestras,
                &mut silenciosos,
                &mut recortados,
                &mut suma_cuadrados,
                &mut pico,
            );
            ultimo_ts_ms = ultimo_ts_ms.max(frame.timestamp_ms + frame.duracion_ms());
            transcriptor.encolar_bloque(frame);
        }
        consumir_eventos(
            &rx_eventos,
            &inicio_captura_real,
            &mut parciales,
            &mut cierres,
            &mut errores,
            &mut primera_parcial_ms,
            &mut primera_parcial_audio_ms,
        );
    }

    let posicion_reproducida_ms = reproductor.posicion_ms();
    drop(reproductor);

    // Recoger la cola de salida del dispositivo antes de cerrar WASAPI.
    let fin_cola = Instant::now() + Duration::from_millis(350);
    while Instant::now() < fin_cola {
        if let Ok(frame) = rx_audio.recv_timeout(Duration::from_millis(50)) {
            acumular(
                &frame.pcm,
                &mut frames,
                &mut muestras,
                &mut silenciosos,
                &mut recortados,
                &mut suma_cuadrados,
                &mut pico,
            );
            ultimo_ts_ms = ultimo_ts_ms.max(frame.timestamp_ms + frame.duracion_ms());
            transcriptor.encolar_bloque(frame);
        }
        consumir_eventos(
            &rx_eventos,
            &inicio_captura_real,
            &mut parciales,
            &mut cierres,
            &mut errores,
            &mut primera_parcial_ms,
            &mut primera_parcial_audio_ms,
        );
    }

    captura.detener()?;
    while let Ok(frame) = rx_audio.try_recv() {
        acumular(
            &frame.pcm,
            &mut frames,
            &mut muestras,
            &mut silenciosos,
            &mut recortados,
            &mut suma_cuadrados,
            &mut pico,
        );
        ultimo_ts_ms = ultimo_ts_ms.max(frame.timestamp_ms + frame.duracion_ms());
        transcriptor.encolar_bloque(frame);
    }

    transcriptor.pedir_fin();
    transcriptor.esperar();

    consumir_eventos(
        &rx_eventos,
        &inicio_captura_real,
        &mut parciales,
        &mut cierres,
        &mut errores,
        &mut primera_parcial_ms,
        &mut primera_parcial_audio_ms,
    );

    let db = Db::abrir(&db_path)?;
    let frases = db.transcripcion(&session_id)?;
    let palabras: usize = frases
        .iter()
        .map(|frase| frase.text.split_whitespace().count())
        .sum();
    let frases_con_bucle = frases
        .iter()
        .filter(|frase| {
            let tokens: Vec<_> = frase.text.split_whitespace().collect();
            dictar_stt::limpieza::tiene_bucle_repeticion(&tokens)
        })
        .count();
    drop(db);

    let descartes = std::fs::read_to_string(&metricas)?
        .lines()
        .filter_map(|linea| serde_json::from_str::<serde_json::Value>(linea).ok())
        .filter_map(|metrica| metrica["trabajos_descartados_total"].as_u64())
        .max()
        .unwrap_or(0);

    let rms = if muestras == 0 {
        0.0
    } else {
        (suma_cuadrados / muestras as f64).sqrt()
    };
    let modelo_stt = std::fs::read_to_string(&metricas)?
        .lines()
        .filter_map(|linea| serde_json::from_str::<serde_json::Value>(linea).ok())
        .find_map(|metrica| metrica["modelo"].as_str().map(str::to_owned));
    let resumen = serde_json::json!({
        "wav": wav.file_name().and_then(|n| n.to_str()).unwrap_or("system.wav"),
        "salidas_wasapi": salidas,
        "modelo_stt": modelo_stt,
        "idioma_stt": "es",
        "preparacion_modelo_ms": preparacion_modelo_ms,
        "formato_captura": "mono, 16000 Hz, f32",
        "segundos_solicitados": segundos,
        "posicion_reproducida_ms": posicion_reproducida_ms,
        "captura_sistema_sin_microfono": true,
        "frames": frames,
        "audio_capturado_ms": ultimo_ts_ms.max(0),
        "muestras": muestras,
        "rms": rms,
        "pico": pico,
        "frames_silenciosos": silenciosos,
        "muestras_recortadas": recortados,
        "parciales": parciales,
        "primera_parcial_desde_captura_ms": primera_parcial_ms,
        "audio_hasta_primera_parcial_ms": primera_parcial_audio_ms,
        "cierres": cierres,
        "frases_guardadas": frases.len(),
        "palabras": palabras,
        "frases_con_bucle": frases_con_bucle,
        "trabajos_stt_descartados": descartes,
        "errores": errores,
    });
    let resumen_path = salida.join("capture_summary.json");
    std::fs::write(&resumen_path, serde_json::to_vec_pretty(&resumen)?)?;

    // La base de transcripción está dentro de `tempfile::TempDir`; se destruye
    // al salir. No se conserva contenido de la clase ni audio capturado.
    println!("{}", serde_json::to_string_pretty(&resumen)?);
    println!("metricas_stt={}", metricas.display());
    println!("resumen_captura={}", resumen_path.display());
    if frames == 0 {
        return Err("WASAPI no entregó ningún bloque de audio durante la prueba".into());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn consumir_eventos(
    rx: &std::sync::mpsc::Receiver<dictar_api::Evento>,
    inicio_captura: &std::time::Instant,
    parciales: &mut u64,
    cierres: &mut u64,
    errores: &mut Vec<String>,
    primera_parcial_ms: &mut Option<u128>,
    primera_parcial_audio_ms: &mut Option<i64>,
) {
    for evento in rx.try_iter() {
        match evento {
            dictar_api::Evento::Frase {
                definitiva, fin_ms, ..
            } => {
                if definitiva {
                    *cierres += 1;
                } else {
                    *parciales += 1;
                    if primera_parcial_ms.is_none() {
                        *primera_parcial_ms = Some(inicio_captura.elapsed().as_millis());
                        *primera_parcial_audio_ms = Some(fin_ms);
                    }
                }
            }
            dictar_api::Evento::Error { mensaje, .. } => errores.push(mensaje),
            _ => {}
        }
    }
}

#[cfg(target_os = "windows")]
fn dispositivos_json(dispositivos: &[dictar_audio::DeviceInfo]) -> Vec<serde_json::Value> {
    dispositivos
        .iter()
        .map(|d| {
            serde_json::json!({
                "nombre": d.nombre,
                "descripcion": d.descripcion,
                "salida_loopback": d.es_monitor,
                "predeterminado": d.por_defecto,
            })
        })
        .collect()
}

#[cfg(target_os = "windows")]
fn acumular(
    pcm: &[f32],
    frames: &mut u64,
    muestras: &mut u64,
    silenciosos: &mut u64,
    recortados: &mut u64,
    suma_cuadrados: &mut f64,
    pico: &mut f32,
) {
    *frames += 1;
    let mut suma_frame = 0.0f64;
    for &muestra in pcm {
        let limpia = if muestra.is_finite() { muestra } else { 0.0 };
        let abs = limpia.abs();
        *pico = (*pico).max(abs);
        if abs >= 0.999 {
            *recortados += 1;
        }
        suma_frame += (limpia as f64).powi(2);
    }
    *muestras += pcm.len() as u64;
    *suma_cuadrados += suma_frame;
    if !pcm.is_empty() && (suma_frame / pcm.len() as f64).sqrt() < 0.001 {
        *silenciosos += 1;
    }
}

/// Subscriber mínimo escrito a mano: `dictar-api` no depende de
/// `tracing-subscriber` y este ejemplo no puede tocar el `Cargo.toml`. Sin
/// él, las transiciones de AV-1 (que solo existen como registros) no se verían.
#[cfg(target_os = "windows")]
struct RegistroEnConsola {
    inicio: std::time::Instant,
    nivel_audio: tracing::Level,
}

#[cfg(target_os = "windows")]
impl tracing::Subscriber for RegistroEnConsola {
    fn enabled(&self, m: &tracing::Metadata<'_>) -> bool {
        // En `tracing` «más detallado» es «mayor»: TRACE > DEBUG > INFO.
        let tope = if m.target().starts_with("dictar_audio") {
            self.nivel_audio
        } else {
            tracing::Level::INFO
        };
        *m.level() <= tope
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, e: &tracing::Event<'_>) {
        struct Texto(String);
        impl tracing::field::Visit for Texto {
            fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                if f.name() == "message" {
                    self.0.insert_str(0, &format!("{v:?} "));
                } else {
                    self.0.push_str(&format!("{}={v:?} ", f.name()));
                }
            }
        }
        let mut t = Texto(String::new());
        e.record(&mut t);
        println!(
            "[{:>7.2}s] {:<5} {}: {}",
            self.inicio.elapsed().as_secs_f32(),
            e.metadata().level(),
            e.metadata().target(),
            t.0.trim_end()
        );
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// Acumula el RMS por segundo de **posición en el WAV** (no de llegada): es lo
/// que el criterio de AV-1 mide, porque el WAV es lo que se queda en disco.
#[cfg(target_os = "windows")]
#[derive(Default)]
struct PistaMedida {
    muestras: u64,
    por_segundo: Vec<(f64, u64)>,
    tramas_en_la_ventana: u64,
    muestras_en_la_ventana: u64,
    suma_en_la_ventana: f64,
    ultima_llegada_ms: Option<u128>,
    mayor_intervalo_entre_tramas_ms: u128,
}

#[cfg(target_os = "windows")]
impl PistaMedida {
    fn anotar(&mut self, pcm: &[f32], ahora_ms: u128) {
        for &m in pcm {
            let seg = (self.muestras / dictar_audio::SAMPLE_RATE as u64) as usize;
            if self.por_segundo.len() <= seg {
                self.por_segundo.resize(seg + 1, (0.0, 0));
            }
            let cuadrado = if m.is_finite() {
                (m as f64).powi(2)
            } else {
                0.0
            };
            self.por_segundo[seg].0 += cuadrado;
            self.por_segundo[seg].1 += 1;
            self.suma_en_la_ventana += cuadrado;
            self.muestras += 1;
        }
        self.tramas_en_la_ventana += 1;
        self.muestras_en_la_ventana += pcm.len() as u64;
        if let Some(antes) = self.ultima_llegada_ms {
            self.mayor_intervalo_entre_tramas_ms =
                self.mayor_intervalo_entre_tramas_ms.max(ahora_ms - antes);
        }
        self.ultima_llegada_ms = Some(ahora_ms);
    }

    fn rms_de_ventana(&self) -> f64 {
        if self.muestras_en_la_ventana == 0 {
            0.0
        } else {
            (self.suma_en_la_ventana / self.muestras_en_la_ventana as f64).sqrt()
        }
    }

    fn cerrar_ventana(&mut self) {
        self.tramas_en_la_ventana = 0;
        self.muestras_en_la_ventana = 0;
        self.suma_en_la_ventana = 0.0;
    }
}

#[cfg(target_os = "windows")]
fn seguir_dispositivo(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use dictar_audio::{wav::EscritorPistas, AudioFrame, CaptureConfig, SAMPLE_RATE};
    use dictar_domain::Track;
    use std::time::{Duration, Instant};

    let dir = std::path::PathBuf::from(args.first().ok_or("falta directorio de salida")?);
    let segundos: u64 = args
        .get(1)
        .filter(|a| !a.starts_with("--"))
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(60);
    let con_tono = args.iter().any(|a| a == "--tono");

    let nivel_audio = match std::env::var("RUST_LOG") {
        Ok(v) if v.contains("dictar_audio=trace") => tracing::Level::TRACE,
        Ok(v) if v.contains("dictar_audio=debug") => tracing::Level::DEBUG,
        _ => tracing::Level::INFO,
    };
    tracing::subscriber::set_global_default(RegistroEnConsola {
        inicio: Instant::now(),
        nivel_audio,
    })?;

    std::fs::create_dir_all(&dir)?;

    // El tono se escribe como un `system.wav` más y se reproduce con el mismo
    // reproductor que usa la aplicación, por la salida por defecto.
    let reproductor = if con_tono {
        let dir_tono = dir.join("tono");
        let mut w = EscritorPistas::nuevo(&dir_tono)?;
        let total = (segundos + 5) * SAMPLE_RATE as u64;
        let pcm: Vec<f32> = (0..total)
            .map(|i| {
                let fase = 2.0 * std::f32::consts::PI * 440.0 * i as f32 / SAMPLE_RATE as f32;
                0.3 * fase.sin()
            })
            .collect();
        w.escribir(&AudioFrame {
            track: Track::System,
            pcm,
            timestamp_ms: 0,
        })?;
        w.cerrar()?;
        Some(dictar_audio::reproductor::Reproductor::iniciar_pausado(
            &dir_tono, 0,
        )?)
    } else {
        None
    };

    let mut escritor = EscritorPistas::nuevo(&dir)?;
    let (rx, captura) = dictar_audio::iniciar(CaptureConfig::default())?;
    if let Some(r) = &reproductor {
        r.pausar(false);
    }
    println!("grabando {segundos} s en {}", dir.display());

    let inicio = Instant::now();
    let limite = Duration::from_secs(segundos);
    // [sistema, micrófono]
    let mut pistas = [PistaMedida::default(), PistaMedida::default()];
    let mut ventana = 1u64;
    let recibir = |frame: AudioFrame,
                   escritor: &mut EscritorPistas,
                   pistas: &mut [PistaMedida; 2]|
     -> Result<(), Box<dyn std::error::Error>> {
        escritor.escribir(&frame)?;
        let i = if frame.track == Track::System { 0 } else { 1 };
        pistas[i].anotar(&frame.pcm, inicio.elapsed().as_millis());
        Ok(())
    };

    while inicio.elapsed() < limite {
        if let Ok(frame) = rx.recv_timeout(Duration::from_millis(50)) {
            recibir(frame, &mut escritor, &mut pistas)?;
        }
        if inicio.elapsed() >= Duration::from_secs(ventana) {
            println!(
                "t={ventana:>3}s  sistema: {:>3} tramas rms={:.4}   mic: {:>3} tramas rms={:.4}",
                pistas[0].tramas_en_la_ventana,
                pistas[0].rms_de_ventana(),
                pistas[1].tramas_en_la_ventana,
                pistas[1].rms_de_ventana(),
            );
            pistas[0].cerrar_ventana();
            pistas[1].cerrar_ventana();
            ventana += 1;
        }
    }

    drop(reproductor);
    captura.detener()?;
    // `detener` vuelca la cola del remuestreador y el relleno final: hay que
    // recogerlos todos antes de cerrar los WAV, o la duración sale corta.
    while let Ok(frame) = rx.try_recv() {
        recibir(frame, &mut escritor, &mut pistas)?;
    }
    let sesion_ms = inicio.elapsed().as_millis();
    let dur_sistema = escritor.duracion_ms(Track::System);
    let dur_mic = escritor.duracion_ms(Track::Mic);
    escritor.cerrar()?;

    println!("--- resumen ---");
    println!("sesion_ms={sesion_ms}");
    println!(
        "system.wav duracion_ms={dur_sistema} muestras={}",
        pistas[0].muestras
    );
    println!(
        "mic.wav    duracion_ms={dur_mic} muestras={}",
        pistas[1].muestras
    );
    println!(
        "mayor_intervalo_entre_tramas_ms sistema={} mic={}",
        pistas[0].mayor_intervalo_entre_tramas_ms, pistas[1].mayor_intervalo_entre_tramas_ms
    );
    println!("--- RMS por segundo de posicion en el WAV (segundo: sistema / mic) ---");
    let filas = pistas[0].por_segundo.len().max(pistas[1].por_segundo.len());
    let rms = |p: &PistaMedida, s: usize| -> String {
        match p.por_segundo.get(s) {
            Some((suma, n)) if *n > 0 => format!("{:.4}", (suma / *n as f64).sqrt()),
            _ => "  -   ".into(),
        }
    };
    for s in 0..filas {
        println!("{s:>3}: {} / {}", rms(&pistas[0], s), rms(&pistas[1], s));
    }
    Ok(())
}
