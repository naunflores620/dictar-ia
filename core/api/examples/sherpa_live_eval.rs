//! Arnés de evaluación de AV-3: nivelación y palabras clave del reconocedor
//! en vivo de Sherpa (español).
//!
//! Cada fila de la tabla de decisión es **un proceso aparte**, lanzado con las
//! variables `DICTAR_SHERPA_DECODIFICACION`, `DICTAR_STT_NIVELAR` y
//! `DICTAR_SHERPA_PALABRAS_CLAVE` de la fila; el reconocedor de Sherpa es único
//! por proceso y su método de decodificación se fija al cargarlo.
//!
//! Modos (Windows):
//!
//! - `repro <system.wav> <mic.wav> <dir> [segundos]`: reproduce las dos pistas a
//!   ritmo real contra `TranscriptorVivo` de producción y escribe `frases.jsonl`,
//!   `stt_metrics.jsonl` y `resumen.json` (carga, cola y edad de D4). El glosario
//!   sale del archivo que apunte `DICTAR_EVAL_GLOSARIO`.
//! - `referencia <wav> <salida.json> <auto|es|en> [segundos]`: Whisper Small sobre
//!   un WAV, con tiempos por frase. Es la referencia de D5.
//! - `explorar <wav> <segundos> <greedy|haz> <ninguna|bpe|bpecrudo|larga|minimo|crudo>
//!   <terminos.txt> [bpe.vocab] [puntuacion]`: el Paso 0 (D3). Alimenta una sola pista sin
//!   esperar al reloj e imprime las frases cerradas y cuántos términos aceptó el
//!   motor. El registro de la DLL sale por stderr: redirigirlo a un archivo.
//! - `generar-bpe <tokens.txt> <bpe.vocab>`: el `bpe.vocab` de la variante (b1).
//! - `puntuar <referencia.json> <H.txt> <E.txt> <etiqueta=dir>...`: tablas de D5.

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("Este harness requiere Windows y Sherpa-ONNX.");
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
    use dictar_api::transcriptor::TranscriptorVivo;
    use dictar_api::Evento;
    use dictar_audio::wav::leer_wav;
    use dictar_domain::{Session, SessionKind, SessionSource, Track};
    use dictar_storage::Db;
    use dictar_stt::modelos::{self, Modelo};
    use dictar_stt::sherpa::{OpcionesSherpa, Segmentacion, SherpaModelo};
    use dictar_stt::whisper::Transcriptor;
    use dictar_stt::SttOpciones;
    use serde_json::{json, Value};
    use std::path::{Path, PathBuf};
    use std::sync::{mpsc, Arc};
    use std::time::{Duration, Instant};

    type Resultado<T> = Result<T, Box<dyn std::error::Error>>;

    const USO: &str = "modos: repro | referencia | explorar | generar-bpe | puntuar; \
                       ver la cabecera del ejemplo";
    const BLOQUE: usize = 1024;
    const FS: u64 = 16_000;

    pub fn principal() -> Resultado<()> {
        let mut args: Vec<String> = std::env::args().skip(1).collect();
        if args.is_empty() {
            return Err(USO.into());
        }
        let modo = args.remove(0);
        match modo.as_str() {
            "repro" => repro(&args),
            "referencia" => referencia(&args),
            "explorar" => explorar(&args),
            "generar-bpe" => generar_bpe(&args),
            "puntuar" => puntuar(&args),
            otro => Err(format!("modo desconocido {otro:?}; {USO}").into()),
        }
    }

    fn argumento<'a>(args: &'a [String], i: usize, nombre: &str) -> Resultado<&'a str> {
        args.get(i)
            .map(String::as_str)
            .ok_or_else(|| format!("falta el argumento {nombre}").into())
    }

    fn segundos_opcionales(args: &[String], i: usize) -> Resultado<Option<usize>> {
        args.get(i)
            .map(|s| s.parse::<usize>())
            .transpose()
            .map_err(|e| format!("segundos no es un entero: {e}").into())
    }

    fn leer_recortado(ruta: &str, segundos: Option<usize>) -> Resultado<Vec<f32>> {
        let (mut pcm, _) = leer_wav(ruta)?;
        if let Some(s) = segundos {
            pcm.truncate(s * FS as usize);
        }
        Ok(pcm)
    }

    fn percentil(valores: &mut [f64], p: f64) -> f64 {
        if valores.is_empty() {
            return 0.0;
        }
        valores.sort_by(|a, b| a.total_cmp(b));
        let indice = ((valores.len() as f64 - 1.0) * p).round() as usize;
        valores[indice]
    }

    // ---------------------------------------------------------------- repro

    fn repro(args: &[String]) -> Resultado<()> {
        let sistema = leer_recortado(
            argumento(args, 0, "system.wav")?,
            segundos_opcionales(args, 3)?,
        )?;
        let mic = leer_recortado(
            argumento(args, 1, "mic.wav")?,
            segundos_opcionales(args, 3)?,
        )?;
        let dir = PathBuf::from(argumento(args, 2, "directorio")?);
        std::fs::create_dir_all(&dir)?;
        let ruta_db = dir.join("bench.db");
        let ruta_metricas = dir.join("stt_metrics.jsonl");
        // Una ejecución previa en el mismo directorio mezclaría sus ventanas con
        // las de esta y falsearía la carga.
        for sobrante in [&ruta_db, &ruta_metricas] {
            if sobrante.exists() {
                std::fs::remove_file(sobrante)?;
            }
        }

        let opciones = OpcionesSherpa::desde_entorno()?;
        let nivelar = std::env::var("DICTAR_STT_NIVELAR").ok();
        let glosario = match std::env::var_os("DICTAR_EVAL_GLOSARIO") {
            Some(ruta) => std::fs::read_to_string(ruta)?.trim().to_owned(),
            None => String::new(),
        };
        let config = json!({
            "decodificacion": if opciones.haz { "haz" } else { "greedy" },
            "palabras_clave": opciones.palabras_clave,
            "puntuacion": opciones.puntuacion,
            "rutas_activas": opciones.rutas_activas,
            "nivelar_variable": nivelar,
            "glosario_caracteres": glosario.chars().count(),
        });
        println!("configuracion efectiva: {config}");

        let db = Db::abrir(&ruta_db)?;
        let sesion = Session::nueva(SessionKind::Class, SessionSource::DesktopLoopback, 0);
        let id = sesion.id.clone();
        db.crear_sesion(&sesion)?;
        drop(db);

        let (tx, rx) = mpsc::channel();
        let inicio = Instant::now();
        let trans = TranscriptorVivo::iniciar(
            id.clone(),
            ruta_db.clone(),
            Modelo::Small,
            glosario,
            Some("es".into()),
            "Profesor".into(),
            tx,
            inicio,
            ruta_metricas.clone(),
        )?;
        let inicio_captura = Instant::now();
        trans.marcar_inicio_captura(inicio_captura);

        let bloques = sistema.len().max(mic.len()).div_ceil(BLOQUE);
        let audio_ms = sistema.len().max(mic.len()) as u64 * 1000 / FS;
        let mut errores = Vec::<String>::new();
        let mut cola: Vec<(u64, usize)> = Vec::with_capacity(bloques);
        for i in 0..bloques {
            let ts = (i * BLOQUE) as u64 * 1000 / FS;
            for (track, pcm) in [(Track::Mic, &mic), (Track::System, &sistema)] {
                if let Some(trozo) = pcm.chunks(BLOQUE).nth(i) {
                    trans.encolar_bloque(dictar_audio::AudioFrame {
                        track,
                        pcm: trozo.to_vec(),
                        timestamp_ms: ts as i64,
                    });
                }
            }
            // La misma cuenta que `pendientes_en_cola` de producción: el trabajo
            // que se está atendiendo no cuenta como cola.
            cola.push((ts, trans.pendientes().saturating_sub(1)));
            while let Ok(evento) = rx.try_recv() {
                if let Evento::Error { mensaje, .. } = evento {
                    errores.push(mensaje);
                }
            }
            let esperado = Duration::from_micros((i as u64 + 1) * BLOQUE as u64 * 1_000_000 / FS);
            if let Some(resto) = esperado.checked_sub(inicio_captura.elapsed()) {
                std::thread::sleep(resto);
            }
        }
        trans.pedir_fin();
        trans.esperar();
        while let Ok(evento) = rx.try_recv() {
            if let Evento::Error { mensaje, .. } = evento {
                errores.push(mensaje);
            }
        }
        let pared_s = inicio_captura.elapsed().as_secs_f64();

        // Frases guardadas.
        let db = Db::abrir(&ruta_db)?;
        let frases = db.transcripcion(&id)?;
        let mut salida = String::new();
        for f in &frases {
            salida.push_str(
                &json!({"pista": f.track.as_str(), "ini": f.start_ms, "fin": f.end_ms,
                        "texto": f.text})
                .to_string(),
            );
            salida.push('\n');
        }
        std::fs::write(dir.join("frases.jsonl"), salida)?;
        let cuenta = |pista: Track| {
            let de_pista = frases.iter().filter(|f| f.track == pista);
            let (mut n, mut p) = (0usize, 0usize);
            for f in de_pista {
                n += 1;
                p += f.text.split_whitespace().count();
            }
            (n, p)
        };
        let (frases_sistema, palabras_sistema) = cuenta(Track::System);
        let (frases_mic, palabras_mic) = cuenta(Track::Mic);

        // Métricas de D4 a partir de las ventanas de 5 s del propio transcriptor.
        let (mut edades, mut edades_max, mut inferencia_max) = (vec![], vec![], vec![]);
        let (mut inferencia_total_ms, mut descartes) = (0.0f64, 0u64);
        for linea in std::fs::read_to_string(&ruta_metricas)
            .unwrap_or_default()
            .lines()
        {
            let Ok(m) = serde_json::from_str::<Value>(linea) else {
                continue;
            };
            let audio = m["audio_ms"].as_f64().unwrap_or(0.0);
            inferencia_total_ms += m["rtf"].as_f64().unwrap_or(0.0) * audio;
            edades.push(m["edad_al_final_inferencia_ms"].as_f64().unwrap_or(0.0));
            edades_max.push(m["edad_audio_max_ms"].as_f64().unwrap_or(0.0));
            inferencia_max.push(m["inferencia_max_ms"].as_f64().unwrap_or(0.0));
            descartes = descartes.max(m["trabajos_descartados_total"].as_u64().unwrap_or(0));
        }
        let ventana_ms = 300_000u64.min(audio_ms / 2);
        let max_cola = |desde: u64, hasta: u64| {
            cola.iter()
                .filter(|(t, _)| *t >= desde && *t < hasta)
                .map(|(_, c)| *c)
                .max()
                .unwrap_or(0)
        };
        let resumen = json!({
            "configuracion": config,
            "audio_s": audio_ms as f64 / 1000.0,
            "pared_s": pared_s,
            "carga_computo": inferencia_total_ms / audio_ms.max(1) as f64,
            "edad_p95_ms": percentil(&mut edades, 0.95),
            "edad_max_p95_ms": percentil(&mut edades_max, 0.95),
            "inferencia_max_p95_ms": percentil(&mut inferencia_max, 0.95),
            "cola_max_primeros": max_cola(0, ventana_ms),
            "cola_max_ultimos": max_cola(audio_ms.saturating_sub(ventana_ms), audio_ms + 1),
            "cola_max_total": cola.iter().map(|(_, c)| *c).max().unwrap_or(0),
            "ventana_cola_s": ventana_ms / 1000,
            "descartes": descartes,
            "errores": errores,
            "frases_sistema": frases_sistema,
            "palabras_sistema": palabras_sistema,
            "frases_mic": frases_mic,
            "palabras_mic": palabras_mic,
        });
        std::fs::write(
            dir.join("resumen.json"),
            serde_json::to_string_pretty(&resumen)?,
        )?;
        println!("{resumen}");
        Ok(())
    }

    // ----------------------------------------------------------- referencia

    fn referencia(args: &[String]) -> Resultado<()> {
        let pcm = leer_recortado(argumento(args, 0, "wav")?, segundos_opcionales(args, 3)?)?;
        let salida = argumento(args, 1, "salida.json")?;
        let idioma = match argumento(args, 2, "idioma")? {
            "auto" => None,
            otro => Some(otro.to_owned()),
        };
        let transcriptor = Transcriptor::cargar(modelos::localizar(Modelo::Small)?)?;
        let opciones = SttOpciones {
            idioma,
            tiempos_por_palabra: false,
            ..SttOpciones::definitiva()
        };
        let inicio = Instant::now();
        let fragmentos = transcriptor.transcribir(&pcm, &opciones)?;
        let lineas: Vec<Value> = fragmentos
            .iter()
            .map(|f| json!({"ini": f.inicio_ms, "fin": f.fin_ms, "texto": f.texto}))
            .collect();
        std::fs::write(salida, serde_json::to_string_pretty(&lineas)?)?;
        println!(
            "referencia Whisper Small: {} fragmentos en {:.1} s ({})",
            lineas.len(),
            inicio.elapsed().as_secs_f64(),
            transcriptor.ruta()
        );
        Ok(())
    }

    // ------------------------------------------------------------- explorar

    fn explorar(args: &[String]) -> Resultado<()> {
        let pcm = leer_recortado(
            argumento(args, 0, "wav")?,
            Some(argumento(args, 1, "segundos")?.parse()?),
        )?;
        let haz = match argumento(args, 2, "decodificacion")? {
            "haz" => true,
            "greedy" => false,
            otro => return Err(format!("decodificacion {otro:?}: use greedy o haz").into()),
        };
        let segmentacion = argumento(args, 3, "segmentacion")?;
        let terminos = std::fs::read_to_string(argumento(args, 4, "terminos.txt")?)?;

        let mut opciones = OpcionesSherpa::produccion();
        opciones.haz = haz;
        opciones.palabras_clave = segmentacion != "ninguna";
        match segmentacion {
            "ninguna" | "larga" | "crudo" => opciones.segmentacion = Segmentacion::PiezaMasLarga,
            "minimo" => opciones.segmentacion = Segmentacion::MinimoDePiezas,
            "bpe" | "bpecrudo" => {
                opciones.segmentacion = Segmentacion::Bpe;
                opciones.vocab_bpe = Some(PathBuf::from(argumento(args, 5, "bpe.vocab")?));
            }
            otro => return Err(format!("segmentacion {otro:?} desconocida").into()),
        }
        if let Some(puntuacion) = args.get(6) {
            opciones.puntuacion = puntuacion.parse()?;
        }
        let carga = Instant::now();
        let modelo = Arc::new(SherpaModelo::cargar(
            &dictar_stt::sherpa::modelo_es()?,
            &opciones,
        )?);
        println!(
            "modelo cargado en {:.1} s; {opciones:?}",
            carga.elapsed().as_secs_f64()
        );

        let mut flujo = match segmentacion {
            "ninguna" => SherpaModelo::nuevo_flujo(&modelo),
            "crudo" | "bpecrudo" => {
                SherpaModelo::nuevo_flujo_con_texto_crudo(&modelo, terminos.trim())
            }
            _ => {
                let preparados = modelo.preparar_terminos(&terminos);
                println!(
                    "terminos aceptados={} rechazados={}",
                    preparados.aceptados.len(),
                    preparados.rechazados.len()
                );
                for aceptado in preparados.aceptados.iter().take(40) {
                    println!("  + {aceptado}");
                }
                for (termino, motivo) in &preparados.rechazados {
                    println!("  - {termino} ({motivo:?})");
                }
                SherpaModelo::nuevo_flujo_con_terminos(&modelo, &preparados)
            }
        };

        let (mut frases, mut palabras) = (0usize, 0usize);
        let inicio = Instant::now();
        let mut desde = 0usize;
        for trozo in pcm.chunks(BLOQUE) {
            let (texto, cerrado) = flujo.aceptar(trozo)?;
            let hasta = desde + trozo.len();
            if cerrado && !texto.trim().is_empty() {
                frases += 1;
                palabras += texto.split_whitespace().count();
                println!("[{:>6.1}s] {}", hasta as f64 / FS as f64, texto.trim());
            }
            desde = hasta;
        }
        let resto = flujo.finalizar()?;
        if !resto.trim().is_empty() {
            frases += 1;
            palabras += resto.split_whitespace().count();
            println!("[final ] {}", resto.trim());
        }
        println!(
            "frases={frases} palabras={palabras} cpu_pared_s={:.1} audio_s={:.1}",
            inicio.elapsed().as_secs_f64(),
            pcm.len() as f64 / FS as f64
        );
        Ok(())
    }

    fn generar_bpe(args: &[String]) -> Resultado<()> {
        let tokens = std::fs::read_to_string(argumento(args, 0, "tokens.txt")?)?;
        let mut vocab = String::new();
        for linea in tokens.lines() {
            let Some((pieza, id)) = linea.rsplit_once(' ') else {
                continue;
            };
            let id: f32 = id.parse()?;
            // Las piezas especiales no son texto y confundirían al codificador.
            if pieza.starts_with('<') || pieza.starts_with('#') {
                continue;
            }
            // Sin las puntuaciones del modelo de piezas original, el orden de
            // los identificadores es lo único que se conoce: las primeras suelen
            // ser las más frecuentes.
            vocab.push_str(&format!("{pieza} {}\n", -id));
        }
        std::fs::write(argumento(args, 1, "bpe.vocab")?, vocab)?;
        Ok(())
    }

    // -------------------------------------------------------------- puntuar

    struct Frase {
        ini: i64,
        fin: i64,
        texto: String,
        palabras: Vec<String>,
    }

    fn palabras_de(texto: &str) -> Vec<String> {
        texto
            .split(|c: char| !c.is_alphanumeric())
            .filter(|p| !p.is_empty())
            .map(str::to_lowercase)
            .collect()
    }

    fn contiene(frase: &[String], termino: &[String]) -> bool {
        !termino.is_empty() && frase.windows(termino.len()).any(|w| w == termino)
    }

    fn solapa(a: &Frase, b: &Frase, margen_ms: i64) -> bool {
        a.fin + margen_ms >= b.ini && a.ini <= b.fin + margen_ms
    }

    fn leer_frases(ruta: &Path, pista: Option<&str>) -> Resultado<Vec<Frase>> {
        let contenido = std::fs::read_to_string(ruta)?;
        let mut frases = Vec::new();
        for linea in contenido.lines().filter(|l| !l.trim().is_empty()) {
            let v: Value = serde_json::from_str(linea)?;
            if pista.is_some_and(|p| v["pista"].as_str() != Some(p)) {
                continue;
            }
            let texto = v["texto"].as_str().unwrap_or_default().to_owned();
            frases.push(Frase {
                ini: v["ini"].as_i64().unwrap_or(0),
                fin: v["fin"].as_i64().unwrap_or(0),
                palabras: palabras_de(&texto),
                texto,
            });
        }
        Ok(frases)
    }

    fn leer_referencia(ruta: &Path) -> Resultado<Vec<Frase>> {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(ruta)?)?;
        let mut frases = Vec::new();
        for f in v.as_array().ok_or("la referencia no es una lista")? {
            let texto = f["texto"].as_str().unwrap_or_default().to_owned();
            frases.push(Frase {
                ini: f["ini"].as_i64().unwrap_or(0),
                fin: f["fin"].as_i64().unwrap_or(0),
                palabras: palabras_de(&texto),
                texto,
            });
        }
        Ok(frases)
    }

    fn leer_terminos(ruta: &str) -> Resultado<Vec<Vec<String>>> {
        Ok(std::fs::read_to_string(ruta)?
            .lines()
            .map(palabras_de)
            .filter(|t| !t.is_empty())
            .collect())
    }

    /// (apariciones en la referencia, apariciones que Sherpa también tiene a ±5 s)
    fn aciertos(
        referencia: &[Frase],
        sherpa: &[Frase],
        terminos: &[Vec<String>],
    ) -> (usize, usize) {
        let (mut apariciones, mut aciertos) = (0, 0);
        for termino in terminos {
            for r in referencia.iter().filter(|r| contiene(&r.palabras, termino)) {
                apariciones += 1;
                if sherpa
                    .iter()
                    .any(|s| solapa(s, r, 5_000) && contiene(&s.palabras, termino))
                {
                    aciertos += 1;
                }
            }
        }
        (apariciones, aciertos)
    }

    /// Veces que Sherpa dice un término que Whisper no dice a ±5 s.
    fn inserciones(referencia: &[Frase], sherpa: &[Frase], terminos: &[Vec<String>]) -> usize {
        let mut total = 0;
        for termino in terminos {
            for s in sherpa.iter().filter(|s| contiene(&s.palabras, termino)) {
                let respaldada = referencia
                    .iter()
                    .any(|r| solapa(s, r, 5_000) && contiene(&r.palabras, termino));
                if !respaldada {
                    total += 1;
                }
            }
        }
        total
    }

    /// Frases del micrófono que repiten texto de la pista de sistema a ±3 s.
    fn duplicados(mic: &[Frase], sistema: &[Frase]) -> usize {
        mic.iter()
            .filter(|m| m.palabras.len() >= 3)
            .filter(|m| {
                sistema.iter().any(|s| {
                    solapa(m, s, 3_000) && {
                        let comunes = m.palabras.iter().filter(|p| s.palabras.contains(p)).count();
                        comunes as f64 >= 0.6 * m.palabras.len() as f64
                    }
                })
            })
            .count()
    }

    fn puntuar(args: &[String]) -> Resultado<()> {
        let referencia = leer_referencia(Path::new(argumento(args, 0, "referencia.json")?))?;
        let h = leer_terminos(argumento(args, 1, "H.txt")?)?;
        let e = leer_terminos(argumento(args, 2, "E.txt")?)?;
        let he: Vec<Vec<String>> = h.iter().chain(e.iter()).cloned().collect();
        let corridas: Vec<(String, PathBuf)> = args[3..]
            .iter()
            .map(|a| {
                a.split_once('=')
                    .map(|(etiqueta, dir)| (etiqueta.to_owned(), PathBuf::from(dir)))
                    .ok_or_else(|| format!("{a:?} no es etiqueta=directorio"))
            })
            .collect::<Result<_, _>>()?;

        println!(
            "| fila | frases | palabras | H aciertos | E aciertos | H inserciones | duplicados | \
             carga | cola ini/fin | edad p95 ms | descartes |"
        );
        println!("|---|---|---|---|---|---|---|---|---|---|---|");
        for (etiqueta, dir) in &corridas {
            let sistema = leer_frases(&dir.join("frases.jsonl"), Some("system"))?;
            let mic = leer_frases(&dir.join("frases.jsonl"), Some("mic"))?;
            let resumen: Value =
                serde_json::from_str(&std::fs::read_to_string(dir.join("resumen.json"))?)?;
            let (h_ref, h_ok) = aciertos(&referencia, &sistema, &h);
            let (e_ref, e_ok) = aciertos(&referencia, &sistema, &e);
            let (_, he_ok) = aciertos(&referencia, &sistema, &he);
            println!(
                "| {etiqueta} | {} | {} | {h_ok}/{h_ref} | {e_ok}/{e_ref} | {} | {} | {:.3} | \
                 {}/{} | {:.0} | {} |",
                sistema.len(),
                sistema.iter().map(|f| f.palabras.len()).sum::<usize>(),
                inserciones(&referencia, &sistema, &h),
                duplicados(&mic, &sistema),
                resumen["carga_computo"].as_f64().unwrap_or(0.0),
                resumen["cola_max_primeros"],
                resumen["cola_max_ultimos"],
                resumen["edad_p95_ms"].as_f64().unwrap_or(0.0),
                resumen["descartes"],
            );
            println!("<!-- {etiqueta}: aciertos H u E = {he_ok} -->");
        }

        // Tres fragmentos de 30 s, A frente a D, centrados en apariciones de H.
        let (Some(a), Some(d)) = (
            corridas
                .iter()
                .find(|(l, _)| l.starts_with('A') && !l.contains('\'')),
            corridas.iter().find(|(l, _)| l.starts_with('D')),
        ) else {
            return Ok(());
        };
        let sistema_a = leer_frases(&a.1.join("frases.jsonl"), Some("system"))?;
        let sistema_d = leer_frases(&d.1.join("frases.jsonl"), Some("system"))?;
        let mut inicios: Vec<i64> = Vec::new();
        for r in &referencia {
            if h.iter().any(|t| contiene(&r.palabras, t))
                && inicios.iter().all(|i| (r.ini - i).abs() >= 60_000)
            {
                inicios.push(r.ini);
            }
            if inicios.len() == 3 {
                break;
            }
        }
        for ini in inicios {
            let ventana = |frases: &[Frase]| {
                frases
                    .iter()
                    .filter(|f| f.fin >= ini && f.ini <= ini + 30_000)
                    .map(|f| f.texto.trim())
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            println!("\n### Fragmento {} s - {} s", ini / 1000, ini / 1000 + 30);
            println!("- Whisper: {}", ventana(&referencia));
            println!("- {}: {}", a.0, ventana(&sistema_a));
            println!("- {}: {}", d.0, ventana(&sistema_d));
        }
        Ok(())
    }
}
