# PLAN — HU-01 «Grabar en Windows (WASAPI loopback)»

HU de origen: [`docs/06-historias-de-usuario.md#hu-01`](../../../docs/06-historias-de-usuario.md)
· Tarea T-4 del [tablero](../../tablero.md)

**Revisión 2**, tras el contradictor. Los cambios respecto de la revisión 1 están justificados
en «Respuesta al contradictor», al final.

## Análisis

Es la historia que más valor desbloquea del tablero: hoy el `.exe` se instala, arranca, y no
graba. Todo lo demás —transcribir, resumir, buscar, leer apuntes— ya funciona en Windows.

`core/audio-capture/src/lib.rs` tiene el hueco hecho: `iniciar()` y `dispositivos()` ya son
funciones con dos ramas, y la de fuera de Linux devuelve `AudioError::NoSoportada`. El trabajo
es sustituir esa rama por una implementación real.

`pipewire_src.rs` es la referencia **de organización**: mismo `CaptureSession`, mismo patrón de
arranque, mismo contrato de `AudioFrame`. **No es la referencia de su mecanismo de reloj**, y
esa distinción es el núcleo de este plan — ver la decisión de abajo.

**Fuera de alcance a propósito:** la reproducción en Windows (HU-07) y el selector de
dispositivo en la interfaz. Esta HU entrega captura con los dispositivos por defecto.

## Decisión de diseño: el reloj y el silencio

La toma el orquestador aquí, no el implementador, porque nadie va a poder comprobarla después.

`pipewire_src.rs` deriva el `timestamp_ms` del **conteo de muestras emitidas**
(`pipewire_src.rs:56-59`): un desfase inicial tomado del reloj de pared, y a partir de ahí solo
muestras. Eso es seguro **solo** porque las dos pistas viven en el mismo grafo de PipeWire, con
un único reloj que resamplea cada nodo. WASAPI no da eso: micrófono y loopback son dos
`IAudioClient` sobre dispositivos con osciladores independientes, y el loopback **no entrega
paquetes durante el silencio absoluto**.

Copiar el mecanismo produciría dos fallos: deriva continua entre pistas, y un desplazamiento
permanente de todo lo posterior a cada silencio.

**La decisión son dos cosas, no una disyuntiva:**

1. **El `timestamp_ms` de cada `AudioFrame` se lee del reloj monótono** en el momento de recibir
   el paquete (`Instant::now()` contra el instante de arranque de la sesión). No se acumulan
   muestras.
2. **El hueco se rellena con silencio antes de escribir.** El timestamp por sí solo no basta:
   `EscritorPistas` escribe secuencialmente, así que si el loopback calló dos minutos y no
   entregó nada, el WAV quedaría dos minutos más corto y todo lo posterior sonaría adelantado.
   Se compara el timestamp del paquete con las muestras ya emitidas en esa pista, y se inserta
   la diferencia como silencio.

Es una desviación deliberada del criterio 3 de la HU, que dice «igual que hace `pipewire_src`».
Se cumple el fondo del criterio —reloj monótono común a las dos pistas— y no la letra. Queda
escrito aquí para que el revisor no lo tome por un descuido.

## Archivos y paquetes afectados

Lista cerrada. Comparada con [`HU-05-llavero/PLAN.md`](../HU-05-llavero/PLAN.md): sin
solapamiento.

| Ruta | Cambio esperado |
|---|---|
| `core/audio-capture/Cargo.toml` | Dependencias de Windows bajo `[target.'cfg(windows)'.dependencies]` |
| `core/audio-capture/src/sincronia.rs` | **Nuevo, y sin `#[cfg]` de plataforma.** La lógica pura: conversión de formato nativo a `f32`, cálculo del relleno de silencio, y política de pistas |
| `core/audio-capture/src/wasapi_src.rs` | **Nuevo, Windows.** Solo COM y bucle de audio; delega en `sincronia` |
| `core/audio-capture/src/lib.rs` | Declarar los dos módulos y enrutar `iniciar()` y `dispositivos()` |
| `.github/workflows/ci.yml` | `cargo test` en el job de Windows, que hoy solo hace `cargo check` |

**No se toca** `core/api`, ni `cli`, ni `mezcla.rs`, ni `wav.rs`, ni `reproductor*.rs`.

El módulo `sincronia.rs` **existe por una razón de verificabilidad, no de estética**: es lo que
permite que las pruebas de esta HU se compilen y ejecuten en el CI de Linux. Un
`wasapi_src.rs` gateado a Windows no compila allí, y en Windows el CI solo hace `check`.

## Dependencias

