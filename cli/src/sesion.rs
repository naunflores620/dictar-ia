//! Orden `sesion`: el producto entero desde la línea de órdenes.
//!
//! Graba las dos pistas, las transcribe con Whisper y genera los apuntes o el
//! acta. Es lo mismo que hará la interfaz al pulsar un botón, y sirve para
//! validar el flujo completo antes de que el puente con Flutter exista.

use anyhow::{bail, Result};
use dictar_api::eventos::{Evento, FaseProceso};
use dictar_api::{Nucleo, OpcionesGrabacion};
use dictar_domain::{SessionKind, Topic, TopicKind};
use dictar_stt::modelos::Modelo;
use std::collections::HashMap;
use std::io::Write;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub async fn ejecutar(opts: &HashMap<String, String>) -> Result<()> {
    let kind = match opts.get("tipo").map(String::as_str).unwrap_or("clase") {
        "clase" => SessionKind::Class,
        "cliente" => SessionKind::ClientMeeting,
        "equipo" | "reunion" => SessionKind::TeamMeeting,
        otro => bail!("tipo desconocido «{otro}»: usa clase, cliente o equipo"),
    };

    let modelo = match opts.get("modelo").map(String::as_str).unwrap_or("turbo") {
        "small" => Modelo::Small,
        "medium" => Modelo::Medium,
        _ => Modelo::LargeV3Turbo,
    };

    let dir = opts
        .get("datos")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(Nucleo::dir_por_defecto);

    let nucleo = Nucleo::abrir(&dir)?;
    println!("Datos en: {}\n", dir.display());

    // Aviso de sesiones que quedaron a medias por un cierre inesperado. El
    // audio está en disco: nunca se descartan en silencio.
    let colgadas = nucleo.interrumpidas()?;
    if !colgadas.is_empty() {
        println!(
            "⚠️  {} grabación(es) quedaron sin cerrar. El audio está a salvo.\n",
            colgadas.len()
        );
    }

    // -- Asignatura o cliente ------------------------------------------------
    let topic_id = match opts.get("contexto") {
        Some(nombre) => Some(asegurar_topic(&nucleo, nombre, kind)?),
        None => None,
    };

    if let Some(t) = &topic_id {
        let g = nucleo.glosario(t)?;
        if !g.is_empty() {
            // El ciclo virtuoso: lo aprendido en clases anteriores mejora esta.
            println!("Glosario de la asignatura ({} car.): {g}\n", g.len());
        }
    }

    // -- Grabación -----------------------------------------------------------
    let segundos: u64 = opts
        .get("segundos")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let solo_mic = opts.contains_key("solo-microfono");

    if kind == SessionKind::ClientMeeting {
        // Grabar a un tercero sin avisar es un problema legal, no una
        // preferencia de la aplicación.
        println!("⚠️  Vas a grabar a un cliente. Anúncialo en voz alta al empezar.\n");
    }

    let (id, rx) = nucleo.iniciar_grabacion(
        OpcionesGrabacion {
            kind,
            topic_id: topic_id.clone(),
            titulo: opts.get("titulo").cloned(),
            capturar_sistema: !solo_mic,
            capturar_diapositivas: !opts.contains_key("sin-diapositivas"),
            solo_local: opts.contains_key("solo-local"),
        },
        ahora_ms(),
    )?;

    if segundos > 0 {
        println!("● Grabando {segundos} s…\n");
    } else {
        println!("● Grabando. Pulsa ENTER para terminar.\n");
        std::thread::spawn(|| {
            let mut s = String::new();
            let _ = std::io::stdin().read_line(&mut s);
            PARAR.store(true, std::sync::atomic::Ordering::SeqCst);
        });
    }

    let inicio = Instant::now();
    loop {
        if segundos > 0 && inicio.elapsed() >= Duration::from_secs(segundos) {
            break;
        }
        if PARAR.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }

        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Evento::Niveles {
                mic,
                sistema,
                transcurrido_ms,
            }) => pintar_niveles(mic, sistema, transcurrido_ms),
            Ok(Evento::Error { mensaje, .. }) => eprintln!("\n⚠️  {mensaje}"),
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    nucleo.detener_grabacion(ahora_ms())?;
    println!("\n\n■ Grabación detenida.\n");

    // -- Procesado -----------------------------------------------------------
    let (tx_prog, rx_prog) = std::sync::mpsc::channel::<Evento>();
    let pintor = std::thread::spawn(move || {
        while let Ok(e) = rx_prog.recv() {
            match e {
                Evento::Progreso {
                    fase,
                    fraccion,
                    detalle,
                } => {
                    print!(
                        "\r  {:<22} {:>3.0}%  {detalle:<28}",
                        fase.etiqueta(),
                        fraccion * 100.0
                    );
                    let _ = std::io::stdout().flush();
                    if fase == FaseProceso::Guardando {
                        println!();
                    }
                }
                Evento::Frase { texto, .. } => {
                    println!("\r  · {}", recortar(&texto, 68));
                }
                Evento::Error { mensaje, .. } => eprintln!("\n  ⚠️  {mensaje}"),
                _ => {}
            }
        }
    });

    let r = nucleo
        .procesar_sesion(&id, modelo, ahora_ms(), Some(tx_prog))
        .await?;
    let _ = pintor.join();

    println!("\n─────────────────────────────────────────────");
    println!("Frases:    {}", r.frases);
    println!("Avisos:    {}", r.avisos);
    println!("Proveedor: {}", r.proveedor);
    println!("Coste:     ${:.4}", r.coste_usd);

    if let Some(md) = nucleo.notas_markdown(&id)? {
        println!("\n{md}");
    }

    let pendientes = nucleo.pendientes()?;
    if !pendientes.is_empty() {
        println!("\n📌 Pendientes acumulados ({}):", pendientes.len());
        for p in pendientes.iter().take(8) {
            let fecha = p.due_date.as_deref().unwrap_or("—");
            println!("  {fecha}  {}", recortar(&p.text, 62));
        }
    }

    Ok(())
}

