use dictar_audio::wav::leer_wav;
use dictar_stt::whisper::Transcriptor;
use dictar_stt::SttOpciones;
use std::path::PathBuf;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let modelo = PathBuf::from(args.next().ok_or("falta ruta del modelo")?);
    let audio = PathBuf::from(args.next().ok_or("falta ruta WAV")?);
    let idioma = args.next().and_then(|s| s.into_string().ok());
    let (pcm, sample_rate) = leer_wav(audio)?;
    let segundos = pcm.len() as f64 / sample_rate as f64;
    let stt = Transcriptor::cargar(modelo)?;
    let mut opts = SttOpciones::en_vivo();
    opts.idioma = idioma.filter(|s| s != "auto");
    let inicio = Instant::now();
    let segmentos = stt.transcribir(&pcm, &opts)?;
    let transcurrido = inicio.elapsed().as_secs_f64();
    let texto = segmentos.iter().map(|s| s.texto.as_str()).collect::<Vec<_>>().join(" ");
    println!("audio_s={segundos:.2} wall_s={transcurrido:.2} rtf={:.3} hilos={}", transcurrido / segundos, opts.hilos);
    println!("{texto}");
    Ok(())
}
