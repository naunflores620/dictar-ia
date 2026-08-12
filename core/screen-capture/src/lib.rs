//! Captura automática de diapositivas.
//!
//! El profesor comparte pantalla, y esa información se pierde por completo si
//! solo se graba el audio. En clases con fórmulas, diagramas o código, la
//! transcripción sola no basta: «como veis aquí» no significa nada dentro de
//! un texto.
//!
//! El mecanismo muestrea la pantalla cada pocos segundos, calcula un hash
//! perceptual y guarda una imagen solo cuando la lámina cambia de verdad. Todo
//! el criterio de «de verdad» vive en [`detector`], porque es lo que separa una
//! función útil de una que llena el disco de capturas repetidas.

pub mod detector;
pub mod hash;

use detector::{Decision, Detector};
use dictar_domain::TsMs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

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
}

pub type Result<T> = std::result::Result<T, ScreenError>;

#[derive(Debug, Clone)]
pub struct ConfigCaptura {
    /// Cada cuánto se mira la pantalla.
    ///
    /// Dos segundos es el equilibrio: más frecuente no encuentra más láminas
    /// —una diapositiva dura minutos— y más espaciado se arriesga a perder la
    /// que el profesor enseña de pasada.
    pub intervalo: Duration,

    /// Fracción que se recorta por cada lado antes de comparar.
    ///
    /// Deja fuera la miniatura de la webcam y las barras de Meet o Zoom, que
    /// cambian constantemente y generarían una lámina por muestra.
    pub margen: f32,

    /// Distancia de Hamming a partir de la cual es otra diapositiva.
    pub umbral: u32,

    /// Índice de la pantalla, o `None` para la principal.
    pub pantalla: Option<usize>,

    /// Área a la que se recortan las capturas. Sin ella se guarda la pantalla
    /// entera, con todo lo que haya en ella.
    pub region: Option<Region>,
}

impl Default for ConfigCaptura {
    fn default() -> Self {
        Self {
            intervalo: Duration::from_secs(2),
            margen: 0.12,
            umbral: detector::UMBRAL_CAMBIO,
            pantalla: None,
            region: None,
        }
    }
}

/// Diapositiva detectada, lista para guardarse en la base de datos.
#[derive(Debug, Clone)]
pub struct SlideCapturada {
    pub ts_ms: TsMs,
    pub phash: String,
    pub ruta: PathBuf,
}

/// Captura en curso. Al detenerla, para el hilo de muestreo.
pub struct SesionCaptura {
    parar: Arc<AtomicBool>,
    hilo: Option<JoinHandle<()>>,
}

impl SesionCaptura {
    pub fn detener(mut self) {
        self.parar.store(true, Ordering::SeqCst);
        if let Some(h) = self.hilo.take() {
            let _ = h.join();
        }
    }
}

impl Drop for SesionCaptura {
    fn drop(&mut self) {
        self.parar.store(true, Ordering::SeqCst);
        if let Some(h) = self.hilo.take() {
            let _ = h.join();
        }
    }
}

/// Arranca la captura de diapositivas.
///
/// Corre en su propio hilo, separado del audio a propósito: si la captura de
/// pantalla falla o se ralentiza, **la grabación de audio no se ve afectada**.
/// Nunca al revés — perder la clase por un fallo capturando imágenes sería
/// inaceptable.
pub fn iniciar(
    dir: impl AsRef<Path>,
    cfg: ConfigCaptura,
) -> Result<(Receiver<SlideCapturada>, SesionCaptura)> {
    let dir = dir.as_ref().to_path_buf();
    std::fs::create_dir_all(&dir)?;

    // Se comprueba aquí que hay pantalla, y no dentro del hilo, para que el
    // error llegue a quien llama en lugar de perderse en un log.
    let monitores = xcap::Monitor::all().map_err(|e| ScreenError::Captura(e.to_string()))?;
    if monitores.is_empty() {
        return Err(ScreenError::SinPantalla);
    }

    let (tx, rx) = mpsc::channel();
    let parar = Arc::new(AtomicBool::new(false));
    let inicio = Instant::now();

    let hilo = {
        let parar = parar.clone();
        std::thread::Builder::new()
            .name("dictar-pantalla".into())
            .spawn(move || {
                let mut det = Detector::con_umbral(cfg.umbral);
                let mut n = 0usize;

                while !parar.load(Ordering::SeqCst) {
                    // Se duerme a tramos de 100 ms: durmiendo el intervalo
                    // entero, detener la grabación se quedaba hasta 2 s
                    // esperando a que este hilo despertara para cerrarlo.
                    let mut dormido = Duration::ZERO;
                    while dormido < cfg.intervalo && !parar.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(100));
                        dormido += Duration::from_millis(100);
                    }
                    if parar.load(Ordering::SeqCst) {
                        break;
                    }

                    match muestrear(&dir, &cfg, &mut det, &mut n, inicio.elapsed()) {
                        Ok(Some(s)) => {
                            if tx.send(s).is_err() {
                                break;
                            }
                        }
                        Ok(None) => {}
                        // Un fallo puntual no detiene la captura: la pantalla
                        // puede estar bloqueada un momento, o el usuario puede
                        // haber desconectado un monitor.
                        Err(e) => tracing::warn!(error = %e, "fallo al muestrear la pantalla"),
                    }
                }

                tracing::info!(laminas = det.guardadas(), "captura de pantalla detenida");
            })
            .map_err(|e| ScreenError::Captura(e.to_string()))?
    };

    Ok((
        rx,
        SesionCaptura {
            parar,
            hilo: Some(hilo),
        },
    ))
}

