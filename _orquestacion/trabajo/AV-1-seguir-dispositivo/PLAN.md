# PLAN: AV-1 «Que la grabación siga al dispositivo de audio activo»

Origen: [docs/investigacion-notion-reuniones-2026-10-12.md §6 bis](../../../docs/investigacion-notion-reuniones-2026-10-12.md) · deuda **T-9** del [tablero](../../tablero.md) · relacionada con HU-15 del [plan de Windows](../../../docs/plan-correccion-calidad-windows.md).

**Revisión 2**, tras el contradictor. Los cambios respecto de la revisión 1 están justificados en «Respuesta al contradictor», al final.

## Problema

`wasapi_src.rs` abre el loopback y el micrófono **una sola vez**, sobre el dispositivo por defecto (`eConsole`) que haya al arrancar (`abrir_cliente`, `wasapi_src.rs:420-470`). Hay dos escenarios reales de clase que hoy fallan:

1. **El usuario cambia la salida por defecto a mitad de clase** (se pone auriculares o pasa a HDMI). El navegador, que está en «Predeterminado», sigue al nuevo dispositivo. Nuestro loopback sigue enganchado al viejo, y probablemente queda mudo sin dar error.
2. **El dispositivo se desconecta** (USB o Bluetooth) y llega `AUDCLNT_E_DEVICE_INVALIDATED`. Hoy la pista se cierra para siempre (`wasapi_src.rs:270-300`) y, si era la única, termina la sesión.

## Paso 0: verificar la premisa antes de programar el resto

La premisa es que el cliente viejo deja de entregar paquetes, o los entrega con `AUDCLNT_BUFFERFLAGS_SILENT`, cuando la salida por defecto cambia. Hay que medirlo durante unos 60 s, con el ejemplo `wasapi_loopback_eval` o uno nuevo:

- registrar `GetNextPacketSize`, las banderas y los errores del cliente abierto sobre el dispositivo viejo, antes y después de cambiar la salida por defecto en Windows;
- dejar el resultado en HANDOFF.

**Si el cliente viejo sigue entregando el audio real** (es decir, Windows no movió el stream), D1 sigue siendo necesario, pero el riesgo de perder audio cambia y hay que decirlo.

## Decisiones de diseño (las toma el orquestador)

### D1. Sondeo del dispositivo por defecto, tolerante a errores y con margen

Cada **1 s**, por pista, se obtiene el id del dispositivo por defecto **reutilizando `id_de_endpoint_por_defecto`** (`wasapi_src.rs:596-640`) para `eRender` y `eCapture`, siempre con el rol `eConsole`.

- **Error del sondeo** (por ejemplo `E_NOTFOUND` o una reenumeración en curso): **no se cambia nada** y no se toca una pista sana.
- **Margen contra cambios que van y vienen:** se considera que hubo un cambio solo si el id nuevo es distinto del abierto **y** se mantiene en **2 sondeos seguidos**. Además, una misma pista no se reabre dos veces en menos de **5 s**. Esto cubre el caso de los auriculares Bluetooth, en los que Windows alterna entre los dispositivos Stereo y Hands-Free al abrir el micrófono.
- **Limitación declarada:** solo se sigue `eConsole`. Si Meet o Teams usan el dispositivo de comunicaciones (`eCommunications`) y este es distinto, no se sigue. Queda escrito en el comentario del módulo.

### D2. Estado por pista y política de terminación

Va en `sincronia.rs`, como lógica pura y probada.

```
EstadoPista = Viva { id_dispositivo }
            | Reabriendo { desde, muestras_emitidas, ultimo_intento, intentos }
            | Ausente { id_fallido: Option<String>, muestras_emitidas }   // fallo permanente con ese dispositivo
            | NoPedida                                                     // cfg no la pidió
```

