//! Arnés de medición de DIA-1 (A7): diarización sobre audio real.
//!
//! Siempre ejercita el motor y las funciones de asignación de producción
//! (`dictar_stt::diarizacion`); aquí solo hay lectura de archivos, métricas y
//! presentación.
//!
//! Modos (Windows):
//!
//! - `sesion <dir_audio> <dictar.db> <id_sesion> [umbral]`: A7.1, A7.4 y A7.5.
//!   Diariza cada pista como lo haría el cierre de la sesión (el micrófono con
//!   el retorno atenuado si se detecta), asigna las frases guardadas en la base
//!   y muestra clústeres, etiquetas y deriva de tiempos. Trabajar sobre una
//!   **copia** de la base: no se escribe nada, pero tampoco se arriesga la real.
//! - `mic-sin-atenuar <dir_audio> <dictar.db> <id_sesion> [umbral]`: lo mismo para
//!   el micrófono sin atenuar, que es el control de A7.5.
//! - `integrada <dir_datos> <id_sesion>`: A7.5 por el camino de producción.
//!   Abre un `Nucleo` sobre una **copia** de los datos (con la sesión ya marcada
//!   como reunión), ejecuta `diarizar_participantes` y muestra quién queda con
//!   cuántas frases. Es la medición que cuenta: aplica el mismo recorte de eco,
//!   atenuación y descarte de voces residuales que el cierre real.
//! - `mezcla <a.wav> <b.wav> [umbral] [fragmento_s] [fragmentos_por_voz] [salto_b_s]`: A7.2.
//!   Intercala fragmentos con voz de dos grabaciones de profesores distintos y
//!   mide qué parte del tiempo hablado de cada uno cae en su clúster.
//! - `larga <wav> [horas] [umbral]`: A7.3. Repite el audio hasta la duración
//!   pedida y mide RTF, pico de memoria y núcleos efectivos.

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("Este arnés requiere Windows y Sherpa-ONNX.");
    std::process::exit(2);
}

#[cfg(target_os = "windows")]
fn main() {
    if let Err(error) = arnes::principal() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "windows")]
mod arnes {
    use dictar_audio::wav::leer_wav;
    use dictar_domain::{SessionId, Track};
    use dictar_storage::Db;
    use dictar_stt::diarizacion::{
        etiquetar_frases, rutas_modelos, Diarizador, OpcionesDiarizacion, SegmentoHablante,
    };
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::time::Instant;

    type Resultado<T> = Result<T, Box<dyn std::error::Error>>;

    const USO: &str = "modos: sesion | mic-sin-atenuar | mezcla | larga; ver la cabecera";
    const FS: usize = 16_000;

    #[repr(C)]
    struct ContadoresMemoria {
        cb: u32,
        fallos_de_pagina: u32,
        pico_conjunto_de_trabajo: usize,
        conjunto_de_trabajo: usize,
        resto: [usize; 6],
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(
            proceso: isize,
            contadores: *mut ContadoresMemoria,
            cb: u32,
        ) -> i32;
        fn GetProcessTimes(
            proceso: isize,
            creacion: *mut u64,
            salida: *mut u64,
            nucleo: *mut u64,
            usuario: *mut u64,
        ) -> i32;
    }

    /// Pico del conjunto de trabajo del proceso, en MiB.
    fn pico_ram_mib() -> f64 {
        let mut c = ContadoresMemoria {
            cb: std::mem::size_of::<ContadoresMemoria>() as u32,
            fallos_de_pagina: 0,
            pico_conjunto_de_trabajo: 0,
            conjunto_de_trabajo: 0,
            resto: [0; 6],
        };
        // SAFETY: `c` es un PROCESS_MEMORY_COUNTERS válido y `cb` su tamaño.
        let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
        if ok == 0 {
            return f64::NAN;
        }
        c.pico_conjunto_de_trabajo as f64 / (1024.0 * 1024.0)
    }