fn muestrear(
    dir: &Path,
    cfg: &ConfigCaptura,
    det: &mut Detector,
    n: &mut usize,
    transcurrido: Duration,
) -> Result<Option<SlideCapturada>> {
    let monitores = xcap::Monitor::all().map_err(|e| ScreenError::Captura(e.to_string()))?;
    let monitor = match cfg.pantalla {
        Some(i) => monitores.get(i).ok_or(ScreenError::SinPantalla)?,
        None => monitores.first().ok_or(ScreenError::SinPantalla)?,
    };

    let imagen = monitor
        .capture_image()
        .map_err(|e| ScreenError::Captura(e.to_string()))?;
    let completa = image::DynamicImage::ImageRgba8(imagen);

    let dyn_img = match cfg.region.filter(|r| r.es_valida()) {
        Some(r) => r.recortar(&completa),
        None => completa,
    };

    let recorte = hash::region_central(&dyn_img, cfg.margen);
    let h = hash::dhash(&recorte);

    match det.observar(h) {
        Decision::Nueva { phash } => {
            *n += 1;
            let ruta = dir.join(format!("{:04}.png", n));

            // Se guarda la captura completa, no el recorte: el recorte solo
            // sirve para decidir, pero al usuario le interesa la lámina entera.
            dyn_img
                .save(&ruta)
                .map_err(|e| ScreenError::Imagen(e.to_string()))?;

            tracing::info!(
                lamina = *n,
                ts_s = transcurrido.as_secs(),
                "diapositiva nueva"
            );

            Ok(Some(SlideCapturada {
                ts_ms: transcurrido.as_millis() as i64,
                phash: hash::a_hex(phash),
                ruta,
            }))
        }
        // Volver a una lámina ya guardada no produce archivo nuevo, pero sí
        // interesa dejar constancia del momento: los apuntes pueden referenciar
        // esa diapositiva desde dos puntos distintos de la clase.
        Decision::Repetida { .. } | Decision::Esperando | Decision::SinCambio => Ok(None),
    }
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

/// Captura la pantalla ahora mismo, sin pasar por el detector.
///
/// Es la vía manual, y existe porque la detección automática nunca acertará
/// siempre: hay clases sin diapositivas, presentaciones con vídeo dentro y
/// profesores que enseñan algo tres segundos. Con un botón, el usuario decide
/// el momento exacto y el resultado es correcto por definición.
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

    // El nombre lleva la marca de tiempo, no un contador: así no choca con las
    // que va guardando el hilo automático, que lleva su propia numeración.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_configuracion_por_defecto_es_razonable() {
        let c = ConfigCaptura::default();
        // Dos segundos: una diapositiva dura minutos, muestrear más a menudo
        // no encuentra más láminas y consume CPU durante toda la clase.
        assert_eq!(c.intervalo, Duration::from_secs(2));
        // Margen suficiente para dejar fuera la webcam y las barras de Meet.
        assert!(c.margen > 0.05 && c.margen < 0.25);
    }

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

    #[test]
    fn sin_region_la_configuracion_no_recorta() {
        assert!(ConfigCaptura::default().region.is_none());
    }

    #[test]
    fn el_directorio_se_crea_al_iniciar() {
        // No se comprueba la captura en sí: en CI no hay servidor gráfico. Lo
        // que sí debe cumplirse es que la carpeta quede lista.
        let dir = tempfile::tempdir().unwrap();
        let destino = dir.path().join("laminas");

        let _ = iniciar(&destino, ConfigCaptura::default());
        assert!(destino.is_dir());
    }
}
