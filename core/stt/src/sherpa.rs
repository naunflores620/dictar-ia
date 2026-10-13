//! Reconocimiento incremental en español con Sherpa-ONNX.
//!
//! El reconocedor se comparte entre las pistas y cada una conserva su propio
//! flujo. La inferencia se limita a un hilo para dejar recursos al audio y al
//! video durante la clase.

use crate::{Result, SttError};
use sherpa_onnx::{OnlineRecognizer, OnlineRecognizerConfig, OnlineStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

const MODELO_ES: &str = "sherpa-onnx-streaming-zipformer-es-kroko-2025-08-06";

static MODELO_ES_CACHE: OnceLock<Mutex<Option<Arc<SherpaModelo>>>> = OnceLock::new();

pub struct SherpaModelo {
    reconocedor: OnlineRecognizer,
}

pub struct SherpaVivo {
    modelo: Arc<SherpaModelo>,
    flujo: OnlineStream,
}

impl SherpaModelo {
    pub fn cargar(ruta: &Path) -> Result<Self> {
        for archivo in ["encoder.onnx", "decoder.onnx", "joiner.onnx", "tokens.txt"] {
            if !ruta.join(archivo).is_file() {
                return Err(SttError::ModeloNoEncontrado(
                    ruta.join(archivo).display().to_string(),
                ));
            }
        }

        let mut config = OnlineRecognizerConfig::default();
        config.model_config.transducer.encoder = Some(ruta.join("encoder.onnx").display().to_string());
        config.model_config.transducer.decoder = Some(ruta.join("decoder.onnx").display().to_string());
        config.model_config.transducer.joiner = Some(ruta.join("joiner.onnx").display().to_string());
        config.model_config.tokens = Some(ruta.join("tokens.txt").display().to_string());
        config.model_config.provider = Some("cpu".to_owned());
        config.model_config.num_threads = 1;
        config.decoding_method = Some("greedy_search".to_owned());
        config.enable_endpoint = true;
        config.rule1_min_trailing_silence = 2.4;
        config.rule2_min_trailing_silence = 1.2;
        config.rule3_min_utterance_length = 20.0;

        let reconocedor = OnlineRecognizer::create(&config).ok_or_else(|| {
            SttError::Carga(format!("Sherpa-ONNX no pudo abrir el modelo {}", ruta.display()))
        })?;
        Ok(Self { reconocedor })
    }

    pub fn nuevo_flujo(modelo: &Arc<Self>) -> SherpaVivo {
        SherpaVivo {
            modelo: modelo.clone(),
            flujo: modelo.reconocedor.create_stream(),
        }
    }
}

/// Carga una vez el modelo español y comparte sus pesos entre sesiones.
pub fn modelo_es_compartido() -> Result<Arc<SherpaModelo>> {
    let cache = MODELO_ES_CACHE.get_or_init(|| Mutex::new(None));
    let mut cache = cache
        .lock()
        .map_err(|e| SttError::Carga(format!("caché Sherpa bloqueada: {e}")))?;
    if let Some(modelo) = cache.as_ref() {
        return Ok(modelo.clone());
    }

    let ruta = modelo_es()?;
    let modelo = Arc::new(SherpaModelo::cargar(&ruta)?);
    *cache = Some(modelo.clone());
    Ok(modelo)
}

/// Deja listos los pesos antes de empezar una sesión en español.
pub fn precalentar_modelo_es() -> Result<()> {
    modelo_es_compartido().map(|_| ())
}

impl SherpaVivo {
    /// Consume audio mono a 16 kHz y devuelve la hipotesis actual o la frase cerrada.
    pub fn aceptar(&mut self, pcm: &[f32]) -> Result<(String, bool)> {
        if pcm.is_empty() {
            return Ok((String::new(), false));
        }
        self.flujo.accept_waveform(16_000, pcm);
        self.decodificar();
        let cerrado = self.modelo.reconocedor.is_endpoint(&self.flujo);
        let texto = self
            .modelo
            .reconocedor
            .get_result(&self.flujo)
            .map(|r| r.text)
            .unwrap_or_default();
        if cerrado {
            self.modelo.reconocedor.reset(&self.flujo);
        }
        Ok((texto, cerrado))
    }

    /// Vacía el último tramo al cerrar la sesión.
    pub fn finalizar(&mut self) -> Result<String> {
        self.flujo.input_finished();
        self.decodificar();
        Ok(self
            .modelo
            .reconocedor
            .get_result(&self.flujo)
            .map(|r| r.text)
            .unwrap_or_default())
    }

    /// Descarta el contexto acústico después de un hueco en la captura.
    pub fn reiniciar(&mut self) {
        self.modelo.reconocedor.reset(&self.flujo);
    }

    fn decodificar(&self) {
        while self.modelo.reconocedor.is_ready(&self.flujo) {
            self.modelo.reconocedor.decode(&self.flujo);
        }
    }
}

/// Busca el paquete local de modelo español instalado junto a la aplicación.
pub fn modelo_es() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("DICTAR_SHERPA_MODEL_ES") {
        let path = PathBuf::from(path);
        if validar_modelo(&path) {
            return Ok(path);
        }
    }

    let mut rutas = Vec::new();
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        rutas.push(PathBuf::from(base).join("dictar_ia/models").join(MODELO_ES));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            rutas.push(dir.join("models").join(MODELO_ES));
        }
    }
    rutas.into_iter().find(|p| validar_modelo(p)).ok_or_else(|| {
        SttError::ModeloNoEncontrado(format!(
            "no se encontró el modelo local de español {MODELO_ES}"
        ))
    })
}

fn validar_modelo(ruta: &Path) -> bool {
    ["encoder.onnx", "decoder.onnx", "joiner.onnx", "tokens.txt"]
        .iter()
        .all(|nombre| ruta.join(nombre).is_file())
}
