//! Captura de audio en dos pistas.
//!
//! La decisión que ordena todo este crate: **el micrófono y la salida del
//! sistema se graban por separado, nunca mezclados**. De ahí salen tres cosas
//! gratis:
//!
//! 1. Diarización exacta sin ningún modelo: la pista `Mic` eres tú, la pista
//!    `System` es el profesor o el cliente.
//! 2. Mejor transcripción, porque no hay voces solapadas dentro de una señal.
//! 3. Control independiente: se puede silenciar tu pista en un acta, o subir
//!    solo la del profesor si tu micrófono captó ruido de casa.

pub mod mezcla;
pub mod sincronia;
pub mod wav;

#[cfg(target_os = "linux")]
pub mod pipewire_src;

#[cfg(target_os = "windows")]
pub mod wasapi_src;

#[cfg(target_os = "android")]
pub mod aaudio_src;

#[cfg(target_os = "linux")]
pub mod reproductor;

// En Windows sale por WASAPI. La mezcla de las dos pistas es común —vive en
// `mezclar`, aquí abajo—, así que lo propio de esta plataforma es solo el
// bucle de salida.
#[cfg(target_os = "windows")]
#[path = "reproductor_wasapi.rs"]
pub mod reproductor;

// `core/api` expone `reproductor::Reproductor` al puente de Flutter sin
// distinguir plataforma (lo necesita para reproducir sesiones ya grabadas), así
// que el módulo tiene que existir en todas partes con la misma superficie
// pública. Donde no hay motor de salida —Android, y cualquier plataforma sin
// backend— se usa un `stub`: mismo criterio que `iniciar()` y `dispositivos()`,
// un poco más abajo, que devuelven `AudioError::NoSoportada` en vez de dejar
// sin compilar a quien los llama.
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
#[path = "reproductor_stub.rs"]
pub mod reproductor;

use dictar_domain::Track;
use std::path::Path;
use std::sync::mpsc::Receiver;

/// Frecuencia de trabajo de todo el pipeline.
///
/// Whisper entrena y opera a 16 kHz: entregarle más no mejora nada y multiplica
/// el coste de cómputo, así que se remuestrea aquí, una sola vez, en el origen.
pub const SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("no se pudo iniciar la captura: {0}")]
    Inicio(String),

    #[error("no hay ningún dispositivo de {0}")]
    SinDispositivo(&'static str),

    #[error("error de E/S: {0}")]
    Io(#[from] std::io::Error),

    #[error("error al escribir el WAV: {0}")]
    Wav(#[from] hound::Error),

    #[error("el remuestreo falló: {0}")]
    Remuestreo(String),

    #[error("esta plataforma todavía no está soportada")]
    NoSoportada,
}

pub type Result<T> = std::result::Result<T, AudioError>;

/// Bloque de audio ya normalizado: mono, 16 kHz, muestras en `f32`.
#[derive(Debug, Clone)]
pub struct AudioFrame {
    pub track: Track,
    pub pcm: Vec<f32>,
    /// Milisegundos desde el inicio de la sesión, con reloj monótono.
    ///
    /// **No** es un instante absoluto ni deriva del reloj del dispositivo de
    /// audio: si las dos pistas no comparten base temporal, en una sesión de
    /// dos horas se desalinean y las notas acaban atribuyendo frases al
    /// interlocutor equivocado.
    pub timestamp_ms: i64,
}

impl AudioFrame {
    pub fn duracion_ms(&self) -> i64 {
        (self.pcm.len() as i64 * 1000) / SAMPLE_RATE as i64
    }

    /// Nivel RMS, para el indicador de voz de la interfaz.
    ///
    /// Sin ese indicador el usuario no descubre que no se estaba captando al
    /// profesor hasta que la clase ha terminado, que es cuando ya no hay
    /// remedio.
    pub fn nivel(&self) -> f32 {
        if self.pcm.is_empty() {
            return 0.0;
        }
        let suma: f32 = self.pcm.iter().map(|x| x * x).sum();
        (suma / self.pcm.len() as f32).sqrt()
    }
}

#[derive(Debug, Clone)]
pub struct CaptureConfig {
    /// Captura la salida del sistema (lo que oyes: Meet, Zoom, Teams).
    pub capturar_sistema: bool,
    /// Captura tu micrófono.
    pub capturar_microfono: bool,
    /// Nodo concreto, o `None` para el predeterminado del sistema.
    pub nodo_sistema: Option<String>,
    pub nodo_microfono: Option<String>,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            capturar_sistema: true,
            capturar_microfono: true,
            nodo_sistema: None,
            nodo_microfono: None,
        }
    }
}

