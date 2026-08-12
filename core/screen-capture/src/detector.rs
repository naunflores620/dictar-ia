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
/// Subido de 10 a 14 tras probarlo contra una pantalla real: con 10, el reloj
/// del sistema, una notificación o el vídeo de la webcam ya bastaban para
/// cruzar el umbral, y salía una captura cada pocos segundos.
pub const UMBRAL_CAMBIO: u32 = 14;

/// Muestras seguidas que la imagen debe mantenerse igual antes de guardarla.
///
/// Es la regla que de verdad separa una diapositiva de todo lo demás: **una
/// lámina se queda quieta**. Un vídeo, una animación, un cursor parpadeando o
/// una página que se desplaza nunca producen tres capturas idénticas seguidas.
///
/// Con el muestreo a 2 s son 6 s de quietud. Una diapositiva que se enseña
/// menos que eso no da tiempo ni a leerla.
pub const CONFIRMACIONES: u32 = 3;

/// Tiempo mínimo entre láminas guardadas.
///
/// Un profesor no pasa dos diapositivas en cinco segundos. Este tope es la
/// última red: aunque el detector se confundiera, nunca puede llenar el disco
/// de capturas.
pub const MIN_ENTRE_LAMINAS_MS: i64 = 5_000;

/// Intentos fallidos tras los cuales se da por hecho que no hay diapositivas.
///
/// A 2 s por muestra son unos 40 s mirando algo que no se está quieto. Si en
/// ese tiempo no ha cuajado ni una lámina, lo que hay en pantalla son las
/// cámaras de los participantes. Rendirse evita gastar CPU y disco, y sobre
/// todo evita acabar guardando fotos de la gente de la reunión.
pub const INESTABLES_PARA_RENDIRSE: u32 = 20;

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
    /// Candidata a la espera de confirmación, con cuántas veces seguidas se ha
    /// visto igual.
    candidata: Option<(u64, u32)>,
    /// Todas las láminas guardadas, en orden.
    vistas: Vec<u64>,
    umbral: u32,
    confirmaciones: u32,
    /// Marca de tiempo de la última lámina guardada.
    ultima_ms: Option<i64>,
    /// Cuántas veces se ha descartado algo por no llegar a estabilizarse.
    /// Sirve para decirle al usuario que está mirando un vídeo, no una
    /// presentación.
    inestables: u32,
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
            confirmaciones: CONFIRMACIONES,
            ultima_ms: None,
            inestables: 0,
        }
    }

    pub fn con_umbral(umbral: u32) -> Self {
        Self {
            umbral,
            ..Self::nuevo()
        }
    }

    pub fn con_confirmaciones(mut self, n: u32) -> Self {
        self.confirmaciones = n.max(1);
        self
    }

    pub fn guardadas(&self) -> usize {
        self.vistas.len()
    }

    /// Cuántos cambios no llegaron a estabilizarse.
    ///
    /// Un número alto significa que la pantalla no está enseñando
    /// diapositivas sino algo en movimiento.
    pub fn inestables(&self) -> u32 {
        self.inestables
    }

    /// Si la pantalla parece vídeo en movimiento y no una presentación.
    ///
    /// Con muchos intentos que nunca se estabilizan y ninguna lámina guardada,
    /// lo que hay delante son participantes en cámara, no diapositivas. Sirve
    /// para dejar de capturar: seguir intentándolo gasta CPU y disco, y
    /// —peor— acaba guardando fotos de la gente de la reunión.
    pub fn parece_video(&self) -> bool {
        self.vistas.is_empty() && self.inestables >= INESTABLES_PARA_RENDIRSE
    }

    /// Procesa el hash de una captura, tomada en el instante `ts_ms`.
    pub fn observar_en(&mut self, phash: u64, ts_ms: i64) -> Decision {
        let Some(actual) = self.actual else {
            // La primera captura no se guarda de inmediato: se trata como
            // candidata igual que las demás. Si la sesión empieza con un vídeo
            // en pantalla, guardar el primer fotograma sería empezar el
            // álbum con basura.
            return self.acumular(phash, ts_ms);
        };

        if distancia(actual, phash) < self.umbral {
            // Sigue la misma lámina. Si había una candidata a medias, era una
            // transición o un parpadeo: se descarta.
            if self.candidata.take().is_some() {
                self.inestables += 1;
            }
            return Decision::SinCambio;
        }

        self.acumular(phash, ts_ms)
    }

    /// Compatibilidad: observa sin marca de tiempo, saltándose el tope mínimo
    /// entre láminas.
    pub fn observar(&mut self, phash: u64) -> Decision {
        let ts = self.ultima_ms.unwrap_or(0) + MIN_ENTRE_LAMINAS_MS;
        self.observar_en(phash, ts)
    }

    /// Cuenta las veces seguidas que se ve la misma imagen candidata, y la da
    /// por buena al llegar al número de confirmaciones.
    fn acumular(&mut self, phash: u64, ts_ms: i64) -> Decision {
        match self.candidata {
            Some((c, veces)) if distancia(c, phash) < self.umbral => {
                let veces = veces + 1;

                if veces < self.confirmaciones {
                    self.candidata = Some((c, veces));
                    return Decision::Esperando;
                }

                // Estable. Pero si acabamos de guardar una hace nada, esto no
                // es una diapositiva nueva: nadie pasa dos en cinco segundos.
                if let Some(ultima) = self.ultima_ms {
                    if ts_ms - ultima < MIN_ENTRE_LAMINAS_MS {
                        self.candidata = None;
                        return Decision::SinCambio;
                    }
                }

                self.actual = Some(phash);
                self.candidata = None;
                self.ultima_ms = Some(ts_ms);

                if let Some(i) = self.buscar_vista(phash) {
                    return Decision::Repetida { indice: i };
                }

                self.vistas.push(phash);
                Decision::Nueva { phash }
            }
            _ => {
                // Candidata nueva o distinta de la anterior: la imagen no se
                // está quieta. Empieza la cuenta otra vez.
                if self.candidata.is_some() {
                    self.inestables += 1;
                }
                self.candidata = Some((phash, 1));
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

    /// Observa el mismo hash hasta que el detector se pronuncia.
    ///
    /// Devuelve la primera decisión que no sea «esperando»: seguir observando
    /// después daría SinCambio, porque la lámina ya estaría aceptada.
    fn estabilizar(d: &mut Detector, h: u64, ts: i64) -> Decision {
        for _ in 0..CONFIRMACIONES + 1 {
            let dec = d.observar_en(h, ts);
            if dec != Decision::Esperando {
                return dec;
            }
        }
        Decision::Esperando
    }

    #[test]
    fn la_primera_captura_tambien_tiene_que_estabilizarse() {
        // Antes se guardaba sin confirmar, y en una reunión que empieza con
        // las cámaras de los participantes eso significaba guardar una foto
        // de la gente como si fuera la primera diapositiva.
        let mut d = Detector::nuevo();
        assert_eq!(d.observar_en(0xaaaa, 0), Decision::Esperando);
        assert_eq!(d.guardadas(), 0, "una sola muestra no basta");

        assert!(matches!(
            estabilizar(&mut d, 0xaaaa, 0),
            Decision::Nueva { .. }
        ));
        assert_eq!(d.guardadas(), 1);
    }

    #[test]
    fn el_video_de_los_participantes_no_genera_ni_una_lamina() {
        // El caso real que destapó el fallo: no había diapositiva, solo las
        // cámaras de los alumnos, y salía una captura cada pocos segundos.
        let mut d = Detector::nuevo();

        // Imagen distinta en cada muestra, como una cámara: nunca se queda
        // quieta el tiempo suficiente.
        for i in 0..60u64 {
            d.observar_en(lamina(i), i as i64 * 2_000);
        }

        assert_eq!(d.guardadas(), 0, "no debe guardar nada mirando vídeo");
        assert!(
            d.parece_video(),
            "debe darse cuenta de que no hay diapositivas"
        );
    }

    #[test]
    fn una_presentacion_de_verdad_no_se_confunde_con_video() {
        let mut d = Detector::nuevo();
        for i in 0..4u64 {
            // Cada lámina se mantiene un minuto, como en una clase.
            for k in 0..30 {
                d.observar_en(lamina(i), (i as i64 * 30 + k) * 2_000);
            }
        }
        assert_eq!(d.guardadas(), 4);
        assert!(!d.parece_video());
    }

    #[test]
    fn una_captura_casi_igual_no_genera_lamina() {
        let mut d = Detector::nuevo();
        estabilizar(&mut d, 0xaaaa_aaaa_aaaa_aaaa, 0);

        // Dos bits distintos: el cursor moviéndose, un vídeo de fondo.
        let casi = lejos_de(0xaaaa_aaaa_aaaa_aaaa, 2);
        assert_eq!(d.observar(casi), Decision::SinCambio);
        assert_eq!(d.guardadas(), 1);
    }

    #[test]
    fn hace_falta_que_la_imagen_se_quede_quieta() {
        // Es la regla que separa una diapositiva de todo lo demás: una lámina
        // no se mueve; un vídeo, una animación o un cursor, sí.
        let mut d = Detector::nuevo();
        estabilizar(&mut d, 0, 0);

        let otra = lejos_de(0, 25);
        assert_eq!(d.observar_en(otra, 10_000), Decision::Esperando);
        assert_eq!(d.guardadas(), 1, "una muestra no basta");

        assert_eq!(d.observar_en(otra, 12_000), Decision::Esperando);
        assert_eq!(d.guardadas(), 1, "dos tampoco");

        assert!(matches!(
            d.observar_en(otra, 14_000),
            Decision::Nueva { .. }
        ));
        assert_eq!(d.guardadas(), 2);
    }

    #[test]
    fn un_fotograma_intermedio_no_llega_a_guardarse() {
        let mut d = Detector::nuevo();
        estabilizar(&mut d, 0, 0);

        // Transición: un fotograma raro y luego la lámina de destino, que sí
        // se queda quieta.
        let intermedio = lejos_de(0, 40);
        let destino = lejos_de(0, 25);

        d.observar_en(intermedio, 10_000);
        estabilizar(&mut d, destino, 16_000);

        // Solo la primera y el destino: el intermedio nunca cuajó.
        assert_eq!(d.guardadas(), 2);
    }

    #[test]
    fn volver_a_una_lamina_anterior_no_la_duplica() {
        let mut d = Detector::nuevo();
        let a = 0x0000_0000_0000_0000u64;
        let b = lejos_de(a, 25);

        estabilizar(&mut d, a, 0);
        estabilizar(&mut d, b, 10_000);
        assert_eq!(d.guardadas(), 2);

        // El profesor vuelve atrás.
        let decision = estabilizar(&mut d, a, 20_000);

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
        let mut ts = 0i64;
        for i in 0..6u64 {
            for _ in 0..8 {
                d.observar_en(lamina(i), ts);
                ts += 2_000;
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
        let mut estricto = Detector::con_umbral(40);
        estabilizar(&mut estricto, 0, 0);

        let cambio_moderado = lejos_de(0, 20);
        assert_eq!(
            estricto.observar_en(cambio_moderado, 10_000),
            Decision::SinCambio,
            "con umbral 40, un cambio de 20 bits no debería contar"
        );
    }

    #[test]
    fn no_se_guardan_dos_laminas_en_cinco_segundos() {
        // Última red: aunque el detector se confundiera, nunca puede llenar el
        // disco. Un profesor no pasa dos diapositivas en cinco segundos.
        let mut d = Detector::nuevo();
        estabilizar(&mut d, 0, 0);

        let otra = lejos_de(0, 25);
        let decision = estabilizar(&mut d, otra, 3_000);

        assert_eq!(decision, Decision::SinCambio);
        assert_eq!(d.guardadas(), 1);
    }
}
