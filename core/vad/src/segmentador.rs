//! Troceado del flujo continuo en segmentos transcribibles.
//!
//! Cortar cada N segundos a ciegas parte palabras por la mitad y degrada la
//! transcripción de forma apreciable. Aquí se busca una frontera natural: se
//! acumula hasta la duración objetivo y luego se cierra en la **primera pausa**
//! que aparezca.

use crate::{Vad, MUESTRAS_VENTANA};
use dictar_audio::SAMPLE_RATE;
use dictar_domain::{Track, TsMs};

/// Duración a partir de la cual se busca dónde cerrar.
pub const OBJETIVO_MS: i64 = 25_000;

/// Corte forzado. Existe para el profesor que encadena diez minutos sin una
/// pausa clara: sin este tope, el segmento crecería hasta agotar la memoria y
/// la transcripción en vivo dejaría de llegar.
pub const MAXIMO_MS: i64 = 45_000;

/// Silencio que cierra un segmento.
pub const PAUSA_CIERRE_MS: i64 = 500;

/// Solape entre segmentos consecutivos.
///
/// Sin él, una palabra a caballo entre dos segmentos se pierde en ambos: el
/// primero la ve empezar sin terminar y el segundo la ve terminar sin empezar.
pub const SOLAPE_MS: i64 = 2_000;

#[derive(Debug, Clone)]
pub struct Segmento {
    pub track: Track,
    pub inicio_ms: TsMs,
    pub fin_ms: TsMs,
    pub pcm: Vec<f32>,
    /// Proporción del segmento con voz. Por debajo de un mínimo no merece la
    /// pena transcribirlo.
    pub proporcion_voz: f32,
}

impl Segmento {
    pub fn duracion_ms(&self) -> i64 {
        self.fin_ms - self.inicio_ms
    }

    /// Si vale la pena mandarlo a Whisper.
    ///
    /// Transcribir treinta segundos de silencio cuesta lo mismo que treinta de
    /// habla y solo puede producir alucinaciones, así que se descarta.
    pub fn merece_transcribirse(&self) -> bool {
        self.proporcion_voz > 0.05 && self.duracion_ms() > 300
    }
}

/// Acumula audio de una pista y va soltando segmentos.
pub struct Segmentador {
    track: Track,
    vad: Vad,
    buffer: Vec<f32>,
    /// Marca de tiempo de la primera muestra del buffer.
    inicio_ms: TsMs,
    /// Ventanas de silencio seguidas al final del buffer.
    silencio_seguido: u32,
    ventanas_con_voz: u32,
    ventanas_totales: u32,
    /// Muestras pendientes de completar una ventana de análisis.
    resto: Vec<f32>,
}

impl Segmentador {
    pub fn nuevo(track: Track) -> Self {
        Self {
            track,
            vad: Vad::nuevo(),
            buffer: Vec::with_capacity(SAMPLE_RATE as usize * 30),
            inicio_ms: 0,
            silencio_seguido: 0,
            ventanas_con_voz: 0,
            ventanas_totales: 0,
            resto: Vec::with_capacity(MUESTRAS_VENTANA),
        }
    }

    pub fn presencial(track: Track) -> Self {
        Self {
            vad: Vad::presencial(),
            ..Self::nuevo(track)
        }
    }

    fn duracion_buffer_ms(&self) -> i64 {
        (self.buffer.len() as i64 * 1000) / SAMPLE_RATE as i64
    }

    /// Añade audio y devuelve los segmentos que hayan quedado cerrados.
    pub fn empujar(&mut self, pcm: &[f32], timestamp_ms: TsMs) -> Vec<Segmento> {
        if self.buffer.is_empty() && self.resto.is_empty() {
            self.inicio_ms = timestamp_ms;
        }

        // El VAD trabaja en ventanas exactas; lo que sobra espera al siguiente
        // bloque en vez de analizarse a medias.
        self.resto.extend_from_slice(pcm);
        let completas = self.resto.len() / MUESTRAS_VENTANA;

        let mut salida = Vec::new();

        for i in 0..completas {
            let ventana: Vec<f32> =
                self.resto[i * MUESTRAS_VENTANA..(i + 1) * MUESTRAS_VENTANA].to_vec();

            let hay_voz = self.vad.procesar(&ventana);
            self.ventanas_totales += 1;
            if hay_voz {
                self.ventanas_con_voz += 1;
            }

            // La pausa se mide con la decisión cruda y no con la suavizada: el
            // hangover existe para no cortar finales de palabra dentro de un
            // segmento, pero sumarlo a la pausa de cierre exigiría 1,1 s de
            // silencio para cerrar en vez de los 500 ms previstos.
            if self.vad.voz_bruta() {
                self.silencio_seguido = 0;
            } else {
                self.silencio_seguido += 1;
            }

            self.buffer.extend_from_slice(&ventana);

            // La comprobación va aquí, ventana a ventana, y no al final del
            // bloque: PipeWire puede entregar un bloque grande, y comprobar
            // solo después haría que el corte forzado se pasara de largo por
            // el tamaño del bloque en vez de respetar el tope.
            if let Some(s) = self.intentar_cerrar() {
                salida.push(s);
            }
        }

        self.resto.drain(..completas * MUESTRAS_VENTANA);
        salida
    }

