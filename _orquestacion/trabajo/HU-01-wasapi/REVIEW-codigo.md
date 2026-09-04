# REVIEW-codigo — HU-01 «Grabar en Windows (WASAPI loopback)»

Revisor: `revisor-codigo`. Revisión estática sobre `core/audio-capture/src/sincronia.rs` (345
líneas, nuevo), `core/audio-capture/src/wasapi_src.rs` (483 líneas, nuevo, 33 bloques `unsafe`),
la parte de `core/audio-capture/src/lib.rs` que declara y enruta a estos módulos,
`core/audio-capture/Cargo.toml` y `.github/workflows/ci.yml`. El `HANDOFF.md` se trata como
declaración a verificar, no como evidencia.

## 1. Veredicto

**NO VERIFICABLE por B-1 (no hay `cargo`/`rustc` en esta máquina; repetí la comprobación de
HANDOFF, mismo resultado) en todo lo que depende de compilar, enlazar, `cargo fmt` o `cargo
clippy -D warnings`. En revisión estática: 1 hallazgo Bloqueante y 3 Importantes, con escenario
de fallo concreto cada uno.**

El hallazgo Bloqueante contradice directamente una decisión del propio `PLAN.md` que estaba
marcada como cubierta («El dispositivo por defecto puede cambiar...»), y el primer Importante
corrige a la baja la propia minimización que hace el `HANDOFF.md` de un defecto que él mismo
confiesa («relleno de silencio... imprecisión menor»): con los números de su propio test, no es
menor y no está limitado al primer paquete de la sesión.

## 2. Hallazgos

### Bloqueante

**B1 — Un fallo COM transitorio en una sola pista termina la grabación completa, sin volcar el
remuestreador, contradiciendo la degradación por pista que el PLAN exige explícitamente.**

`core/audio-capture/src/wasapi_src.rs:219-225` (bucle principal):

```rust
if let Some(c) = mic.as_mut() {
    hubo_datos |= procesar_paquetes(c, inicio, &tx)?;
}
if let Some(c) = sistema.as_mut() {
    hubo_datos |= procesar_paquetes(c, inicio, &tx)?;
}
```

`procesar_paquetes` (líneas 264-315) propaga con `?` cualquier error de `GetNextPacketSize`
(272), `GetBuffer` (283-285) o `ReleaseBuffer` (303). El `?` de las líneas 221 y 224 hace que
cualquiera de esos errores en cualquiera de las dos pistas retorne inmediatamente de `bucle()`
con `Err(...)`, saltándose por completo el bloque de limpieza de las líneas 235-258 —que es el
único sitio donde se llama `pista.vaciar()` y `cliente.Stop()`—, porque ese bloque solo es
alcanzable rompiendo el `loop` por la vía normal (señal de `rx_parar`).

Qué debería pasar, según el propio PLAN (sección «Casos del dominio», marcado con `[x]`):
«El dispositivo por defecto puede cambiar a mitad de clase. [...] Si el dispositivo desaparece,
la pista termina y se registra en el log; la otra sigue.» El comentario de las líneas 166-168 de
este mismo archivo promete lo mismo: «Cada pista se abre por separado y un fallo en una no
aborta la otra.» El código no hace eso: aborta las dos.

**Escenario concreto:** en el minuto 40 de una clase de dos horas, el usuario desconecta los
auriculares USB que eran la salida por defecto (exactamente el caso que el PLAN anticipa). El
`IAudioCaptureClient` de loopback empieza a devolver un HRESULT de error en la siguiente llamada
a `GetBuffer` o `GetNextPacketSize` (por ejemplo `AUDCLNT_E_DEVICE_INVALIDATED`, documentado por
Microsoft para este caso). Ese error sube por `?` hasta matar `bucle()` entero: el micrófono, que
seguía funcionando con normalidad, deja de grabar también, sin volcar sus últimas muestras
pendientes (hasta ~21-64 ms según el estado del `Remuestreador`, ver `mezcla.rs:103-118`), y el
resto de la clase —80 minutos— no se captura en ninguna pista. Lo único que queda es un
`tracing::error!` en el cierre del hilo (línea 77 de `iniciar`); nada en la interfaz ni en el
archivo resultante distingue esto de un cierre limpio.

**Segunda manifestación, misma causa raíz:** líneas 198-203, los `Start()` de `mic` y `sistema`
también usan `?` sin aislar la pista. Si `sistema` se abrió bien (`abrir_cliente` tuvo éxito)
pero su `Start()` falla (por ejemplo `AUDCLNT_E_DEVICE_IN_USE`, otro proceso con acceso
exclusivo), la sesión completa falla al arrancar en vez de arrancar solo con el micrófono. Este
caso es menos grave que el anterior porque sí se comunica como un `Err` explícito al llamador de
`iniciar()` (vía `tx_listo`, líneas 89-97) antes de empezar a grabar nada — no hay pérdida de
audio ya capturado —, pero sigue sin cumplir «la otra sigue».

**Por qué no es un defecto de `pipewire_src.rs` replicado:** en PipeWire cada pista es un
stream/callback independiente registrado en el mismo `mainloop` (`pipewire_src.rs:267-319`); un
error dentro del callback `process` de una pista simplemente hace `return;` de ese callback
(líneas 272, 275, 277, 298, etc.), sin afectar al `mainloop` ni al otro stream. El aislamiento
por pista es una propiedad casi gratuita de esa arquitectura. `wasapi_src.rs` sondea las dos
pistas en un único bucle compartido (líneas 212-233): la propiedad no se replicó al traducir el
patrón, y es precisamente el caso que la HU pide cubrir explícitamente en su lista de «Casos del
dominio».

Relacionado y sin cobertura: el AC 7 de `docs/06-historias-de-usuario.md#hu-01` menciona como
ejemplo de test sin hardware «el manejo del cambio de dispositivo por defecto»; ninguna de las 9
pruebas de `sincronia.rs` ejercita una pista que falla a mitad de sesión (las dos que tocan
`pistas_a_grabar` solo cubren la ausencia de dispositivo al arrancar). Es el hueco de prueba que
habría hecho tropezar con este hallazgo.

### Importante

**I1 — La fórmula de relleno de silencio cuenta dos veces las muestras del paquete que la
dispara. No es «una imprecisión menor en el primer paquete de cada pista» como dice el HANDOFF:
se repite en cada reanudación tras un silencio real del loopback.**

`core/audio-capture/src/sincronia.rs:167-191` (`procesar_paquete`):

```rust
let relleno = muestras_de_relleno(timestamp_ms, self.muestras_emitidas);
...
let mut pcm = vec![0.0_f32; relleno as usize];
pcm.extend_from_slice(&reales);
self.muestras_emitidas += pcm.len() as u64;
```

`timestamp_ms` se lee en el instante de recepción del paquete (`wasapi_src.rs:289`, tras
`GetBuffer`), es decir, después de que el propio paquete ya contiene su porción de audio real.
`muestras_de_relleno` (líneas 76-79) calcula `esperadas = timestamp_ms * SAMPLE_RATE / 1000` y
resta `muestras_ya_emitidas` — pero no resta las muestras reales de este mismo paquete
(`reales.len()`) antes de comparar. El resultado: el relleno insertado cubre el hueco de
silencio más la duración del propio paquete que se está procesando, que se añade después sin
descontarse.

**Verificación con los números del propio test** (`sincronia.rs:252-283`,
`un_silencio_en_loopback_no_adelanta_lo_que_viene_despues`): primer paquete, 1600 muestras
reales (100 ms) a `timestamp_ms=0` → `muestras_emitidas` pasa a 1600. Segundo paquete, otras
1600 muestras reales, recibido a `timestamp_ms=2000`. El código calcula
`relleno = muestras_de_relleno(2000, 1600) = 32000 - 1600 = 30400`, y el test asegura
`segundo.pcm.len() == 30400 + 1600 == 32000` (línea 281) — exactamente lo que produce el código,
así que el test no puede detectar el defecto: codifica el resultado erróneo como esperado. Pero
la posición correcta del audio real de este segundo paquete —si de verdad llegó «prácticamente
ahora» tras 1,9 s de silencio real— es en las muestras `[28800, 30400)`, no en `[30400, 32000)`:
el silencio genuino dura 1800 ms = 28800 muestras (de 100 ms a 1900 ms), no 1900 ms. El total
acumulado tras este segundo paquete queda en `muestras_emitidas = 1600 + 32000 = 33600`, cuando
el total correcto para `timestamp_ms=2000` es `32000` (2000 ms × 16 000 Hz / 1000). El
excedente, 1600 muestras = 100 ms, es exactamente `reales.len()` del segundo paquete: la firma
del defecto que predije analíticamente antes de mirar el test.

**Por qué no queda acotado al primer paquete de la sesión, como sostiene el HANDOFF:** para el
micrófono (WASAPI entrega paquetes de forma continua, sin huecos) el exceso se inyecta una sola
vez al arrancar y luego se autolimita (`relleno` satura en 0 mientras `muestras_emitidas` siga
por delante de lo esperado). Pero la pista de sistema —el loopback, la razón de ser de toda esta
sección del PLAN— solo entrega paquetes cuando hay audio real; cada vez que el sistema calla y
vuelve a sonar (una diapositiva sin audio seguida de un vídeo, algo habitual en una clase de dos
horas), se repite exactamente el mismo cálculo con el mismo defecto: el segmento de audio real
que reanuda tras el silencio queda desplazado hasta la duración de un paquete (acotado, no crece
indefinidamente en una única reanudación, pero se repite en cada reanudación). El efecto no se
acumula sin límite entre eventos separados (lo comprobé simbólicamente: el excedente de un
evento se «absorbe» en el siguiente cálculo de relleno vía `saturating_sub`), pero sí introduce,
en cada transición silencio→audio del sistema, un desplazamiento de hasta la duración de un
paquete entre `mic.wav` y `system.wav` en ese punto — el síntoma que la «decisión del reloj» del
PLAN dice existir para evitar.

