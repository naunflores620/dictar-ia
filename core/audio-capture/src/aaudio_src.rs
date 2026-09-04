//! Captura en Android mediante AAudio.
//!
//! Solo la FFI y el bucle de lectura viven aquí: toda la lógica que se puede
//! probar sin un móvil delante —traducir el formato nativo, calcular el
//! relleno de silencio, decidir qué pistas se graban— vive en `sincronia.rs`,
//! compartida con el backend de Windows. Ver el porqué en la cabecera de ese
//! archivo.
//!
//! **Aquí no hay pista de sistema, y no es una carencia de esta
//! implementación.** Android no deja capturar el audio de una videollamada
//! ajena, a propósito y a nivel de sistema operativo: no hay equivalente al
//! *loopback* de WASAPI ni al monitor de PipeWire. El caso de uso de esta
//! plataforma es la reunión presencial, con el micrófono. Pedir la pista de
//! sistema no falla, se ignora — la razón está en
//! [`sincronia::pistas_en_android`].
//!
//! ## AAudio por FFI directa, no el crate `oboe`
//!
//! AAudio es C plano en `libaaudio.so`, presente desde API 26, y la
//! superficie que hace falta son las quince funciones de más abajo. `oboe`
//! envuelve exactamente eso mismo, pero arrastra un build de C++ al cruce con
//! el NDK — otra pieza que puede romperse en el CI, a cambio de nada que se
//! use. Es el mismo criterio con el que `wasapi_src` declara las constantes
//! de la ABI de Windows a mano en vez de arrastrar dependencias por comodidad.
//!
//! ## Lectura bloqueante, no callback
//!
//! AAudio ofrece un callback de baja latencia en su propio hilo de tiempo
//! real. No se usa: en ese hilo no se puede reservar memoria ni bloquear, y
//! este pipeline remuestrea y envía por un canal, que son las dos cosas. La
//! latencia tampoco importa —esto graba una clase de dos horas, no monitoriza
//! en directo—, así que un `AAudioStream_read` bloqueante con tiempo de
//! espera acotado, en un hilo normal, es la forma correcta y además la que
//! deja el bucle idéntico al de `wasapi_src`.

use crate::sincronia::{
    bytes_por_muestra, formato_desde_aaudio, pistas_en_android, PistaCapturada,
};
use crate::{AudioError, AudioFrame, CaptureConfig, CaptureSession, DeviceInfo, Result};
use dictar_domain::Track;
use std::ffi::{c_void, CStr};
use std::os::raw::c_char;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::Instant;

// -- FFI ------------------------------------------------------------------

#[repr(C)]
struct AAudioStreamBuilder {
    _opaco: [u8; 0],
}

#[repr(C)]
struct AAudioStream {
    _opaco: [u8; 0],
}

#[link(name = "aaudio")]
extern "C" {
    fn AAudio_createStreamBuilder(builder: *mut *mut AAudioStreamBuilder) -> i32;
    fn AAudioStreamBuilder_setDirection(builder: *mut AAudioStreamBuilder, direccion: i32);
    fn AAudioStreamBuilder_setFormat(builder: *mut AAudioStreamBuilder, formato: i32);
    fn AAudioStreamBuilder_setChannelCount(builder: *mut AAudioStreamBuilder, canales: i32);
    fn AAudioStreamBuilder_setSampleRate(builder: *mut AAudioStreamBuilder, frecuencia: i32);
    fn AAudioStreamBuilder_setPerformanceMode(builder: *mut AAudioStreamBuilder, modo: i32);
    fn AAudioStreamBuilder_setInputPreset(builder: *mut AAudioStreamBuilder, preset: i32);
    fn AAudioStreamBuilder_setSharingMode(builder: *mut AAudioStreamBuilder, modo: i32);
    fn AAudioStreamBuilder_openStream(
        builder: *mut AAudioStreamBuilder,
        flujo: *mut *mut AAudioStream,
    ) -> i32;
    fn AAudioStreamBuilder_delete(builder: *mut AAudioStreamBuilder) -> i32;
    fn AAudioStream_requestStart(flujo: *mut AAudioStream) -> i32;
    fn AAudioStream_requestStop(flujo: *mut AAudioStream) -> i32;
    fn AAudioStream_close(flujo: *mut AAudioStream) -> i32;
    fn AAudioStream_read(
        flujo: *mut AAudioStream,
        destino: *mut c_void,
        tramas: i32,
        espera_ns: i64,
    ) -> i32;
    fn AAudioStream_getSampleRate(flujo: *mut AAudioStream) -> i32;
    fn AAudioStream_getChannelCount(flujo: *mut AAudioStream) -> i32;
    fn AAudioStream_getFormat(flujo: *mut AAudioStream) -> i32;
    fn AAudio_convertResultToText(resultado: i32) -> *const c_char;
}

