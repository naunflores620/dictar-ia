//! Normalización del audio: a mono y a 16 kHz.
//!
//! Los dispositivos entregan lo que les da la gana —48 kHz estéreo lo habitual,
//! 44,1 kHz en algunos micrófonos— y Whisper quiere 16 kHz mono. Convertir aquí,
//! una sola vez y en el origen, evita que cada etapa posterior tenga que
//! preguntarse en qué formato le llega la señal.

use crate::{AudioError, Result, SAMPLE_RATE};
use rubato::{FastFixedIn, PolynomialDegree, Resampler};

/// Muestras que se procesan de una vez.
///
/// A 48 kHz son 21 ms: lo bastante corto para no añadir latencia perceptible a
/// la transcripción en vivo, y lo bastante largo para que el remuestreo no se
/// pase el rato con trabajo de preparación.
const BLOQUE: usize = 1024;

/// Mezcla los canales intercalados a mono.
///
/// Se promedia en lugar de quedarse con el canal izquierdo: en una llamada de
/// Meet la voz puede venir panoramizada, y descartar un canal significaría
/// perder al interlocutor a la mitad de volumen o del todo.
pub fn a_mono(intercalado: &[f32], canales: usize) -> Vec<f32> {
    if canales <= 1 {
        return intercalado.to_vec();
    }

    intercalado
        .chunks_exact(canales)
        .map(|c| c.iter().sum::<f32>() / canales as f32)
        .collect()
}

/// Remuestreador con estado, para un flujo continuo.
///
/// Guarda las muestras que no completan un bloque en lugar de descartarlas: si
/// se tiraran, en dos horas de clase se perderían miles de muestras sueltas y
/// el audio se desincronizaría poco a poco del reloj de la sesión.
pub struct Remuestreador {
    interno: Option<FastFixedIn<f32>>,
    pendientes: Vec<f32>,
    salida: Vec<f32>,
}

impl Remuestreador {
    pub fn nuevo(frecuencia_entrada: u32) -> Result<Self> {
        // Si ya viene a 16 kHz, no se toca: cualquier remuestreo, por bueno que
        // sea, degrada un poco.
        if frecuencia_entrada == SAMPLE_RATE {
            return Ok(Self {
                interno: None,
                pendientes: Vec::new(),
                salida: Vec::new(),
            });
        }

        let ratio = SAMPLE_RATE as f64 / frecuencia_entrada as f64;
        let interno = FastFixedIn::<f32>::new(ratio, 1.0, PolynomialDegree::Septic, BLOQUE, 1)
            .map_err(|e| AudioError::Remuestreo(e.to_string()))?;

        Ok(Self {
            interno: Some(interno),
            pendientes: Vec::with_capacity(BLOQUE * 2),
            salida: Vec::new(),
        })
    }

    /// Procesa lo que se pueda y devuelve las muestras ya a 16 kHz.
    pub fn procesar(&mut self, entrada: &[f32]) -> Result<Vec<f32>> {
        let Some(r) = self.interno.as_mut() else {
            return Ok(entrada.to_vec());
        };

        self.pendientes.extend_from_slice(entrada);
        self.salida.clear();

        while self.pendientes.len() >= BLOQUE {
            let bloque: Vec<f32> = self.pendientes.drain(..BLOQUE).collect();
            let procesado = r
                .process(&[bloque], None)
                .map_err(|e| AudioError::Remuestreo(e.to_string()))?;

            if let Some(canal) = procesado.first() {
                self.salida.extend_from_slice(canal);
            }
        }

        Ok(std::mem::take(&mut self.salida))
    }

    /// Muestras aún sin procesar. Solo para comprobaciones.
    pub fn pendientes(&self) -> usize {
        self.pendientes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_estereo_se_promedia_en_vez_de_descartar_un_canal() {
        // Si nos quedáramos con el izquierdo, una voz panoramizada a la derecha
        // desaparecería.
        let estereo = vec![0.0, 1.0, 0.0, 1.0];
        assert_eq!(a_mono(&estereo, 2), vec![0.5, 0.5]);
    }

    #[test]
    fn el_mono_pasa_intacto() {
        let mono = vec![0.1, 0.2, 0.3];
        assert_eq!(a_mono(&mono, 1), mono);
    }

    #[test]
    fn una_trama_incompleta_no_rompe_la_conversion() {
        // chunks_exact descarta el resto: es preferible perder una muestra
        // suelta a leer fuera de rango.
        let raro = vec![0.0, 1.0, 0.5];
        assert_eq!(a_mono(&raro, 2), vec![0.5]);
    }

    #[test]
    fn a_16k_no_se_remuestrea_nada() {
        let mut r = Remuestreador::nuevo(16_000).unwrap();
        let entrada = vec![0.5; 1000];
        assert_eq!(r.procesar(&entrada).unwrap().len(), 1000);
    }

    #[test]
    fn de_48k_a_16k_sale_aproximadamente_un_tercio() {
        let mut r = Remuestreador::nuevo(48_000).unwrap();

        // Un segundo de señal a 48 kHz.
        let entrada: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();

        let salida = r.procesar(&entrada).unwrap();

        // No es exacto porque quedan muestras pendientes de completar bloque.
        let esperado = 16_000;
        assert!(
            (salida.len() as i64 - esperado).abs() < 400,
            "salieron {} muestras, esperaba ~{esperado}",
            salida.len()
        );
    }

    #[test]
    fn de_44100_a_16k_tambien_funciona() {
        // Muchos micrófonos USB entregan 44,1 kHz, que no es múltiplo de 16k.
        let mut r = Remuestreador::nuevo(44_100).unwrap();
        let entrada: Vec<f32> = (0..44_100).map(|i| (i as f32 * 0.01).sin()).collect();
        let salida = r.procesar(&entrada).unwrap();
        assert!((salida.len() as i64 - 16_000).abs() < 500);
    }

    #[test]
    fn las_muestras_sobrantes_se_guardan_y_no_se_pierden() {
        let mut r = Remuestreador::nuevo(48_000).unwrap();

        // Menos de un bloque: no debe salir nada todavía, pero tampoco
        // perderse. En dos horas, tirar los restos desincronizaría el audio.
        let salida = r.procesar(&vec![0.1; 500]).unwrap();
        assert!(salida.is_empty());
        assert_eq!(r.pendientes(), 500);

        // Al completar el bloque, ya sale.
        let salida = r.procesar(&vec![0.1; 600]).unwrap();
        assert!(!salida.is_empty());
        assert_eq!(r.pendientes(), 1100 - BLOQUE);
    }

    #[test]
    fn el_remuestreo_conserva_el_nivel_de_la_senal() {
        // Un remuestreo que altera la amplitud rompería el VAD, que decide por
        // energía si hay voz.
        let mut r = Remuestreador::nuevo(48_000).unwrap();
        let entrada: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.05).sin() * 0.4).collect();

        let salida = r.procesar(&entrada).unwrap();
        let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt();

        let antes = rms(&entrada);
        let despues = rms(&salida);
        assert!(
            (antes - despues).abs() < 0.05,
            "el nivel cambió de {antes} a {despues}"
        );
    }
}