Arreglo apuntado, no aplicado por mí (no me corresponde): calcular `relleno` contra
`muestras_ya_emitidas + reales.len()` (o, equivalentemente, usar el timestamp de inicio del
paquete, no el de recepción) antes de anteponerlo a `reales`.

**I2 — `dispositivos()` en Windows no enumera «los dispositivos reales del sistema» (AC 4 de la
HU); solo devuelve el par por defecto.**

`core/audio-capture/src/wasapi_src.rs:418-447`. Confirmado independientemente del HANDOFF (que
ya lo confiesa): la función llama dos veces a `GetDefaultAudioEndpoint`
(`id_de_endpoint_por_defecto`, líneas 449-459) y no usa `IMMDeviceEnumerator::EnumAudioEndpoints`
en ningún punto del archivo (comprobado con `grep -n "EnumAudioEndpoints"`, sin resultados). El
AC 4 de `docs/06-historias-de-usuario.md#hu-01` dice, sin matices: «`dictar_audio::dispositivos()`
enumera los dispositivos reales del sistema.» La sección «Fuera de alcance a propósito» del PLAN
solo excluye explícitamente «el selector de dispositivo en la interfaz», no la capacidad de
enumeración en sí — son cosas distintas (una es UI, la otra es lo que el AC 4 pide de la función
Rust). No hay ninguna decisión del orquestador en el PLAN que rebaje el AC 4 a «solo el par por
defecto»; es una desviación real, ya declarada por el implementador, que sigue sin decisión.

Nota de contexto, no atenuante: `pipewire_src::dispositivos()` (Linux) tiene la misma limitación
desde antes de esta HU, así que no es una regresión de Windows frente a Linux — pero tampoco la
excusa, porque el AC 4 se está cerrando ahora mismo con esta historia.

**I3 — Fuga de un búfer COM (`PWSTR`) si `to_string()` falla en el camino de error de
`id_de_endpoint_por_defecto`.**

`core/audio-capture/src/wasapi_src.rs:455-457`:

```rust
let puntero = unsafe { dispositivo.GetId() }.map_err(err)?;
let id = unsafe { puntero.to_string() }.map_err(err)?;
unsafe { CoTaskMemFree(Some(puntero.0 as *const c_void)) };
```

`IMMDevice::GetId()` reserva la cadena; quien la pide es responsable de liberarla con
`CoTaskMemFree`, y aquí se hace correctamente si `to_string()` tiene éxito. Pero si falla (la
documentación de `PWSTR::to_string()` contempla el caso de una secuencia UTF-16 inválida), el `?`
de la línea 456 retorna antes de llegar a la línea 457: el búfer que reservó COM nunca se libera.
Impacto real bajo —los ID de dispositivo de Windows son cadenas bien formadas en la práctica, así
que el disparador es infrecuente, y el efecto es una fuga de una única cadena, no un bucle
caliente—, pero es exactamente el patrón que se pidió comprobar («que cada puntero COM se
libere... en todos los caminos de salida») y el único de los 33 bloques `unsafe` donde encontré
ese patrón roto; el resto de los caminos de `CoTaskMemFree` (líneas 335-339, 356, para el
`WAVEFORMATEX*` de `GetMixFormat`) sí cubren el camino de error.

### Menor

Ninguno adicional, más allá de lo ya cubierto arriba con evidencia suficiente para Importante.

### Nota

**N1 — Asignación de memoria en el bucle de captura** (`wasapi_src.rs:296`, `vec![0u8;
tam_bytes]` para paquetes marcados `AUDCLNT_BUFFERFLAGS_SILENT`; y `sincronia.rs:51-66`,
`normalizar_a_f32` hace `.collect()` en cada paquete). Es una asignación en el camino caliente,
pero no es una regresión de esta HU: `pipewire_src.rs:281-284` asigna un `Vec<f32>` nuevo en cada
invocación de su callback `process` con el mismo patrón. Ambos hilos son de sondeo/callback, no
el hilo de tiempo real del motor de audio de Windows, así que el coste es tolerable en ese
contexto. Se anota, no se exige corrección.

**N2 — `Plataforma`/`tiene_backend`** (`sincronia.rs:103-121`) es una política duplicada de la
que usa `lib.rs::iniciar`/`dispositivos` vía `#[cfg]`, y nada comprueba que coincidan. Si algún
día se añade una plataforma nueva en `lib.rs` con `#[cfg]` y se olvida actualizar este enum, el
test `fuera_de_windows_y_linux_sigue_devolviendo_no_soportada` seguiría en verde sin decir nada
sobre el enrutado real. Es el precio consciente que el propio HANDOFF explica (evitar un `#[cfg]`
real que no correría en ningún job de CI existente); lo dejo anotado como riesgo de deriva entre
las dos copias de la política, no como defecto.

**N3 — No pude determinar si el job `nucleo-windows` de `ci.yml` ya existía (con solo
`cargo check`) antes de que este implementador empezara, o si lo escribió entero.**
`git show HEAD:.github/workflows/ci.yml` —el último commit real— no tiene ese job en absoluto:
solo `nucleo` (Linux) y `app`. El PLAN da por hecho que existe («el job de Windows, que hoy solo
hace `cargo check`»), y el HANDOFF describe el cambio como «paso nuevo... con el comentario del
job actualizado», lo que da a entender que el job y su comentario ya estaban y solo se tocó una
parte. Con la regla suspendida de commits (`tablero.md`: árbol compartido sin commitear entre
tareas), «antes de esta tarea» no es lo mismo que «en HEAD», así que no puedo resolver esto con
las herramientas que tengo. Si el job entero es nuevo, es una desviación de alcance bastante
mayor que «añadir un paso de test» y debería constar como tal en «Decisiones que se apartan del
PLAN». Se lo traslado al orquestador para que lo confirme directamente.

**N4 — `Cargo.lock` todavía no incluye `windows` entre las dependencias de `dictar-audio`.**
Comprobado: el bloque `[[package]] name = "dictar-audio"` del lockfile no lista `windows` (sí
`hound`, `libspa`, `pipewire`, `rubato`, etc.). El comentario nuevo en `Cargo.toml` («es la que
ya fija `Cargo.lock`... así que no añade una segunda versión») es parcialmente verificable por
inspección: `xcap 0.0.14` sí depende de `windows 0.58.0` exacto (confirmado en el lockfile), así
que es razonable esperar que Cargo reutilice esa versión al resolver la nueva dependencia — pero
eso solo lo confirma un `cargo check`/`cargo tree` real, que B-1 impide. No es un hallazgo, es
una verificación parcial con el límite anotado.

## 3. Premisas que cuestiono

**Premisa 1 — «Las dos medidas del PLAN (reloj monótono por paquete + relleno comparando con las
muestras ya emitidas) bastan para mantener alineadas las pistas.»** La ataqué reconstruyendo la
aritmética exacta de `procesar_paquete` con los valores del propio test de la implementación
(ver I1). **Conclusión: la premisa arquitectónica —dos medidas, no una— es correcta y necesaria;
el contradictor y el orquestador acertaron al rechazar la disyuntiva falsa. Pero la
implementación concreta de la segunda medida tiene un error aritmético real (no descuenta las
muestras del propio paquete antes de comparar), y ese error no es «menor» ni «de un único
evento» como lo describe el HANDOFF: recurre en cada transición silencio→audio de la pista de
sistema, que es el caso de uso central que motivó toda esta sección del PLAN.**

**Premisa 2 — «Reproducir la misma organización que `pipewire_src.rs` (mismo `CaptureSession`,
mismo patrón de arranque, mismo contrato de `AudioFrame`) da el mismo nivel de robustez, aunque
el mecanismo del reloj tenga que cambiar.»** El PLAN es explícito en que solo el reloj cambia, no
la organización. La ataqué comparando cómo cada implementación aísla el fallo de una pista de la
otra. **Conclusión: la premisa es falsa en un punto concreto y verificable (hallazgo B1).
PipeWire logra aislamiento por pista casi gratis, por tener un stream/callback independiente por
pista dentro del mismo mainloop; WASAPI, tal como está escrito, sondea las dos pistas en un
bucle compartido con `?`, así que «misma organización» no se tradujo en «mismo aislamiento de
fallos». Es precisamente el escenario que la lista de «Casos del dominio» del PLAN marca como
cubierto (`[x]`) y no lo está.**

**Premisa 3 — Del HANDOFF: «Es un efecto de un único evento al principio de la grabación... no
algo que crezca con la duración de la clase.»** Es la premisa que el propio implementador
propone para minimizar su hallazgo. La ataqué derivando simbólicamente la recurrencia del error
a través de varios paquetes y varios eventos de silencio separados. **Conclusión: la primera
mitad es cierta —el error no se acumula sin límite entre eventos separados, cada uno se
«resetea» por el `saturating_sub`—, pero la segunda es engañosa: no es «un único evento», es un
evento que se repite en cada reanudación de audio tras un silencio del loopback, y esa es
precisamente la pista para la que «el silencio no entrega paquetes» es la premisa de diseño de
todo el archivo. En una clase con audio de sistema intermitente (vídeos, clips, alternando con
diapositivas mudas), esto podría repetirse decenas de veces.**

## 4. Qué verifiqué y no marqué

- **Recuento de bloques `unsafe`.** `grep -n "unsafe" wasapi_src.rs` da 33 coincidencias,
  coincide con lo que declara el encargo. Repasé las 33 una por una; solo I3 tiene una fuga real
  en un camino de salida.
- **Ancho de línea, contando caracteres, no bytes.** `awk '{print length($0)}'` sobre
  `sincronia.rs` y `wasapi_src.rs` (no `wc -c`, que cuenta bytes UTF-8): la línea más larga de
  los dos archivos tiene 99 caracteres (`sincronia.rs:63`); ninguna llega a 100. No hay `─` ni
  `¿` en ninguno de los dos archivos que pudieran generar falsos positivos en un conteo por
  bytes.
