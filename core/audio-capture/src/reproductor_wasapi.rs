//! Reproducción de las sesiones grabadas, en Windows.
//!
//! El espejo de `reproductor.rs` (PipeWire, Linux). La superficie pública
//! tiene que ser IDÉNTICA a la de aquel y a la del stub, tipo por tipo y
//! método por método: `core/api/src/lib.rs` usa
//! `dictar_audio::reproductor::Reproductor` sin ningún `#[cfg]` propio, y
//! `core/api/src/puente.rs` expone esas llamadas a Flutter. Si esta superficie
//! se desvía, deja de compilar `dictar-api` en Windows.
//!
//! ## Por qué no hay remuestreador aquí
//!
//! Las sesiones están guardadas a 16 kHz mono, y el dispositivo de salida
//! casi siempre quiere 48 kHz estéreo. La conversión la hace Windows: se
//! inicializa el `IAudioClient` con **nuestro** formato y las banderas
//! `AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM` y `..._SRC_DEFAULT_QUALITY`, que
//! existen justo para esto desde Windows 7.
//!
//! La alternativa era remuestrear a mano, y no valía la pena: `mezcla::
//! Remuestreador` convierte *hacia* 16 kHz, que es la dirección de la
//! captura, así que habría que escribir la contraria y probarla, para hacer
//! peor lo que el sistema ya hace bien.
//!
//! ## Por qué se sondea en vez de usar un evento
//!
//! WASAPI ofrece un modo por evento (`AUDCLNT_STREAMFLAGS_EVENTCALLBACK`) que
//! despierta al hilo cuando el buffer necesita datos. Aquí se sondea cada
//! pocos milisegundos, que con un buffer de 200 ms sobra de largo: esto
//! reproduce una clase ya grabada, no es una ruta de baja latencia, y el
//! modo por evento obliga a manejar un `HANDLE` más y su cierre.

use crate::{AudioError, Result, SAMPLE_RATE};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioClient, IAudioRenderClient, IMMDevice, IMMDeviceEnumerator,
    MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED, WAVEFORMATEX,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
};

// `mezclar` no depende de la plataforma —solo de `wav::leer_wav`—, así que
// vive en `lib.rs` y aquí se reexporta, igual que hacen `reproductor.rs` y el
// stub. Ver el porqué en el comentario de `crate::mezclar`.
pub use crate::mezclar;

// audioclient.h. Valores crudos y no las constantes con nombre del crate
// `windows`, por el mismo motivo que en `wasapi_src.rs`: son fijos en la ABI y
// así no se depende de si esta versión las expone como `u32` o como un
// newtype.
const AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM: u32 = 0x8000_0000;
const AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY: u32 = 0x0800_0000;

/// mmreg.h: muestras en coma flotante.
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;

/// Buffer de salida, en unidades de 100 ns. 200 ms: no es una ruta de baja
/// latencia y un buffer holgado evita cortes si el equipo está ocupado
/// transcribiendo otra sesión de fondo.
const DURACION_BUFFER_HNS: i64 = 2_000_000;

/// Cada cuánto se mira si hace falta rellenar. Muy por debajo de los 200 ms
/// del buffer: el margen es lo que impide que un hilo que llega tarde deje el
/// audio en silencio.
const SONDEO: Duration = Duration::from_millis(20);

/// Una reproducción en curso.
pub struct Reproductor {
    posicion: Arc<AtomicUsize>,
    pausado: Arc<AtomicBool>,
    terminado: Arc<AtomicBool>,
    parar: Arc<AtomicBool>,
    total: usize,
    hilo: Option<JoinHandle<()>>,
}

impl Reproductor {
    /// Empieza a reproducir la sesión guardada en `dir`, desde `desde_ms`.
    pub fn iniciar(dir: &Path, desde_ms: i64) -> Result<Self> {
        Self::iniciar_con_pausa(dir, desde_ms, false)
    }

    /// Abre el dispositivo de salida sin avanzar el audio hasta que se quite
    /// la pausa. Se usa para coordinar reproducción y captura en evaluaciones.
    pub fn iniciar_pausado(dir: &Path, desde_ms: i64) -> Result<Self> {
        Self::iniciar_con_pausa(dir, desde_ms, true)
    }

