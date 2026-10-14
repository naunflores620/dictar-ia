use dictar_api::transcriptor::TranscriptorVivo;
use dictar_api::Evento;
use dictar_audio::wav::leer_wav;
use dictar_domain::{Session, SessionKind, SessionSource, Track};
use dictar_storage::Db;
use dictar_stt::modelos::Modelo;
use dictar_vad::segmentador::Segmentador;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let wav = PathBuf::from(args.next().ok_or("falta ruta WAV")?);
    let data_dir = PathBuf::from(args.next().ok_or("falta directorio de datos")?);
    std::fs::create_dir_all(&data_dir)?;
    let db_path = data_dir.join("bench.db");
    let metrics_path = data_dir.join("stt_metrics.jsonl");
    let (pcm, sample_rate) = leer_wav(&wav)?;
    let audio_s = pcm.len() as f64 / sample_rate as f64;

    let db = Db::abrir(&db_path)?;
    let session = Session::nueva(SessionKind::Class, SessionSource::DesktopLoopback, 0);
    db.crear_sesion(&session)?;
    drop(db);

    let (tx_eventos, rx_eventos) = mpsc::channel();
    let inicio = Instant::now();
    let modelo = match std::env::var("DICTAR_WHISPER_EVAL_MODEL")
        .unwrap_or_else(|_| "tiny".to_owned())
        .as_str()
    {
        "tiny" => Modelo::Tiny,
        "small" => Modelo::Small,
        otro => return Err(format!("modelo no admitido: {otro}; use tiny o small").into()),
    };
    let trans = TranscriptorVivo::iniciar(
        session.id.clone(),
        db_path,
        modelo,
        String::new(),
        Some("es".into()),
        "Profesor".into(),
        tx_eventos,
        inicio,
        metrics_path.clone(),
    )?;

    let block = (sample_rate as usize) / 5;
    let mut segmentador = Segmentador::nuevo(Track::System);
    let mut ultimo_borrador = Instant::now() - Duration::from_secs(3);
    let mut vistas = 0usize;
    let mut cerrados = 0usize;
    let mut actualizaciones = 0usize;
    let mut spans = HashSet::<(String, i64)>::new();
    let mut edades = Vec::<i64>::new();
    let mut primera_ed = None;

    let drenar = |rx: &mpsc::Receiver<Evento>,
                  actual_ms: i64,
                  actualizaciones: &mut usize,
                  spans: &mut HashSet<(String, i64)>,
                  edades: &mut Vec<i64>,
                  primera_ed: &mut Option<i64>| {
        while let Ok(ev) = rx.try_recv() {
            if let Evento::Frase {
                track,
                inicio_ms,
                fin_ms,
                definitiva,
                ..
            } = ev
            {
                if !definitiva {
                    *actualizaciones += 1;
                    spans.insert((track, inicio_ms));
                    let edad = (actual_ms - fin_ms).max(0);
                    edades.push(edad);
                    primera_ed.get_or_insert(edad);
                }
            }
        }
    };

    let bloque_ms = block as i64 * 1000 / sample_rate as i64;
    for (i, frame) in pcm.chunks(block).enumerate() {
        let marca = (i as i64) * bloque_ms;
        let cerrados_ahora = segmentador.empujar(frame, marca);
        let cerro = !cerrados_ahora.is_empty();
        for seg in cerrados_ahora {
            cerrados += 1;
            trans.encolar(seg);
        }
        let reloj = Instant::now();
        if ultimo_borrador.elapsed() >= Duration::from_secs(3) && !cerro && trans.pendientes() == 0
        {
            if let Some(vista) = segmentador.vista_previa() {
                if vista.duracion_ms() >= 2_000 && vista.merece_transcribirse() {
                    trans.encolar_hipotesis(vista);
                    vistas += 1;
                    ultimo_borrador = reloj;
                }
            }
        }
        let reloj_actual_ms = inicio.elapsed().as_millis() as i64;
        drenar(
            &rx_eventos,
            reloj_actual_ms,
            &mut actualizaciones,
            &mut spans,
            &mut edades,
            &mut primera_ed,
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    if let Some(seg) = segmentador.finalizar() {
        trans.encolar(seg);
    }
    trans.pedir_fin();
    trans.esperar();
    let reloj_final_ms = inicio.elapsed().as_millis() as i64;
    drenar(
        &rx_eventos,
        reloj_final_ms,
        &mut actualizaciones,
        &mut spans,
        &mut edades,
        &mut primera_ed,
    );

    edades.sort_unstable();
    let p95 = edades
        .get((edades.len().saturating_sub(1) * 95) / 100)
        .copied()
        .unwrap_or(0);
    println!("audio_s={audio_s:.2} total_wall_s={:.2} VAD_cerrados={cerrados} hipotesis_enviadas={vistas} actualizaciones_visibles={actualizaciones} spans_actualizados={} primera_edad_ms={} p95_edad_ms={p95}", inicio.elapsed().as_secs_f64(), spans.len(), primera_ed.unwrap_or(-1));
    println!("metricas={}", metrics_path.display());
    Ok(())
}
