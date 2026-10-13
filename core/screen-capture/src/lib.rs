//! Captura manual de diapositivas durante una sesión.

pub mod hash;

use dictar_domain::TsMs;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ScreenError {
    #[error("no hay ninguna pantalla disponible")]
    SinPantalla,

    #[error("no se pudo capturar la pantalla: {0}")]
    Captura(String),

    #[error("error de E/S: {0}")]
    Io(#[from] std::io::Error),

    #[error("no se pudo guardar la imagen: {0}")]
    Imagen(String),

    #[error("esta plataforma todavía no está soportada")]
    NoSoportada,
}

pub type Result<T> = std::result::Result<T, ScreenError>;

/// Diapositiva capturada manualmente, lista para guardarse en la base de datos.
#[derive(Debug, Clone)]
pub struct SlideCapturada {
    pub ts_ms: TsMs,
    pub phash: String,
    pub ruta: PathBuf,
}

/// Región de la pantalla a la que se recortan las capturas.
///
/// Sin recorte, una captura de una clase por videollamada incluye la barra de
/// tareas, las pestañas del navegador, la URL de la reunión y **las caras y
/// los nombres de todos los participantes**. Eso es un problema de privacidad
/// antes que de calidad: son datos de terceros que nadie ha consentido que se
/// guarden ni que se manden a un modelo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub ancho: u32,
    pub alto: u32,
}

impl Region {
    /// Recorta la imagen, ajustando la región a lo que exista de verdad.
    ///
    /// Una región guardada puede quedarse fuera de rango si el usuario cambia
    /// de resolución o desconecta un monitor. Ajustar es mejor que fallar: se
    /// obtiene una captura algo distinta, no ninguna.
    pub fn recortar(&self, img: &image::DynamicImage) -> image::DynamicImage {
        use image::GenericImageView;

        let (w, h) = img.dimensions();
        let x = self.x.min(w.saturating_sub(1));
        let y = self.y.min(h.saturating_sub(1));
        let ancho = self.ancho.min(w - x).max(1);
        let alto = self.alto.min(h - y).max(1);

        img.crop_imm(x, y, ancho, alto)
    }

    pub fn es_valida(&self) -> bool {
        self.ancho > 20 && self.alto > 20
    }
}

/// Guarda una captura de la pantalla completa y devuelve su ruta y tamaño.
///
/// Sirve para que la interfaz enseñe la pantalla y el usuario dibuje encima el
/// área de la diapositiva.
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
pub fn captura_para_seleccion(destino: impl AsRef<Path>) -> Result<(PathBuf, u32, u32)> {
    use image::GenericImageView;

    let monitores = xcap::Monitor::all().map_err(|e| ScreenError::Captura(e.to_string()))?;
    let monitor = monitores.first().ok_or(ScreenError::SinPantalla)?;

    let imagen = monitor
        .capture_image()
        .map_err(|e| ScreenError::Captura(e.to_string()))?;
    let dyn_img = image::DynamicImage::ImageRgba8(imagen);
    let (w, h) = dyn_img.dimensions();

    let ruta = destino.as_ref().to_path_buf();
    if let Some(padre) = ruta.parent() {
        std::fs::create_dir_all(padre)?;
    }
    dyn_img
        .save(&ruta)
        .map_err(|e| ScreenError::Imagen(e.to_string()))?;

    Ok((ruta, w, h))
}

