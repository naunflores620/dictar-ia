//! Reconciliación de frases repetidas por el solape entre segmentos VAD.

use dictar_storage::transcript::NewUtterance;
use dictar_stt::limpieza::similitud;

/// Quita de `nuevas` solo frases repetidas en el borde compartido con
/// segmentos anteriores de la misma pista. Repeticiones en momentos distintos
/// y frases repetidas dentro del mismo segmento se conservan.
pub(crate) fn deduplicar_solape(
    anteriores: &[NewUtterance],
    nuevas: &mut Vec<NewUtterance>,
    inicio_segmento_ms: i64,
    solape_ms: i64,
) -> usize {
    const SIMILITUD_MINIMA: f32 = 0.8;
    const PALABRAS_MINIMAS: usize = 3;

    let limite_borde_ms = inicio_segmento_ms.saturating_add(solape_ms);
    let antes = nuevas.len();
    nuevas.retain(|nueva| {
        let palabras = nueva.text.split_whitespace().count();
        if nueva.start_ms > limite_borde_ms || palabras < PALABRAS_MINIMAS {
            return true;
        }

        !anteriores.iter().any(|anterior| {
            if anterior.track != nueva.track
                || anterior.text.split_whitespace().count() < PALABRAS_MINIMAS
            {
                return false;
            }

            let interseccion =
                anterior.end_ms.min(nueva.end_ms) - anterior.start_ms.max(nueva.start_ms);
            let duracion_corta = (anterior.end_ms - anterior.start_ms)
                .min(nueva.end_ms - nueva.start_ms)
                .max(1);

            interseccion > 0
                && interseccion.saturating_mul(2) >= duracion_corta
                && similitud(&anterior.text, &nueva.text) >= SIMILITUD_MINIMA
        })
    });

    antes - nuevas.len()
}

/// Ventana temporal pequeña usada para descartar historial ya fuera del
/// alcance de un solape entre segmentos consecutivos.
pub(crate) fn conservar_historial_reciente(frases: &mut Vec<NewUtterance>, desde_ms: i64) {
    frases.retain(|frase| frase.end_ms >= desde_ms);
}