- **Emparejamiento `CoInitializeEx`/`CoUninitialize` en todos los caminos de salida, incluido
  pánico.** `ComGuard::iniciar()` (wasapi_src.rs:466-473) solo construye el guard si
  `CoInitializeEx` tuvo éxito, así que un fallo no dispara un `CoUninitialize` de más. `_com`
  vive en el ámbito completo de `bucle()` (línea 161) y de `dispositivos()` (línea 419), así que
  su `Drop` (líneas 475-479) se ejecuta en: retorno normal, cualquier `?` intermedio, y también
  en un `panic!` dentro de esas funciones — comprobé que el `Cargo.toml` raíz del workspace no
  fija `panic = "abort"` en ningún perfil (`grep -n panic Cargo.toml`, sin resultados), así que
  el desenrollado por defecto de Rust sigue ejecutando destructores locales incluida esta guarda.
- **Lectura de buffers COM dentro de su longitud declarada.** `tam_bytes = num_frames as usize *
  c.bloque_bytes as usize` (línea 290) y `std::slice::from_raw_parts(puntero, tam_bytes)` (línea
  299) usan exactamente el `num_frames` devuelto por el propio `GetBuffer` y el `nBlockAlign`
  negociado; no encontré ningún sitio donde se lea más allá de eso.
- **`CoTaskMemFree` del `WAVEFORMATEX*` de `GetMixFormat`.** Cubierto en el camino de error de
  `leer_formato` (líneas 335-339) y en el camino normal, incondicionalmente antes de propagar el
  resultado de `Initialize` (líneas 356-357). Sin fuga en ese puntero.
- **Invariante 2 (dos pistas, nunca mezcla).** Cada pista tiene su propio `CapturaAbierta` /
  `PistaWasapi`; el `AudioFrame` se construye una vez por pista y por paquete
  (`sincronia.rs:186-190`, `sincronia.rs:208-212`); no encontré ningún punto donde se sumen antes
  de emitir. (La función `mezclar()` que sí suma pistas vive en `lib.rs:213-232` y es para
  reproducción de una sesión ya grabada, no para captura — no es de esta HU, ver más abajo.)
- **Invariante 3 (16 kHz mono `f32`, remuestreado en origen una sola vez, reutilizando
  `mezcla.rs`).** `sincronia.rs:26` importa `a_mono`/`Remuestreador` de `mezcla.rs`;
  `PistaWasapi::procesar_paquete` (líneas 167-191) llama `normalizar_a_f32` → `a_mono` →
  `remuestreador.procesar`, en ese orden, exactamente una vez por paquete. No hay una segunda
  implementación de remuestreo o de mezcla a mono en ninguno de los dos archivos nuevos.
- **Invariante 4 (ninguna firma pública cambia según la plataforma).** `wasapi_src::iniciar` y
  `wasapi_src::dispositivos` (líneas 59 y 418) tienen la misma firma que sus equivalentes de
  `pipewire_src.rs` (líneas 62 y 356), y `lib.rs::iniciar`/`dispositivos` (líneas 166-200) siguen
  sin `#[cfg]` visible para quien las llama.
- **Clase de dos horas / contadores de muestras.** `muestras_emitidas: u64` en `PistaWasapi`
  (`sincronia.rs:135`); ningún `as u32`/`as i32` en el camino de conteo. Reproduje a mano la
  prueba `dos_horas_de_muestras_no_desbordan_el_contador` (usa un valor por encima de
  `u32::MAX`) y cuadra.
- **`Remuestreador::vaciar()` en el camino feliz.** Sí se llama, para ambas pistas, al romper el
  bucle por la señal de parada (`wasapi_src.rs:240-247`). Lo que no cubre es el camino de error
  — eso es el hallazgo B1.
- **Las tres desviaciones confesadas en el HANDOFF:**
  - Enumeración parcial de dispositivos: confirmada de forma independiente (I2), y clasificada
    como Importante, no como aceptable sin más, porque el AC 4 no distingue «función» de
    «selector de interfaz» y el PLAN tampoco lo excluye explícitamente para la función.
  - `cargo test` acotado a `-p dictar-audio`: verifiqué que el nombre del paquete en
    `core/audio-capture/Cargo.toml:2` es `dictar-audio`, coincide con el `-p` del paso nuevo del
    CI. Verifiqué también, con `grep -rn` sobre `cfg(windows)`/`target_os = "windows"` en todo
    `cli/`, `core/` y `app/` excluyendo `core/audio-capture`, que ningún otro crate tiene código
    condicionado a Windows — el argumento del HANDOFF para acotar el alcance se sostiene.
  - `Plataforma`/`tiene_backend` no declarado en la tabla del PLAN: revisado, ver N2. Aceptable
    como pieza aislada; anoto el riesgo de que se desincronice de la política real de `lib.rs`.
- **Comentarios nuevos, uno por uno, contra la línea que acompañan.** Los repasé todos en los dos
  archivos nuevos. La cabecera de `sincronia.rs` sobre «el reloj y el silencio» describe la
  intención correctamente — el defecto que encontré (I1) está en la ejecución, no en lo que dice
  el comentario, así que no lo cuento como «comentario que miente». El comentario de
  `wasapi_src.rs:166-168` («un fallo en una no aborta la otra») sí describe con precisión el paso
  de apertura (`abrir_cliente`) que tiene justo debajo, pero el bucle principal y el arranque
  (`Start()`) no cumplen esa misma propiedad — lo tomé como parte del hallazgo B1 (el código no
  hace lo que el diseño promete), no como una mentira aislada del comentario en sí.
- **Contenido de `lib.rs` fuera de la tabla de archivos del PLAN.** El diff de `lib.rs` incluye,
  además de la declaración de los dos módulos y el enrutado de `iniciar()`/`dispositivos()`, una
  función `mezclar()` nueva, el módulo `reproductor_stub`, y pruebas asociadas
  (`lib.rs:202-232`, `288-393`). Ninguno de esos cambios está en la tabla de «Archivos y
  paquetes afectados» del PLAN de HU-01 ni en la tabla «Archivos tocados» del HANDOFF, que solo
  declara los cinco archivos ya listados. Con la regla suspendida del árbol compartido sin
  commits, esto corresponde a otro trabajo (aparentemente relacionado con reproducción/HU-07, ya
  presente en el árbol) que quedó mezclado en el mismo archivo. Lo excluí de esta revisión por no
  ser parte del encargo de HU-01, y lo dejo anotado para que el orquestador confirme que no se le
  atribuye a este implementador ni a esta tarea.

## 5. Qué no pude verificar y qué haría falta

- **Todo lo que dependa de compilar.** `cargo fmt --all -- --check`, `cargo check -p dictar-audio
  --target x86_64-pc-windows-msvc`, `cargo clippy -p dictar-audio --all-targets -- -D warnings`,
  `cargo test -p dictar-audio`. Bloqueo B-1: repetí la comprobación del HANDOFF (`cargo
  --version`, `rustc --version`) en esta sesión, mismo resultado, `command not found`. Haría
  falta instalar la toolchain de Rust (1.75+) para correr cualquiera de los cuatro.
- **Firmas exactas de la API de `windows` 0.58** (tipo de retorno de `CoInitializeEx`, forma de
  `CoTaskMemFree`, orden de parámetros de `Initialize`/`GetBuffer`, existencia de
  `PWSTR::to_string()` con esa firma). El propio HANDOFF ya lo señala en detalle; repetí la
  búsqueda de `~/.cargo/registry` y de instalaciones de Rust en `C:\Users\naunf` y
  `C:\Program Files` — nada. Solo un `cargo check` con el target de Windows lo puede confirmar.
- **Comportamiento real del loopback de WASAPI ante silencio absoluto** (si de verdad deja de
  entregar paquetes, como asume toda la sección «el reloj y el silencio», o si entrega paquetes
  marcados `AUDCLNT_BUFFERFLAGS_SILENT` a intervalos regulares — que cambiaría la magnitud,
  aunque no la existencia, del hallazgo I1). Sin Windows con tarjeta de sonido no se puede
  confirmar; es una premisa de diseño del PLAN, no algo que yo pueda contrastar aquí.
- **Si `nucleo-windows` en `ci.yml` ya existía sin commitear antes de esta tarea.** Ver N3. Haría
  falta que el orquestador lo confirme directamente (por ejemplo, si guardó una copia del árbol
  antes de lanzar al implementador) o acceso a un historial más fino que un solo `git diff`
  contra el último commit.
- **Si `cargo` resuelve `windows = "0.58"` de `dictar-audio` a la misma versión 0.58.0 que ya usa
  `xcap`, sin añadir una tercera versión al árbol.** Ver N4. Requiere un `cargo tree -p
  dictar-audio` o `cargo check` real.
- **Los AC no comprobables que el propio PLAN ya señala** (1, 2, 4, 5, 6, total o parcialmente):
  requieren una máquina Windows con tarjeta de sonido y una sesión de captura real de al menos 30
  minutos para el AC 6. Ninguna herramienta de las que tengo lo sustituye.

---

## Segunda vuelta — 2026-09-03

Re-revisión acotada, contra `HANDOFF.md` (sección «Segunda vuelta») y los nueve hallazgos de
`REVIEW.md` (primera vuelta: 3 Bloqueantes, 4 Importantes, 1 Menor, más dos Notas sin acción para
el implementador). Alcance: los tres archivos tocados en esta vuelta,
`core/audio-capture/src/{wasapi_src.rs, sincronia.rs, lib.rs}`. No toqué `core/providers`,
`core/api` ni `app/lib` (HU-05, en su propia segunda vuelta, en paralelo); sí leí
`core/api/src/grabacion.rs` puntualmente, solo para dimensionar el impacto real de un hallazgo
nuevo (v2-I2 más abajo), no para evaluar su calidad.