- HU previas: ninguna. HU-12 dejó el terreno.
- Bloqueos: **B-1** (no hay `cargo`) y, más grave aquí, **no hay Windows con tarjeta de sonido
  donde probar**. Los criterios 1, 2, 4, 5 y 6 son **no comprobables aquí**.

## Casos del dominio que hay que cubrir

- [x] **Audio que no viene a 16 kHz.** WASAPI entrega el formato del dispositivo: 48 kHz estéreo
      en `f32`, a veces `i16` o `i32`, a veces 44,1. Se convierte a `f32` en `sincronia`, y de
      ahí a `mezcla::a_mono` y `mezcla::Remuestreador`. Invariante 3.
- [x] **Cola del remuestreador.** Al detener **sí** se llama a `Remuestreador::vaciar()`.
      `pipewire_src` no lo hace y pierde ~21 ms por pista en cada cierre; eso es un defecto
      conocido, no un contrato que replicar. Ver la deuda que esto abre, abajo.
- [x] **Pista de sistema ausente.** Sin dispositivo de salida, o con el loopback denegado, se
      emite solo micrófono. La política es pura y testeable en `sincronia`.
- [x] **Pistas de distinta longitud.** No se recorta a la más corta; el reloj monótono y el
      relleno de silencio son lo que las mantiene alineadas.
- [x] **Clase de dos horas.** Los contadores de muestras van en `u64`/`i64`, no en `u32`: a
      16 kHz, 7200 s son 115 millones de muestras. Y ningún buffer que crezca sin drenarse.
- [x] **Plataforma sin backend.** Al añadir Windows, la rama `NoSoportada` sigue viva para
      Android y macOS. Invariante 4: la firma no cambia.
- [x] **El loopback se abre sobre el dispositivo de SALIDA**, no sobre uno de entrada:
      `IAudioClient` en modo captura con `AUDCLNT_STREAMFLAGS_LOOPBACK` sobre el *render
      endpoint* por defecto.
- [x] **`CoInitializeEx` va en cada hilo** que hable con WASAPI, y se despareja al terminar.
- [x] **El dispositivo por defecto puede cambiar** a mitad de clase. **Decisión: no se soporta
      en esta HU.** Si el dispositivo desaparece, la pista termina y se registra en el log; la
      otra sigue. No se intenta reengancharla. Se anota como deuda.

## Riesgos

| Riesgo | Mitigación |
|---|---|
| Ni compilador ni Windows con audio | Se asume; el cierre es condicionado. Toda prueba nueva vive en `sincronia.rs` y corre sin hardware |
| La API de `windows 0.58` podría no traer las *features* de WASAPI que hacen falta | El implementador lo verifica **antes** de escribir el bucle, y si falta algo lo escala en vez de improvisar |
| Un `unsafe` mal puesto en el bucle de audio corrompe la grabación | El `unsafe` se aísla en las llamadas COM; `sincronia.rs` es 100 % seguro |
| Reimplementar lo que ya está en `mezcla.rs` | Prohibido: se reutiliza `a_mono`, `Remuestreador::procesar` y `vaciar` |

## Estrategia

1. Leer `pipewire_src.rs` entero y anotar el contrato de `AudioFrame` que cumple.
2. Escribir `sincronia.rs` **primero**, con sus pruebas. Es lo único verificable, así que va
   antes: conversión de formato, relleno de silencio y política de pistas.
3. Verificar la API de `windows 0.58` para WASAPI y decidir las *features*.
4. Captura del micrófono (dispositivo de entrada normal).
5. Loopback sobre el dispositivo de salida.
6. Enrutar en `lib.rs` y añadir `cargo test` al job de Windows del CI.

## Pruebas necesarias

Todas viven en `sincronia.rs`, compilan en cualquier plataforma y **se ejecutan en el CI de
Linux**. Ninguna necesita tarjeta de sonido; si una la necesitara, estaría mal planteada.

| Prueba | Cubre | Mutación esperada que la pone en rojo |
|---|---|---|
| `las_muestras_i16_del_dispositivo_se_normalizan_a_f32` | AC 3 (parte nueva) | Dividir por `i16::MAX + 1` mal, o no dividir |
| `un_silencio_en_loopback_no_adelanta_lo_que_viene_despues` | AC 3, silencio en loopback | Devolver `0` de relleno siempre |
| `el_relleno_no_se_calcula_a_partir_de_las_muestras_recibidas` | AC 3, decisión del reloj | Derivar el timestamp del conteo de muestras |
| `sin_dispositivo_de_salida_se_graba_solo_el_microfono` | AC 2, pista ausente | Hacer que el fallo del loopback aborte la sesión entera |
| `sin_ninguna_pista_disponible_es_un_error_explicito` | AC 2 | Devolver `Ok(vec![])` en vez de error |
| `dos_horas_de_muestras_no_desbordan_el_contador` | Clase de dos horas | Cambiar el contador a `u32` |
| `fuera_de_windows_y_linux_sigue_devolviendo_no_soportada` | Invariante 4 | Quitar la rama `NoSoportada` |

