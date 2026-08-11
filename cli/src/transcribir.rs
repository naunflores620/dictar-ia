//! Orden `transcribir`: pasa un WAV por el VAD y por Whisper.
//!
//! Es la prueba 3 de la fase 0, la que decide el proyecto: medir la velocidad
//! **real** de `large-v3-turbo` en esta máquina, no la del README de nadie.
//! Si va a 3× tiempo real, diez horas de clase son tres horas y media de
//! procesado nocturno y todo funciona. Si va a 0,5×, hay que replantear.

use anyhow::{bail, Context, Result};
use dictar_audio::wav::leer_wav;
use dictar_audio::SAMPLE_RATE;
use dictar_domain::Track;
use dictar_stt::modelos::{self, Modelo};
use dictar_stt::whisper::Transcriptor;
use dictar_stt::SttOpciones;
use dictar_vad::segmentador::Segmentador;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

pub fn ejecutar(opts: &HashMap<String, String>) -> Result<()> {
    let Some(entrada) = opts.get("in") else {
        bail!("falta --in <archivo.wav>");
    };

    let modelo = match opts.get("modelo").map(String::as_str).unwrap_or("turbo") {
        "small" => Modelo::Small,
        "medium" => Modelo::Medium,
        "turbo" | "large" => Modelo::LargeV3Turbo,
        otro => bail!("modelo desconocido «{otro}»: usa small, medium o turbo"),
    };

    let ruta_modelo = modelos::localizar(modelo)?;

    println!("Modelo:  {}", modelo.nombre_humano());
    println!("Archivo: {entrada}");

    let (pcm, sr) = leer_wav(entrada).with_context(|| format!("no se pudo leer «{entrada}»"))?;
    if sr != SAMPLE_RATE {
        bail!(
            "el audio está a {sr} Hz y hace falta a {SAMPLE_RATE}. \
             Grábalo con «dictar grabar», que ya remuestrea."
        );
    }

    let duracion_s = pcm.len() as f64 / SAMPLE_RATE as f64;
    println!("Duración: {duracion_s:.1} s\n");

    // -- Troceado ------------------------------------------------------------
    let mut seg = Segmentador::nuevo(Track::System);
    let mut segmentos = seg.empujar(&pcm, 0);
    if let Some(ultimo) = seg.finalizar() {
        segmentos.push(ultimo);
    }

    let utiles: Vec<_> = segmentos
        .iter()
        .filter(|s| s.merece_transcribirse())
        .collect();

    let descartados = segmentos.len() - utiles.len();
    println!(
        "Segmentos: {} ({} descartados por no tener voz)",
        segmentos.len(),
        descartados
    );
    if descartados > 0 {
        // El ahorro no es solo de tiempo: es donde Whisper se inventa frases.
        println!("  → ese silencio no llega a Whisper, que es donde alucina\n");
    }

    // -- Carga del modelo ----------------------------------------------------
    let t_carga = Instant::now();
    let trans = Transcriptor::cargar(&ruta_modelo)?;
    println!(
        "Modelo cargado en {:.1} s\n",
        t_carga.elapsed().as_secs_f64()
    );

    // -- Transcripción -------------------------------------------------------
    let mut opciones = SttOpciones::definitiva();
    if let Some(g) = opts.get("glosario") {
        opciones.glosario = Some(g.clone());
    }
    if let Some(i) = opts.get("idioma") {
        opciones.idioma = Some(i.clone());
    }

    println!("Transcribiendo con {} hilos…\n", opciones.hilos);
    let t0 = Instant::now();
    let mut audio_procesado_s = 0.0f64;

    for s in &utiles {
        let frags = trans.transcribir(&s.pcm, &opciones)?;
        audio_procesado_s += s.pcm.len() as f64 / SAMPLE_RATE as f64;

        for f in frags {
            let ts = s.inicio_ms + f.inicio_ms;
            // La confianza se muestra: distinguir lo cierto de lo dudoso es lo
            // que permite fiarse de unos apuntes automáticos.
            let marca = if f.confianza < 0.5 { " ⚠️" } else { "" };
            println!("[{}]{marca} {}", fmt(ts), f.texto);
        }
    }

    let transcurrido = t0.elapsed().as_secs_f64();
    let velocidad = if transcurrido > 0.0 {
        audio_procesado_s / transcurrido
    } else {
        0.0
    };

    println!("\n─────────────────────────────────────────────");
    println!("Audio con voz:   {audio_procesado_s:.1} s de {duracion_s:.1} s");
    println!("Tiempo de CPU:   {transcurrido:.1} s");
    println!("Velocidad:       {velocidad:.1}× tiempo real");

    // Lo que de verdad quiere saber el usuario: cuánto tarda su semana.
    if velocidad > 0.0 {
        let diez_horas_min = (10.0 * 3600.0 / velocidad) / 60.0;
        println!(
            "\n→ Diez horas de clase se procesarían en {:.0} min ({:.1} h)",
            diez_horas_min,
            diez_horas_min / 60.0
        );
        if diez_horas_min > 300.0 {
            println!(
                "  ⚠️  Es mucho. Prueba con --modelo medium, o revisa que Vulkan esté activo."
            );
        }
    }

    Ok(())
}

/// Descarga un modelo.
pub fn descargar(opts: &HashMap<String, String>) -> Result<()> {
    let modelo = match opts.get("descargar").map(String::as_str).unwrap_or("turbo") {
        "small" => Modelo::Small,
        "medium" => Modelo::Medium,
        _ => Modelo::LargeV3Turbo,
    };

    let dir = modelos::preparar_dir()?;
    let destino: PathBuf = dir.join(modelo.archivo());

    if modelos::esta_descargado(modelo) {
        println!("✔ {} ya está en {}", modelo.archivo(), destino.display());
        return Ok(());
    }

    println!("Descarga manual ({} MB):\n", modelo.tamano_mb());
    println!("  curl -fL --progress-bar -o {} \\", destino.display());
    println!("    {}\n", modelo.url());
    println!("La descarga integrada llegará con la interfaz; desde la línea de");
    println!("órdenes, curl da mejor barra de progreso y permite reanudar.");

    Ok(())
}

fn fmt(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}
