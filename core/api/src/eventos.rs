//! Eventos que el núcleo emite hacia la interfaz.
//!
//! Un solo canal para todo lo que la interfaz necesita pintar. Cuando entre
//! `flutter_rust_bridge`, este enum se convierte en un `Stream` que un
//! `StreamBuilder` de Flutter consume directamente, sin *polling* ni IPC a
//! mano.

use dictar_domain::{SessionId, Track, TsMs};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum Evento {
    /// Niveles de las dos pistas, para los indicadores de voz.
    ///
    /// Se emiten a menudo y son lo que evita el peor fallo posible: descubrir
    /// al terminar la clase que no se estaba captando al profesor.
    Niveles {
        mic: f32,
        sistema: f32,
        /// Duración de la grabación hasta ahora.
        transcurrido_ms: i64,
    },

    /// Una frase transcrita.
    Frase {
        session_id: String,
        track: String,
        inicio_ms: TsMs,
        fin_ms: TsMs,
        texto: String,
        /// `false` mientras es la pasada en vivo, que se sustituye entera al
        /// terminar. La interfaz la pinta atenuada.
        definitiva: bool,
    },

    /// Avance del procesado posterior.
    Progreso {
        fase: FaseProceso,
        /// De 0 a 1. La interfaz no debe inventarse porcentajes.
        fraccion: f32,
        detalle: String,
    },

    /// La sesión terminó de procesarse.
    Terminada {
        session_id: String,
        coste_usd: f64,
        avisos: usize,
    },

    /// Algo falló, pero la sesión no se pierde.
    Error { mensaje: String, recuperable: bool },
}

impl Evento {
    pub fn frase(
        session_id: &SessionId,
        track: Track,
        inicio_ms: TsMs,
        fin_ms: TsMs,
        texto: impl Into<String>,
        definitiva: bool,
    ) -> Self {
        Evento::Frase {
            session_id: session_id.0.clone(),
            track: track.as_str().to_owned(),
            inicio_ms,
            fin_ms,
            texto: texto.into(),
            definitiva,
        }
    }

    pub fn progreso(fase: FaseProceso, fraccion: f32, detalle: impl Into<String>) -> Self {
        Evento::Progreso {
            fase,
            fraccion: fraccion.clamp(0.0, 1.0),
            detalle: detalle.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaseProceso {
    Grabando,
    Segmentando,
    CargandoModelo,
    Transcribiendo,
    GenerandoNotas,
    Guardando,
    Lista,
}

impl FaseProceso {
    pub fn etiqueta(&self) -> &'static str {
        match self {
            FaseProceso::Grabando => "Grabando",
            FaseProceso::Segmentando => "Preparando el audio",
            FaseProceso::CargandoModelo => "Cargando el modelo",
            FaseProceso::Transcribiendo => "Transcribiendo",
            FaseProceso::GenerandoNotas => "Generando notas",
            FaseProceso::Guardando => "Guardando",
            FaseProceso::Lista => "Lista",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_eventos_llevan_su_tipo_al_serializar() {
        // El puente con Flutter los distingue por ese campo.
        let e = Evento::progreso(FaseProceso::Transcribiendo, 0.5, "3 de 6");
        let j = serde_json::to_value(&e).unwrap();
        assert_eq!(j["tipo"], "progreso");
        assert_eq!(j["fase"], "transcribiendo");
    }

    #[test]
    fn la_fraccion_de_progreso_no_se_sale_de_rango() {
        // La interfaz dibuja una barra: un 1.5 la rompería.
        let e = Evento::progreso(FaseProceso::Transcribiendo, 1.8, "");
        let Evento::Progreso { fraccion, .. } = e else {
            panic!("tipo equivocado");
        };
        assert_eq!(fraccion, 1.0);

        let e = Evento::progreso(FaseProceso::Transcribiendo, -0.3, "");
        let Evento::Progreso { fraccion, .. } = e else {
            panic!("tipo equivocado");
        };
        assert_eq!(fraccion, 0.0);
    }

    #[test]
    fn una_frase_conserva_pista_y_tiempos() {
        let e = Evento::frase(
            &SessionId::from("ses_1"),
            Track::System,
            1000,
            4000,
            "hola",
            false,
        );
        let j = serde_json::to_value(&e).unwrap();
        assert_eq!(j["track"], "system");
        assert_eq!(j["definitiva"], false);
        assert_eq!(j["inicio_ms"], 1000);
    }

    #[test]
    fn todas_las_fases_tienen_etiqueta_legible() {
        for f in [
            FaseProceso::Grabando,
            FaseProceso::Segmentando,
            FaseProceso::CargandoModelo,
            FaseProceso::Transcribiendo,
            FaseProceso::GenerandoNotas,
            FaseProceso::Guardando,
            FaseProceso::Lista,
        ] {
            assert!(!f.etiqueta().is_empty());
        }
    }
}