    /// Tiempo de CPU consumido por el proceso (núcleo + usuario), en segundos.
    fn cpu_s() -> f64 {
        let (mut a, mut b, mut nucleo, mut usuario) = (0u64, 0u64, 0u64, 0u64);
        // SAFETY: cuatro FILETIME (64 bits) válidos para escritura.
        let ok = unsafe {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut a,
                &mut b,
                &mut nucleo,
                &mut usuario,
            )
        };
        if ok == 0 {
            return f64::NAN;
        }
        (nucleo + usuario) as f64 / 1e7
    }

    pub fn principal() -> Resultado<()> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let modo = args.first().map(String::as_str).ok_or(USO)?;
        match modo {
            "sesion" => sesion(&args[1..], true),
            "mic-sin-atenuar" => sesion(&args[1..], false),
            "integrada" => integrada(&args[1..]),
            "mezcla" => mezcla(&args[1..]),
            "larga" => larga(&args[1..]),
            _ => Err(USO.into()),
        }
    }

    /// Producción, con el umbral de la línea de órdenes y, solo para medir,
    /// `DICTAR_DIA_HILOS` y `DICTAR_DIA_VENTANA` por encima.
    fn opciones(umbral: Option<&String>) -> Resultado<OpcionesDiarizacion> {
        let mut o = OpcionesDiarizacion::produccion();
        if let Some(v) = umbral {
            o.umbral = v.parse()?;
        }
        if let Ok(v) = std::env::var("DICTAR_DIA_HILOS") {
            o.hilos = v.parse()?;
        }
        if let Ok(v) = std::env::var("DICTAR_DIA_VENTANA") {
            o.desplazamiento_ventana = v.parse()?;
        }
        Ok(o)
    }

    fn cargar(o: OpcionesDiarizacion) -> Resultado<(Diarizador, i32)> {
        let rutas = rutas_modelos().ok_or("no se encontraron los modelos de diarización")?;
        let t = Instant::now();
        let d = Diarizador::cargar(&rutas, &o)?;
        println!(
            "modelos: {} + {}
{o:?}, carga {:.2} s",
            rutas.segmentacion.display(),
            rutas.embedding.display(),
            t.elapsed().as_secs_f64()
        );
        Ok((d, o.hilos))
    }

    /// Diariza y devuelve (segmentos, segundos de reloj).
    fn medir(d: &Diarizador, pcm: &[f32]) -> Resultado<(Vec<SegmentoHablante>, f64)> {
        let t = Instant::now();
        let segmentos = d.diarizar(pcm)?;
        Ok((segmentos, t.elapsed().as_secs_f64()))
    }

    fn tiempo_por_cluster(segmentos: &[SegmentoHablante]) -> BTreeMap<usize, i64> {
        let mut m = BTreeMap::new();
        for s in segmentos {
            *m.entry(s.hablante).or_default() += s.fin_ms - s.inicio_ms;
        }
        m
    }

    fn sesion(args: &[String], atenuar: bool) -> Resultado<()> {
        let [dir, db, id, ..] = args else {
            return Err("sesion <dir_audio> <dictar.db> <id_sesion> [umbral]".into());
        };
        let (d, _) = cargar(opciones(args.get(3))?)?;
        let db = Db::abrir(db)?;
        let frases = db.transcripcion(&SessionId::from(id.as_str()))?;
        let dir = Path::new(dir);

        let mut habla_del_sistema: Vec<SegmentoHablante> = Vec::new();
        for (pista, defecto) in [(Track::System, "Cliente"), (Track::Mic, "Yo")] {
            let ruta = dir.join(format!("{}.wav", pista.as_str()));
            if !ruta.is_file() {
                println!("\n== {}: sin WAV", pista.as_str());
                continue;
            }
            let (mut pcm, _) = leer_wav(&ruta)?;
            let duracion_ms = (pcm.len() * 1000 / FS) as i64;
            let mut nota = String::new();
            if pista == Track::Mic && atenuar {
                let sistema = dir.join("system.wav");
                if sistema.is_file() {
                    if let Some(r) = dictar_audio::detectar_retorno_en_wav(&ruta, &sistema)? {
                        r.atenuar_microfono(&mut pcm);
                        nota = format!(
                            "retorno detectado (correlación {:.3}, retardo {} ms): atenuado",
                            r.correlacion, r.retardo_ms
                        );
                    } else {
                        nota = "sin retorno detectado: se diariza tal cual".to_owned();
                    }
                }
            } else if pista == Track::Mic {
                nota = "SIN atenuar (control)".to_owned();
            }
            let (segmentos, s) = medir(&d, &pcm)?;
            println!(
                "\n== {} ({:.1} min) {nota}\ndiarización {s:.1} s, RTF {:.4}, {} segmentos",
                pista.as_str(),
                duracion_ms as f64 / 60_000.0,
                s / (duracion_ms as f64 / 1000.0),
                segmentos.len()
            );
            for (h, ms) in tiempo_por_cluster(&segmentos) {
                let mut x = format!("  clúster {h}: {:.1} s de habla", ms as f64 / 1000.0);
                if pista == Track::Mic && ms >= 3_000 {
                    // Qué parte de esta voz suena a la vez que alguien en el
                    // sistema: un eco de los altavoces sería casi toda.
                    let propio: i64 = segmentos
                        .iter()
                        .filter(|m| m.hablante == h)
                        .map(|m| {
                            habla_del_sistema
                                .iter()
                                .map(|r| {
                                    (m.fin_ms.min(r.fin_ms + 500) - m.inicio_ms.max(r.inicio_ms))
                                        .max(0)
                                })
                                .sum::<i64>()
                                .min(m.fin_ms - m.inicio_ms)
                        })
                        .sum();
                    x += &format!(
                        ", {:.0} % a la vez que el sistema",
                        propio as f64 * 100.0 / ms as f64
                    );
                }
                println!("{x}");
            }
            if pista == Track::System {
                habla_del_sistema = segmentos.clone();
            }

            let propias: Vec<_> = frases.iter().filter(|u| u.track == pista).collect();
            let tramos: Vec<_> = propias.iter().map(|u| (u.start_ms, u.end_ms)).collect();
            let et = etiquetar_frases(pista, &tramos, &segmentos, defecto);
            let mut por_etiqueta: BTreeMap<&str, (usize, i64)> = BTreeMap::new();
            for (u, e) in propias.iter().zip(&et.etiquetas) {
                let x = por_etiqueta.entry(e).or_default();
                x.0 += 1;
                x.1 += u.end_ms - u.start_ms;
            }
            println!(
                "frases {}; voces con frases {}; clústeres descartados {:?}",
                propias.len(),
                et.voces,
                et.descartados
            );
            for (e, (n, ms)) in &por_etiqueta {
                println!("  {e}: {n} frases, {:.1} s", *ms as f64 / 1000.0);
            }
            let fin_frases = propias.iter().map(|u| u.end_ms).max().unwrap_or(0);
            let fin_segmentos = segmentos.iter().map(|s| s.fin_ms).max().unwrap_or(0);
            println!(
                "deriva: WAV {duracion_ms} ms; última frase termina en {fin_frases} ms \
                 ({:+} ms); último segmento de diarización en {fin_segmentos} ms ({:+} ms)",
                fin_frases - duracion_ms,
                fin_segmentos - duracion_ms
            );
            // Muestra para revisar a oído lo que el número no dice.
            for e in por_etiqueta.keys() {
                let ejemplos: Vec<String> = propias
                    .iter()
                    .zip(&et.etiquetas)
                    .filter(|(_, x)| x.as_str() == *e)
                    .take(3)
                    .map(|(u, _)| format!("[{}s] {}", u.start_ms / 1000, u.text.trim()))
                    .collect();
                println!("  ejemplos de {e}: {}", ejemplos.join(" | "));
            }
        }
        Ok(())
    }

    fn integrada(args: &[String]) -> Resultado<()> {
        let [datos, id] = args else {
            return Err("integrada <dir_datos> <id_sesion>".into());
        };
        let nucleo = dictar_api::Nucleo::abrir(datos)?;
        let id = SessionId::from(id.as_str());
        let antes = nucleo.transcripcion(&id)?.len();
        let t = Instant::now();
        let resultado = nucleo.diarizar_participantes(&id, None)?;
        println!(
            "resultado: {resultado:?} en {:.1} s; frases antes {antes}",
            t.elapsed().as_secs_f64()
        );
        let mut por_voz: BTreeMap<(String, String), (usize, i64)> = BTreeMap::new();
        for u in nucleo.transcripcion(&id)? {
            let x = por_voz
                .entry((u.track.as_str().to_owned(), u.speaker.unwrap_or_default()))
                .or_default();
            x.0 += 1;
            x.1 += u.end_ms - u.start_ms;
        }
        for ((pista, voz), (n, ms)) in &por_voz {
            println!(
                "  {pista:<6} {voz:<20} {n:>4} frases {:>8.1} s",
                *ms as f64 / 1000.0
            );
        }
        Ok(())
    }

    /// Fragmentos de `largo_s` segundos con voz sostenida, separados entre sí.
    fn fragmentos_con_voz(pcm: &[f32], largo_s: usize, cuantos: usize) -> Vec<(usize, usize)> {
        let largo = largo_s * FS;
        let mut elegidos = Vec::new();
        let mut inicio = 0;
        while inicio + largo <= pcm.len() && elegidos.len() < cuantos {
            let bloques: Vec<f32> = pcm[inicio..inicio + largo]
                .chunks(FS / 50)
                .map(|b| (b.iter().map(|m| m * m).sum::<f32>() / b.len() as f32).sqrt())
                .collect();
            let activos = bloques.iter().filter(|r| **r > 0.01).count();
            if activos * 100 >= bloques.len() * 60 {
                elegidos.push((inicio, inicio + largo));
                // Salto de un fragmento para no repetir casi el mismo audio.
                inicio += largo * 2;
            } else {
                inicio += largo / 2;
            }
        }
        elegidos
    }

    fn mezcla(args: &[String]) -> Resultado<()> {
        let [a, b, ..] = args else {
            return Err(
                "mezcla <a.wav> <b.wav> [umbral] [fragmento_s] [fragmentos] [salto_b_s]".into(),
            );
        };
        let largo_s: usize = args.get(3).map_or(Ok(30), |v| v.parse())?;
        let cuantos: usize = args.get(4).map_or(Ok(10), |v| v.parse())?;
        let (d, _) = cargar(opciones(args.get(2))?)?;
        let (pa, _) = leer_wav(a)?;
        let (pb, _) = leer_wav(b)?;
        // Saltar el principio de B permite mezclar dos trozos de la MISMA
        // grabación: tienen que quedar en un único clúster.
        let salto_b: usize = args.get(5).map_or(Ok(0), |v| v.parse())?;
        let pb = pb[(salto_b * FS).min(pb.len())..].to_vec();
        let fa = fragmentos_con_voz(&pa, largo_s, cuantos);
        let fb = fragmentos_con_voz(&pb, largo_s, cuantos);
        let n = fa.len().min(fb.len());
        if n == 0 {
            return Err("no hay fragmentos con voz suficiente".into());
        }

        let mut mezcla: Vec<f32> = Vec::new();
        // Verdad: (origen, inicio_ms, fin_ms) en la línea de tiempo de la mezcla.
        let mut verdad: Vec<(usize, i64, i64)> = Vec::new();
        for i in 0..n {
            for (origen, pcm, (ini, fin)) in [(0, &pa, fa[i]), (1, &pb, fb[i])] {
                let desde = (mezcla.len() * 1000 / FS) as i64;
                mezcla.extend_from_slice(&pcm[ini..fin]);
                verdad.push((origen, desde, (mezcla.len() * 1000 / FS) as i64));
            }
        }
        println!(
            "mezcla: {n} fragmentos de {largo_s} s por voz ({:.1} min)",
            mezcla.len() as f64 / FS as f64 / 60.0
        );
        let (segmentos, s) = medir(&d, &mezcla)?;
        println!(
            "diarización {s:.1} s; clústeres: {:?}",
            tiempo_por_cluster(&segmentos)
        );

        // solape[origen][cluster] = ms de habla del clúster dentro de los
        // fragmentos de ese origen.
        let mut solape: [BTreeMap<usize, i64>; 2] = Default::default();
        for (origen, ini, fin) in &verdad {
            for sg in &segmentos {
                let comun = fin.min(&sg.fin_ms) - ini.max(&sg.inicio_ms);
                if comun > 0 {
                    *solape[*origen].entry(sg.hablante).or_default() += comun;
                }
            }
        }
        for origen in 0..2 {
            let total: i64 = solape[origen].values().sum();
            // El clúster que mejor representa a este origen es el de mayor solape.
            let (mejor, ms) = solape[origen]
                .iter()
                .max_by_key(|(_, v)| **v)
                .map(|(h, v)| (*h, *v))
                .unwrap_or((usize::MAX, 0));
            println!(
                "voz {}: mejor clúster {mejor}, {:.1} % de su habla ({:.1} s de {:.1} s); \
                 reparto {:?}",
                ['A', 'B'][origen],
                ms as f64 * 100.0 / total.max(1) as f64,
                ms as f64 / 1000.0,
                total as f64 / 1000.0,
                solape[origen]
            );
        }
        Ok(())
    }

    fn larga(args: &[String]) -> Resultado<()> {
        let [wav, ..] = args else {
            return Err("larga <wav> [horas] [umbral]".into());
        };
        let horas: f64 = args.get(1).map_or(Ok(1.0), |v| v.parse())?;
        let (d, hilos) = cargar(opciones(args.get(2))?)?;
        let (base, _) = leer_wav(wav)?;
        let objetivo = (horas * 3600.0 * FS as f64) as usize;
        let mut pcm = Vec::with_capacity(objetivo);
        while pcm.len() < objetivo {
            let falta = (objetivo - pcm.len()).min(base.len());
            pcm.extend_from_slice(&base[..falta]);
        }
        println!(
            "pista sintética de {:.2} h con {} repeticiones",
            horas,
            objetivo / base.len() + 1
        );

        let cpu0 = cpu_s();
        let ram_antes = pico_ram_mib();
        let (segmentos, s) = medir(&d, &pcm)?;
        let cpu = cpu_s() - cpu0;
        println!(
            "diarización {s:.1} s => RTF {:.4}; {} segmentos, {} clústeres\n\
             CPU {cpu:.1} s => {:.2} núcleos efectivos (hilos configurados {hilos})\n\
             pico de RAM del proceso {:.0} MiB (antes de diarizar {ram_antes:.0} MiB)",
            s / (objetivo as f64 / FS as f64),
            segmentos.len(),
            tiempo_por_cluster(&segmentos).len(),
            cpu / s,
            pico_ram_mib()
        );
        Ok(())
    }
}
