//! Captura en Windows mediante WASAPI, en modo *loopback* para el sistema.
//!
//! Solo COM y el bucle de lectura de paquetes viven aquí: toda la lógica que
//! se puede probar sin tarjeta de sonido —normalizar el formato nativo,
//! calcular el relleno de silencio, decidir con qué pistas seguir— vive en
//! `sincronia.rs`, que no depende de esta plataforma. Ver el porqué de esa
//! separación en la cabecera de ese archivo.
//!
//! El *loopback* se abre sobre el dispositivo de **salida** por defecto (el
//! *render endpoint*), no sobre uno de entrada: es la parte que más se
//! equivoca quien implementa esto por primera vez. `AUDCLNT_STREAMFLAGS_LOOPBACK`
//! le dice a WASAPI que, en vez de capturar lo que entra por ese dispositivo
//! (nada, es una salida), entregue una copia de lo que se está reproduciendo
//! en él.
//!
//! La grabación **sigue al dispositivo por defecto** (AV-1). Una vez por
//! segundo se consulta cuál es el de salida y cuál el de entrada; si el
//! id cambia y se mantiene, se reabre esa pista sobre el dispositivo nuevo. Si
//! un dispositivo desaparece (`AUDCLNT_E_DEVICE_INVALIDATED`), la pista pasa a
//! «reabriendo» y la sesión sigue. Solo se sigue el rol `eConsole`: si Meet o
//! Teams usan el dispositivo de comunicaciones (`eCommunications`) y es otro,
//! no se sigue. La lógica de decisión vive en `sincronia::ControlPista`.
//!
//! Sondeo, no eventos: WASAPI admite un modo dirigido por eventos
//! (`AUDCLNT_STREAMFLAGS_EVENTCALLBACK`), pero tiene historial de no
//! dispararse de forma fiable en *loopback* en algunas versiones de
//! Windows. Un sondeo corto y regular es más código, pero no depende de esa
//! garantía.

use crate::sincronia::{
    fecha_por_pendientes, ControlPista, FormatoNativo, PistaCapturada, RelojCaptura, Veredicto,
};
use crate::{AudioError, AudioFrame, CaptureConfig, CaptureSession, DeviceInfo, Result};
use dictar_domain::Track;
use std::ffi::c_void;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use windows::core::GUID;
use windows::Win32::Media::Audio::{
    eCapture, eConsole, eRender, EDataFlow, IAudioCaptureClient, IAudioClient, IMMDevice,
    IMMDeviceCollection, IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED,
    DEVICE_STATE, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED,
};

// audioclient.h. El resto de valores crudos se quedan como están —son fijos en
// la ABI y evitan depender de cómo los exponga esta versión del crate—, pero
// el del bucle NO, y la razón está escrita con sangre.
//
// Decía `0x0010_0000`. Ese valor existe y es válido, solo que es
// `AUDCLNT_STREAMFLAGS_RATEADJUST`; el del bucle es `0x0002_0000`. Al pasar el
// flag equivocado, `Initialize` **no se queja** —RATEADJUST es aceptable— y el
// dispositivo de salida nunca entra en modo bucle. El fallo aparece más tarde
// y en otro sitio: `GetService::<IAudioCaptureClient>()` devuelve
// `AUDCLNT_E_WRONG_ENDPOINT_TYPE` (0x88890003), «pides un cliente de captura
// sobre un dispositivo de salida». Y como abrir una pista no aborta la sesión,
// la grabación seguía adelante solo con micrófono.
//
// Consecuencia real, medida el 2026-09-11: en Windows **nunca se capturó la
// voz del otro lado**. Ni en una clase de Meet ni en una reunión con cliente.
// El archivo `system.wav` no llegó a existir una sola vez, y nada lo avisaba.
// Es la mitad del producto —la diarización de dos pistas— caída por un dígito
// hexadecimal.
//
// Por eso este se toma del crate: un número mágico mal escrito no lo caza
// ningún test, porque no hay con qué contrastarlo. El importado sí.
use windows::Win32::Media::Audio::AUDCLNT_STREAMFLAGS_LOOPBACK;

const AUDCLNT_BUFFERFLAGS_SILENT: u32 = 0x2;
// La posición QPC de este bloque no es fiable (audioclient.h).
const AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR: u32 = 0x4;

// ksmedia.h. Mismo motivo que las anteriores: identifica el subformato de un
// `WAVEFORMATEXTENSIBLE` como `f32`, y es un identificador fijo de la ABI, no
// algo que dependa de qué módulo de `windows` lo reexporte.
const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID =
    GUID::from_u128(0x0000_0003_0000_0010_8000_00AA_0038_9B71);

// mmdeviceapi.h. Sin esta máscara, `EnumAudioEndpoints` también devolvería
// los dispositivos deshabilitados o desconectados.
//
// A diferencia de las constantes de audioclient.h de más arriba, esta **sí**
// hay que envolverla: `EnumAudioEndpoints` pide un `DEVICE_STATE`, que en
// este crate es un newtype y no un `u32`. El comentario original de aquí
// afirmaba lo contrario —que daba igual cómo lo expusiera esta versión del
// crate— y era falso; no se detectó porque este archivo no se compiló nunca
// hasta hoy.
const DEVICE_STATE_ACTIVE: u32 = 0x1;

