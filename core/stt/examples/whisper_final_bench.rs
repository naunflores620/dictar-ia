//! Mide la pasada definitiva de Whisper sobre un WAV local.
//!
//! Uso: cargo run --release -p dictar-stt --example whisper_final_bench --
//!      <modelo.bin> <audio.wav> [idioma|auto] [hilos] [salida.txt]

use dictar_audio::wav::leer_wav;
use dictar_stt::SttOpciones;
use dictar_stt::whisper::Transcriptor;
use std::path::PathBuf;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let modelo = PathBuf::from(args.next().ok_or("falta ruta del modelo")?);
    let audio = PathBuf::from(args.next().ok_or("falta ruta WAV")?);
    let idioma = args
        .next()
        .and_then(|s| s.into_string().ok())
        .unwrap_or_else(|| "es".into());
    let hilos = args.next().and_then(|s| s.into_string().ok());
    let salida = args.next().map(PathBuf::from);

    let (pcm, sample_rate) = leer_wav(&audio)?;
    let duracion_s = pcm.len() as f64 / sample_rate as f64;

    let inicio_carga = Instant::now();
    let transcriptor = Transcriptor::cargar(&modelo)?;
    let carga_s = inicio_carga.elapsed().as_secs_f64();

    let mut opciones = SttOpciones::definitiva();
    opciones.idioma = (idioma != "auto").then_some(idioma);
    if let Some(hilos) = hilos {
        opciones.hilos = hilos.parse()?;
    }

    let inicio = Instant::now();
    let fragmentos = transcriptor.transcribir(&pcm, &opciones)?;
    let inferencia_s = inicio.elapsed().as_secs_f64();
    let texto = fragmentos
        .iter()
        .map(|f| f.texto.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let palabras = texto.split_whitespace().count();

    if let Some(ruta) = salida {
        std::fs::write(&ruta, &texto)?;
        println!("transcripcion={}", ruta.display());
    }
    println!(
        "audio_s={duracion_s:.3} carga_s={carga_s:.2} inferencia_s={inferencia_s:.2} rtf={:.3} hilos={} marcas_tiempo={} segmentos={} palabras={palabras}",
        inferencia_s / duracion_s,
        opciones.hilos,
        opciones.tiempos_por_palabra,
        fragmentos.len(),
    );
    println!("{texto}");
    Ok(())
}
