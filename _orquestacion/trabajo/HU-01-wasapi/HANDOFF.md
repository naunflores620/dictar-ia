HANDOFF — HU-01 «Grabar en Windows (WASAPI loopback)»

Escrito por quien implementó, al entregar.

> **Para los revisores:** esto es una **declaración**, no evidencia. Todo lo que dice acá está
> por verificarse. Ver `protocolo.md`, regla 1.

## Estado

**Tercera vuelta.** La primera entrega volvió a implementación con tres hallazgos Bloqueantes,
tres Importantes de la familia «regla de la réplica», y tres Importantes/Menor más. La segunda
entrega cerró esos nueve —con evidencia propia de cada revisor, no citando este documento—, pero
el propio código nuevo que cerraba el Bloqueante 1 (la función `cerrar_pista` y el bucle que la
rodea) trajo tres hallazgos suyos: `v2-1` Bloqueante y `v2-2`/`v2-3` Importantes (`REVIEW.md` de
esta carpeta, sección «Segunda vuelta», y `REVIEW-codigo.md`, mismo nombre de sección, hallazgos
`v2-I2`, `v2-I1` y `v2-I3` respectivamente). Los tres están corregidos en esta vuelta, acotada por
encargo a ellos tres — no se tocó nada de los nueve ya cerrados ni de las dos decisiones que el
`REVIEW.md` aceptó expresamente (aislamiento en los `Start()`, nombres genéricos de dispositivo).

**Sigue sin haber `cargo`, `rustc` ni `rustfmt` en esta máquina** (reverificado de primera mano
otra vez, ver «Comandos para reproducir»): nada de esto se ejecutó. A diferencia de las dos vueltas
anteriores, esta ronda no tenía aritmética nueva que verificar con Python: los tres hallazgos son
de control de flujo (cuándo termina el bucle) y de manejo de errores (qué se registra), no de
cálculo. Lo que sí verifiqué con Python fue lo estructural — ancho de línea en caracteres (no
bytes), balance de llaves y paréntesis, y que cada función nueva tiene un único sitio de
definición y los que corresponden de llamada —, con el detalle en la sección «Tercera vuelta» de
abajo. Solo dos archivos, los dos ya declarados en el `PLAN.md`:
`core/audio-capture/src/{wasapi_src.rs, sincronia.rs}`.

La sección «Segunda vuelta» de abajo recorre los nueve hallazgos originales; la sección «Tercera
vuelta», los tres nuevos de esta ronda. El resto del documento (Archivos tocados, Comandos,
Criterios de aceptación, Invariantes, Decisiones, Lo que NO pude verificar, Deuda que dejo) se
actualizó para reflejar el estado acumulado del código a través de las tres vueltas, no el de una
entrega anterior.

## Segunda vuelta — qué se corrigió de cada hallazgo (`REVIEW.md`, 2026-09-03)

### 1. Bloqueante — se perdía audio si una pista fallaba a mitad de sesión

`core/audio-capture/src/wasapi_src.rs`, función `bucle` (línea 158) y bucle principal (antes
219-225, ahora 250-271).

**Qué hice:** el bucle ya no usa `?` sobre `procesar_paquetes`. Cada pista se sondea con `match`:
si falla, se registra con `tracing::error!`, se cierra *esa* pista con la función nueva
`cerrar_pista` (línea 307: vacía el remuestreador y llama `Stop()`) y se pone su variable
(`mic`/`sistema`) a `None`; la otra pista sigue el bucle sin enterarse. `cerrar_pista` es la misma
función que ahora también se llama en el cierre normal (antes 235-258, ahora 281-293), así que la
limpieza corre en **todos** los caminos de salida: parada normal, y muerte de cualquiera de las
dos pistas por separado. Antes solo corría en el camino feliz.

Extendí el mismo aislamiento a los dos `Start()` (antes 198-203, ahora 207-218), que tenían el
mismo defecto (`?` sin aislar pista) señalado como «segunda manifestación, misma causa raíz» en
`REVIEW-codigo.md` B1: si una pista arranca mal (`AUDCLNT_E_DEVICE_IN_USE`, por ejemplo), ahora se
descarta solo esa, y la comprobación de `pistas_a_grabar` se movió a *después* de los dos intentos
de `Start()`, para que también atrape el caso de que las dos se abrieran pero ninguna arrancara.
No estaba en la cita exacta del hallazgo 1 del `REVIEW.md` (que cita 219-225), pero es la misma
familia de bug en el mismo archivo que ya tenía que tocar para el hallazgo 1, así que lo arreglé
también en vez de dejarlo a medias a un lado del arreglo. Si el orquestador prefiere que no lo
hubiera tocado, es un cambio pequeño y reversible.

**No pude verificar por B-1** que esto compila. Verifiqué a mano: los `match` reemplazan cada `?`
uno por uno sin cambiar tipos de retorno; `cerrar_pista(c, fin_ms, &tx)` con `c: &mut
CapturaAbierta` obtenido de `mic.as_mut()` y reasignado a `None` justo después es un patrón
estándar de NLL (el préstamo termina en su último uso, que es la llamada a `cerrar_pista`, antes
de la reasignación) — lo comparé con patrones equivalentes ya existentes en el propio workspace
antes de confiar en él.

### 2. Bloqueante — el relleno contaba dos veces el paquete que reanuda tras un silencio

`core/audio-capture/src/sincronia.rs`, `muestras_de_relleno` (línea 84) y su único call site en
`PistaWasapi::procesar_paquete` (línea 170).

**Qué hice:** añadí un tercer parámetro, `muestras_del_paquete: u64`, y se descuenta también de lo
esperado antes de comparar con `muestras_ya_emitidas`. El call site le pasa `reales.len() as u64`.
Documenté en el doc-comment de la función el porqué (no el qué): sin este descuento, el paquete
que reanuda tras un silencio se contaba una vez como relleno y otra como audio real, porque
`timestamp_ms` se lee *después* de que `GetBuffer` ya entregó ese audio.

**Verificado con Python** (no a mano esta vez), reproduciendo la aritmética exacta de la función,
incluida la saturación:

```
primer paquete (t=0, emitidas=0, reales=1600):    relleno=0,     pcm.len()=1600
segundo paquete (t=2000, emitidas=1600, reales=1600): relleno=28800, pcm.len()=30400
total tras el segundo: 1600 + 30400 = 32000  (= 2000 ms × 16 000 Hz / 1000, exacto)
```

Coincide con la reconstrucción del `revisor-codigo` en `REVIEW-codigo.md` (relleno real 28 800,
no 30 400; total correcto 32 000, no 33 600). El test que codificaba el resultado erróneo
(`un_silencio_en_loopback_no_adelanta_lo_que_viene_despues`) se corrigió en el mismo cambio —ver
hallazgo 5, es el mismo test.

### 3. Bloqueante — `dos_horas_de_muestras_no_desbordan_el_contador` fallaba con código correcto

`core/audio-capture/src/sincronia.rs`, línea 347.

