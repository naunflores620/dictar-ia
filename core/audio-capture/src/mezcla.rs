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
    // Se guarda aparte del `interno` (en vez de recalcularla a partir de la
    // frecuencia de entrada) porque `vaciar` la necesita para saber cuántas
    // muestras de salida corresponden a las últimas muestras reales, y no hay
    // otro sitio donde consultarla una vez construido el `FastFixedIn`.
    ratio: f64,
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
                ratio: 1.0,
                pendientes: Vec::new(),
                salida: Vec::new(),
            });
        }

        let ratio = SAMPLE_RATE as f64 / frecuencia_entrada as f64;
        let interno = FastFixedIn::<f32>::new(ratio, 1.0, PolynomialDegree::Septic, BLOQUE, 1)
            .map_err(|e| AudioError::Remuestreo(e.to_string()))?;

        Ok(Self {
            interno: Some(interno),
            ratio,
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

    /// Vacía lo que haya quedado en `pendientes` sin completar un bloque.
    ///
    /// `procesar` solo emite bloques completos: en un flujo continuo eso no
    /// importa, porque siempre llega más audio detrás. Pero al final de un
    /// archivo (por ejemplo al leer un WAV entero de una sola pasada, en vez
    /// de recibirlo en vivo) pueden quedar hasta `BLOQUE - 1` muestras —hasta
    /// 21 ms a 48 kHz— sin remuestrear, y nadie más va a pedirlas.
    ///
    /// `FastFixedIn` exige bloques de tamaño fijo, así que la única forma de
    /// remuestrear ese resto es rellenarlo con ceros hasta completar uno y
    /// procesarlo. Pero esos ceros también generan muestras de salida, y si
    /// no se recortaran se colaría un silencio artificial al final de cada
    /// archivo importado —poco, pero audible si varios clips se concatenan—.
    /// Por eso se recorta la salida al número de muestras que corresponde,
    /// según el ratio de remuestreo, a las muestras reales que había (no al
    /// bloque completo con relleno).
    pub fn vaciar(&mut self) -> Result<Vec<f32>> {
        let Some(r) = self.interno.as_mut() else {
            // A 16 kHz `procesar` no acumula nada en `pendientes`: no hay
            // cola que vaciar.
            return Ok(Vec::new());
        };

        if self.pendientes.is_empty() {
            return Ok(Vec::new());
        }

        let reales = self.pendientes.len();
        self.pendientes.resize(BLOQUE, 0.0);
        let bloque: Vec<f32> = self.pendientes.drain(..BLOQUE).collect();

        let procesado = r
            .process(&[bloque], None)
            .map_err(|e| AudioError::Remuestreo(e.to_string()))?;

        // Redondeo, no truncado: truncar sistemáticamente recortaría por
        // debajo (por ejemplo 1,9 muestras se convertirían en 1), perdiendo
        // un poco de señal real en cada archivo importado.
        let cantidad = (reales as f64 * self.ratio).round() as usize;

        Ok(procesado
            .first()
            .map(|canal| canal.iter().take(cantidad).copied().collect())
            .unwrap_or_default())
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
    fn vaciar_no_inventa_muestras_cuando_ya_venia_a_16k() {
        // A 16 kHz `procesar` no pasa por `pendientes` (devuelve la entrada
        // tal cual), así que `vaciar` no tiene nada que rellenar con ceros ni
        // que recortar: debe devolver un vector vacío, no un bloque de
        // silencio fabricado de la nada.
        let mut r = Remuestreador::nuevo(16_000).unwrap();
        r.procesar(&vec![0.5; 1000]).unwrap();
        assert!(r.vaciar().unwrap().is_empty());
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
    fn vaciar_recupera_la_cola_que_procesar_deja_sin_emitir() {
        // Antes de `vaciar`, las últimas muestras de un archivo (las que no
        // llegaban a completar un bloque de 1024) se quedaban para siempre en
        // `pendientes`: `procesar` nunca las devolvía y no había forma de
        // pedírselas. En un WAV importado eso es hasta 21 ms de audio real
        // que desaparecen en silencio, sin ningún error.
        let mut r = Remuestreador::nuevo(48_000).unwrap();

        // Un segundo y medio a 48 kHz: no es múltiplo de 1024, así que algo
        // queda pendiente tras `procesar`.
        let entrada: Vec<f32> = (0..72_000).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();

        let mut salida = r.procesar(&entrada).unwrap();
        assert!(r.pendientes() > 0, "la entrada era múltiplo del bloque");

        salida.extend(r.vaciar().unwrap());

        // 72 000 muestras a 48 kHz equivalen a ~24 000 a 16 kHz (un tercio).
        // La tolerancia cubre el redondeo del remuestreo, no una pérdida real.
        let esperado = 24_000;
        assert!(
            (salida.len() as i64 - esperado).abs() < 400,
            "salieron {} muestras, esperaba ~{esperado}",
            salida.len()
        );

        // Y la cola queda realmente vacía: nada se queda atascado dentro.
        assert_eq!(r.pendientes(), 0);
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
