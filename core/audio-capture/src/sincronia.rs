//! Lógica pura de la captura: sin COM, sin AAudio, sin hilos, sin `#[cfg]`.
//!
//! Existe como archivo aparte por una razón muy concreta, no por estética: es
//! lo único de estas HU que se puede compilar y ejecutar en el CI de Linux. Un
//! `wasapi_src.rs` gateado a `cfg(windows)` no compila allí, y en el job de
//! Windows el CI solo hacía `cargo check` hasta HU-01 (ver
//! `.github/workflows/ci.yml`), que tipa-chequea pero no ejecuta nada. Lo
//! mismo, y peor, le pasa a `aaudio_src.rs`: ningún job ejecuta nada en
//! Android. Sin este módulo, ninguna prueba de esas historias correría de
//! verdad en ningún sitio.
//!
//! Nació con HU-01 como «lógica pura de WASAPI», y HU-02 lo reutilizó tal
//! cual: normalizar el formato nativo, calcular el relleno y decidir con qué
//! pistas se sigue no tienen nada de Windows. Ese fue el cobro de haberlo
//! separado. Lo único que se añadió para Android es lo que sí le es propio
//! —[`formato_desde_aaudio`] y [`pistas_en_android`]— y por el mismo motivo:
//! poder probarlo sin un móvil delante.
//!
//! ## El reloj y el silencio
//!
//! `pipewire_src` deriva el `timestamp_ms` del conteo de muestras emitidas, y
//! eso es seguro solo porque las dos pistas viven en el mismo grafo de
//! PipeWire, con un único reloj que resamplea cada nodo. WASAPI son dos
//! `IAudioClient` sobre dispositivos con osciladores independientes, y el
//! *loopback* no entrega ningún paquete durante el silencio absoluto. Copiar
//! el mecanismo de `pipewire_src` produciría deriva entre pistas y, peor,
//! dejaría el WAV en disco más corto que la duración real de cada silencio.
//!
//! La solución de aquí son dos cosas, no una disyuntiva: el llamador (
//! `wasapi_src`) lee el `timestamp_ms` del reloj monótono en cada paquete, y
//! [`muestras_de_relleno`] compara ese timestamp con lo ya escrito en la
//! pista para calcular cuánto silencio insertar antes del audio real.

use crate::mezcla::{a_mono, Remuestreador};
use crate::{AudioError, AudioFrame, Result, SAMPLE_RATE};
use dictar_domain::Track;
use std::time::Duration;

/// Formato nativo con el que WASAPI entrega las muestras del dispositivo.
///
/// `IAudioClient::GetMixFormat` casi siempre devuelve `f32` en el modo
/// compartido de Windows 10/11, pero el contrato de WASAPI no lo garantiza:
/// algunos dispositivos profesionales anuncian PCM entero de 16 o 32 bits, y
/// ahí hace falta normalizar antes de que `mezcla::a_mono` reciba la señal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatoNativo {
    I16,
    I32,
    F32,
}

/// Convierte una trama de bytes intercalados, en el formato nativo del
/// dispositivo, a muestras `f32` en `[-1, 1]`.
///
/// Se divide por `i16::MAX as f32 + 1.0` (32768.0) y no por `i16::MAX`
/// (32767.0): con 32767 la muestra mínima, `i16::MIN`, se saldría de
/// `[-1, 1]` por un pelo y el clamp de `EscritorPistas::escribir` la
/// recortaría en silencio, perdiendo la señal real en cada pico grabado a
/// todo volumen.
pub fn normalizar_a_f32(bytes: &[u8], formato: FormatoNativo) -> Vec<f32> {
    match formato {
        FormatoNativo::F32 => bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect(),
        FormatoNativo::I16 => bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes(*b) as f32 / (i16::MAX as f32 + 1.0))
            .collect(),
        FormatoNativo::I32 => bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| i32::from_le_bytes(*b) as f32 / (i32::MAX as f32 + 1.0))
            .collect(),
    }
}

/// Cuántas muestras de silencio (a 16 kHz) hay que insertar en una pista
/// antes del bloque que acaba de llegar.
///
/// `timestamp_ms` viene del reloj monótono, leído en el instante en que se
/// recibió el paquete — **no** del conteo de muestras ya emitidas: derivarlo
/// de ahí (lo que hace `pipewire_src`, a salvo porque comparte grafo) haría
/// que lo "esperado" y lo "emitido" fueran siempre la misma cifra por
/// construcción, y el hueco nunca se detectaría.
///
/// `muestras_del_paquete` (las que trae, ya remuestreadas, el bloque que
/// dispara este cálculo) también se descuenta de lo esperado. Sin ese
/// descuento el paquete que reanuda tras un silencio se contaba dos veces
/// —como relleno y como audio real, con el mismo tamaño de más en cada
/// transición silencio→audio del loopback— porque `timestamp_ms` se lee
/// *después* de que `GetBuffer` ya entregó ese audio, no antes de que
/// empezara a sonar.
pub fn muestras_de_relleno(
    timestamp_ms: i64,
    muestras_ya_emitidas: u64,
    muestras_del_paquete: u64,
) -> u64 {
    let esperadas = (timestamp_ms.max(0) as u64 * SAMPLE_RATE as u64) / 1000;
    esperadas
        .saturating_sub(muestras_ya_emitidas)
        .saturating_sub(muestras_del_paquete)
}

/// Qué pistas hay que grabar de verdad, a partir de cuáles se consiguieron
/// abrir.
///
/// La apertura real de cada `IAudioClient` vive en `wasapi_src` y no se
/// puede simular aquí sin envolver COM entero — sobreingeniería para probar
/// lo que, en el fondo, es un `if`—. Lo que sí es responsabilidad de esta
/// función, y sí se puede probar sin hardware, es la política: seguir con lo
/// que haya, o fallar si no quedó ninguna pista.
pub fn pistas_a_grabar(microfono_abierto: bool, sistema_abierto: bool) -> Result<Vec<Track>> {
    let mut pistas = Vec::new();
    if microfono_abierto {
        pistas.push(Track::Mic);
    }
    if sistema_abierto {
        pistas.push(Track::System);
    }
    if pistas.is_empty() {
        return Err(AudioError::SinDispositivo("captura"));
    }
    Ok(pistas)
}

/// Cada cuánto se pregunta cuál es el dispositivo por defecto.
pub const INTERVALO_DE_SONDEO: Duration = Duration::from_secs(1);

/// Cuántos sondeos seguidos tienen que ver el mismo id nuevo antes de creer
/// que el cambio es real. Con uno solo, un auricular Bluetooth que alterna
/// entre su perfil estéreo y el de manos libres al abrirse el micrófono haría
/// reabrir las dos pistas en cada vaivén.
pub const SONDEOS_PARA_CONFIRMAR: u32 = 2;

/// Tiempo mínimo entre dos reaperturas de la misma pista provocadas por un
/// cambio de dispositivo. Es el segundo seguro contra el vaivén anterior: aun
/// con el id confirmado, no se reabre más de una vez cada 5 s.
pub const ENFRIAMIENTO_DE_REAPERTURA: Duration = Duration::from_secs(5);

/// Durante este tiempo desde que la pista se queda sin dispositivo se
/// reintenta rápido (cada [`REINTENTO_RAPIDO`]); pasado, cada
/// [`REINTENTO_LENTO`]. Un dispositivo que vuelve suele hacerlo en segundos;
/// uno que no ha vuelto en un minuto no merece abrir y cerrar el motor de
/// audio cada 2 s durante el resto de la clase.
pub const PRIMER_MINUTO: Duration = Duration::from_secs(60);
pub const REINTENTO_RAPIDO: Duration = Duration::from_secs(2);
pub const REINTENTO_LENTO: Duration = Duration::from_secs(5);

/// Separación mínima entre dos avisos mientras una pista sigue reabriendo.
pub const AVISO_MAXIMO_CADA: Duration = Duration::from_secs(30);

/// En qué punto está una pista de captura. Los tiempos son `Duration` desde el
/// inicio de la sesión (el mismo reloj monótono que fecha las tramas), así la
/// lógica se prueba sin dormir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EstadoPista {
    /// Hay un cliente abierto sobre `id_dispositivo`.
    Viva { id_dispositivo: String },
    /// No hay cliente: se perdió el dispositivo o nunca se pudo abrir. El
    /// contador de muestras vive aquí, y no en la `PistaCapturada` que ya no
    /// existe, para que ni los reintentos fallidos ni el tiempo que pase
    /// puedan reiniciarlo: es lo que permite rellenar el hueco al volver.
    Reabriendo {
        desde: Duration,
        muestras_emitidas: u64,
        ultimo_intento: Option<Duration>,
        intentos: u32,
    },
    /// Falló de forma permanente con ese dispositivo (formato que no se sabe
    /// leer): reintentar con él no sirve. Solo vuelve si el dispositivo por
    /// defecto pasa a ser otro.
    Ausente {
        id_fallido: Option<String>,
        muestras_emitidas: u64,
    },
    /// La configuración no pidió esta pista.
    NoPedida,
}

/// Lo que el sondeo del dispositivo por defecto le pide hacer al llamador.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Veredicto {
    Nada,
    /// El dispositivo por defecto cambió de forma estable: hay que cerrar el
    /// cliente actual (si lo hay) y abrir otro.
    Reabrir,
}

