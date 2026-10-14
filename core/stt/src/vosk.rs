//! Adaptador local de baja carga para reconocimiento incremental con Vosk.
//!
//! El modelo recomendado se distribuye junto a la app en Windows. Se carga la
//! DLL en tiempo de ejecución para que otras plataformas no dependan de ella.

use crate::{Result, SttError};
use libloading::Library;
use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[repr(C)]
struct VoskModelOpaque {
    _private: [u8; 0],
}

#[repr(C)]
struct VoskRecognizerOpaque {
    _private: [u8; 0],
}

type ModelNew = unsafe extern "C" fn(*const c_char) -> *mut VoskModelOpaque;
type ModelFree = unsafe extern "C" fn(*mut VoskModelOpaque);
type RecognizerNew = unsafe extern "C" fn(*mut VoskModelOpaque, f32) -> *mut VoskRecognizerOpaque;
type RecognizerFree = unsafe extern "C" fn(*mut VoskRecognizerOpaque);
type AcceptShort = unsafe extern "C" fn(*mut VoskRecognizerOpaque, *const i16, i32) -> i32;
type ResultJson = unsafe extern "C" fn(*mut VoskRecognizerOpaque) -> *const c_char;
type RecognizerReset = unsafe extern "C" fn(*mut VoskRecognizerOpaque);
type SetLogLevel = unsafe extern "C" fn(i32);

/// Modelo compartido por varios reconocedores independientes.
pub struct VoskModelo {
    // La biblioteca debe seguir cargada mientras los punteros C existan.
    _library: Library,
    model: *mut VoskModelOpaque,
    model_free: ModelFree,
    recognizer_free: RecognizerFree,
    recognizer_new: RecognizerNew,
    accept_short: AcceptShort,
    partial_result: ResultJson,
    result: ResultJson,
    final_result: ResultJson,
    reset: RecognizerReset,
}

/// Reconocedor con estado para un flujo de audio.
pub struct VoskVivo {
    modelo: Arc<VoskModelo>,
    recognizer: *mut VoskRecognizerOpaque,
}

impl VoskModelo {
    /// Carga una sola copia del runtime y del modelo español local.
    pub fn cargar(modelo: &Path) -> Result<Self> {
        let dll = localizar_dll()?;
        // SAFETY: las funciones que se resuelven abajo corresponden a la API C
        // estable de Vosk; `_library` conserva el módulo cargado.
        let library = unsafe { Library::new(&dll) }
            .map_err(|e| SttError::Carga(format!("no se pudo abrir {}: {e}", dll.display())))?;

        unsafe fn simbolo<T: Copy>(lib: &Library, nombre: &[u8]) -> Result<T> {
            // SAFETY: cada nombre y tipo es parte de `vosk_api.h`.
            unsafe { lib.get::<T>(nombre) }
                .map(|s| *s)
                .map_err(|e| SttError::Carga(format!("símbolo Vosk ausente: {e}")))
        }

        let model_new: ModelNew = unsafe { simbolo(&library, b"vosk_model_new\0")? };
        let model_free: ModelFree = unsafe { simbolo(&library, b"vosk_model_free\0")? };
        let recognizer_new: RecognizerNew = unsafe { simbolo(&library, b"vosk_recognizer_new\0")? };
        let recognizer_free: RecognizerFree =
            unsafe { simbolo(&library, b"vosk_recognizer_free\0")? };
        let accept_short: AcceptShort =
            unsafe { simbolo(&library, b"vosk_recognizer_accept_waveform_s\0")? };
        let partial_result: ResultJson =
            unsafe { simbolo(&library, b"vosk_recognizer_partial_result\0")? };
        let result: ResultJson = unsafe { simbolo(&library, b"vosk_recognizer_result\0")? };
        let final_result: ResultJson =
            unsafe { simbolo(&library, b"vosk_recognizer_final_result\0")? };
        let reset: RecognizerReset = unsafe { simbolo(&library, b"vosk_recognizer_reset\0")? };
        let set_log_level: SetLogLevel = unsafe { simbolo(&library, b"vosk_set_log_level\0")? };

        // Evita que Kaldi escriba mensajes de carga del modelo en stderr.
        unsafe { set_log_level(-1) };

        let ruta = CString::new(modelo.to_string_lossy().as_bytes())
            .map_err(|_| SttError::Carga("la ruta del modelo contiene NUL".into()))?;
        // SAFETY: la API Vosk toma posesión internamente y devuelve punteros
        // que se liberan en Drop del wrapper.
        let model = unsafe { model_new(ruta.as_ptr()) };
        if model.is_null() {
            return Err(SttError::Carga(format!(
                "Vosk no pudo abrir el modelo {}",
                modelo.display()
            )));
        }
        Ok(Self {
            _library: library,
            model,
            model_free,
            recognizer_free,
            recognizer_new,
            accept_short,
            partial_result,
            result,
            final_result,
            reset,
        })
    }

    /// Crea otro flujo sin volver a cargar los pesos del modelo.
    pub fn nuevo_reconocedor(modelo: &Arc<Self>) -> Result<VoskVivo> {
        let recognizer = unsafe { (modelo.recognizer_new)(modelo.model, 16_000.0) };
        if recognizer.is_null() {
            return Err(SttError::Carga("Vosk no pudo crear el reconocedor".into()));
        }
        Ok(VoskVivo {
            modelo: modelo.clone(),
            recognizer,
        })
    }
}

impl VoskVivo {
    /// Carga un modelo y un reconocedor para usos de una sola pista.
    pub fn cargar(ruta: &Path) -> Result<Self> {
        // Hacerlo Send/Sync exigiría un contrato de hilos que la FFI de Vosk no documenta aquí.
        #[allow(clippy::arc_with_non_send_sync)]
        let modelo = Arc::new(VoskModelo::cargar(ruta)?);
        VoskModelo::nuevo_reconocedor(&modelo)
    }

