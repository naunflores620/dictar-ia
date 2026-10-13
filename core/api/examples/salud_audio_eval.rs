//! Prueba sin conexión de los avisos de salud con audio real.
//!
//! Alimenta `SaludAudio` con los pares `mic.wav` / `system.wav` de sesiones ya
//! grabadas, en bloques de 20 ms con `timestamp_ms` sintético y un reloj
//! sintético, y cuenta cuándo se activa y se desactiva cada aviso. Es la única
//! comprobación de falsos positivos y negativos con datos que no inventamos.
//!
//! Uso: `salud_audio_eval [carpeta_de_sesion ...]`. Sin argumentos recorre
//! `%APPDATA%\dictar_ia\audio\ses_*` (o `$HOME/.local/share/dictar_ia/audio`).
//!
//! Hay que ejecutarlo con `--release`: en Debug las duraciones que imprime no
//! dicen nada del coste real.

use dictar_api::salud_audio::{CambioAviso, SaludAudio};
use dictar_audio::wav::leer_wav;
use dictar_audio::{AudioFrame, SAMPLE_RATE};
use dictar_domain::Track;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const BLOQUE: usize = SAMPLE_RATE as usize / 50;

fn carpeta_audio() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("dictar_ia").join("audio");
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".local")
        .join("share")
        .join("dictar_ia")
        .join("audio")
}

fn rms(pcm: &[f32]) -> f64 {
    let suma: f64 = pcm.iter().map(|m| f64::from(*m).powi(2)).sum();
    (suma / pcm.len().max(1) as f64).sqrt()
}

fn evaluar(carpeta: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let (mic, fs_mic) = leer_wav(carpeta.join("mic.wav"))?;
    let (sistema, fs_sis) = leer_wav(carpeta.join("system.wav"))?;
    if fs_mic != SAMPLE_RATE || fs_sis != SAMPLE_RATE {
        return Err(format!("frecuencias {fs_mic}/{fs_sis}: se esperaba 16 kHz").into());
    }
    let total = mic.len().max(sistema.len());
    println!(
        "== {} ==\n  mic {:.1} s (RMS {:.4}), sistema {:.1} s (RMS {:.4})",
        carpeta.file_name().unwrap_or_default().to_string_lossy(),
        mic.len() as f64 / f64::from(SAMPLE_RATE),
        rms(&mic),
        sistema.len() as f64 / f64::from(SAMPLE_RATE),
        rms(&sistema),
    );

    let inicio = Instant::now();
    let mut salud = SaludAudio::nueva(inicio, true);
    let mut cambios: Vec<(f64, CambioAviso)> = Vec::new();
    // Ticks que costaron más de 100 µs: son las evaluaciones periódicas, la
    // del eco la más cara.
    let mut caros: Vec<Duration> = Vec::new();
    let mut pos = 0usize;
    while pos < total {
        let ts_ms = (pos * 1000 / SAMPLE_RATE as usize) as i64;
        let ahora = inicio + Duration::from_millis(ts_ms as u64 + 20);
        for (track, pcm) in [(Track::Mic, &mic), (Track::System, &sistema)] {
            if pos >= pcm.len() {
                continue;
            }
            let fin = (pos + BLOQUE).min(pcm.len());
            let frame = AudioFrame {
                track,
                pcm: pcm[pos..fin].to_vec(),
                timestamp_ms: ts_ms,
            };
            for c in salud.trama(&frame, ahora) {
                cambios.push((ts_ms as f64 / 1000.0, c));
            }
        }
        let t0 = Instant::now();
        for c in salud.tick(ahora) {
            cambios.push((ts_ms as f64 / 1000.0, c));
        }
        let dt = t0.elapsed();
        if dt > Duration::from_micros(100) {
            caros.push(dt);
        }
        pos += BLOQUE;
    }

    if cambios.is_empty() {
        println!("  (ningún aviso)");
    }
    for (s, c) in &cambios {
        println!(
            "  {:>8.1} s  {}  {:<17} {:<7} valor {:.3}",
            s,
            if c.activo { "ACTIVA " } else { "apaga  " },
            c.codigo,
            c.pista.as_str(),
            c.valor
        );
    }
    if !caros.is_empty() {
        let max = caros.iter().max().copied().unwrap_or_default();
        let media = caros.iter().sum::<Duration>() / caros.len() as u32;
        println!(
            "  coste de tick: {} evaluaciones > 100 µs, máx {:.2} ms, media {:.2} ms",
            caros.len(),
            max.as_secs_f64() * 1000.0,
            media.as_secs_f64() * 1000.0
        );
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut carpetas: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if carpetas.is_empty() {
        let raiz = carpeta_audio();
        for entrada in std::fs::read_dir(&raiz)
            .map_err(|e| format!("no se pudo leer {}: {e}", raiz.display()))?
        {
            let ruta = entrada?.path();
            if ruta.join("mic.wav").exists() && ruta.join("system.wav").exists() {
                carpetas.push(ruta);
            }
        }
        carpetas.sort();
    }
    for carpeta in carpetas {
        if let Err(e) = evaluar(&carpeta) {
            eprintln!("{}: {e}", carpeta.display());
        }
    }
    Ok(())
}
