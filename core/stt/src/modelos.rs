//! Descarga y localización de los modelos de Whisper.
//!
//! Los modelos **no se empaquetan en el instalador**: `large-v3-turbo` pesa
//! quince veces más que la aplicación entera. Se descargan en el primer
//! arranque, a la carpeta de datos del usuario, y así se puede cambiar de
//! modelo sin publicar una versión nueva.

use crate::{Result, SttError};
use std::path::{Path, PathBuf};

/// Modelos que ofrece la aplicación.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modelo {
    /// Pasada en vivo. Lo que se ve mientras hablas es desechable, así que
    /// prima la latencia sobre la precisión.
    Small,
    /// Pasada definitiva. Precisión cercana a `large-v3` a ~6× su velocidad, y
    /// notablemente más robusto al ruido que los modelos pequeños.
    LargeV3Turbo,
    /// Alternativa para equipos sin GPU donde `turbo` va justo.
    Medium,
}

impl Modelo {
    pub fn archivo(&self) -> &'static str {
        match self {
            Modelo::Small => "ggml-small-q5_1.bin",
            Modelo::LargeV3Turbo => "ggml-large-v3-turbo-q5_0.bin",
            Modelo::Medium => "ggml-medium-q5_0.bin",
        }
    }

    pub fn url(&self) -> String {
        format!(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
            self.archivo()
        )
    }

    /// Tamaño aproximado en MB, para avisar antes de descargar.
    pub fn tamano_mb(&self) -> u32 {
        match self {
            Modelo::Small => 190,
            Modelo::LargeV3Turbo => 550,
            Modelo::Medium => 540,
        }
    }

    pub fn nombre_humano(&self) -> &'static str {
        match self {
            Modelo::Small => "Small (rápido, para la transcripción en vivo)",
            Modelo::LargeV3Turbo => "Large v3 Turbo (el bueno, para la pasada final)",
            Modelo::Medium => "Medium (equilibrio para equipos sin GPU)",
        }
    }
}

/// Carpeta donde viven los modelos.
///
/// - Linux: `~/.local/share/dictar_ia/models`
/// - Windows: `%LOCALAPPDATA%\dictar_ia\models`
pub fn dir_modelos() -> PathBuf {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));

    base.join("dictar_ia").join("models")
}

pub fn ruta(modelo: Modelo) -> PathBuf {
    dir_modelos().join(modelo.archivo())
}

pub fn esta_descargado(modelo: Modelo) -> bool {
    let p = ruta(modelo);
    // Se comprueba el tamaño y no solo la existencia: una descarga interrumpida
    // deja un archivo que existe pero que `whisper.cpp` no puede cargar, y el
    // error que da entonces no ayuda nada a entender qué pasó.
    std::fs::metadata(&p)
        .map(|m| m.len() > (modelo.tamano_mb() as u64 * 1024 * 1024) / 2)
        .unwrap_or(false)
}

/// Localiza un modelo, mirando también junto al ejecutable.
///
/// Permite llevar la aplicación y sus modelos en una carpeta portátil, o poner
/// el modelo a mano sin depender de la descarga.
pub fn localizar(modelo: Modelo) -> Result<PathBuf> {
    let candidatas = [
        ruta(modelo),
        // `$HOME/.local/share` explícito, además de `XDG_DATA_HOME`. No es
        // redundante: dentro de un sandbox —el snap de VS Code, un Flatpak—
        // esa variable apunta a un directorio privado de la aplicación, y el
        // modelo que el usuario descargó a su carpeta real quedaría invisible.
        std::env::var_os("HOME")
            .map(|h| {
                PathBuf::from(h)
                    .join(".local/share/dictar_ia/models")
                    .join(modelo.archivo())
            })
            .unwrap_or_default(),
        PathBuf::from("models").join(modelo.archivo()),
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|p| p.join("models").join(modelo.archivo())))
            .unwrap_or_default(),
    ];

    for c in candidatas.iter().filter(|c| !c.as_os_str().is_empty()) {
        if c.is_file() {
            return Ok(c.clone());
        }
    }

    Err(SttError::ModeloNoEncontrado(mensaje_ausente(modelo)))
}