    fn iniciar_con_pausa(dir: &Path, desde_ms: i64, iniciar_pausado: bool) -> Result<Self> {
        let pcm = Arc::new(mezclar(dir)?);
        if pcm.is_empty() {
            return Err(AudioError::Inicio(
                "esta sesión no tiene audio guardado".into(),
            ));
        }

        let total = pcm.len();
        let posicion = Arc::new(AtomicUsize::new(ms_a_muestras(desde_ms).min(total)));
        let pausado = Arc::new(AtomicBool::new(iniciar_pausado));
        let terminado = Arc::new(AtomicBool::new(false));
        let parar = Arc::new(AtomicBool::new(false));

        // El hilo avisa de si consiguió abrir el dispositivo antes de que
        // `iniciar` devuelva: sin esto, un equipo sin tarjeta de sonido daría
        // un `Reproductor` aparentemente válido que no suena y nunca termina.
        let (tx_listo, rx_listo) = mpsc::channel::<Result<()>>();

        let hilo = {
            let pcm = pcm.clone();
            let posicion = posicion.clone();
            let pausado = pausado.clone();
            let terminado = terminado.clone();
            let parar = parar.clone();
            std::thread::spawn(move || {
                bucle(pcm, posicion, pausado, terminado, parar, tx_listo);
            })
        };

        match rx_listo.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                let _ = hilo.join();
                return Err(e);
            }
            // El hilo murió sin llegar a decir nada. No debería pasar, pero
            // colgarse esperando un mensaje que no va a llegar sería peor.
            Err(_) => {
                let _ = hilo.join();
                return Err(AudioError::Inicio(
                    "el hilo de reproducción terminó antes de abrir el dispositivo".into(),
                ));
            }
        }

        Ok(Self {
            posicion,
            pausado,
            terminado,
            parar,
            total,
            hilo: Some(hilo),
        })
    }

    pub fn posicion_ms(&self) -> i64 {
        muestras_a_ms(self.posicion.load(Ordering::Relaxed))
    }

    pub fn duracion_ms(&self) -> i64 {
        muestras_a_ms(self.total)
    }

    pub fn pausar(&self, pausar: bool) {
        self.pausado.store(pausar, Ordering::Relaxed);
    }

    pub fn pausado(&self) -> bool {
        self.pausado.load(Ordering::Relaxed)
    }

    pub fn terminado(&self) -> bool {
        self.terminado.load(Ordering::Relaxed)
    }

    pub fn saltar(&self, ms: i64) {
        self.posicion
            .store(ms_a_muestras(ms).min(self.total), Ordering::Relaxed);
        // Saltar hacia atrás después de que acabara tiene que volver a
        // reproducir, no dejar el reproductor marcado como terminado para
        // siempre.
        self.terminado.store(false, Ordering::Relaxed);
    }
}

impl Drop for Reproductor {
    fn drop(&mut self) {
        self.parar.store(true, Ordering::Relaxed);
        if let Some(h) = self.hilo.take() {
            let _ = h.join();
        }
    }
}

fn ms_a_muestras(ms: i64) -> usize {
    (ms.max(0) as usize * SAMPLE_RATE as usize) / 1000
}

fn muestras_a_ms(muestras: usize) -> i64 {
    (muestras as i64 * 1000) / SAMPLE_RATE as i64
}

/// Llena `destino` con lo que toque sonar y devuelve la posición nueva.
///
/// Toda la decisión de qué suena vive aquí, sin COM ni WASAPI de por medio,
/// para que se pueda probar sin tarjeta de sonido — el mismo motivo por el que
/// existe `sincronia.rs`. Devuelve también si se acabó el audio.
///
/// En pausa se escribe silencio en vez de dejar de escribir: un buffer que no
/// se rellena hace que WASAPI repita lo último que había, y eso se oye como un
/// zumbido, no como una pausa.
fn llenar(destino: &mut [f32], pcm: &[f32], posicion: usize, pausado: bool) -> (usize, bool) {
    if pausado {
        destino.fill(0.0);
        return (posicion, false);
    }

    let disponibles = pcm.len().saturating_sub(posicion);
    let copiadas = disponibles.min(destino.len());

    destino[..copiadas].copy_from_slice(&pcm[posicion..posicion + copiadas]);
    // La cola del último buffer va a silencio: si se dejara sin tocar sonaría
    // lo que hubiera quedado ahí de la vuelta anterior.
    destino[copiadas..].fill(0.0);

    let nueva = posicion + copiadas;
    (nueva, nueva >= pcm.len())
}