/// Captura la pantalla ahora mismo, cuando el usuario lo solicita.
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
pub fn capturar_ahora(
    dir: impl AsRef<Path>,
    ts_ms: TsMs,
    pantalla: Option<usize>,
    region: Option<Region>,
) -> Result<SlideCapturada> {
    let dir = dir.as_ref();
    std::fs::create_dir_all(dir)?;

    let monitores = xcap::Monitor::all().map_err(|e| ScreenError::Captura(e.to_string()))?;
    let monitor = match pantalla {
        Some(i) => monitores.get(i).ok_or(ScreenError::SinPantalla)?,
        None => monitores.first().ok_or(ScreenError::SinPantalla)?,
    };

    let imagen = monitor
        .capture_image()
        .map_err(|e| ScreenError::Captura(e.to_string()))?;
    let completa = image::DynamicImage::ImageRgba8(imagen);

    // Recortar a la región elegida deja fuera las caras de los participantes,
    // las pestañas del navegador y la propia ventana de la aplicación.
    let dyn_img = match region.filter(|r| r.es_valida()) {
        Some(r) => r.recortar(&completa),
        None => completa,
    };

    let h = hash::dhash(&hash::region_central(&dyn_img, 0.12));

    // La marca temporal identifica el momento de la captura manual.
    let ruta = dir.join(format!("manual-{ts_ms}.png"));
    dyn_img
        .save(&ruta)
        .map_err(|e| ScreenError::Imagen(e.to_string()))?;

    tracing::info!(ruta = %ruta.display(), "captura manual");

    Ok(SlideCapturada {
        ts_ms,
        phash: hash::a_hex(h),
        ruta,
    })
}

/// Pantallas disponibles, para que el usuario elija.
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
pub fn pantallas() -> Result<Vec<String>> {
    let monitores = xcap::Monitor::all().map_err(|e| ScreenError::Captura(e.to_string()))?;

    Ok(monitores
        .iter()
        .enumerate()
        .map(|(i, m)| {
            // En esta versión de xcap los datos del monitor son valores
            // directos, no Result.
            let _ = i;
            format!("{} ({}x{})", m.name(), m.width(), m.height())
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Sustitutos para las plataformas sin backend de captura de pantalla.
//
// Misma superficie pública, firma por firma, que las funciones de arriba: es
// lo que permite que `core/api` las llame sin un solo `#[cfg]` propio, igual
// que hace con `dictar_audio::reproductor`. Sin esto, cruzar el núcleo a
// Android no falla en tiempo de ejecución: falla al compilar, y se lleva por
// delante `dictar-api` y con él la aplicación entera.
// ---------------------------------------------------------------------------

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
pub fn captura_para_seleccion(_destino: impl AsRef<Path>) -> Result<(PathBuf, u32, u32)> {
    Err(ScreenError::NoSoportada)
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
pub fn capturar_ahora(
    _dir: impl AsRef<Path>,
    _ts_ms: TsMs,
    _pantalla: Option<usize>,
    _region: Option<Region>,
) -> Result<SlideCapturada> {
    Err(ScreenError::NoSoportada)
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
pub fn pantallas() -> Result<Vec<String>> {
    // Vacío y no error: preguntar qué pantallas hay es legítimo en cualquier
    // plataforma, y la respuesta honesta aquí es «ninguna».
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lienzo(w: u32, h: u32) -> image::DynamicImage {
        image::DynamicImage::ImageRgba8(image::RgbaImage::new(w, h))
    }

    #[test]
    fn la_region_recorta_lo_que_se_le_pide() {
        use image::GenericImageView;

        let img = lienzo(1920, 1080);
        let r = Region {
            x: 100,
            y: 200,
            ancho: 800,
            alto: 600,
        };
        assert_eq!(r.recortar(&img).dimensions(), (800, 600));
    }

    #[test]
    fn una_region_que_se_sale_se_ajusta_en_vez_de_fallar() {
        use image::GenericImageView;

        // Pasa al cambiar de resolución o desconectar un monitor. Una captura
        // algo distinta es mejor que ninguna.
        let img = lienzo(1280, 720);
        let r = Region {
            x: 1000,
            y: 600,
            ancho: 800,
            alto: 600,
        };

        let (w, h) = r.recortar(&img).dimensions();
        assert_eq!((w, h), (280, 120));
    }

    #[test]
    fn una_region_diminuta_no_se_considera_valida() {
        // Un clic sin arrastrar no debe dejar la captura en un cuadro de dos
        // píxeles: se ignora y se guarda la pantalla entera.
        assert!(!Region {
            x: 0,
            y: 0,
            ancho: 5,
            alto: 5
        }
        .es_valida());
        assert!(Region {
            x: 0,
            y: 0,
            ancho: 800,
            alto: 600
        }
        .es_valida());
    }
}