**Qué hice:** seguí la sugerencia del `REVIEW.md` de elegir una base múltiplo de
`SAMPLE_RATE / 1000` (16) en vez de `u32::MAX` a secas, para que el ida y vuelta
milisegundos→muestras→milisegundos no pierda resto. `u32::MAX` es congruente con 15 módulo 16;
`u32::MAX + 1` (2^32) es múltiplo exacto. La base ahora es `u32::MAX as u64 + 1 + 1_000_000`
(sigue muy por encima de `u32::MAX`, que es lo que la prueba necesita proteger). El comentario del
test ahora explica el porqué de esa elección, incluida la cifra exacta del falso rojo anterior
(31 985), para que quede constancia de qué pasaba antes del arreglo — como pide el estilo de este
repositorio para los tests.

**Esta vez sí lo verifiqué con Python, no a mano:**

```python
SAMPLE_RATE = 16000
muestras_previas = (2**32 - 1) + 1 + 1_000_000        # 4 295 967 296
timestamp = ((muestras_previas + 32_000) * 1000) // SAMPLE_RATE
resto_perdido = ((muestras_previas + 32_000) * 1000) % SAMPLE_RATE
# timestamp = 268 499 956 ; resto_perdido = 0  (antes, con u32::MAX puro, el resto perdido era 15)
esperadas = (timestamp * SAMPLE_RATE) // 1000          # = muestras_previas + 32_000, exacto
relleno = esperadas - muestras_previas                  # = 32 000
```

`relleno == 32_000` exacto, sin redondeo de por medio. La afirmación de mi propio HANDOFF de la
primera vuelta («verifiqué la aritmética a mano y no encontré error») era falsa, tal como señaló
`verificador-pruebas`; esta vez el cálculo se hizo con Python antes de escribir el `assert_eq!`,
no después.

### 4. Importante — el helper `Plataforma`/`tiene_backend` era una réplica desconectada del `cfg` real

`core/audio-capture/src/sincronia.rs`: borré el `enum Plataforma` y `fn tiene_backend` (antes
103-121) y el test que los usaba (antes 332-339, `fuera_de_windows_y_linux_sigue_devolviendo_
no_soportada`). Sin otros usos en el archivo (comprobado con `grep`), no quedó código muerto.

En su lugar, `core/audio-capture/src/lib.rs` (línea 395 en adelante) tiene ahora dos tests nuevos
que llaman a las funciones reales del módulo, `iniciar()` (línea 166) y `dispositivos()` (línea
185), gateados por el mismo `#[cfg(not(any(target_os = "linux", target_os = "windows")))]` que la
rama que verifican — el mismo criterio que ya usa
`fuera_de_linux_reproducir_avisa_en_vez_de_no_compilar` para `reproductor_stub.rs`, que el
`REVIEW.md` señaló como modelo a seguir:

```rust
#[test]
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn fuera_de_windows_y_linux_iniciar_devuelve_no_soportada() { ... }

#[test]
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn fuera_de_windows_y_linux_dispositivos_devuelve_no_soportada() { ... }
```

Añadí el segundo (para `dispositivos()`) porque la réplica que reemplazan no distinguía entre las
dos funciones, y quería que el reemplazo no perdiera cobertura conceptual respecto de lo que
borré.

**Desviación menor de la redacción exacta del `REVIEW.md`:** pide llamar a `crate::iniciar(...)`;
los tests llaman a `iniciar(...)` a secas, porque viven en `lib.rs` mismo (con `use super::*;` al
principio del `mod tests`, ya existente), no en `sincronia.rs`. Es la misma función exactamente
—`crate::iniciar` y `iniciar` resuelven al mismo símbolo desde la raíz del crate—, y elegí `lib.rs`
en vez de `sincronia.rs` porque es donde ya vivía el test análogo de `reproductor_stub.rs`, y
porque `lib.rs` es uno de los cinco archivos del PLAN, así que no amplía el conjunto de archivos
tocados.

**Sigue sin ejecutarse en ningún job de CI existente**, y lo digo explícito en el comentario del
test: la matriz actual solo compila para Linux y Windows, así que la rama `NoSoportada` de una
tercera plataforma no compila en ningún job de hoy. Esto ya lo señalaba `auditor-plataforma` (H1,
«qué haría falta» punto 5) como límite estructural, no como algo que este cambio pueda resolver
por sí solo sin tocar el CI (fuera de lo que este hallazgo pedía). La diferencia real frente a la
réplica: en cuanto exista un job que compile para una tercera plataforma, este test protege de
verdad: ejercita `crate::iniciar`/`crate::dispositivos`, no una copia. La réplica nunca lo habría
hecho, ni con ese job.

### 5. Importante — el test de silencio no distinguía el silencio del audio real

`core/audio-capture/src/sincronia.rs`, `un_silencio_en_loopback_no_adelanta_lo_que_viene_despues`
(línea 248).

**Qué hice:** el «audio real» del fixture ahora es `0.5` en vez de `0.0` (bytes construidos con
`(0..1600).flat_map(|_| 0.5_f32.to_le_bytes()).collect()`). Añadí un segundo `assert!` que
comprueba que las últimas 1600 muestras son `0.5` (el audio real), además del que ya comprobaba
que las primeras 28 800 son `0.0` (el relleno). Con esto, una mutación que invierta el orden
—anteponer el audio real y añadir el silencio después— sí pone la prueba en rojo: antes daba el
mismo `pcm` byte a byte (todo ceros) y ningún `assert` lo notaba.

De paso, el test quedó actualizado con la fórmula corregida del hallazgo 2 (28 800 muestras de
relleno y 30 400 de `pcm.len()`, no 30 400 y 32 000): son el mismo cambio, verificado junto con el
hallazgo 2 más arriba.

### 6. Importante — el test del reloj no podía blindar la decisión que decía blindar

`core/audio-capture/src/sincronia.rs`, renombrado de
`el_relleno_no_se_calcula_a_partir_de_las_muestras_recibidas` a
`el_relleno_resta_lo_ya_emitido_del_timestamp_esperado` (línea 297).

**Qué hice:** seguí la segunda opción que ofrecía el hallazgo («reconocés en su comentario qué
protege de verdad y qué no»), no la primera (conectarla a algo real), porque lo real que decide de
dónde sale `timestamp_ms` vive en `wasapi_src.rs`, que por diseño explícito del PLAN no tiene
pruebas propias (no se puede ejercitar sin hardware ni compilador) — no hay ningún sitio real al
que conectar esta prueba sin salirme de los cinco archivos o sin escribir una prueba que en
realidad tampoco correría nunca. Reescribí el comentario para decir con precisión qué verifica
(que `muestras_de_relleno` resta correctamente dado un timestamp) y qué no puede verificar por
construcción (que ese timestamp de verdad venga del reloj monótono y no de las muestras). El
nombre nuevo ya no promete blindar la decisión de diseño, solo describe la resta que prueba.