No hay commit entre la primera y esta vuelta —`wasapi_src.rs` y `sincronia.rs` siguen `??` en
`git status`, y no hay ningún backup en `_orquestacion`— así que no existe un `git diff` que
aísle «lo que cambió en esta vuelta» de lo que ya estaba. Cada verificación de esta sección es
lectura completa del código tal como está hoy, más aritmética reproducida con Python, no un
diff contra la entrega anterior. El `HANDOFF.md` se trata igual que en la primera vuelta:
declaración a verificar, no evidencia.

### 1. Veredicto

**NO VERIFICABLE por B-1** (reconfirmado de primera mano: sin `cargo`, `rustc` ni caché de
`crates.io` en esta máquina; ni el crate `windows` ni ningún otro están vendorizados en ningún
sitio que encontrara) en todo lo que depende de compilar. **En revisión estática: los cinco
hallazgos priorizados (1, 2, 3, 7, 8) están corregidos, con evidencia reproducida por mí, no solo
leída del HANDOFF.** La ampliación de alcance a los dos `Start()` es correcta y no introduce un
camino nuevo mal manejado. Los nombres genéricos de `dispositivos()` cumplen la letra del AC 4;
hay una ambigüedad legítima sobre su espíritu, ya declarada por el implementador, que corresponde
decidir al orquestador/PO, no es un incumplimiento disfrazado. Ningún comentario nuevo miente.

**Encontré 3 hallazgos Importantes nuevos**, los tres en el código que cierra el hallazgo 1
Bloqueante (la función `cerrar_pista` y el bucle que la rodea): un error que se traga
(`vaciar()` puede fallar en el cierre y nadie se entera), un caso límite sin cubrir (si las
*dos* pistas mueren, no solo una, el hilo queda vivo indefinidamente sin decirlo), y el propio
comportamiento que corrige el Bloqueante original sin ninguna prueba que lo proteja de una
regresión futura.

### 2. Hallazgos

#### 2.1 Los nueve hallazgos de la primera vuelta, uno por uno

| # | Severidad original | Estado verificado esta vuelta |
|---|---|---|
| 1 | Bloqueante | **Corregido**, con matiz — ver 2.2 (v2-I1, v2-I2, v2-I3) |
| 2 | Bloqueante | **Corregido**, verificado con Python, no a ojo |
| 3 | Bloqueante | **Corregido**, verificado con Python, no a ojo |
| 4 | Importante | **Corregido**: la réplica se borró, los tests nuevos llaman a las funciones reales |
| 5 | Importante | **Corregido**: el fixture ya distingue silencio de audio real y comprueba el orden |
| 6 | Importante | **Corregido de la única forma honesta posible**: el comentario ya no promete blindar lo que no puede |
| 7 | Importante | **Corregido de verdad, con una decisión declarada pendiente** — ver 2.3 |
| 8 | Importante | **Corregido**, y aplicado también a la superficie COM nueva |
| 9 | Menor | **Corregido**: test nuevo que ejercita `esperadas < muestras_ya_emitidas` |

**Hallazgo 1 — el bucle ya no usa `?` sobre `procesar_paquetes`.**
`core/audio-capture/src/wasapi_src.rs:250-271`. Cada pista se sondea con `match`:

```rust
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
```

(idéntico para `sistema`, líneas 261-271). `cerrar_pista` (líneas 307-314) vacía el remuestreador
y llama `Stop()`. Recorrí **todos** los caminos de salida de `bucle()`, no solo el que cita el
HANDOFF:
- Fallo de `abrir_cliente` (apertura): ya usaba `match` desde antes de esta vuelta, no `?`; no
  hay `CapturaAbierta` que cerrar.
- Fallo de `Start()` (líneas 207-218, nuevo en esta vuelta): `if let Err(e) = ... { mic = None; }`
  sin `cerrar_pista` — correcto, porque un cliente cuyo `Start()` falló nunca llegó a producir
  audio ni hace falta `Stop()`; el `IAudioClient` se libera solo vía `Drop` (RAII del crate
  `windows`) al reasignar `mic = None`.
- Fallo de una pista a mitad de sondeo (arriba): `cerrar_pista` corre, la variable pasa a `None`.
- Cierre normal tras `rx_parar` (líneas 287-293): `cerrar_pista` corre para cada pista que
  **siga** `Some` en ese punto — no se llama dos veces sobre la misma pista, porque la que ya
  falló ya está en `None`.

No encontré ningún `?` residual entre la apertura de las pistas y el final de la función que
pueda saltarse esta limpieza. Es el defecto original resuelto: confirmé por lectura completa,
no solo en los tramos que cita el HANDOFF.

**Sobre «no deja el Receiver colgado ni el hilo vivo» (lo que se me pidió comprobar
explícitamente):** cierto para el caso de **una** pista muerta — la otra sigue, y si luego
también muere, `cerrar_pista` corre para ella también, sin doble cierre. **Falso para el caso de
que mueran las dos**: ver v2-I2 más abajo. No es una regresión del hallazgo 1 en sí —el hallazgo
1 hablaba de una pista, y ese caso está resuelto—, es un caso nuevo que la propia extensión de
alcance (aislar también el arranque) deja mejor cubierto pero no del todo, porque nadie decidió
qué hacer cuando ya no queda ninguna pista.

**Hallazgo 2 — el relleno ya no cuenta dos veces el paquete que reanuda tras un silencio.**
`core/audio-capture/src/sincronia.rs:84-93` (`muestras_de_relleno`) tiene el tercer parámetro
`muestras_del_paquete: u64`, y se resta también:

```rust
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
```

Confirmé el único *call site* de producción con `grep -n "muestras_de_relleno" -r
core/audio-capture/src`: es `sincronia.rs:171`
(`muestras_de_relleno(timestamp_ms, self.muestras_emitidas, reales.len() as u64)`), dentro de
`procesar_paquete`. No hay un segundo llamante en `wasapi_src.rs` que pudiera pasar mal el
parámetro nuevo — la única puerta de entrada es esta, y está bien.

Reproduje la aritmética exacta del test `un_silencio_en_loopback_no_adelanta_lo_que_viene_despues`
con Python (script en el scratchpad de esta sesión), no leyéndola:

```
relleno1 = muestras_de_relleno(0, 0, 1600)       = 0      -> pcm1 = 1600
relleno2 = muestras_de_relleno(2000, 1600, 1600) = 28800  -> pcm2 = 30400
total tras el segundo paquete = 1600 + 30400 = 32000  (== 2000 ms x 16 000 Hz / 1000, exacto)
```

Coincide con lo que declara el HANDOFF y con la reconstrucción de la primera vuelta (relleno real
28 800, no 30 400). El excedente de 1600 muestras (justo `reales.len()` del segundo paquete) ya
no aparece. Corregido, verificado por mí, no solo leído.

**Hallazgo 3 — `dos_horas_de_muestras_no_desbordan_el_contador` ya no falla con código correcto.**
`core/audio-capture/src/sincronia.rs:364`: la base pasó de `u32::MAX` a `u32::MAX as u64 + 1 +
1_000_000`. Reproduje el cálculo con Python:

```python
muestras_previas = (2**32 - 1) + 1 + 1_000_000        # 4 295 967 296, múltiplo de 16
suma = muestras_previas + 32_000                       # 4 295 999 296, múltiplo de 16
timestamp = (suma * 1000) // 16000                      # 268 499 956
resto_perdido = (suma * 1000) % 16000                    # 0  <- exacto, no se pierde nada
relleno = muestras_de_relleno(timestamp, muestras_previas, 0)  # 32 000, exacto
```

Da `32_000` exacto, sin truncar — confirma la corrección. Y reproduje también la base **rota**
de la primera vuelta (`u32::MAX` a secas, sin el `+ 1`) para confirmar que efectivamente
producía `31_985`, tal como calculó `verificador-pruebas`: coincide exactamente. El mismo bug,
ahora con la causa (congruencia módulo 16) resuelta en la raíz, no parcheada con un número
mágico distinto.

**Hallazgo 4 — la réplica `Plataforma`/`tiene_backend` ya no existe.**
`grep -n "Plataforma\|tiene_backend" core/audio-capture/src/{sincronia,wasapi_src,lib}.rs` solo
encuentra menciones en comentarios que hablan de la réplica ya borrada, como referencia
histórica (`sincronia.rs:394`, `lib.rs:398,423`), no código real. En su lugar,
`core/audio-capture/src/lib.rs:415-431` tiene dos tests (`fuera_de_windows_y_linux_
iniciar_devuelve_no_soportada`, `fuera_de_windows_y_linux_dispositivos_devuelve_no_soportada`)
gateados por `#[cfg(not(any(target_os = "linux", target_os = "windows")))]` — el mismo `cfg`
exacto que la rama que verifican (`lib.rs:177,196`) — y que llaman a `iniciar()`/`dispositivos()`
reales, no a una copia. Esto sí ejercita el `#[cfg]` real el día que exista un job de CI que
compile para una tercera plataforma; la réplica nunca lo habría hecho ni con ese job. Sigue sin
correr en ningún job de CI existente (matriz actual: solo Linux y Windows), y el HANDOFF lo dice
sin rodeos en el comentario del propio test (`lib.rs:409-411`) — no es una promesa incumplida,
es un límite estructural ya señalado por `auditor-plataforma` en la primera vuelta.

**Hallazgos 5 y 6 — el test de silencio distingue orden, y el test del reloj ya no promete lo
que no puede.** `sincronia.rs:248-294`: el «audio real» del fixture es `0.5`, no `0.0`, y hay un
segundo `assert!` (línea 290-293) que comprueba que las últimas 1600 muestras son `0.5`. Una
mutación que invierta el orden (silencio después del audio) ya no pasaría inadvertida: antes,
con ambos en `0.0`, el `pcm` resultante era idéntico byte a byte. `sincronia.rs:296-322`, el test
renombrado `el_relleno_resta_lo_ya_emitido_del_timestamp_esperado`, tiene un comentario que dice
con precisión qué protege (`muestras_de_relleno` resta bien, dado un timestamp) y qué no puede
proteger por construcción (que ese timestamp de verdad venga del reloj monótono y no del conteo
de muestras — eso vive en `wasapi_src.rs`, sin pruebas posibles sin hardware). Leí el comentario
contra el cuerpo del test línea por línea: no hay ninguna afirmación que el código no cumpla.