    fn intentar_cerrar(&mut self) -> Option<Segmento> {
        let dur = self.duracion_buffer_ms();

        let pausa_ms = (self.silencio_seguido as i64) * crate::VENTANA_MS as i64;
        let por_pausa = dur >= OBJETIVO_MS && pausa_ms >= PAUSA_CIERRE_MS;
        let por_tope = dur >= MAXIMO_MS;

        if !por_pausa && !por_tope {
            return None;
        }

        Some(self.cortar(dur))
    }

    fn cortar(&mut self, dur_ms: i64) -> Segmento {
        let proporcion = if self.ventanas_totales == 0 {
            0.0
        } else {
            self.ventanas_con_voz as f32 / self.ventanas_totales as f32
        };

        let seg = Segmento {
            track: self.track,
            inicio_ms: self.inicio_ms,
            fin_ms: self.inicio_ms + dur_ms,
            pcm: self.buffer.clone(),
            proporcion_voz: proporcion,
        };

        // Se conserva la cola como principio del siguiente segmento: es el
        // solape que impide perder las palabras de la costura.
        let muestras_solape =
            ((SOLAPE_MS * SAMPLE_RATE as i64) / 1000).min(self.buffer.len() as i64) as usize;
        let corte = self.buffer.len() - muestras_solape;

        self.buffer.drain(..corte);
        self.inicio_ms += (corte as i64 * 1000) / SAMPLE_RATE as i64;
        self.silencio_seguido = 0;
        self.ventanas_con_voz = 0;
        self.ventanas_totales = 0;

        seg
    }

    /// Cierra el segmento en curso al terminar la sesión.
    ///
    /// Sin esto se perdería lo último que se dijo, que suele ser justamente el
    /// resumen del profesor o el acuerdo final con el cliente.
    pub fn finalizar(&mut self) -> Option<Segmento> {
        if !self.resto.is_empty() {
            let resto = std::mem::take(&mut self.resto);
            self.buffer.extend_from_slice(&resto);
            self.ventanas_totales += 1;
        }

        if self.buffer.is_empty() {
            return None;
        }

        let dur = self.duracion_buffer_ms();
        let proporcion = if self.ventanas_totales == 0 {
            0.0
        } else {
            self.ventanas_con_voz as f32 / self.ventanas_totales as f32
        };

        let seg = Segmento {
            track: self.track,
            inicio_ms: self.inicio_ms,
            fin_ms: self.inicio_ms + dur,
            pcm: std::mem::take(&mut self.buffer),
            proporcion_voz: proporcion,
        };

        self.ventanas_con_voz = 0;
        self.ventanas_totales = 0;
        Some(seg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tono(ms: i64, amplitud: f32) -> Vec<f32> {
        let n = (SAMPLE_RATE as i64 * ms / 1000) as usize;
        (0..n).map(|i| (i as f32 * 0.15).sin() * amplitud).collect()
    }

    /// Señal con forma de habla: portadora modulada a ritmo silábico (~4 Hz)
    /// con pausas breves entre «frases».
    ///
    /// Un tono constante no sirve como fixture: para un detector basado en el
    /// mínimo del fondo, un zumbido sostenido **es** ruido, y con razón. El
    /// habla real siempre baja de nivel entre sílabas, y es justo esa
    /// modulación la que el detector aprovecha.
    fn habla(ms: i64, amplitud: f32) -> Vec<f32> {
        let n = (SAMPLE_RATE as i64 * ms / 1000) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / SAMPLE_RATE as f32;
                // Pausa de 250 ms cada 2,5 s, como entre frases.
                if (t % 2.5) > 2.25 {
                    return 0.0005;
                }
                let envolvente = 0.5 + 0.5 * (t * 4.0 * std::f32::consts::TAU).sin().abs();
                (i as f32 * 0.15).sin() * amplitud * envolvente
            })
            .collect()
    }

    fn silencio(ms: i64) -> Vec<f32> {
        vec![0.0; (SAMPLE_RATE as i64 * ms / 1000) as usize]
    }

    #[test]
    fn un_audio_corto_no_cierra_ningun_segmento() {
        let mut s = Segmentador::nuevo(Track::System);
        assert!(s.empujar(&tono(5_000, 0.3), 0).is_empty());
    }