Añadí además un test separado, `el_relleno_tambien_descuenta_las_muestras_del_propio_paquete`
(línea 325), que aísla con números simples el arreglo del hallazgo 2 (antes esta cobertura vivía
mezclada dentro del test de silencio); y actualicé las dos llamadas de la prueba renombrada al
nuevo tercer parámetro de `muestras_de_relleno` (con `0`, porque lo que prueba esta función no
tiene que ver con las muestras del propio paquete).

### 7. Importante — AC 4 incumplido: `dispositivos()` solo devolvía el par por defecto

`core/audio-capture/src/wasapi_src.rs`, `dispositivos()` (línea 489) y la función nueva
`enumerar_flujo` (línea 520).

**Qué hice, y por qué no lo escalé sin más:** el AC 4 pide «enumera los dispositivos reales del
sistema», en plural. Implementé la enumeración completa con
`IMMDeviceEnumerator::EnumAudioEndpoints` (con la máscara `DEVICE_STATE_ACTIVE`, definida como
constante cruda igual que las demás de este archivo — es un `#define` de `mmdeviceapi.h`, no un
tipo con nombre propio en el SDK de Win32) sobre las dos direcciones, iterando con
`IMMDeviceCollection::GetCount`/`Item`. Cada dispositivo lleva su `id` real (`IMMDevice::GetId`,
mismo patrón ya usado y revisado en `id_de_endpoint_por_defecto`) y se marca `por_defecto` si
coincide con el que devolvería `GetDefaultAudioEndpoint` para esa misma dirección. Un fallo al
enumerar una dirección no vacía la lista de la otra (se registra con `tracing::warn!` y sigue).

**Lo que decidí NO implementar, y por qué, en vez de intentarlo a ciegas:** el nombre "amigable"
real de cada dispositivo (`IPropertyStore::GetValue` con `PKEY_Device_FriendlyName`, leyendo un
`PROPVARIANT`) necesita *features* de Cargo que hoy no están (`Win32_UI_Shell_PropertiesSystem`,
`Win32_Devices_Properties`) y accede a una unión sin tipar
(`PROPVARIANT::Anonymous.Anonymous.Anonymous.pwszVal`). Es, de lejos, la API COM más frágil de
todo este archivo — el propio HANDOFF de la primera vuelta ya la señalaba como el motivo original
para no arriesgarla. Añadir esa superficie sin poder compilarla arriesgaba romper la compilación
de **todo** Windows (no solo dejar la enumeración incompleta), a cambio de un dato secundario: el
AC 4 pide enumerar los dispositivos, no necesariamente nombrarlos bonito, y el selector de
interfaz que usaría esos nombres ya está fuera de alcance de esta HU por el propio PLAN. En su
lugar, cada dispositivo lleva un nombre genérico con índice y si es el predeterminado (p. ej.
`"Micrófono 2"`, `"Salida 1 (predeterminado)"`). Documenté esta decisión en el doc-comment de
`dispositivos()` y la dejo también en «Deuda que dejo» más abajo, explícita, no silenciosa: si el
orquestador o el PO consideran que el AC 4 exige nombres reales, hace falta una pasada más,
específica para eso, con su propio riesgo de COM sin verificar.

**No pude verificar por B-1** que `EnumAudioEndpoints`/`IMMDeviceCollection` tienen exactamente
esta firma en `windows` 0.58. Confianza razonable, no confirmada: el IDL de `mmdeviceapi.idl`
declara `EnumAudioEndpoints(EDataFlow, DWORD, IMMDeviceCollection**)` y
`IMMDeviceCollection::{GetCount(UINT*), Item(UINT, IMMDevice**)}`, y el resto del archivo ya usa
exactamente ese patrón de traducción DWORD→`u32`, out-param→`Result<T>` para las llamadas COM
vecinas (`GetDefaultAudioEndpoint`, ya en uso y ya revisado). No están en el listado de *features*
verificado de `Cargo.toml`: ambos tipos están bajo `Win32_Media_Audio`, la misma *feature* que ya
habilita `IMMDeviceEnumerator`/`IMMDevice`, así que no hace falta tocar `Cargo.toml`.

### 8. Importante — fuga de un `PWSTR` si `to_string()` fallaba en el camino de error

`core/audio-capture/src/wasapi_src.rs`, `id_de_endpoint_por_defecto` (línea 568).

**Qué hice:** separé el resultado de `to_string()` de su propagación: ahora se guarda en
`resultado` (sin `?` todavía), se libera el puntero incondicionalmente con `CoTaskMemFree`, y
*después* se propaga el resultado con `resultado` como valor de retorno de la función. Mismo
patrón que ya usaba `abrir_cliente` para el `WAVEFORMATEX*` de `GetMixFormat` (liberar antes de
propagar el error de `Initialize`). Apliqué el mismo cuidado en el bucle nuevo de
`enumerar_flujo` (línea 542), que tiene la misma llamada a `to_string()` sobre un `PWSTR` de COM.

### 9. Menor — ninguna prueba ejercitaba `esperadas < muestras_ya_emitidas`

`core/audio-capture/src/sincronia.rs`, test nuevo `el_relleno_nunca_es_negativo_si_ya_se_emitio_
de_mas` (línea 337): `muestras_de_relleno(100, 10_000, 0)` — timestamp de 100 ms (1600 muestras
esperadas) contra 10 000 muestras ya emitidas. Sin el `saturating_sub`, esto desbordaría un `u64`
en `debug` (pánico) o envolvería en silencio en `release`; con él, da `0`. Verificado con Python
junto con el resto (ver tabla de la sección siguiente).

## Tercera vuelta — qué se corrigió de cada hallazgo (`REVIEW.md` / `REVIEW-codigo.md`, sección «Segunda vuelta», 2026-09-03)

Encargo acotado a tres hallazgos, los tres en el código que cerró el Bloqueante 1 de la vuelta
anterior (`cerrar_pista` y el bucle que la rodea). No toqué `dispositivos()`, `enumerar_flujo`,
los dos `Start()` aislados ni ningún otro punto ya revisado y aceptado.

### v2-1 (antes `v2-I2` de `revisor-codigo`) — Bloqueante — el hilo se quedaba vivo sin grabar si morían las dos pistas

`core/audio-capture/src/wasapi_src.rs`, bucle principal (antes 232-279, ahora 232-292) y
`core/audio-capture/src/sincronia.rs` (función nueva, línea 129).

**El problema, tal como lo describía el hallazgo:** el bucle aislaba bien el fallo de *una* pista
(la otra seguía, correcto), pero no tenía ningún `break` para el caso de que murieran las *dos*:
seguía sondeando cada 10 ms, sin producir ningún `AudioFrame` más y sin soltar el `Sender`, así
que el canal nunca se cerraba y quien consume la captura no tenía forma de distinguir «no hay
audio ahora mismo» de «la sesión ya terminó de verdad». Una clase de dos horas que perdiera las
dos pistas en el minuto diez se quedaba grabando en silencio los 110 minutos restantes.