// aaudio/AAudio.h. Mismo criterio que las constantes crudas de `wasapi_src`:
// son valores fijos de una ABI publicada, no algo que dependa de la versión
// del NDK con el que se cruce.
const AAUDIO_OK: i32 = 0;
const AAUDIO_DIRECTION_INPUT: i32 = 1;
const AAUDIO_FORMAT_PCM_FLOAT: i32 = 2;
const AAUDIO_SHARING_MODE_SHARED: i32 = 0;

/// Búferes grandes y pocas interrupciones, en vez de baja latencia.
///
/// `AAUDIO_PERFORMANCE_MODE_LOW_LATENCY` despierta la CPU cada pocos
/// milisegundos; en una reunión de una hora eso se nota en la batería y no
/// aporta nada, porque nadie escucha esta captura en directo.
const AAUDIO_PERFORMANCE_MODE_POWER_SAVING: i32 = 11;

/// `AAUDIO_INPUT_PRESET_VOICE_RECOGNITION`.
///
/// Android aplica al micrófono, por defecto, control automático de ganancia y
/// supresión de ruido pensados para una llamada telefónica. Eso bombea el
/// volumen en los silencios y recorta las voces de fondo, que es justo lo que
/// hace que Whisper invente texto. Este preset los desactiva. No se usa
/// `UNPROCESSED` (9), que sería aún más crudo, porque no todos los
/// dispositivos lo implementan y la degradación no se anuncia.
const AAUDIO_INPUT_PRESET_VOICE_RECOGNITION: i32 = 6;

/// Cuánto espera `AAudioStream_read` antes de volver con lo que haya.
///
/// Acota el bloqueo para que la señal de parada se atienda como mucho 100 ms
/// después de darse. Sin tiempo de espera, detener la grabación dejaría el
/// hilo colgado dentro de la FFI hasta que el dispositivo entregara audio,
/// que en silencio absoluto puede no ocurrir.
const ESPERA_LECTURA_NS: i64 = 100_000_000;

/// Tramas por lectura: 20 ms a 48 kHz.
///
/// Se pide en tramas y no en bytes porque es lo que cuenta AAudio. Sobra para
/// que el remuestreador reciba bloques útiles, y es lo bastante corto para
/// que el búfer intermedio no crezca.
const TRAMAS_POR_LECTURA: i32 = 960;

// -- API del módulo -------------------------------------------------------

pub fn iniciar(cfg: CaptureConfig) -> Result<(Receiver<AudioFrame>, Box<dyn CaptureSession>)> {
    // La política —qué se graba y qué se ignora— vive en `sincronia` y se
    // consulta antes de tocar AAudio: si no queda ninguna pista, no tiene
    // sentido abrir nada.
    pistas_en_android(cfg.capturar_microfono, cfg.capturar_sistema)?;

    let (tx_frames, rx_frames) = mpsc::channel::<AudioFrame>();
    let (tx_parar, rx_parar) = mpsc::channel::<()>();
    let (tx_listo, rx_listo) = mpsc::channel::<Result<()>>();

    let inicio = Instant::now();

    let hilo = std::thread::Builder::new()
        .name("dictar-aaudio".into())
        .spawn(move || {
            let r = bucle(tx_frames, rx_parar, inicio, &tx_listo);
            if let Err(e) = &r {
                tracing::error!(error = %e, "el bucle de AAudio terminó con error");
                // Si falla después de haber arrancado, el receptor ya no
                // espera confirmación; el envío puede fallar sin
                // consecuencias.
                let _ = tx_listo.send(Err(AudioError::Inicio(e.to_string())));
            }
        })
        .map_err(|e| AudioError::Inicio(e.to_string()))?;

    // Esperar a saber si el micrófono se pudo abrir. Sin esto, un permiso
    // denegado se manifestaría como "no llega audio" sin ningún error, que es
    // lo peor que puede pasar al empezar a grabar una reunión.
    match rx_listo.recv() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            return Err(AudioError::Inicio(
                "el hilo de captura murió durante el arranque".into(),
            ))
        }
    }

    Ok((
        rx_frames,
        Box::new(SesionAAudio {
            tx_parar: Some(tx_parar),
            hilo: Some(hilo),
        }),
    ))
}