/// `CoInitializeEx` es por hilo, y hay que desparejarlo con `CoUninitialize`.
/// Igual que en `wasapi_src.rs`, y por el mismo motivo.
struct ComGuard;

impl ComGuard {
    fn iniciar() -> Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(err)?;
        Ok(Self)
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

/// El `IAudioClient` de salida ya inicializado, con su cliente de render.
struct SalidaAbierta {
    cliente: IAudioClient,
    render: IAudioRenderClient,
    /// Tramas que caben en el buffer del dispositivo.
    tramas: u32,
}

fn abrir_salida() -> Result<SalidaAbierta> {
    let enumerador: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(err)?;

    let dispositivo: IMMDevice =
        unsafe { enumerador.GetDefaultAudioEndpoint(eRender, eConsole) }.map_err(err)?;

    let cliente: IAudioClient = unsafe { dispositivo.Activate(CLSCTX_ALL, None) }.map_err(err)?;

    // Nuestro formato, no el del dispositivo: la conversión la hace Windows
    // por las banderas de más abajo. Mono, 32 bits en coma flotante, 16 kHz,
    // que es exactamente lo que devuelve `mezclar`.
    let formato = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_IEEE_FLOAT,
        nChannels: 1,
        nSamplesPerSec: SAMPLE_RATE,
        wBitsPerSample: 32,
        nBlockAlign: 4,
        nAvgBytesPerSec: SAMPLE_RATE * 4,
        cbSize: 0,
    };

    unsafe {
        cliente.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
            DURACION_BUFFER_HNS,
            0,
            &formato,
            None,
        )
    }
    .map_err(err)?;

    let render: IAudioRenderClient = unsafe { cliente.GetService() }.map_err(err)?;
    let tramas = unsafe { cliente.GetBufferSize() }.map_err(err)?;

    Ok(SalidaAbierta {
        cliente,
        render,
        tramas,
    })
}

fn bucle(
    pcm: Arc<Vec<f32>>,
    posicion: Arc<AtomicUsize>,
    pausado: Arc<AtomicBool>,
    terminado: Arc<AtomicBool>,
    parar: Arc<AtomicBool>,
    tx_listo: mpsc::Sender<Result<()>>,
) {
    let _com = match ComGuard::iniciar() {
        Ok(g) => g,
        Err(e) => {
            let _ = tx_listo.send(Err(e));
            return;
        }
    };

    let salida = match abrir_salida() {
        Ok(s) => s,
        Err(e) => {
            let _ = tx_listo.send(Err(e));
            return;
        }
    };

    if let Err(e) = unsafe { salida.cliente.Start() } {
        let _ = tx_listo.send(Err(err(e)));
        return;
    }

    // A partir de aquí `iniciar` puede devolver: el dispositivo está abierto y
    // sonando. Si el aviso no llega, el otro extremo se fue y no hay a quién
    // reproducirle.
    if tx_listo.send(Ok(())).is_err() {
        let _ = unsafe { salida.cliente.Stop() };
        return;
    }

    let mut fin_de_audio = false;

    while !parar.load(Ordering::Relaxed) {
        // Cuando se acabó el audio se deja de escribir y solo se espera a que
        // el dispositivo termine de sonar lo que ya tiene dentro. Cortar en
        // seco se comería los últimos 200 ms de la clase.
        if fin_de_audio {
            match unsafe { salida.cliente.GetCurrentPadding() } {
                Ok(0) => break,
                Ok(_) => {
                    std::thread::sleep(SONDEO);
                    continue;
                }
                Err(_) => break,
            }
        }

        let ocupadas = match unsafe { salida.cliente.GetCurrentPadding() } {
            Ok(p) => p,
            Err(_) => break,
        };

        let libres = salida.tramas.saturating_sub(ocupadas);
        if libres == 0 {
            std::thread::sleep(SONDEO);
            continue;
        }

        let ptr = match unsafe { salida.render.GetBuffer(libres) } {
            Ok(p) => p,
            Err(_) => break,
        };

        // Mono y `f32`, que es como se inicializó el cliente: una trama es una
        // muestra, así que `libres` sirve igual para las dos cuentas.
        let destino = unsafe { std::slice::from_raw_parts_mut(ptr as *mut f32, libres as usize) };

        let (nueva, agotado) = llenar(
            destino,
            &pcm,
            posicion.load(Ordering::Relaxed),
            pausado.load(Ordering::Relaxed),
        );

        if unsafe { salida.render.ReleaseBuffer(libres, 0) }.is_err() {
            break;
        }

        posicion.store(nueva, Ordering::Relaxed);

        if agotado {
            fin_de_audio = true;
            terminado.store(true, Ordering::Relaxed);
        }

        std::thread::sleep(SONDEO);
    }

    let _ = unsafe { salida.cliente.Stop() };
}