/// Mensaje de «falta el modelo».
///
/// Un error tiene que llevar a la solución, no solo describir el fallo: por eso
/// incluye la orden exacta y el tamaño de la descarga.
pub fn mensaje_ausente(modelo: Modelo) -> String {
    format!(
        "{} — descárgalo con «dictar modelos --descargar {}» ({} MB)",
        ruta(modelo).display(),
        match modelo {
            Modelo::Small => "small",
            Modelo::LargeV3Turbo => "turbo",
            Modelo::Medium => "medium",
        },
        modelo.tamano_mb()
    )
}

/// Prepara la carpeta de modelos.
pub fn preparar_dir() -> Result<PathBuf> {
    let d = dir_modelos();
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

/// Comprueba que un archivo descargado tiene pinta de modelo GGML.
///
/// Los servidores devuelven páginas de error con código 200 más a menudo de lo
/// que parece; sin esta comprobación, el usuario descarga un HTML de 3 KB y el
/// fallo aparece mucho más tarde, al cargar el modelo.
pub fn parece_ggml(ruta: &Path) -> bool {
    use std::io::Read;

    let Ok(mut f) = std::fs::File::open(ruta) else {
        return false;
    };
    let mut magia = [0u8; 4];
    if f.read_exact(&mut magia).is_err() {
        return false;
    }
    // "ggml" o "lmgg" según el orden de bytes con el que se escribió.
    &magia == b"ggml" || &magia == b"lmgg"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn cada_modelo_tiene_archivo_y_url_coherentes() {
        for m in [Modelo::Small, Modelo::LargeV3Turbo, Modelo::Medium] {
            assert!(m.archivo().ends_with(".bin"));
            assert!(m.url().ends_with(m.archivo()));
            assert!(m.tamano_mb() > 0);
        }
    }

    #[test]
    fn la_carpeta_de_modelos_cuelga_de_los_datos_del_usuario() {
        let d = dir_modelos();
        assert!(d.ends_with("dictar_ia/models") || d.ends_with("dictar_ia\\models"));
    }

    #[test]
    fn un_modelo_ausente_da_un_error_que_dice_como_arreglarlo() {
        // Se prueba el mensaje directamente: si dependiera de `localizar`, el
        // test pasaría o fallaría según si el modelo está descargado en la
        // máquina, que es justo lo que un test no debe hacer.
        let msg = mensaje_ausente(Modelo::LargeV3Turbo);
        assert!(msg.contains("descárgalo"), "mensaje: {msg}");
        assert!(msg.contains("turbo"), "debe decir qué descargar: {msg}");
        assert!(msg.contains("550"), "debe avisar del tamaño: {msg}");
    }

    #[test]
    fn se_detecta_un_archivo_que_no_es_un_modelo() {
        // Un servidor que devuelve una página de error con código 200 es más
        // común de lo que parece.
        let dir = tempfile::tempdir().unwrap();
        let falso = dir.path().join("ggml-small-q5_1.bin");
        let mut f = std::fs::File::create(&falso).unwrap();
        f.write_all(b"<!DOCTYPE html><html>404</html>").unwrap();

        assert!(!parece_ggml(&falso));
    }

    #[test]
    fn se_acepta_una_cabecera_ggml_valida() {
        let dir = tempfile::tempdir().unwrap();
        let bueno = dir.path().join("m.bin");
        let mut f = std::fs::File::create(&bueno).unwrap();
        f.write_all(b"ggml\x00\x00\x00\x00").unwrap();

        assert!(parece_ggml(&bueno));
    }

    #[test]
    fn un_archivo_a_medias_no_cuenta_como_descargado() {
        // Si no, el fallo aparece mucho más tarde y con un error que no ayuda.
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_DATA_HOME", dir.path());

        let p = ruta(Modelo::Small);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, vec![0u8; 1024]).unwrap();

        assert!(!esta_descargado(Modelo::Small));
    }
}