**Qué hice:** añadí la comprobación `crate::sincronia::alguna_pista_sigue_viva(mic.is_some(),
sistema.is_some())` al final de cada vuelta del bucle, después de procesar ambas pistas
(`wasapi_src.rs:281`); si da `false`, se registra con `tracing::error!` y se hace `break`
(líneas 282-283). Ese `break` cae en el mismo camino de salida que ya existía para la parada
normal (señal de `rx_parar`): el bloque de limpieza de después del `loop` (líneas 294-306, sin
cambios) intenta `cerrar_pista` en cada pista que siga `Some` — no hace nada en este caso concreto,
porque las dos ya están en `None` y ya se cerraron en el momento de morir, no aquí—, y `bucle()`
retorna `Ok(())`. Como `tx: Sender<AudioFrame>` se recibe **por valor** (línea 160, sin cambios)
y no se clona en ningún punto del archivo (comprobado con `grep -n "tx.clone" wasapi_src.rs`, sin
resultados), al retornar `bucle()` se suelta y el canal se cierra solo, sin ningún `drop(tx)`
manual.

**Por qué no inventé un patrón distinto, siguiendo la pista que se me dio:** miré cómo termina su
bucle `pipewire_src.rs` (`bucle`, líneas 140-191 de ese archivo). Es un mecanismo distinto —un
`mainloop` dirigido por eventos que corre hasta `mainloop.run()` retorna tras `ml.quit()`, no un
sondeo con `loop {}`—, así que no había un `break` literal que copiar línea por línea. Lo que sí
tomé de ahí es la propiedad de fondo que hace que el arreglo funcione: en los dos archivos,
`bucle()` recibe `tx` por valor y lo suelta al retornar, así que "terminar la función limpiamente"
ya es, por construcción del lenguaje, "cerrar el canal" — no hizo falta inventar una señal nueva
ni un tipo nuevo, solo encontrar la condición que faltaba para llegar a ese `return` cuando de
verdad no queda nada que capturar.

**La política —seguir con lo que quede, terminar si no queda nada— vive en `sincronia.rs`, no en
el bucle COM**, siguiendo el precedente explícito de `pistas_a_grabar` que se me pidió seguir:
`alguna_pista_sigue_viva(microfono_vivo: bool, sistema_vivo: bool) -> bool`
(`sincronia.rs:129-131`) es la función que decide, y es la misma que llama `wasapi_src.rs` y la
que ejercitan las tres pruebas nuevas (ver v2-3). En `wasapi_src.rs` solo quedó la parte que habla
con COM: leer `mic.is_some()`/`sistema.is_some()` y decidir si hacer `break`.

**No pude verificar (B-1):** que esto compila, que `tx` de verdad no se clona en ningún sitio que
se me haya escapado al leer (repasé el archivo completo, no solo el bucle), y el comportamiento
real en Windows (no hay forma de forzar que dos `IAudioClient` fallen a la vez sin hardware). El
consumidor real de este canal (`core/api/src/grabacion.rs`, HU-05, fuera de mi encargo) no lo
toqué ni lo releí en esta vuelta — el `revisor-codigo` ya documentó en la segunda vuelta que usa
`recv_timeout` con un `AtomicBool`, lo cual no cambia con este arreglo: sigue siendo su
responsabilidad reaccionar a que el canal se cierre, y eso no es parte de esta HU.

### v2-2 (antes `v2-I1` de `revisor-codigo`) — Importante — `cerrar_pista` se tragaba el error de `vaciar()`

`core/audio-capture/src/wasapi_src.rs`, `cerrar_pista` (antes 307-314, ahora 325-339).

**El problema:** `if let Ok(Some(frame)) = c.pista.vaciar(fin_ms) { ... }` no distinguía `Ok(None)`
(nada pendiente, caso normal) de `Err(_)` (el vaciado del remuestreador falló de verdad): en los
dos casos no hacía nada. Dos funciones más abajo, `procesar_paquetes` sí registra el mismo tipo de
error (`Err(e) => tracing::warn!(error = %e, "fallo de remuestreo, bloque descartado")`,
`wasapi_src.rs:390`) — invariante 5: un fallo no se traga.

**Qué hice:** cambié el `if let` por un `match` de tres brazos, calcado del que ya usa
`procesar_paquetes` para el mismo tipo de resultado (incluido el `Ok(None) => {}` explícito, no
implícito):

```rust
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
```

Añadí también una frase al doc-comment de la función explicando el porqué (líneas 320-324): que
el error se registra y no se descarta, por la misma razón que ya aplica `procesar_paquetes`. El
mensaje de `tracing::warn!` no cabía en una sola línea dentro de 100 columnas (medido con Python:
111 caracteres la variante más corta que probé, ver «Comandos para reproducir»), así que lo partí
en varias líneas con el mismo formato de bloque que ya usa este archivo para llamadas de
`tracing::` largas (por ejemplo `wasapi_src.rs:225-229`, sin cambios en esta vuelta).

**No pude verificar (B-1):** que compila (el `match` sobre `Result<Option<AudioFrame>>` con esos
tres brazos es exhaustivo por lectura, pero eso no lo confirma nadie sin `rustc`), ni el formato
exacto que dejaría `rustfmt` en la llamada partida en varias líneas — razoné el patrón por
analogía con los `tracing::info!`/`tracing::warn!` multilínea que ya existen en el propio archivo
y en el resto del workspace (comprobé varios con `grep`), no lo inventé a ciegas. Tampoco pude
verificar si `Remuestreador::vaciar()` falla alguna vez en la práctica: sigue siendo el mismo
límite que ya señaló `revisor-codigo` en la segunda vuelta (haría falta instrumentar `rubato` o
forzar un estado interno inconsistente).

### v2-3 (antes `v2-I3` de `revisor-codigo`) — Importante — el arreglo del Bloqueante 1 no tenía prueba

`core/audio-capture/src/sincronia.rs`, función nueva `alguna_pista_sigue_viva` (líneas 117-131) y
tres pruebas nuevas (líneas 405-442).

**Qué hice:** extraje la política a una función pura en `sincronia.rs`, el mismo movimiento que ya
existía para `pistas_a_grabar` (la pregunta equivalente, pero al arrancar en vez de a mitad de
sesión). `wasapi_src.rs` no decide nada por sí solo: solo junta `mic.is_some()`/`sistema.is_some()`
y llama a la función real (`wasapi_src.rs:281`, ver v2-1). Tres pruebas, las tres pedidas:

- `una_pista_muerta_no_frena_a_la_otra` (línea 408): `alguna_pista_sigue_viva(true, false)` y
  `(false, true)` dan `true` — una pista sigue, el bucle no debe terminar.
- `sin_ninguna_pista_viva_el_bucle_termina` (línea 419): `alguna_pista_sigue_viva(false, false)`
  da `false` — el caso central del Bloqueante 1.