La conversión a mono y el remuestreo **no se reprueban**: ya están cubiertos en `mezcla.rs`
(`de_48k_a_16k_sale_aproximadamente_un_tercio`, `el_estereo_se_promedia_…`). Lo que se prueba
aquí es solo la parte nueva: normalizar el formato nativo del dispositivo a `f32`.

## Deuda que este plan abre a propósito

Va al tablero, no se resuelve aquí:

- `pipewire_src` no llama a `Remuestreador::vaciar()` al cerrar y pierde ~21 ms por pista en
  cada cierre. WASAPI sí lo hará; Linux queda inconsistente.
- El cambio de dispositivo por defecto a mitad de sesión no se soporta en ninguna plataforma.
- Al cerrar esta HU, el comentario de cabecera de `.github/workflows/release.yml` («Falta WASAPI
  *loopback* en Windows») queda falso. Lo actualiza quien cierre, no el implementador.

## Agentes a lanzar

`implementador`, y a la vuelta los tres: `revisor-codigo`, `verificador-pruebas` y
`auditor-plataforma` — este último obligatorio: se toca `Cargo.toml` de plataforma, `#[cfg]` y
un workflow.

---

## Respuesta al contradictor

| # | Objeción | Veredicto del orquestador |
|---|---|---|
| 1 | El reloj de `pipewire_src` no es trasladable: deriva del conteo de muestras y eso solo funciona con el grafo único de PipeWire. Y el plan se contradecía — mandaba imitarlo mientras proponía una prueba que reprobaría ese mismo mecanismo | **Aceptada, y es la objeción que más valor tuvo.** Se añade la sección «Decisión de diseño: el reloj y el silencio», que resuelve lo que la revisión 1 dejaba abierto. Corrijo además el planteamiento del propio contradictor: no es «rellenar con silencio **o** usar el reloj monótono», son **las dos cosas**. El reloj arregla el `timestamp_ms` del frame; sin el relleno, el WAV en disco sigue quedando corto y todo lo posterior suena adelantado. La disyuntiva era falsa |
| 2 | Las cuatro pruebas no se ejecutarían nunca: el módulo es Windows-only, el job de Linux corre `cargo test` pero no compila ese módulo, y el de Windows solo hace `cargo check --all-targets`, que tipa-chequea sin ejecutar | **Aceptada.** Dos medidas: `sincronia.rs` sin `#[cfg]` de plataforma, para que lo verificable compile y corra en Linux; y `cargo test` añadido al job de Windows. `.github/workflows/ci.yml` entra en la lista de archivos, que en la revisión 1 lo omitía |
| 3 | `sin_dispositivo_de_salida_se_graba_solo_el_microfono` no es escribible: no hay capa de inyección para el enumerador de dispositivos ni para `IAudioClient::Initialize`, y el precedente de `pipewire_src` no traslada porque ahí es una validación de entrada, no una simulación | **Aceptada.** Se hace pura la **política**, no el dispositivo: `sincronia` recibe qué pistas se abrieron y decide qué hacer. Inyectar COM entero sería sobreingeniería para probar un `if`. La apertura real queda declarada como no comprobable aquí |
| 4 | La prueba de conversión de formato es ambigua entre redundante y necesaria: no distingue si ejercita `a_mono`/`Remuestreador` (ya cubiertos) o la conversión específica de los formatos nativos de WASAPI (lógica nueva) | **Aceptada.** Se parte en pruebas concretas sobre la parte nueva, y se dice explícitamente qué no se reprueba y dónde ya está cubierto |
| 5 | Nota: al cerrar la HU, el comentario de `release.yml` queda falso | **Aceptada como deuda de cierre**, no del implementador: ese archivo no está en su lista |
| 6 | Observación: `pipewire_src` no llama a `vaciar()` al cerrar, así que «hacer lo mismo» significa aceptar la misma pérdida de ~21 ms — la premisa de que hay algo que replicar ahí es falsa | **Aceptada, y se decide al revés de lo que sugería.** WASAPI **sí** llamará a `vaciar()`. Replicar un defecto conocido por simetría es la peor de las dos opciones; la inconsistencia con Linux va al tablero como deuda |
| — | No verificado por el contradictor, y se traslada al implementador: si `windows 0.58` trae completas las *features* de WASAPI (loopback, `IMMNotificationClient`) | Anotado en Riesgos. Se verifica **antes** de escribir el bucle, y si falta algo se escala en vez de improvisar |