/// La máquina de estados de una pista, sin COM. `wasapi_src` ejecuta lo que
/// ella decide.
pub struct ControlPista {
    estado: EstadoPista,
    ultimo_sondeo: Duration,
    candidato: Option<(String, u32)>,
    ultima_apertura: Option<Duration>,
    ultimo_aviso: Duration,
}

impl ControlPista {
    fn con_estado(estado: EstadoPista, ahora: Duration) -> Self {
        Self {
            estado,
            ultimo_sondeo: ahora,
            candidato: None,
            ultima_apertura: None,
            ultimo_aviso: ahora,
        }
    }

    pub fn no_pedida() -> Self {
        Self::con_estado(EstadoPista::NoPedida, Duration::ZERO)
    }

    /// Una pista que acaba de abrirse sobre `id_dispositivo`.
    pub fn viva(id_dispositivo: String, ahora: Duration) -> Self {
        let mut c = Self::con_estado(EstadoPista::Viva { id_dispositivo }, ahora);
        c.ultima_apertura = Some(ahora);
        c
    }

    /// Una pista pedida que no se pudo abrir al arrancar, por un motivo que
    /// puede pasar: empieza con el contador a cero, de modo que el relleno
    /// cubra desde el inicio de la sesión en cuanto aparezca el dispositivo.
    pub fn sin_abrir_al_arrancar(ahora: Duration) -> Self {
        Self::con_estado(
            EstadoPista::Reabriendo {
                desde: ahora,
                muestras_emitidas: 0,
                ultimo_intento: Some(ahora),
                intentos: 1,
            },
            ahora,
        )
    }

    /// Una pista pedida cuyo dispositivo por defecto no sirve de forma
    /// permanente ya desde el arranque.
    pub fn ausente_desde_el_arranque(id_fallido: String, ahora: Duration) -> Self {
        Self::con_estado(
            EstadoPista::Ausente {
                id_fallido: Some(id_fallido),
                muestras_emitidas: 0,
            },
            ahora,
        )
    }

    pub fn estado(&self) -> &EstadoPista {
        &self.estado
    }

    pub fn es_viva(&self) -> bool {
        matches!(self.estado, EstadoPista::Viva { .. })
    }

    /// Muestras ya emitidas por una pista que no tiene cliente. Es lo que hay
    /// que pasarle a [`PistaCapturada::continuar`] al reabrir.
    pub fn contador(&self) -> u64 {
        match self.estado {
            EstadoPista::Reabriendo {
                muestras_emitidas, ..
            }
            | EstadoPista::Ausente {
                muestras_emitidas, ..
            } => muestras_emitidas,
            EstadoPista::Viva { .. } | EstadoPista::NoPedida => 0,
        }
    }

    /// Desde cuándo está sin dispositivo, si lo está. Para el registro.
    pub fn sin_dispositivo_desde(&self) -> Option<Duration> {
        match self.estado {
            EstadoPista::Reabriendo { desde, .. } => Some(desde),
            _ => None,
        }
    }

    /// Sondeo del dispositivo por defecto (D1). `id_por_defecto` solo se
    /// invoca cuando toca sondear, para que el llamador no haga una llamada
    /// COM por vuelta del bucle; devuelve `None` si el sondeo falló.
    ///
    /// Un fallo del sondeo —por ejemplo una reenumeración en curso— no cambia
    /// nada: es peor perder una pista sana por un tropiezo del sistema que
    /// esperar un segundo más.
    pub fn sondear(
        &mut self,
        ahora: Duration,
        id_por_defecto: impl FnOnce() -> Option<String>,
    ) -> Veredicto {
        // Una pista que está reabriendo ya prueba el dispositivo por defecto
        // en cada reintento, y una no pedida no tiene nada que seguir.
        if !matches!(
            self.estado,
            EstadoPista::Viva { .. } | EstadoPista::Ausente { .. }
        ) {
            return Veredicto::Nada;
        }
        if ahora.saturating_sub(self.ultimo_sondeo) < INTERVALO_DE_SONDEO {
            return Veredicto::Nada;
        }
        self.ultimo_sondeo = ahora;

        let Some(id) = id_por_defecto() else {
            // Un fallo corta la racha: «dos sondeos seguidos» no puede saltar
            // por encima de uno que no supo responder.
            self.candidato = None;
            return Veredicto::Nada;
        };

        let en_uso: Option<&str> = match &self.estado {
            EstadoPista::Viva { id_dispositivo } => Some(id_dispositivo),
            EstadoPista::Ausente { id_fallido, .. } => id_fallido.as_deref(),
            _ => None,
        };
        if en_uso == Some(id.as_str()) {
            self.candidato = None;
            return Veredicto::Nada;
        }

        let veces = match &mut self.candidato {
            Some((visto, n)) if *visto == id => {
                *n += 1;
                *n
            }
            otro => {
                *otro = Some((id, 1));
                1
            }
        };
        if veces < SONDEOS_PARA_CONFIRMAR {
            return Veredicto::Nada;
        }

        // El enfriamiento solo frena a una pista que está sirviendo: una
        // ausente no tiene nada que proteger de un vaivén.
        let enfriando = self.es_viva()
            && self
                .ultima_apertura
                .is_some_and(|t| ahora.saturating_sub(t) < ENFRIAMIENTO_DE_REAPERTURA);
        if enfriando {
            return Veredicto::Nada;
        }
        Veredicto::Reabrir
    }

    /// Pasa a «reabriendo». Es idempotente a propósito: un error de lectura y
    /// un cambio de dispositivo pueden coincidir en la misma vuelta, y tiene
    /// que haber un solo cierre y una sola reapertura. Devuelve `false` si ya
    /// no estaba viva (ni ausente) y no hizo nada.
    ///
    /// `muestras_viva` es el contador de la `PistaCapturada` **después** de
    /// vaciarla: leerlo antes dejaría fuera la cola del remuestreador, y esas
    /// muestras se rellenarían otra vez como silencio.
    pub fn empezar_reapertura(&mut self, ahora: Duration, muestras_viva: u64) -> bool {
        // Una pista que se acaba de abrir y ya vuelve a fallar no debe
        // reintentar de inmediato: abriría y cerraría el motor de audio sin
        // pausa, con dos registros por vuelta. Se trata como un intento
        // fallido y entra en el calendario de 2 s / 5 s. Un fallo de una
        // pista que llevaba rato sirviendo sí reintenta enseguida.
        let recien_abierta = self.es_viva()
            && self
                .ultima_apertura
                .is_some_and(|t| ahora.saturating_sub(t) < ENFRIAMIENTO_DE_REAPERTURA);
        let muestras_emitidas = match self.estado {
            EstadoPista::Viva { .. } => muestras_viva,
            EstadoPista::Ausente {
                muestras_emitidas, ..
            } => muestras_emitidas,
            EstadoPista::Reabriendo { .. } | EstadoPista::NoPedida => return false,
        };
        self.estado = EstadoPista::Reabriendo {
            desde: ahora,
            muestras_emitidas,
            ultimo_intento: recien_abierta.then_some(ahora),
            intentos: 0,
        };
        self.candidato = None;
        self.ultimo_aviso = ahora;
        true
    }

    /// Si ya toca intentar abrir: de inmediato tras `empezar_reapertura`, y
    /// después cada 2 s durante el primer minuto y cada 5 s a partir de ahí.
    pub fn toca_reintentar(&self, ahora: Duration) -> bool {
        let EstadoPista::Reabriendo {
            desde,
            ultimo_intento,
            ..
        } = &self.estado
        else {
            return false;
        };
        match ultimo_intento {
            None => true,
            Some(t) => {
                ahora.saturating_sub(*t) >= intervalo_de_reintento(ahora.saturating_sub(*desde))
            }
        }
    }

    pub fn intento_fallido(&mut self, ahora: Duration) {
        if let EstadoPista::Reabriendo {
            ultimo_intento,
            intentos,
            ..
        } = &mut self.estado
        {
            *ultimo_intento = Some(ahora);
            *intentos += 1;
        }
    }

    /// El dispositivo por defecto no sirve de forma permanente.
    pub fn fallo_permanente(&mut self, id_fallido: String) {
        if let EstadoPista::Reabriendo {
            muestras_emitidas, ..
        } = self.estado
        {
            self.estado = EstadoPista::Ausente {
                id_fallido: Some(id_fallido),
                muestras_emitidas,
            };
            self.candidato = None;
        }
    }

    /// La reapertura salió bien.
    pub fn al_abrir(&mut self, id_dispositivo: String, ahora: Duration) {
        if matches!(self.estado, EstadoPista::Reabriendo { .. }) {
            self.estado = EstadoPista::Viva { id_dispositivo };
            self.candidato = None;
            self.ultima_apertura = Some(ahora);
            self.ultimo_sondeo = ahora;
        }
    }

    /// Si toca un aviso más mientras la pista sigue sin dispositivo: como
    /// mucho uno cada 30 s, para que un dispositivo ausente toda la tarde no
    /// llene el registro.
    pub fn toca_avisar(&mut self, ahora: Duration) -> bool {
        if !matches!(self.estado, EstadoPista::Reabriendo { .. }) {
            return false;
        }
        if ahora.saturating_sub(self.ultimo_aviso) >= AVISO_MAXIMO_CADA {
            self.ultimo_aviso = ahora;
            return true;
        }
        false
    }