    /// Consume muestras mono a 16 kHz. Devuelve `(texto, segmento_cerrado)`.
    pub fn aceptar(&mut self, pcm: &[f32]) -> Result<(String, bool)> {
        if pcm.is_empty() {
            return Ok((String::new(), false));
        }
        let n = i32::try_from(pcm.len())
            .map_err(|_| SttError::Transcripcion("bloque de audio demasiado grande".into()))?;
        // SAFETY: `pcm16` permanece válido durante la llamada y el reconocedor
        // solo consume `n` muestras.
        // WASAPI entrega f32 normalizado [-1, 1]. La API C de Vosk procesa
        // PCM16; convertir explícitamente evita enviarle amplitudes 32768
        // veces menores, que en la prueba real no generaban parciales.
        let pcm16: Vec<i16> = pcm
            .iter()
            .map(|muestra| {
                let muestra = if muestra.is_finite() { *muestra } else { 0.0 };
                (muestra.clamp(-1.0, 1.0) * 32767.0).round() as i16
            })
            .collect();
        let estado = unsafe { (self.modelo.accept_short)(self.recognizer, pcm16.as_ptr(), n) };
        if estado < 0 {
            return Err(SttError::Transcripcion(
                "Vosk rechazó el bloque de audio".into(),
            ));
        }
        let cerrado = estado > 0;
        let ptr = unsafe {
            if cerrado {
                (self.modelo.result)(self.recognizer)
            } else {
                (self.modelo.partial_result)(self.recognizer)
            }
        };
        Ok((leer_texto(ptr)?, cerrado))
    }

    /// Vacía la última hipótesis al detener la captura.
    pub fn finalizar(&mut self) -> Result<String> {
        leer_texto(unsafe { (self.modelo.final_result)(self.recognizer) })
    }

    /// Vacía el estado cuando cambia la pista de audio durante una sesión.
    pub fn reiniciar(&mut self) {
        unsafe { (self.modelo.reset)(self.recognizer) }
    }
}

impl Drop for VoskVivo {
    fn drop(&mut self) {
        // SAFETY: el reconocedor pertenece al modelo compartido, que Arc
        // mantiene vivo hasta que termina esta llamada.
        unsafe { (self.modelo.recognizer_free)(self.recognizer) };
    }
}

impl Drop for VoskModelo {
    fn drop(&mut self) {
        // SAFETY: Arc garantiza que ningún reconocedor lo siga usando.
        unsafe { (self.model_free)(self.model) };
    }
}

fn leer_texto(ptr: *const c_char) -> Result<String> {
    if ptr.is_null() {
        return Err(SttError::Transcripcion(
            "Vosk devolvió un resultado vacío".into(),
        ));
    }
    // Vosk conserva el JSON hasta la siguiente llamada al reconocedor.
    let json = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|e| SttError::Transcripcion(e.to_string()))?;
    let valor: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| SttError::Transcripcion(format!("respuesta Vosk inválida: {e}")))?;
    Ok(valor
        .get("text")
        .or_else(|| valor.get("partial"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned())
}

fn localizar_dll() -> Result<PathBuf> {
    let mut rutas = Vec::new();
    if let Some(p) = std::env::var_os("DICTAR_VOSK_DLL") {
        rutas.push(PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            rutas.push(dir.join("libvosk.dll"));
            rutas.push(dir.join("runtime").join("libvosk.dll"));
        }
    }
    rutas.push(PathBuf::from("libvosk.dll"));
    rutas.retain(|p| p.is_file());
    rutas.into_iter().next().ok_or_else(|| {
        SttError::Carga(
            "falta libvosk.dll; el runtime de Vosk debe instalarse junto a dictar_ia".into(),
        )
    })
}

/// Ruta predeterminada de un modelo pequeño instalado junto a la app.
pub fn modelo_para_idioma(idioma: &str) -> Result<PathBuf> {
    let (variable, carpeta, nombre) = match idioma {
        "es" => (
            "DICTAR_VOSK_MODEL_ES",
            "vosk-model-small-es-0.42",
            "español",
        ),
        "en" => (
            "DICTAR_VOSK_MODEL_EN",
            "vosk-model-small-en-us-0.15",
            "inglés",
        ),
        otro => {
            return Err(SttError::ModeloNoEncontrado(format!(
                "no hay un modelo Vosk local configurado para el idioma {otro}"
            )))
        }
    };
    // Variable histórica usada por el evaluador y por instalaciones manuales.
    // La variable específica permite instalar ambos modelos simultáneamente.
    if let Some(p) = std::env::var_os("DICTAR_VOSK_MODEL") {
        let p = PathBuf::from(p);
        if p.is_dir() {
            return Ok(p);
        }
    }
    if let Some(p) = std::env::var_os(variable) {
        let p = PathBuf::from(p);
        if p.is_dir() {
            return Ok(p);
        }
    }
    let mut rutas = Vec::new();
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        rutas.push(PathBuf::from(base).join("dictar_ia/models").join(carpeta));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            rutas.push(dir.join("models").join(carpeta));
        }
    }
    rutas.into_iter().find(|p| p.is_dir()).ok_or_else(|| {
        SttError::ModeloNoEncontrado(format!("modelo local {nombre} ausente ({carpeta})"))
    })
}

/// Compatibilidad para consumidores que solo necesitan el modelo español.
pub fn modelo_espanol() -> Result<PathBuf> {
    modelo_para_idioma("es")
}
