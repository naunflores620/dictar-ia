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
//! Sondeo, no eventos: WASAPI admite un modo dirigido por eventos
//! (`AUDCLNT_STREAMFLAGS_EVENTCALLBACK`), pero tiene historial de no
//! dispararse de forma fiable en *loopback* en algunas versiones de
//! Windows. Un sondeo corto y regular es más código, pero no depende de esa
//! garantía.

use crate::sincronia::{FormatoNativo, PistaCapturada};
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

// audioclient.h. Se usan los valores crudos y no las constantes con nombre
// del crate `windows`: son fijos en la ABI de Windows y no van a cambiar, así
// que evitan depender de si esta versión del crate las expone como `u32`
// puro o como un newtype que necesitaría envoltura para las operaciones a
// nivel de bit de más abajo.
const AUDCLNT_STREAMFLAGS_LOOPBACK: u32 = 0x0010_0000;
const AUDCLNT_BUFFERFLAGS_SILENT: u32 = 0x2;

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

/// Ventana de captura. Bastante para no perder paquetes entre dos sondeos
/// del bucle (que como mucho duerme 10 ms), y corta para que la latencia de
/// la transcripción en vivo no se note.
const DURACION_BUFFER_HNS: i64 = 200 * 10_000;

pub fn iniciar(cfg: CaptureConfig) -> Result<(Receiver<AudioFrame>, Box<dyn CaptureSession>)> {
    if !cfg.capturar_microfono && !cfg.capturar_sistema {
        return Err(AudioError::Inicio(
            "hay que capturar al menos una pista".into(),
        ));
    }

    let (tx_frames, rx_frames) = mpsc::channel::<AudioFrame>();
    let (tx_parar, rx_parar) = mpsc::channel::<()>();
    let (tx_listo, rx_listo) = mpsc::channel::<Result<()>>();

    let inicio = Instant::now();

    let hilo = std::thread::Builder::new()
        .name("dictar-wasapi".into())
        .spawn(move || {
            let r = bucle(cfg, tx_frames, rx_parar, inicio, &tx_listo);
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
}

fn bucle(
    cfg: CaptureConfig,
    tx: Sender<AudioFrame>,
    rx_parar: Receiver<()>,
    inicio: Instant,
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
    // del sistema, la sesión sigue solo con micrófono (o al revés).
    let mut mic = if cfg.capturar_microfono {
        match unsafe { abrir_cliente(&enumerador, eCapture, false, Track::Mic) } {
            Ok(c) => Some(c),
            Err(e) => {
                tracing::warn!(error = %e, "no se pudo abrir el micrófono");
                None
            }
        }
    } else {
        None
    };

    let mut sistema = if cfg.capturar_sistema {
        match unsafe { abrir_cliente(&enumerador, eRender, true, Track::System) } {
            Ok(c) => Some(c),
            Err(e) => {
                tracing::warn!(error = %e, "no se pudo abrir el bucle del sistema");
                None
            }
        }
    } else {
        None
    };

    // El mismo aislamiento por pista que en la apertura, unas líneas más
    // arriba: si una arranca mal (por ejemplo `AUDCLNT_E_DEVICE_IN_USE`,
    // otro proceso con acceso exclusivo al dispositivo) se descarta solo
    // esa pista, no toda la sesión. La comprobación de `pistas_a_grabar` va
    // después de intentar los dos `Start()`, no antes, para que también
    // atrape el caso de que las dos pistas se abrieran pero ninguna llegara
    // a arrancar.
    if let Some(c) = &mic {
        if let Err(e) = unsafe { c.cliente.Start() } {
            tracing::warn!(error = %e, "el micrófono no pudo arrancar");
            mic = None;
        }
    }
    if let Some(c) = &sistema {
        if let Err(e) = unsafe { c.cliente.Start() } {
            tracing::warn!(error = %e, "el bucle del sistema no pudo arrancar");
            sistema = None;
        }
    }

    if let Err(e) = crate::sincronia::pistas_a_grabar(mic.is_some(), sistema.is_some()) {
        let _ = tx_listo.send(Err(e));
        return Ok(());
    }

    tracing::info!(
        microfono = mic.is_some(),
        sistema = sistema.is_some(),
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

        // Un error COM aquí (por ejemplo `AUDCLNT_E_DEVICE_INVALIDATED`: el
        // usuario desconecta el dispositivo a mitad de clase) cierra solo la
        // pista que falló, con su `vaciar()`/`Stop()` de rigor -- la otra
        // sigue grabando. Antes esto propagaba el error con `?`, mataba
        // `bucle()` entero y se saltaba la limpieza de las dos pistas: se
        // perdía el resto de la grabación en ambas, no solo en la que
        // falló, y eso rompe el invariante 1 (nada se escribe si no se
        // vació primero el remuestreador). Ver la decisión del PLAN: "la
        // pista termina y se registra en el log; la otra sigue".
        if let Some(c) = mic.as_mut() {
            match procesar_paquetes(c, inicio, &tx) {
                Ok(datos) => hubo_datos |= datos,
                Err(e) => {
                    tracing::error!(error = %e, "el micrófono falló, se cierra su pista");
                    let fin_ms = inicio.elapsed().as_millis() as i64;
                    cerrar_pista(c, fin_ms, &tx);
                    mic = None;
                }
            }
        }
        if let Some(c) = sistema.as_mut() {
            match procesar_paquetes(c, inicio, &tx) {
                Ok(datos) => hubo_datos |= datos,
                Err(e) => {
                    tracing::error!(error = %e, "el bucle del sistema falló, se cierra su pista");
                    let fin_ms = inicio.elapsed().as_millis() as i64;
                    cerrar_pista(c, fin_ms, &tx);
                    sistema = None;
                }
            }
        }

        // Si ninguna pista sigue viva (no solo una), faltaba un `break`
        // aquí: el bucle seguía sondeando cada 10 ms sin producir ningún
        // `AudioFrame` más y sin soltar `tx`, así que el canal nunca se
        // cerraba y quien consume la captura no tenía forma de distinguir
        // "no hay audio ahora mismo" de "la sesión ya terminó de verdad". La
        // política -- seguir con lo que quede, terminar si no queda nada --
        // vive en `sincronia::alguna_pista_sigue_viva`, no aquí, por la
        // misma razón que `pistas_a_grabar`: se puede probar sin COM.
        if !crate::sincronia::alguna_pista_sigue_viva(mic.is_some(), sistema.is_some()) {
            tracing::error!("ninguna pista de captura sigue viva, se termina la sesión");
            break;
        }

        if !hubo_datos {
            // 10 ms es una fracción pequeña de la ventana de 200 ms del
            // búfer: no arriesga a perder un paquete entero entre dos
            // sondeos, y no ocupa la CPU con un bucle activo.
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    // Al parar sí se vacía la cola del remuestreador de cada pista que siga
    // viva (la que ya falló arriba se cerró, con su propio vaciado, en el
    // momento del fallo, no aquí): a diferencia de `pipewire_src`, que no lo
    // hace y pierde hasta ~21 ms por pista en cada cierre (deuda anotada en
    // el PLAN de esta HU), aquí no hay ninguna razón para tirar el final de
    // la clase.
    let fin_ms = inicio.elapsed().as_millis() as i64;
    if let Some(c) = mic.as_mut() {
        cerrar_pista(c, fin_ms, &tx);
    }
    if let Some(c) = sistema.as_mut() {
        cerrar_pista(c, fin_ms, &tx);
    }

    tracing::info!("captura de WASAPI detenida");
    Ok(())
}

/// Vacía el remuestreador de la pista y detiene su `IAudioClient`.
///
/// Se llama tanto al terminar la sesión con normalidad (para las pistas que
/// sigan vivas en ese momento) como cuando una pista muere sola por un error
/// COM a mitad de grabación (ver el bucle de arriba): en los dos casos hay
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
        unsafe {
            c.captura
                .GetBuffer(&mut puntero, &mut num_frames, &mut banderas, None, None)
        }
        .map_err(err)?;

        // El reloj monótono, no el conteo de muestras: ver la decisión de
        // diseño en `sincronia.rs`.
        let timestamp_ms = inicio.elapsed().as_millis() as i64;
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

        unsafe { c.captura.ReleaseBuffer(num_frames) }.map_err(err)?;

        match resultado {
            Ok(Some(frame)) => {
                let _ = tx.send(frame);
            }
            Ok(None) => {}
            Err(e) => tracing::warn!(error = %e, "fallo de remuestreo, bloque descartado"),
        }
    }

    Ok(hubo)
}

/// Abre y activa un `IAudioClient` sobre el dispositivo por defecto de
/// `flujo`. `loopback` distingue el caso del bucle del sistema (que se abre
/// en modo captura sobre el dispositivo de **salida**) del micrófono normal.
unsafe fn abrir_cliente(
    enumerador: &IMMDeviceEnumerator,
    flujo: EDataFlow,
    loopback: bool,
    track: Track,
) -> Result<CapturaAbierta> {
    let dispositivo: IMMDevice =
        unsafe { enumerador.GetDefaultAudioEndpoint(flujo, eConsole) }.map_err(err)?;

    let cliente: IAudioClient = unsafe { dispositivo.Activate(CLSCTX_ALL, None) }.map_err(err)?;

    let formato_ptr = unsafe { cliente.GetMixFormat() }.map_err(err)?;
    let campos_formato = unsafe { leer_formato(formato_ptr) };

    let (formato_nativo, canales, frecuencia, bloque_bytes) = match campos_formato {
        Ok(f) => f,
        Err(e) => {
            unsafe { CoTaskMemFree(Some(formato_ptr as *const c_void)) };
            return Err(e);
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
    resultado_init.map_err(err)?;

    let captura: IAudioCaptureClient = unsafe { cliente.GetService() }.map_err(err)?;
    let pista = PistaCapturada::nueva(track, canales, formato_nativo, frecuencia)?;

    Ok(CapturaAbierta {
        cliente,
        captura,
        pista,
        bloque_bytes,
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
