//! Decide cuándo una captura es una diapositiva nueva.
//!
//! Es la parte que separa una función útil de una que molesta. Tres reglas, y
//! cada una nació de un modo de fallo concreto:
//!
//! 1. **Umbral sobre la región central**, no sobre la pantalla entera: la
//!    miniatura de la webcam del profesor cambia sin parar y generaría una
//!    lámina por segundo.
//! 2. **Confirmación en dos muestras seguidas**: las transiciones y las
//!    animaciones de PowerPoint producen fotogramas intermedios que no son
//!    ninguna diapositiva.
//! 3. **Deduplicación contra todo lo visto**: cuando el profesor vuelve atrás
//!    a una lámina anterior, se referencia la que ya existe en vez de guardar
//!    la misma imagen dos veces.

use crate::hash::distancia;
use dictar_domain::TsMs;

/// Bits de diferencia a partir de los cuales se considera otra diapositiva.
///
/// Con 64 bits de hash, 10 es un cambio claro de estructura. Más bajo y las
/// animaciones cuentan como láminas; más alto y se pierden las diapositivas
/// que solo cambian un párrafo.
pub const UMBRAL_CAMBIO: u32 = 10;

/// Distancia por debajo de la cual se considera la **misma** lámina ya vista.
///
/// Es más estricto que el umbral de cambio a propósito: para decir «esto es
/// nueva» basta un cambio claro, pero para decir «esto ya lo tengo» hay que
/// estar bastante seguro, o se descartarían láminas parecidas pero distintas.
pub const UMBRAL_DUPLICADO: u32 = 4;

#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// Sigue la misma lámina.
    SinCambio,
    /// Cambió, pero falta confirmarlo en la siguiente muestra.
    Esperando,
    /// Diapositiva nueva: hay que guardarla.
    Nueva { phash: u64 },
    /// El profesor volvió a una lámina ya guardada.
    Repetida { indice: usize },
}

pub struct Detector {
    /// Hash de la lámina que se da por buena ahora mismo.
    actual: Option<u64>,
    /// Candidata a la espera de confirmación.
    candidata: Option<u64>,
    /// Todas las láminas guardadas, en orden.
    vistas: Vec<u64>,
    umbral: u32,
}

impl Default for Detector {
    fn default() -> Self {
        Self::nuevo()
    }
}

impl Detector {
    pub fn nuevo() -> Self {
        Self {
            actual: None,
            candidata: None,
            vistas: Vec::new(),
            umbral: UMBRAL_CAMBIO,
        }
    }

    pub fn con_umbral(umbral: u32) -> Self {
        Self {
            umbral,
            ..Self::nuevo()
        }
    }

    pub fn guardadas(&self) -> usize {
        self.vistas.len()
    }

    /// Procesa el hash de una captura.
    pub fn observar(&mut self, phash: u64) -> Decision {
        let Some(actual) = self.actual else {
            // La primera captura de la sesión se guarda sin confirmar: no hay
            // con qué compararla, y perder la primera lámina sería peor que
            // guardar una de más.
            self.actual = Some(phash);
            self.vistas.push(phash);
            return Decision::Nueva { phash };
        };

        if distancia(actual, phash) < self.umbral {
            self.candidata = None;
            return Decision::SinCambio;
        }

        // Cambió respecto a la actual. ¿Ya lo habíamos visto en esta muestra?
        match self.candidata {
            Some(c) if distancia(c, phash) < self.umbral => {
                // Confirmado: dos muestras seguidas dicen lo mismo.
                self.actual = Some(phash);
                self.candidata = None;

                if let Some(i) = self.buscar_vista(phash) {
                    return Decision::Repetida { indice: i };
                }

                self.vistas.push(phash);
                Decision::Nueva { phash }
            }
            _ => {
                // Primer aviso: puede ser una transición. Se espera.
                self.candidata = Some(phash);
                Decision::Esperando
            }
        }
    }

    fn buscar_vista(&self, phash: u64) -> Option<usize> {
        self.vistas
            .iter()
            .position(|v| distancia(*v, phash) <= UMBRAL_DUPLICADO)
    }
}

/// Diapositiva lista para guardarse.
#[derive(Debug, Clone)]
pub struct Diapositiva {
    pub ts_ms: TsMs,
    pub phash: u64,
    pub ruta: std::path::PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hashes con la distancia que se pida respecto a otro.
    fn lejos_de(base: u64, bits: u32) -> u64 {
        let mut h = base;
        for i in 0..bits {
            h ^= 1 << i;
        }
        h
    }