/// Enumera los dispositivos de captura.
///
/// AAudio no tiene enumeración: los dispositivos se listan con
/// `AudioManager.getDevices`, que es Java y exigiría una JNI entera para
/// devolver algo que esta aplicación no usa —en Android se graba con el
/// micrófono que el sistema tenga por defecto, y el usuario lo cambia desde
/// los ajustes del teléfono, no desde aquí—.
///
/// Se devuelve esa única entrada en vez de `NoSoportada` a propósito: quien
/// llama pinta una lista, y una lista con el micrófono real es la verdad. Un
/// error diría que no hay micrófono, que es falso.
pub fn dispositivos() -> Result<Vec<DeviceInfo>> {
    Ok(vec![DeviceInfo {
        id: "default".into(),
        nombre: "Micrófono del teléfono".into(),
        descripcion: "Entrada por defecto del sistema".into(),
        es_monitor: false,
        por_defecto: true,
    }])
}

struct SesionAAudio {
    tx_parar: Option<mpsc::Sender<()>>,
    hilo: Option<JoinHandle<()>>,
}

impl CaptureSession for SesionAAudio {
    fn detener(mut self: Box<Self>) -> Result<()> {
        parar(&mut self.tx_parar, &mut self.hilo);
        Ok(())
    }
}

impl Drop for SesionAAudio {
    fn drop(&mut self) {
        // Soltar la sesión sin llamar a `detener` no debe dejar el micrófono
        // abierto: en Android eso además mantiene el indicador de grabación
        // encendido en la barra de estado, y el usuario cree, con razón, que
        // se le sigue escuchando.
        parar(&mut self.tx_parar, &mut self.hilo);
    }
}

/// Señala la parada y espera al hilo. Idempotente: `detener()` la llama y
/// luego `Drop` la vuelve a llamar sobre la misma sesión, y la segunda vez no
/// queda nada que hacer.
fn parar(tx_parar: &mut Option<mpsc::Sender<()>>, hilo: &mut Option<JoinHandle<()>>) {
    if let Some(tx) = tx_parar.take() {
        let _ = tx.send(());
    }
    if let Some(h) = hilo.take() {
        let _ = h.join();
    }
}

// -- El bucle -------------------------------------------------------------

/// Un flujo de AAudio abierto, con el estado puro que convierte lo que
/// entrega en `AudioFrame`.
struct FlujoAbierto {
    flujo: *mut AAudioStream,
    pista: PistaCapturada,
    /// Bytes de una trama (todos los canales) en el formato con el que se
    /// abrió el flujo. Convierte las tramas que devuelve `AAudioStream_read`
    /// a bytes sin recalcular canales × ancho cada vez.
    bloque_bytes: usize,
}

impl Drop for FlujoAbierto {
    fn drop(&mut self) {
        // `close` sin `requestStop` deja el dispositivo en un estado que
        // AAudio documenta como indefinido.
        unsafe {
            AAudioStream_requestStop(self.flujo);
            AAudioStream_close(self.flujo);
        }
    }
}

