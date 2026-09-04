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

// `core/api` expone `reproductor::Reproductor` al puente de Flutter sin
// distinguir plataforma (lo necesita para reproducir sesiones ya grabadas), así
// que el módulo tiene que existir en todas partes con la misma superficie
// pública. Fuera de Linux no hay PipeWire para reproducir de verdad, así que se
// usa un `stub`: mismo criterio que `iniciar()` y `dispositivos()`, un poco más
// abajo, que devuelven `AudioError::NoSoportada` en vez de dejar sin compilar
// a quien los llama.
#[cfg(not(target_os = "linux"))]
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

/// Mezcla las pistas de una sesión en una sola señal mono a 16 kHz.
///
/// Suma y recorta a [-1, 1]: las dos voces rara vez coinciden —cuando una
/// habla, la otra calla— así que la suma directa no satura en la práctica, y
/// el recorte cubre el caso en que sí.
///
/// Vive en `lib.rs` y no en `reproductor` porque solo depende de
/// [`wav::leer_wav`], que es independiente de plataforma. `reproductor.rs`
/// (Linux, con PipeWire) y `reproductor_stub.rs` (el resto) la reexportan
/// como `reproductor::mezclar` en vez de reimplementarla cada uno: así no hay
/// dos copias de esta lógica que puedan divergir con el tiempo.
pub fn mezclar(dir: &Path) -> Result<Vec<f32>> {
    let mut mezcla: Vec<f32> = Vec::new();

    for nombre in ["mic.wav", "system.wav"] {
        let ruta = dir.join(nombre);
        if !ruta.is_file() {
            continue;
        }

        let (pcm, _sr) = wav::leer_wav(&ruta)?;
        if pcm.len() > mezcla.len() {
            mezcla.resize(pcm.len(), 0.0);
        }
        for (m, v) in mezcla.iter_mut().zip(pcm.iter()) {
            *m = (*m + *v).clamp(-1.0, 1.0);
        }
    }

    Ok(mezcla)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wav::EscritorPistas;

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

    // -- reproductor::Reproductor, fuera de Linux ------------------------------

    /// Documenta el contrato del `stub`: sin PipeWire, `Reproductor::iniciar`
    /// tiene que fallar en tiempo de ejecución con `NoSoportada`, igual que
    /// `iniciar()` y `dispositivos()`. Sin este test, un cambio futuro en
    /// `reproductor_stub.rs` podría romper ese contrato sin que nada lo avisara,
    /// porque en Linux —donde sí corre la CI— `reproductor.rs` es otro archivo
    /// por completo y nunca ejercita esta rama.
    #[test]
    #[cfg(not(target_os = "linux"))]
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
    // Con la matriz de CI actual (jobs solo para Linux y Windows, más el
    // cruce a Android que compila pero no ejecuta) esta rama no compila en
    // ningún job existente, así que este test tampoco corre hoy -- ninguna prueba automatizada puede ejercitarla sin
    // compilar para una tercera plataforma. Sigue siendo mejor que la
    // réplica: en cuanto exista un job así, protege de verdad; la réplica
    // nunca lo habría hecho.
    #[test]
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "android")))]
    fn fuera_de_windows_linux_y_android_iniciar_devuelve_no_soportada() {
        let err = iniciar(CaptureConfig::default()).unwrap_err();
        assert!(matches!(err, AudioError::NoSoportada));
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
