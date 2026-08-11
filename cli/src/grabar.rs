//! Orden `grabar`: captura real de las dos pistas.
//!
//! Es la prueba 1 y 2 de la fase 0 —el riesgo más alto del proyecto— hecha
//! ejecutable: comprobar que el loopback de PipeWire funciona en esta máquina,
//! con esta distribución y con una sesión real de Meet o Zoom.

use anyhow::{bail, Result};
use dictar_audio::wav::EscritorPistas;
use dictar_audio::{AudioFrame, CaptureConfig};
use dictar_domain::Track;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub fn ejecutar(opts: &HashMap<String, String>) -> Result<()> {
    let segundos: u64 = opts
        .get("segundos")
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    let salida = PathBuf::from(
        opts.get("salida")
            .cloned()
            .unwrap_or_else(|| "grabaciones/prueba".to_owned()),
    );

    let solo_mic = opts.contains_key("solo-microfono");
    let cfg = if solo_mic {
        CaptureConfig::solo_microfono()
    } else {
        CaptureConfig::default()
    };

    println!(
        "Pistas:  {}",
        if solo_mic {
            "micrófono"
        } else {
            "micrófono + salida del sistema"
        }
    );
    println!("Destino: {}", salida.display());
    println!("Duración: {segundos} s\n");

    if !solo_mic {
        println!("💡 Pon música o un vídeo para comprobar que se capta la salida.\n");
    }

    let (rx, sesion) = dictar_audio::iniciar(cfg)?;
    let mut escritor = EscritorPistas::nuevo(&salida)?;

    let inicio = Instant::now();
    let limite = Duration::from_secs(segundos);

    let mut niveles: HashMap<Track, (f32, u64)> = HashMap::new();
    let mut ultimo_pintado = Instant::now();

    while inicio.elapsed() < limite {
        // El tiempo de espera acotado permite salir aunque una pista deje de
        // entregar bloques: sin él, un micrófono desconectado colgaría la
        // grabación en lugar de terminarla con lo que hubiera.
        let restante = limite.saturating_sub(inicio.elapsed());
        let Ok(frame) = rx.recv_timeout(restante.min(Duration::from_millis(250))) else {
            continue;
        };

        acumular(&mut niveles, &frame);
        escritor.escribir(&frame)?;

        // Volcar cada pocos segundos: un corte de luz debe costar segundos de
        // audio, no la sesión entera.
        if ultimo_pintado.elapsed() > Duration::from_millis(500) {
            escritor.sincronizar()?;
            pintar(&niveles, inicio.elapsed());
            ultimo_pintado = Instant::now();
        }
    }

    sesion.detener()?;
    println!();

    let rutas = escritor.cerrar()?;
    if rutas.is_empty() {
        bail!("no llegó ni un solo bloque de audio: la captura no funcionó");
    }

    println!("\nArchivos escritos:");
    for r in &rutas {
        let tam = std::fs::metadata(r).map(|m| m.len()).unwrap_or(0);
        println!("  {}  ({} KB)", r.display(), tam / 1024);
    }

    // Un archivo en silencio absoluto casi siempre significa que el flujo se
    // conectó al nodo equivocado. Decirlo aquí ahorra descubrirlo tras grabar
    // una clase de dos horas.
    println!("\nDiagnóstico:");
    let mut alguna_muda = false;
    for pista in [Track::Mic, Track::System] {
        let Some((suma, n)) = niveles.get(&pista) else {
            continue;
        };
        let medio = if *n == 0 { 0.0 } else { suma / *n as f32 };
        let estado = if medio < 0.0001 {
            alguna_muda = true;
            "⚠️  SILENCIO — revisa la fuente"
        } else if medio < 0.01 {
            "🔉 muy bajo"
        } else {
            "✅ señal correcta"
        };
        println!(
            "  {:<8} nivel medio {:.4}  {estado}",
            pista.as_str(),
            medio,
            estado = estado
        );
    }

    if alguna_muda && !solo_mic {
        println!("\n  Si la pista «system» está muda, comprueba que había audio sonando.");
        println!("  Con `wpctl status` puedes ver qué salida es la predeterminada.");
    }

    Ok(())
}

fn acumular(niveles: &mut HashMap<Track, (f32, u64)>, frame: &AudioFrame) {
    let e = niveles.entry(frame.track).or_insert((0.0, 0));
    e.0 += frame.nivel();
    e.1 += 1;
}

fn pintar(niveles: &HashMap<Track, (f32, u64)>, transcurrido: Duration) {
    let barra = |t: Track| -> String {
        let Some((suma, n)) = niveles.get(&t) else {
            return "········".into();
        };
        let medio = if *n == 0 { 0.0 } else { suma / *n as f32 };
        let bloques = ((medio * 40.0).min(8.0)) as usize;
        format!("{}{}", "█".repeat(bloques), "·".repeat(8 - bloques))
    };

    print!(
        "\r  {:>3}s   micro [{}]   sistema [{}]  ",
        transcurrido.as_secs(),
        barra(Track::Mic),
        barra(Track::System)
    );
    let _ = std::io::stdout().flush();
}