    /// El silencio que falta para que esta pista dure lo que la sesión, si no
    /// tiene cliente al detenerse, en tramas de como mucho 1 s.
    ///
    /// Una pista viva no entra aquí: `cerrar_pista` solo emite la cola de su
    /// remuestreador, así que un loopback que termina en silencio sale más
    /// corto que la sesión (deuda anterior a AV-1, ver HANDOFF).
    pub fn relleno_final(&self, track: Track, fin_ms: i64) -> Vec<AudioFrame> {
        if !matches!(
            self.estado,
            EstadoPista::Reabriendo { .. } | EstadoPista::Ausente { .. }
        ) {
            return Vec::new();
        }
        let ya = self.contador();
        silencio_en_tramas(track, ya, muestras_de_relleno(fin_ms, ya, 0))
    }
}

/// Duración máxima de una trama de silencio de relleno. Una pista pedida que
/// nunca abrió en una clase de 2 h necesita 115 millones de muestras (~460 MB)
/// de silencio: en una sola `AudioFrame`, justo al detener, sería el pico de
/// memoria de toda la sesión. En tramas de 1 s `EscritorPistas` las escribe
/// en streaming.
pub const TRAMA_DE_RELLENO_MAX: u64 = SAMPLE_RATE as u64;

/// `faltan` muestras de silencio como tramas de [`TRAMA_DE_RELLENO_MAX`] como
/// mucho. `ya_emitidas` es el contador de la pista antes del relleno: de él
/// salen los `timestamp_ms` (el instante en que termina cada trama).
pub fn silencio_en_tramas(track: Track, ya_emitidas: u64, faltan: u64) -> Vec<AudioFrame> {
    let mut tramas = Vec::new();
    let mut hechas = 0u64;
    while hechas < faltan {
        let n = (faltan - hechas).min(TRAMA_DE_RELLENO_MAX);
        hechas += n;
        tramas.push(AudioFrame {
            track,
            pcm: vec![0.0; n as usize],
            timestamp_ms: ((ya_emitidas + hechas) * 1000 / SAMPLE_RATE as u64) as i64,
        });
    }
    tramas
}

/// Convierte la posición QPC de `IAudioCaptureClient::GetBuffer`, que viene en
/// unidades de 100 ns, a milisegundos.
pub fn qpc_a_ms(qpc_100ns: u64) -> i64 {
    (qpc_100ns / 10_000) as i64
}

/// Fecha de un bloque cuando WASAPI no da posición QPC utilizable: el bloque
/// terminó de capturarse hace tanto como tardan en sonar las tramas que aún
/// esperan detrás de él.
pub fn fecha_por_pendientes(leido_ms: i64, tramas_pendientes: u32, frecuencia: u32) -> i64 {
    let pendientes_ms = tramas_pendientes as i64 * 1000 / frecuencia.max(1) as i64;
    (leido_ms - pendientes_ms).max(0)
}

/// Pone a cada bloque la fecha en que se **capturó**, en el reloj de la
/// sesión, y no la del momento en que el hilo llegó a leerlo.
///
/// Importa desde que el búfer es de 3 s: si el hilo se bloquea (abrir un
/// cliente tarda hasta 2,5 s) el audio de ese tiempo se queda esperando, y
/// fecharlo al drenar lo pondría todo en el instante de la lectura:
/// `muestras_de_relleno` rellenaría el bloqueo entero con silencio y luego
/// anexaría el audio retenido, dejando la pista adelantada para siempre.
///
/// La posición QPC de WASAPI está en el reloj de rendimiento del sistema,
/// que no es el de `Instant`. Sin una API de QPC a mano se calibra el desfase
/// entre los dos como el **mínimo** observado de «lectura − captura»: una
/// lectura nunca es anterior a la captura, así que el mínimo es la lectura
/// más puntual y el que más se acerca al desfase real. Se comparte entre las
/// dos pistas y las reaperturas, que usan el mismo reloj.
#[derive(Debug, Default)]
pub struct RelojCaptura {
    desfase_ms: Option<i64>,
}

impl RelojCaptura {
    /// Fecha (fin del bloque, en ms de la sesión) de un bloque de `tramas`
    /// cuya primera trama se capturó en `qpc_100ns` y que se leyó en
    /// `leido_ms`. Nunca devuelve algo posterior a la lectura.
    pub fn fechar(&mut self, qpc_100ns: u64, tramas: u32, frecuencia: u32, leido_ms: i64) -> i64 {
        let duracion_ms = tramas as i64 * 1000 / frecuencia.max(1) as i64;
        let fin_ms = qpc_a_ms(qpc_100ns) + duracion_ms;
        let desfase = leido_ms - fin_ms;
        let minimo = self.desfase_ms.map_or(desfase, |m| m.min(desfase));
        self.desfase_ms = Some(minimo);
        (fin_ms + minimo).clamp(0, leido_ms.max(0))
    }
}

/// Cada cuánto se reintenta abrir una pista que lleva `en_reabriendo` sin
/// dispositivo.
pub fn intervalo_de_reintento(en_reabriendo: Duration) -> Duration {
    if en_reabriendo < PRIMER_MINUTO {
        REINTENTO_RAPIDO
    } else {
        REINTENTO_LENTO
    }
}

/// Bytes que ocupa una muestra en el formato nativo del dispositivo.
///
/// Hace falta para convertir las *tramas* que cuenta AAudio —una por canal y
/// punto temporal— a los bytes que hay que leer del búfer. Equivale al
/// `nBlockAlign` que WASAPI entrega ya calculado en su `WAVEFORMATEX`;
/// AAudio no da ese dato, así que se deriva del formato y del número de
/// canales.
pub fn bytes_por_muestra(formato: FormatoNativo) -> usize {
    match formato {
        FormatoNativo::I16 => 2,
        FormatoNativo::I32 | FormatoNativo::F32 => 4,
    }
}

/// Traduce el código de formato que devuelve `AAudioStream_getFormat`.
///
/// Se consulta el formato **real del flujo ya abierto**, nunca el que se
/// pidió: `AAudioStreamBuilder_setFormat` es una preferencia, y AAudio abre
/// con lo que el dispositivo tenga sin avisar de que cambió de opinión. Dar
/// por bueno lo pedido interpretaría `i16` como `f32` y grabaría ruido
/// blanco a todo volumen durante la clase entera.
///
/// `AAUDIO_FORMAT_PCM_I24_PACKED` (3) existe y no se soporta: son tres bytes
/// por muestra y [`normalizar_a_f32`] trabaja con anchos de 2 y 4. Se
/// rechaza en voz alta en vez de leerlo desalineado.
pub fn formato_desde_aaudio(codigo: i32) -> Result<FormatoNativo> {
    match codigo {
        1 => Ok(FormatoNativo::I16),
        2 => Ok(FormatoNativo::F32),
        4 => Ok(FormatoNativo::I32),
        otro => Err(AudioError::Inicio(format!(
            "AAudio abrió el flujo con un formato que no se sabe leer (código {otro})"
        ))),
    }
}

/// Qué pistas se graban en Android, a partir de lo que pide la configuración.
///
/// **En Android no hay pista de sistema y no la va a haber.** No es una
/// carencia de esta implementación: Android no deja capturar el audio de una
/// videollamada ajena, a propósito y a nivel de sistema operativo. El caso de
/// uso aquí es la reunión presencial.
///
/// Por eso `capturar_sistema` se ignora en vez de fallar. Quien llama suele
/// reutilizar la misma [`crate::CaptureConfig`] que en el escritorio, donde
/// viene en `true` por defecto: devolver un error ahí dejaría al usuario sin
/// grabar la reunión por pedir de más, que es el peor desenlace posible de
/// los tres. Se registra en el log y se sigue con el micrófono.
pub fn pistas_en_android(capturar_microfono: bool, capturar_sistema: bool) -> Result<Vec<Track>> {
    if capturar_sistema {
        tracing::warn!(
            "se pidió capturar el audio del sistema, pero Android no lo permite:              se graba solo el micrófono"
        );
    }
    pistas_a_grabar(capturar_microfono, false)
}

/// Pipeline completo de una pista: de bytes nativos del dispositivo a un
/// [`AudioFrame`] listo para escribir.
///
/// Agrupa aquí, y no en `wasapi_src`, todo lo que no toca COM: así el bucle
/// de captura queda reducido a leer bytes y llamar a
/// [`PistaCapturada::procesar_paquete`], que es lo único de esta pieza que un
/// test puede ejercitar sin tarjeta de sonido.
pub struct PistaCapturada {
    track: Track,
    canales: usize,
    formato: FormatoNativo,
    remuestreador: Remuestreador,
    muestras_emitidas: u64,
}

impl PistaCapturada {
    pub fn nueva(
        track: Track,
        canales: usize,
        formato: FormatoNativo,
        frecuencia: u32,
    ) -> Result<Self> {
        Self::continuar(track, canales, formato, frecuencia, 0)
    }

    /// Una pista nueva que sigue la línea de tiempo de otra anterior.
    ///
    /// Al reabrir un dispositivo (AV-1) la pista no empieza en cero: el WAV
    /// lleva ya `muestras_emitidas` escritas, y el primer paquete del cliente
    /// nuevo tiene que medirse contra ese contador para que
    /// [`muestras_de_relleno`] convierta el hueco en silencio. Con `nueva`, la
    /// pista creía llevar cero muestras y rellenaba la sesión entera otra vez.
    pub fn continuar(
        track: Track,
        canales: usize,
        formato: FormatoNativo,
        frecuencia: u32,
        muestras_emitidas: u64,
    ) -> Result<Self> {
        Ok(Self {
            track,
            canales: canales.max(1),
            formato,
            remuestreador: Remuestreador::nuevo(frecuencia)?,
            muestras_emitidas,
        })
    }