fn err<E: std::fmt::Display>(e: E) -> AudioError {
    AudioError::Inicio(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn en_pausa_suena_silencio_y_la_posicion_no_avanza() {
        // Y no «no se escribe nada»: un buffer sin tocar hace que WASAPI
        // repita lo anterior, que se oye como un zumbido.
        let pcm = vec![0.5; 100];
        let mut destino = [9.0; 10];

        let (pos, fin) = llenar(&mut destino, &pcm, 30, true);

        assert_eq!(pos, 30, "la pausa no debe mover la posición");
        assert!(!fin);
        assert!(destino.iter().all(|m| *m == 0.0), "debería ser silencio");
    }

    #[test]
    fn se_copia_desde_la_posicion_pedida() {
        let pcm: Vec<f32> = (0..100).map(|i| i as f32).collect();
        let mut destino = [0.0; 4];

        let (pos, fin) = llenar(&mut destino, &pcm, 10, false);

        assert_eq!(destino, [10.0, 11.0, 12.0, 13.0]);
        assert_eq!(pos, 14);
        assert!(!fin);
    }

    #[test]
    fn el_ultimo_buffer_se_completa_con_silencio_y_avisa_del_final() {
        // Quedan 3 muestras para un buffer de 8. Sin rellenar, sonaría lo que
        // hubiera quedado en el buffer de la vuelta anterior.
        let pcm = vec![0.5; 13];
        let mut destino = [9.0; 8];

        let (pos, fin) = llenar(&mut destino, &pcm, 10, false);

        assert_eq!(&destino[..3], &[0.5, 0.5, 0.5]);
        assert!(destino[3..].iter().all(|m| *m == 0.0));
        assert_eq!(pos, 13);
        assert!(fin, "debe avisar de que se acabó el audio");
    }

    #[test]
    fn pasado_el_final_no_se_lee_fuera_del_vector() {
        // `saltar` acota a `total`, pero esta función no debe depender de que
        // quien la llame lo haya hecho bien.
        let pcm = vec![0.5; 10];
        let mut destino = [9.0; 4];

        let (pos, fin) = llenar(&mut destino, &pcm, 10, false);

        assert!(destino.iter().all(|m| *m == 0.0));
        assert_eq!(pos, 10);
        assert!(fin);
    }

    #[test]
    fn la_conversion_entre_ms_y_muestras_va_y_vuelve() {
        for ms in [0_i64, 1, 500, 1_000, 61_000, 3_600_000] {
            assert_eq!(muestras_a_ms(ms_a_muestras(ms)), ms, "ms = {ms}");
        }
    }

    #[test]
    fn un_ms_negativo_no_desborda_al_pasar_a_usize() {
        // `saltar(-1)` puede llegar desde el puente; `as usize` sobre un
        // negativo daría un índice astronómico.
        assert_eq!(ms_a_muestras(-5_000), 0);
    }
}
