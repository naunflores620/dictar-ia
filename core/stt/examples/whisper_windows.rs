use dictar_audio::wav::leer_wav;
use dictar_stt::acuerdo::AcuerdoLocal;
use dictar_stt::whisper::Transcriptor;
use dictar_stt::SttOpciones;
use std::path::PathBuf;
use std::time::Instant;

fn percentile(v: &mut [u128], p: usize) -> u128 {
    if v.is_empty() { return 0; }
    v.sort_unstable();
    v[((v.len() - 1) * p) / 100]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let modelo = PathBuf::from(args.next().ok_or("falta modelo")?);
    let wav = PathBuf::from(args.next().ok_or("falta WAV")?);
    let idioma = args.next().and_then(|s| s.into_string().ok()).unwrap_or_else(|| "es".into());
    let ventana_s: usize = args.next().and_then(|s| s.into_string().ok()).and_then(|s| s.parse().ok()).unwrap_or(12);
    let (pcm, sr) = leer_wav(wav)?;
    let audio_s = pcm.len() as f64 / sr as f64;
    let trans = Transcriptor::cargar(modelo)?;
    let mut opts = SttOpciones::en_vivo();
    opts.idioma = if idioma == "auto" { None } else { Some(idioma) };
    let mut acuerdo = AcuerdoLocal::nuevo();
    let mut duraciones = Vec::new();
    let mut audio_ventanas = 0usize;
    let mut completion_s = 0.0f64;
    let mut max_age = 0.0f64;
    let mut total_stable_words = 0usize;
    let mut windows = 0usize;
    let start = Instant::now();
    let ventana_n = ventana_s * sr as usize;
    let mut fin = 3 * sr as usize;
    while fin <= pcm.len() {
        let inicio = fin.saturating_sub(ventana_n);
        let muestra = &pcm[inicio..fin];
        let t = Instant::now();
        let fragments = trans.transcribir(muestra, &opts)?;
        let infer_s = t.elapsed().as_secs_f64();
        duraciones.push(t.elapsed().as_millis());
        audio_ventanas += muestra.len();
        let capture_s = fin as f64 / sr as f64;
        completion_s = completion_s.max(capture_s) + infer_s;
        max_age = max_age.max(completion_s - capture_s);
        let text = fragments.iter().map(|f| f.texto.as_str()).collect::<Vec<_>>().join(" ");
        let (_, provisional) = acuerdo.actualizar(&text);
        total_stable_words = acuerdo.texto_confirmado().split_whitespace().count();
        windows += 1;
        println!("t={:.1}s infer={:.2}s age_est={:.2}s stable_words={} provisional={}", capture_s, infer_s, completion_s-capture_s, total_stable_words, provisional);
        fin = fin.saturating_add(3 * sr as usize);
    }
    let elapsed = start.elapsed().as_secs_f64();
    let p50 = percentile(&mut duraciones, 50);
    let p95 = percentile(&mut duraciones, 95);
    println!("SUMMARY audio_s={audio_s:.2} windows={windows} window_s={ventana_s} stride_s=3 audio_windows_s={:.2} infer_wall_s={elapsed:.2} rtf={:.3} infer_p50_ms={p50} infer_p95_ms={p95} estimated_max_age_s={max_age:.2} final_stable_words={total_stable_words}", audio_ventanas as f64 / sr as f64, elapsed / audio_s);
    Ok(())
}
