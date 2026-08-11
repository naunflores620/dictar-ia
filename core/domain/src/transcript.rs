//! Transcripción, diapositivas y glosario.

use crate::ids::{SessionId, TopicId};
use crate::TsMs;
use serde::{Deserialize, Serialize};

str_enum! {
    /// Las dos pistas se graban por separado, nunca mezcladas. De ahí sale la
    /// diarización exacta sin ningún modelo: `Mic` eres tú, `System` es el
    /// profesor o el cliente.
    pub enum Track {
        Mic    => "mic",
        System => "system",
    }
}

str_enum! {
    pub enum DenoiseLevel {
        Off    => "off",
        Light  => "light",
        Strong => "strong",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Utterance {
    pub id: i64,
    pub session_id: SessionId,
    pub track: Track,
    /// "Yo", "Profesor", "Cliente". Derivado de la pista salvo en presencial.
    pub speaker: Option<String>,
    pub start_ms: TsMs,
    pub end_ms: TsMs,
    pub text: String,
    pub confidence: Option<f32>,
    /// `false` = pasada en vivo con el modelo pequeño (desechable).
    /// `true`  = pasada definitiva con `large-v3-turbo`.
    pub is_final: bool,
    pub revision: i32,
}

impl Utterance {
    pub fn duracion_ms(&self) -> i64 {
        self.end_ms - self.start_ms
    }

    /// Aproximación suficiente para trocear la transcripción antes de mandarla
    /// al modelo. No pretende igualar al tokenizador real: se usa para decidir
    /// dónde cortar un bloque, no para facturar.
    pub fn tokens_aprox(&self) -> usize {
        // ~0.75 palabras por token en español.
        (self.text.split_whitespace().count() as f32 / 0.75).ceil() as usize
    }
}

/// Diapositiva capturada de la pantalla compartida, anclada al segundo exacto.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slide {
    pub id: i64,
    pub session_id: SessionId,
    pub path: String,
    pub ts_ms: TsMs,
    /// Hash perceptual (dHash). Sirve para no volver a guardar una lámina a la
    /// que el profesor regresa más tarde.
    pub phash: String,
    pub ocr_text: Option<String>,
    pub caption: Option<String>,
}

impl Slide {
    /// Distancia de Hamming entre dos hashes hexadecimales de 64 bits.
    ///
    /// Devuelve `None` si alguno no es un hash válido, en lugar de fingir que
    /// son idénticos: un error de formato no debe leerse como "misma lámina".
    pub fn distancia(a: &str, b: &str) -> Option<u32> {
        let x = u64::from_str_radix(a, 16).ok()?;
        let y = u64::from_str_radix(b, 16).ok()?;
        Some((x ^ y).count_ones())
    }
}

/// Término del vocabulario de un `topic`. Alimenta el `initial_prompt` de
/// Whisper, que es lo que hace que cada clase transcriba mejor que la anterior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlossaryTerm {
    pub id: i64,
    pub topic_id: TopicId,
    pub term: String,
    pub definition: Option<String>,
    pub source: GlossarySource,
    /// Cuántas veces se ha visto. Ordena qué entra en el prompt cuando no cabe
    /// todo el glosario.
    pub hits: i64,
}

str_enum! {
    pub enum GlossarySource {
        /// Escrito por el usuario.
        User          => "user",
        /// Extraído del temario o los PDF del curso.
        Syllabus      => "syllabus",
        /// OCR de las diapositivas capturadas. La fuente más fiable: es el
        /// término escrito, no oído.
        SlideOcr      => "slide_ocr",
        /// Detectado por repetición en sesiones anteriores.
        AutoRecurrent => "auto_recurrent",
    }
}

impl GlossarySource {
    /// Peso al ordenar el glosario para el prompt. El texto de una diapositiva
    /// es ortografía cierta; una repetición detectada por el propio STT puede
    /// estar propagando un error de transcripción.
    pub fn fiabilidad(&self) -> u8 {
        match self {
            GlossarySource::User => 4,
            GlossarySource::SlideOcr => 3,
            GlossarySource::Syllabus => 2,
            GlossarySource::AutoRecurrent => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distancia_entre_hashes() {
        assert_eq!(
            Slide::distancia("ffffffffffffffff", "ffffffffffffffff"),
            Some(0)
        );
        assert_eq!(
            Slide::distancia("0000000000000000", "0000000000000001"),
            Some(1)
        );
        assert_eq!(
            Slide::distancia("0000000000000000", "ffffffffffffffff"),
            Some(64)
        );
    }

    #[test]
    fn un_hash_invalido_no_se_confunde_con_lamina_identica() {
        assert_eq!(Slide::distancia("no-es-hex", "0000000000000000"), None);
    }

    #[test]
    fn el_ocr_pesa_mas_que_la_repeticion_detectada() {
        assert!(GlossarySource::SlideOcr.fiabilidad() > GlossarySource::AutoRecurrent.fiabilidad());
    }
}