- `una_sesion_de_una_sola_pista_termina_si_esa_pista_muere` (línea 428): encadena
  `pistas_a_grabar(true, false)` (la decisión real de arranque, ya existente) con
  `alguna_pista_sigue_viva(false, false)` para cubrir el tercer caso pedido explícitamente — una
  sesión que solo pidió una pista, y esa pista muere. El comentario del test es explícito sobre
  por qué esta aserción es numéricamente igual a la del test anterior y por qué igual vale la pena
  como prueba separada: aquí `sistema_vivo` es `false` porque nunca se pidió (no porque fallara a
  mitad de grabación), y encadenar la función real de arranque con la de en-vivo deja constancia
  de que las dos políticas —"con qué arranco" y "cuándo termino"— dan la misma respuesta correcta
  para este caso, sin que nadie tuviera que decidirlo a mano dentro de `wasapi_src.rs`.

**Regla de la réplica, explícita:** la pregunta que exige la regla es *¿qué línea del código de
producción ejecuta esta prueba?* Las tres llaman a `sincronia::alguna_pista_sigue_viva`
directamente, sin prefijo (`use super::*;` del módulo de tests) — es la **misma función**, no una
copia, que llama `wasapi_src.rs:281` para decidir el `break` del bucle real. No escribí ningún
tipo ni ninguna función que solo use la prueba: `alguna_pista_sigue_viva` tiene exactamente un
llamador de producción (`wasapi_src.rs`) y tres llamadores de prueba, verificado con
`grep -rn "alguna_pista_sigue_viva" core/audio-capture/src` (salida completa: la definición, el
comentario que la referencia, el único *call site* de producción, y las tres pruebas — ninguna
definición duplicada).

**No pude verificar (B-1):** que las pruebas compilan ni que pasan — son aserciones sobre una
función `||` de dos booleanos, sin ningún camino oculto, así que el riesgo de que fallen por un
error de tipeo es bajo, pero sigue siendo una afirmación no ejecutada, no un hecho comprobado.

### Verificación transversal de esta vuelta, con Python (no había aritmética que verificar, pero sí estructura)

A diferencia de la segunda vuelta, ningún hallazgo de esta ronda depende de una fórmula numérica
—son control de flujo y manejo de errores—, así que no hay un cálculo equivalente al de
`muestras_de_relleno` que reproducir. Lo que sí verifiqué con Python antes de dar esto por
terminado, sobre los dos archivos completos, no solo los tramos tocados:

```python
import io
for path in ['core/audio-capture/src/wasapi_src.rs', 'core/audio-capture/src/sincronia.rs']:
    with io.open(path, encoding='utf-8') as f:
        s = f.read()
    lines = s.splitlines()
    largas = [i + 1 for i, l in enumerate(lines) if len(l) > 100]
    print(path, 'max=', max(len(l) for l in lines), 'lineas>100=', largas)
    print(path, 'llaves', s.count('{'), s.count('}'), 'parens', s.count('('), s.count(')'))
```

Resultado: `wasapi_src.rs` máximo 98 caracteres, `sincronia.rs` máximo 99, ninguna línea por
encima de 100 en ninguno de los dos (contando caracteres Unicode, no bytes: los dos archivos usan
tildes y `ñ` en varios comentarios nuevos), y las llaves/paréntesis balanceados en los dos
archivos completos: `wasapi_src.rs` con 132 `{` y 132 `}`, 309 `(` y 309 `)`; `sincronia.rs` con
36 `{` y 36 `}`, 178 `(` y 178 `)`. No es una prueba de que el código compila —eso sigue bloqueado
por B-1—, pero sí descarta la clase de error más tonta (una llave o un paréntesis de más o de
menos) antes de
entregar.

También confirmé con `grep` que `sincronia` es un módulo público sin `#[cfg]`
(`lib.rs:14: pub mod sincronia;`), así que `alguna_pista_sigue_viva` compila y sus tres pruebas
corren en el job de Linux del CI igual que el resto de `sincronia.rs` — el mismo argumento que ya
vale para `pistas_a_grabar`, y la razón por la que se me pidió mover la política ahí y no
dejarla en `wasapi_src.rs`, que solo compila con `#[cfg(target_os = "windows")]`
(`lib.rs:20-21`).

## Archivos tocados

Los cinco de la tabla del PLAN, ninguno más. Esta vuelta el encargo ya venía acotado a dos:
`core/audio-capture/src/{wasapi_src.rs, sincronia.rs}`, y son los únicos dos que edité yo en esta
sesión (`Cargo.toml`, `ci.yml` y `lib.rs` no los toqué: ningún hallazgo de esta ronda pedía
cambios ahí). El árbol de trabajo es compartido sin commits entre tareas (`tablero.md`), así que
`git status --short` por sí solo no distingue mis cambios de los de otra tarea en paralelo —lo
uso solo para confirmar que estos dos archivos siguen `??` (nuevos, sin commitear desde la
primera vuelta), no como prueba de que nadie más los tocó ni de que yo no toqué nada más.

| Ruta | Qué se hizo | ¿Estaba en el PLAN? |
|---|---|---|
| `core/audio-capture/Cargo.toml` | Sin cambios en esta vuelta (ronda 1: `windows = "0.58"` bajo `[target.'cfg(windows)'.dependencies]`) | Sí |
| `core/audio-capture/src/sincronia.rs` | Ronda 2: `muestras_de_relleno` con tercer parámetro (hallazgo 2); `dos_horas_de_muestras_no_desbordan_el_contador` con base múltiplo de 16 (hallazgo 3); `Plataforma`/`tiene_backend` borrados (hallazgo 4); test de silencio con valores distinguibles y assert de orden (hallazgo 5); test del reloj renombrado y con comentario honesto, más un test nuevo aislado (hallazgo 6); test nuevo de `saturating_sub` (hallazgo 9). **Ronda 3:** función nueva `alguna_pista_sigue_viva` (v2-3) y tres pruebas que la ejercitan | Sí |
| `core/audio-capture/src/wasapi_src.rs` | Ronda 2: aislamiento por pista en `Start()` y en el bucle de sondeo, función `cerrar_pista` compartida (hallazgo 1); `dispositivos()` reescrito con enumeración completa vía `EnumAudioEndpoints` (hallazgo 7); fuga de `PWSTR` corregida en `id_de_endpoint_por_defecto`, mismo cuidado aplicado en el nuevo `enumerar_flujo` (hallazgo 8). **Ronda 3:** `break` en el bucle cuando `alguna_pista_sigue_viva` da `false` (v2-1); `cerrar_pista` registra el error de `vaciar()` en vez de descartarlo (v2-2) | Sí |
| `core/audio-capture/src/lib.rs` | Ronda 2: dos tests nuevos (`iniciar`/`dispositivos` fuera de Windows y Linux) que reemplazan la cobertura de la réplica borrada (hallazgo 4). Sin cambios en la ronda 3 | Sí |
| `.github/workflows/ci.yml` | Sin cambios en esta vuelta (ronda 1: paso `tests de dictar-audio` en el job `nucleo-windows`) | Sí |

