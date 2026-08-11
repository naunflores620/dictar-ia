//! CLI de `dictar_ia`.
//!
//! Sirve para validar el pipeline de notas contra proveedores reales sin
//! depender todavía de la captura de audio ni de Whisper: le das una
//! transcripción en texto y te devuelve los apuntes o el acta.
//!
//! Es la prueba 5 de la fase 0, y además el banco de pruebas para afinar los
//! prompts, que es el riesgo que decide si el producto sirve.

mod grabar;
mod sesion;
mod transcribir;
mod transcripcion;

use anyhow::{bail, Context, Result};
use dictar_domain::{SessionId, SessionKind};
use dictar_notes::{markdown, NotesGenerator, SessionContext};
use dictar_providers::config::ProvidersFile;
use dictar_providers::router::{resolver_por_defecto, KeyResolver, Router};
use std::collections::HashMap;

const AYUDA: &str = "\
dictar — apuntes y actas a partir de transcripciones

USO
  dictar sesion [--tipo clase|cliente|equipo] [--contexto \"...\"] [--segundos N]
  dictar grabar [--segundos N] [--salida <dir>] [--solo-microfono]
  dictar transcribir --in <archivo.wav> [--modelo turbo|medium|small]
  dictar modelos [--descargar turbo]
  dictar notas --in <archivo> [opciones]
  dictar proveedores
  dictar ayuda

OPCIONES de `sesion`   ← el flujo completo: graba, transcribe y resume
  --tipo <t>            clase | cliente | equipo        [por defecto: clase]
  --contexto <texto>    \"Cálculo II — Dra. Pérez\". Crea la ficha si no existe
  --segundos <n>        Duración; sin esto, para con ENTER
  --modelo <m>          turbo | medium | small          [por defecto: turbo]
  --solo-microfono      Reunión presencial: no capturar la salida del sistema
  --solo-local          Confidencial: no se envía a ningún proveedor externo
  --datos <dir>         Carpeta de datos alternativa

OPCIONES de `grabar`
  --segundos <n>        Duración de la captura           [por defecto: 10]
  --salida <dir>        Carpeta de destino    [por defecto: grabaciones/prueba]
  --solo-microfono      No capturar la salida del sistema (reunión presencial)

OPCIONES de `transcribir`
  --in <archivo.wav>    Audio a 16 kHz mono, tal como lo deja `grabar`
  --modelo <m>          turbo | medium | small          [por defecto: turbo]
  --glosario <lista>    Términos de la asignatura, para el initial_prompt
  --idioma <cod>        Código ISO                          [por defecto: es]

OPCIONES de `notas`
  --in <archivo>        Transcripción en texto plano (obligatorio)
  --tipo <t>            clase | cliente | equipo        [por defecto: clase]
  --contexto <texto>    \"Cálculo II — Dra. Pérez\" o \"Acme S.A.\"
  --glosario <lista>    Términos separados por comas
  --duracion <min>      Duración de la sesión en minutos
  --fecha <ISO>         Fecha de la sesión           [por defecto: hoy]
  --config <archivo>    providers.toml alternativo
  --json                Imprime el JSON en bruto en vez de Markdown
  --out <archivo>       Escribe el resultado en un archivo

FORMATO DE LA TRANSCRIPCIÓN
  Una línea por intervención. Los tres formatos valen:
    [123456] Profesor: la transformada de Laplace...
    [01:02:03] Profesor: ...
    Profesor: ...
  Sin marca de tiempo se generan tiempos sintéticos, que mantienen el orden.
  El hablante «Yo» se asigna a tu pista; el resto, al interlocutor.

CLAVES DE API
  Se buscan en este orden:
    1. Variables de entorno
    2. Archivo .env (en el directorio actual, el padre, o ~/.config/dictar_ia/)

  Nombres:  GEMINI_API_KEY, DEEPSEEK_API_KEY, OPENAI_API_KEY

  Para empezar:  cp .env.example .env   y rellena las que uses.
  `dictar proveedores` te dice de dónde ha cogido cada clave.

  Sin ninguna clave, se usa Ollama en local (http://localhost:11434).

EJEMPLO
  dictar notas --in clase.txt --tipo clase \\
      --contexto \"Cálculo II — Dra. Pérez\" --duracion 90 --out apuntes.md
";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dictar=info,warn".into()),
        )
        .with_target(false)
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("notas") => notas(&args[1..]).await,
        Some("proveedores") => proveedores(&args[1..]),
        Some("grabar") => grabar::ejecutar(&opciones(&args[1..])),
        Some("sesion") => sesion::ejecutar(&opciones(&args[1..])).await,
        Some("transcribir") => transcribir::ejecutar(&opciones(&args[1..])),
        Some("modelos") => transcribir::descargar(&opciones(&args[1..])),
        Some("ayuda") | Some("--help") | Some("-h") | None => {
            print!("{AYUDA}");
            Ok(())
        }
        Some(otro) => {
            eprintln!("orden desconocida: «{otro}»\n");
            print!("{AYUDA}");
            std::process::exit(2);
        }
    }
}

