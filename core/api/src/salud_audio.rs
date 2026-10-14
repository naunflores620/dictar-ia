//! Salud del audio **mientras se graba**.
//!
//! Las métricas de captura (`capture_metrics.jsonl`, `dictar metricas`) existen
//! desde hace tiempo, pero se leen después de la clase. Que el loopback estaba
//! mudo, que el volumen era bajísimo o que el micrófono recogía los altavoces
//! se descubría cuando ya no tenía remedio. Este módulo hace esas mismas
//! comprobaciones en vivo y dice, solo cuando algo cambia, qué aviso aparece o
//! desaparece.
//!
//! Es lógica pura: no abre dispositivos, no manda eventos y no lee el reloj.
//! Quien la usa le pasa las tramas y el instante actual, así que se prueba con
//! un reloj sintético y se puede alimentar con WAV ya grabados (ver
//! `examples/salud_audio_eval.rs`).
//!
//! # Dos entradas, porque varios avisos son ausencia de datos
//!
//! `trama` recibe audio; `tick` solo avanza el reloj. Sin `tick`, «el
//! micrófono dejó de enviar audio» o «el loopback lleva 30 s mudo» no se
//! detectarían nunca, porque justo entonces no llega ninguna trama que
//! dispare la comprobación.
//!
//! # La línea de tiempo manda el `timestamp_ms`, no el orden de llegada
//!
//! El loopback de Windows no entrega paquetes en silencio y `sincronia.rs`
//! rellena al volver, así que contar tramas no sirve de reloj. Cada pista
//! guarda un anillo indexado por la posición absoluta `timestamp_ms × 16`, con
//! los huecos a cero, y el eco se evalúa sobre la misma ventana absoluta de las
//! dos pistas.

use dictar_audio::{AudioFrame, SAMPLE_RATE};
use dictar_domain::Track;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::eventos::Evento;

// ---------------------------------------------------------------------------
// Umbrales. Son iniciales: salen de las tres sesiones reales de octubre y de
// ninguna calibración más amplia, así que se ajustan con datos, no a ojo.
// ---------------------------------------------------------------------------

/// Muestras por milisegundo; el pipeline entero va a 16 kHz.
const MUESTRAS_POR_MS: i64 = SAMPLE_RATE as i64 / 1000;

/// Una «ventana activa» dura 30 ms y tiene RMS por encima de este umbral, el
/// mismo de casi silencio de `VentanaCaptura`. Mide **actividad**, no voz: un
/// ventilador también la supera.
const VENTANA_MUESTRAS: usize = SAMPLE_RATE as usize * 30 / 1000;
const RMS_ACTIVA: f64 = 0.003;
const SEGUNDOS_POR_VENTANA: f64 = 0.03;

/// Un anillo de 15 s más un segundo de margen: la ventana del eco llega hasta
/// 15 s atrás y la última trama puede ir por delante de `ahora` (un paquete de
/// relleno de WASAPI trae audio hasta su instante de recepción). El margen
/// evita que el comienzo de la ventana caiga fuera del anillo por ese desvío.
const CAPACIDAD_ANILLO: i64 = 16 * SAMPLE_RATE as i64;

/// Cuánto sin ventanas activas desde el arranque antes de avisar.
const SIN_SENAL_INICIO: Duration = Duration::from_secs(30);
/// Cuánto sin ventanas activas, tras haber tenido actividad, antes de avisar.
const SIN_SENAL_PAUSA: Duration = Duration::from_secs(180);
/// Un micrófono abierto entrega tramas siempre, aunque sea silencio.
const MIC_SIN_TRAMAS: Duration = Duration::from_secs(10);

const VENTANA_NIVEL_S: i64 = 10;
const MIN_ACTIVO_NIVEL_S: f64 = 3.0;
/// −38 dBFS.
const RMS_NIVEL_BAJO: f64 = 0.012_6;
/// −34 dBFS: la histéresis evita que un volumen justo en el borde parpadee.
const RMS_NIVEL_RECUPERADO: f64 = 0.019_95;

const VENTANA_RECORTE_S: i64 = 5;
const PICO_RECORTE: f32 = 0.99;
const PROPORCION_RECORTE: f64 = 0.001;
const CALMA_RECORTE: Duration = Duration::from_secs(10);

const PERIODO_METRICAS: Duration = Duration::from_secs(1);
const PERIODO_ECO: Duration = Duration::from_secs(5);
/// La ventana del eco termina medio segundo antes de `ahora`: lo último puede
/// no haber llegado todavía.
const ECO_COLA_MS: i64 = 500;
const ECO_VENTANA_S: i64 = 15;
/// Mínimos propios de este módulo. El de `analizar_retorno_acustico` (30
/// bloques, unos 0,6 s) es insuficiente para fiarse de una correlación.
const ECO_MIN_ACTIVO_SISTEMA_S: f64 = 5.0;
const ECO_MIN_ACTIVO_MIC_S: f64 = 3.0;
const ECO_CORRELACION_ALTA: f64 = 0.65;
const ECO_CORRELACION_BAJA: f64 = 0.55;
const ECO_SIN_DATOS: Duration = Duration::from_secs(30);

/// Un aviso que se desactiva no puede reactivarse antes de este tiempo.
const BLOQUEO_REACTIVACION: Duration = Duration::from_secs(15);

/// Un aviso que aparece (`activo`) o desaparece.
#[derive(Debug, Clone, PartialEq)]
pub struct CambioAviso {
    pub pista: Track,
    pub codigo: &'static str,
    pub activo: bool,
    /// Medida que lo disparó: segundos sin señal, dBFS, proporción recortada o
    /// correlación, según el código. Solo informativa.
    pub valor: f32,
    pub mensaje: String,
}

impl CambioAviso {
    pub fn a_evento(&self) -> Evento {
        Evento::Salud {
            pista: self.pista.as_str().to_owned(),
            codigo: self.codigo.to_owned(),
            activo: self.activo,
            valor: self.valor,
            mensaje: self.mensaje.clone(),
        }
    }
}