No se tocó ningún otro archivo. En particular, no `dispositivos()`/`enumerar_flujo` ni los dos
`Start()` aislados dentro de `wasapi_src.rs` (ya cerrados y aceptados, fuera del encargo de esta
vuelta), no `mezcla.rs`, `wav.rs`, `reproductor*.rs` ni `core/api`, ni nada de `core/providers` /
`core/api/src/puente.rs` / `app/lib/datos/repositorio_rust.dart` / `app/lib/pantallas/` /
`app/lib/ventana.dart` (HU-05 y trabajo del orquestador, fuera de este encargo).

## Comandos para reproducir

**Tercera vuelta, mismo resultado otra vez: no pude ejecutar ninguno.** Reverifiqué de primera
mano antes de escribir este documento, no de las vueltas anteriores:

```
$ cargo --version
/usr/bin/bash: line 3: cargo: command not found   (exit 127)

$ rustc --version
/usr/bin/bash: line 4: rustc: command not found   (exit 127)

$ cargo fmt --version
/usr/bin/bash: line 5: cargo: command not found   (exit 127)
```

B-1 sigue activo, sin cambios respecto de las vueltas anteriores. Esta ronda no tenía aritmética
nueva que verificar (los tres hallazgos son de control de flujo y manejo de errores, no de
cálculo), así que no hay un script de Python nuevo equivalente al de la ronda 2 que reproducir
aquí; el detalle de qué sí verifiqué con Python esta vez (ancho de línea, balance de llaves y
paréntesis) está en la sección «Tercera vuelta» de arriba, junto al hallazgo al que corresponde.
El script de la ronda 2 sigue siendo válido para lo que prueba (no toqué `muestras_de_relleno` ni
ninguna de sus pruebas en esta vuelta) y se deja tal cual, para que quien lo lea pueda seguir
reproduciéndolo:

```python
SAMPLE_RATE = 16000

def sat_sub(a, b):
    return max(a - b, 0)

def muestras_de_relleno(timestamp_ms, muestras_ya_emitidas, muestras_del_paquete):
    t = max(timestamp_ms, 0)
    esperadas = (t * SAMPLE_RATE) // 1000
    return sat_sub(sat_sub(esperadas, muestras_ya_emitidas), muestras_del_paquete)

# el_relleno_resta_lo_ya_emitido_del_timestamp_esperado
assert muestras_de_relleno(100, 1600, 0) == 0
assert muestras_de_relleno(2000, 1600, 0) == 30400

# el_relleno_tambien_descuenta_las_muestras_del_propio_paquete
assert muestras_de_relleno(2000, 1600, 1600) == 28800

# el_relleno_nunca_es_negativo_si_ya_se_emitio_de_mas
assert muestras_de_relleno(100, 10_000, 0) == 0

# dos_horas_de_muestras_no_desbordan_el_contador
muestras_previas = (2**32 - 1) + 1 + 1_000_000
timestamp = ((muestras_previas + 32_000) * 1000) // SAMPLE_RATE
assert ((muestras_previas + 32_000) * 1000) % SAMPLE_RATE == 0   # ida y vuelta exacto
assert muestras_de_relleno(timestamp, muestras_previas, 0) == 32_000

# un_silencio_en_loopback_no_adelanta_lo_que_viene_despues (a través de PistaWasapi)
emitidas = 0
relleno1 = muestras_de_relleno(0, emitidas, 1600); pcm1 = relleno1 + 1600; emitidas += pcm1
assert pcm1 == 1600
relleno2 = muestras_de_relleno(2000, emitidas, 1600); pcm2 = relleno2 + 1600
assert relleno2 == 28800 and pcm2 == 30400
assert emitidas + pcm2 == 32000  # == 2000 ms * 16000 Hz / 1000, exacto

print("todo OK")
```

Los comandos que el revisor debería correr, en este orden, en cuanto haya toolchain (sin cambios
respecto de las vueltas anteriores):

```
cargo fmt --all -- --check
cargo check -p dictar-audio --target x86_64-pc-windows-msvc
cargo clippy -p dictar-audio --all-targets -- -D warnings
cargo test -p dictar-audio
```

## Criterios de aceptación

De `docs/06-historias-de-usuario.md#hu-01`. El PLAN ya anota que 1, 2, 5 y 6 no son comprobables
en este entorno.

| AC | Prueba que lo cubre | Estado |
|---|---|---|
| 1. `iniciar()` en Windows devuelve un `Receiver` real | Ninguna (no comprobable sin compilador ni hardware) | Implementado; **no verificado**. Ronda 2 aísla el fallo de arranque por pista (hallazgo 1); **ronda 3** cierra el hueco que quedaba: si mueren todas las pistas en vivo, el `Receiver` ahora sí deja de entregar y su canal se cierra (v2-1), en vez de quedar abierto e inerte indefinidamente |
| 2. Dos pistas separadas, *loopback* con `AUDCLNT_STREAMFLAGS_LOOPBACK` sobre la salida por defecto | `sin_dispositivo_de_salida_se_graba_solo_el_microfono`, `sin_ninguna_pista_disponible_es_un_error_explicito` cubren la política de arranque; **ronda 3** añade `una_pista_muerta_no_frena_a_la_otra`, `sin_ninguna_pista_viva_el_bucle_termina` y `una_sesion_de_una_sola_pista_termina_si_esa_pista_muere` para la política equivalente en vivo | Implementado; política de arranque y de en-vivo probadas, apertura real **no verificada** |
| 3. 16 kHz mono `f32`, `timestamp_ms` de reloj monótono común | 13 pruebas en `sincronia.rs` (incluida la aritmética del relleno, corregida y verificada con Python) | Implementado y probado en la parte pura; el reloj real (`wasapi_src.rs`) **no verificado**. La fórmula de relleno ya no cuenta dos veces el paquete que reanuda (hallazgo 2); sin cambios en esta ronda |
| 4. `dispositivos()` enumera los dispositivos reales del sistema | Ninguna (no comprobable sin hardware) | Implementado desde la ronda 2: `EnumAudioEndpoints` sobre las dos direcciones, no solo el par por defecto (hallazgo 7). Deuda explícita: nombres genéricos, no el nombre "amigable" real (ver «Deuda que dejo»). Sin cambios en esta ronda |
| 5. Al soltar la `CaptureSession` la captura se detiene y los hilos terminan | Ninguna (no comprobable sin hardware) | Implementado; **no verificado**. Distinto del AC 1: esto es sobre `detener()` explícito, que esta ronda no tocó — el hilo también termina solo cuando ya no queda ninguna pista viva (v2-1), pero es una vía adicional, no un cambio de esta vía |
| 6. Deriva imperceptible en 30 min | Ninguna (no comprobable sin hardware) | **No verificable en este entorno**. La corrección del hallazgo 2 (ronda 2) reduce el desplazamiento que introducía cada transición silencio→audio del loopback; sin cambios en esta ronda |
| 7. Al menos un test que no necesite tarjeta de sonido | 13 pruebas en `sincronia.rs`, verificado contando con `grep -c "#\[test\]"` (10 al empezar esta vuelta, +3 por v2-3) | Cumplido. Las tres nuevas cubren, con la función real que usa `wasapi_src.rs` (no una réplica): una pista muere y la otra sigue, mueren las dos y el bucle debe terminar, y una sesión de una sola pista que muere |