static PARAR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Busca la asignatura o cliente por nombre, y la crea si no existe.
fn asegurar_topic(
    nucleo: &Nucleo,
    nombre: &str,
    kind: SessionKind,
) -> Result<dictar_domain::TopicId> {
    // El nombre puede venir como "Cálculo II — Dra. Pérez".
    let (nombre, persona) = match nombre.split_once('—').or_else(|| nombre.split_once(" - ")) {
        Some((n, p)) => (n.trim(), Some(p.trim().to_owned())),
        None => (nombre.trim(), None),
    };

    if let Some(t) = nucleo
        .topics()?
        .into_iter()
        .find(|t| t.name.eq_ignore_ascii_case(nombre))
    {
        return Ok(t.id);
    }

    let tipo = if kind == SessionKind::ClientMeeting {
        TopicKind::Client
    } else {
        TopicKind::Course
    };

    let mut t = Topic::nuevo(tipo, nombre);
    t.person = persona;
    nucleo.crear_topic(&t)?;

    println!("＋ Creada la ficha «{}»", t.name);
    Ok(t.id)
}

fn pintar_niveles(mic: f32, sistema: f32, transcurrido_ms: i64) {
    let barra = |n: f32| {
        let bloques = ((n * 40.0).min(8.0)) as usize;
        format!("{}{}", "█".repeat(bloques), "·".repeat(8 - bloques))
    };

    let s = transcurrido_ms / 1000;
    print!(
        "\r  {:02}:{:02}   tú [{}]   sistema [{}]  ",
        s / 60,
        s % 60,
        barra(mic),
        barra(sistema)
    );
    let _ = std::io::stdout().flush();
}

fn recortar(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let corte: String = s.chars().take(max - 1).collect();
    format!("{corte}…")
}

fn ahora_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_contexto_se_parte_en_asignatura_y_profesor() {
        let dir = tempfile::tempdir().unwrap();
        let n = Nucleo::abrir(dir.path()).unwrap();

        let id = asegurar_topic(&n, "Cálculo II — Dra. Pérez", SessionKind::Class).unwrap();
        let t = n
            .topics()
            .unwrap()
            .into_iter()
            .find(|t| t.id == id)
            .unwrap();

        assert_eq!(t.name, "Cálculo II");
        assert_eq!(t.person.as_deref(), Some("Dra. Pérez"));
        assert_eq!(t.kind, TopicKind::Course);
    }

    #[test]
    fn no_se_duplica_la_ficha_al_grabar_otra_clase_de_la_misma_asignatura() {
        let dir = tempfile::tempdir().unwrap();
        let n = Nucleo::abrir(dir.path()).unwrap();

        let a = asegurar_topic(&n, "Cálculo II", SessionKind::Class).unwrap();
        let b = asegurar_topic(&n, "cálculo ii", SessionKind::Class).unwrap();

        assert_eq!(a, b, "debe reutilizar la ficha existente");
        assert_eq!(n.topics().unwrap().len(), 1);
    }

    #[test]
    fn una_reunion_con_cliente_crea_ficha_de_cliente() {
        let dir = tempfile::tempdir().unwrap();
        let n = Nucleo::abrir(dir.path()).unwrap();

        let id = asegurar_topic(&n, "Acme S.A. - Juan Ruiz", SessionKind::ClientMeeting).unwrap();
        let t = n
            .topics()
            .unwrap()
            .into_iter()
            .find(|t| t.id == id)
            .unwrap();

        assert_eq!(t.kind, TopicKind::Client);
        assert_eq!(t.person.as_deref(), Some("Juan Ruiz"));
    }

    #[test]
    fn el_texto_largo_se_recorta_sin_partir_caracteres() {
        // Cortar por bytes partiría una tilde por la mitad.
        let s = "áéíóú".repeat(30);
        let r = recortar(&s, 10);
        assert_eq!(r.chars().count(), 10);
    }
}