fn mensaje(pista: Track, codigo: &str) -> String {
    match codigo {
        "sin_senal_inicio" => "No llega audio del sistema. Comprueba que la clase suena en este \
                               equipo y por la salida predeterminada."
            .to_owned(),
        "sin_senal_pausa" => {
            "Hace más de 3 minutos que no suena nada en el equipo. Si la clase sigue, \
                              revisa la salida de audio."
                .to_owned()
        }
        "mic_sin_tramas" => {
            "El micrófono dejó de enviar audio. Revisa que siga conectado.".to_owned()
        }
        "nivel_bajo" => "El volumen de la clase es muy bajo; súbelo en la aplicación que la \
                         reproduce."
            .to_owned(),
        "recorte" => {
            let quien = match pista {
                Track::Mic => "El micrófono",
                Track::System => "El audio del sistema",
            };
            format!("{quien} satura; baja el volumen.")
        }
        "eco" => "El micrófono recoge lo que suena por los altavoces. Usa auriculares para \
                  evitar eco."
            .to_owned(),
        otro => otro.to_owned(),
    }
}

/// Audio de una pista, indexado por posición absoluta en muestras.
struct Pista {
    anillo: Vec<f32>,
    /// Posición justo tras la última muestra escrita.
    fin: i64,
    ultima_trama: Option<Instant>,
    ultima_activa: Option<Instant>,
}

impl Pista {
    fn nueva() -> Self {
        Self {
            anillo: vec![0.0; CAPACIDAD_ANILLO as usize],
            fin: 0,
            ultima_trama: None,
            ultima_activa: None,
        }
    }

    /// Escribe una trama que **termina** en `timestamp_ms`.
    ///
    /// Los backends no coinciden: en WASAPI el `timestamp_ms` se lee tras
    /// `GetBuffer`, así que es el final del bloque (y el paquete que reanuda
    /// tras un silencio trae `relleno + audio` acabando justo ahí); en PipeWire
    /// es el inicio. Sin `cfg` (el invariante 4 lo prohíbe), `max(fin, ts − len)`
    /// vale para los dos: con inicio de bloque nunca hay hueco que el máximo
    /// recorte, y con final de bloque el relleno cae en su sitio. Leerlo siempre
    /// como inicio colocaba el relleno de un hueco largo 45 s por delante y
    /// dejaba ciega la pista del sistema hasta que el reloj lo alcanzaba.
    ///
    /// El máximo con `fin` también implica que una trama atrasada se pega a
    /// continuación en vez de pisar lo ya escrito; no hay tramas fuera de orden
    /// dentro de una misma pista.
    fn escribir(&mut self, timestamp_ms: i64, pcm: &[f32]) {
        let ini = (timestamp_ms.max(0) * MUESTRAS_POR_MS - pcm.len() as i64).max(self.fin);
        let fin = ini + pcm.len() as i64;
        // Lo que no se entregó es silencio, no «lo que hubiera antes en esa
        // posición del anillo».
        if ini > self.fin {
            let desde = self.fin.max(ini - CAPACIDAD_ANILLO);
            for p in desde..ini {
                self.anillo[(p % CAPACIDAD_ANILLO) as usize] = 0.0;
            }
        }
        for (i, m) in pcm.iter().enumerate() {
            let p = ini + i as i64;
            if p < fin - CAPACIDAD_ANILLO {
                continue;
            }
            self.anillo[(p % CAPACIDAD_ANILLO) as usize] = if m.is_finite() { *m } else { 0.0 };
        }
        self.fin = fin;
    }

    /// Copia `[desde, hasta)`. Lo que no se ha escrito, o ya salió del anillo,
    /// vale cero.
    fn extraer(&self, desde: i64, hasta: i64) -> Vec<f32> {
        let desde = desde.max(0);
        (desde..hasta.max(desde))
            .map(|p| {
                if p >= self.fin || p < self.fin - CAPACIDAD_ANILLO {
                    0.0
                } else {
                    self.anillo[(p % CAPACIDAD_ANILLO) as usize]
                }
            })
            .collect()
    }
}

/// Ventanas de 30 ms con RMS por encima del umbral, y la energía de esas.
#[derive(Default)]
struct Actividad {
    ventanas_activas: usize,
    suma_cuadrados_activas: f64,
    muestras_activas: usize,
}

impl Actividad {
    fn medir(muestras: &[f32]) -> Self {
        let mut a = Actividad::default();
        for ventana in muestras.chunks(VENTANA_MUESTRAS) {
            // El resto final, más corto, no es una ventana completa.
            if ventana.len() < VENTANA_MUESTRAS {
                break;
            }
            let energia: f64 = ventana.iter().map(|m| f64::from(*m).powi(2)).sum();
            if (energia / VENTANA_MUESTRAS as f64).sqrt() > RMS_ACTIVA {
                a.ventanas_activas += 1;
                a.suma_cuadrados_activas += energia;
                a.muestras_activas += VENTANA_MUESTRAS;
            }
        }
        a
    }

    fn segundos(&self) -> f64 {
        self.ventanas_activas as f64 * SEGUNDOS_POR_VENTANA
    }

    fn rms(&self) -> f64 {
        if self.muestras_activas == 0 {
            return 0.0;
        }
        (self.suma_cuadrados_activas / self.muestras_activas as f64).sqrt()
    }
}

#[derive(Default)]
struct EstadoAviso {
    activo: bool,
    desactivado_en: Option<Instant>,
}

#[derive(Default)]
struct EstadoEco {
    altas_seguidas: u8,
    bajas_seguidas: u8,
    sin_datos_desde: Option<Instant>,
}