impl CaptureConfig {
    /// Solo micrófono: reuniones presenciales.
    pub fn solo_microfono() -> Self {
        Self {
            capturar_sistema: false,
            capturar_microfono: true,
            ..Default::default()
        }
    }

    pub fn pistas(&self) -> Vec<Track> {
        let mut v = Vec::new();
        if self.capturar_microfono {
            v.push(Track::Mic);
        }
        if self.capturar_sistema {
            v.push(Track::System);
        }
        v
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceInfo {
    pub id: String,
    pub nombre: String,
    pub descripcion: String,
    /// `true` si es la monitorización de una salida, es decir, loopback.
    pub es_monitor: bool,
    pub por_defecto: bool,
}

/// Una captura en curso. Al soltarla, se detiene.
pub trait CaptureSession: Send {
    fn detener(self: Box<Self>) -> Result<()>;
}

/// Arranca la captura para esta plataforma.
///
/// Devuelve el canal por el que llegan los bloques de ambas pistas, ya
/// remuestreados a 16 kHz mono y con marca de tiempo común.
pub fn iniciar(cfg: CaptureConfig) -> Result<(Receiver<AudioFrame>, Box<dyn CaptureSession>)> {
    #[cfg(target_os = "linux")]
    {
        pipewire_src::iniciar(cfg)
    }

    #[cfg(target_os = "windows")]
    {
        wasapi_src::iniciar(cfg)
    }

    #[cfg(target_os = "android")]
    {
        aaudio_src::iniciar(cfg)
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "android")))]
    {
        let _ = cfg;
        Err(AudioError::NoSoportada)
    }
}

/// Enumera los dispositivos de captura disponibles.
pub fn dispositivos() -> Result<Vec<DeviceInfo>> {
    #[cfg(target_os = "linux")]
    {
        pipewire_src::dispositivos()
    }

    #[cfg(target_os = "windows")]
    {
        wasapi_src::dispositivos()
    }

    #[cfg(target_os = "android")]
    {
        aaudio_src::dispositivos()
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "android")))]
    {
        Err(AudioError::NoSoportada)
    }
}

