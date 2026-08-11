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
pub mod wav;

#[cfg(target_os = "linux")]
pub mod pipewire_src;

use dictar_domain::Track;
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

    #[cfg(not(target_os = "linux"))]
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

    #[cfg(not(target_os = "linux"))]
    {
        Err(AudioError::NoSoportada)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