    /// Muestras (a 16 kHz) ya entregadas por esta pista. `wasapi_src` lo lee
    /// después de [`PistaCapturada::vaciar`] para pasárselo a la pista que
    /// reabre.
    pub fn muestras_emitidas(&self) -> u64 {
        self.muestras_emitidas
    }

    /// Rellena de silencio, en tramas acotadas, hasta el instante `hasta_ms`
    /// y avanza el contador. Se usa al reabrir: el hueco entero de la pista
    /// sin dispositivo sale en tramas de 1 s y el primer paquete del cliente
    /// nuevo solo rellena lo que dure la propia apertura.
    pub fn rellenar_hasta(&mut self, hasta_ms: i64) -> Vec<AudioFrame> {
        let faltan = muestras_de_relleno(hasta_ms, self.muestras_emitidas, 0);
        let tramas = silencio_en_tramas(self.track, self.muestras_emitidas, faltan);
        self.muestras_emitidas += faltan;
        tramas
    }

    /// Procesa un paquete de bytes nativos recién leído de WASAPI.
    ///
    /// `timestamp_ms` es el reloj monótono en el instante en que se recibió
    /// el paquete (ver la decisión de diseño al principio del archivo).
    /// Devuelve `None` cuando, tras remuestrear, no hay nada nuevo que
    /// escribir todavía: `Remuestreador::procesar` solo emite bloques
    /// completos de 1024 muestras de entrada, así que un paquete pequeño
    /// puede no generar salida por sí solo.
    pub fn procesar_paquete(
        &mut self,
        timestamp_ms: i64,
        bytes: &[u8],
    ) -> Result<Option<AudioFrame>> {
        let nativo = normalizar_a_f32(bytes, self.formato);
        let mono = a_mono(&nativo, self.canales);
        let reales = self.remuestreador.procesar(&mono)?;

        let relleno =
            muestras_de_relleno(timestamp_ms, self.muestras_emitidas, reales.len() as u64);
        if relleno == 0 && reales.is_empty() {
            return Ok(None);
        }

        let mut pcm = vec![0.0_f32; relleno as usize];
        pcm.extend_from_slice(&reales);

        self.muestras_emitidas += pcm.len() as u64;

        Ok(Some(AudioFrame {
            track: self.track,
            pcm,
            timestamp_ms,
        }))
    }

