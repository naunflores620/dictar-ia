//! Reproduce WAV locales a ritmo real contra el reconocedor en vivo de producci?n.
//! Uso: cargo run --release -p dictar-api --example vosk_live_eval -- <system.wav> <dir> [mic.wav]

use dictar_api::transcriptor::TranscriptorVivo;
use dictar_api::Evento;
use dictar_audio::wav::leer_wav;
use dictar_domain::{Session, SessionKind, SessionSource, Track};
use dictar_storage::Db;
use dictar_stt::modelos::Modelo;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let wav = PathBuf::from(args.next().ok_or("falta ruta WAV")?);
    let data_dir = PathBuf::from(args.next().ok_or("falta directorio de datos")?);
    let mic_wav = args.next().map(PathBuf::from);
    let idioma = std::env::var("DICTAR_LIVE_EVAL_LANGUAGE")
        .unwrap_or_else(|_| "es".to_owned());
    std::fs::create_dir_all(&data_dir)?;
    let db_path = data_dir.join("bench.db");
    let metrics_path = data_dir.join("stt_metrics.jsonl");
    let (pcm, sample_rate) = leer_wav(&wav)?;
    let pcm_mic = mic_wav
        .as_ref()
        .map(leer_wav)
        .transpose()?
        .map(|(pcm, sr)| {
            if sr != sample_rate {
                return Err(format!(
                    "las frecuencias no coinciden: sistema={sample_rate}, micrófono={sr}"
                ));
            }
            Ok(pcm)
        })
        .transpose()?;
    let audio_ms =
        pcm.len().max(pcm_mic.as_ref().map_or(0, Vec::len)) as u64 * 1000 / sample_rate as u64;
    let bloques_sistema: Vec<_> = pcm.chunks(1024).collect();
    let bloques_mic: Option<Vec<_>> = pcm_mic.as_ref().map(|p| p.chunks(1024).collect());
    let cantidad_bloques = bloques_sistema
        .len()
        .max(bloques_mic.as_ref().map_or(0, Vec::len));

    let db = Db::abrir(&db_path)?;
    let session = Session::nueva(SessionKind::Class, SessionSource::DesktopLoopback, 0);
    let session_id = session.id.clone();
    db.crear_sesion(&session)?;
    drop(db);

    let (tx, rx) = mpsc::channel();
    let inicio = Instant::now();
    let trans = TranscriptorVivo::iniciar(
        session_id.clone(),
        db_path.clone(),
        Modelo::Small,
        String::new(),
        Some(idioma),
        "Profesor".into(),
        tx,
        inicio,
        metrics_path.clone(),
    )?;
    let inicio_captura = Instant::now();
    trans.marcar_inicio_captura(inicio_captura);

    // WASAPI normaliza audio a 16 kHz en bloques cercanos a 1024 muestras.
    // El feeder respeta la duraciÃ³n del audio para representar captura viva.
    let bloque = 1024usize;
    let mut bloques = 0usize;
    let mut actualizaciones = 0usize;
    let mut finales = 0usize;
    let mut rangos = HashSet::<(String, i64)>::new();
    let mut anteriores = HashMap::<(String, i64), (String, i64, String)>::new();
    let mut revisiones = 0usize;
    let mut retrocesos_estables = 0usize;
    let mut estables_revisados = 0usize;
    let mut snapshots_duplicados = 0usize;
    let mut limites_utf16_invalidos = 0usize;
    let mut descartes = 0usize;
    let mut errores = Vec::<String>::new();
    let mut primera_parcial_ms = None::<u64>;
    let mut edad_primera_parcial_ms = None::<u64>;
    let mut edad_parcial_max_ms = 0u64;
    let inicio_feeder = Instant::now();

    for i in 0..cantidad_bloques {
        let timestamp_ms = i as u64 * bloque as u64 * 1000 / sample_rate as u64;
        if let Some(muestras) = bloques_mic.as_ref().and_then(|v| v.get(i)) {
            trans.encolar_bloque(dictar_audio::AudioFrame {
                track: Track::Mic,
                pcm: muestras.to_vec(),
                timestamp_ms: timestamp_ms as i64,
            });
            bloques += 1;
        }
        if let Some(muestras) = bloques_sistema.get(i) {
            trans.encolar_bloque(dictar_audio::AudioFrame {
                track: Track::System,
                pcm: muestras.to_vec(),
                timestamp_ms: timestamp_ms as i64,
            });
            bloques += 1;
        }

        while let Ok(evento) = rx.try_recv() {
            match evento {
                Evento::Frase {
                    track,
                    inicio_ms,
                    fin_ms,
                    estable_hasta_utf16,
                    texto,
                    definitiva,
                    ..
                } => {
                    let clave = (track.clone(), inicio_ms);
                    rangos.insert(clave.clone());
                    if definitiva {
                        finales += 1;
                        anteriores.remove(&clave);
                    } else {
                        let ahora_ms = inicio_captura.elapsed().as_millis() as u64;
                        let edad_ms = ahora_ms.saturating_sub(fin_ms.max(0) as u64);
                        edad_parcial_max_ms = edad_parcial_max_ms.max(edad_ms);
                        if primera_parcial_ms.is_none() {
                            primera_parcial_ms = Some(ahora_ms);
                            edad_primera_parcial_ms = Some(edad_ms);
                        }
                        actualizaciones += 1;
                        let Some((estable, _)) = prefijo_utf16(&texto, estable_hasta_utf16) else {
                            limites_utf16_invalidos += 1;
                            continue;
                        };
                        if let Some((texto_anterior, limite_anterior, estable_anterior)) =
                            anteriores.get(&clave)
                        {
                            if texto_anterior == &texto && limite_anterior == &estable_hasta_utf16 {
                                snapshots_duplicados += 1;
                            } else {
                                revisiones += 1;
                            }
                            if estable_hasta_utf16 < *limite_anterior {
                                retrocesos_estables += 1;
                            }
                            if !estable.starts_with(estable_anterior) {
                                estables_revisados += 1;
                            }
                        }
                        let estable = estable.to_owned();
                        anteriores.insert(clave, (texto, estable_hasta_utf16, estable));
                    }
                }
                Evento::Error { mensaje, .. } => errores.push(mensaje),
                _ => {}
            }
        }

        let esperado =
            Duration::from_micros((i as u64 + 1) * bloque as u64 * 1_000_000 / sample_rate as u64);
        let restante = esperado.saturating_sub(inicio_feeder.elapsed());
        if !restante.is_zero() {
            std::thread::sleep(restante);
        }
    }

    trans.pedir_fin();
    trans.esperar();
    while let Ok(evento) = rx.try_recv() {
        match evento {
            Evento::Frase {
                track,
                inicio_ms,
                fin_ms,
                estable_hasta_utf16,
                texto,
                definitiva,
                ..
            } => {
                let clave = (track.clone(), inicio_ms);
                rangos.insert(clave.clone());
                if definitiva {
                    finales += 1;
                    anteriores.remove(&clave);
                } else {
                    let ahora_ms = inicio_captura.elapsed().as_millis() as u64;
                    let edad_ms = ahora_ms.saturating_sub(fin_ms.max(0) as u64);
                    edad_parcial_max_ms = edad_parcial_max_ms.max(edad_ms);
                    if primera_parcial_ms.is_none() {
                        primera_parcial_ms = Some(ahora_ms);
                        edad_primera_parcial_ms = Some(edad_ms);
                    }
                    actualizaciones += 1;
                    let Some((estable, _)) = prefijo_utf16(&texto, estable_hasta_utf16) else {
                        limites_utf16_invalidos += 1;
                        continue;
                    };
                    if let Some((texto_anterior, limite_anterior, estable_anterior)) =
                        anteriores.get(&clave)
                    {
                        if texto_anterior == &texto && limite_anterior == &estable_hasta_utf16 {
                            snapshots_duplicados += 1;
                        } else {
                            revisiones += 1;
                        }
                        if estable_hasta_utf16 < *limite_anterior {
                            retrocesos_estables += 1;
                        }
                        if !estable.starts_with(estable_anterior) {
                            estables_revisados += 1;
                        }
                    }
                    let estable = estable.to_owned();
                    anteriores.insert(clave, (texto, estable_hasta_utf16, estable));
                }
            }
            Evento::Error { mensaje, .. } => errores.push(mensaje),
            _ => {}
        }
    }

    if let Ok(raw) = std::fs::read_to_string(&metrics_path) {
        for linea in raw.lines() {
            if let Ok(m) = serde_json::from_str::<serde_json::Value>(linea) {
                descartes =
                    descartes.max(m["trabajos_descartados_total"].as_u64().unwrap_or(0) as usize);
            }
        }
    }

    println!(
        "audio_ms={audio_ms} bloques={bloques} pared_s={:.2} actualizaciones={actualizaciones} primera_parcial_ms={primera_parcial_ms:?} edad_primera_parcial_ms={edad_primera_parcial_ms:?} edad_parcial_max_ms={edad_parcial_max_ms} finales={finales} rangos={} revisiones={revisiones} retrocesos_estables={retrocesos_estables} estables_revisados={estables_revisados} snapshots_duplicados={snapshots_duplicados} limites_utf16_invalidos={limites_utf16_invalidos} descartes={descartes} metricas={}",
        inicio.elapsed().as_secs_f64(),
        rangos.len(),
        metrics_path.display(),
    );
    println!("errores={errores:?}");

    let db = Db::abrir(&db_path)?;
    let frases = db.transcripcion(&session_id)?;
    let texto = frases
        .iter()
        .map(|u| {
            format!(
                "[{}] {}: {}",
                u.start_ms,
                u.speaker.as_deref().unwrap_or("Profesor"),
                u.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let ruta_texto = data_dir.join("transcripcion_en_vivo.txt");
    std::fs::write(&ruta_texto, &texto)?;
    let palabras: usize = frases
        .iter()
        .map(|u| u.text.split_whitespace().count())
        .sum();
    let frases_con_bucle = frases
        .iter()
        .filter(|u| {
            let palabras: Vec<_> = u.text.split_whitespace().collect();
            dictar_stt::limpieza::tiene_bucle_repeticion(&palabras)
        })
        .count();
    println!(
        "frases_guardadas={} palabras={} frases_con_bucle={} transcripcion={}",
        frases.len(),
        palabras,
        frases_con_bucle,
        ruta_texto.display()
    );
    Ok(())
}

fn prefijo_utf16(texto: &str, limite: i64) -> Option<(&str, &str)> {
    let limite = usize::try_from(limite).ok()?;
    if limite == 0 {
        return Some(("", texto));
    }
    let mut unidades = 0usize;
    for (indice, caracter) in texto.char_indices() {
        if unidades == limite {
            return Some(texto.split_at(indice));
        }
        unidades += caracter.len_utf16();
        if unidades > limite {
            return None;
        }
    }
    (unidades == limite).then_some((texto, ""))
}