/// Analizador de argumentos mínimo: `--clave valor`.
fn opciones(args: &[String]) -> HashMap<String, String> {
    let mut m = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        if let Some(clave) = args[i].strip_prefix("--") {
            let valor = args
                .get(i + 1)
                .filter(|v| !v.starts_with("--"))
                .cloned()
                .unwrap_or_else(|| "true".to_owned());
            let consumidos = if valor == "true" { 1 } else { 2 };
            m.insert(clave.to_owned(), valor);
            i += consumidos;
        } else {
            i += 1;
        }
    }
    m
}

fn cargar_config(ruta: Option<&String>) -> Result<ProvidersFile> {
    match ruta {
        Some(r) => {
            let texto = std::fs::read_to_string(r)
                .with_context(|| format!("no se pudo leer la configuración «{r}»"))?;
            Ok(ProvidersFile::desde_toml(&texto)?)
        }
        None => Ok(ProvidersFile::desde_toml(
            dictar_providers::config::PLANTILLA_TOML,
        )?),
    }
}

fn proveedores(args: &[String]) -> Result<()> {
    let opts = opciones(args);
    let cfg = cargar_config(opts.get("config"))?;
    let resolver = resolver_por_defecto();

    // Decir dónde se está buscando evita la pérdida de tiempo más habitual:
    // la clave existe, pero en un archivo que la aplicación no mira.
    println!("Buscando claves en:");
    println!("  1. variables de entorno");
    for ruta in dictar_providers::secretos::rutas_dotenv() {
        let marca = if ruta.is_file() { "←" } else { " " };
        println!("  2. {} {marca}", ruta.display());
    }

    println!("\nProveedores configurados:\n");
    for p in &cfg.providers {
        let hallazgo = p
            .api_key_ref
            .as_deref()
            .and_then(|r| resolver.resolver_con_origen(r));

        let estado = match (&hallazgo, p.es_local()) {
            (Some((_, origen)), _) => format!("✅ clave desde {origen}"),
            (None, true) => "✅ local (no necesita clave)".to_owned(),
            (None, false) => "⚠️  sin clave — se omitirá".to_owned(),
        };

        println!(
            "  {:<14} {:<22} {:<10} {}",
            p.id,
            p.model,
            format!("{:?}", p.structured).to_lowercase(),
            estado
        );
    }

    println!("\nCadenas de respaldo:");
    for (tarea, cadena) in &cfg.routing.cadenas {
        println!("  {tarea:<16} {}", cadena.join(" → "));
    }

    // Si no hay ninguna clave, todo cae al respaldo local: conviene decirlo.
    let utilizables = cfg
        .providers
        .iter()
        .filter(|p| {
            p.es_local()
                || p.api_key_ref
                    .as_deref()
                    .and_then(|r| resolver.resolver(r))
                    .is_some()
        })
        .count();

    if utilizables == 0 {
        println!("\n⚠️  Ningún proveedor utilizable.");
        println!("   Copia .env.example a .env y pon al menos una clave, o levanta Ollama.");
    }

    Ok(())
}

