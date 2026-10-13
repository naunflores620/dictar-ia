//! Resumen de las métricas locales emitidas por el transcriptor en vivo.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, Deserialize)]
struct Metrica {
    tipo: String,
    pista: String,
    inferencia_ms: u64,
    espera_cola_ms: u64,
    edad_al_final_inferencia_ms: u64,
    rtf: f64,
    cola_pendiente: usize,
    hipotesis_descartadas_total: usize,
    exito: bool,
    #[serde(default)]
    bloques: Option<u64>,
    #[serde(default)]
    fallos: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct MetricaCaptura {
    pista: String,
    inicio_audio_ms: i64,
    fin_audio_ms: i64,
    tramas: u64,
    muestras: u64,
    duracion_audio_ms: u64,
    nivel_rms: f64,
    pico_abs: f64,
    muestras_recortadas: u64,
    tramas_casi_silencio: u64,
}

pub fn ejecutar(opciones: &HashMap<String, String>) -> Result<()> {
    let Some(ruta) = opciones.get("in") else {
        bail!("falta --in <archivo_de_métricas|carpeta_de_sesión>");
    };
    let ruta_path = Path::new(ruta);
    if ruta_path.is_dir() {
        return resumir_sesion(ruta_path);
    }

    let archivo = File::open(ruta).with_context(|| format!("no se pudo abrir «{ruta}»"))?;
    let mut lineas = Vec::new();
    for (i, linea) in BufReader::new(archivo).lines().enumerate() {
        let linea = linea.with_context(|| format!("no se pudo leer la línea {}", i + 1))?;
        if !linea.trim().is_empty() {
            lineas.push((i + 1, linea));
        }
    }
    if lineas.is_empty() {
        bail!("el archivo no contiene métricas");
    }
    let primero: serde_json::Value = serde_json::from_str(&lineas[0].1)
        .with_context(|| format!("JSON inválido en la línea {}", lineas[0].0))?;
    if primero.get("tipo").and_then(serde_json::Value::as_str) == Some("captura") {
        return resumir_captura(&lineas);
    }

    let mut metricas = Vec::new();
    for (i, linea) in lineas {
        metricas.push(
            serde_json::from_str::<Metrica>(&linea)
                .with_context(|| format!("JSON inválido en la línea {i}"))?,
        );
    }

    let fallos: u64 = metricas
        .iter()
        .map(|m| m.fallos.unwrap_or(if m.exito { 0 } else { 1 }))
        .sum();
    let bloques_vosk: u64 = metricas.iter().filter_map(|m| m.bloques).sum();
    let exitosas: Vec<_> = metricas.iter().filter(|m| m.exito).collect();
    println!(
        "Registros: {} correctos; inferencias fallidas: {fallos}; bloques Vosk: {bloques_vosk}",
        exitosas.len()
    );

    let mut por_tipo: BTreeMap<(&str, &str), Vec<&Metrica>> = BTreeMap::new();
    for metrica in &exitosas {
        por_tipo
            .entry((&metrica.tipo, &metrica.pista))
            .or_default()
            .push(metrica);
    }

    for ((tipo, pista), filas) in por_tipo {
        if tipo == "vosk_bloques" {
            let bloques: u64 = filas.iter().filter_map(|m| m.bloques).sum();
            println!(
                "\n{tipo} · {pista} · {} ventanas agregadas, {bloques} bloques",
                filas.len()
            );
        } else {
            println!("\n{tipo} · {pista} · {} inferencias", filas.len());
        }
        imprimir_percentiles(
            "edad al finalizar inferencia",
            filas.iter().map(|m| m.edad_al_final_inferencia_ms),
        );
        imprimir_percentiles("inferencia", filas.iter().map(|m| m.inferencia_ms));
        imprimir_percentiles("espera en cola", filas.iter().map(|m| m.espera_cola_ms));
        imprimir_rtf(filas.iter().map(|m| m.rtf));
        let max_cola = filas.iter().map(|m| m.cola_pendiente).max().unwrap_or(0);
        let descartadas = filas
            .iter()
            .map(|m| m.hipotesis_descartadas_total)
            .max()
            .unwrap_or(0);
        println!("cola máxima pendiente: {max_cola}");
        if tipo == "vosk_bloques" {
            println!("trabajos descartados acumulados: {descartadas}");
        } else {
            println!("hipótesis descartadas acumuladas: {descartadas}");
        }
    }

    Ok(())
}

fn resumir_sesion(dir: &Path) -> Result<()> {
    let mut encontro_datos = false;
    for nombre in ["capture_metrics.jsonl", "stt_metrics.jsonl"] {
        let ruta = dir.join(nombre);
        if ruta.is_file() {
            println!("\n== {nombre} ==");
            let opciones = HashMap::from([("in".to_owned(), ruta.to_string_lossy().into_owned())]);
            ejecutar(&opciones)?;
            encontro_datos = true;
        }
    }

    let mic_path = dir.join("mic.wav");
    let system_path = dir.join("system.wav");
    if mic_path.is_file() && system_path.is_file() {
        let (mic, sr_mic) = dictar_audio::wav::leer_wav(&mic_path)
            .with_context(|| format!("no se pudo leer {}", mic_path.display()))?;
        let (system, sr_system) = dictar_audio::wav::leer_wav(&system_path)
            .with_context(|| format!("no se pudo leer {}", system_path.display()))?;
        imprimir_estadisticas_wav("mic", &mic, sr_mic);
        imprimir_estadisticas_wav("system", &system, sr_system);
        if sr_mic == dictar_audio::SAMPLE_RATE && sr_system == dictar_audio::SAMPLE_RATE {
            println!("\n== Posible retorno acústico micrófono/sistema ==");
            match dictar_audio::analizar_retorno_acustico(&mic, &system) {
                Some((correlacion, retardo_ms)) if correlacion >= 0.65 => println!(
                    "indicador compatible con retorno: correlación {:.3}, retardo estimado {} ms (umbral 0.65)",
                    correlacion, retardo_ms
                ),
                Some((correlacion, retardo_ms)) => println!(
                    "correlación máxima {:.3} a {} ms; bajo el umbral 0.65 del detector de mezcla",
                    correlacion, retardo_ms
                ),
                None => println!("sin suficientes bloques activos para estimar el retorno"),
            }

            println!("\n== Decisión del filtro usado al procesar la sesión ==");
            match dictar_audio::detectar_retorno_en_wav(&mic_path, &system_path)? {
                Some(retorno) => println!(
                    "el micrófono se atenuaría durante la salida del sistema: correlación {:.3}, retardo {} ms",
                    retorno.correlacion, retorno.retardo_ms
                ),
                None => println!("no se detectó retorno; el micrófono se conservaría sin filtrar"),
            }
        } else {
            println!(
                "\nno se calculó correlación: las frecuencias de muestreo son {sr_mic} y {sr_system} Hz"
            );
        }
        encontro_datos = true;
    }

    if !encontro_datos {
        bail!("la carpeta no contiene WAV de pistas ni archivos de métricas reconocidos");
    }
    Ok(())
}

fn imprimir_estadisticas_wav(pista: &str, pcm: &[f32], sample_rate: u32) {
    if pcm.is_empty() || sample_rate == 0 {
        println!("\n{pista}: WAV vacío o con frecuencia de muestreo inválida");
        return;
    }
    const UMBRAL_SILENCIO_RMS: f64 = 0.003;
    let energia = pcm.iter().map(|x| (*x as f64).powi(2)).sum::<f64>();
    let rms = (energia / pcm.len() as f64).sqrt();
    let pico = pcm
        .iter()
        .filter(|x| x.is_finite())
        .map(|x| x.abs() as f64)
        .max_by(f64::total_cmp)
        .unwrap_or(0.0);
    let recortadas = pcm.iter().filter(|x| x.abs() >= 0.999).count();
    let tam_ventana = (sample_rate as usize / 50).max(1);
    let ventanas: Vec<_> = pcm.chunks(tam_ventana).collect();
    let silenciosas = ventanas
        .iter()
        .filter(|bloque| {
            let e = bloque.iter().map(|x| (*x as f64).powi(2)).sum::<f64>();
            (e / bloque.len().max(1) as f64).sqrt() < UMBRAL_SILENCIO_RMS
        })
        .count();
    let duracion_ms = pcm.len() as u128 * 1000 / sample_rate as u128;
    println!(
        "\n{pista} · {duracion_ms} ms · RMS={rms:.4} · pico={pico:.4} · recorte={:.4}% de muestras · silencio={:.1}% de ventanas de 20 ms",
        recortadas as f64 * 100.0 / pcm.len() as f64,
        silenciosas as f64 * 100.0 / ventanas.len().max(1) as f64,
    );
}

fn resumir_captura(lineas: &[(usize, String)]) -> Result<()> {
    let metricas: Vec<MetricaCaptura> = lineas
        .iter()
        .map(|(i, linea)| {
            serde_json::from_str(linea)
                .with_context(|| format!("métrica de captura inválida en la línea {i}"))
        })
        .collect::<Result<_>>()?;
    let mut por_pista: BTreeMap<&str, Vec<&MetricaCaptura>> = BTreeMap::new();
    for metrica in &metricas {
        por_pista.entry(&metrica.pista).or_default().push(metrica);
    }

    println!("Métricas de captura: {} ventanas", metricas.len());
    for (pista, ventanas) in por_pista {
        let tramas: u64 = ventanas.iter().map(|m| m.tramas).sum();
        let muestras: u64 = ventanas.iter().map(|m| m.muestras).sum();
        let silencio: u64 = ventanas.iter().map(|m| m.tramas_casi_silencio).sum();
        let recortadas: u64 = ventanas.iter().map(|m| m.muestras_recortadas).sum();
        let duracion_ms: u64 = ventanas.iter().map(|m| m.duracion_audio_ms).sum();
        let inicio = ventanas
            .iter()
            .map(|m| m.inicio_audio_ms)
            .min()
            .unwrap_or(0);
        let fin = ventanas.iter().map(|m| m.fin_audio_ms).max().unwrap_or(0);
        let mut rms: Vec<f64> = ventanas.iter().map(|m| m.nivel_rms).collect();
        rms.sort_by(f64::total_cmp);
        let pico = ventanas
            .iter()
            .map(|m| m.pico_abs)
            .max_by(f64::total_cmp)
            .unwrap_or(0.0);
        println!(
            "\n{pista} · {} ventanas · {duracion_ms} ms de audio",
            ventanas.len()
        );
        println!("rango de marcas de audio: {inicio}–{fin} ms");
        println!(
            "RMS por ventana: p50={:.4} · p95={:.4}; pico máximo={pico:.4}",
            percentil_f64(&rms, 0.50),
            percentil_f64(&rms, 0.95)
        );
        println!(
            "tramas casi silenciosas: {silencio}/{tramas} ({:.1}%)",
            porcentaje(silencio, tramas)
        );
        println!(
            "muestras recortadas: {recortadas}/{muestras} ({:.4}%)",
            porcentaje(recortadas, muestras)
        );
    }
    Ok(())
}

fn porcentaje(parte: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        parte as f64 * 100.0 / total as f64
    }
}