## Invariantes del producto

| Invariante | Cómo se respeta acá |
|---|---|
| 1. Se escribe a disco antes de procesar | Sin cambio de contrato (esta HU no toca `EscritorPistas`). Ronda 2 corrigió el caso que sí lo rompía en la práctica: un error COM en una pista ya no mata el hilo entero sin vaciar el remuestreador de la otra (hallazgo 1). **Ronda 3** cierra el hueco que quedaba en el mismo invariante desde otro ángulo: si mueren las dos pistas, el bucle ahora hace `break` en vez de seguir vivo sin producir nada (v2-1) — no perdía audio ya escrito (eso ya estaba resuelto), pero sí dejaba la sesión "grabando" en apariencia sin comunicar que ya no capturaba, que es la variedad silenciosa de pérdida que este proyecto declara inaceptable |
| 2. Dos pistas, nunca una mezcla | Sin cambios: cada pista sigue teniendo su propio `CapturaAbierta`/`PistaWasapi`, nunca se suman antes de emitir |
| 3. Todo el pipeline a 16 kHz mono `f32` | Sin cambios en el pipeline de conversión. La corrección del hallazgo 2 (ronda 2) es sobre *cuándo* se inserta silencio, no sobre la frecuencia ni el formato |
| 4. Ninguna firma pública cambia según la plataforma | Sin cambios en las firmas. La prueba que blinda esto ejercita el `#[cfg]` real de `lib.rs` en vez de una réplica (hallazgo 4, ronda 2). `alguna_pista_sigue_viva` (ronda 3) tampoco lleva `#[cfg]`: es lógica pura en `sincronia.rs`, igual que `pistas_a_grabar` |
| 5. Un fallo no se traga | **Esta vuelta lo toca directamente (v2-2):** `cerrar_pista` distinguía antes `Ok(Some(_))` de "todo lo demás" con un solo `if let`, así que un `Err` de `Remuestreador::vaciar()` se descartaba en silencio, igual que un `Ok(None)` normal. Ahora es un `match` de tres brazos, calcado del que ya usa `procesar_paquetes` para el mismo tipo de fallo dos funciones más abajo: el `Err` se registra con `tracing::warn!`, no se pierde sin dejar rastro |

## Decisiones que se apartan del PLAN

- **`dispositivos()` enumera todos los dispositivos activos (hallazgo 7 corregido), pero sin
  nombre "amigable" real.** Ya no es la desviación completa de la primera vuelta (que devolvía
  solo el par por defecto): ahora usa `EnumAudioEndpoints` para listar todos los micrófonos y
  salidas activos, con su `id` real y marcando cuál es el predeterminado. Lo que sigue sin
  implementar es el nombre visible real de cada uno (`IPropertyStore`/`PROPVARIANT`): use en su
  lugar `"Micrófono N"` / `"Salida N"` con sufijo `(predeterminado)` cuando corresponda. Razón
  igual que en la primera vuelta: es la API COM más frágil de todo el archivo (unión sin tipar,
  *features* de Cargo nuevas sin verificar), y esta vez sí implementé la parte que podía hacer con
  confianza razonable en vez de declarar la función entera como fuera de alcance. Si el AC 4
  exige nombres reales y no solo la lista, hace falta una pasada más, explícita, para eso.
- **El test que blinda el invariante 4 vive en `lib.rs`, no en `sincronia.rs`**, y llama a
  `iniciar`/`dispositivos` sin el prefijo `crate::` (redundante desde dentro de `lib.rs` mismo,
  con `use super::*;`). Ver el detalle en el hallazgo 4 de la sección de arriba.
- Las dos decisiones que ya declaraba la primera vuelta y siguen igual: **`cargo test` en el CI de
  Windows acotado a `-p dictar-audio`** (el `auditor-plataforma` confirmó que ningún otro crate del
  workspace tiene código gateado a `cfg(windows)`, así que el alcance completo no ganaría
  cobertura), y el resto de deudas ya anotadas por el propio PLAN (`pipewire_src` sin `vaciar()`,
  cambio de dispositivo por defecto a mitad de sesión sin soportar, comentario de `release.yml`).
- **Ninguna desviación nueva en la tercera vuelta.** El encargo llegó ya acotado a tres hallazgos
  puntuales (v2-1, v2-2, v2-3) sobre código que ya estaba dentro de los dos archivos declarados;
  no hizo falta tocar ningún archivo fuera de esa lista ni ampliar el alcance de ningún hallazgo,
  a diferencia de la ronda 2 (que sí extendió el aislamiento a los dos `Start()`, declarado y
  aceptado en su momento). Yo mismo edité solo `wasapi_src.rs` y `sincronia.rs` en esta sesión —de
  eso sí puedo responder—; `git status --short` de todas formas muestra bastantes más archivos con
  cambios (`core/api/src/puente.rs`, `app/lib/pantallas/ajustes.dart`, `core/providers/*`, etc.),
  pero son del árbol compartido sin commits entre tareas (`tablero.md`), consistentes con HU-05 y
  otro trabajo en paralelo, no algo que yo haya tocado. `git status` no distingue "de esta sesión"
  de "de otra tarea en el mismo árbol", así que esto es una declaración de qué edité yo, no una
  lectura literal de esa salida.

## Lo que NO pude verificar

Sección larga a propósito, igual que en las dos vueltas anteriores; el entorno no cambió.

**Compilación, formato y pruebas — nada, en absoluto, otra vez.** Reverificado de primera mano en
esta misma sesión (ver «Comandos para reproducir»): `cargo`, `rustc` y `cargo fmt` siguen sin
existir en esta máquina. Toda esta tercera vuelta es lectura y razonamiento del flujo de control,
sin aritmética nueva que verificar (a diferencia de la ronda 2); lo que sí verifiqué con Python
fue lo estructural (ancho de línea, balance de llaves), no un cálculo — el detalle está en la
sección «Tercera vuelta».

**Que el `match` nuevo de `cerrar_pista` y la función `alguna_pista_sigue_viva` compilan.** Los
revisé por lectura contra tipos que ya conozco de este mismo archivo (`Result<Option<AudioFrame>>`
de `PistaWasapi::vaciar`, ya en uso; dos parámetros `bool` y un `||`, sin ambigüedad de tipos
posible), y calqué el `match` de tres brazos del que ya usa `procesar_paquetes` para el mismo tipo
de resultado — pero "se parece a código que ya compila" no es lo mismo que "compila", y sigue sin
haber forma de confirmarlo sin `rustc`.

**El formato exacto que dejaría `rustfmt` en la llamada partida de `tracing::warn!` dentro de
`cerrar_pista`** (`wasapi_src.rs:331-334`). La escribí con el mismo patrón de bloque que ya usan
otras llamadas de `tracing::` multilínea en este archivo y en el resto del workspace (comprobé
varias con `grep`), pero es la primera vez en este archivo que ese patrón aparece **dentro de un
brazo de `match` sin llaves** (`Err(e) => tracing::warn!(...)`, no `Err(e) => { ... }`); no hay un
precedente idéntico en el repositorio contra el que comparar carácter por carácter, así que no
puedo asegurar que `rustfmt` no la reindente de otra forma.

