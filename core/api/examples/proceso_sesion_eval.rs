//! Ejecuta la ruta final de `Nucleo::procesar_sesion` sobre WAV existentes,
//! en un directorio aislado y sin permitir llamadas a proveedores externos.
//!
//! Uso:
//! cargo run --release -p dictar-api --example proceso_sesion_eval -- \
//!   <system.wav> <mic.wav> <directorio-temporal> [turbo|small|tiny] [vivo.json]

use dictar_api::Nucleo;
use dictar_audio::wav::leer_wav;
use dictar_domain::{Session, SessionKind, SessionSource, Track};
use dictar_storage::Db;
use dictar_stt::modelos::Modelo;
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

#[derive(Deserialize)]
struct FraseViva {
    track: String,
    speaker: Option<String>,
    start_ms: i64,
    end_ms: i64,
    text: String,
    confidence: Option<f32>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let system = PathBuf::from(args.next().ok_or("falta system.wav")?);
    let mic = PathBuf::from(args.next().ok_or("falta mic.wav")?);
    let data = PathBuf::from(args.next().ok_or("falta directorio temporal")?);
    let modelo = match args
        .next()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .as_deref()
    {
        None | Some("turbo") => Modelo::LargeV3Turbo,
        Some("small") => Modelo::Small,
        Some("tiny") => Modelo::Tiny,
        Some(otro) => return Err(format!("modelo no reconocido: {otro}").into()),
    };
    let semilla_viva = args.next().map(PathBuf::from);

    std::fs::create_dir_all(&data)?;
    // El proveedor está definido pero no tiene una clave resoluble. Router lo
    // omite antes de enviar solicitudes, y el procesado devuelve su error al
    // llegar a la fase de notas, después de persistir la transcripción.
    std::fs::write(
        data.join("providers.toml"),
        "[[provider]]\nid = \"bench-no-network\"\nkind = \"openai_compat\"\n\
         base_url = \"https://invalid.example/v1\"\nmodel = \"disabled\"\n\
         api_key_ref = \"env:DICTAR_BENCH_NO_NETWORK_KEY_20261008\"\n",
    )?;

    let (system_pcm, system_rate) = leer_wav(&system)?;
    let (mic_pcm, mic_rate) = leer_wav(&mic)?;
    if system_rate != mic_rate {
        return Err(format!("sample rates distintos: system={system_rate}, mic={mic_rate}").into());
    }
    let system_ms = system_pcm.len() as u64 * 1000 / system_rate as u64;
    let mic_ms = mic_pcm.len() as u64 * 1000 / mic_rate as u64;
    let duracion_ms = system_ms.max(mic_ms) as i64;
    let mut session = Session::nueva(
        SessionKind::Class,
        SessionSource::DesktopLoopback,
        1_000_000,
    );
    session.title = Some("Evaluación local de procesamiento final".into());
    session.language = Some("es".into());
    session.ended_at = Some(session.started_at + duracion_ms);
    let audio_dir = data.join("audio").join(session.id.as_str());
    std::fs::create_dir_all(&audio_dir)?;
    std::fs::copy(&system, audio_dir.join("system.wav"))?;
    std::fs::copy(&mic, audio_dir.join("mic.wav"))?;
    session.audio_path = Some(audio_dir.to_string_lossy().into_owned());
    let session_id = session.id.clone();

    let db_path = data.join("dictar.db");
    let mut db = Db::abrir(&db_path)?;
    db.crear_sesion(&session)?;
    let frases_semilla = if let Some(path) = semilla_viva {
        let frases: Vec<FraseViva> = serde_json::from_slice(&std::fs::read(path)?)?;
        let nuevas: Vec<_> = frases
            .into_iter()
            .map(|f| dictar_storage::transcript::NewUtterance {
                track: if f.track == "mic" {
                    Track::Mic
                } else {
                    Track::System
                },
                speaker: f.speaker,
                start_ms: f.start_ms,
                end_ms: f.end_ms,
                text: f.text,
                confidence: f.confidence,
                is_final: false,
            })
            .collect();
        db.insertar_frases(&session_id, &nuevas)?;
        nuevas.len()
    } else {
        0
    };
    drop(db);

    let nucleo = Nucleo::abrir(&data)?;
    let (tx, rx) = mpsc::channel();
    let inicio = Instant::now();
    let resultado = nucleo
        .procesar_sesion(&session_id, modelo, 2_000_000, Some(tx))
        .await;
    let segundos = inicio.elapsed().as_secs_f64();

    let frases = nucleo.transcripcion(&session_id)?;
    let finales = frases.iter().filter(|f| f.is_final).count();
    let fts_hits = if frases_semilla > 0 {
        Db::abrir(&db_path)?
            .buscar("tablero", Some(&session_id))?
            .len()
    } else {
        0
    };
    let frases_finales: Vec<_> = frases
        .iter()
        .map(|f| {
            json!({
                "track": f.track.as_str(),
                "speaker": f.speaker.as_deref(),
                "start_ms": f.start_ms,
                "end_ms": f.end_ms,
                "text": f.text.as_str(),
                "is_final": f.is_final,
                "confidence": f.confidence,
            })
        })
        .collect();
    let salida = data.join("transcripcion_final.json");
    std::fs::write(&salida, serde_json::to_vec_pretty(&frases_finales)?)?;

    let cuenta = |track: Track| frases.iter().filter(|f| f.track == track).count();
    let palabras: usize = frases
        .iter()
        .map(|f| f.text.split_whitespace().count())
        .sum();
    let eventos: Vec<_> = rx.try_iter().collect();
    let progreso = eventos
        .iter()
        .filter(|e| matches!(e, dictar_api::Evento::Progreso { .. }))
        .count();
    let ultimo_fin_ms = frases.iter().map(|f| f.end_ms).max().unwrap_or(0);
    let cobertura = if duracion_ms > 0 {
        ultimo_fin_ms as f64 / duracion_ms as f64
    } else {
        0.0
    };

    println!(
        "model={modelo:?} audio_ms={duracion_ms} system_ms={system_ms} mic_ms={mic_ms} elapsed_s={segundos:.2} RTF={:.3} utterances={} final_utterances={finales} seeded_live={frases_semilla} fts_hits_tablero={fts_hits} mic_utterances={} system_utterances={} words={} last_end_ms={} coverage={:.3} progress_events={progreso} transcript={}",
        segundos * 1000.0 / duracion_ms.max(1) as f64,
        frases.len(),
        cuenta(Track::Mic),
        cuenta(Track::System),
        palabras,
        ultimo_fin_ms,
        cobertura,
        salida.display(),
    );
    println!("session_id={session_id}");
    println!(
        "session_status={}",
        nucleo.sesion(&session_id)?.status.as_str()
    );
    match resultado {
        Ok(res) => println!(
            "process_result=ok provider={} notes={}",
            res.proveedor, res.frases
        ),
        Err(error) => println!("process_result=expected_notes_provider_error error={error}"),
    }
    Ok(())
}