    #[test]
    fn la_primera_captura_siempre_se_guarda() {
        // No hay con qué compararla, y perder la primera lámina de la clase
        // sería peor que guardar una de más.
        let mut d = Detector::nuevo();
        assert!(matches!(d.observar(0xaaaa), Decision::Nueva { .. }));
        assert_eq!(d.guardadas(), 1);
    }

    #[test]
    fn una_captura_casi_igual_no_genera_lamina() {
        let mut d = Detector::nuevo();
        d.observar(0xaaaa_aaaa_aaaa_aaaa);

        // Dos bits distintos: el cursor moviéndose, un vídeo de fondo.
        let casi = lejos_de(0xaaaa_aaaa_aaaa_aaaa, 2);
        assert_eq!(d.observar(casi), Decision::SinCambio);
        assert_eq!(d.guardadas(), 1);
    }

    #[test]
    fn hace_falta_confirmar_el_cambio_en_dos_muestras() {
        // Las transiciones de PowerPoint producen fotogramas intermedios que
        // no son ninguna diapositiva.
        let mut d = Detector::nuevo();
        d.observar(0x0000_0000_0000_0000);

        let otra = lejos_de(0, 20);
        assert_eq!(d.observar(otra), Decision::Esperando);
        assert_eq!(d.guardadas(), 1, "todavía no se guarda");

        assert!(matches!(d.observar(otra), Decision::Nueva { .. }));
        assert_eq!(d.guardadas(), 2);
    }

    #[test]
    fn un_fotograma_intermedio_no_llega_a_guardarse() {
        let mut d = Detector::nuevo();
        d.observar(0);

        // Transición: un fotograma raro, y luego la lámina de destino.
        let intermedio = lejos_de(0, 30);
        let destino = lejos_de(0, 20);

        assert_eq!(d.observar(intermedio), Decision::Esperando);
        assert_eq!(d.observar(destino), Decision::Esperando);
        assert!(matches!(d.observar(destino), Decision::Nueva { .. }));

        // Solo la primera y el destino: el intermedio nunca se guardó.
        assert_eq!(d.guardadas(), 2);
    }

    #[test]
    fn volver_a_una_lamina_anterior_no_la_duplica() {
        let mut d = Detector::nuevo();
        let a = 0x0000_0000_0000_0000u64;
        let b = lejos_de(a, 20);

        d.observar(a);
        d.observar(b);
        d.observar(b); // confirmada
        assert_eq!(d.guardadas(), 2);

        // El profesor vuelve atrás.
        d.observar(a);
        let decision = d.observar(a);

        assert!(
            matches!(decision, Decision::Repetida { indice: 0 }),
            "decisión: {decision:?}"
        );
        assert_eq!(
            d.guardadas(),
            2,
            "no debe guardar la misma imagen dos veces"
        );
    }

    /// Hashes bien repartidos, como los de diapositivas realmente distintas.
    ///
    /// Multiplicar por la proporción áurea dispersa los bits: valores
    /// consecutivos quedan a unos 30 bits, muy por encima del umbral.
    fn lamina(i: u64) -> u64 {
        (i + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15)
    }

    #[test]
    fn una_secuencia_de_clase_produce_una_lamina_por_diapositiva() {
        // El fixture se valida a sí mismo: si las láminas de prueba no fueran
        // realmente distintas, el test pasaría o fallaría por el motivo
        // equivocado.
        for i in 0..6u64 {
            for j in (i + 1)..6 {
                let d = distancia(lamina(i), lamina(j));
                assert!(
                    d > UMBRAL_CAMBIO,
                    "las láminas {i} y {j} distan solo {d} bits"
                );
            }
        }

        let mut d = Detector::nuevo();

        // Seis diapositivas, cada una vista varias veces: el muestreo va cada
        // dos segundos y una lámina dura minutos.
        for i in 0..6u64 {
            for _ in 0..8 {
                d.observar(lamina(i));
            }
        }

        assert_eq!(
            d.guardadas(),
            6,
            "una lámina por diapositiva, ni más ni menos"
        );
    }

    #[test]
    fn el_umbral_es_configurable_para_afinarlo_con_clases_reales() {
        let mut estricto = Detector::con_umbral(30);
        estricto.observar(0);

        let cambio_moderado = lejos_de(0, 15);
        estricto.observar(cambio_moderado);
        assert_eq!(
            estricto.observar(cambio_moderado),
            Decision::SinCambio,
            "con umbral 30, un cambio de 15 bits no debería contar"
        );
    }
}