    /// Se llama al detener la sesión: recupera lo que `Remuestreador` tenga
    /// sin completar un bloque.
    ///
    /// `pipewire_src` no hace este último `vaciar()` y pierde hasta ~21 ms
    /// por pista en cada cierre (ver la deuda anotada en el PLAN de esta
    /// HU). Aquí sí se llama, porque no hay ninguna razón para tirar el
    /// final de la clase.
    pub fn vaciar(&mut self, timestamp_ms: i64) -> Result<Option<AudioFrame>> {
        let reales = self.remuestreador.vaciar()?;
        if reales.is_empty() {
            return Ok(None);
        }

        self.muestras_emitidas += reales.len() as u64;

        Ok(Some(AudioFrame {
            track: self.track,
            pcm: reales,
            timestamp_ms,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- normalizar_a_f32 -----------------------------------------------

    #[test]
    fn las_muestras_i16_del_dispositivo_se_normalizan_a_f32() {
        // Dividir por i16::MAX (32767) en vez de por 32768 dejaría la
        // muestra mínima fuera de [-1, 1], y el clamp de más adelante en el
        // pipeline la recortaría, perdiendo el pico real.
        let bytes: Vec<u8> = [i16::MAX.to_le_bytes(), i16::MIN.to_le_bytes()].concat();

        let f32s = normalizar_a_f32(&bytes, FormatoNativo::I16);

        assert!((f32s[0] - (32_767.0 / 32_768.0)).abs() < 1e-6);
        assert_eq!(f32s[1], -1.0);
    }

    #[test]
    fn el_f32_del_dispositivo_pasa_sin_tocarse() {
        let bytes: Vec<u8> = [0.25_f32.to_le_bytes(), (-0.5_f32).to_le_bytes()].concat();
        let f32s = normalizar_a_f32(&bytes, FormatoNativo::F32);
        assert_eq!(f32s, vec![0.25, -0.5]);
    }

    #[test]
    fn el_i32_del_dispositivo_se_normaliza_igual_que_el_i16() {
        let bytes: Vec<u8> = [i32::MAX.to_le_bytes(), i32::MIN.to_le_bytes()].concat();
        let f32s = normalizar_a_f32(&bytes, FormatoNativo::I32);
        assert!((f32s[0] - 1.0).abs() < 1e-6);
        assert_eq!(f32s[1], -1.0);
    }

    // -- muestras_de_relleno ----------------------------------------------

    #[test]
    fn un_silencio_en_loopback_no_adelanta_lo_que_viene_despues() {
        // Si el relleno se calculara siempre en 0, un paquete que llega
        // después de un hueco de silencio no generaría ningún hueco en el
        // WAV: el archivo saldría más corto que la clase real y todo lo
        // que sigue sonaría adelantado.
        //
        // El audio "real" usa 0.5, no 0.0: con ceros era indistinguible del
        // silencio de relleno, y una mutación que invirtiera el orden
        // (audio real antes, silencio después) daba el mismo `pcm` byte a
        // byte sin que ningún assert lo notara.
        let mut pista = PistaCapturada::nueva(Track::System, 1, FormatoNativo::F32, SAMPLE_RATE)
            .expect("remuestreador a la misma frecuencia no falla");

        // Primer paquete: 100 ms de audio real (1600 muestras a 16 kHz),
        // que llega prácticamente al arrancar la sesión (timestamp ~0): no
        // hay hueco que rellenar todavía, `muestras_ya_emitidas` parte de 0
        // y el propio paquete cubre ese primer tramo.
        let cien_ms_de_audio: Vec<u8> = (0..1600).flat_map(|_| 0.5_f32.to_le_bytes()).collect();
        let primero = pista
            .procesar_paquete(0, &cien_ms_de_audio)
            .unwrap()
            .expect("hay muestras reales, debe emitir algo");
        assert_eq!(primero.pcm.len(), 1600);

        // El loopback calla durante 1,9 s: el siguiente paquete llega a los
        // 2000 ms con otros 100 ms de audio real.
        let segundo = pista
            .procesar_paquete(2000, &cien_ms_de_audio)
            .unwrap()
            .expect("hay relleno y audio real, debe emitir algo");

        // Esperado a los 2000 ms: 32 000 muestras. Ya se emitieron 1600
        // antes de este paquete, y el propio paquete aporta otras 1600 de
        // audio real que también hay que descontar de lo esperado -- si no,
        // el paquete que reanuda el audio se cuenta dos veces (relleno y
        // señal a la vez) y el hueco real, de 28 800 muestras (1800 ms, no
        // 1900), queda 1600 muestras más largo de lo que debería.
        assert_eq!(segundo.pcm.len(), 28_800 + 1_600);
        assert!(
            segundo.pcm[..28_800].iter().all(|&m| m == 0.0),
            "las primeras muestras tienen que ser el silencio de relleno"
        );
        assert!(
            segundo.pcm[28_800..].iter().all(|&m| m == 0.5),
            "el audio real va después del silencio, no antes"
        );
    }

    #[test]
    fn el_relleno_resta_lo_ya_emitido_del_timestamp_esperado() {
        // Ojo con lo que esta prueba protege y lo que no: `muestras_de_relleno`
        // es una función pura que recibe `timestamp_ms` ya calculado, así que
        // no puede distinguir si ese número vino del reloj monótono (la
        // decisión correcta, ver la cabecera del archivo) o de derivarlo del
        // propio conteo de muestras (el error de copiar `pipewire_src`). Esa
        // decisión se toma en `wasapi_src.rs`, que no tiene pruebas propias
        // porque no se puede ejercitar sin hardware ni compilador. Lo que
        // esta prueba sí verifica: que, dado el timestamp que produciría esa
        // derivación errónea (numéricamente igual a lo ya emitido), el
        // relleno da cero, y que con un timestamp distinto, no.
        let muestras_ya_emitidas = 1_600u64; // 100 ms ya escritos
        let timestamp_derivado_mal = (muestras_ya_emitidas as i64 * 1000) / SAMPLE_RATE as i64;
        assert_eq!(
            muestras_de_relleno(timestamp_derivado_mal, muestras_ya_emitidas, 0),
            0
        );

        // El reloj de pared, en cambio, sabe que en realidad pasaron 2 s:
        // hay que rellenar la diferencia con silencio.
        let timestamp_real = 2_000;
        assert_eq!(
            muestras_de_relleno(timestamp_real, muestras_ya_emitidas, 0),
            32_000 - 1_600
        );
    }

    #[test]
    fn el_relleno_tambien_descuenta_las_muestras_del_propio_paquete() {
        // Antes de este arreglo, `muestras_de_relleno` solo restaba
        // `muestras_ya_emitidas` de lo esperado: el paquete que reanuda tras
        // un silencio se contaba dos veces (como relleno y como audio real),
        // 100 ms de más en cada transición silencio→audio del loopback. Ver
        // `un_silencio_en_loopback_no_adelanta_lo_que_viene_despues` para el
        // caso completo a través de `PistaCapturada`; esta prueba aísla la
        // función pura con números simples.
        assert_eq!(
            muestras_de_relleno(2_000, 1_600, 1_600),
            32_000 - 1_600 - 1_600
        );
    }

    #[test]
    fn el_relleno_nunca_es_negativo_si_ya_se_emitio_de_mas() {
        // Sin el `saturating_sub`, un paquete que llega con menos muestras
        // esperadas (según el timestamp) que las ya emitidas -- el
        // micrófono, que no deja huecos, adelantándose por redondeo a un
        // paquete de sistema que sí los tiene -- desbordaría el `u64` en vez
        // de devolver 0.
        assert_eq!(muestras_de_relleno(100, 10_000, 0), 0);
    }

    #[test]
    fn dos_horas_de_muestras_no_desbordan_el_contador() {
        // Una clase de dos horas por sí sola cabe en un u32 (115 200 000
        // muestras), pero el contador se declaró u64 para no tener que
        // revisar esto de nuevo si algún día una sesión dura más. Se
        // prueba bastante por encima de `u32::MAX` para que un cambio a
        // u32 se note.
        //
        // La base se elige múltiplo de `SAMPLE_RATE / 1000` (16) a
        // propósito, y no `u32::MAX` a secas: convertir muestras a
        // milisegundos (como hace este mismo test, para simular el
        // timestamp que reportaría el reloj en ese instante) trunca el
        // resto de la división cuando el valor no es múltiplo de 16, y
        // `u32::MAX` no lo es (resto 15). Con `u32::MAX` como base, este
        // test pedía 32 000 y el código -- correcto -- daba 31 985: un
        // falso rojo por un redondeo del propio fixture, no un error real.
        // `u32::MAX + 1` (2^32) sí es múltiplo de 16, así que el ida y
        // vuelta ms → muestras → ms no pierde nada.
        let muestras_previas: u64 = u32::MAX as u64 + 1 + 1_000_000;
        let timestamp_correspondiente =
            ((muestras_previas + 32_000) as i64 * 1000) / SAMPLE_RATE as i64;

        let relleno = muestras_de_relleno(timestamp_correspondiente, muestras_previas, 0);
        assert_eq!(relleno, 32_000);
    }

    // -- pistas_a_grabar ----------------------------------------------------

    #[test]
    fn sin_dispositivo_de_salida_se_graba_solo_el_microfono() {
        let pistas = pistas_a_grabar(true, false).unwrap();
        assert_eq!(pistas, vec![Track::Mic]);
    }

    #[test]
    fn sin_ninguna_pista_disponible_es_un_error_explicito() {
        // Antes de esta comprobación, arrancar sin mic ni loopback abría
        // una sesión vacía: la interfaz mostraba "grabando" y no se
        // guardaba nada, sin ningún aviso.
        let err = pistas_a_grabar(false, false).unwrap_err();
        assert!(matches!(err, AudioError::SinDispositivo(_)));
    }

    // -- La sesión no termina sola por el estado de los dispositivos --------

    fn seg(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    /// Una pista en cada uno de los cuatro estados, construida por los
    /// caminos de producción.
    fn pista_en(estado: &str) -> ControlPista {
        let mut c = ControlPista::viva("a".into(), Duration::ZERO);
        match estado {
            "viva" => {}
            "reabriendo" => {
                c.empezar_reapertura(seg(1), 100);
            }
            "ausente" => {
                c.empezar_reapertura(seg(1), 100);
                c.fallo_permanente("a".into());
            }
            "no_pedida" => c = ControlPista::no_pedida(),
            otro => panic!("estado desconocido {otro}"),
        }
        c
    }

    #[test]
    fn ninguna_combinacion_de_estados_termina_la_sesion_y_la_ausente_sigue_vigilando() {
        // Antes existía `alguna_pista_sigue_viva` y después `debe_terminar`;
        // las dos cortaban la grabación por el estado de los dispositivos, y
        // una clase solo ocurre una vez. Ahora la sesión termina únicamente
        // cuando la persona la detiene, así que no hay función que decida
        // terminar. Lo que sí se puede fijar es lo que lo hace posible: en
        // las 16 combinaciones de dos pistas, cada una responde sola al
        // sondeo según su estado (viva y ausente ven un id distinto y piden
        // reabrir; reabriendo y no pedida no tienen nada que sondear), y
        // dos pistas ausentes a la vez siguen pudiendo volver.
        let estados = ["viva", "reabriendo", "ausente", "no_pedida"];
        let esperado = |e: &str| match e {
            "viva" | "ausente" => Veredicto::Reabrir,
            _ => Veredicto::Nada,
        };
        let mut combinaciones = 0;
        for a in estados {
            for b in estados {
                combinaciones += 1;
                let mut pa = pista_en(a);
                let mut pb = pista_en(b);
                // Dos sondeos seguidos, pasado el enfriamiento, con otro id.
                for t in [10, 11] {
                    let ra = pa.sondear(seg(t), || Some("otro".into()));
                    let rb = pb.sondear(seg(t), || Some("otro".into()));
                    if t == 11 {
                        assert_eq!(ra, esperado(a), "pista A en {a} (con B en {b})");
                        assert_eq!(rb, esperado(b), "pista B en {b} (con A en {a})");
                    }
                }
            }
        }
        assert_eq!(combinaciones, 16);
    }

    // -- ControlPista: transiciones -------------------------------------------

    #[test]
    fn un_error_de_lectura_lleva_a_reabriendo_con_el_contador_leido_tras_vaciar() {
        // 48 kHz: el remuestreador retiene muestras que solo salen al
        // vaciarlo. Leer el contador antes de `vaciar` lo dejaba corto, y esas
        // muestras se rellenaban de nuevo como silencio al reabrir.
        let mut pista = PistaCapturada::nueva(Track::Mic, 1, FormatoNativo::F32, 48_000).unwrap();
        let medio_segundo: Vec<u8> = (0..24_000).flat_map(|_| 0.5_f32.to_le_bytes()).collect();
        pista.procesar_paquete(500, &medio_segundo).unwrap();
        let antes_de_vaciar = pista.muestras_emitidas();
        pista.vaciar(500).unwrap();
        let despues_de_vaciar = pista.muestras_emitidas();
        assert!(
            despues_de_vaciar > antes_de_vaciar,
            "la cola no estaba vacía"
        );

        let mut control = ControlPista::viva("micro".into(), Duration::ZERO);
        assert!(control.empezar_reapertura(seg(10), pista.muestras_emitidas()));

        assert_eq!(control.contador(), despues_de_vaciar);
        assert!(!control.es_viva());
    }

    #[test]
    fn un_error_y_un_cambio_de_dispositivo_a_la_vez_reabren_una_sola_vez() {
        // El cierre y la reapertura son por evento, no por causa: si los dos
        // disparadores coinciden, el segundo no puede volver a cerrar ni
        // reiniciar el contador.
        let mut control = ControlPista::viva("a".into(), Duration::ZERO);
        assert!(control.empezar_reapertura(seg(10), 1_000));
        assert!(!control.empezar_reapertura(seg(10), 99_999));
        assert_eq!(control.contador(), 1_000);
    }

    #[test]
    fn reabrir_deja_la_pista_viva_sobre_el_dispositivo_nuevo() {
        let mut control = ControlPista::viva("a".into(), Duration::ZERO);
        control.empezar_reapertura(seg(10), 1_000);
        control.al_abrir("b".into(), seg(11));
        assert_eq!(
            control.estado(),
            &EstadoPista::Viva {
                id_dispositivo: "b".into()
            }
        );
    }

    #[test]
    fn los_reintentos_fallidos_nunca_reinician_el_contador() {
        let mut control = ControlPista::viva("a".into(), Duration::ZERO);
        control.empezar_reapertura(seg(10), 7_000);
        for t in [12, 14, 16, 18] {
            control.intento_fallido(seg(t));
        }
        assert_eq!(control.contador(), 7_000);
        match control.estado() {
            EstadoPista::Reabriendo { intentos, .. } => assert_eq!(*intentos, 4),
            otro => panic!("esperaba Reabriendo, hay {otro:?}"),
        }
    }

    #[test]
    fn un_fallo_permanente_deja_la_pista_ausente_con_el_contador_intacto() {
        let mut control = ControlPista::viva("a".into(), Duration::ZERO);
        control.empezar_reapertura(seg(10), 7_000);
        control.fallo_permanente("a".into());
        assert_eq!(
            control.estado(),
            &EstadoPista::Ausente {
                id_fallido: Some("a".into()),
                muestras_emitidas: 7_000,
            }
        );
        // Una ausente no reintenta por su cuenta: solo la saca de ahí otro
        // dispositivo por defecto.
        assert!(!control.toca_reintentar(seg(600)));
    }

    #[test]
    fn una_pista_pedida_que_no_abrio_al_arrancar_empieza_reabriendo_desde_cero() {
        // El relleno tiene que cubrir desde el inicio de la sesión, no desde
        // el momento en que aparezca el dispositivo.
        let control = ControlPista::sin_abrir_al_arrancar(Duration::ZERO);
        assert_eq!(control.contador(), 0);
        assert!(matches!(
            control.estado(),
            EstadoPista::Reabriendo {
                intentos: 1,
                ultimo_intento: Some(_),
                ..
            }
        ));
        assert!(!control.toca_reintentar(Duration::from_millis(1_999)));
        assert!(control.toca_reintentar(seg(2)));
    }

    #[test]
    fn una_pista_que_nunca_se_pidio_no_se_toca() {
        let mut control = ControlPista::no_pedida();
        assert!(!control.empezar_reapertura(seg(10), 5));
        assert!(!control.toca_reintentar(seg(100)));
        assert!(!control.toca_avisar(seg(100)));
        assert_eq!(
            control.sondear(seg(100), || panic!("no debe sondear")),
            Veredicto::Nada
        );
        assert_eq!(control.estado(), &EstadoPista::NoPedida);
    }

    // -- ControlPista: calendario de reintentos -------------------------------

    #[test]
    fn el_primer_intento_tras_perder_el_dispositivo_es_inmediato() {
        let mut control = ControlPista::viva("a".into(), Duration::ZERO);
        control.empezar_reapertura(seg(10), 0);
        assert!(control.toca_reintentar(seg(10)));
    }

    #[test]
    fn en_el_primer_minuto_se_reintenta_cada_dos_segundos() {
        let mut control = ControlPista::viva("a".into(), Duration::ZERO);
        control.empezar_reapertura(seg(100), 0);
        control.intento_fallido(seg(100));

        assert!(!control.toca_reintentar(Duration::from_millis(101_999)));
        assert!(control.toca_reintentar(seg(102)), "la frontera cuenta: >=");
    }

    #[test]
    fn pasado_el_primer_minuto_se_reintenta_cada_cinco_segundos() {
        let mut control = ControlPista::viva("a".into(), Duration::ZERO);
        control.empezar_reapertura(seg(100), 0);
        // Último intento justo al cumplirse el minuto: ya cuenta el ritmo lento.
        control.intento_fallido(seg(160));

        assert!(!control.toca_reintentar(Duration::from_millis(164_999)));
        assert!(control.toca_reintentar(seg(165)));
    }

    #[test]
    fn el_ritmo_cambia_exactamente_a_los_sesenta_segundos() {
        // La frontera del umbral, con `>=` como en el resto: 59,999 s sigue
        // siendo ritmo rápido, 60 s ya es lento.
        assert_eq!(
            intervalo_de_reintento(Duration::from_millis(59_999)),
            seg(2)
        );
        assert_eq!(intervalo_de_reintento(seg(60)), seg(5));
    }

    #[test]
    fn el_aviso_de_una_pista_reabriendo_sale_como_mucho_cada_treinta_segundos() {
        let mut control = ControlPista::viva("a".into(), Duration::ZERO);
        control.empezar_reapertura(seg(100), 0);

        assert!(!control.toca_avisar(Duration::from_millis(129_999)));
        assert!(control.toca_avisar(seg(130)));
        assert!(
            !control.toca_avisar(seg(131)),
            "el aviso reinicia la cuenta"
        );
        assert!(control.toca_avisar(seg(160)));
    }

    // -- ControlPista: sondeo del dispositivo por defecto (D1) ----------------

    fn sondeo(control: &mut ControlPista, t: Duration, id: &str) -> Veredicto {
        control.sondear(t, || Some(id.to_string()))
    }

    #[test]
    fn un_id_nuevo_que_aparece_en_un_solo_sondeo_no_reabre() {
        // Es el vaivén de los auriculares Bluetooth entre sus dos perfiles:
        // con un solo sondeo bastando, se reabría en cada alternancia.
        let mut control = ControlPista::viva("estereo".into(), Duration::ZERO);
        assert_eq!(
            sondeo(&mut control, seg(10), "manos-libres"),
            Veredicto::Nada
        );
        assert_eq!(sondeo(&mut control, seg(11), "estereo"), Veredicto::Nada);
        assert_eq!(
            sondeo(&mut control, seg(12), "manos-libres"),
            Veredicto::Nada
        );
    }

    #[test]
    fn el_mismo_id_nuevo_en_dos_sondeos_seguidos_si_reabre() {
        let mut control = ControlPista::viva("a".into(), Duration::ZERO);
        assert_eq!(sondeo(&mut control, seg(10), "b"), Veredicto::Nada);
        assert_eq!(sondeo(&mut control, seg(11), "b"), Veredicto::Reabrir);
    }

    #[test]
    fn dos_ids_nuevos_distintos_seguidos_no_cuentan_como_el_mismo() {
        let mut control = ControlPista::viva("a".into(), Duration::ZERO);
        assert_eq!(sondeo(&mut control, seg(10), "b"), Veredicto::Nada);
        assert_eq!(sondeo(&mut control, seg(11), "c"), Veredicto::Nada);
        assert_eq!(sondeo(&mut control, seg(12), "c"), Veredicto::Reabrir);
    }

    #[test]
    fn un_sondeo_que_falla_no_cambia_nada_y_corta_la_racha() {
        // Una reenumeración en curso devuelve error un instante: no es motivo
        // para tocar una pista sana, ni para dar por «seguido» lo que no lo es.
        let mut control = ControlPista::viva("a".into(), Duration::ZERO);
        assert_eq!(sondeo(&mut control, seg(10), "b"), Veredicto::Nada);
        assert_eq!(control.sondear(seg(11), || None), Veredicto::Nada);
        assert_eq!(sondeo(&mut control, seg(12), "b"), Veredicto::Nada);
        assert!(control.es_viva());
    }

    #[test]
    fn no_se_sondea_mas_de_una_vez_por_segundo() {
        let mut control = ControlPista::viva("a".into(), Duration::ZERO);
        let llamadas = std::cell::Cell::new(0);
        for ms in [1_000, 1_300, 1_999] {
            control.sondear(Duration::from_millis(ms), || {
                llamadas.set(llamadas.get() + 1);
                Some("a".into())
            });
        }
        // El de 1 000 ms cae justo en la frontera y sí cuenta; los otros dos
        // están a menos de un segundo del anterior.
        assert_eq!(llamadas.get(), 1);
        control.sondear(Duration::from_millis(2_000), || {
            llamadas.set(llamadas.get() + 1);
            Some("a".into())
        });
        assert_eq!(llamadas.get(), 2);
    }

    #[test]
    fn dos_reaperturas_de_la_misma_pista_no_pueden_ir_a_menos_de_cinco_segundos() {
        let mut control = ControlPista::viva("a".into(), Duration::ZERO);
        // La pista se acaba de reabrir en el segundo 20.
        control.empezar_reapertura(seg(19), 0);
        control.al_abrir("b".into(), seg(20));

        // El dispositivo vuelve a cambiar y se confirma enseguida, pero aún
        // no han pasado 5 s desde la apertura.
        assert_eq!(sondeo(&mut control, seg(21), "a"), Veredicto::Nada);
        assert_eq!(sondeo(&mut control, seg(22), "a"), Veredicto::Nada);
        assert_eq!(sondeo(&mut control, seg(24), "a"), Veredicto::Nada);
        // A los 5 s justos se puede, y la racha acumulada sigue valiendo.
        assert_eq!(sondeo(&mut control, seg(25), "a"), Veredicto::Reabrir);
    }

    #[test]
    fn una_pista_ausente_vuelve_cuando_el_dispositivo_por_defecto_es_otro() {
        let mut control = ControlPista::viva("a".into(), Duration::ZERO);
        control.empezar_reapertura(seg(10), 4_000);
        control.fallo_permanente("a".into());

        // Con el mismo dispositivo que falló, nada.
        assert_eq!(sondeo(&mut control, seg(11), "a"), Veredicto::Nada);
        assert_eq!(sondeo(&mut control, seg(12), "a"), Veredicto::Nada);
        // Con otro, tras la confirmación de siempre.
        assert_eq!(sondeo(&mut control, seg(13), "b"), Veredicto::Nada);
        assert_eq!(sondeo(&mut control, seg(14), "b"), Veredicto::Reabrir);

        // Y reabre conservando el contador de la ausencia.
        assert!(control.empezar_reapertura(seg(14), 0));
        assert_eq!(control.contador(), 4_000);
        assert!(control.toca_reintentar(seg(14)));
    }

    #[test]
    fn una_pista_ausente_puede_volver_dentro_de_los_cinco_segundos_de_su_ultima_apertura() {
        // La espera de 5 s protege a una pista que está sirviendo de un
        // vaivén; una ausente no tiene nada que proteger y no debe esperar.
        // Sin esta distinción una ausente recién fallada tardaba 5 s de más
        // en volver con un dispositivo bueno.
        let mut control = ControlPista::viva("a".into(), Duration::ZERO);
        control.empezar_reapertura(seg(19), 0);
        control.al_abrir("b".into(), seg(20));
        control.empezar_reapertura(seg(20), 100);
        control.fallo_permanente("b".into());

        assert_eq!(sondeo(&mut control, seg(21), "c"), Veredicto::Nada);
        assert_eq!(sondeo(&mut control, seg(22), "c"), Veredicto::Reabrir);
    }

    #[test]
    fn una_apertura_que_falla_al_leer_enseguida_no_reintenta_de_inmediato() {
        // Abre bien y la primera lectura devuelve DEVICE_INVALIDATED: sin esto
        // cada vuelta reiniciaba el calendario y el bucle abría y cerraba el
        // motor de audio sin pausa, con dos registros por vuelta.
        let mut control = ControlPista::viva("a".into(), Duration::ZERO);
        control.empezar_reapertura(seg(10), 0);
        control.al_abrir("a".into(), seg(11));
        assert!(control.empezar_reapertura(Duration::from_millis(11_300), 0));

        assert!(!control.toca_reintentar(Duration::from_millis(11_300)));
        assert!(!control.toca_reintentar(Duration::from_millis(13_299)));
        assert!(control.toca_reintentar(Duration::from_millis(13_300)));
    }

    #[test]
    fn una_pista_que_servia_desde_hace_rato_reintenta_enseguida_al_fallar() {
        let mut control = ControlPista::viva("a".into(), Duration::ZERO);
        assert!(control.empezar_reapertura(seg(60), 0));
        assert!(control.toca_reintentar(seg(60)));
    }

    #[test]
    fn una_pista_que_ya_esta_reabriendo_no_sondea() {
        let mut control = ControlPista::viva("a".into(), Duration::ZERO);
        control.empezar_reapertura(seg(10), 0);
        assert_eq!(
            control.sondear(seg(20), || panic!("los reintentos ya prueban el defecto")),
            Veredicto::Nada
        );
    }

    // -- relleno final (D3) ------------------------------------------------------

    fn muestras_de(tramas: &[AudioFrame]) -> usize {
        tramas.iter().map(|t| t.pcm.len()).sum()
    }

    #[test]
    fn una_pista_sin_dispositivo_al_detener_se_rellena_hasta_el_final_de_la_sesion() {
        // Antes `cerrar_pista` solo emitía la cola del remuestreador: el WAV
        // de una pista que murió a los 10 s de una sesión de 60 s duraba 10 s
        // y todo lo que venía detrás de él en la mezcla quedaba desplazado.
        let mut control = ControlPista::viva("a".into(), Duration::ZERO);
        control.empezar_reapertura(seg(10), 160_000);

        let tramas = control.relleno_final(Track::System, 60_000);

        assert_eq!(muestras_de(&tramas), 50 * 16_000);
        assert!(tramas.iter().all(|t| t.track == Track::System));
        assert!(tramas.iter().all(|t| t.pcm.iter().all(|&m| m == 0.0)));
        assert_eq!(tramas.last().unwrap().timestamp_ms, 60_000);
    }

    #[test]
    fn el_relleno_final_sale_en_tramas_de_como_mucho_un_segundo() {
        // Una pista pedida que nunca abrió en una clase de 2 h necesita
        // ~460 MB de silencio en una sola trama, justo al detener.
        let control = ControlPista::sin_abrir_al_arrancar(Duration::ZERO);

        let tramas = control.relleno_final(Track::System, 600 * 1_000);

        assert_eq!(muestras_de(&tramas), 600 * 16_000);
        assert!(tramas
            .iter()
            .all(|t| t.pcm.len() as u64 <= TRAMA_DE_RELLENO_MAX));
        // Los timestamps avanzan: cada trama termina donde acaba su silencio.
        assert!(tramas
            .windows(2)
            .all(|w| w[0].timestamp_ms < w[1].timestamp_ms));
    }

    #[test]
    fn el_relleno_en_tramas_conserva_la_cola_corta() {
        // 2,5 s: dos tramas enteras y una de medio segundo, sin perder ni
        // inventar muestras.
        let tramas = silencio_en_tramas(Track::Mic, 0, 40_000);
        let largos: Vec<usize> = tramas.iter().map(|t| t.pcm.len()).collect();
        assert_eq!(largos, vec![16_000, 16_000, 8_000]);
        assert_eq!(tramas[2].timestamp_ms, 2_500);
        assert!(silencio_en_tramas(Track::Mic, 0, 0).is_empty());
    }

    #[test]
    fn una_pista_ausente_tambien_se_rellena_al_detener() {
        let mut control = ControlPista::viva("a".into(), Duration::ZERO);
        control.empezar_reapertura(seg(10), 160_000);
        control.fallo_permanente("a".into());
        let tramas = control.relleno_final(Track::Mic, 12_000);
        assert_eq!(muestras_de(&tramas), 2 * 16_000);
    }

    #[test]
    fn no_hay_relleno_final_para_una_pista_viva_ni_para_una_no_pedida() {
        let viva = ControlPista::viva("a".into(), Duration::ZERO);
        assert!(viva.relleno_final(Track::Mic, 60_000).is_empty());
        assert!(ControlPista::no_pedida()
            .relleno_final(Track::Mic, 60_000)
            .is_empty());
    }

    #[test]
    fn no_hay_relleno_final_si_la_pista_ya_cubre_la_sesion() {
        let mut control = ControlPista::viva("a".into(), Duration::ZERO);
        control.empezar_reapertura(seg(10), 60 * 16_000);
        assert!(control.relleno_final(Track::Mic, 60_000).is_empty());
    }

    #[test]
    fn una_pista_pedida_que_nunca_abrio_se_rellena_desde_el_cero() {
        let control = ControlPista::sin_abrir_al_arrancar(Duration::ZERO);
        let tramas = control.relleno_final(Track::System, 3_000);
        assert_eq!(muestras_de(&tramas), 3 * 16_000);
    }

    #[test]
    fn rellenar_hasta_al_reabrir_avanza_el_contador_y_acota_las_tramas() {
        // La pista lleva 1 s y reabre a los 600 s: el hueco (10 min) no puede
        // salir como una sola trama de cientos de MB dentro del primer paquete.
        let mut pista =
            PistaCapturada::continuar(Track::Mic, 1, FormatoNativo::F32, SAMPLE_RATE, 16_000)
                .unwrap();

        let tramas = pista.rellenar_hasta(600_000);

        assert!(tramas
            .iter()
            .all(|t| t.pcm.len() as u64 <= TRAMA_DE_RELLENO_MAX));
        assert_eq!(muestras_de(&tramas), 599 * 16_000);
        assert_eq!(pista.muestras_emitidas(), 600 * 16_000);
        // Ya al día: un segundo `rellenar_hasta` no duplica nada.
        assert!(pista.rellenar_hasta(600_000).is_empty());
    }

    // -- B-1: fechar por el instante de captura -------------------------------

    #[test]
    fn la_posicion_qpc_de_wasapi_viene_en_cien_nanosegundos() {
        assert_eq!(qpc_a_ms(0), 0);
        assert_eq!(qpc_a_ms(9_999), 0);
        assert_eq!(qpc_a_ms(10_000), 1);
        assert_eq!(qpc_a_ms(25_000_000), 2_500);
    }

    #[test]
    fn sin_qpc_la_fecha_resta_lo_que_aun_esta_pendiente_en_el_bufer() {
        // 48 000 tramas pendientes a 48 kHz = 1 s: el bloque que se acaba de
        // leer terminó de capturarse un segundo antes de la lectura.
        assert_eq!(fecha_por_pendientes(5_000, 48_000, 48_000), 4_000);
        assert_eq!(fecha_por_pendientes(5_000, 0, 48_000), 5_000);
        // Nunca sale negativa.
        assert_eq!(fecha_por_pendientes(500, 48_000, 48_000), 0);
    }

    #[test]
    fn el_primer_bloque_se_fecha_en_el_instante_de_su_lectura() {
        let mut reloj = RelojCaptura::default();
        // Sin historial no se sabe el desfase entre relojes: se confía en la
        // lectura, que es lo que hacía el código anterior.
        assert_eq!(reloj.fechar(123_456_000_000, 480, 48_000, 2_000), 2_000);
    }

    #[test]
    fn un_bloque_leido_tarde_se_fecha_en_su_captura_y_no_en_su_lectura() {
        let mut reloj = RelojCaptura::default();
        let base_qpc: u64 = 777_000_000_000; // 100 ns; el origen no importa
        let t = |ms: u64| base_qpc + ms * 10_000;
        // Lectura puntual: el bloque de 10 ms que termina en t=1 000 ms se
        // lee en t=1 002 ms. Calibra el desfase (2 ms) y, sin historial, se
        // fecha en la lectura.
        assert_eq!(reloj.fechar(t(990), 480, 48_000, 1_002), 1_002);
        // Un bloqueo del hilo de 2 500 ms: el bloque que terminó de
        // capturarse en t=1 500 ms se lee en t=3 502 ms. Con la fecha de la
        // lectura iba 2 s adelantado; ahora sale 1 502 (captura + los 2 ms
        // de desfase mínimo observado).
        assert_eq!(reloj.fechar(t(1_490), 480, 48_000, 3_502), 1_502);
    }

    #[test]
    fn una_rafaga_tras_un_bloqueo_no_adelanta_la_pista() {
        // El caso de B-1 con la lógica real de la pista: paquetes de 10 ms a
        // 48 kHz; el hilo se bloquea 2,5 s y luego drena de golpe lo retenido
        // en el búfer de 3 s. Antes se fechaban todos con el instante del
        // drenaje y la pista quedaba ≈2,48 s adelantada para siempre.
        let mut pista = PistaCapturada::nueva(Track::Mic, 1, FormatoNativo::F32, 48_000).unwrap();
        let mut reloj = RelojCaptura::default();
        let diez_ms: Vec<u8> = (0..480).flat_map(|_| 0.5_f32.to_le_bytes()).collect();
        let qpc = |ms_fin: u64| (ms_fin - 10) * 10_000 + 500_000_000;

        // 1 s normal: el paquete que termina en t se lee en t + 2 ms.
        for i in 1..=100u64 {
            let t = i * 10;
            let ts = reloj.fechar(qpc(t), 480, 48_000, t as i64 + 2);
            pista.procesar_paquete(ts, &diez_ms).unwrap();
        }
        // Bloqueo: de t=1 000 a t=3 500 ms nadie lee. Se drena todo a t=3 502.
        for i in 101..=350u64 {
            let t = i * 10;
            let ts = reloj.fechar(qpc(t), 480, 48_000, 3_502);
            pista.procesar_paquete(ts, &diez_ms).unwrap();
        }
        // 1 s normal más.
        for i in 351..=450u64 {
            let t = i * 10;
            let ts = reloj.fechar(qpc(t), 480, 48_000, t as i64 + 2);
            pista.procesar_paquete(ts, &diez_ms).unwrap();
        }

        // Se capturaron 4 500 ms de audio: 72 000 muestras. El remuestreador
        // retiene como mucho un bloque (~341 de salida); lo que no puede
        // haber es el desfase de 2,5 s (40 000 muestras).
        let emitidas = pista.muestras_emitidas() as i64;
        assert!(
            (72_000 - 400..=72_000 + 400).contains(&emitidas),
            "la pista lleva {emitidas} muestras de 72 000"
        );
    }

    // -- PistaCapturada::continuar ----------------------------------------------

    #[test]
    fn continuar_conserva_el_contador_de_muestras() {
        let pista =
            PistaCapturada::continuar(Track::Mic, 1, FormatoNativo::F32, SAMPLE_RATE, 123_456)
                .unwrap();
        assert_eq!(pista.muestras_emitidas(), 123_456);
    }

    #[test]
    fn un_hueco_de_n_ms_al_reabrir_produce_n_ms_de_silencio() {
        // La pista llevaba 1 s emitido (16 000 muestras). Reabre y su primer
        // paquete, de 100 ms, llega en el instante 2 100 ms: el 1,0 s que
        // falta entre medias tiene que salir como silencio, antes del audio.
        let mut pista =
            PistaCapturada::continuar(Track::Mic, 1, FormatoNativo::F32, SAMPLE_RATE, 16_000)
                .unwrap();
        let cien_ms: Vec<u8> = (0..1_600).flat_map(|_| 0.5_f32.to_le_bytes()).collect();

        let frame = pista.procesar_paquete(2_100, &cien_ms).unwrap().unwrap();

        assert_eq!(frame.pcm.len(), 16_000 + 1_600);
        assert!(frame.pcm[..16_000].iter().all(|&m| m == 0.0));
        assert!(frame.pcm[16_000..].iter().all(|&m| m == 0.5));
        assert_eq!(pista.muestras_emitidas(), 2_100 * 16);
    }

    #[test]
    fn un_primer_paquete_tras_reabrir_sin_salida_del_remuestreador_no_pierde_ni_duplica() {
        // A 48 kHz el remuestreador solo emite bloques completos de 1 024
        // muestras: el primer paquete de una pista recién reabierta puede no
        // producir nada. El hueco, aun así, no se puede perder (el paquete
        // llega con su timestamp), y cuando el remuestreador por fin emita no
        // se puede contar otra vez lo ya rellenado.
        let mut pista =
            PistaCapturada::continuar(Track::Mic, 1, FormatoNativo::F32, 48_000, 16_000).unwrap();
        let diez_ms: Vec<u8> = (0..480).flat_map(|_| 0.5_f32.to_le_bytes()).collect();

        // 1 500 ms: esperadas 24 000, emitidas 16 000, el paquete aún no
        // aporta nada.
        let primero = pista.procesar_paquete(1_500, &diez_ms).unwrap().unwrap();
        assert_eq!(primero.pcm.len(), 8_000);
        assert!(primero.pcm.iter().all(|&m| m == 0.0));

        // Diez milisegundos después, 1 000 muestras más: ya hay bloque.
        let mil_muestras: Vec<u8> = (0..1_000).flat_map(|_| 0.5_f32.to_le_bytes()).collect();
        let segundo = pista
            .procesar_paquete(1_510, &mil_muestras)
            .unwrap()
            .expect("el bloque completo sale ahora");

        // La cuenta es exacta, sin margen para el relleno: a los 1 510 ms
        // la pista debería llevar 24 160 muestras y ya lleva 24 000, pero el
        // bloque de 1 024 muestras de entrada a 48 kHz trae 339 de salida
        // (1 024 / 3 = 341, menos el retardo del filtro), que cubren esas 160
        // con holgura. Por eso este
        // paquete no añade relleno propio: toda su salida es audio. Si el
        // relleno se contara dos veces (o se perdiera el hueco del primer
        // paquete) la longitud saldría de este rango.
        assert_eq!(segundo.pcm.len(), 339);
        assert_eq!(
            pista.muestras_emitidas(),
            16_000 + 8_000 + segundo.pcm.len() as u64
        );
    }

    // La prueba de la rama `NoSoportada` para una tercera plataforma vive en
    // `lib.rs` (`fuera_de_windows_y_linux_iniciar_devuelve_no_soportada`),
    // no aquí: tiene que llamar al `iniciar()` real, gateado por el mismo
    // `#[cfg]` que la rama que verifica, y ese enrutado con `#[cfg]` vive en
    // `lib.rs`, no en este módulo. Una versión anterior de esta prueba
    // llamaba a un enum `Plataforma` escrito a mano en este archivo, sin
    // ninguna conexión con el `#[cfg]` real: seguía en verde aunque el
    // enrutado de `lib.rs` se rompiera.

    // -- Android: formato y pistas ----------------------------------------

    #[test]
    fn el_formato_real_del_flujo_manda_sobre_el_que_se_pidio() {
        // `abrir_microfono` pide siempre PCM_FLOAT, pero AAudio abre con lo
        // que el dispositivo tenga y no avisa. Si esta traducción devolviera
        // el formato pedido en vez del recibido, un dispositivo que entrega
        // i16 se leería como f32: cuatro bytes interpretados como uno, ruido
        // blanco a todo volumen durante la reunión entera.
        assert_eq!(formato_desde_aaudio(1).unwrap(), FormatoNativo::I16);
        assert_eq!(formato_desde_aaudio(2).unwrap(), FormatoNativo::F32);
        assert_eq!(formato_desde_aaudio(4).unwrap(), FormatoNativo::I32);
    }

    #[test]
    fn un_formato_de_aaudio_que_no_se_sabe_leer_se_rechaza_en_voz_alta() {
        // PCM_I24_PACKED (3) son tres bytes por muestra y `normalizar_a_f32`
        // solo sabe de 2 y 4. Tratarlo como cualquiera de los dos leería el
        // búfer desalineado, y eso no suena a error: suena a ruido.
        assert!(formato_desde_aaudio(3).is_err());
        assert!(formato_desde_aaudio(0).is_err());
    }

    #[test]
    fn el_ancho_de_muestra_corresponde_a_cada_formato() {
        // De esto sale `bloque_bytes`, y con él cuántos bytes se leen del
        // búfer por trama. Equivocarlo desalinea todas las muestras.
        assert_eq!(bytes_por_muestra(FormatoNativo::I16), 2);
        assert_eq!(bytes_por_muestra(FormatoNativo::I32), 4);
        assert_eq!(bytes_por_muestra(FormatoNativo::F32), 4);
    }

    #[test]
    fn una_sesion_de_android_graba_solo_el_microfono() {
        // Android no permite capturar el audio de una videollamada ajena. Si
        // esta política incluyera `Track::System`, `aaudio_src` abriría una
        // pista que nunca recibe nada y la sesión acabaría con un
        // `system.wav` vacío que la mezcla suma como silencio.
        let pistas = pistas_en_android(true, false).unwrap();
        assert_eq!(pistas, vec![Track::Mic]);
    }

    #[test]
    fn pedir_capturar_el_sistema_en_android_no_impide_grabar() {
        // `CaptureConfig::default()` trae `capturar_sistema: true`, y la
        // interfaz reutiliza la misma configuración que en el escritorio.
        // Fallar aquí dejaría al usuario sin grabar la reunión por haber
        // pedido de más: se ignora esa pista y se graba el micrófono.
        let pistas = pistas_en_android(true, true).unwrap();
        assert_eq!(pistas, vec![Track::Mic]);
    }

    #[test]
    fn sin_microfono_no_hay_sesion_de_android_que_valga() {
        // El otro lado de la moneda: ignorar la pista de sistema no puede
        // llegar al extremo de aceptar una sesión que no graba nada. Pedir
        // solo sistema en Android es pedir lo imposible.
        assert!(pistas_en_android(false, true).is_err());
        assert!(pistas_en_android(false, false).is_err());
    }

    #[test]
    fn un_microfono_a_44100_se_remuestrea_a_16000() {
        // AAudio entrega la frecuencia nativa del dispositivo, no la pedida.
        // Sin remuestrear, el WAV sale a casi el triple de velocidad y
        // Whisper transcribe ardillas.
        let mut pista = PistaCapturada::nueva(Track::Mic, 1, FormatoNativo::F32, 44_100)
            .expect("44100 es una frecuencia de entrada válida");

        // Un segundo de audio real a 44,1 kHz.
        let un_segundo: Vec<u8> = (0..44_100).flat_map(|_| 0.5_f32.to_le_bytes()).collect();
        let frame = pista
            .procesar_paquete(1000, &un_segundo)
            .unwrap()
            .expect("un segundo de audio produce muestras");

        // El remuestreador trabaja por bloques y deja una cola sin emitir, así
        // que no salen exactamente 16 000; lo que no puede pasar es que salgan
        // las 44 100 de entrada.
        let emitidas = frame.pcm.len();
        assert!(
            (15_000..=16_100).contains(&emitidas),
            "un segundo a 44,1 kHz debe salir como ~16 000 muestras, salieron {emitidas}"
        );
    }
}
