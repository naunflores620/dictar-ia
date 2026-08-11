//! Troceado de la transcripción en bloques para la fase de *map*.
//!
//! Mandar dos horas de transcripción en una sola llamada degrada el resultado
//! aunque quepa en la ventana de contexto: el modelo pierde el detalle del
//! medio y devuelve un resumen genérico. Por eso se trocea y se consolida
//! después.

use dictar_domain::{Track, TsMs, Utterance};

/// Tokens por bloque. Con ~19.000 tokens de transcripción para una sesión de
/// 1,5 h, esto da unos 6 bloques: suficiente detalle sin multiplicar el coste.
pub const TOKENS_POR_BLOQUE: usize = 3_500;

/// Frases de solape entre bloques consecutivos.
///
/// Sin solape, una idea que cruza la frontera se pierde en los dos lados: el
/// bloque A la ve empezar sin conclusión y el B la ve acabar sin contexto.
pub const SOLAPE_FRASES: usize = 3;

#[derive(Debug, Clone)]
pub struct Bloque {
    pub indice: usize,
    pub inicio_ms: TsMs,
    pub fin_ms: TsMs,
    /// Texto ya formateado con marca de tiempo y hablante.
    pub texto: String,
    pub tokens_aprox: usize,
}

/// Divide la transcripción en bloques con solape.
pub fn trocear(frases: &[Utterance], tokens_por_bloque: usize) -> Vec<Bloque> {
    if frases.is_empty() {
        return Vec::new();
    }

    let mut bloques = Vec::new();
    let mut inicio = 0usize;

    while inicio < frases.len() {
        let mut fin = inicio;
        let mut tokens = 0usize;

        while fin < frases.len() {
            let t = frases[fin].tokens_aprox();
            // Siempre al menos una frase, aunque sola supere el presupuesto:
            // si no, un monólogo de diez minutos bloquearía el bucle.
            if tokens + t > tokens_por_bloque && fin > inicio {
                break;
            }
            tokens += t;
            fin += 1;
        }

        bloques.push(Bloque {
            indice: bloques.len(),
            inicio_ms: frases[inicio].start_ms,
            fin_ms: frases[fin - 1].end_ms,
            texto: formatear(&frases[inicio..fin]),
            tokens_aprox: tokens,
        });

        if fin >= frases.len() {
            break;
        }
        // Retroceder para solapar, sin llegar nunca a no avanzar.
        inicio = fin.saturating_sub(SOLAPE_FRASES).max(inicio + 1);
    }

    bloques
}

/// Formatea las frases con marca de tiempo y hablante.
///
/// El `ts_ms` viaja dentro del texto porque es lo que permite al modelo
/// devolver marcas de tiempo verificables: sin verlas, no puede citarlas.
pub fn formatear(frases: &[Utterance]) -> String {
    frases
        .iter()
        .map(|u| {
            let quien = u.speaker.as_deref().unwrap_or(match u.track {
                Track::Mic => "Yo",
                Track::System => "Interlocutor",
            });
            format!("[{}] {}: {}", u.start_ms, quien, u.text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use dictar_domain::SessionId;

    fn frase(id: i64, ini: TsMs, palabras: usize) -> Utterance {
        Utterance {
            id,
            session_id: SessionId::from("s"),
            track: Track::System,
            speaker: None,
            start_ms: ini,
            end_ms: ini + 5_000,
            text: vec!["palabra"; palabras].join(" "),
            confidence: Some(0.9),
            is_final: true,
            revision: 1,
        }
    }

    #[test]
    fn una_transcripcion_vacia_no_produce_bloques() {
        assert!(trocear(&[], 1000).is_empty());
    }

    #[test]
    fn una_transcripcion_corta_cabe_en_un_solo_bloque() {
        let frases: Vec<_> = (0..5).map(|i| frase(i, i * 5_000, 10)).collect();
        let bloques = trocear(&frases, 3_500);
        assert_eq!(bloques.len(), 1);
        assert_eq!(bloques[0].inicio_ms, 0);
        assert_eq!(bloques[0].fin_ms, 4 * 5_000 + 5_000);
    }

    #[test]
    fn los_bloques_se_solapan_para_no_partir_ideas() {
        let frases: Vec<_> = (0..60).map(|i| frase(i, i * 5_000, 30)).collect();
        let bloques = trocear(&frases, 200);

        assert!(bloques.len() > 2, "deberían salir varios bloques");
        // El bloque siguiente empieza antes de donde acabó el anterior.
        for par in bloques.windows(2) {
            assert!(
                par[1].inicio_ms < par[0].fin_ms,
                "sin solape entre el bloque {} y el {}",
                par[0].indice,
                par[1].indice
            );
        }
    }

    #[test]
    fn una_frase_mas_larga_que_el_presupuesto_no_bloquea_el_bucle() {
        // Un monólogo de diez minutos sin pausa no debe colgar el troceado.
        let frases = vec![frase(1, 0, 5_000), frase(2, 600_000, 10)];
        let bloques = trocear(&frases, 100);
        assert!(!bloques.is_empty());
        assert!(
            bloques[0].tokens_aprox > 100,
            "la frase larga entra igualmente"
        );
    }

    #[test]
    fn se_cubre_toda_la_transcripcion_sin_huecos() {
        let frases: Vec<_> = (0..40).map(|i| frase(i, i * 5_000, 40)).collect();
        let bloques = trocear(&frases, 300);

        assert_eq!(bloques.first().unwrap().inicio_ms, 0);
        assert_eq!(
            bloques.last().unwrap().fin_ms,
            frases.last().unwrap().end_ms,
            "el último bloque debe llegar al final de la sesión"
        );
    }

    #[test]
    fn el_texto_lleva_marca_de_tiempo_y_hablante() {
        let mut f = frase(1, 12_345, 2);
        f.track = Track::Mic;
        let texto = formatear(&[f]);
        assert!(texto.starts_with("[12345] Yo: "));
    }

    #[test]
    fn se_respeta_el_nombre_de_hablante_cuando_existe() {
        let mut f = frase(1, 0, 2);
        f.speaker = Some("Profesor".into());
        assert!(formatear(&[f]).contains("Profesor:"));
    }
}