/// Prepara las pistas de una sesión para reproducirlas como señal mono a 16 kHz.
///
/// Suma y recorta a [-1, 1]. Si el micrófono resulta ser un retorno acústico
/// retrasado de la pista de sistema, atenúa el micrófono durante la voz remota
/// y lo conserva en las pausas.
///
/// Vive en `lib.rs` y no en `reproductor` porque solo depende de
/// [`wav::leer_wav`], que es independiente de plataforma. `reproductor.rs`
/// (Linux, con PipeWire) y `reproductor_stub.rs` (el resto) la reexportan
/// como `reproductor::mezclar` en vez de reimplementarla cada uno: así no hay
/// dos copias de esta lógica que puedan divergir con el tiempo.
pub fn mezclar(dir: &Path) -> Result<Vec<f32>> {
    let ruta_mic = dir.join("mic.wav");
    let ruta_sistema = dir.join("system.wav");
    let mic = ruta_mic
        .is_file()
        .then(|| wav::leer_wav(&ruta_mic))
        .transpose()?;
    let sistema = ruta_sistema
        .is_file()
        .then(|| wav::leer_wav(&ruta_sistema))
        .transpose()?;

    match (mic, sistema) {
        (Some((mic, _)), Some((sistema, _))) => {
            // En una clase reproducida por altavoces, el micrófono vuelve a
            // capturar el audio del sistema con un retardo de sala. Se atenúa
            // el micrófono mientras la copia de sistema está activa y se lo
            // conserva en las pausas, donde suelen estar las preguntas locales.
            if let Some(retardo) = retardo_de_sistema(&mic, &sistema) {
                Ok(mezclar_con_atenuacion_de_retorno(&mic, &sistema, retardo))
            } else {
                Ok(sumar_pistas(&mic, &sistema))
            }
        }
        (mic, sistema) => Ok(match (mic, sistema) {
            (Some((mic, _)), None) => mic,
            (None, Some((sistema, _))) => sistema,
            (None, None) => Vec::new(),
            (Some((mic, _)), Some((sistema, _))) => sumar_pistas(&mic, &sistema),
        }),
    }
}

/// Detecta si el micrófono contiene una copia retrasada del audio de sistema.
///
/// Compara las envolventes RMS de bloques de 20 ms. La correlación de forma
/// de onda no sirve aquí: altavoces, sala y micrófono alteran la fase, aunque
/// el patrón de voz sea el mismo. Se requieren al menos 30 bloques activos y
/// una correlación alta en algún retardo entre 100 y 500 ms.
fn retardo_de_sistema(mic: &[f32], sistema: &[f32]) -> Option<usize> {
    const UMBRAL_CORRELACION: f64 = 0.65;
    analizar_retorno_acustico(mic, sistema)
        .filter(|(correlacion, _)| *correlacion >= UMBRAL_CORRELACION)
        .map(|(_, retardo_ms)| (retardo_ms / 20) as usize)
}

/// Estima el eco acústico en dos pistas mediante correlación de envolventes.
/// Devuelve (correlación, retardo_ms) en la banda de 100–500 ms cuando hay
/// suficientes bloques activos. Es un indicador, no una cancelación de eco.
pub fn analizar_retorno_acustico(mic: &[f32], sistema: &[f32]) -> Option<(f64, u64)> {
    let mic = envolvente_rms(mic);
    let sistema = envolvente_rms(sistema);
    correlacion_retorno(&mic, &sistema)
}

const BLOQUE_ENVOLVENTE: usize = SAMPLE_RATE as usize / 50;
const UMBRAL_RMS_RETORNO: f64 = 0.005;
const UMBRAL_CORRELACION_RETORNO: f64 = 0.65;

fn correlacion_retorno(mic: &[f64], sistema: &[f64]) -> Option<(f64, u64)> {
    const MIN_BLOQUES_ACTIVOS: usize = 30;
    const UMBRAL_RMS: f64 = UMBRAL_RMS_RETORNO;
    let max_lag = 500 / 20;
    let min_lag = 100 / 20;

    let mut mejor: Option<(f64, usize)> = None;
    for lag in min_lag..=max_lag {
        let pares: Vec<(f64, f64)> = sistema
            .iter()
            .take(mic.len().saturating_sub(lag))
            .zip(mic.iter().skip(lag))
            .filter(|(s, _)| **s >= UMBRAL_RMS)
            .map(|(s, m)| (*s, *m))
            .collect();
        if pares.len() < MIN_BLOQUES_ACTIVOS {
            continue;
        }

        let n = pares.len() as f64;
        let media_s = pares.iter().map(|p| p.0).sum::<f64>() / n;
        let media_m = pares.iter().map(|p| p.1).sum::<f64>() / n;
        let mut cov = 0.0;
        let mut var_s = 0.0;
        let mut var_m = 0.0;
        for (s, m) in pares {
            let ds = s - media_s;
            let dm = m - media_m;
            cov += ds * dm;
            var_s += ds * ds;
            var_m += dm * dm;
        }
        if var_s == 0.0 || var_m == 0.0 {
            continue;
        }
        let correlacion = cov / (var_s * var_m).sqrt();
        if mejor.map(|m| correlacion > m.0).unwrap_or(true) {
            mejor = Some((correlacion, lag));
        }
    }

    mejor.map(|(correlacion, lag)| (correlacion, lag as u64 * 20))
}