/// Duración del búfer de cada cliente: 3 s. Abrir un cliente en el hilo único
/// del bucle bloquea la lectura de la otra pista mientras dura, y las
/// aperturas del micrófono medidas en AV-1 llegan a 2,5 s. Con el búfer
/// anterior, de 200 ms, la pista del sistema (la voz de la clase) perdía datos
/// en cada reapertura.
///
/// Un búfer mayor no añade latencia de entrega (el bucle sigue sondeando cada
/// 10 ms), pero **sí cambia lo que vale el instante de lectura**: tras un
/// bloqueo, el búfer conserva segundos de audio que se drenan de golpe. Por
/// eso cada bloque se fecha con su instante de captura (`RelojCaptura`) y no
/// con el de la lectura; fecharlos al leer adelantaba la pista tanto como
/// durase el bloqueo. Tras `Initialize` se lee `GetBufferSize` y se registra,
/// porque Windows puede conceder menos de lo pedido (ver `BUFFER_MINIMO_MS`).
const DURACION_BUFFER_HNS: i64 = 3_000 * 10_000;

/// Por debajo de esto el búfer no cubre la apertura más lenta medida (2,5 s).
const BUFFER_MINIMO_MS: u64 = 2_500;

pub fn iniciar(cfg: CaptureConfig) -> Result<(Receiver<AudioFrame>, Box<dyn CaptureSession>)> {
    if !cfg.capturar_microfono && !cfg.capturar_sistema {
        return Err(AudioError::Inicio(
            "hay que capturar al menos una pista".into(),
        ));
    }

    let (tx_frames, rx_frames) = mpsc::channel::<AudioFrame>();
    let (tx_parar, rx_parar) = mpsc::channel::<()>();
    let (tx_listo, rx_listo) = mpsc::channel::<Result<()>>();

    let hilo = std::thread::Builder::new()
        .name("dictar-wasapi".into())
        .spawn(move || {
            let r = bucle(cfg, tx_frames, rx_parar, &tx_listo);
            if let Err(e) = &r {
                tracing::error!(error = %e, "el bucle de WASAPI terminó con error");
                // Si falla después de haber arrancado, el receptor ya no
                // espera confirmación; el envío puede fallar sin
                // consecuencias.
                let _ = tx_listo.send(Err(AudioError::Inicio(e.to_string())));
            }
        })
        .map_err(|e| AudioError::Inicio(e.to_string()))?;

    // Esperar a saber si al menos una pista se pudo abrir. Sin esto, un
    // fallo se manifestaría como "no llega audio" sin ningún error, que es
    // lo peor que puede pasar al empezar a grabar una clase.
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
        Box::new(SesionWasapi {
            tx_parar: Some(tx_parar),
            hilo: Some(hilo),
        }),
    ))
}

struct SesionWasapi {
    tx_parar: Option<Sender<()>>,
    hilo: Option<JoinHandle<()>>,
}

impl CaptureSession for SesionWasapi {
    fn detener(mut self: Box<Self>) -> Result<()> {
        if let Some(tx) = self.tx_parar.take() {
            let _ = tx.send(());
        }
        if let Some(h) = self.hilo.take() {
            let _ = h.join();
        }
        Ok(())
    }
}

impl Drop for SesionWasapi {
    fn drop(&mut self) {
        // Soltar la sesión sin llamar a `detener` no debe dejar el hilo de
        // WASAPI girando en segundo plano con el micrófono abierto.
        if let Some(tx) = self.tx_parar.take() {
            let _ = tx.send(());
        }
        if let Some(h) = self.hilo.take() {
            let _ = h.join();
        }
    }
}

/// Un `IAudioClient` ya inicializado, con su `IAudioCaptureClient` y el
/// estado puro (`sincronia::PistaCapturada`) que convierte lo que entrega en
/// `AudioFrame`.
struct CapturaAbierta {
    cliente: IAudioClient,
    captura: IAudioCaptureClient,
    pista: PistaCapturada,
    /// `WAVEFORMATEX::nBlockAlign`: bytes de una trama (todos los canales) en
    /// el formato negociado. Convierte `num_frames` de `GetBuffer` a bytes
    /// sin recalcular canales × bytes-por-muestra por separado.
    bloque_bytes: u32,
    /// Identificador del dispositivo sobre el que se abrió, leído de ese mismo
    /// `IMMDevice` y no de una segunda consulta: así el sondeo compara contra
    /// lo que de verdad se abrió, aunque el sistema cambie de opinión entre
    /// una llamada y otra.
    id_dispositivo: String,
    /// Lo que Windows concedió de verdad (`GetBufferSize`), en tramas y ms.
    buffer_tramas: u32,
    buffer_ms: u64,
    /// Frecuencia nativa del dispositivo: convierte tramas a milisegundos.
    frecuencia: u32,
}

/// Una pista abierta y arrancada, con los milisegundos que costó (D4), o el
/// motivo del fallo.
type Apertura = std::result::Result<(CapturaAbierta, u128), FalloApertura>;

/// Por qué no se pudo abrir una pista. La diferencia manda: un fallo
/// transitorio se reintenta; uno permanente con ese dispositivo, no.
enum FalloApertura {
    Transitorio(AudioError),
    /// El dispositivo existe y responde, pero su formato no se sabe leer.
    /// Reintentar con él daría el mismo resultado cada 2 s para siempre.
    Permanente {
        id: String,
        causa: AudioError,
    },
}

impl std::fmt::Display for FalloApertura {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transitorio(e) | Self::Permanente { causa: e, .. } => e.fmt(f),
        }
    }
}

/// Una de las dos pistas de la sesión: qué es, qué cliente tiene abierto (si
/// tiene) y la máquina de estados que decide qué hacer con ella.
struct Pista {
    /// Para los registros: «el micrófono», «el bucle del sistema».
    nombre: &'static str,
    track: Track,
    flujo: EDataFlow,
    loopback: bool,
    abierta: Option<CapturaAbierta>,
    control: ControlPista,
    /// Último dispositivo que tuvo abierto, para decir de cuál se viene en el
    /// registro de una reapertura.
    ultimo_id: Option<String>,
    /// Por qué se soltó el último cliente, para el registro de la reapertura.
    motivo: &'static str,
    /// Para avisar del primer sondeo fallido de una racha y no de los demás.
    sondeo_fallando: bool,
}