async fn notas(args: &[String]) -> Result<()> {
    let opts = opciones(args);

    let Some(entrada) = opts.get("in") else {
        bail!("falta --in <archivo>. Usa «dictar ayuda» para ver el uso.");
    };

    let kind = match opts.get("tipo").map(String::as_str).unwrap_or("clase") {
        "clase" => SessionKind::Class,
        "cliente" => SessionKind::ClientMeeting,
        "equipo" | "reunion" => SessionKind::TeamMeeting,
        otro => bail!("tipo desconocido «{otro}»: usa clase, cliente o equipo"),
    };

    let texto =
        std::fs::read_to_string(entrada).with_context(|| format!("no se pudo leer «{entrada}»"))?;

    let sesion = SessionId::nuevo();
    let frases = transcripcion::parsear(&texto, &sesion, 5_000);
    if frases.is_empty() {
        bail!("la transcripción está vacía");
    }

    let duracion_ms = opts
        .get("duracion")
        .and_then(|d| d.parse::<i64>().ok())
        .map(|min| min * 60_000)
        .unwrap_or_else(|| frases.last().map(|f| f.end_ms).unwrap_or(0));

    let ctx = SessionContext {
        kind: kind.into(),
        contexto: opts.get("contexto").cloned().unwrap_or_default(),
        duracion_ms,
        fecha: opts
            .get("fecha")
            .cloned()
            .unwrap_or_else(dictar_notes::fecha::hoy),
        glosario: opts.get("glosario").cloned().unwrap_or_default(),
        diapositivas: vec![],
    };

    let cfg = cargar_config(opts.get("config"))?;
    let router = Router::desde_config(&cfg, &resolver_por_defecto())?;

    eprintln!(
        "→ {} intervenciones · {} · cadena: {}",
        frases.len(),
        ctx.plantilla().nombre_humano(),
        router.cadena(ctx_tarea(&ctx)).join(" → ")
    );

    let generador = NotesGenerator::nuevo(&router);
    let g = generador.generar(&ctx, &frases).await?;

    eprintln!(
        "✔ generado con «{}» · {} tokens entrada / {} salida · ${:.4}",
        g.proveedor, g.usage.tokens_in, g.usage.tokens_out, g.usage.cost_usd
    );

    let salida = if opts.contains_key("json") {
        serde_json::to_string_pretty(&g.payload)?
    } else {
        let titulo = if ctx.contexto.is_empty() {
            ctx.plantilla().nombre_humano().to_owned()
        } else {
            ctx.contexto.clone()
        };
        markdown::render(&g.payload, &titulo)
    };

    match opts.get("out") {
        Some(ruta) => {
            std::fs::write(ruta, &salida)
                .with_context(|| format!("no se pudo escribir «{ruta}»"))?;
            eprintln!("✔ escrito en {ruta}");
        }
        None => println!("{salida}"),
    }

    Ok(())
}

fn ctx_tarea(ctx: &SessionContext) -> dictar_providers::config::Task {
    use dictar_domain::NoteTemplate;
    use dictar_providers::config::Task;
    match ctx.plantilla() {
        NoteTemplate::Class => Task::ClassNotes,
        NoteTemplate::Client => Task::ClientMinutes,
        NoteTemplate::Team => Task::TeamNotes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn se_analizan_las_opciones_con_y_sin_valor() {
        let args: Vec<String> = ["--in", "a.txt", "--json", "--tipo", "clase"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let o = opciones(&args);

        assert_eq!(o.get("in").unwrap(), "a.txt");
        assert_eq!(o.get("tipo").unwrap(), "clase");
        assert!(
            o.contains_key("json"),
            "una bandera sin valor debe registrarse"
        );
    }

    #[test]
    fn una_bandera_seguida_de_otra_no_se_traga_el_siguiente_argumento() {
        let args: Vec<String> = ["--json", "--in", "a.txt"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let o = opciones(&args);
        assert_eq!(o.get("in").unwrap(), "a.txt");
    }

    #[test]
    fn la_configuracion_por_defecto_carga_sin_archivo_externo() {
        // Si esto falla, la aplicación no arranca en un equipo recién instalado.
        assert!(cargar_config(None).is_ok());
    }
}