fn bucle(
    tx: Sender<AudioFrame>,
    rx_parar: Receiver<()>,
    inicio: Instant,
    tx_listo: &Sender<Result<()>>,
) -> Result<()> {
    let mut abierto = match abrir_microfono() {
        Ok(a) => a,
        Err(e) => {
            // El caso corriente aquí es que el usuario denegara
            // `RECORD_AUDIO`: AAudio devuelve `AAUDIO_ERROR_INVALID_STATE` al
            // abrir, sin decir que es un problema de permisos.
            let _ = tx_listo.send(Err(e));
            return Ok(());
        }
    };

    if let Err(e) = comprobar(
        unsafe { AAudioStream_requestStart(abierto.flujo) },
        "arrancar",
    ) {
        let _ = tx_listo.send(Err(e));
        return Ok(());
    }

    tracing::info!("captura de AAudio iniciada");
    let _ = tx_listo.send(Ok(()));

    let mut buffer = vec![0_u8; TRAMAS_POR_LECTURA as usize * abierto.bloque_bytes];

    loop {
        match rx_parar.try_recv() {
            Ok(()) => break,
            Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => {}
        }

        let tramas = unsafe {
            AAudioStream_read(
                abierto.flujo,
                buffer.as_mut_ptr() as *mut c_void,
                TRAMAS_POR_LECTURA,
                ESPERA_LECTURA_NS,
            )
        };

        // Negativo es error; cero es que se agotó la espera sin audio, que en
        // silencio es normal y no tiene nada de excepcional.
        if tramas < 0 {
            // A diferencia de WASAPI, aquí solo hay una pista: si muere, la
            // sesión termina. No hay ninguna otra con la que seguir, así que
            // `alguna_pista_sigue_viva` no tendría a quién preguntar.
            tracing::error!(
                error = %texto_de_resultado(tramas),
                "la lectura de AAudio falló, se termina la sesión"
            );
            break;
        }
        if tramas == 0 {
            continue;
        }

        let bytes = &buffer[..tramas as usize * abierto.bloque_bytes];
        let ts_ms = inicio.elapsed().as_millis() as i64;

        match abierto.pista.procesar_paquete(ts_ms, bytes) {
            Ok(Some(frame)) => {
                // Que el receptor haya desaparecido no es un error: es que
                // quien consumía la grabación se fue. Se termina en vez de
                // seguir capturando para nadie.
                if tx.send(frame).is_err() {
                    break;
                }
            }
            Ok(None) => {}
            Err(e) => {
                tracing::error!(error = %e, "no se pudo procesar el paquete de audio");
                break;
            }
        }
    }

    // El último bloque que el remuestreador tenga sin completar es audio ya
    // capturado: tirarlo se come el final de la reunión. Es lo que
    // `pipewire_src` todavía no hace (T-8).
    let fin_ms = inicio.elapsed().as_millis() as i64;
    match abierto.pista.vaciar(fin_ms) {
        Ok(Some(frame)) => {
            let _ = tx.send(frame);
        }
        Ok(None) => {}
        Err(e) => tracing::error!(error = %e, "no se pudo vaciar el remuestreador al cerrar"),
    }

    tracing::info!("captura de AAudio detenida");
    Ok(())
}

