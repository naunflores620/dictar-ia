//! Transcripción con `whisper.cpp`.
//!
//! Dos requisitos duros que condicionan la elección del motor:
//!
//! 1. **`initial_prompt`** — es por donde entra el glosario de la asignatura.
//!    Sin eso, el vocabulario acumulado no sirve de nada y cada clase se
//!    transcribe tan mal como la primera.
//! 2. **Ejecución por segmentos con contexto** — necesaria para la pasada en
//!    vivo con LocalAgreement-2.

pub mod acuerdo;
pub mod limpieza;
pub mod modelos;
pub mod whisper;

use dictar_domain::TsMs;

#[derive(Debug, thiserror::Error)]
pub enum SttError {
    #[error("no se encontró el modelo en {0}")]
    ModeloNoEncontrado(String),

    #[error("no se pudo cargar el modelo: {0}")]
    Carga(String),

    #[error("la transcripción falló: {0}")]
    Transcripcion(String),

    #[error("error de E/S: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, SttError>;

/// Un fragmento transcrito, con sus tiempos relativos al segmento de entrada.
#[derive(Debug, Clone, PartialEq)]
pub struct Fragmento {
    pub texto: String,
    pub inicio_ms: TsMs,
    pub fin_ms: TsMs,
    /// Probabilidad media de los tokens. Sirve para marcar como dudoso lo que
    /// el modelo no tenía claro, en vez de presentarlo como cierto.
    pub confianza: f32,
}

#[derive(Debug, Clone)]
pub struct SttOpciones {
    /// Código ISO del idioma, o `None` para detección automática.
    ///
    /// Fijarlo importa: en una clase en español con términos en inglés, la
    /// detección automática a veces cambia de idioma a mitad de segmento y
    /// devuelve una traducción en lugar de una transcripción.
    pub idioma: Option<String>,

    /// Vocabulario de la asignatura o del cliente.
    ///
    /// Whisper solo atiende a los últimos ~224 tokens de este prompt, así que
    /// quien lo construye debe ordenarlo por relevancia y recortarlo. Meterle
    /// el glosario entero desplazaría los términos importantes fuera de la
    /// ventana.
    pub glosario: Option<String>,

    /// Últimas palabras ya transcritas, como contexto.
    pub contexto_previo: Option<String>,

    pub hilos: i32,
    /// Marcas de tiempo por palabra. Cuestan tiempo; solo para la pasada final.
    pub tiempos_por_palabra: bool,
}

impl Default for SttOpciones {
    fn default() -> Self {
        Self {
            idioma: Some("es".to_owned()),
            glosario: None,
            contexto_previo: None,
            // Dejar un núcleo libre: durante una clase online el equipo también
            // está reproduciendo vídeo, y quedarse sin CPU corta el audio.
            hilos: (std::thread::available_parallelism()
                .map(|n| n.get() as i32)
                .unwrap_or(4)
                - 1)
            .max(1),
            tiempos_por_palabra: false,
        }
    }
}

impl SttOpciones {
    /// Ajustes para la transcripción durante la clase.
    ///
    /// Se dejan tres núcleos libres, no la mitad. Con la mitad la cola no
    /// llegaba a seguir el ritmo: hay **dos pistas**, así que hay que
    /// transcribir el doble de audio que dura la clase, y al detener quedaba
    /// un minuto y medio de trabajo pendiente. Con tres libres, el equipo
    /// sigue holgado para la videollamada y la cola no se acumula.
    pub fn en_vivo() -> Self {
        let nucleos = std::thread::available_parallelism()
            .map(|n| n.get() as i32)
            .unwrap_or(4);

        Self {
            tiempos_por_palabra: false,
            hilos: (nucleos - 3).max(1),
            ..Default::default()
        }
    }

    /// Ajustes para la pasada definitiva, que corre en segundo plano.
    pub fn definitiva() -> Self {
        Self {
            tiempos_por_palabra: true,
            ..Default::default()
        }
    }

    /// Prompt inicial combinando glosario y contexto previo.
    ///
    /// El glosario va al final a propósito: Whisper atiende a la parte final
    /// del prompt, así que los términos técnicos —que es lo que más falla— son
    /// lo último que lee.
    pub fn prompt_inicial(&self) -> Option<String> {
        match (&self.contexto_previo, &self.glosario) {
            (None, None) => None,
            (Some(c), None) => Some(c.clone()),
            (None, Some(g)) => Some(g.clone()),
            (Some(c), Some(g)) => Some(format!("{c} {g}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn por_defecto_se_fija_el_espanol() {
        // La detección automática puede cambiar de idioma a mitad de segmento
        // y devolver una traducción en vez de una transcripción.
        assert_eq!(SttOpciones::default().idioma.as_deref(), Some("es"));
    }

    #[test]
    fn en_vivo_deja_nucleos_libres_pero_no_se_queda_corta() {
        // Durante la clase el equipo también reproduce la videollamada, así
        // que no puede usarlos todos. Pero con la mitad la cola no seguía el
        // ritmo de las dos pistas.
        let vivo = SttOpciones::en_vivo().hilos;
        let final_ = SttOpciones::definitiva().hilos;

        assert!(vivo < final_, "debe dejar núcleos libres");
        assert!(vivo >= 1);

        // Con ocho núcleos, cinco: suficiente para ir por delante de dos
        // pistas en tiempo real.
        if std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(0)
            == 8
        {
            assert_eq!(vivo, 5);
        }
    }

    #[test]
    fn los_tiempos_por_palabra_solo_en_la_pasada_definitiva() {
        assert!(!SttOpciones::en_vivo().tiempos_por_palabra);
        assert!(SttOpciones::definitiva().tiempos_por_palabra);
    }

    #[test]
    fn el_glosario_va_al_final_del_prompt() {
        // Whisper atiende al final del prompt: los términos técnicos, que son
        // lo que más falla, deben ser lo último que lea.
        let o = SttOpciones {
            contexto_previo: Some("...y entonces vimos que".into()),
            glosario: Some("Laplace, Fourier".into()),
            ..Default::default()
        };
        let p = o.prompt_inicial().unwrap();
        assert!(p.ends_with("Laplace, Fourier"), "prompt: {p}");
    }

    #[test]
    fn sin_glosario_ni_contexto_no_hay_prompt() {
        assert!(SttOpciones::default().prompt_inicial().is_none());
    }
}