**Hallazgo 7 — `dispositivos()` enumera de verdad, con una pieza declarada pendiente.** Ver
detalle completo en 2.3, con el mismo rigor pedido para el resto de los `unsafe`.

**Hallazgo 8 — la fuga de `PWSTR` está corregida en los dos sitios donde aplica.**
`id_de_endpoint_por_defecto` (`wasapi_src.rs:568-582`):

```rust
let puntero = unsafe { dispositivo.GetId() }.map_err(err)?;
let resultado = unsafe { puntero.to_string() }.map_err(err);
unsafe { CoTaskMemFree(Some(puntero.0 as *const c_void)) };
resultado
```

El `?` que antes estaba pegado a `to_string()` desapareció: ahora `resultado` se calcula sin
propagar, el puntero se libera **incondicionalmente**, y solo entonces se retorna `resultado`
(última expresión, sin `;`). Mismo patrón exacto en `enumerar_flujo` (`wasapi_src.rs:538-544`),
que tiene la misma llamada a `to_string()` sobre un `PWSTR` de COM dentro del bucle de
enumeración — confirmé que es el mismo código, no una aproximación distinta que pudiera volver a
romperse. Los dos caminos de fuga posibles del archivo están cerrados.

**Hallazgo 9 — el `saturating_sub` ya tiene quien lo ejercite.**
`sincronia.rs:336-344`, `el_relleno_nunca_es_negativo_si_ya_se_emitio_de_mas`:
`muestras_de_relleno(100, 10_000, 0)`. Reproduje: `esperadas = 1600`, `1600.saturating_sub(10_000)
= 0` (sin el `saturating_sub` sería *underflow* de `u64`, pánico en *debug*). Corregido.

#### 2.2 Hallazgos nuevos, en el código que cierra el hallazgo 1

Los tres viven en `cerrar_pista` y en el bucle que la rodea — exactamente el código nuevo más
importante de esta vuelta. Ninguno estaba en los nueve de la primera vuelta porque la función no
existía.

**v2-I1 (Importante) — `cerrar_pista` descarta en silencio el error de `vaciar()`; el mismo tipo
de error sí se registra unas líneas más abajo, en `procesar_paquetes`.**

`core/audio-capture/src/wasapi_src.rs:307-314`:

```rust
fn cerrar_pista(c: &mut CapturaAbierta, fin_ms: i64, tx: &Sender<AudioFrame>) {
    if let Ok(Some(frame)) = c.pista.vaciar(fin_ms) {
        let _ = tx.send(frame);
    }
    unsafe {
        let _ = c.cliente.Stop();
    }
}
```

`PistaWasapi::vaciar` (`sincronia.rs:195-208`) devuelve `Result<Option<AudioFrame>>`: puede
fallar de verdad si `Remuestreador::vaciar()` (`mezcla.rs:119-136`) falla, lo que ocurre si
`FastFixedIn::process` (la librería `rubato`) devuelve `Err` al procesar el último bloque
pendiente. El `if let Ok(Some(frame)) = ...` de la línea 308 no distingue `Ok(None)` (no había
nada pendiente, el caso normal) de `Err(_)` (el vaciado falló de verdad): en ambos casos, no
hace nada. Comparado con el mismo tipo de error dos funciones más arriba, `procesar_paquetes`
(línea 365): `Err(e) => tracing::warn!(error = %e, "fallo de remuestreo, bloque descartado")` —
ahí sí se registra. Es la misma clase de fallo (remuestreo), tratada de forma distinta en el
mismo archivo, y la diferencia no está explicada en ningún comentario.

**Qué debería pasar:** como mínimo, un `tracing::warn!` en la rama `Err`, igual que en
`procesar_paquetes` — es el patrón que el propio archivo ya usa a pocas líneas de distancia.

**Escenario concreto:** una clase de dos horas termina con normalidad (o una pista se cierra
porque la otra falló). Si en ese instante `Remuestreador::vaciar()` falla — el escenario menos
raro no es un bug de `rubato`, es un `pendientes.len()` que por alguna razón no coincide con lo
que `process` espera tras `resize(BLOQUE, 0.0)` —, las últimas muestras pendientes de esa pista
(hasta ~21-64 ms, según la propia estimación de la primera vuelta para este mismo remuestreador)
se pierden **sin ningún rastro**: ni `tracing::warn!`, ni `tracing::error!`, nada en el log, nada
en el archivo. Es exactamente el patrón «fallar en silencio» que este proyecto ya sufrió, aunque
aquí la magnitud es de milisegundos, no de una clase entera — por eso lo marco Importante y no
Bloqueante: la definición de Bloqueante de este proyecto es perder audio a la escala de «la
clase», no de un bloque de remuestreo.

**v2-I2 (Importante) — si mueren las dos pistas, no solo una, el hilo de captura queda vivo
indefinidamente sin producir nada y sin ninguna forma de que el llamador de `iniciar()` se
entere.**

`core/audio-capture/src/wasapi_src.rs:232-279`, el bucle principal:

```rust
loop {
    match rx_parar.try_recv() {
        Ok(()) => break,
        Err(TryRecvError::Disconnected) => break,
        Err(TryRecvError::Empty) => {}
    }
    let mut hubo_datos = false;
    if let Some(c) = mic.as_mut() { /* ... puede poner mic = None ... */ }
    if let Some(c) = sistema.as_mut() { /* ... puede poner sistema = None ... */ }
    if !hubo_datos {
        std::thread::sleep(Duration::from_millis(10));
    }
}
```

Confirmé por lectura completa que no hay ningún `if mic.is_none() && sistema.is_none() { break;
}` en ningún punto (grep de `break` en el archivo: solo las dos líneas de `rx_parar` y el
`break` interno de `procesar_paquetes`, que es otro bucle). El aislamiento por pista —correcto y
necesario para el caso de que **una** muera— no contempla el caso de que mueran **las dos**: el
bucle sigue iterando cada 10 ms, revisando solo `rx_parar`, indefinidamente, sin producir ningún
`AudioFrame` más y sin dar ninguna señal de que la captura real ya terminó. El canal `tx`
(`Sender<AudioFrame>`) no se cierra —solo se cierra cuando `bucle()` retorna, y `bucle()` no
retorna hasta que llega la señal manual de parar—, así que ningún consumidor puede distinguir
«no hay audio en este instante» de «ya no va a haber audio nunca más en esta sesión».

Esto no es una regresión del hallazgo 1: en la primera vuelta, con `?`, **cualquier** fallo
(de una pista o de las dos) mataba `bucle()` entero, y eso sí cerraba `tx` al retornar,
comunicando el fin —aunque perdiendo audio ya capturado, que era el defecto real—. Al arreglar
el caso de una sola pista, el caso de las dos quedó, sin que nadie lo decidiera, con una
propiedad nueva: ya no se pierde audio, pero tampoco se comunica que la captura real terminó.
Ninguno de los nueve hallazgos de la primera vuelta pedía esto, y el PLAN tampoco lo contempla en
su lista de «Casos del dominio» (que habla de «el dispositivo por defecto puede cambiar»,
singular, con «la otra sigue» dando por hecho que hay una otra).

**Escenario concreto:** en una videollamada grabada con `dictar_ia`, el usuario desconecta a la
vez el micrófono USB y los auriculares USB (por ejemplo, un hub USB que se desenchufa) a los 40
minutos de una clase de dos horas. Las dos pistas fallan en la misma iteración del bucle (o en
iteraciones consecutivas), cada una se cierra bien (sin pérdida, `cerrar_pista` corre para
ambas), pero el hilo sigue vivo, «grabando» sin grabar nada, durante los 80 minutos que restan
hasta que el usuario decide parar manualmente porque nota, por su cuenta, que algo no va bien.

**Atenuante que sí verifiqué, sin ser mi alcance evaluarlo a fondo:** en
`core/api/src/grabacion.rs:344` (HU-05, fuera de mi encargo, solo lo leí para contexto), el
consumidor real de este `Receiver` usa `rx.recv_timeout(Duration::from_millis(200))` con un
`AtomicBool` de parada revisado en cada vuelta — el propio comentario dice «permite comprobar la
señal de parada aunque una pista deje de entregar bloques, en vez de quedarse colgado». Esto
evita que **ese** consumidor específico se bloquee para siempre en un `recv()`. No resuelve el
problema de fondo: el hilo de `wasapi_src.rs` sigue vivo sin trabajo, y el contrato público de
`dictar_audio::iniciar()` (solo `Receiver<AudioFrame>` + `Box<dyn CaptureSession>`) no tiene
ningún canal por el que comunicar «ya no queda ninguna pista activa» a este ni a ningún otro
consumidor futuro.

**v2-I3 (Importante) — el comportamiento que corrige el hallazgo 1 Bloqueante no tiene ninguna
prueba que lo proteja de una regresión.**

El AC 7 de la HU menciona explícitamente, como ejemplo de lo que un test sin hardware debería
cubrir: «el manejo del cambio de dispositivo por defecto». Es exactamente lo que hace el bucle
de `wasapi_src.rs:250-271`, y sigue sin ninguna prueba — ni en la primera vuelta ni en esta.
`pistas_a_grabar` (`sincronia.rs:103-115`) demuestra que la política («qué pistas seguir con lo
que haya») **sí** se puede extraer a una función pura y testear sin COM; la política equivalente
para el caso de en-vivo («si esta pista falla a mitad de sesión, ¿la cierro y sigo con la otra,
o freno del todo?») no se extrajo de la misma forma — quedó como control de flujo directo dentro
de `bucle()`, que por diseño del propio PLAN no tiene pruebas y no puede tenerlas sin hardware.
No es una promesa incumplida (el PLAN nunca prometió testear `wasapi_src.rs`), pero sí es el
hueco de prueba que, de existir, habría hecho tropezar con v2-I2: una prueba pura del tipo «dado
que las dos pistas devuelven `Err`, ¿qué decide el bucle?» habría forzado a decidir qué pasa
cuando no queda ninguna, en vez de dejarlo sin decidir.