| Disparador | Transición |
|---|---|
| Error COM transitorio (`DEVICE_INVALIDATED` u otro HRESULT al leer) | `cerrar_pista` (vacía el remuestreador) y después **Reabriendo** con el contador leído **después** del vaciado |
| D1 detecta un cambio estable | Igual: un solo cierre y una sola reapertura, aunque coincida con un `DEVICE_INVALIDATED` |
| En Reabriendo: reintento | Cada 2 s durante el primer minuto y cada 5 s después. Si abre, **Viva** con `PistaCapturada::continuar(…, muestras_emitidas)` |
| En Reabriendo: fallo permanente (formato no soportado por `leer_formato`, `wasapi_src.rs:507-516`) | **Ausente** `{ id_fallido }` |
| En Ausente: D1 ve un id por defecto distinto de `id_fallido` | **Reabriendo** |
| Pista pedida que **no abrió al arrancar** | Empieza en **Reabriendo** con contador 0: el relleno cubrirá desde el inicio. `pistas_a_grabar` sigue exigiendo que al menos una abra al arrancar, como hoy |

**Política:** la sesión **no termina sola por perder dispositivos**. Termina cuando la persona la detiene.

- `alguna_pista_sigue_viva` se sustituye por `debe_terminar(&[EstadoPista]) -> bool`, que solo es `true` si **todas** las pistas pedidas están en `Ausente` y ninguna puede volver (no hay id por defecto distinto que probar).
- Las cinco pruebas de `sincronia.rs:481-539` se **reescriben** sobre la nueva política, con la misma tabulación exhaustiva que exigió HU-01 (v2-3, v3-1): todas las combinaciones de estados de las dos pistas, y la frontera de cada umbral de tiempo con `>=`.
- Las pruebas que fijaban el comportamiento antiguo («una sesión de una sola pista termina si esa pista muere») se sustituyen explícitamente por su contrario. HANDOFF lo explica.

**Justificación:** grabar una clase que ocurre una sola vez es más importante que liberar recursos. Una sesión que termina sola porque se desconectaron los auriculares pierde el resto de la clase, y una que espera el dispositivo no pierde nada.

### D3. La línea de tiempo sobrevive a la reapertura y al cierre

- `PistaCapturada::continuar(track, canales, formato, frecuencia, muestras_emitidas)` es nuevo.
- El contador vive en el `EstadoPista` cuando no hay `PistaCapturada`. Los reintentos fallidos nunca lo reinician.
- **Al detener la sesión**, para cada pista pedida que no esté viva (Reabriendo o Ausente con contador), se emite un relleno de silencio hasta `fin_ms`, para que todos los WAV duren lo mismo que la sesión. Hoy `cerrar_pista` solo emite la cola del remuestreador (`wasapi_src.rs:348-362`).

### D4. Reabrir sin dejar sin servicio a la otra pista

- Antes de abrir un cliente nuevo, se drena la otra pista con `procesar_paquetes`.
- Se mide el tiempo de cada apertura (`Activate` + `GetMixFormat` + `Initialize` + `Start`) y se registra.
- **Si en la prueba manual alguna apertura supera 150 ms**, el implementador **no** mueve la apertura a otro hilo por su cuenta: lo informa en HANDOFF con las medidas, y el orquestador decide si hace falta una revisión 3.

### D5. Registros acotados

- Cada transición deja **un** `info!`/`warn!` con pista, id viejo y nuevo, motivo y milisegundos de apertura.
- Durante Reabriendo: un `warn!` al entrar, uno al salir, y como mucho uno cada 30 s entre medias.
- Un fallo permanente se registra con su causa propia, distinta de la de un error transitorio.

## Qué ve quien consume el canal (`core/api/src/grabacion.rs`)

| Caso | Hoy | Con AV-1 |
|---|---|---|
| Pista reabriendo | (no existe) | No llegan tramas de esa pista. Al reabrir llega una trama cuyo `timestamp_ms` refleja el hueco; `muestras_de_relleno` lo rellena |
| Las dos pistas sin dispositivo | La sesión terminaba y el canal se cerraba | El canal **sigue abierto** y no llegan tramas. `Evento::Niveles` sigue con `transcurrido_ms` detenido |
| Detener | Se cierra el canal | Igual, más el relleno final de D3 |

`bucle_escritura` no depende de que las pistas terminen (verificado por el contradictor). **No se toca `grabacion.rs`.**

El caso «sin dispositivo y sin aviso» lo cubre **AV-2**:

- **Micrófono:** `mic_sin_tramas`, a los 10 s sin tramas. Un micrófono abierto siempre entrega tramas, aunque sean de silencio.
- **Sistema:** `sin_senal_inicio` (30 s) y `sin_senal_pausa` (3 min). En el loopback, la falta de tramas también ocurre en un silencio normal, así que no puede usarse sola como señal de dispositivo perdido. **AV-1 y AV-2 se integran juntas.** Ninguna se da por cerrada si la otra no lo está, porque AV-1 sola convertiría «la sesión termina» en «la sesión sigue grabando nada».

## Fuera de alcance

- Captura por proceso (AV-5).
- El aviso en la interfaz (AV-2).
- Linux y Android.
- Elegir dispositivo a mano.
- El dispositivo de comunicaciones (`eCommunications`).
- **`cli/src/grabar.rs`**: gira al 100 % de CPU con el canal cerrado y no drena `rx` al parar. Es un defecto previo, anotado como **T-18** en el tablero. Por eso la prueba manual usa el ejemplo y no `dictar grabar`.

## Archivos (lista cerrada)

| Ruta | Cambio |
|---|---|
| `core/audio-capture/src/wasapi_src.rs` | Sondeo D1, estados D2, drenaje y medición D4, relleno final D3, registros D5 |
| `core/audio-capture/src/sincronia.rs` | `EstadoPista`, `debe_terminar` (sustituye a `alguna_pista_sigue_viva`), `PistaCapturada::continuar`, lógica pura de margen y reintentos, con pruebas |
| `core/audio-capture/src/aaudio_src.rs` | Solo si su comentario de la línea 309 nombra `alguna_pista_sigue_viva`: actualizar el nombre. Sin cambio de código |
| `core/api/examples/wasapi_loopback_eval.rs` | **Obligatorio:** modo `--seguir-dispositivo` que registra transiciones, escribe `system.wav` y `mic.wav`, e imprime un resumen por ventana de 1 s (RMS) para medir el criterio 3 |

**No toca `Cargo.toml`**: las features necesarias ya están y `GetId` + `CoTaskMemFree` ya se usan. Disjunta de AV-2 y AV-3.

## Criterios de aceptación

1. **Pruebas sin COM**, `cargo test -p dictar-audio`, en verde. Cubren:
   - las transiciones de la tabla D2, todas;
   - `debe_terminar` en todas las combinaciones de estados de dos pistas;
   - el margen de D1: un id que aparece un solo sondeo no reabre, y dos reaperturas de la misma pista en menos de 5 s tampoco;
   - el calendario de reintentos (2 s y luego 5 s) en sus fronteras;
   - `continuar`: el contador se conserva; un hueco de N ms produce N ms de silencio a 16 kHz; y un **primer paquete tras reabrir que no produce salida del remuestreador** no duplica ni pierde muestras;
   - el relleno final hasta `fin_ms`.
2. `cargo clippy -p dictar-audio --all-targets -- -D warnings` limpio en Windows, y `cargo check --target x86_64-unknown-freebsd -p dictar-audio --all-targets` en verde.
3. **Pruebas manuales en Windows**, con `RUST_LOG=dictar_audio=debug,info` y el ejemplo en Release. Para cada una, HANDOFF registra la aplicación que reproducía el audio (por ejemplo Chrome con YouTube en «Predeterminado»), el log de transiciones, la duración de los WAV y el resumen de RMS por segundo:
   - **(a) Cambio de salida por defecto:** a los 20 s y de vuelta a los 40 s, sobre 60 s con audio continuo. Pasa si: tras cada cambio, la primera ventana de 1 s con RMS > 0,003 en `system.wav` llega **≤ 3 s** después del cambio; la duración de `system.wav` = sesión ± 0,5 s; y el log muestra exactamente 2 reaperturas.
   - **(b) Dispositivo invalidado:** deshabilitar el dispositivo de salida activo en el Panel de sonido durante 15 s y volver a habilitarlo, con un solo dispositivo. Pasa si: la pista pasa a Reabriendo y vuelve a Viva; la sesión no termina; la duración de `system.wav` = sesión ± 0,5 s; y el hueco es silencio.
   - **(c) Micrófono:** cambiar el micrófono por defecto con la grabación en marcha. Mismos criterios que (a) sobre `mic.wav`.
   - **(d) Sin cambios:** 60 s sin tocar nada, frente a una ejecución equivalente con el código del respaldo (`refs/respaldo/antes-audio-vivo-2026-10-13`). La duración y el número de muestras de cada WAV coinciden ± 0,1 s, y no hay ninguna transición en el log.
   - Si (a) o (c) no se pueden hacer por falta de un segundo dispositivo, se marcan **NO VERIFICABLE**, pero (b) y (d) son obligatorias.