fn percentil_f64(valores: &[f64], p: f64) -> f64 {
    if valores.is_empty() {
        return 0.0;
    }
    valores[percentil_indice(valores.len(), p)]
}

fn imprimir_percentiles(etiqueta: &str, valores: impl Iterator<Item = u64>) {
    let mut valores: Vec<u64> = valores.collect();
    valores.sort_unstable();
    if valores.is_empty() {
        return;
    }
    println!(
        "{etiqueta}: p50={} ms · p95={} ms",
        percentil(&valores, 0.50),
        percentil(&valores, 0.95)
    );
}

fn percentil(valores: &[u64], p: f64) -> u64 {
    let indice = percentil_indice(valores.len(), p);
    valores[indice]
}

fn imprimir_rtf(valores: impl Iterator<Item = f64>) {
    let mut valores: Vec<f64> = valores.collect();
    valores.sort_by(f64::total_cmp);
    if valores.is_empty() {
        return;
    }
    let p50 = percentil_indice(valores.len(), 0.50);
    let p95 = percentil_indice(valores.len(), 0.95);
    println!(
        "RTF por llamada: p50={:.2}× · p95={:.2}×",
        valores[p50], valores[p95]
    );
}

fn percentil_indice(longitud: usize, p: f64) -> usize {
    ((longitud as f64 * p).ceil() as usize)
        .saturating_sub(1)
        .min(longitud - 1)
}