#### 2.3 Evaluación de las decisiones declaradas por el implementador

**La extensión del aislamiento a los dos `Start()` es correcta.**
`wasapi_src.rs:207-218`. No estaba en la cita literal del hallazgo 1 (`REVIEW.md` cita
219-225), pero el propio `REVIEW-codigo.md` de la primera vuelta ya la señalaba como «segunda
manifestación, misma causa raíz» del mismo B1, no como un hallazgo aparte — así que tratarla
junto con el hallazgo 1 es razonable, no una ampliación de alcance no pedida.

Verifiqué el patrón de préstamo que hace posible el aislamiento (`if let Some(c) = &mic { if let
Err(e) = unsafe { c.cliente.Start() } { ...; mic = None; } }`): es *non-lexical lifetimes*
estándar — el préstamo compartido de `c` (derivado de `&mic`) termina en su último uso real
(`c.cliente.Start()`), que ocurre **antes**, en el flujo de control, de la reasignación
`mic = None` dentro del mismo brazo. Es el mismo patrón, ya verificado y aceptado, que usa el
propio `cerrar_pista(c, ...)` con `mic.as_mut()` en el bucle principal. No encontré ningún camino
donde el orden de evaluación pudiera hacer que se usara `c` **después** de la reasignación (lo
que sí rompería el préstamo). Razonado, no compilado — sigue bajo B-1 — pero coherente con el
resto del archivo.

También verifiqué la consecuencia funcional: mover la comprobación de `pistas_a_grabar`
(`wasapi_src.rs:220-223`) a **después** de los dos `Start()` (no solo después de las dos
aperturas) cierra un caso que, de haber quedado antes, se habría colado: las dos pistas se abren
bien pero ninguna arranca — antes de este cambio, comprobar solo `mic.is_some() ||
sistema.is_some()` tras la apertura habría dejado pasar una sesión "abierta" con cero pistas
realmente capturando, el mismo bug que ya cubre `sin_ninguna_pista_disponible_es_un_error_
explicito` para el caso de apertura. Es una mejora real, no solo un cambio cosmético.

**Los nombres genéricos de `dispositivos()` cumplen la letra del AC 4; el espíritu queda
ambiguo, y está declarado, no oculto.**

El AC 4 dice, textual (`docs/06-historias-de-usuario.md:61`): «`dictar_audio::dispositivos()`
enumera los dispositivos reales del sistema.» No menciona el nombre visible en ningún punto.
Verifiqué que la implementación cumple la parte que sí exige el texto: `enumerar_flujo`
(`wasapi_src.rs:520-566`) usa `IMMDeviceEnumerator::EnumAudioEndpoints` con
`DEVICE_STATE_ACTIVE` sobre las dos direcciones, cada `DeviceInfo.id` es el `IMMDevice::GetId()`
real (no un valor fijo), y el campo `por_defecto` se calcula comparando contra
`GetDefaultAudioEndpoint` real, no se asume. Comparé contra `pipewire_src::dispositivos()`
(`pipewire_src.rs:356-377`, Linux, no tocado por esta HU): ahí los dos `DeviceInfo` son
literales fijos (`id: "default_source".into()`, etc.), ni siquiera consultan el registro real de
PipeWire — así que la implementación de Windows de esta vuelta enumera **más** dispositivos
reales que la de Linux hoy, no menos.

Lo que falta es el nombre "amigable" (`IPropertyStore`/`PROPVARIANT`), sustituido por
`"Micrófono N"` / `"Salida N (predeterminado)"`. Esto es una decisión declarada explícitamente
tres veces —doc-comment de `dispositivos()` (`wasapi_src.rs:477-488`), «Decisiones que se apartan
del PLAN» y «Deuda que dejo» del HANDOFF—, con la razón técnica concreta (unión sin tipar,
*features* de Cargo nuevas sin verificar). No es un incumplimiento disfrazado: es exactamente lo
contrario, una desviación declarada con su justificación y su alcance, tal como pide el
protocolo. Mi conclusión: el AC 4, en su letra, se cumple. Si el criterio real que se quiere
exigir es «un futuro selector de dispositivo tiene que poder distinguir un micrófono de otro por
su nombre», eso no está en el texto del AC 4 tal como está escrito hoy en `docs/06`, y el PLAN
excluye explícitamente el selector de interfaz de esta HU. Es una decisión de producto, no un
hallazgo de código: la dejo para que el orquestador o el PO la resuelvan con esa distinción a la
vista.

**Nota, no hallazgo:** el número (`"Micrófono N"`) sale del orden de `EnumAudioEndpoints`, que
Microsoft no documenta como estable entre llamadas. El `id` de cada dispositivo sí es estable (es
el identificador COM real), así que nada se rompe funcionalmente, pero un futuro selector de UI
que memorice «Micrófono 2» entre sesiones distintas podría referirse a un dispositivo físico
distinto cada vez. Fuera de alcance de esta HU (el selector está explícitamente excluido), lo
anoto para quien retome el nombre amigable real.

**Comentarios nuevos: ninguno miente.**
Repasé, línea por línea contra el código que acompañan, todos los comentarios nuevos de esta
vuelta: la cabecera de `muestras_de_relleno` (`sincronia.rs:68-83`, describe con precisión el
porqué del tercer parámetro), el comentario del bucle principal
(`wasapi_src.rs:241-249` — describe bien el caso de una pista, no dice nada falso sobre el caso
de las dos, ver v2-I2), el doc-comment de `cerrar_pista` (`wasapi_src.rs:299-306`), el de
`dispositivos()`/`enumerar_flujo` (`wasapi_src.rs:467-488, 517-519`), y el comentario del test
renombrado `el_relleno_resta_lo_ya_emitido_del_timestamp_esperado`
(`sincronia.rs:298-307` — dice explícitamente qué protege y qué no, y el cuerpo del test hace
justo eso). Ninguno afirma algo que la línea que acompaña no haga.

### 3. Premisas que cuestiono

**Premisa 1 — «Aislar cada pista con `match` en vez de `?`, extendido a los dos `Start()`, deja
resuelto el problema de origen del hallazgo 1.»** La ataqué buscando qué pasa cuando el
aislamiento funciona en las dos pistas a la vez, es decir, cuando ambas fallan por separado en
vez de solo una. **Conclusión: la premisa es correcta para el caso que el hallazgo 1 describía
—una pista falla, la otra sigue— y ahí el arreglo es sólido, verificado en todos sus caminos de
salida. Es incompleta para el caso de que fallen las dos: nadie decidió qué debe pasar cuando ya
no queda ninguna pista viva, ni el PLAN original (que habla de «la pista», singular, con «la otra
sigue» dando por hecho que hay una otra) ni esta corrección. El resultado (v2-I2) no reintroduce
la pérdida de audio que el hallazgo 1 corrigió, pero sí dejó sin decidir si la sesión debe
comunicar de alguna forma que la captura real terminó.**

**Premisa 2 — «Esta vez la aritmética se verificó con Python de verdad, no a mano, así que el
nivel de confianza general de la entrega subió respecto de la primera vuelta.»** Es la premisa
central que el propio HANDOFF usa para distinguirse de la primera vuelta («no la vuelta
anterior»). La ataqué separando qué parte del código puede verificarse con Python (aritmética
pura, en `sincronia.rs`) de qué parte no puede, bajo ninguna circunstancia sin compilador
(`unsafe`, superficie COM). **Conclusión: cierta y valiosa para la parte que cubre —los cinco
tests de `muestras_de_relleno` y el de las dos horas, que ahora sí están verificados con una
herramienta externa, no solo con lectura—, pero la superficie sin verificar de ninguna forma
creció en esta vuelta, no decreció: `enumerar_flujo` (`wasapi_src.rs:520-566`,
`EnumAudioEndpoints`/`IMMDeviceCollection::GetCount`/`Item`) es código `unsafe` nuevo contra una
API que el propio HANDOFF señala como «la pieza de menos confianza» de toda la entrega. El nivel
de confianza de la parte pura subió; el de la parte que más importa —si esto compila y funciona
contra Windows real— sigue exactamente donde estaba, con más líneas dentro.**

### 4. Qué verifiqué y no marqué

- **Aritmética de `muestras_de_relleno` con los tres parámetros**, para los seis casos que la
  ejercitan (los cinco tests de `sincronia.rs` más el escenario completo de dos paquetes de
  `un_silencio_en_loopback_no_adelanta_lo_que_viene_despues`), con un script de Python que
  reproduce la función exacta (`sat_sub` con la misma cascada de `saturating_sub`), no una
  aproximación. Coincide con cada `assert_eq!` del código, y coincide con las cifras del HANDOFF
  — pero las reproduje yo, no las copié.
- **El único llamante de producción de `muestras_de_relleno`**: `grep -n "muestras_de_relleno" -r
  core/audio-capture/src` da un solo *call site* fuera de tests y de la propia definición
  (`sincronia.rs:171`), con el tercer parámetro correcto (`reales.len() as u64`). No hay un
  segundo llamante en `wasapi_src.rs` (que no llama a esta función directamente en ningún punto,
  solo a través de `PistaWasapi::procesar_paquete`) que pudiera pasar el parámetro nuevo mal.
- **Los 40 bloques `unsafe` de `wasapi_src.rs`** (`grep -n unsafe`, incluye las 8 declaraciones
  `unsafe fn`), leídos uno por uno contra el archivo completo. Los que ya existían en la primera
  vuelta no cambiaron de forma relevante; los nuevos son los de `enumerar_flujo` y su llamada
  desde `dispositivos()` — ver el detalle en 2.1 (hallazgo 7) y 2.3.