/// Abre el micrófono por defecto y devuelve el flujo con su estado.
///
/// Todo lo que se pide al `builder` es una **preferencia**: AAudio abre con lo
/// que el dispositivo tenga y no avisa de que cambió de opinión. Por eso, en
/// cuanto el flujo está abierto, se vuelve a consultar la frecuencia, los
/// canales y el formato reales, y es con esos con los que se construye la
/// pista.
fn abrir_microfono() -> Result<FlujoAbierto> {
    let mut builder: *mut AAudioStreamBuilder = std::ptr::null_mut();
    comprobar(
        unsafe { AAudio_createStreamBuilder(&mut builder) },
        "crear el constructor de flujos",
    )?;
    if builder.is_null() {
        return Err(AudioError::Inicio(
            "AAudio devolvió un constructor nulo".into(),
        ));
    }

    // `BuilderGuard` libera el constructor pase lo que pase: los `?` de más
    // abajo salen de esta función por varios caminos.
    let _guarda = BuilderGuard(builder);

    unsafe {
        AAudioStreamBuilder_setDirection(builder, AAUDIO_DIRECTION_INPUT);
        AAudioStreamBuilder_setSharingMode(builder, AAUDIO_SHARING_MODE_SHARED);
        AAudioStreamBuilder_setFormat(builder, AAUDIO_FORMAT_PCM_FLOAT);
        AAudioStreamBuilder_setChannelCount(builder, 1);
        AAudioStreamBuilder_setSampleRate(builder, crate::SAMPLE_RATE as i32);
        AAudioStreamBuilder_setPerformanceMode(builder, AAUDIO_PERFORMANCE_MODE_POWER_SAVING);
        AAudioStreamBuilder_setInputPreset(builder, AAUDIO_INPUT_PRESET_VOICE_RECOGNITION);
    }

    let mut flujo: *mut AAudioStream = std::ptr::null_mut();
    comprobar(
        unsafe { AAudioStreamBuilder_openStream(builder, &mut flujo) },
        "abrir el flujo de entrada",
    )?;
    if flujo.is_null() {
        return Err(AudioError::Inicio("AAudio devolvió un flujo nulo".into()));
    }

    let frecuencia = unsafe { AAudioStream_getSampleRate(flujo) };
    let canales = unsafe { AAudioStream_getChannelCount(flujo) };
    let formato = formato_desde_aaudio(unsafe { AAudioStream_getFormat(flujo) });

    // Si algo de lo real no sirve, hay que cerrar el flujo que ya está
    // abierto: sin esto el micrófono quedaría tomado por un proceso que
    // acaba de renunciar a grabar.
    let formato = match formato {
        Ok(f) => f,
        Err(e) => {
            unsafe {
                AAudioStream_close(flujo);
            }
            return Err(e);
        }
    };
    if frecuencia <= 0 || canales <= 0 {
        unsafe {
            AAudioStream_close(flujo);
        }
        return Err(AudioError::Inicio(format!(
            "AAudio abrió el flujo con parámetros imposibles ({frecuencia} Hz, {canales} canales)"
        )));
    }

    tracing::info!(
        frecuencia,
        canales,
        ?formato,
        "flujo de AAudio abierto (lo pedido fue 16 kHz mono f32)"
    );

    let pista =
        match PistaCapturada::nueva(Track::Mic, canales as usize, formato, frecuencia as u32) {
            Ok(p) => p,
            Err(e) => {
                unsafe {
                    AAudioStream_close(flujo);
                }
                return Err(e);
            }
        };

    Ok(FlujoAbierto {
        flujo,
        pista,
        bloque_bytes: canales as usize * bytes_por_muestra(formato),
    })
}

/// Libera el `AAudioStreamBuilder` al salir de `abrir_microfono` por
/// cualquier camino. El constructor es independiente del flujo que crea: se
/// borra en cuanto se ha abierto, y también si la apertura falló.
struct BuilderGuard(*mut AAudioStreamBuilder);

impl Drop for BuilderGuard {
    fn drop(&mut self) {
        unsafe {
            AAudioStreamBuilder_delete(self.0);
        }
    }
}

/// Convierte un `aaudio_result_t` en `Result`, con el texto que da el propio
/// AAudio en vez de un número suelto.
fn comprobar(resultado: i32, que_se_hacia: &str) -> Result<()> {
    if resultado == AAUDIO_OK {
        return Ok(());
    }
    Err(AudioError::Inicio(format!(
        "AAudio falló al {que_se_hacia}: {}",
        texto_de_resultado(resultado)
    )))
}

/// El texto de un código de AAudio.
///
/// `AAudio_convertResultToText` devuelve punteros a literales estáticos del
/// propio `libaaudio.so`, así que no hay que liberarlos. Un nulo sería una
/// violación de su contrato, pero comprobarlo cuesta una línea y evita un
/// fallo de segmentación mientras se registra otro error.
fn texto_de_resultado(resultado: i32) -> String {
    let ptr = unsafe { AAudio_convertResultToText(resultado) };
    if ptr.is_null() {
        return format!("código {resultado}");
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}