/// Evaluador de los avisos de salud de una sesión de grabación.
pub struct SaludAudio {
    inicio: Instant,
    capturar_sistema: bool,
    mic: Pista,
    sistema: Pista,
    avisos: HashMap<(Track, &'static str), EstadoAviso>,
    ultima_metrica: Instant,
    ultima_eval_eco: Instant,
    eco: EstadoEco,
    ultimo_recorte: HashMap<Track, Instant>,
}

impl SaludAudio {
    /// `inicio` es el inicio de la sesión, no la primera trama: «lleva 30 s sin
    /// señal» se cuenta desde que se pulsó grabar.
    pub fn nueva(inicio: Instant, capturar_sistema: bool) -> Self {
        Self {
            inicio,
            capturar_sistema,
            mic: Pista::nueva(),
            sistema: Pista::nueva(),
            avisos: HashMap::new(),
            ultima_metrica: inicio,
            ultima_eval_eco: inicio,
            eco: EstadoEco::default(),
            ultimo_recorte: HashMap::new(),
        }
    }

    /// Registra una trama ya escrita a disco. Solo evalúa los avisos de
    /// presencia (señal, micrófono), que dependen de cuándo llegó algo; las
    /// métricas periódicas esperan a `tick`.
    pub fn trama(&mut self, frame: &AudioFrame, ahora: Instant) -> Vec<CambioAviso> {
        let pista = match frame.track {
            Track::Mic => &mut self.mic,
            Track::System => &mut self.sistema,
        };
        pista.escribir(frame.timestamp_ms, &frame.pcm);
        pista.ultima_trama = Some(ahora);
        if hay_ventana_activa(&frame.pcm) {
            pista.ultima_activa = Some(ahora);
        }

        let mut cambios = Vec::new();
        self.reglas_de_presencia(ahora, &mut cambios);
        cambios
    }

    /// Avanza el reloj sin audio. Hay que llamarlo en cada vuelta del bucle,
    /// también cuando `recv_timeout` vence sin trama.
    pub fn tick(&mut self, ahora: Instant) -> Vec<CambioAviso> {
        let mut cambios = Vec::new();
        self.reglas_de_presencia(ahora, &mut cambios);

        if ahora.saturating_duration_since(self.ultima_metrica) >= PERIODO_METRICAS {
            self.ultima_metrica = ahora;
            self.evaluar_nivel(ahora, &mut cambios);
            self.evaluar_recorte(ahora, &mut cambios);
        }
        if self.capturar_sistema
            && ahora.saturating_duration_since(self.ultima_eval_eco) >= PERIODO_ECO
        {
            self.ultima_eval_eco = ahora;
            self.evaluar_eco(ahora, &mut cambios);
        }
        cambios
    }

    fn posicion(&self, ahora: Instant) -> i64 {
        ahora.saturating_duration_since(self.inicio).as_millis() as i64 * MUESTRAS_POR_MS
    }

    fn fijar(
        &mut self,
        pista: Track,
        codigo: &'static str,
        activo: bool,
        valor: f64,
        ahora: Instant,
        cambios: &mut Vec<CambioAviso>,
    ) {
        let estado = self.avisos.entry((pista, codigo)).or_default();
        if estado.activo == activo {
            return;
        }
        if activo {
            let reciente = estado
                .desactivado_en
                .is_some_and(|d| ahora.saturating_duration_since(d) < BLOQUEO_REACTIVACION);
            if reciente {
                return;
            }
        } else {
            estado.desactivado_en = Some(ahora);
        }
        estado.activo = activo;
        cambios.push(CambioAviso {
            pista,
            codigo,
            activo,
            valor: valor as f32,
            mensaje: mensaje(pista, codigo),
        });
    }

    /// Avisos que dependen de cuánto hace que llegó algo.
    fn reglas_de_presencia(&mut self, ahora: Instant, cambios: &mut Vec<CambioAviso>) {
        if self.capturar_sistema {
            match self.sistema.ultima_activa {
                None => {
                    let sin = ahora.saturating_duration_since(self.inicio);
                    self.fijar(
                        Track::System,
                        "sin_senal_inicio",
                        sin >= SIN_SENAL_INICIO,
                        sin.as_secs_f64(),
                        ahora,
                        cambios,
                    );
                }
                Some(ultima) => {
                    // Con actividad registrada, «nunca hubo señal» ya no es
                    // cierto; lo que corresponde es el aviso de pausa.
                    self.fijar(
                        Track::System,
                        "sin_senal_inicio",
                        false,
                        0.0,
                        ahora,
                        cambios,
                    );
                    let sin = ahora.saturating_duration_since(ultima);
                    self.fijar(
                        Track::System,
                        "sin_senal_pausa",
                        sin >= SIN_SENAL_PAUSA,
                        sin.as_secs_f64(),
                        ahora,
                        cambios,
                    );
                }
            }
        }

        let referencia = self.mic.ultima_trama.unwrap_or(self.inicio);
        let sin = ahora.saturating_duration_since(referencia);
        self.fijar(
            Track::Mic,
            "mic_sin_tramas",
            sin >= MIC_SIN_TRAMAS,
            sin.as_secs_f64(),
            ahora,
            cambios,
        );
    }

    fn evaluar_nivel(&mut self, ahora: Instant, cambios: &mut Vec<CambioAviso>) {
        // Solo el sistema: el ruido de sala del micrófono lo marcaría bajo toda
        // la clase y no hay un detector de voz propio con el que calibrarlo
        // (T-19).
        if !self.capturar_sistema {
            return;
        }
        let fin = self.posicion(ahora);
        let ventana = self
            .sistema
            .extraer(fin - VENTANA_NIVEL_S * SAMPLE_RATE as i64, fin);
        let actividad = Actividad::medir(&ventana);
        if actividad.segundos() < MIN_ACTIVO_NIVEL_S {
            return;
        }
        let rms = actividad.rms();
        let dbfs = 20.0 * rms.max(1e-9).log10();
        if rms < RMS_NIVEL_BAJO {
            self.fijar(Track::System, "nivel_bajo", true, dbfs, ahora, cambios);
        } else if rms >= RMS_NIVEL_RECUPERADO {
            self.fijar(Track::System, "nivel_bajo", false, dbfs, ahora, cambios);
        }
    }