fn bucle(
    cfg: CaptureConfig,
    tx: Sender<AudioFrame>,
    rx_parar: Receiver<()>,
    tx_listo: &Sender<Result<()>>,
) -> Result<()> {
    // `CoInitializeEx` es por hilo: este es el único que habla con WASAPI.
    // `ComGuard` lo despareja con `CoUninitialize` al salir de esta función
    // por cualquier camino, incluido un error de arranque.
    let _com = ComGuard::iniciar()?;

    let enumerador: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(err)?;

    // Cada pista se abre por separado y un fallo en una no aborta la otra:
    // sin dispositivo de salida, o con el loopback denegado por políticas
    // del sistema, la sesión sigue solo con micrófono (o al revés). La
    // comprobación de `pistas_a_grabar` va después de intentar abrir las dos
    // **y** arrancar las dos, para que también atrape el caso de que se
    // abrieran pero ninguna llegara a arrancar (p. ej.
    // `AUDCLNT_E_DEVICE_IN_USE`: otro proceso con acceso exclusivo).
    //
    // El orden importa: primero se abren las dos y después se arrancan las dos,
    // sin nada lento entre un `Start` y el otro. Arrancar el micrófono y luego
    // abrir el sistema (62–732 ms medidos) dejaba ese tiempo de audio en el
    // búfer del micrófono antes de que naciera el reloj de la sesión.
    let mic = abrir_si_se_pidio(
        cfg.capturar_microfono,
        &enumerador,
        eCapture,
        false,
        Track::Mic,
        "el micrófono",
    );
    let sistema = abrir_si_se_pidio(
        cfg.capturar_sistema,
        &enumerador,
        eRender,
        true,
        Track::System,
        "el bucle del sistema",
    );

    let mic = mic
        .map(arrancar)
        .map(|r| registrar_arranque(r, "el micrófono"));
    let sistema = sistema
        .map(arrancar)
        .map(|r| registrar_arranque(r, "el bucle del sistema"));

    let abierta = |a: &Option<Apertura>| matches!(a, Some(Ok(_)));
    if let Err(e) = crate::sincronia::pistas_a_grabar(abierta(&mic), abierta(&sistema)) {
        let _ = tx_listo.send(Err(e));
        return Ok(());
    }

    // El reloj nace después de abrir y arrancar las pistas. Iniciarlo antes de
    // COM/Initialize hacía que `muestras_de_relleno` convirtiera el tiempo de
    // preparación del dispositivo en silencio al principio de cada WAV.
    let inicio = Instant::now();
    let origen = Duration::ZERO;
    // Compartido por las dos pistas y las reaperturas: todas leen el mismo QPC.
    let mut reloj = RelojCaptura::default();

    let mut pistas = [
        nueva_pista("el micrófono", Track::Mic, eCapture, false, mic, origen),
        nueva_pista(
            "el bucle del sistema",
            Track::System,
            eRender,
            true,
            sistema,
            origen,
        ),
    ];

    tracing::info!(
        microfono = pistas[0].abierta.is_some(),
        sistema = pistas[1].abierta.is_some(),
        "captura de WASAPI iniciada"
    );
    let _ = tx_listo.send(Ok(()));

    loop {
        match rx_parar.try_recv() {
            Ok(()) => break,
            Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => {}
        }

        let mut hubo_datos = false;
        let ahora = inicio.elapsed();

        // 1. Leer. Un error COM aquí (por ejemplo `AUDCLNT_E_DEVICE_INVALIDATED`:
        // el usuario desconecta el dispositivo a mitad de clase) ya no cierra
        // la pista para siempre: la manda a reabrir. Antes de eso propagaba el
        // error con `?`, mataba `bucle()` entero y se saltaba la limpieza de
        // las dos pistas, que rompe el invariante 1; y después de arreglar
        // eso, la pista cerrada no volvía nunca, que para una clase que
        // ocurre una sola vez es perderla igual.
        for p in pistas.iter_mut() {
            let Some(c) = p.abierta.as_mut() else {
                continue;
            };
            match procesar_paquetes(c, inicio, &mut reloj, &tx) {
                Ok(datos) => hubo_datos |= datos,
                Err(e) => {
                    tracing::error!(
                        pista = p.nombre,
                        error = %e,
                        "falló la lectura, se cierra el cliente y se reabrirá"
                    );
                    soltar_para_reabrir(p, "error de lectura", inicio, &tx);
                }
            }
        }

        // 2. Seguir al dispositivo por defecto (D1). El sondeo tiene su
        // propio ritmo dentro de `ControlPista::sondear`; aquí solo se
        // ofrece la ocasión en cada vuelta.
        for p in pistas.iter_mut() {
            let veredicto = {
                let Pista {
                    control,
                    flujo,
                    nombre,
                    sondeo_fallando,
                    ..
                } = &mut *p;
                control.sondear(ahora, || {
                    match unsafe { id_de_endpoint_por_defecto(&enumerador, *flujo) } {
                        Ok(id) => {
                            *sondeo_fallando = false;
                            Some(id)
                        }
                        Err(e) => {
                            // Solo el primero de la racha: una sala sin
                            // ningún dispositivo por defecto fallaría una vez
                            // por segundo durante toda la clase.
                            if !*sondeo_fallando {
                                tracing::warn!(
                                    pista = *nombre,
                                    error = %e,
                                    "no se pudo consultar el dispositivo por defecto, \
                                     se deja la pista como está"
                                );
                            }
                            *sondeo_fallando = true;
                            None
                        }
                    }
                })
            };
            if veredicto == Veredicto::Reabrir {
                tracing::warn!(
                    pista = p.nombre,
                    id_anterior = p.ultimo_id.as_deref().unwrap_or("-"),
                    "el dispositivo por defecto cambió, se reabre la pista"
                );
                soltar_para_reabrir(p, "el dispositivo por defecto cambió", inicio, &tx);
            }
        }

        // 3. Reintentos (D2). Una pista que acaba de soltarse se reabre en
        // esta misma vuelta: `toca_reintentar` es verdadero de inmediato.
        for i in 0..pistas.len() {
            if pistas[i].control.toca_avisar(ahora) {
                tracing::warn!(
                    pista = pistas[i].nombre,
                    contador_muestras = pistas[i].control.contador(),
                    "la pista sigue sin dispositivo"
                );
            }
            if pistas[i].control.toca_reintentar(ahora) {
                reabrir(&mut pistas, i, &enumerador, inicio, &mut reloj, &tx);
            }
        }

        // No hay salida por aquí a propósito: la sesión no termina por el
        // estado de los dispositivos, ni siquiera si todas las pistas están
        // ausentes (una ausente sigue vigilando el dispositivo por defecto).
        // Termina solo cuando `detener()` llega por `rx_parar`. El aviso a la
        // persona de que no se graba nada es cosa de AV-2.

        if !hubo_datos {
            // 10 ms es una fracción mínima de la ventana de 3 s del búfer: no
            // arriesga a perder un paquete entero entre dos sondeos, y no
            // ocupa la CPU con un bucle activo.
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    // Al parar sí se vacía la cola del remuestreador de cada pista que siga
    // viva: a diferencia de `pipewire_src`, que no lo hace y pierde hasta
    // ~21 ms por pista en cada cierre (deuda anotada en el PLAN de esa HU),
    // aquí no hay ninguna razón para tirar el final de la clase. Y las pistas
    // que se quedaron sin dispositivo se rellenan de silencio hasta el final,
    // para que todos los WAV duren lo mismo que la sesión.
    let fin_ms = inicio.elapsed().as_millis() as i64;
    for p in pistas.iter_mut() {
        match p.abierta.as_mut() {
            Some(c) => cerrar_pista(c, fin_ms, &tx),
            None => {
                let relleno = p.control.relleno_final(p.track, fin_ms);
                if !relleno.is_empty() {
                    tracing::info!(
                        pista = p.nombre,
                        relleno_ms = relleno.iter().map(|t| t.duracion_ms()).sum::<i64>(),
                        "la pista no tenía dispositivo al detener, se rellena de silencio"
                    );
                    for trama in relleno {
                        let _ = tx.send(trama);
                    }
                }
            }
        }
    }

    tracing::info!("captura de WASAPI detenida");
    Ok(())
}

/// Abre (sin arrancar) una pista si se pidió, midiendo cuánto tarda (D4).
/// `None` es «no se pidió», que no es lo mismo que «se pidió y falló».
fn abrir_si_se_pidio(
    pedida: bool,
    enumerador: &IMMDeviceEnumerator,
    flujo: EDataFlow,
    loopback: bool,
    track: Track,
    nombre: &str,
) -> Option<Apertura> {
    if !pedida {
        return None;
    }
    let t0 = Instant::now();
    let resultado = unsafe { abrir_cliente(enumerador, flujo, loopback, track, 0) }
        .map(|c| (c, t0.elapsed().as_millis()));
    if resultado.is_err() {
        // El registro del fallo, con su causa, lo hace `registrar_arranque`.
        tracing::debug!(pista = nombre, "la apertura falló");
    }
    Some(resultado)
}

/// `Start` de una pista ya abierta. Un fallo aquí es transitorio: la pista
/// se tratará como no abierta al arrancar y se reintentará.
fn arrancar(apertura: Apertura) -> Apertura {
    let (c, ms) = apertura?;
    unsafe { c.cliente.Start() }.map_err(|e| FalloApertura::Transitorio(err(e)))?;
    Ok((c, ms))
}

/// Deja constancia del fallo, con su causa, de una pista que no abrió o no
/// arrancó al empezar.
fn registrar_arranque(apertura: Apertura, nombre: &str) -> Apertura {
    match &apertura {
        Ok(_) => {}
        Err(FalloApertura::Transitorio(causa)) => {
            tracing::warn!(pista = nombre, error = %causa, "no se pudo abrir la pista")
        }
        Err(FalloApertura::Permanente { id, causa }) => tracing::error!(
            pista = nombre,
            id_dispositivo = %id,
            error = %causa,
            "el dispositivo por defecto no sirve para capturar"
        ),
    }
    apertura
}

/// Construye la `Pista` con el estado que corresponde a cómo salió su
/// apertura al arrancar.
fn nueva_pista(
    nombre: &'static str,
    track: Track,
    flujo: EDataFlow,
    loopback: bool,
    apertura: Option<Apertura>,
    ahora: Duration,
) -> Pista {
    let (abierta, control, ultimo_id) = match apertura {
        None => (None, ControlPista::no_pedida(), None),
        Some(Ok((c, apertura_ms))) => {
            tracing::info!(
                pista = nombre,
                id_dispositivo = %c.id_dispositivo,
                apertura_ms = apertura_ms as u64,
                buffer_tramas = c.buffer_tramas,
                buffer_ms = c.buffer_ms,
                "pista abierta"
            );
            let id = c.id_dispositivo.clone();
            (Some(c), ControlPista::viva(id.clone(), ahora), Some(id))
        }
        Some(Err(FalloApertura::Permanente { id, .. })) => (
            None,
            ControlPista::ausente_desde_el_arranque(id.clone(), ahora),
            Some(id),
        ),
        Some(Err(FalloApertura::Transitorio(_))) => {
            (None, ControlPista::sin_abrir_al_arrancar(ahora), None)
        }
    };
    Pista {
        nombre,
        track,
        flujo,
        loopback,
        abierta,
        control,
        ultimo_id,
        motivo: "",
        sondeo_fallando: false,
    }
}

/// Cierra el cliente de la pista, si lo hay, y la deja en «reabriendo» con el
/// contador leído **después** de vaciar el remuestreador. Un solo cierre por
/// evento: si ya no había cliente (el error de lectura y el cambio de
/// dispositivo coincidieron) no hace nada de COM.
fn soltar_para_reabrir(
    p: &mut Pista,
    motivo: &'static str,
    inicio: Instant,
    tx: &Sender<AudioFrame>,
) {
    let ahora = inicio.elapsed();
    let muestras = match p.abierta.as_mut() {
        Some(c) => {
            cerrar_pista(c, ahora.as_millis() as i64, tx);
            c.pista.muestras_emitidas()
        }
        None => 0,
    };
    p.abierta = None;
    p.motivo = motivo;
    p.control.empezar_reapertura(ahora, muestras);
}

/// Intenta abrir de nuevo la pista `i`. Antes de hacerlo drena la otra, que
/// sigue grabando: abrir un cliente WASAPI ha llegado a tardar 2,5 s (medido
/// con el micrófono) y durante ese tiempo no se lee nada; el búfer de 3 s
/// aguanta ese tiempo y las tramas se fechan por su captura, no por cuándo
/// se leen (D4).
fn reabrir(
    pistas: &mut [Pista; 2],
    i: usize,
    enumerador: &IMMDeviceEnumerator,
    inicio: Instant,
    reloj: &mut RelojCaptura,
    tx: &Sender<AudioFrame>,
) {
    for (j, otra) in pistas.iter_mut().enumerate() {
        if j == i {
            continue;
        }
        if let Some(c) = otra.abierta.as_mut() {
            // Si falla, el siguiente giro del bucle vuelve a leer, vuelve a
            // fallar y la manda a reabrir por el camino normal.
            if let Err(e) = procesar_paquetes(c, inicio, reloj, tx) {
                tracing::warn!(pista = otra.nombre, error = %e, "fallo al drenar antes de reabrir");
            }
        }
    }

    let ahora = inicio.elapsed();
    let p = &mut pistas[i];
    let contador = p.control.contador();
    let resultado = unsafe { abrir_y_arrancar(enumerador, p.flujo, p.loopback, p.track, contador) };
    match resultado {
        Ok((mut c, apertura_ms)) => {
            // El hueco de la pista sin dispositivo, en tramas de 1 s, hasta el
            // instante en que empezó este intento: así el primer paquete del
            // cliente nuevo solo rellena lo que duró la propia apertura.
            for trama in c.pista.rellenar_hasta(ahora.as_millis() as i64) {
                let _ = tx.send(trama);
            }
            let sin_pista_ms = p
                .control
                .sin_dispositivo_desde()
                .map(|desde| inicio.elapsed().saturating_sub(desde).as_millis() as u64);
            tracing::info!(
                pista = p.nombre,
                id_anterior = p.ultimo_id.as_deref().unwrap_or("-"),
                id_nuevo = %c.id_dispositivo,
                motivo = p.motivo,
                apertura_ms = apertura_ms as u64,
                buffer_ms = c.buffer_ms,
                muestras_ya_emitidas = contador,
                sin_pista_ms = sin_pista_ms.unwrap_or(0),
                "pista reabierta"
            );
            p.ultimo_id = Some(c.id_dispositivo.clone());
            p.control
                .al_abrir(c.id_dispositivo.clone(), inicio.elapsed());
            p.abierta = Some(c);
        }
        Err(FalloApertura::Transitorio(e)) => {
            // Cada intento, en `debug`: el aviso periódico ya lo da
            // `toca_avisar`, y uno por intento llenaría el registro.
            tracing::debug!(pista = p.nombre, error = %e, "reintento de apertura fallido");
            // El instante de **después** del intento: con aperturas de 2,5 s,
            // contar desde antes encadenaba un intento tras otro sin atender a
            // la otra pista.
            p.control.intento_fallido(inicio.elapsed());
        }
        Err(FalloApertura::Permanente { id, causa }) => {
            tracing::error!(
                pista = p.nombre,
                id_dispositivo = %id,
                error = %causa,
                "fallo permanente con este dispositivo, no se reintentará hasta que cambie el \
                 dispositivo por defecto"
            );
            p.ultimo_id = Some(id.clone());
            p.control.fallo_permanente(id);
        }
    }
}

/// Abre y arranca un cliente, y devuelve cuánto tardó en milisegundos
/// (`Activate` + `GetMixFormat` + `Initialize` + `Start`).
unsafe fn abrir_y_arrancar(
    enumerador: &IMMDeviceEnumerator,
    flujo: EDataFlow,
    loopback: bool,
    track: Track,
    muestras_previas: u64,
) -> Apertura {
    let t0 = Instant::now();
    let c = unsafe { abrir_cliente(enumerador, flujo, loopback, track, muestras_previas) }?;
    unsafe { c.cliente.Start() }.map_err(|e| FalloApertura::Transitorio(err(e)))?;
    Ok((c, t0.elapsed().as_millis()))
}

/// Vacía el remuestreador de la pista y detiene su `IAudioClient`.
///
/// Se llama tanto al terminar la sesión con normalidad (para las pistas que
/// sigan vivas en ese momento) como cuando una pista pierde su dispositivo a
/// mitad de grabación (ver el bucle de arriba): en los dos casos hay
/// que devolver las últimas muestras pendientes del remuestreador antes de
/// soltar el dispositivo, no solo en el camino feliz -- si no, se pierde
/// audio ya capturado que nunca llegó a escribirse.
///
/// Si el propio `vaciar()` falla (el remuestreador, no la ausencia de
/// muestras pendientes), el error se registra en vez de descartarse --
/// invariante 5, y el mismo tratamiento que ya recibe este tipo de fallo en
/// `procesar_paquetes`, unas líneas más abajo.
fn cerrar_pista(c: &mut CapturaAbierta, fin_ms: i64, tx: &Sender<AudioFrame>) {
    match c.pista.vaciar(fin_ms) {
        Ok(Some(frame)) => {
            let _ = tx.send(frame);
        }
        Ok(None) => {}
        Err(e) => tracing::warn!(
            error = %e,
            "fallo de remuestreo al vaciar la pista, se pierden las últimas muestras"
        ),
    }
    unsafe {
        let _ = c.cliente.Stop();
    }
}

/// Drena todos los paquetes disponibles de una pista y envía lo que
/// `sincronia::PistaCapturada` vaya devolviendo. Devuelve `true` si había algo
/// que leer, para que el bucle solo duerma cuando de verdad no hay trabajo.
fn procesar_paquetes(
    c: &mut CapturaAbierta,
    inicio: Instant,
    reloj: &mut RelojCaptura,
    tx: &Sender<AudioFrame>,
) -> Result<bool> {
    let mut hubo = false;

    loop {
        let disponibles = unsafe { c.captura.GetNextPacketSize() }.map_err(err)?;
        if disponibles == 0 {
            break;
        }
        hubo = true;

        let mut puntero: *mut u8 = std::ptr::null_mut();
        let mut num_frames: u32 = 0;
        let mut banderas: u32 = 0;
        let mut posicion_qpc: u64 = 0;
        unsafe {
            c.captura.GetBuffer(
                &mut puntero,
                &mut num_frames,
                &mut banderas,
                None,
                Some(&mut posicion_qpc),
            )
        }
        .map_err(err)?;

        // Se fecha cuándo se **capturó** el bloque, no cuándo se leyó: tras un
        // bloqueo del hilo el búfer entrega de golpe segundos de audio, y
        // fecharlos al leer los dejaba todos en el instante del drenaje
        // (pista adelantada de forma permanente). Ver `RelojCaptura`.
        let leido_ms = inicio.elapsed().as_millis() as i64;
        let timestamp_ms =
            if posicion_qpc != 0 && banderas & AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR == 0 {
                reloj.fechar(posicion_qpc, num_frames, c.frecuencia, leido_ms)
            } else {
                // Sin posición utilizable: cuánto falta por leer detrás de este
                // bloque dice cuánto hace que se capturó. `GetCurrentPadding`
                // incluye el bloque que aún no se ha liberado.
                let pendientes = unsafe { c.cliente.GetCurrentPadding() }
                    .map(|p| p.saturating_sub(num_frames))
                    .unwrap_or(0);
                fecha_por_pendientes(leido_ms, pendientes, c.frecuencia)
            };
        let tam_bytes = num_frames as usize * c.bloque_bytes as usize;

        let resultado = if banderas & AUDCLNT_BUFFERFLAGS_SILENT != 0 || puntero.is_null() {
            // La documentación de `GetBuffer` es explícita: con esta
            // bandera el puntero puede no señalar a datos válidos, y hay
            // que tratar el bloque como silencio en vez de leerlo.
            let silencio = vec![0u8; tam_bytes];
            c.pista.procesar_paquete(timestamp_ms, &silencio)
        } else {
            let bytes = unsafe { std::slice::from_raw_parts(puntero, tam_bytes) };
            c.pista.procesar_paquete(timestamp_ms, bytes)
        };

        let liberado = unsafe { c.captura.ReleaseBuffer(num_frames) };

        // La trama se entrega **antes** de propagar un fallo de
        // `ReleaseBuffer`: `procesar_paquete` ya la contó en el contador de
        // la pista (con su relleno), y descartarla dejaba todo lo posterior
        // desplazado por ese tamaño.
        match resultado {
            Ok(Some(frame)) => {
                let _ = tx.send(frame);
            }
            Ok(None) => {}
            Err(e) => tracing::warn!(error = %e, "fallo de remuestreo, bloque descartado"),
        }
        liberado.map_err(err)?;
    }

    Ok(hubo)
}

/// Abre y activa un `IAudioClient` sobre el dispositivo por defecto de
/// `flujo`. `loopback` distingue el caso del bucle del sistema (que se abre
/// en modo captura sobre el dispositivo de **salida**) del micrófono normal.
///
/// `muestras_previas` es el contador con el que arranca la pista: cero al
/// abrir por primera vez, y lo ya emitido al reabrir (ver
/// `PistaCapturada::continuar`).
unsafe fn abrir_cliente(
    enumerador: &IMMDeviceEnumerator,
    flujo: EDataFlow,
    loopback: bool,
    track: Track,
    muestras_previas: u64,
) -> std::result::Result<CapturaAbierta, FalloApertura> {
    fn transitorio<E: std::fmt::Display>(e: E) -> FalloApertura {
        FalloApertura::Transitorio(err(e))
    }

    let dispositivo: IMMDevice =
        unsafe { enumerador.GetDefaultAudioEndpoint(flujo, eConsole) }.map_err(transitorio)?;
    let id_dispositivo =
        unsafe { id_de_dispositivo(&dispositivo) }.map_err(FalloApertura::Transitorio)?;

    let cliente: IAudioClient =
        unsafe { dispositivo.Activate(CLSCTX_ALL, None) }.map_err(transitorio)?;

    let formato_ptr = unsafe { cliente.GetMixFormat() }.map_err(transitorio)?;
    let campos_formato = unsafe { leer_formato(formato_ptr) };

    let (formato_nativo, canales, frecuencia, bloque_bytes) = match campos_formato {
        Ok(f) => f,
        Err(causa) => {
            unsafe { CoTaskMemFree(Some(formato_ptr as *const c_void)) };
            // Un formato que no se sabe leer no mejora por reintentar: se
            // distingue de un fallo cualquiera para no abrir y cerrar el
            // dispositivo cada 2 s durante toda la clase.
            return Err(FalloApertura::Permanente {
                id: id_dispositivo,
                causa,
            });
        }
    };

    let banderas: u32 = if loopback {
        AUDCLNT_STREAMFLAGS_LOOPBACK
    } else {
        0
    };

    let resultado_init = unsafe {
        cliente.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            banderas,
            DURACION_BUFFER_HNS,
            0,
            formato_ptr,
            None,
        )
    };
    // `GetMixFormat` reserva la estructura; liberarla es responsabilidad de
    // quien la pide, y hay que hacerlo pase lo que pase con `Initialize`.
    unsafe { CoTaskMemFree(Some(formato_ptr as *const c_void)) };
    resultado_init.map_err(transitorio)?;

    let captura: IAudioCaptureClient = unsafe { cliente.GetService() }.map_err(transitorio)?;
    let buffer_tramas = unsafe { cliente.GetBufferSize() }.map_err(transitorio)?;
    let buffer_ms = buffer_tramas as u64 * 1000 / frecuencia as u64;
    if buffer_ms < BUFFER_MINIMO_MS {
        // No se acepta en silencio: una apertura lenta perdería audio de la
        // otra pista y habría que abrir en un hilo auxiliar.
        tracing::warn!(
            buffer_tramas,
            buffer_ms,
            minimo_ms = BUFFER_MINIMO_MS,
            "Windows concedió un búfer menor que la apertura más lenta medida"
        );
    }
    let pista =
        PistaCapturada::continuar(track, canales, formato_nativo, frecuencia, muestras_previas)
            .map_err(|causa| FalloApertura::Permanente {
                id: id_dispositivo.clone(),
                causa,
            })?;

    Ok(CapturaAbierta {
        cliente,
        captura,
        pista,
        bloque_bytes,
        id_dispositivo,
        buffer_tramas,
        buffer_ms,
        frecuencia,
    })
}

/// Lee `wFormatTag` / `wBitsPerSample` (y, si hace falta, el `SubFormat` de
/// un `WAVEFORMATEXTENSIBLE`) para decidir en qué formato nativo llegan las
/// muestras. Un formato que no sea `f32`, PCM de 16 bits o PCM de 32 bits
/// produce un error explícito en vez de una suposición: interpretar mal el
/// ancho de muestra desalinea todos los bytes que vengan detrás, y eso
/// corrompería la pista entera en vez de sonar simplemente mal.
unsafe fn leer_formato(formato: *const WAVEFORMATEX) -> Result<(FormatoNativo, usize, u32, u32)> {
    // mmreg.h.
    const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
    const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

    let f = unsafe { &*formato };

    let es_float = if f.wFormatTag == WAVE_FORMAT_EXTENSIBLE {
        // `GetMixFormat` en modo compartido casi siempre devuelve esto en
        // vez de un `wFormatTag` directo: el formato real va en el
        // `SubFormat` de la estructura extendida.
        let ext = unsafe { &*(formato as *const WAVEFORMATEXTENSIBLE) };
        // La copia a una variable local no sobra: `WAVEFORMATEXTENSIBLE`
        // está empaquetado, y comparar con `==` directamente tomaría una
        // referencia a `SubFormat`, que puede estar desalineada —
        // comportamiento indefinido, y el compilador lo rechaza. Leer el
        // campo por valor (`GUID` es `Copy`) es lo único que un struct
        // empaquetado permite.
        let subformato = ext.SubFormat;
        subformato == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
    } else {
        f.wFormatTag == WAVE_FORMAT_IEEE_FLOAT
    };

    let nativo = match (es_float, f.wBitsPerSample) {
        (true, 32) => FormatoNativo::F32,
        (false, 32) => FormatoNativo::I32,
        (false, 16) => FormatoNativo::I16,
        (flotante, bits) => {
            return Err(AudioError::Inicio(format!(
                "formato de audio del dispositivo no soportado: float={flotante}, bits={bits}"
            )))
        }
    };

    Ok((
        nativo,
        f.nChannels.max(1) as usize,
        f.nSamplesPerSec.max(1),
        f.nBlockAlign as u32,
    ))
}

/// Enumera micrófonos y salidas activos del sistema, con su identificador
/// real (`IMMDevice::GetId`) y marcando cuál es el dispositivo por defecto
/// de cada dirección.
///
/// El AC 4 de la HU pide "los dispositivos reales del sistema", en plural:
/// la primera versión de esta función devolvía solo el par por defecto
/// (`GetDefaultAudioEndpoint` × 2), que no es una enumeración. Aquí se usa
/// `IMMDeviceEnumerator::EnumAudioEndpoints` sobre las dos direcciones para
/// listar todos los dispositivos activos, no solo esos dos.
///
/// El nombre visible **no** sale de `IPropertyStore`/`PROPVARIANT` (el
/// nombre "amigable" real de Windows para cada dispositivo): esa API
/// entrega el valor a través de una unión sin tipar
/// (`PROPVARIANT::Anonymous`) y necesita *features* de Cargo que hoy no
/// están en `Cargo.toml` (`Win32_UI_Shell_PropertiesSystem` para
/// `IPropertyStore`, `Win32_Devices_Properties` para
/// `PKEY_Device_FriendlyName`), ninguna verificable sin compilador en este
/// entorno: es, con diferencia, la API COM más frágil de acertar a ciegas
/// de todo este archivo. Se usa en su lugar un nombre genérico con el
/// índice y si es el dispositivo por defecto; el nombre amigable real queda
/// como deuda explícita (ver HANDOFF), no como una enumeración a medias sin
/// decirlo.
pub fn dispositivos() -> Result<Vec<DeviceInfo>> {
    let _com = ComGuard::iniciar()?;

    let enumerador: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(err)?;

    let mut lista = Vec::new();

    // Un fallo al enumerar una dirección (por ejemplo, el subsistema de
    // audio del micrófono deshabilitado por política) no debe vaciar la
    // lista de la otra dirección: se registra y se sigue con lo que haya.
    let resultado_mic =
        unsafe { enumerar_flujo(&enumerador, eCapture, false, "Micrófono", &mut lista) };
    if let Err(e) = resultado_mic {
        tracing::warn!(error = %e, "no se pudieron enumerar los micrófonos");
    }

    let resultado_salida =
        unsafe { enumerar_flujo(&enumerador, eRender, true, "Salida", &mut lista) };
    if let Err(e) = resultado_salida {
        tracing::warn!(error = %e, "no se pudieron enumerar las salidas");
    }

    Ok(lista)
}

/// Enumera todos los dispositivos activos de una dirección (captura o
/// *render*) y los añade a `lista`, marcando cuál es el que devolvería
/// `GetDefaultAudioEndpoint` para esa misma dirección.
unsafe fn enumerar_flujo(
    enumerador: &IMMDeviceEnumerator,
    flujo: EDataFlow,
    es_salida: bool,
    etiqueta: &str,
    lista: &mut Vec<DeviceInfo>,
) -> Result<()> {
    // Si no hay dispositivo por defecto para esta dirección (por ejemplo,
    // sin ningún micrófono conectado), no es un error de esta función:
    // sencillamente ningún elemento de la lista se marca `por_defecto`.
    let id_por_defecto = unsafe { id_de_endpoint_por_defecto(enumerador, flujo) }.ok();

    let coleccion: IMMDeviceCollection =
        unsafe { enumerador.EnumAudioEndpoints(flujo, DEVICE_STATE(DEVICE_STATE_ACTIVE)) }
            .map_err(err)?;
    let total = unsafe { coleccion.GetCount() }.map_err(err)?;

    for i in 0..total {
        let dispositivo = unsafe { coleccion.Item(i) }.map_err(err)?;
        let puntero = unsafe { dispositivo.GetId() }.map_err(err)?;
        // Mismo cuidado que en `id_de_endpoint_por_defecto`: el puntero se
        // libera pase lo que pase con `to_string()`, no solo en el camino
        // feliz.
        let resultado_id = unsafe { puntero.to_string() }.map_err(err);
        unsafe { CoTaskMemFree(Some(puntero.0 as *const c_void)) };
        let id = resultado_id?;

        let por_defecto = id_por_defecto.as_deref() == Some(id.as_str());
        let numero = i + 1;
        lista.push(DeviceInfo {
            id,
            nombre: if por_defecto {
                format!("{etiqueta} {numero} (predeterminado)")
            } else {
                format!("{etiqueta} {numero}")
            },
            descripcion: if es_salida {
                "Loopback de esta salida".into()
            } else {
                "Dispositivo de entrada".into()
            },
            es_monitor: es_salida,
            por_defecto,
        });
    }

    Ok(())
}

unsafe fn id_de_endpoint_por_defecto(
    enumerador: &IMMDeviceEnumerator,
    flujo: EDataFlow,
) -> Result<String> {
    let dispositivo =
        unsafe { enumerador.GetDefaultAudioEndpoint(flujo, eConsole) }.map_err(err)?;
    unsafe { id_de_dispositivo(&dispositivo) }
}

/// El identificador de un dispositivo (`IMMDevice::GetId`) como `String`.
unsafe fn id_de_dispositivo(dispositivo: &IMMDevice) -> Result<String> {
    let puntero = unsafe { dispositivo.GetId() }.map_err(err)?;
    // `to_string()` puede fallar si la cadena UTF-16 que reservó COM no es
    // válida (`PWSTR::to_string()` contempla ese caso): el puntero hay que
    // liberarlo también en ese camino de error, no solo en el feliz, o se
    // pierde el búfer que reservó `GetId()`.
    let resultado = unsafe { puntero.to_string() }.map_err(err);
    unsafe { CoTaskMemFree(Some(puntero.0 as *const c_void)) };
    resultado
}

/// `CoInitializeEx` es por hilo: hace falta uno para cada hilo que hable con
/// WASAPI (aquí, solo el de captura), y desparejarlo con `CoUninitialize` si
/// no se quiere dejar ese hilo en un estado COM a medio inicializar.
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

fn err<E: std::fmt::Display>(e: E) -> AudioError {
    AudioError::Inicio(e.to_string())
}
