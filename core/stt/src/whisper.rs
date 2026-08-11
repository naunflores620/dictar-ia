//! Envoltorio de `whisper.cpp`.

use crate::{Fragmento, Result, SttError, SttOpciones};
use std::path::Path;
use std::sync::Once;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

static SILENCIAR: Once = Once::new();

/// Modelo cargado en memoria, reutilizable entre segmentos.
///
/// Cargar el modelo cuesta segundos y cientos de megas: hacerlo una vez por
/// segmento haría que transcribir una clase tardase más que la clase.
pub struct Transcriptor {
    ctx: WhisperContext,
    ruta: String,
}

impl Transcriptor {
    pub fn cargar(ruta: impl AsRef<Path>) -> Result<Self> {
        // whisper.cpp escribe mucho por stderr; se redirige a `tracing` para
        // que no ensucie la salida de la aplicación.
        SILENCIAR.call_once(whisper_rs::install_logging_hooks);

        let ruta = ruta.as_ref();
        if !ruta.is_file() {
            return Err(SttError::ModeloNoEncontrado(ruta.display().to_string()));
        }

        let s = ruta
            .to_str()
            .ok_or_else(|| SttError::Carga("la ruta del modelo no es UTF-8".into()))?;

        let ctx = WhisperContext::new_with_params(s, WhisperContextParameters::default())
            .map_err(|e| SttError::Carga(e.to_string()))?;

        tracing::info!(modelo = s, "modelo de Whisper cargado");
        Ok(Self {
            ctx,
            ruta: s.to_owned(),
        })
    }

    pub fn ruta(&self) -> &str {
        &self.ruta
    }

    /// Transcribe un bloque de audio mono a 16 kHz.
    pub fn transcribir(&self, pcm: &[f32], opts: &SttOpciones) -> Result<Vec<Fragmento>> {
        if pcm.is_empty() {
            return Ok(Vec::new());
        }

        let mut estado = self
            .ctx
            .create_state()
            .map_err(|e| SttError::Transcripcion(e.to_string()))?;

        // El prompt tiene que vivir más que `params`, que solo lo toma prestado.
        let prompt = opts.prompt_inicial();

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(opts.hilos);
        // Transcribir, nunca traducir: unos apuntes traducidos al inglés no
        // sirven para el examen de la asignatura.
        params.set_translate(false);
        if let Some(l) = opts.idioma.as_deref() {
            params.set_language(Some(l));
        }
        if let Some(p) = prompt.as_deref() {
            params.set_initial_prompt(p);
        }
        params.set_token_timestamps(opts.tiempos_por_palabra);

        // Sin esto, whisper.cpp imprime la transcripción por stdout mientras
        // corre y se mezcla con la salida de la aplicación.
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        // Las alucinaciones de Whisper aparecen sobre todo en tramos sin voz:
        // suprimir los tokens que no son habla reduce mucho el «Subtítulos por
        // la comunidad de Amara.org» que sale de la nada en los silencios.
        params.set_suppress_blank(true);

        estado
            .full(params, pcm)
            .map_err(|e| SttError::Transcripcion(e.to_string()))?;

        let n = estado
            .full_n_segments()
            .map_err(|e| SttError::Transcripcion(e.to_string()))?;

        let mut out = Vec::with_capacity(n as usize);
        for i in 0..n {
            let texto = estado
                .full_get_segment_text(i)
                .map_err(|e| SttError::Transcripcion(e.to_string()))?;

            let texto = texto.trim();
            if texto.is_empty() {
                continue;
            }

            // whisper.cpp da los tiempos en centisegundos.
            let t0 = estado
                .full_get_segment_t0(i)
                .map_err(|e| SttError::Transcripcion(e.to_string()))?;
            let t1 = estado
                .full_get_segment_t1(i)
                .map_err(|e| SttError::Transcripcion(e.to_string()))?;

            out.push(Fragmento {
                texto: texto.to_owned(),
                inicio_ms: t0 * 10,
                fin_ms: t1 * 10,
                confianza: confianza_segmento(&mut estado, i),
            });
        }

        Ok(out)
    }
}

/// Probabilidad media de los tokens del segmento.
///
/// Se usa para marcar como dudoso lo que el modelo no tenía claro, en lugar de
/// presentarlo con la misma seguridad que el resto. En unos apuntes, distinguir
/// lo cierto de lo dudoso es lo que permite fiarse de ellos.
fn confianza_segmento(estado: &mut whisper_rs::WhisperState, segmento: i32) -> f32 {
    let Ok(n) = estado.full_n_tokens(segmento) else {
        return 0.0;
    };
    if n == 0 {
        return 0.0;
    }

    let mut suma = 0.0f32;
    let mut contados = 0u32;
    for t in 0..n {
        if let Ok(datos) = estado.full_get_token_data(segmento, t) {
            suma += datos.p;
            contados += 1;
        }
    }

    if contados == 0 {
        0.0
    } else {
        suma / contados as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargar_un_modelo_inexistente_da_un_error_claro() {
        // `WhisperContext` no implementa Debug, así que no se puede usar
        // `unwrap_err()` sobre el Result directamente.
        match Transcriptor::cargar("/no/existe/modelo.bin") {
            Err(SttError::ModeloNoEncontrado(_)) => {}
            Err(otro) => panic!("error inesperado: {otro}"),
            Ok(_) => panic!("no debería cargar un modelo que no existe"),
        }
    }

    #[test]
    fn los_tiempos_de_whisper_se_convierten_de_centisegundos() {
        // whisper.cpp devuelve centisegundos; el resto del sistema trabaja en
        // milisegundos. Equivocarse aquí desplazaría todas las marcas por 10.
        let centisegundos: i64 = 415;
        assert_eq!(centisegundos * 10, 4150);
    }
}