fn envolvente_rms(pcm: &[f32]) -> Vec<f64> {
    pcm.chunks(BLOQUE_ENVOLVENTE)
        .map(|bloque| {
            let energia = bloque.iter().map(|m| (*m as f64).powi(2)).sum::<f64>();
            (energia / bloque.len().max(1) as f64).sqrt()
        })
        .collect()
}

/// Atenúa el retorno acústico del micrófono durante la transcripción por lotes.
///
/// Lee solo envolventes RMS de los WAV (una muestra cada 20 ms), no carga ambas
/// pistas completas a memoria. Devuelve `None` si el formato no es el de las
/// capturas de sesión o si la correlación no supera el umbral.
pub fn detectar_retorno_en_wav(
    ruta_mic: &Path,
    ruta_sistema: &Path,
) -> Result<Option<RetornoAcustico>> {
    let Some(mic) = envolvente_wav(ruta_mic)? else {
        return Ok(None);
    };
    let Some(sistema) = envolvente_wav(ruta_sistema)? else {
        return Ok(None);
    };
    let Some((correlacion, retardo_ms)) = correlacion_retorno(&mic, &sistema) else {
        return Ok(None);
    };
    if correlacion < UMBRAL_CORRELACION_RETORNO {
        return Ok(None);
    }

    Ok(Some(RetornoAcustico {
        correlacion,
        retardo_ms,
        envolvente_sistema: sistema,
    }))
}

/// Evidencia de que `mic.wav` contiene una copia retrasada de `system.wav`.
pub struct RetornoAcustico {
    pub correlacion: f64,
    pub retardo_ms: u64,
    envolvente_sistema: Vec<f64>,
}

impl RetornoAcustico {
    /// Atenúa la pista de micrófono solo en los tramos donde detectó la copia
    /// retrasada del sistema. El WAV fuente no se modifica.
    pub fn atenuar_microfono(&self, mic: &mut [f32]) {
        let retardo = (self.retardo_ms / 20) as usize;
        let mut ganancia = 1.0f32;

        for (indice, bloque) in mic.chunks_mut(BLOQUE_ENVOLVENTE).enumerate() {
            let indice_sistema = indice.checked_sub(retardo);
            let hay_retorno = indice_sistema
                .and_then(|i| self.envolvente_sistema.get(i))
                .is_some_and(|rms| *rms >= UMBRAL_RMS_RETORNO);
            let objetivo = if hay_retorno { 0.0 } else { 1.0 };
            let paso = (objetivo - ganancia) / bloque.len().max(1) as f32;
            for muestra in bloque {
                ganancia += paso;
                *muestra *= ganancia;
            }
        }
    }
}

fn envolvente_wav(ruta: &Path) -> Result<Option<Vec<f64>>> {
    let mut wav = hound::WavReader::open(ruta)?;
    let spec = wav.spec();
    if spec.channels != 1 || spec.sample_rate != SAMPLE_RATE {
        return Ok(None);
    }

    let envolvente = match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Int, 16) => {
            envolvente_de_muestras(wav.samples::<i16>().map(|r| r.map(|v| v as f64 / 32768.0)))?
        }
        (hound::SampleFormat::Int, 32) => envolvente_de_muestras(
            wav.samples::<i32>()
                .map(|r| r.map(|v| v as f64 / 2_147_483_648.0)),
        )?,
        (hound::SampleFormat::Float, 32) => {
            envolvente_de_muestras(wav.samples::<f32>().map(|r| r.map(f64::from)))?
        }
        _ => return Ok(None),
    };
    Ok(Some(envolvente))
}

