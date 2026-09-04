//! Reproducción de sesiones grabadas, fuera de Linux.
//!
//! La reproducción real sale por PipeWire (ver `reproductor.rs`), y PipeWire
//! solo existe en Linux. Este archivo ocupa su lugar en el resto de
//! plataformas: mismo criterio que `iniciar()` y `dispositivos()` en
//! `lib.rs`, que devuelven `AudioError::NoSoportada` en vez de dejar sin
//! compilar a quien los llama.
//!
//! La superficie pública de aquí tiene que ser IDÉNTICA a la de
//! `reproductor.rs`, tipo por tipo y método por método: `core/api/src/lib.rs`
//! usa `dictar_audio::reproductor::Reproductor` sin ningún `#[cfg]` propio, y
//! `core/api/src/puente.rs` expone esas llamadas a Flutter vía
//! `flutter_rust_bridge`. Si esta superficie se desvía de la real, deja de
//! compilar `dictar-api` —y con él, el binario `dictar` de la CLI— en
//! cualquier plataforma que no sea Linux, que es exactamente el problema que
//! este archivo existe para evitar.

use crate::{AudioError, Result};
use std::path::Path;

// `mezclar` no depende de PipeWire —solo de `wav::leer_wav`—, así que vive en
// `lib.rs` como función independiente de plataforma, y tanto este stub como
// `reproductor.rs` la reexportan en vez de reimplementarla. Ver el porqué en
// el comentario de `crate::mezclar`.
pub use crate::mezclar;

/// Una reproducción en curso.
///
/// En esta plataforma nunca llega a existir de verdad: `iniciar` siempre
/// falla, así que no hay ningún estado real que guardar ni que soltar al
/// destruirse.
pub struct Reproductor;

impl Reproductor {
    /// Empieza a reproducir la sesión guardada en `dir`, desde `desde_ms`.
    ///
    /// Aquí no hay motor de audio de salida: se rechaza con el mismo error que
    /// usa `dictar_audio::iniciar` para la captura fuera de Linux, en vez de
    /// fingir que la reproducción funciona.
    pub fn iniciar(_dir: &Path, _desde_ms: i64) -> Result<Self> {
        Err(AudioError::NoSoportada)
    }

    /// Inalcanzable en la práctica: sin una instancia real no hay quien llame
    /// a esto, pero el método tiene que existir para que `core/api` compile.
    pub fn posicion_ms(&self) -> i64 {
        0
    }

    pub fn duracion_ms(&self) -> i64 {
        0
    }

    pub fn pausar(&self, _pausar: bool) {}

    pub fn pausado(&self) -> bool {
        false
    }

    pub fn terminado(&self) -> bool {
        false
    }

    pub fn saltar(&self, _ms: i64) {}
}
