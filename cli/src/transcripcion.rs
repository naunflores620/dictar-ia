//! Lectura de transcripciones desde un archivo de texto.
//!
//! Permite validar el pipeline de notas contra proveedores reales sin depender
//! todavía de la captura de audio ni de Whisper. Es la prueba 5 de la fase 0.

use dictar_domain::{SessionId, Track, Utterance};

/// Interpreta una transcripción en texto plano.
///
/// Admite dos formatos, porque los dos aparecen en la práctica:
///
/// ```text
/// [123456] Profesor: la transformada de Laplace...
/// Profesor: la transformada de Laplace...
/// la transformada de Laplace...
/// ```
///
/// Cuando falta la marca de tiempo se generan marcas sintéticas espaciadas: no
/// serán exactas, pero mantienen el orden y permiten que el modelo devuelva
/// `ts_ms` coherentes con el texto que se le pasó.
pub fn parsear(texto: &str, sesion: &SessionId, ms_por_linea: i64) -> Vec<Utterance> {
    let mut out = Vec::new();
    let mut reloj = 0i64;

    for linea in texto.lines() {
        let linea = linea.trim();
        if linea.is_empty() {
            continue;
        }

        let (ts, resto) = match extraer_marca(linea) {
            Some((ts, r)) => (ts, r),
            None => (reloj, linea),
        };

        let (hablante, contenido) = match separar_hablante(resto) {
            Some((h, c)) => (Some(h.to_owned()), c),
            None => (None, resto),
        };

        if contenido.trim().is_empty() {
            continue;
        }

        // Sin más información, se atribuye al interlocutor: en una clase o una
        // reunión, la mayor parte del texto es de la otra parte.
        let track = match hablante.as_deref() {
            Some(h) if h.eq_ignore_ascii_case("yo") => Track::Mic,
            _ => Track::System,
        };

        let fin = ts + ms_por_linea;
        out.push(Utterance {
            id: out.len() as i64,
            session_id: sesion.clone(),
            track,
            speaker: hablante,
            start_ms: ts,
            end_ms: fin,
            text: contenido.trim().to_owned(),
            confidence: None,
            is_final: true,
            revision: 1,
        });

        reloj = fin.max(reloj + ms_por_linea);
    }

    out
}

/// `[123456] resto` → `(123456, "resto")`
fn extraer_marca(linea: &str) -> Option<(i64, &str)> {
    let resto = linea.strip_prefix('[')?;
    let fin = resto.find(']')?;
    let dentro = resto[..fin].trim();
    let ts = parsear_tiempo(dentro)?;
    Some((ts, resto[fin + 1..].trim_start()))
}

/// Admite milisegundos crudos y también `hh:mm:ss` o `mm:ss`, que es como los
/// escriben las herramientas de subtítulos.
fn parsear_tiempo(s: &str) -> Option<i64> {
    if let Ok(ms) = s.parse::<i64>() {
        return Some(ms);
    }

    let partes: Vec<&str> = s.split(':').collect();
    if partes.len() < 2 || partes.len() > 3 {
        return None;
    }

    let mut total = 0i64;
    for p in &partes {
        let n: i64 = p.split('.').next()?.trim().parse().ok()?;
        total = total * 60 + n;
    }
    Some(total * 1000)
}

/// `Profesor: texto` → `("Profesor", "texto")`
///
/// Distinguir un rótulo de hablante de unos dos puntos normales importa: sin
/// ello, «En resumen: lo importante es...» produce un hablante fantasma
/// llamado «En resumen», y el modelo acaba atribuyéndole frases.
///
/// La señal que de verdad discrimina es la mayúscula: un rótulo es un nombre
/// propio o un rol —«Profesor», «Yo», «Juan Pérez»—, así que **todas** sus
/// palabras empiezan por mayúscula. En «En resumen», la segunda no.
fn separar_hablante(linea: &str) -> Option<(&str, &str)> {
    let i = linea.find(':')?;
    let nombre = linea[..i].trim();

    if nombre.is_empty() || nombre.len() > 40 || nombre.contains([',', ';', '?', '¿', '!', '¡']) {
        return None;
    }

    let palabras: Vec<&str> = nombre.split_whitespace().collect();
    if palabras.is_empty() || palabras.len() > 3 {
        return None;
    }

    // Se admiten partículas de nombre en minúscula ("Juan de la Cruz") pero
    // nunca como primera palabra.
    const PARTICULAS: &[&str] = &["de", "del", "la", "las", "los", "y", "van", "von"];

    for (idx, p) in palabras.iter().enumerate() {
        let empieza_mayus = p.chars().next().is_some_and(char::is_uppercase);
        let es_particula = idx > 0 && PARTICULAS.contains(&p.to_lowercase().as_str());
        if !empieza_mayus && !es_particula {
            return None;
        }
    }

    Some((nombre, &linea[i + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ses() -> SessionId {
        SessionId::from("ses_test")
    }

    #[test]
    fn formato_completo_con_marca_y_hablante() {
        let t = parsear("[12000] Profesor: la transformada de Laplace", &ses(), 5000);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].start_ms, 12_000);
        assert_eq!(t[0].speaker.as_deref(), Some("Profesor"));
        assert_eq!(t[0].text, "la transformada de Laplace");
        assert_eq!(t[0].track, Track::System);
    }

    #[test]
    fn el_hablante_yo_va_a_la_pista_de_microfono() {
        let t = parsear("Yo: ¿puede repetir?", &ses(), 5000);
        assert_eq!(t[0].track, Track::Mic);
    }

    #[test]
    fn sin_marcas_se_generan_tiempos_sinteticos_ordenados() {
        let t = parsear("primera\nsegunda\ntercera", &ses(), 5000);
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].start_ms, 0);
        assert_eq!(t[1].start_ms, 5_000);
        assert_eq!(t[2].start_ms, 10_000);
    }

    #[test]
    fn se_admiten_marcas_en_formato_de_subtitulo() {
        let t = parsear("[01:01:11] Profesor: claro", &ses(), 5000);
        assert_eq!(t[0].start_ms, 3_671_000);

        let corto = parsear("[02:30] Profesor: claro", &ses(), 5000);
        assert_eq!(corto[0].start_ms, 150_000);
    }

    #[test]
    fn los_dos_puntos_de_una_frase_normal_no_crean_un_hablante() {
        // "En resumen: lo importante..." no es un hablante llamado "En resumen".
        let t = parsear(
            "En resumen: lo importante es la región de convergencia",
            &ses(),
            5000,
        );
        assert!(t[0].speaker.is_none());
        assert!(t[0].text.starts_with("En resumen:"));
    }

    #[test]
    fn una_frase_larga_con_dos_puntos_no_se_parte() {
        let linea = "Y entonces, como decíamos antes, la conclusión es esta: converge";
        let t = parsear(linea, &ses(), 5000);
        assert!(t[0].speaker.is_none(), "hablante detectado por error");
    }

    #[test]
    fn las_lineas_vacias_se_ignoran() {
        let t = parsear("uno\n\n  \n\ndos", &ses(), 5000);
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn una_linea_con_hablante_pero_sin_texto_se_descarta() {
        let t = parsear("Profesor:\nProfesor: algo", &ses(), 5000);
        assert_eq!(t.len(), 1);
    }
}