- **Liberación de `PWSTR` en los dos sitios que llaman `to_string()` sobre un puntero COM**
  (`id_de_endpoint_por_defecto` y el bucle de `enumerar_flujo`): confirmé que ambos separan el
  cálculo del resultado de su propagación, liberan incondicionalmente y solo entonces propagan.
  Mismo patrón, aplicado dos veces.
- **`IMMDeviceCollection`/`IMMDevice` dentro de `enumerar_flujo`**: no hay ningún `Release()`
  manual, igual que para `IMMDeviceEnumerator`/`IMMDevice` en el resto del archivo (ya confirmado
  en la primera vuelta): los tipos del crate `windows` para interfaces COM implementan `Drop`
  con `Release()` automático. Razonado por continuidad con el mismo patrón ya usado y aceptado en
  `abrir_cliente` para tipos COM idénticos, no verificado con compilador — sigue bajo B-1.
- **Comportamiento de `enumerar_flujo` con cero dispositivos activos**: `for i in 0..total` con
  `total = 0` no itera; la función retorna `Ok(())` sin tocar `lista`. Verificado por lectura del
  control de flujo (determinista, no requiere ejecución). `dispositivos()` en conjunto: si las
  dos direcciones dan cero, retorna `Ok(vec![])`, no un error — coherente con el principio ya
  declarado en el propio código («un fallo en una dirección no vacía la lista de la otra»).
- **Comparación con `pipewire_src::dispositivos()`** (Linux, no tocado por esta HU): confirma que
  la enumeración de Windows de esta vuelta va más allá de lo que hace Linux hoy (ver 2.3).
- **Los dos tests nuevos del hallazgo 4** (`lib.rs:415-431`): confirmé que llaman a `iniciar()` y
  `dispositivos()` sin prefijo (correcto, `use super::*;` ya en el módulo), bajo el mismo `#[cfg]`
  que las ramas que verifican (`lib.rs:177,196`), y que `Plataforma`/`tiene_backend` no aparecen
  en ningún sitio del crate salvo en comentarios que hablan de la réplica ya borrada.
- **Ancho de línea, contando caracteres Unicode, no bytes**: script de Python (`len()` sobre cada
  línea decodificada como UTF-8, no `wc -c`) sobre los tres archivos completos. Máximo: 99
  caracteres (`sincronia.rs:63`, sin cambios respecto de la primera vuelta), 98
  (`wasapi_src.rs:265`), 95 (`lib.rs:166`). Ninguno llega a 100. Sin `─` ni `¿` en ninguno de los
  tres archivos.
- **Restas aritméticas nuevas**: `grep -n " - "` sobre `wasapi_src.rs` y `sincronia.rs` — fuera de
  los dos `saturating_sub` ya cubiertos y de literales dentro de `assert_eq!` en tests (valores
  conocidos en tiempo de compilación, sin riesgo), no encontré ninguna resta nueva sobre enteros
  sin signo que pudiera desbordar.
- **`Cargo.toml` y `.github/workflows/ci.yml` no cambiaron en esta vuelta**, tal como declara el
  HANDOFF: los parseé de nuevo (`tomllib` para el primero, `yaml.safe_load` para el segundo) y el
  resultado es idéntico, campo por campo, a lo que documentó `auditor-plataforma` en
  `REVIEW-plataforma.md` de la primera vuelta (mismas *features* de Windows, mismo paso `tests de
  dictar-audio` con `cargo test -p dictar-audio`). No es una verificación exhaustiva nueva de
  esos dos archivos —no era el encargo de esta vuelta—, es una comprobación de que la afirmación
  «no se tocaron» se sostiene.
- **B-1, reconfirmado de primera mano una tercera vez** (ya lo habían hecho el implementador y yo
  mismo en la primera vuelta): sin `cargo`, `rustc`, ni caché de `crates.io` en
  `C:\Users\naunf\.cargo` ni en ningún otro sitio de esta máquina.

### 5. Qué no pude verificar y qué haría falta

- **Todo lo que dependa de compilar.** Sin cambios respecto de la primera vuelta: `cargo fmt`,
  `cargo check -p dictar-audio --target x86_64-pc-windows-msvc`, `cargo clippy -p dictar-audio
  --all-targets -- -D warnings`, `cargo test -p dictar-audio`. Haría falta la misma toolchain que
  ya pedía la primera vuelta.
- **Las firmas exactas de `IMMDeviceEnumerator::EnumAudioEndpoints` y
  `IMMDeviceCollection::{GetCount, Item}` contra el crate `windows` 0.58 real.** Es la superficie
  `unsafe` nueva de esta vuelta, y el propio HANDOFF ya la marca como el punto de menos confianza.
  Razoné, igual que el implementador, a partir del IDL público de `mmdeviceapi.idl` y por
  analogía con `GetDefaultAudioEndpoint` (ya en uso, mismo archivo) — no es una verificación
  independiente adicional a la suya, es el mismo límite: sin compilador no hay forma de
  confirmarlo. Un `cargo check -p dictar-audio --target x86_64-pc-windows-msvc` lo resolvería de
  inmediato si algo no encaja.
- **Que el patrón de préstamo NLL que verifiqué a mano para los dos `Start()` compile en la
  versión exacta de `rustc` que use el CI.** Razonado por analogía con un patrón ya presente y
  aceptado en el mismo archivo (`mic.as_mut()` en el bucle principal), no compilado.
- **El comportamiento real de `EnumAudioEndpoints` con 0, 1 y N dispositivos activos en un
  Windows real**, y si el orden de enumeración es o no estable entre llamadas (afecta solo al
  nombre genérico numerado, no al `id`, ver 2.3). Sin Windows con hardware de audio no se puede
  confirmar.
- **El escenario de v2-I2 (las dos pistas mueren a mitad de sesión) en un Windows real.** No hay
  forma de disparar dos `AUDCLNT_E_DEVICE_INVALIDATED` a la vez sin hardware y sin dos
  dispositivos que desconectar durante una captura activa.
- **Si `Remuestreador::vaciar()` (v2-I1) falla alguna vez en la práctica.** Por lectura de
  `mezcla.rs`, el camino de error existe pero no encontré ningún escenario de uso normal que lo
  dispare — requeriría instrumentar `rubato::FastFixedIn` o forzar un estado interno
  inconsistente, algo que no puedo hacer sin compilador.

Revisor-codigo · 2026-09-03

---

## Tercera vuelta — 2026-09-03

Re-revisión acotada a v2-1, v2-2 y v2-3 (REVIEW.md, sección «Segunda vuelta»), contra HANDOFF.md
(sección «Tercera vuelta») y el estado actual de core/audio-capture/src/wasapi_src.rs y
core/audio-capture/src/sincronia.rs. No reevalúo los nueve hallazgos de la primera vuelta ni las
dos decisiones ya aceptadas (aislamiento en los Start(), nombres genéricos de dispositivo): están
cerrados y fuera de este encargo. Tampoco toqué core/providers, core/api ni app/lib (HU-05, en
re-revisión aparte). El HANDOFF.md se trata como declaración a verificar, no como evidencia --
incluida la hipótesis que el orquestador adelantó sobre v2-1, que trato con el mismo escepticismo.

### 1. Veredicto

NO VERIFICABLE por B-1 (reconfirmado: sin cargo ni rustc en esta máquina) en todo lo que dependa
de compilar. En revisión estática: los tres hallazgos de la segunda vuelta están corregidos tal
como se declara, sin hallazgos nuevos Bloqueantes ni Importantes. La pregunta concreta que el
encargo dejaba abierta sobre v2-1 -- si el break puede dispararse con audio pendiente de procesar
o sin vaciar la cola de la última pista en morir -- la verifiqué recorriendo los cuatro (y solo
cuatro) caminos que tocan mic/sistema: no ocurre. Un punto declarado como deuda (el canal no
distingue "no queda pista" de "detener()") lo trato como aceptable, no como hallazgo -- ver la
sección 3.

### 2. Hallazgos

Ninguno nuevo con severidad Bloqueante o Importante, con la búsqueda documentada en la sección 4.
Una Nota sobre la deuda declarada:

Nota -- el cierre del canal (v2-1) no distingue "no queda pista" de "detener()" explícito.
wasapi_src.rs:281-284 y HANDOFF.md, «Deuda que dejo». Confirmado: las dos vías de salida de
bucle() (romper por rx_parar o por alguna_pista_sigue_viva) sueltan tx de la misma forma -- ningún
valor ni evento acompaña el cierre para decir por qué. No lo elevo a hallazgo porque no rompe
ninguno de los seis invariantes declarados: el audio ya capturado no se pierde (es lo que corrige
v2-1) y el motivo sí queda en el log (tracing::error! en las líneas 254, 265 y 282). Resolverlo de
raíz exige enriquecer el contrato de iniciar() (hoy Receiver<AudioFrame> a secas) o el consumo en
core/api/src/grabacion.rs -- los dos fuera de este encargo y de HU-01.

v2-1 -- confirmado. `wasapi_src.rs:281-284`. Verifiqué de primera mano, no repitiendo el HANDOFF:
`tx: Sender<AudioFrame>` se recibe por valor en la firma de `bucle` (línea 160); `grep -n clone
wasapi_src.rs` no da resultados; `grep -n "spawn("` da una sola coincidencia (línea 81). El break
en cuestión (línea 283), junto con los de `rx_parar` (234-235), es el único punto donde el bucle
principal termina -- `grep -n break` da una cuarta coincidencia en la línea 354, que es el break
interno de `procesar_paquetes`, un bucle distinto.

