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

/// Si, tras el sondeo de esta vuelta del bucle, sigue habiendo alguna pista
/// viva para seguir capturando -- o si ya no queda ninguna y hay que
/// terminar la sesión.
///
/// Es la misma pregunta que `pistas_a_grabar` responde al arrancar
/// (`microfono_abierto`/`sistema_abierto` ahí), pero para cuando una pista
/// muere sola a mitad de grabación: la apertura real y el sondeo COM viven
/// en `wasapi_src` y no se pueden probar aquí sin envolver COM entero, pero
/// la política sí -- seguir con lo que quede, o terminar si no queda nada.
/// Sin este corte, el bucle de `wasapi_src` seguiría sondeando cada 10 ms
/// sin producir ningún `AudioFrame` más y sin soltar el `Sender` que cierra
/// el canal: la sesión aparentaría seguir grabando sin grabar nada.
pub fn alguna_pista_sigue_viva(microfono_vivo: bool, sistema_vivo: bool) -> bool {
    microfono_vivo || sistema_vivo
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
        Ok(Self {
            track,
            canales: canales.max(1),
            formato,
            remuestreador: Remuestreador::nuevo(frecuencia)?,
            muestras_emitidas: 0,
        })
    }

    /// Muestras (a 16 kHz) ya entregadas por esta pista. Solo para pruebas.
    pub fn muestras_emitidas(&self) -> u64 {
        self.muestras_emitidas
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

    // -- alguna_pista_sigue_viva --------------------------------------------

    #[test]
    fn una_pista_muerta_no_frena_a_la_otra() {
        // Antes de extraer esta política a una función pura, la decisión
        // vivía como control de flujo directo dentro del bucle COM de
        // `wasapi_src`, sin ninguna prueba posible sin hardware: el AC 7 de
        // la HU pide cubrir "el manejo del cambio de dispositivo por
        // defecto" y no había forma de hacerlo.
        assert!(alguna_pista_sigue_viva(true, false));
        assert!(alguna_pista_sigue_viva(false, true));
    }

    #[test]
    fn con_las_dos_pistas_vivas_la_sesion_sigue() {
        // El caso más frecuente —una grabación sana— y el único que faltaba
        // por cubrir. No es redundante: `||` y `!=` dan el mismo resultado en
        // las otras tres combinaciones, así que sin esta línea una mutación
        // de `||` a `!=` pasaría las demás pruebas en verde y haría que el
        // bucle terminara de inmediato justo cuando todo va bien.
        assert!(alguna_pista_sigue_viva(true, true));
    }

    #[test]
    fn sin_ninguna_pista_viva_el_bucle_termina() {
        // Antes de este arreglo, `wasapi_src::bucle` no tenía ningún
        // `break` para este caso: el hilo seguía vivo sondeando cada 10 ms
        // sin producir ningún `AudioFrame` más y sin cerrar el canal, así
        // que la sesión aparentaba seguir grabando sin grabar nada.
        assert!(!alguna_pista_sigue_viva(false, false));
    }

    #[test]
    fn una_sesion_de_una_sola_pista_termina_si_esa_pista_muere() {
        // Antes arrancaba con `pistas_a_grabar(true, false)`, el mismo par
        // que ya fija `sin_dispositivo_de_salida_se_graba_solo_el_microfono`.
        // Medido por mutación (verificador-pruebas, T-15/T-16), ese
        // `assert_eq!` deja `pistas_al_arrancar` en exactamente `[Mic]`, así
        // que `.contains(&Track::System)` no podía dar otra cosa que
        // `false`: ninguna mutación pasaba ese `assert_eq!` y a la vez movía
        // el resultado. La prueba entera quedaba 100% redundante con esa otra
        // y con `sin_ninguna_pista_viva_el_bucle_termina`.
        //
        // Arrancar aquí con el sistema en vez del micrófono no es cosmético:
        // ninguna otra prueba del archivo fija el resultado exacto de la
        // rama `if sistema_abierto` en solitario, así que un
        // `pistas.push(Track::Mic)` por error ahí —en vez de
        // `Track::System`— pasaría todas las demás pruebas en verde y solo
        // esta lo detectaría.
        let pistas_al_arrancar = pistas_a_grabar(false, true).unwrap();
        assert_eq!(pistas_al_arrancar, vec![Track::System]);

        // `microfono_vivo` se deriva de lo que devolvió `pistas_a_grabar`, no
        // se escribe a mano: si algún día `pistas_a_grabar(false, true)`
        // empezara a incluir `Mic` por error, esta prueba lo notaría también
        // por este lado.
        let microfono_vivo = pistas_al_arrancar.contains(&Track::Mic);
        let sistema_vivo = false; // la única pista pedida, y murió

        assert!(!alguna_pista_sigue_viva(microfono_vivo, sistema_vivo));
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