4. **Tiempos de apertura** de D4 en HANDOFF: mínimo, máximo y en qué dispositivo.

## Respuesta al contradictor (revisión 1 → 2)

| Objeción | Respuesta |
|---|---|
| 1. D2 contradictoria; cláusula de 30 s inalcanzable; pistas no abiertas al inicio; las pruebas antiguas se rompen | Aceptada. Nueva máquina de estados con `Ausente` permanente y `NoPedida`; la sesión no termina sola; pista no abierta al inicio = Reabriendo; las pruebas se reescriben con tabulación exhaustiva y se declaran sus inversiones |
| 2. Consumidor: el canal no se cierra; ventana sin aviso; CLI | Aceptada. Tabla «Qué ve quien consume el canal»; AV-2 avisa por ausencia de tramas en ≥ 10 s; AV-1 y AV-2 se integran juntas; CLI fuera de alcance como T-18 y prueba manual con el ejemplo |
| 3. Errores del sondeo, margen, `eConsole`/`eCommunications` | Aceptada. Error de sondeo = sin cambios; 2 sondeos estables y 5 s entre reaperturas; `eCommunications` declarado fuera |
| 4. Bloqueo del hilo al reabrir | Aceptada en parte. Se drena la otra pista y se mide; si pasa de 150 ms decide el orquestador, no el implementador |
| 5. Dónde vive el contador; idempotencia; primer paquete sin salida | Aceptada. El contador va en `EstadoPista`, se lee tras vaciar, hay un solo cierre por evento, y hay prueba del primer paquete vacío |
| 6. Criterio 3 aprobable sin reapertura; falta (b) y micrófono; registros | Aceptada. Criterios con umbral medible (≤ 3 s hasta señal), escenarios (b), (c) y (d), y `RUST_LOG` explícito con el ejemplo obligatorio |
| 7. Inundación de registros; fallo permanente de formato | Aceptada. D5 y estado `Ausente` |
| 8. `Cargo.toml` innecesario; reutilizar `id_de_endpoint_por_defecto` | Aceptada |
| Premisa: paquetes SILENT o ninguno | Aceptada como Paso 0 medido |
| Premisa: el hueco final no se rellena | Aceptada; relleno final en D3 |

## Adenda del orquestador (13/10, tras la primera entrega)

1. **`debe_terminar`:** la sesión **nunca** termina sola por el estado de los dispositivos, ni siquiera con todas las pistas pedidas en `Ausente`. `Ausente` sigue vigilando D1 y, si aparece un id por defecto distinto de `id_fallido`, pasa a Reabriendo. Solo `detener()` termina la sesión. El implementador había propuesto que «todas Ausente» terminara la sesión; se descarta por coherencia con la justificación de D2. El aviso al usuario lo da AV-2.
2. **Criterio de duración, ahora relativo.** La línea base medida muestra que el código anterior ya deja los WAV entre 0,85 y 1,45 s por debajo de `sesion_ms`, así que el «± 0,5 s» frente a la sesión era inalcanzable por razones ajenas a AV-1. Ahora:
   - en (d), la duración de cada WAV con el código nuevo = la de la línea base ± 0,2 s en la misma prueba;
   - en (a) y (b), lo que falta respecto de `sesion_ms` no supera lo que faltaba en la línea base + 0,3 s.
3. **Incidente registrado.** Antes de recibir la indicación de compartir `target/`, el implementador rodeó Smart App Control en su propia copia borrando y volviendo a enlazar el paquete bloqueado. Lo declaró él mismo en HANDOFF. No se repite.