Sobre la pregunta que quedaba sin mirar por nadie: recorrí los sitios donde `mic`/`sistema` pasan
a `None`. `grep -n "= None" wasapi_src.rs` da exactamente cuatro reasignaciones -- líneas 210 y
216 (fallo de `Start()`), 257 y 268 (fallo de sondeo a mitad de captura) -- fuera de la asignación
inicial (líneas 176-198, un `let` con `match`, no una reasignación). Contra los cuatro sitios de
`cerrar_pista` (`grep -n cerrar_pista`: 256, 267, 302, 305, más la definición en 325):

- Apertura fallida (asignación inicial) y `Start()` fallido (210, 216): pasan a `None` sin
  `cerrar_pista`, pero en los dos casos el `IAudioClient` nunca llegó a producir audio --
  `PistaWasapi` recién creada, remuestreador vacío --, así que no hay nada que vaciar.
- Fallo a mitad de sondeo (257, 268): `cerrar_pista` corre (256, 267) antes de la reasignación,
  dentro del mismo brazo del `match`.
- Cierre normal al final de `bucle()` (301-306): corre para cada pista que siga `Some` en ese
  punto, sin doble cierre sobre la que ya falló.

No hay un quinto camino. Como `alguna_pista_sigue_viva` (línea 281) solo da `false` cuando ambas
variables ya son `None`, y las dos vías que las ponen en `None` con audio real pendiente pasan por
`cerrar_pista` antes, el break no se dispara sin haber vaciado la cola de la última pista en
morir. Sobre "audio pendiente de procesar": `procesar_paquetes` agota `GetNextPacketSize()` hasta
0 o hasta un error en cada llamada (351-355), así que ninguna pista que siga `Some` puede tener
paquetes ya disponibles sin leer en el momento del chequeo -- y si una pista está en `None`, es
porque ya se cerró, no porque quedara sin mirar. Coincido con la hipótesis del orquestador, con
evidencia propia y con la pregunta concreta que quedaba abierta respondida, no solo con el
argumento de la firma por valor.

v2-2 -- confirmado, `match` equivalente. `wasapi_src.rs:325-339`, comparado brazo por brazo contra
`procesar_paquetes:385-391`: `Ok(Some(frame))` con el mismo envío por `tx.send`, idéntico en
estructura; `Ok(None) => {}` idéntico; `Err(e) => tracing::warn!(...)` mismo patrón (`error = %e`,
mensaje literal), solo cambia el texto del mensaje, coherente con el contexto (vaciar vs. procesar
un bloque). Contra el `cerrar_pista` anterior a esta vuelta (el que cita la sección «Segunda
vuelta» de este mismo documento, hallazgo v2-I1): los brazos `Ok(Some(_))` y `Ok(None)` se
comportan exactamente igual que antes -- el único cambio de comportamiento real es que `Err(_)`
ahora se registra en vez de descartarse. `Stop()` sigue corriendo igual después del `match`, sin
condicionar su ejecución al resultado de `vaciar()`.

v2-3 -- confirmado, sin decisión duplicada. `sincronia.rs:129-131` (`microfono_vivo ||
sistema_vivo`, sin más). `grep -rn alguna_pista_sigue_viva core/audio-capture/src` da: la
definición, dos menciones en comentarios, un sitio de llamada de producción
(`wasapi_src.rs:281`) y cuatro asserts en tres pruebas (`sincronia.rs:414, 415, 424, 441`) --
ninguna definición duplicada, coincide con lo declarado. Repasé el resto del bucle buscando una
segunda decisión sobre "seguir o terminar por falta de pistas": el único otro punto de salida es
`rx_parar` (parada explícita, política distinta y ya existente), y el aislamiento por pista
(250-271) decide aislar, no terminar -- decide qué pista cerrar, no si la sesión sigue.
`wasapi_src.rs` aporta exactamente `mic.is_some()`/`sistema.is_some()` (estado COM, dos bool) y
delega el resto a la función real; no encontré una segunda condición "OR" en el bucle.

### 3. Premisas que cuestiono

Premisa -- "El break de v2-1 nunca deja audio sin vaciar, porque `cerrar_pista` ya corrió para
toda pista que pasa a `None` con datos capturados." Es la hipótesis que el propio encargo pedía
verificar, no dar por buena. La ataqué enumerando con `grep` todas las reasignaciones de
`mic`/`sistema` (cuatro) y contrastándolas contra los cuatro sitios de `cerrar_pista`. Conclusión:
se sostiene. Los dos caminos que ponen una pista en `None` sin pasar por `cerrar_pista` (apertura
y `Start()` fallidos) nunca llegaron a capturar nada; los dos que sí capturaron algo pasan por
`cerrar_pista` antes de la reasignación, dentro del mismo brazo del `match`. No encontré un quinto
camino.

Premisa -- "La deuda declarada (el canal no distingue el motivo de cierre) no exige acción en este
encargo." La ataqué revisando si algún invariante de los seis del proyecto la cubre. Conclusión:
se sostiene como deuda aceptable, no como hallazgo abierto -- ningún invariante exige que el
consumidor sepa por qué se cerró el canal, solo que el audio no se pierda (ya corregido por v2-1)
y que el fallo no se trague (se registra con `tracing::error!` en cada pista y al terminar el
bucle). Resolverlo de raíz toca `core/api`, fuera de HU-01.

### 4. Qué verifiqué y no marqué

- Firma por valor de `tx` y ausencia de clones: línea 160, `grep -n clone` sin resultados,
  `grep -n "spawn("` una sola coincidencia (línea 81).
- Las cuatro (y solo cuatro) reasignaciones de `mic`/`sistema` a `None` (`grep -n "= None"`: 210,
  216, 257, 268), leídas contra si ya habían capturado audio antes de ese punto.
- Los cuatro sitios de `cerrar_pista` (`grep -n cerrar_pista`: 256, 267, 302, 305, más la
  definición en 325) y que ninguno se llama dos veces sobre la misma pista.
- El `match` de `cerrar_pista` (325-339) brazo por brazo contra el de `procesar_paquetes`
  (385-391), y contra el `cerrar_pista` anterior a esta vuelta citado en la sección «Segunda
  vuelta» de este documento: los brazos que ya existían no cambiaron de comportamiento.
- `alguna_pista_sigue_viva`: un solo sitio de definición, un solo sitio de llamada de producción,
  cuatro asserts en tres pruebas (`grep -rn`). Regla de la réplica: las pruebas llaman a la
  función real, no a una copia.
- Que `wasapi_src.rs` no duplica la decisión de `alguna_pista_sigue_viva`: repasé el bucle
  completo buscando un segundo punto de salida por falta de pistas -- solo está el de la línea
  281.
- Ancho de línea, caracteres Unicode, no bytes (script propio en Python, `io.open` con
  `encoding=utf-8`, no `wc -c`, recalculado por mí, no copiado del HANDOFF): máximo 98 caracteres
  en `wasapi_src.rs`, 99 en `sincronia.rs`, ninguna línea por encima de 100 en ninguno de los dos
  archivos completos. Ninguno usa los caracteres que producen falsos positivos al contar bytes
  (guion largo, rayas de caja, signo de interrogación de apertura), así que no hay ninguna línea
  donde el conteo por bytes y por caracteres discrepe.
- Balance de llaves y paréntesis, recalculado por mí: `wasapi_src.rs` 132/132 llaves, 309/309
  paréntesis; `sincronia.rs` 36/36 llaves, 178/178 paréntesis. Coincide con lo que declara el
  HANDOFF, verificado de forma independiente, no copiado.
- Los tres comentarios nuevos de esta vuelta (`wasapi_src.rs:241-249, 273-280, 312-324`) contra el
  código que acompañan, línea por línea: ninguno afirma algo que el código no haga. El de 241-249
  describe el caso de una pista (sin cambios, sigue siendo cierto); el de 273-280 describe con
  precisión el caso de las dos pistas y por qué la política vive en `sincronia.rs`; el doc-comment
  de `cerrar_pista` (312-324) describe el registro del error de `vaciar()`, que es justo lo que
  hace el `match` de debajo.
- Clippy, razonado sin ejecutarlo. El `match` de tres brazos no es reducible a `if let`; el
  `Err(e) => tracing::warn!(...)` sin llaves dentro de un brazo repite el patrón ya usado en
  `procesar_paquetes` unas líneas más arriba, en el mismo archivo; la ruta completa
  `crate::sincronia::alguna_pista_sigue_viva(...)` en vez de importar la función replica el
  estilo ya usado para `crate::sincronia::pistas_a_grabar(...)` seis líneas más arriba (línea
  220), no es una inconsistencia nueva. No se añadió ningún `use` nuevo que pudiera quedar sin
  usar.

### 5. Qué no pude verificar y qué haría falta

- Todo lo que dependa de compilar: `cargo fmt`, `cargo check -p dictar-audio --target
  x86_64-pc-windows-msvc`, `cargo clippy -p dictar-audio --all-targets -- -D warnings`, `cargo
  test -p dictar-audio`. B-1 sigue activo, reconfirmado en esta sesión.
- El formato exacto que dejaría `rustfmt` en la llamada de `tracing::warn!` partida dentro de
  `cerrar_pista` (líneas 331-334). Razonado por analogía con el resto del archivo, no confirmado
  -- mismo límite que ya señala el HANDOFF.
- El comportamiento real en Windows del escenario de v2-1 (dos `IAudioClient` fallando en la
  misma iteración del bucle, o en iteraciones consecutivas). No hay forma de forzarlo sin
  hardware y sin dos dispositivos que desconectar durante una captura activa.
- Si `Remuestreador::vaciar()` falla alguna vez en la práctica (v2-2). Mismo límite señalado en
  la segunda vuelta: haría falta instrumentar `rubato::FastFixedIn` o forzar un estado interno
  inconsistente. El brazo `Err` de `cerrar_pista` queda sin ejercitar por ninguna prueba -- no lo
  elevo a hallazgo porque no hay ningún escenario concreto y reproducible que lo dispare con las
  herramientas de este entorno.

Revisor-codigo · 2026-09-03