fn envolvente_de_muestras<I>(muestras: I) -> Result<Vec<f64>>
where
    I: Iterator<Item = std::result::Result<f64, hound::Error>>,
{
    let mut salida = Vec::new();
    let mut energia = 0.0;
    let mut cantidad = 0usize;
    for muestra in muestras {
        let muestra = muestra?;
        energia += muestra * muestra;
        cantidad += 1;
        if cantidad == BLOQUE_ENVOLVENTE {
            salida.push((energia / cantidad as f64).sqrt());
            energia = 0.0;
            cantidad = 0;
        }
    }
    if cantidad > 0 {
        salida.push((energia / cantidad as f64).sqrt());
    }
    Ok(salida)
}

fn sumar_pistas(mic: &[f32], sistema: &[f32]) -> Vec<f32> {
    let mut mezcla = vec![0.0; mic.len().max(sistema.len())];
    for (m, v) in mezcla.iter_mut().zip(mic) {
        *m = (*m + *v).clamp(-1.0, 1.0);
    }
    for (m, v) in mezcla.iter_mut().zip(sistema) {
        *m = (*m + *v).clamp(-1.0, 1.0);
    }
    mezcla
}

fn mezclar_con_atenuacion_de_retorno(mic: &[f32], sistema: &[f32], retardo: usize) -> Vec<f32> {
    const BLOQUE: usize = SAMPLE_RATE as usize / 50;
    const UMBRAL_RMS: f64 = 0.005;
    let sys_env = envolvente_rms(sistema);
    let total = mic.len().max(sistema.len());
    let mut mezcla = vec![0.0; total];
    let mut ganancia_mic = 1.0f32;

    for inicio in (0..total).step_by(BLOQUE) {
        let bloque = (total - inicio).min(BLOQUE);
        let indice_sistema = (inicio / BLOQUE).checked_sub(retardo);
        let hay_retorno = indice_sistema
            .and_then(|i| sys_env.get(i))
            .map(|rms| *rms >= UMBRAL_RMS)
            .unwrap_or(false);
        let objetivo = if hay_retorno { 0.0 } else { 1.0 };
        let paso = (objetivo - ganancia_mic) / bloque as f32;

        for desplazamiento in 0..bloque {
            let i = inicio + desplazamiento;
            let s = sistema.get(i).copied().unwrap_or(0.0);
            let m = mic.get(i).copied().unwrap_or(0.0);
            ganancia_mic += paso;
            mezcla[i] = (s + m * ganancia_mic).clamp(-1.0, 1.0);
        }
    }
    mezcla
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wav::EscritorPistas;

    fn senal_amplitud_variable(semilla: u32, duracion_s: usize) -> Vec<f32> {
        let mut estado = semilla;
        let mut salida = Vec::with_capacity(SAMPLE_RATE as usize * duracion_s);
        for bloque in 0..(duracion_s * 10) {
            estado = estado.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let amplitud = 0.08 + (estado % 10_000) as f32 / 20_000.0;
            for i in 0..(SAMPLE_RATE as usize / 10) {
                let fase = (bloque * (SAMPLE_RATE as usize / 10) + i) as f32;
                salida.push(amplitud * (fase * 0.15).sin());
            }
        }
        salida
    }

    fn escribir_wav(path: &Path, pcm: &[f32]) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut wav = hound::WavWriter::create(path, spec).unwrap();
        for muestra in pcm {
            wav.write_sample((muestra.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                .unwrap();
        }
        wav.finalize().unwrap();
    }

    #[test]
    fn la_configuracion_por_defecto_graba_las_dos_pistas() {
        let c = CaptureConfig::default();
        assert_eq!(c.pistas(), vec![Track::Mic, Track::System]);
    }

    #[test]
    fn en_presencial_solo_hay_microfono() {
        let c = CaptureConfig::solo_microfono();
        assert_eq!(c.pistas(), vec![Track::Mic]);
    }

    #[test]
    fn el_analisis_en_wav_detecta_y_atenúa_el_retorno_sin_cargar_ambas_pistas() {
        let dir = tempfile::tempdir().unwrap();
        let ruta_sistema = dir.path().join("system.wav");
        let ruta_mic = dir.path().join("mic.wav");
        let sistema = senal_amplitud_variable(17, 8);
        let demora = SAMPLE_RATE as usize / 5;
        let mut mic = vec![0.0; sistema.len()];
        for (destino, origen) in mic[demora..].iter_mut().zip(&sistema) {
            *destino = *origen * 0.6;
        }
        escribir_wav(&ruta_sistema, &sistema);
        escribir_wav(&ruta_mic, &mic);

        let filtro = detectar_retorno_en_wav(&ruta_mic, &ruta_sistema)
            .unwrap()
            .expect("el WAV debe detectar la copia retrasada");
        assert!(filtro.correlacion >= UMBRAL_CORRELACION_RETORNO);
        assert_eq!(filtro.retardo_ms, 200);

        let mut filtrada = mic.clone();
        filtro.atenuar_microfono(&mut filtrada);
        let desde = demora + SAMPLE_RATE as usize;
        let rms_antes = rms(&mic[desde..]);
        let rms_despues = rms(&filtrada[desde..]);
        assert!(
            rms_despues < rms_antes * 0.05,
            "el retorno quedó a {:.1}% del nivel original",
            rms_despues / rms_antes * 100.0
        );

        // La función de transcripción modifica su búfer en memoria, no la
        // pista original que la persona podrá reproducir o revisar después.
        let (original, _) = crate::wav::leer_wav(&ruta_mic).unwrap();
        // El WAV cuantiza a PCM de 16 bits; se admite el error de cuantización
        // de una muestra para comparar con el búfer f32 previo a escribirlo.
        assert!((original[desde] - mic[desde]).abs() <= 1.0 / i16::MAX as f32);
    }

    #[test]
    fn no_se_filtra_una_pista_de_microfono_sin_correlacion_con_el_sistema() {
        let dir = tempfile::tempdir().unwrap();
        let ruta_sistema = dir.path().join("system.wav");
        let ruta_mic = dir.path().join("mic.wav");
        escribir_wav(&ruta_sistema, &senal_amplitud_variable(17, 8));
        escribir_wav(&ruta_mic, &senal_amplitud_variable(91, 8));

        assert!(detectar_retorno_en_wav(&ruta_mic, &ruta_sistema)
            .unwrap()
            .is_none());
    }

    fn rms(pcm: &[f32]) -> f64 {
        (pcm.iter().map(|m| (*m as f64).powi(2)).sum::<f64>() / pcm.len() as f64).sqrt()
    }

    #[test]
    fn la_duracion_de_un_bloque_sale_de_su_numero_de_muestras() {
        let f = AudioFrame {
            track: Track::Mic,
            pcm: vec![0.0; 16_000],
            timestamp_ms: 0,
        };
        assert_eq!(f.duracion_ms(), 1000);
    }

    #[test]
    fn el_nivel_distingue_silencio_de_voz() {
        let silencio = AudioFrame {
            track: Track::Mic,
            pcm: vec![0.0; 1600],
            timestamp_ms: 0,
        };
        assert_eq!(silencio.nivel(), 0.0);

        let voz = AudioFrame {
            track: Track::Mic,
            pcm: vec![0.5; 1600],
            timestamp_ms: 0,
        };
        assert!((voz.nivel() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn un_bloque_vacio_no_divide_por_cero() {
        let f = AudioFrame {
            track: Track::Mic,
            pcm: vec![],
            timestamp_ms: 0,
        };
        assert_eq!(f.nivel(), 0.0);
        assert_eq!(f.duracion_ms(), 0);
    }

    // -- mezclar --------------------------------------------------------------
    //
    // Movidos aquí junto con la función: antes vivían en `reproductor.rs`, que
    // en Windows y macOS ni siquiera se compilaba, así que esta lógica —pura,
    // sin PipeWire de por medio— se quedaba sin probar fuera de Linux.

    #[test]
    fn la_mezcla_suma_las_dos_pistas() {
        let dir = tempfile::tempdir().unwrap();
        let mut e = EscritorPistas::nuevo(dir.path()).unwrap();
        e.escribir(&AudioFrame {
            track: Track::Mic,
            pcm: vec![0.25; 1600],
            timestamp_ms: 0,
        })
        .unwrap();
        e.escribir(&AudioFrame {
            track: Track::System,
            pcm: vec![0.25; 1600],
            timestamp_ms: 0,
        })
        .unwrap();
        e.cerrar().unwrap();

        let m = mezclar(dir.path()).unwrap();
        assert_eq!(m.len(), 1600);
        assert!((m[0] - 0.5).abs() < 0.01, "suma: {}", m[0]);
    }

    #[test]
    fn las_pistas_de_distinta_longitud_no_se_truncan() {
        // El monitor de salida arranca unos ms más tarde que el micro: las
        // pistas casi nunca miden lo mismo, y recortar a la corta comería el
        // final de la clase.
        let dir = tempfile::tempdir().unwrap();
        let mut e = EscritorPistas::nuevo(dir.path()).unwrap();
        e.escribir(&AudioFrame {
            track: Track::Mic,
            pcm: vec![0.1; 3200],
            timestamp_ms: 0,
        })
        .unwrap();
        e.escribir(&AudioFrame {
            track: Track::System,
            pcm: vec![0.1; 1600],
            timestamp_ms: 0,
        })
        .unwrap();
        e.cerrar().unwrap();

        assert_eq!(mezclar(dir.path()).unwrap().len(), 3200);
    }

    #[test]
    fn la_suma_de_dos_picos_no_desborda() {
        let dir = tempfile::tempdir().unwrap();
        let mut e = EscritorPistas::nuevo(dir.path()).unwrap();
        e.escribir(&AudioFrame {
            track: Track::Mic,
            pcm: vec![0.9; 160],
            timestamp_ms: 0,
        })
        .unwrap();
        e.escribir(&AudioFrame {
            track: Track::System,
            pcm: vec![0.9; 160],
            timestamp_ms: 0,
        })
        .unwrap();
        e.cerrar().unwrap();

        let m = mezclar(dir.path()).unwrap();
        assert!(m.iter().all(|v| *v <= 1.0), "debe recortar, no desbordar");
    }

    #[test]
    fn una_sesion_solo_de_microfono_se_reproduce_igual() {
        // Las reuniones presenciales no tienen pista de sistema.
        let dir = tempfile::tempdir().unwrap();
        let mut e = EscritorPistas::nuevo(dir.path()).unwrap();
        e.escribir(&AudioFrame {
            track: Track::Mic,
            pcm: vec![0.3; 800],
            timestamp_ms: 0,
        })
        .unwrap();
        e.cerrar().unwrap();

        assert_eq!(mezclar(dir.path()).unwrap().len(), 800);
    }

    // -- reproductor::Reproductor, donde no hay motor de salida ---------------

    /// Documenta el contrato del `stub`: donde no hay motor de audio de salida,
    /// `Reproductor::iniciar` tiene que fallar en tiempo de ejecución con
    /// `NoSoportada`, igual que `iniciar()` y `dispositivos()`. Sin este test,
    /// un cambio futuro en `reproductor_stub.rs` podría romper ese contrato sin
    /// que nada lo avisara, porque en Linux —donde sí corre la CI—
    /// `reproductor.rs` es otro archivo por completo y nunca ejercita esta rama.
    ///
    /// El `cfg` excluye también Windows desde que existe `reproductor_wasapi.rs`:
    /// allí la reproducción está implementada y esperar `NoSoportada` sería
    /// exigir que **no** funcione.
    #[test]
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    fn fuera_de_linux_reproducir_avisa_en_vez_de_no_compilar() {
        // Se compara contra el `Result` entero y no con `unwrap_err()`:
        // ese método exige que el tipo Ok implemente `Debug` para poder
        // imprimirlo, y `Reproductor` no lo implementa en ninguna de las dos
        // plataformas. Derivarlo solo para esto obligaría a derivarlo también
        // en `reproductor.rs`, que guarda manejadores de PipeWire.
        let r = reproductor::Reproductor::iniciar(Path::new("."), 0);
        assert!(matches!(r, Err(AudioError::NoSoportada)));
    }

    // -- iniciar()/dispositivos(), enrutado por plataforma ---------------
    //
    // Antes, el invariante 4 (ninguna plataforma sin backend rompe la
    // firma) lo probaba un `enum Plataforma` escrito a mano en
    // `sincronia.rs`, desconectado de los `#[cfg(target_os)]` de aquí
    // abajo: seguía en verde aunque este `cfg` se rompiera -- incluido el
    // caso de confundir Android con "no-Linux" (`target_os = "android"` no
    // es `"linux"` ni `"windows"`, así que cae bien en esta rama, pero un
    // `cfg` mal escrito podría dejar de excluirlo). Estos tests llaman a
    // las funciones reales de este módulo (`iniciar`/`dispositivos`, más
    // abajo), gateados por el mismo `#[cfg]` que la rama que verifican:
    // mismo criterio que
    // `fuera_de_linux_reproducir_avisa_en_vez_de_no_compilar`, arriba.
    //
    // Ningún job de la matriz satisface este `cfg`, y eso resultó ser algo
    // peor que "no se ejecuta": la prueba de `iniciar`, más abajo, **no
    // compilaba**. El comentario que había aquí decía que en cuanto
    // existiera un job para esta rama la prueba protegería de verdad. Era al
    // revés: lo primero que habría hecho ese job es fallar al compilar.
    //
    // Se descubrió añadiendo un objetivo cualquiera que cumpla el `cfg`
    // (`rustup target add x86_64-unknown-freebsd`) y ejecutando
    // `cargo check --target x86_64-unknown-freebsd --all-targets`.
    //
    // De ahí sale la forma barata de cerrar T-13: no hace falta un runner de
    // macOS para *ejecutar* estas pruebas, porque lo que estaba roto era la
    // compilación. Un `cargo check` cruzado a un objetivo así, que no
    // necesita runner propio, ya lo habría cazado.
    #[test]
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "android")))]
    fn fuera_de_windows_linux_y_android_iniciar_devuelve_no_soportada() {
        // Contra el `Result` entero, no con `unwrap_err()`: ese método pide
        // `Debug` en el tipo Ok para poder imprimirlo, y aquí el tipo Ok es
        // `(Receiver<AudioFrame>, Box<dyn CaptureSession>)` --
        // `CaptureSession` es `Send` y nada más. Mismo motivo que en la
        // prueba del reproductor, unas líneas más arriba.
        let r = iniciar(CaptureConfig::default());
        assert!(matches!(r, Err(AudioError::NoSoportada)));
    }

    /// Mismo caso que la de arriba, para la otra mitad del invariante 4: la
    /// réplica que reemplaza (`tiene_backend`) no distinguía entre
    /// `iniciar()` y `dispositivos()`, así que esta prueba cubre la que
    /// aquella dejaba fuera.
    #[test]
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "android")))]
    fn fuera_de_windows_linux_y_android_dispositivos_devuelve_no_soportada() {
        let err = dispositivos().unwrap_err();
        assert!(matches!(err, AudioError::NoSoportada));
    }
}