    fn evaluar_recorte(&mut self, ahora: Instant, cambios: &mut Vec<CambioAviso>) {
        let fin = self.posicion(ahora);
        let pistas = [Track::Mic, Track::System];
        for track in pistas {
            if track == Track::System && !self.capturar_sistema {
                continue;
            }
            let pista = match track {
                Track::Mic => &self.mic,
                Track::System => &self.sistema,
            };
            let ventana = pista.extraer(fin - VENTANA_RECORTE_S * SAMPLE_RATE as i64, fin);
            let recortadas = ventana.iter().filter(|m| m.abs() >= PICO_RECORTE).count();
            let proporcion = recortadas as f64 / ventana.len().max(1) as f64;
            if proporcion > PROPORCION_RECORTE {
                self.ultimo_recorte.insert(track, ahora);
                self.fijar(track, "recorte", true, proporcion, ahora, cambios);
            } else if let Some(ultimo) = self.ultimo_recorte.get(&track).copied() {
                if ahora.saturating_duration_since(ultimo) >= CALMA_RECORTE {
                    self.fijar(track, "recorte", false, proporcion, ahora, cambios);
                }
            }
        }
    }

    fn evaluar_eco(&mut self, ahora: Instant, cambios: &mut Vec<CambioAviso>) {
        let pos = self.posicion(ahora);
        let fin = pos - ECO_COLA_MS * MUESTRAS_POR_MS;
        let ini = (pos - ECO_VENTANA_S * SAMPLE_RATE as i64).max(0);
        // Se copian las dos pistas una vez por evaluación, no en cada tick.
        let mic = self.mic.extraer(ini, fin);
        let sistema = self.sistema.extraer(ini, fin);

        let suficiente = Actividad::medir(&sistema).segundos() >= ECO_MIN_ACTIVO_SISTEMA_S
            && Actividad::medir(&mic).segundos() >= ECO_MIN_ACTIVO_MIC_S;
        let correlacion = if suficiente {
            dictar_audio::analizar_retorno_acustico(&mic, &sistema).map(|(c, _)| c)
        } else {
            None
        };

        let Some(c) = correlacion else {
            self.eco.altas_seguidas = 0;
            self.eco.bajas_seguidas = 0;
            let desde = *self.eco.sin_datos_desde.get_or_insert(ahora);
            if ahora.saturating_duration_since(desde) >= ECO_SIN_DATOS {
                self.fijar(Track::Mic, "eco", false, 0.0, ahora, cambios);
            }
            return;
        };
        self.eco.sin_datos_desde = None;

        if c >= ECO_CORRELACION_ALTA {
            self.eco.bajas_seguidas = 0;
            self.eco.altas_seguidas = self.eco.altas_seguidas.saturating_add(1);
            if self.eco.altas_seguidas >= 2 {
                self.fijar(Track::Mic, "eco", true, c, ahora, cambios);
            }
        } else if c < ECO_CORRELACION_BAJA {
            self.eco.altas_seguidas = 0;
            self.eco.bajas_seguidas = self.eco.bajas_seguidas.saturating_add(1);
            if self.eco.bajas_seguidas >= 2 {
                self.fijar(Track::Mic, "eco", false, c, ahora, cambios);
            }
        } else {
            // Zona intermedia: ni confirma ni desmiente, y rompe las rachas.
            self.eco.altas_seguidas = 0;
            self.eco.bajas_seguidas = 0;
        }
    }
}

/// ¿Alguna ventana de 30 ms de la trama supera el umbral de actividad? Los
/// restos de menos de 10 ms no cuentan: un par de muestras sueltas dan RMS
/// arbitrarios.
fn hay_ventana_activa(pcm: &[f32]) -> bool {
    const MINIMA: usize = VENTANA_MUESTRAS / 3;
    pcm.chunks(VENTANA_MUESTRAS)
        .filter(|v| v.len() >= MINIMA.min(pcm.len()))
        .any(|v| {
            let energia: f64 = v.iter().map(|m| f64::from(*m).powi(2)).sum();
            (energia / v.len() as f64).sqrt() > RMS_ACTIVA
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRAMA_MS: i64 = 20;
    const TRAMA_MUESTRAS: usize = 320;

    /// Ruido determinista de acceso aleatorio en [-1, 1].
    fn ruido(semilla: u64, i: i64) -> f32 {
        let mut z = (i as u64)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(semilla.wrapping_mul(0xD1B5_4A32_D192_ED03));
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 40) as f32 / (1u64 << 23) as f32 - 1.0
    }

    /// Envolvente silábica: suma de sinusoides de 3 a 6 Hz con fases y
    /// frecuencias propias de cada semilla. El ruido blanco no vale para
    /// probar el eco: su envolvente es plana y no se parece a nada.
    fn envolvente(semilla: u64, t_s: f64) -> f64 {
        let mut suma = 0.0;
        for k in 0..8u64 {
            let f = 3.0 + 3.0 * (ruido(semilla, k as i64 * 2).abs() as f64);
            let fase = std::f64::consts::TAU * (ruido(semilla, k as i64 * 2 + 1).abs() as f64);
            suma += (std::f64::consts::TAU * f * t_s + fase).sin();
        }
        (suma / 8f64.sqrt()).abs().min(1.5)
    }

    /// «Voz» sintética: portadora de ruido modulada por una envolvente silábica.
    fn voz(semilla: u64, amplitud: f32, muestra: i64) -> f32 {
        let t = muestra as f64 / f64::from(SAMPLE_RATE);
        ruido(semilla + 1000, muestra) * amplitud * envolvente(semilla, t) as f32
    }

    fn trama(track: Track, ts_ms: i64, f: &dyn Fn(i64) -> f32) -> AudioFrame {
        let base = ts_ms * MUESTRAS_POR_MS;
        AudioFrame {
            track,
            pcm: (0..TRAMA_MUESTRAS as i64).map(|i| f(base + i)).collect(),
            // Como WASAPI: el final del bloque.
            timestamp_ms: ts_ms + TRAMA_MS,
        }
    }

    /// Banco de pruebas: alimenta tramas de 20 ms con un reloj sintético y
    /// recoge los cambios con el segundo en que ocurrieron.
    struct Banco {
        salud: SaludAudio,
        inicio: Instant,
        cambios: Vec<(i64, CambioAviso)>,
    }

    impl Banco {
        fn nuevo(capturar_sistema: bool) -> Self {
            let inicio = Instant::now();
            Self {
                salud: SaludAudio::nueva(inicio, capturar_sistema),
                inicio,
                cambios: Vec::new(),
            }
        }

        fn ahora(&self, ms: i64) -> Instant {
            self.inicio + Duration::from_millis(ms as u64)
        }

        /// Entre `desde_ms` y `hasta_ms`, cada pista con su generador (o sin
        /// tramas si es `None`), y un `tick` tras cada paso, como el bucle de
        /// escritura.
        fn correr(
            &mut self,
            desde_ms: i64,
            hasta_ms: i64,
            mic: Option<&dyn Fn(i64) -> f32>,
            sistema: Option<&dyn Fn(i64) -> f32>,
        ) {
            let mut ts = desde_ms;
            while ts < hasta_ms {
                let ahora = self.ahora(ts + TRAMA_MS);
                let mut nuevos = Vec::new();
                if let Some(f) = mic {
                    nuevos.extend(self.salud.trama(&trama(Track::Mic, ts, f), ahora));
                }
                if let Some(f) = sistema {
                    nuevos.extend(self.salud.trama(&trama(Track::System, ts, f), ahora));
                }
                nuevos.extend(self.salud.tick(ahora));
                self.cambios
                    .extend(nuevos.into_iter().map(|c| (ts + TRAMA_MS, c)));
                ts += TRAMA_MS;
            }
        }

        /// Como `correr`, pero sin `tick`: para controlar cuándo se evalúa.
        fn cargar(
            &mut self,
            desde_ms: i64,
            hasta_ms: i64,
            mic: Option<&dyn Fn(i64) -> f32>,
            sistema: Option<&dyn Fn(i64) -> f32>,
        ) {
            let mut ts = desde_ms;
            while ts < hasta_ms {
                let ahora = self.ahora(ts + TRAMA_MS);
                if let Some(f) = mic {
                    self.salud.trama(&trama(Track::Mic, ts, f), ahora);
                }
                if let Some(f) = sistema {
                    self.salud.trama(&trama(Track::System, ts, f), ahora);
                }
                ts += TRAMA_MS;
            }
        }

        fn tick_en(&mut self, ms: i64) {
            let ahora = self.ahora(ms);
            let nuevos = self.salud.tick(ahora);
            self.cambios.extend(nuevos.into_iter().map(|c| (ms, c)));
        }

        fn activaciones(&self, codigo: &str) -> Vec<i64> {
            self.cambios
                .iter()
                .filter(|(_, c)| c.codigo == codigo && c.activo)
                .map(|(ms, _)| *ms)
                .collect()
        }

        fn desactivaciones(&self, codigo: &str) -> Vec<i64> {
            self.cambios
                .iter()
                .filter(|(_, c)| c.codigo == codigo && !c.activo)
                .map(|(ms, _)| *ms)
                .collect()
        }
    }

    fn silencio(_: i64) -> f32 {
        0.0
    }

    fn habla_normal(semilla: u64) -> impl Fn(i64) -> f32 {
        move |i| voz(semilla, 0.25, i)
    }

    // -- sin_senal_inicio / sin_senal_pausa -------------------------------

    #[test]
    fn sin_senal_inicio_salta_a_los_treinta_segundos_de_silencio_en_el_sistema() {
        let mut b = Banco::nuevo(true);
        let mic = habla_normal(1);
        // El loopback no entrega nada, como en Windows con la salida muda.
        b.correr(0, 29_000, Some(&mic), None);
        assert!(b.activaciones("sin_senal_inicio").is_empty());

        b.correr(29_000, 32_000, Some(&mic), None);
        let a = b.activaciones("sin_senal_inicio");
        assert_eq!(a.len(), 1);
        assert!((30_000..=30_200).contains(&a[0]), "saltó en {} ms", a[0]);
        // El caso vecino: sin actividad previa no es una «pausa».
        assert!(b.activaciones("sin_senal_pausa").is_empty());
    }

    #[test]
    fn sin_senal_inicio_se_va_con_la_primera_ventana_activa() {
        let mut b = Banco::nuevo(true);
        let mic = habla_normal(1);
        let sis = habla_normal(2);
        b.correr(0, 35_000, Some(&mic), None);
        assert_eq!(b.activaciones("sin_senal_inicio").len(), 1);

        b.correr(35_000, 37_000, Some(&mic), Some(&sis));
        let d = b.desactivaciones("sin_senal_inicio");
        assert_eq!(d.len(), 1);
        assert!(d[0] <= 36_000, "tardó hasta {} ms", d[0]);
    }

    #[test]
    fn el_silencio_digital_del_sistema_no_cuenta_como_senal() {
        // `sincronia.rs` rellena con ceros al volver: llegan tramas, pero
        // mudas. Contar tramas en vez de ventanas activas lo taparía.
        let mut b = Banco::nuevo(true);
        let mic = habla_normal(1);
        b.correr(0, 35_000, Some(&mic), Some(&silencio));
        assert_eq!(b.activaciones("sin_senal_inicio").len(), 1);
    }

    #[test]
    fn sin_senal_pausa_salta_a_los_tres_minutos_y_no_antes() {
        let mut b = Banco::nuevo(true);
        let mic = habla_normal(1);
        let sis = habla_normal(2);
        b.correr(0, 10_000, Some(&mic), Some(&sis));
        // Con actividad previa, 100 s de silencio no son aún una pausa, y en
        // ningún caso el aviso de «nunca llegó nada».
        b.correr(10_000, 150_000, Some(&mic), None);
        assert!(b.cambios.is_empty(), "cambios: {:?}", b.cambios);

        b.correr(150_000, 195_000, Some(&mic), None);
        assert!(b.activaciones("sin_senal_inicio").is_empty());
        let a = b.activaciones("sin_senal_pausa");
        assert_eq!(a.len(), 1);
        // La última ventana activa cae un poco antes de los 10 s.
        assert!((188_000..=190_500).contains(&a[0]), "saltó en {} ms", a[0]);

        b.correr(195_000, 196_000, Some(&mic), Some(&sis));
        assert_eq!(b.desactivaciones("sin_senal_pausa").len(), 1);
    }

    #[test]
    fn sin_capturar_sistema_la_falta_de_audio_del_sistema_no_avisa() {
        let mut b = Banco::nuevo(false);
        let mic = habla_normal(1);
        b.correr(0, 200_000, Some(&mic), None);
        assert!(b.activaciones("sin_senal_inicio").is_empty());
        assert!(b.activaciones("sin_senal_pausa").is_empty());
        assert!(b.activaciones("eco").is_empty());
    }

    // -- mic_sin_tramas ----------------------------------------------------

    #[test]
    fn mic_sin_tramas_necesita_el_tick_porque_no_llega_ninguna_trama() {
        let mut b = Banco::nuevo(true);
        let mic = habla_normal(1);
        let sis = habla_normal(2);
        b.correr(0, 5_000, Some(&mic), Some(&sis));
        // El micrófono se calla del todo: solo el sistema sigue entregando.
        b.correr(5_000, 14_000, None, Some(&sis));
        assert!(b.activaciones("mic_sin_tramas").is_empty());

        b.correr(14_000, 16_000, None, Some(&sis));
        let a = b.activaciones("mic_sin_tramas");
        assert_eq!(a.len(), 1);
        assert!((15_000..=15_200).contains(&a[0]), "saltó en {} ms", a[0]);

        b.correr(16_000, 17_000, Some(&mic), Some(&sis));
        assert_eq!(b.desactivaciones("mic_sin_tramas").len(), 1);
    }

    #[test]
    fn un_microfono_en_silencio_pero_abierto_no_dispara_mic_sin_tramas() {
        // Entregar tramas mudas es lo normal en un micrófono abierto.
        let mut b = Banco::nuevo(true);
        let sis = habla_normal(2);
        b.correr(0, 60_000, Some(&silencio), Some(&sis));
        assert!(b.activaciones("mic_sin_tramas").is_empty());
    }

    #[test]
    fn un_aviso_desactivado_no_se_reactiva_antes_de_quince_segundos() {
        let mut b = Banco::nuevo(true);
        let mic = habla_normal(1);
        let sis = habla_normal(2);
        b.correr(0, 5_000, Some(&mic), Some(&sis));
        b.correr(5_000, 16_000, None, Some(&sis));
        assert_eq!(b.activaciones("mic_sin_tramas").len(), 1);
        // El micrófono vuelve un instante y se va otra vez.
        b.correr(16_000, 16_200, Some(&mic), Some(&sis));
        let apagado = b.desactivaciones("mic_sin_tramas")[0];

        // A los 10 s sin tramas la regla ya se cumple, pero faltan 5 s para
        // que el bloqueo termine.
        b.correr(16_200, apagado + 14_000, None, Some(&sis));
        assert_eq!(b.activaciones("mic_sin_tramas").len(), 1, "parpadeó");

        b.correr(apagado + 14_000, apagado + 16_000, None, Some(&sis));
        let a = b.activaciones("mic_sin_tramas");
        assert_eq!(a.len(), 2);
        assert!(
            a[1] >= apagado + 15_000,
            "reactivó en {} ms",
            a[1] - apagado
        );
    }

    // -- nivel_bajo --------------------------------------------------------

    #[test]
    fn nivel_bajo_salta_con_el_sistema_a_menos_de_38_dbfs_y_se_va_al_subir() {
        let mut b = Banco::nuevo(true);
        let mic = habla_normal(1);
        // RMS global cercano a 0,005 (−46 dBFS) en las ventanas activas.
        let bajo = |i| voz(2, 0.012, i);
        b.correr(0, 15_000, Some(&mic), Some(&bajo));
        let a = b.activaciones("nivel_bajo");
        assert_eq!(a.len(), 1, "cambios: {:?}", b.cambios);
        assert!(a[0] <= 6_000, "tardó hasta {} ms", a[0]);

        let normal = habla_normal(2);
        b.correr(15_000, 40_000, Some(&mic), Some(&normal));
        assert_eq!(b.desactivaciones("nivel_bajo").len(), 1);
    }

    #[test]
    fn nivel_bajo_no_salta_con_una_clase_a_volumen_normal() {
        let mut b = Banco::nuevo(true);
        let mic = habla_normal(1);
        let sis = habla_normal(2);
        b.correr(0, 60_000, Some(&mic), Some(&sis));
        assert!(b.activaciones("nivel_bajo").is_empty());
    }

    #[test]
    fn nivel_bajo_pide_al_menos_tres_segundos_de_ventanas_activas() {
        // Un «sí» bajito en medio de un silencio no es una clase en voz baja.
        let mut b = Banco::nuevo(true);
        let mic = habla_normal(1);
        let sis = |i: i64| {
            if i < 2_000 * MUESTRAS_POR_MS {
                voz(2, 0.012, i)
            } else {
                0.0
            }
        };
        b.correr(0, 12_000, Some(&mic), Some(&sis));
        assert!(b.activaciones("nivel_bajo").is_empty());
    }

    #[test]
    fn nivel_bajo_no_se_aplica_al_microfono() {
        // Ruido de sala entre −50 y −38 dBFS: lo marcaría bajo toda la clase.
        let mut b = Banco::nuevo(true);
        let sis = habla_normal(2);
        let bajo = |i| voz(1, 0.012, i);
        b.correr(0, 60_000, Some(&bajo), Some(&sis));
        assert!(b.activaciones("nivel_bajo").is_empty());
        assert!(
            b.cambios.iter().all(|(_, c)| c.codigo != "nivel_bajo"),
            "cambios: {:?}",
            b.cambios
        );
    }

    #[test]
    fn el_ruido_de_sala_a_menos_45_dbfs_en_el_microfono_no_dispara_nada() {
        let mut b = Banco::nuevo(true);
        let sis = habla_normal(2);
        // −45 dBFS de RMS es 0,0056; el ruido uniforme tiene RMS de amp/√3.
        let sala = |i| ruido(9, i) * 0.0056 * 3f32.sqrt();
        b.correr(0, 90_000, Some(&sala), Some(&sis));
        assert!(b.cambios.is_empty(), "cambios: {:?}", b.cambios);
    }

    // -- recorte -----------------------------------------------------------

    #[test]
    fn recorte_salta_con_mas_de_una_milesima_de_muestras_saturadas() {
        let mut b = Banco::nuevo(true);
        let mic = habla_normal(1);
        // Una muestra de cada 100 clavada a 1,0: 1 %.
        let saturado = |i: i64| if i % 100 == 0 { 1.0 } else { voz(2, 0.25, i) };
        b.correr(0, 10_000, Some(&mic), Some(&saturado));
        let a = b.activaciones("recorte");
        assert_eq!(a.len(), 1, "cambios: {:?}", b.cambios);
        let (_, c) = b
            .cambios
            .iter()
            .find(|(_, c)| c.codigo == "recorte")
            .unwrap();
        assert_eq!(c.pista, Track::System);
        assert!(c.mensaje.contains("sistema"), "texto: {}", c.mensaje);
    }

    #[test]
    fn recorte_no_salta_con_un_pico_ocasional() {
        // Una muestra de cada 5000 (0,02 %) está por debajo del umbral.
        let mut b = Banco::nuevo(true);
        let mic = habla_normal(1);
        let casi = |i: i64| if i % 5_000 == 0 { 1.0 } else { voz(2, 0.25, i) };
        b.correr(0, 30_000, Some(&mic), Some(&casi));
        assert!(b.activaciones("recorte").is_empty());
    }

    #[test]
    fn recorte_se_va_tras_diez_segundos_sin_saturar() {
        let mut b = Banco::nuevo(true);
        let sis = habla_normal(2);
        let saturado = |i: i64| if i % 100 == 0 { 1.0 } else { voz(1, 0.25, i) };
        b.correr(0, 8_000, Some(&saturado), Some(&sis));
        let sat = b.activaciones("recorte");
        assert_eq!(sat.len(), 1);

        let mic = habla_normal(1);
        b.correr(8_000, 17_000, Some(&mic), Some(&sis));
        assert!(
            b.desactivaciones("recorte").is_empty(),
            "se fue demasiado pronto"
        );
        // La última saturación sigue dentro de la ventana de 5 s hasta los
        // 13 s, y desde ahí corren los 10 s de calma.
        b.correr(17_000, 25_000, Some(&mic), Some(&sis));
        assert_eq!(b.desactivaciones("recorte").len(), 1);
    }

    // -- eco ---------------------------------------------------------------

    /// Lo que el micrófono recoge del sistema: retrasado 200 ms y atenuado.
    fn eco_de(semilla: u64) -> impl Fn(i64) -> f32 {
        move |i| voz(semilla, 0.25, i - 200 * MUESTRAS_POR_MS) * 0.4
    }

    #[test]
    fn el_eco_se_detecta_cuando_el_microfono_repite_el_sistema_retrasado() {
        let mut b = Banco::nuevo(true);
        let sis = habla_normal(2);
        let mic = eco_de(2);
        b.correr(0, 60_000, Some(&mic), Some(&sis));
        let a = b.activaciones("eco");
        assert_eq!(a.len(), 1, "cambios: {:?}", b.cambios);
        assert!(a[0] <= 40_000, "tardó {} ms", a[0]);
        let (_, c) = b.cambios.iter().find(|(_, c)| c.codigo == "eco").unwrap();
        assert_eq!(c.pista, Track::Mic);
        assert!(c.valor >= 0.65, "correlación {}", c.valor);
    }

    #[test]
    fn dos_voces_distintas_con_ritmo_silabico_independiente_no_son_eco() {
        // El ruido blanco no vale como caso negativo: una envolvente plana
        // no correlaciona con nada y el test pasaría con cualquier umbral.
        let mut b = Banco::nuevo(true);
        let sis = habla_normal(2);
        let mic = habla_normal(7);
        b.correr(0, 120_000, Some(&mic), Some(&sis));
        assert!(b.activaciones("eco").is_empty(), "cambios: {:?}", b.cambios);
    }

    #[test]
    fn el_eco_necesita_dos_evaluaciones_seguidas_por_encima_del_umbral() {
        let mut b = Banco::nuevo(true);
        let sis = habla_normal(2);
        let eco = eco_de(2);
        // El sistema habla solo los primeros 6 s: material para una única
        // evaluación con datos suficientes.
        b.cargar(0, 6_000, Some(&eco), Some(&sis));
        b.cargar(6_000, 10_000, Some(&eco), None);
        b.tick_en(10_000);
        assert_eq!(b.salud.eco.altas_seguidas, 1, "la evaluación no fue alta");
        assert!(b.activaciones("eco").is_empty(), "saltó con una sola");

        // La siguiente evaluación ya no tiene datos del sistema: la racha se
        // rompe, y un eco aislado no se arrastra hasta la próxima.
        b.cargar(10_000, 25_000, Some(&eco), None);
        b.tick_en(25_000);
        assert_eq!(b.salud.eco.altas_seguidas, 0);
        assert!(b.activaciones("eco").is_empty());
    }

    #[test]
    fn el_eco_se_apaga_con_dos_evaluaciones_seguidas_bajas() {
        let mut b = Banco::nuevo(true);
        let sis = habla_normal(2);
        let eco = eco_de(2);
        b.correr(0, 40_000, Some(&eco), Some(&sis));
        assert_eq!(b.activaciones("eco").len(), 1);

        let propia = habla_normal(7);
        b.correr(40_000, 100_000, Some(&propia), Some(&sis));
        assert_eq!(
            b.desactivaciones("eco").len(),
            1,
            "cambios: {:?}",
            b.cambios
        );
    }

    #[test]
    fn el_eco_se_apaga_si_faltan_datos_durante_treinta_segundos() {
        let mut b = Banco::nuevo(true);
        let sis = habla_normal(2);
        let eco = eco_de(2);
        b.correr(0, 40_000, Some(&eco), Some(&sis));
        assert_eq!(b.activaciones("eco").len(), 1);

        // El sistema se calla: ya no hay con qué comparar.
        b.correr(40_000, 120_000, Some(&eco), None);
        assert_eq!(
            b.desactivaciones("eco").len(),
            1,
            "cambios: {:?}",
            b.cambios
        );
    }

    #[test]
    fn un_eco_sin_suficiente_sistema_activo_no_se_evalua() {
        // 4 s de sistema activo: por debajo del mínimo de 5 s, aunque la
        // correlación de esos 4 s sea perfecta.
        let mut b = Banco::nuevo(true);
        let sis = |i: i64| {
            if i < 4_000 * MUESTRAS_POR_MS {
                voz(2, 0.25, i)
            } else {
                0.0
            }
        };
        let eco = eco_de(2);
        b.correr(0, 30_000, Some(&eco), Some(&sis));
        assert!(b.activaciones("eco").is_empty());
    }

    #[test]
    fn el_eco_se_alinea_por_timestamp_y_no_por_orden_de_llegada() {
        // Si `escribir` ignorase `timestamp_ms` y pusiera cada trama a
        // continuación de la anterior, el hueco del loopback (que no entrega
        // paquetes en silencio) desaparecería de la línea de tiempo del
        // sistema y su voz posterior coincidiría con la del micrófono, que sí
        // es continua: un falso eco. Esta prueba falla en ese caso.
        //
        // El sistema suena 4 s, calla 7 s sin entregar **nada** (tampoco el
        // relleno de `sincronia.rs`, para que el orden de llegada no coincida
        // con el de los timestamps) y vuelve por donde iba, `C(t - 7 s)`. El
        // micrófono repite esa voz continua, retrasada 200 ms. Por llegada
        // coinciden; por timestamp la voz del sistema va 7 s por detrás de la
        // del micrófono, y eso no es eco.
        let continua = |i: i64| voz(2, 0.25, i);
        let sistema = move |i: i64| {
            if i < 4_000 * MUESTRAS_POR_MS {
                continua(i)
            } else {
                continua(i - 7_000 * MUESTRAS_POR_MS)
            }
        };
        let microfono = |i: i64| voz(2, 0.25, i - 200 * MUESTRAS_POR_MS) * 0.4;

        let mut b = Banco::nuevo(true);
        b.correr(0, 4_000, Some(&microfono), Some(&sistema));
        b.correr(4_000, 11_000, Some(&microfono), None);
        b.correr(11_000, 60_000, Some(&microfono), Some(&sistema));
        assert!(b.activaciones("eco").is_empty(), "cambios: {:?}", b.cambios);
    }

    // -- el anillo ---------------------------------------------------------

    #[test]
    fn un_hueco_sin_tramas_queda_a_cero_en_su_posicion_absoluta() {
        let mut p = Pista::nueva();
        p.escribir(200, &vec![0.5; 3_200]);
        // 1 s de hueco (16 000 muestras) y otra trama.
        p.escribir(1_400, &vec![0.25; 3_200]);
        let v = p.extraer(0, 22_400);
        assert_eq!(v[100], 0.5);
        assert_eq!(v[3_199], 0.5);
        assert!(v[3_200..19_200].iter().all(|m| *m == 0.0));
        assert_eq!(v[19_200], 0.25);
    }

    #[test]
    fn el_anillo_no_devuelve_audio_viejo_donde_hubo_un_hueco_largo() {
        // Con un hueco de más de 16 s, las posiciones del anillo conservan
        // audio de una vuelta anterior: tiene que quedar a cero.
        let mut p = Pista::nueva();
        p.escribir(1_000, &vec![0.5; 16_000]);
        p.escribir(30_020, &vec![0.25; 320]);
        let v = p.extraer(480_000 - 8_000, 480_000 + 320);
        assert!(v[..8_000].iter().all(|m| *m == 0.0));
        assert!(v[8_000..].iter().all(|m| *m == 0.25));
    }

    #[test]
    fn el_relleno_de_wasapi_tras_un_hueco_largo_no_deja_ciega_la_pista() {
        // Antes del arreglo `escribir` leía `timestamp_ms` como el inicio: el
        // paquete que reanuda el loopback (45 s de ceros más audio, con el
        // timestamp de su FINAL) se colocaba 45 s por delante, y las tramas
        // siguientes se descartaban como «demasiado antiguas» hasta que el
        // reloj lo alcanzaba.
        let mut p = Pista::nueva();
        p.escribir(20, &vec![0.5; 320]);
        let mut paquete = vec![0.0; 720_000 - 320];
        paquete.extend(vec![0.25; 320]);
        p.escribir(45_020, &paquete);
        assert_eq!(p.fin, 720_320);

        p.escribir(45_040, &vec![0.75; 320]);
        assert_eq!(p.fin, 720_640);
        assert!(p.extraer(720_320, 720_640).iter().all(|m| *m == 0.75));
        assert!(p.extraer(720_000, 720_320).iter().all(|m| *m == 0.25));
    }

    #[test]
    fn una_trama_con_timestamp_atrasado_se_pega_a_continuacion_sin_pisar() {
        let mut p = Pista::nueva();
        p.escribir(20, &vec![0.25; 320]);
        p.escribir(20, &vec![0.9; 320]);
        assert_eq!(p.fin, 640);
        assert!(p.extraer(0, 320).iter().all(|m| *m == 0.25));
    }

    #[test]
    fn un_cambio_se_convierte_en_el_evento_que_cruza_el_puente() {
        let c = CambioAviso {
            pista: Track::Mic,
            codigo: "eco",
            activo: true,
            valor: 0.7,
            mensaje: mensaje(Track::Mic, "eco"),
        };
        let Evento::Salud {
            pista,
            codigo,
            activo,
            ..
        } = c.a_evento()
        else {
            panic!("tipo equivocado");
        };
        assert_eq!(pista, "mic");
        assert_eq!(codigo, "eco");
        assert!(activo);
    }
}