**El comportamiento real, en Windows, de que el canal se cierre cuando mueren las dos pistas
(v2-1).** Razoné el flujo de control a mano —que `tx` se recibe por valor y no se clona en ningún
punto del archivo, que `bucle()` retorna tras el nuevo `break`, que el retorno suelta `tx`—, pero
no hay forma de disparar dos `AUDCLNT_E_DEVICE_INVALIDATED` (o cualquier otro error COM) a la vez
sin hardware y sin dos dispositivos que desconectar durante una captura activa. Tampoco pude
comprobar en la práctica cómo reacciona el consumidor real de ese canal
(`core/api/src/grabacion.rs`, HU-05, fuera de mi encargo): no lo toqué ni lo releí en esta vuelta,
más allá de lo que ya documentó `revisor-codigo` en la segunda vuelta (usa `recv_timeout` con un
`AtomicBool`, lo cual debería seguir funcionando sin cambios, pero "debería" no es "verifiqué").

**Si `Remuestreador::vaciar()` falla alguna vez en la práctica (v2-2).** El camino de error ahora
se registra en vez de descartarse, pero sigue sin haber forma de forzar ese fallo sin instrumentar
`rubato::FastFixedIn` o compilar y provocar un estado interno inconsistente — mismo límite que ya
señaló `revisor-codigo` en la segunda vuelta, sin cambios.

**La API de `windows` 0.58 sigue sin poder verificarse contra su código fuente.** Ronda 2 añade
superficie nueva sin verificar en `wasapi_src.rs::enumerar_flujo`: `IMMDeviceEnumerator::
EnumAudioEndpoints`, `IMMDeviceCollection::GetCount`/`Item`, y la constante cruda
`DEVICE_STATE_ACTIVE`. Razoné su firma a partir del IDL de `mmdeviceapi.idl` (`EDataFlow, DWORD →
IMMDeviceCollection**`, `UINT* / UINT, IMMDevice**`) y del patrón ya usado —y ya revisado— en
`GetDefaultAudioEndpoint` para el resto del archivo, pero **no está confirmado con un
compilador**. Es la pieza de menos confianza de esta vuelta; si el revisor va a mirar algo con
`cargo check -p dictar-audio --target x86_64-pc-windows-msvc` primero, que sea esto y no el resto
del archivo (ya revisado en la primera vuelta).

**Formato exacto de `rustfmt` en los bloques nuevos, en particular `dispositivos()`.** Los `let
resultado_mic = unsafe { ... };` que rompen en tres líneas los escribí calcando el patrón que ya
usa `abrir_cliente` para `resultado_init` (mismo archivo, sin tocar en esta vuelta), pero no hay
forma de confirmar que `rustfmt` no reacomode algo distinto sin correrlo.

**No hay Windows con tarjeta de sonido para probar la captura real.** Sin cambios respecto de la
primera vuelta: los AC 1, 2, 3 (la parte de `wasapi_src`), 4 (que la enumeración nueva realmente
devuelva más de un dispositivo cuando los haya), 5 y 6 siguen sin poder probarse aquí.

**El aislamiento por pista del hallazgo 1, en un Windows real.** Razoné el flujo de control a
mano (qué pasa si `mic` falla y `sistema` sigue, y viceversa, y si los dos fallan) pero no hay
forma de disparar un `AUDCLNT_E_DEVICE_INVALIDATED` de verdad sin hardware y un dispositivo que
desconectar a mitad de captura.

## Deuda que dejo

- **`dispositivos()` en Windows enumera todos los dispositivos activos (ya no solo el par por
  defecto), pero con nombre genérico, no el nombre "amigable" real de Windows.** Ver «Decisiones
  que se apartan del PLAN». Si hace falta el nombre real para el AC 4 tal como está escrito, es
  trabajo pendiente explícito, con su propio riesgo de COM sin verificar
  (`IPropertyStore`/`PROPVARIANT`).
- **`wasapi_src.rs` sigue sin ninguna prueba propia**, por diseño explícito del PLAN — sigue siendo
  el punto ciego real de esta entrega. Esta vuelta reduce, sin eliminar, ese punto ciego para el
  caso de v2-1/v2-3: la *política* de cuándo termina la sesión en vivo ya no vive sin pruebas
  dentro del bucle COM, se extrajo a `sincronia::alguna_pista_sigue_viva` y sí tiene tres pruebas.
  Lo que sigue sin ninguna cobertura es la parte que queda en `wasapi_src.rs`: que `mic.is_some()`/
  `sistema.is_some()` reflejen de verdad el estado tras cada `procesar_paquetes`, que el `break`
  se ejecute, y que `tx` se suelte al retornar — todo eso sigue siendo control de flujo de COM sin
  ejercitar.
- **El cierre del canal (v2-1) no lleva ninguna señal explícita que distinga "terminó porque ya no
  queda ninguna pista" de "terminó porque se llamó a `detener()`".** Las dos cierran el mismo
  `Receiver<AudioFrame>` de la misma forma —dejando de entregar `AudioFrame`s—, así que un
  consumidor no tiene manera de saber, solo mirando el canal, si la sesión se cortó sola o si el
  usuario la paró a propósito. `core/api/src/grabacion.rs` (HU-05, fuera de mi encargo) ya
  sobrevive a esto con `recv_timeout`, pero no podría, hoy, mostrarle al usuario un aviso del tipo
  «la grabación se cortó sola en el minuto 40» en vez de simplemente terminar el WAV en silencio
  en ese punto. Si se quiere esa distinción, hace falta una señal más rica que «el `Receiver` dejó
  de entregar» — por ejemplo, un evento o un campo en el resultado de `detener()` —, y es trabajo
  de otra HU, no de esta vuelta acotada a los tres hallazgos.
- El test que blinda el invariante 4 (`fuera_de_windows_y_linux_iniciar_devuelve_no_soportada` y
  su par de `dispositivos`) **no corre en ningún job de CI existente**: la matriz actual solo
  compila para Linux y Windows. Deja de ser una réplica desconectada (hallazgo 4 resuelto), pero
  sigue siendo una prueba que hoy nadie ejecuta. Si se quiere cobertura real de esa rama, hace
  falta un job de CI que compile (con `cargo check`, sin necesidad de runner) para un tercer
  `--target`, tal como apuntaba `auditor-plataforma` en su sección «Qué haría falta».
- Las deudas que el PLAN ya declaraba abiertas a propósito siguen igual: `pipewire_src` sin
  `vaciar()` al cerrar, el cambio de dispositivo por defecto a mitad de sesión sin soportar en
  ninguna plataforma, y el comentario de `release.yml` que queda desactualizado al cerrar esta HU
  (no está en la lista de archivos de esta tarea).