    #[test]
    fn se_cierra_en_la_pausa_tras_alcanzar_el_objetivo() {
        let mut s = Segmentador::nuevo(Track::System);

        // Un segundo de ambiente antes de hablar, como en una clase real: el
        // detector necesita ver el fondo para saber qué es voz.
        s.empujar(&vec![0.001; 16_000], 0);

        // 26 s hablando: pasa del objetivo pero sin pausa, no debe cerrar.
        assert!(s.empujar(&habla(26_000, 0.3), 1_000).is_empty());

        // Llega la pausa y ahí sí.
        let segs = s.empujar(&silencio(800), 27_000);
        assert_eq!(segs.len(), 1, "debería haberse cerrado en la pausa");
        assert!(segs[0].duracion_ms() >= OBJETIVO_MS);
    }

    #[test]
    fn el_tope_corta_aunque_no_haya_pausa() {
        // Un profesor que encadena diez minutos sin pausa no puede hacer crecer
        // el buffer indefinidamente.
        let mut s = Segmentador::nuevo(Track::System);
        let segs = s.empujar(&habla(50_000, 0.3), 0);

        assert_eq!(segs.len(), 1);
        assert!(
            segs[0].duracion_ms() >= MAXIMO_MS && segs[0].duracion_ms() < MAXIMO_MS + 1000,
            "duración {}",
            segs[0].duracion_ms()
        );
    }

    #[test]
    fn los_segmentos_consecutivos_se_solapan() {
        let mut s = Segmentador::nuevo(Track::System);
        s.empujar(&vec![0.001; 16_000], 0);

        s.empujar(&habla(26_000, 0.3), 1_000);
        let mut cerrados = s.empujar(&silencio(800), 27_000);
        assert_eq!(cerrados.len(), 1, "no se cerró el primer segmento");
        let primero = cerrados.remove(0);

        s.empujar(&habla(26_000, 0.3), 27_800);
        let mut cerrados = s.empujar(&silencio(800), 53_800);
        assert_eq!(cerrados.len(), 1, "no se cerró el segundo segmento");
        let segundo = cerrados.remove(0);

        // El segundo empieza antes de que acabe el primero: esas dos palabras
        // de más son lo que evita perder la de la costura.
        assert!(
            segundo.inicio_ms < primero.fin_ms,
            "primero acaba en {} y el segundo empieza en {}",
            primero.fin_ms,
            segundo.inicio_ms
        );
        let solape = primero.fin_ms - segundo.inicio_ms;
        assert!((solape - SOLAPE_MS).abs() < 200, "solape de {solape} ms");
    }

    #[test]
    fn un_segmento_de_puro_silencio_no_merece_transcribirse() {
        // Es la comprobación que evita las alucinaciones: Whisper, ante
        // silencio, se inventa frases.
        let mut s = Segmentador::nuevo(Track::System);
        s.empujar(&silencio(50_000), 0);
        let seg = s.finalizar().unwrap();

        assert!(seg.proporcion_voz < 0.05);
        assert!(!seg.merece_transcribirse());
    }

    #[test]
    fn un_segmento_hablado_si_merece_transcribirse() {
        let mut s = Segmentador::nuevo(Track::System);
        // Ambiente para fijar el suelo de ruido, y luego voz.
        s.empujar(&vec![0.001; 16_000], 0);
        s.empujar(&habla(20_000, 0.3), 1_000);

        let seg = s.finalizar().unwrap();
        assert!(seg.merece_transcribirse(), "voz {}", seg.proporcion_voz);
    }

    #[test]
    fn al_finalizar_se_emite_lo_que_quedaba() {
        // Lo último que se dice suele ser el resumen del profesor o el acuerdo
        // final con el cliente: perderlo sería lo peor.
        let mut s = Segmentador::nuevo(Track::System);
        s.empujar(&tono(3_000, 0.3), 0);

        let seg = s.finalizar().expect("debe quedar un segmento pendiente");
        assert!(seg.duracion_ms() >= 2_900);
    }

    #[test]
    fn finalizar_dos_veces_no_duplica_audio() {
        let mut s = Segmentador::nuevo(Track::System);
        s.empujar(&tono(3_000, 0.3), 0);
        assert!(s.finalizar().is_some());
        assert!(s.finalizar().is_none());
    }

    #[test]
    fn la_marca_de_tiempo_del_segmento_arranca_donde_toca() {
        let mut s = Segmentador::nuevo(Track::Mic);
        // La sesión ya llevaba diez minutos cuando llegó este audio.
        s.empujar(&tono(3_000, 0.3), 600_000);

        let seg = s.finalizar().unwrap();
        assert_eq!(seg.inicio_ms, 600_000);
        assert_eq!(seg.track, Track::Mic);
    }

    #[test]
    fn las_muestras_sueltas_no_se_pierden_entre_llamadas() {
        // Los bloques de PipeWire no son múltiplos de la ventana de análisis.
        let mut s = Segmentador::nuevo(Track::System);
        for i in 0..100 {
            s.empujar(&tono(37, 0.3), i * 37);
        }
        let seg = s.finalizar().unwrap();
        // 100 bloques de 37 ms ≈ 3,7 s; se admite algo de pérdida por redondeo.
        assert!(seg.duracion_ms() > 3_500, "duración {}", seg.duracion_ms());
    }
}
