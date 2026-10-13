# HANDOFF — AV-1 «Que la grabación siga al dispositivo de audio activo»

Escrito por quien implementó, al entregar.

> **Para los revisores:** esto es una **declaración**, no evidencia. Todo lo que dice acá está
> por verificarse. Ver `protocolo.md`, regla 1.

## Estado

**Parcial.** El código de D1–D5 está escrito, compila, pasa `cargo test -p dictar-audio` (82
pruebas) y el check de FreeBSD. **No está verificado en ejecución**: el ejecutable nuevo del ejemplo
quedó bloqueado por Smart App Control (os error 4551) y la máquina tiene un solo dispositivo de
salida y uno de entrada, así que tampoco se podía cambiar el dispositivo por defecto. Faltan: Paso 0,
pruebas manuales (a), (b), (c) y la comparación de (d) contra el código nuevo. Guion para el PO más
abajo.

Clippy con `-D warnings` falla por **un error que ya estaba en `lib.rs:304`** (ver «Comandos»),
archivo fuera del PLAN. En los archivos tocados no queda ningún aviso.

## Archivos tocados

| Ruta | Qué se hizo | ¿Estaba en el PLAN? |
|---|---|---|
| `core/audio-capture/src/sincronia.rs` | `EstadoPista`, `ControlPista` (máquina de estados pura), `Veredicto`, `debe_terminar` (sustituye a `alguna_pista_sigue_viva`, borrada), `intervalo_de_reintento`, `PistaCapturada::continuar` (`nueva` delega en ella), `ControlPista::relleno_final`; pruebas | sí |
| `core/audio-capture/src/wasapi_src.rs` | Bucle reescrito sobre `Pista`/`ControlPista`: sondeo D1, reapertura, drenaje y medición D4, relleno final D3, registros D5; `abrir_cliente` devuelve `FalloApertura` y el id del dispositivo; `id_de_dispositivo` factorizado | sí |
| `core/audio-capture/src/aaudio_src.rs` | Solo el nombre en el comentario (`alguna_pista_sigue_viva` → `debe_terminar`) | sí (condicional, se cumplía) |
| `core/api/examples/wasapi_loopback_eval.rs` | Modo `--seguir-dispositivo <dir> [segundos] [--tono]` | sí |

`Cargo.toml` no se tocó. Nada más, ni `lib.rs` ni `grabacion.rs`.

## Qué hice por cada decisión

- **D1.** `ControlPista::sondear(ahora, closure)`: gatea a 1 s (`>=`), el closure COM solo se invoca
  si toca. El closure llama a `id_de_endpoint_por_defecto` (reutilizada; solo se factorizó su cola
  en `id_de_dispositivo`) con `eRender`/`eCapture`, rol `eConsole`. Error del sondeo: devuelve
  `Nada` y además **corta la racha** (decisión mía: «2 seguidos» no salta sobre un sondeo que no
  supo responder). Confirmación a 2 sondeos seguidos con el mismo id; enfriamiento de 5 s desde la
  última apertura, solo para pistas Viva. El id comparado es el del `IMMDevice` realmente abierto
  (se guarda en `CapturaAbierta`), no una segunda consulta. Limitación `eConsole` escrita en la
  cabecera de `wasapi_src.rs`. Primer fallo de una racha de sondeos fallidos: `warn!` una vez.
- **D2.** Estados y tabla del PLAN implementados. Calendario: 2 s si lleva `< 60 s` en Reabriendo,
  5 s después. El primer intento tras perder el dispositivo es inmediato (`ultimo_intento: None`),
  también en el camino de D1 (cierre y apertura en la misma vuelta). Pista pedida que no abrió al
  arrancar: `Reabriendo` con contador 0. Fallo de formato (`leer_formato`) o de remuestreador:
  `FalloApertura::Permanente` → `Ausente`. `Ausente` vuelve por D1 (id distinto de `id_fallido`,
  mismos 2 sondeos, sin enfriamiento). `empezar_reapertura` es idempotente (un cierre por evento) y
  el contador se lee tras `cerrar_pista` (que vacía).
- **D3.** `PistaCapturada::continuar`; contador en el estado mientras no hay pista; al detener,
  `relleno_final` para toda pista Reabriendo/Ausente hasta `fin_ms`. No se rellena una pista Viva
  (comportamiento previo, ver Deuda).
- **D4.** `reabrir` drena la otra pista con `procesar_paquetes` antes de abrir; `abrir_y_arrancar`
  mide `Activate`+`GetMixFormat`+`Initialize`+`Start` y lo registra como `apertura_ms`. No se movió
  nada a otro hilo. **No hay mediciones** (ver abajo).
- **D5.** Un `warn!` al entrar en Reabriendo (lectura fallida o cambio de dispositivo), un `info!`
  «pista reabierta» al salir con `id_anterior`, `id_nuevo`, `motivo`, `apertura_ms`,
  `muestras_ya_emitidas`, `sin_pista_ms`; entre medias `warn!` como mucho cada 30 s
  (`toca_avisar`); cada reintento fallido en `debug!`; fallo permanente con `error!` y causa propia.
- **Sin cambio en `grabacion.rs`**, según el PLAN.

## Comandos para reproducir

Entorno: Git Bash con `source entorno-msvc.sh` y
`CARGO_TARGET_DIR=D:\dev_v1\dictar_ia\target` (target compartido aprobado por el PO).

```
cargo test -p dictar-audio
cargo clippy -p dictar-audio --all-targets -- -D warnings
cargo check --target x86_64-unknown-freebsd -p dictar-audio --all-targets
rustfmt --edition 2021 --check core/audio-capture/src/wasapi_src.rs core/audio-capture/src/sincronia.rs core/api/examples/wasapi_loopback_eval.rs
cargo build --release -p dictar-api --example wasapi_loopback_eval
```

**Resultados reales:**

- `cargo test -p dictar-audio`: **exit 0**, `test result: ok. 82 passed; 0 failed`.
- `cargo check --target x86_64-unknown-freebsd -p dictar-audio --all-targets`: **exit 0**.
- `rustfmt --check` de los tres archivos: **exit 0**.
- `cargo build --release ... --example wasapi_loopback_eval`: **exit 0** (17 min 58 s).
- `cargo clippy -p dictar-audio --all-targets -- -D warnings`: **exit 101**. Único error restante:
  `core\audio-capture\src\lib.rs:304:37 clippy::unnecessary_cast (UMBRAL_RMS as f64)`, en un archivo
  que no toqué (`git diff 73e141e -- core/audio-capture/src/lib.rs` vacío) y que no está en el
  PLAN; lo destapa el clippy de rustc 1.98.1. Un error propio (`needless_range_loop` en el bucle
  del sondeo) sí lo había y lo corregí.
- No corrí `cargo fmt --all`, ni `cargo test --workspace`, ni nada de Flutter.

Nota de entorno: en la copia nueva, Smart App Control bloqueaba build scripts y proc-macros recién
enlazados (os error 4551). **Antes** de la instrucción de usar el target compartido, yo lo rodeé
borrando el paquete bloqueado y reenlazando (script `cb.sh` en mi scratchpad) para compilar en
`D:\dev_v1\dictar_ia-av1\target`; ahí se obtuvo el binario **baseline** (código de 73e141e + el
ejemplo nuevo), que sí se ejecutó. Tras la instrucción no volví a rodear nada.

## Criterios de aceptación

| AC | Prueba que lo cubre | Estado |
|---|---|---|
| 1. Pruebas sin COM | En `sincronia.rs`: `debe_terminar` 16 combinaciones con resultado literal por fila; transiciones (error→Reabriendo con contador tras vaciar, idempotencia, Reabriendo→Viva, intentos fallidos conservan contador, permanente→Ausente, Ausente→Reabriendo por D1, arranque sin abrir); margen D1 (un solo sondeo, mismo id 2 veces, ids distintos, sondeo con error, gating de 1 s, enfriamiento 5 s en sus fronteras); calendario 2 s/5 s y frontera de 60 s y 30 s; `continuar` (contador, hueco de N ms = N ms de silencio, primer paquete sin salida del remuestreador a 48 kHz); relleno final. | Cumplido (82 pasan) |
| 2. clippy y FreeBSD | FreeBSD exit 0. Clippy: ver arriba | **Parcial**: falla por `lib.rs:304`, previo y ajeno |
| 3. Manuales (a) (b) (c) | Requieren un 2.º dispositivo o deshabilitar uno (administrador) | **NO VERIFICABLE** |
| 3(d). Sin cambios, 60 s, contra el respaldo | Solo se ejecutó el baseline (abajo). El binario nuevo está bloqueado por SAC | **NO VERIFICABLE** (falta la mitad) |
| 4. Tiempos de apertura D4 | Los registra el log (`apertura_ms`); sin ejecución del binario nuevo no hay cifra | **NO VERIFICABLE** |
| Paso 0 | Requiere cambiar la salida por defecto | **NO MEDIDO** |

### Sobre las pruebas, regla de la réplica

Todas llaman a producción: `ControlPista`, `debe_terminar`, `intervalo_de_reintento`,
`PistaCapturada::{continuar,procesar_paquete,vaciar}`. No hay tipos ni funciones que solo use la
prueba (los helpers `viva()/reabriendo()/ausente()/seg()/sondeo()` solo construyen entradas). Lo
**no cubierto por ninguna prueba**: todo `wasapi_src.rs` (COM, bucle, `reabrir`, cableado de
`soltar_para_reabrir`), porque no se puede ejercitar sin hardware; ahí el único control es el
compilador.

**Inversión declarada de pruebas** (PLAN): se borraron `una_pista_muerta_no_frena_a_la_otra`,
`con_las_dos_pistas_vivas_la_sesion_sigue`, `sin_ninguna_pista_viva_el_bucle_termina` y
`una_sesion_de_una_sola_pista_termina_si_esa_pista_muere`, y la función
`alguna_pista_sigue_viva`. Sustitutas: `la_sesion_solo_termina_sola_si_todas_las_pistas_pedidas_estan_ausentes`,
`una_sesion_de_una_sola_pista_no_termina_aunque_esa_pista_pierda_su_dispositivo` y
`sin_pistas_pedidas_no_hay_nada_que_terminar`. Esa segunda fija lo contrario que la antigua.

## Mediciones (lo poco que hay)

Baseline (código de 73e141e), `--seguir-dispositivo ... 60 --tono`, un solo dispositivo, sin tocar
nada, RMS del sistema ≈ 0,283 constante durante los 60 s:

```
sesion_ms=61995
system.wav duracion_ms=60550 muestras=968810
mic.wav    duracion_ms=61148 muestras=978383
mayor_intervalo_entre_tramas_ms sistema=2054 mic=1931
```

Ojo: el baseline ya muestra un hueco de ~2 s entre tramas y WAV más cortos que la sesión (el
ejemplo mide `sesion_ms` hasta que `detener` vuelve, y las pistas Viva no se rellenan al final).
La máquina estaba compilando varios proyectos a la vez; no lo atribuyo a nada concreto. Esto afecta
a cómo se debe leer el criterio «duración = sesión ± 0,5 s»: con el baseline, mic queda 0,85 s por
debajo de `sesion_ms` y sistema 1,45 s. Hay que decidir si el criterio compara contra `sesion_ms`
del ejemplo o contra la duración de la captura.

## Invariantes del producto

| Invariante | Cómo se respeta |
|---|---|
| 1. Escribir antes de procesar | Sin cambio: el canal sigue entregando tramas a quien escribe. Al soltar una pista se vacía el remuestreador antes de cerrar (`cerrar_pista`) |
| 2. Dos pistas, nunca mezcla | Cada `Pista` conserva su `Track`; el relleno final es por pista |
| 3. 16 kHz mono f32 en el origen | `continuar` reconstruye el remuestreador con la frecuencia del dispositivo nuevo; el relleno es a 16 kHz |
| 4. Firmas públicas sin `cfg` | `iniciar`, `dispositivos` sin cambio. `sincronia` no tiene `cfg` y ahora exporta más API pública, igual en todas las plataformas |

## Decisiones que se apartan del PLAN

1. **`debe_terminar`**: el PLAN dice «todas Ausente y ninguna puede volver (no hay id por defecto
   distinto que probar)». El estado no contiene si hay otro id disponible, y la firma pedida
   (`&[EstadoPista]`) no admite más datos. Lo implementé como: hay ≥1 pista pedida y **todas las
   pedidas están Ausente** (las NoPedida se ignoran). Consecuencia: si ambas pistas fallan de forma
   permanente (formato no soportado), la sesión sí termina sola y el canal se cierra, sin dar a D1
   la ocasión de ver otro dispositivo. Reabriendo nunca termina. **Pido que el orquestador confirme
   esto o pida variante** (p. ej. no terminar nunca).
2. Un error de sondeo corta la racha de 2 (el PLAN solo dice «no se cambia nada»).
3. El `info!` de salida de Reabriendo es `info!`, no `warn!` (D5 dice «uno al salir»; elegí el nivel
   por ser una recuperación). La entrada sí es `warn!`/`error!`.
4. `ultimo_intento` es `Option<Duration>` (None = intentar ya) en vez de `Duration`, para que la
   reapertura tras D1 o tras un error sea inmediata.
5. El ejemplo lleva un `Subscriber` de `tracing` escrito a mano porque `dictar-api` no depende de
   `tracing-subscriber` y no podía tocar el `Cargo.toml`. Solo entiende `RUST_LOG` con
   `dictar_audio=debug|trace`; el resto va a nivel INFO. Es código de ejemplo, no de producción.
6. El ejemplo añade `--tono` (reproduce un tono de 440 Hz por la salida por defecto) para poder
   hacer (d) sin navegador. No estaba en el PLAN, pero es del mismo archivo.
7. Al arrancar, cada pista se abre **y arranca** seguida (antes se abrían las dos y luego se
   arrancaban). Equivale en efecto; el reloj sigue naciendo después de ambas.

## Lo que NO pude verificar

- **El binario nuevo nunca se ejecutó.** `wasapi_loopback_eval.exe` recién enlazado en el target
  compartido da os error 4551 (Smart App Control) tanto desde Git Bash como desde PowerShell. No lo
  rodeé, por la instrucción del coordinador. Por eso no hay ninguna comprobación en ejecución de
  nada de `wasapi_src.rs`: ni que no haya transiciones espurias en 60 s sin cambios, ni (d) contra
  el baseline.
- **Paso 0** (qué hace el cliente viejo al cambiar el default): no medido. La máquina tiene un solo
  dispositivo de salida activo (Speaker Realtek) y un solo micrófono (Intel Smart Sound); el HDMI
  (EPSON PJ) aparece desconectado. No hay a qué cambiar, y no soy administrador (deshabilitar un
  dispositivo lo exige). **La premisa del PLAN sigue sin medir.**
- **(a), (b), (c)**: ídem.
- **Mediciones D4 (`apertura_ms`)**: ninguna.
- No se probó contra Bluetooth ni con `eCommunications`.
- Que el cableado de `wasapi_src.rs` (bucle, `reabrir`, `soltar_para_reabrir`) haga lo que las
  pruebas de `ControlPista` suponen: solo lo ha visto el compilador.

### Guion para el PO (cuando pueda)

Preparación (Git Bash, en `D:\dev_v1\dictar_ia-av1`; el ejecutable nuevo hay que reconstruirlo o
firmarlo/permitirlo porque SAC lo bloquea):
```
source entorno-msvc.sh
export CARGO_TARGET_DIR='D:\dev_v1\dictar_ia\target'
cargo build --release -p dictar-api --example wasapi_loopback_eval
export RUST_LOG=dictar_audio=debug,info
EXE=D:/dev_v1/dictar_ia/target/release/examples/wasapi_loopback_eval.exe
```
Baseline para comparar (código de 73e141e): el ejecutable ya compilado está en
`C:\Users\naunf\AppData\Local\Temp\claude\d--dev-v1-dictar-ia\55361fc4-d84a-407c-8bcf-7f3856406244\scratchpad\base\`
(junto a sus DLL).

- **Paso 0** (con el **baseline**, dos salidas disponibles, p. ej. altavoces y HDMI/auriculares):
  `base/wasapi_loopback_eval.exe --seguir-dispositivo out0 60 --tono`. A los 20 s cambiar la salida
  por defecto (Configuración → Sonido → «Elegir dónde reproducir el audio») y a los 40 s volver. En
  la salida, columna `sistema: N tramas rms=`: si pasa a 0 tramas, el cliente viejo deja de
  entregar; si sigue con tramas y rms≈0, entrega silencio; si sigue con rms≈0,28, Windows lo movió.
  Anotar cuál de las tres. Nota: el tono lo reproduce el propio ejemplo y su reproductor puede
  seguir o no al cambio; usar mejor Chrome con YouTube en «Predeterminado» y **sin** `--tono`.
- **(a)**: `$EXE --seguir-dispositivo outa 60` con audio continuo en Chrome; cambiar la salida a los
  20 s y volver a los 40 s. Pasa si: tras cada cambio, la primera fila de la tabla RMS de
  `system.wav` con valor > 0,003 llega ≤ 3 s después; duración de `system.wav` = sesión ± 0,5 s; y
  el log contiene exactamente 2 líneas `pista reabierta` del bucle del sistema.
- **(b)**: con un solo dispositivo, en un PowerShell **de administrador** a los 20 s:
  `Get-PnpDevice -Class AudioEndpoint -FriendlyName "Speaker*" | Disable-PnpDevice -Confirm:$false`
  y a los 35 s `... | Enable-PnpDevice -Confirm:$false` (o por el Panel de sonido). Pasa si el log
  muestra `falló la lectura` → `pista reabierta`, la sesión no termina y `system.wav` ≈ sesión.
- **(c)**: igual que (a) pero cambiando el micrófono por defecto; mirar `mic.wav` (con un
  micrófono que capte algo, p. ej. hablando).
- **(d)**: 60 s sin tocar nada con `base/...exe` y con `$EXE`, mismo comando
  `--seguir-dispositivo outX 60 --tono`; comparar `system.wav`/`mic.wav` y comprobar que en el log
  del nuevo no aparece ninguna línea `pista reabierta` ni `el dispositivo por defecto cambió`.
- **D4**: de cualquiera de los logs, `grep apertura_ms` y anotar mínimo y máximo y en qué
  dispositivo; si alguno pasa de 150 ms, informar al orquestador (no mover a otro hilo).

## Deuda que dejo

- `clippy::unnecessary_cast` en `core/audio-capture/src/lib.rs:304` (previo): una línea,
  `UMBRAL_RMS as f64` → `UMBRAL_RMS`. Rompe el CI de clippy con este toolchain.
- Smart App Control bloquea ejecutables recién enlazados en esta máquina (relacionado con T-17):
  cualquier prueba manual de este tipo necesita una vía para ejecutar lo recién compilado.
- Una pista **Viva** de loopback que termina en silencio sigue sin rellenarse hasta `fin_ms` (solo
  se rellenan las no vivas). Previo a AV-1.
- `debe_terminar` (decisión 1 arriba) a confirmar.
- Mientras ninguna pista tiene dispositivo, el canal sigue abierto sin tramas: depende de AV-2 para
  avisar a la persona. AV-1 no debería integrarse sin AV-2.

## Ronda 2

Esta sección **sustituye** lo que arriba diga lo contrario sobre `debe_terminar`, el criterio de
duración y «el binario nuevo nunca se ejecutó».

### Decisión 1 aplicada: la sesión nunca termina sola

- `debe_terminar` **eliminada** (quedaba trivial: siempre `false`), junto con su uso en el bucle de
  `wasapi_src.rs`; en su lugar hay un comentario que explica por qué ese bucle no tiene salida por
  estado de dispositivos. La sesión solo termina por `detener()`. `Ausente` sigue vigilando D1.
- Se borraron las tres pruebas de `debe_terminar` y se escribió
  `ninguna_combinacion_de_estados_termina_la_sesion_y_la_ausente_sigue_vigilando`: recorre las 16
  combinaciones de estados de dos pistas (construidas por los caminos de producción) y comprueba, con
  `ControlPista::sondear`, que `Viva` y `Ausente` piden reabrir ante un id distinto estable y que
  `Reabriendo` y `NoPedida` no. Ojo: ya no hay una función de terminación que probar; esta prueba
  fija lo que hace posible que la sesión siga (en particular que dos `Ausente` pueden volver). Que
  el bucle no tenga `break` por estado no lo puede probar nadie sin COM.
- Comentario de `aaudio_src.rs` ajustado (ya no nombra una función que no existe).

Resultados (con `CARGO_TARGET_DIR=D:\dev_v1\dictar_ia\target`):
- `cargo test -p dictar-audio`: **exit 0**, 80 passed, 0 failed.
- `rustfmt --check` de `wasapi_src.rs`, `sincronia.rs`, `aaudio_src.rs`: exit 0.
- `cargo check --target x86_64-unknown-freebsd -p dictar-audio --all-targets`: **exit 0**.
- `cargo clippy -p dictar-audio --all-targets -- -D warnings`: **exit 101**, único error el
  previo de `lib.rs:304` (`unnecessary_cast`); ninguno en mis archivos.

### Decisión 3: ejecución en perfil de depuración

`cargo run -p dictar-api --example wasapi_loopback_eval -- --seguir-dispositivo <dir> N --tono`
(sin `--release`, `RUST_LOG=dictar_audio=debug,info`) **sí corrió: exit 0**, sin 4551. Por tanto el
código nuevo se ejecutó. Una prueba corta de 8 s y la de 60 s.

### Prueba (d), 60 s sin tocar nada, un solo dispositivo, `--tono`

Mismo comando para ambos, uno tras otro. Baseline = ejecutable release de 73e141e; nuevo = debug.

| | baseline | nuevo | diferencia |
|---|---|---|---|
| `system.wav` ms (muestras) | 60353 (965659) | 60193 (963088) | 0,16 s |
| `mic.wav` ms (muestras) | 60606 (969710) | 60687 (970997) | 0,08 s |
| `sesion_ms` | 61313 | 62078 | |
| mayor intervalo entre tramas sist./mic (ms) | 1362 / 1313 | 2094 / 2061 | |

Criterio relativo (±0,2 s): **cumplido** en las dos pistas. Registro del nuevo: **ninguna** línea
`pista reabierta`, `el dispositivo por defecto cambió` ni `falló la lectura` (conteo 0); solo
`pista abierta` ×2, `captura iniciada` y `detenida`. RMS del sistema ≈ 0,283 todo el tiempo.
Salidas completas en el scratchpad (`d_new60.txt`, `d_base60.txt`). Una sola repetición por lado:
la varianza entre ejecuciones del propio baseline (60550 en la ronda 1 frente a 60353 ahora) es
del orden de 0,2 s, así que el margen es justo.

Observación: los huecos de ~2 s entre tramas aparecen en baseline y en nuevo (1,3–2,1 s); no los
causa AV-1, pero no sé qué los causa (máquina cargada compilando, o el arranque del reproductor).

### Tiempos de apertura (D4)

Solo en el arranque, sin cambio de dispositivo (no se pudo provocar ninguno):

| Pista | Dispositivo | `apertura_ms` |
|---|---|---|
| micrófono | `{0.0.1.00000000}.{17bd4290-...}` (Intel Smart Sound) | **2472** (prueba de 60 s); ver nota |
| bucle del sistema | `{0.0.0.00000000}.{a8bff37c-...}` (Speaker Realtek) | **62** |

Esto **supera los 150 ms de D4** en el micrófono (y es una apertura de arranque, no de reapertura).
Según el plan no muevo nada a otro hilo: lo informo para que el orquestador decida. Si una
reapertura del micrófono tardara igual, la pista del sistema quedaría ~2,5 s sin leerse, y su búfer
es de 200 ms (se perdería audio del sistema). El drenaje previo de D4 solo cubre lo ya disponible.
Prueba corta de 8 s (mismo equipo): micrófono 694 ms, sistema 188 ms. Es decir, dos mediciones por
pista: micrófono 694 y 2472 ms, sistema 62 y 188 ms. Mínimo 62 ms (sistema), máximo 2472 ms
(micrófono). Todas en depuración y con la máquina cargada, así que no son concluyentes, pero el
micrófono supera 150 ms las dos veces.

### Decisión 2: guion del PO actualizado

Criterios de duración (sustituyen a «sesión ± 0,5 s»), con el baseline ejecutado en la misma
prueba:
- (d): duración de cada WAV nuevo = la del baseline ± 0,2 s.
- (a)/(b): el faltante respecto de `sesion_ms` de cada WAV no puede superar el del baseline
  (`sesion_ms - duracion_ms` en la misma prueba) + 0,3 s.
El resto del guion de la sección anterior no cambia, salvo que el ejecutable nuevo ya se puede
ejecutar con `cargo run -p dictar-api --example wasapi_loopback_eval -- ...` en depuración.

### Sigue sin verificar

Paso 0, (a), (b), (c): un solo dispositivo de salida y de entrada, y sin administrador. Nada del
camino de reapertura se ha ejecutado nunca (ni siquiera `reabrir`), solo el de arranque y parada.
El incidente de haber rodeado el bloqueo en mi target (declarado arriba) queda como tal.

## Ronda 3 (D4: búfer de 3 s, opción A)

### Qué cambió (`wasapi_src.rs`, solo ese archivo)

- `DURACION_BUFFER_HNS` pasa de 200 ms a **3 000 ms**, con comentario del porqué (aperturas del
  micrófono medidas de hasta 2,5 s; abrir en el hilo único deja sin leer la otra pista; un búfer
  mayor no añade latencia porque el sondeo sigue siendo de 10 ms).
- Tras `Initialize` se lee `GetBufferSize`; se guarda en `CapturaAbierta` (`buffer_tramas`,
  `buffer_ms`) y se registra en `pista abierta` y `pista reabierta`. Si da menos de 2 500 ms
  (`BUFFER_MINIMO_MS`) se emite un `warn!`. **La opción B (hilo auxiliar) no se implementó**: no hizo
  falta.

### Resultados

- `cargo test -p dictar-audio`: **exit 0**, 80 passed.
- `rustfmt --check` (`wasapi_src.rs`, `sincronia.rs`): exit 0.
- `cargo check --target x86_64-unknown-freebsd -p dictar-audio --all-targets`: **exit 0**.
- `cargo clippy -p dictar-audio --all-targets -- -D warnings`: **exit 101**, solo el error
  preexistente de `lib.rs:304`.

### GetBufferSize (Windows concedió lo pedido, en micrófono y en sistema)

`buffer_tramas=144000`, **`buffer_ms=3000`** en las dos pistas, en las tres ejecuciones (el log lo
imprime con ese valor para ambas; 144000 tramas a 48 kHz). Cumple ≥ 2,5 s.

### Prueba (d) con el búfer nuevo, 60 s sin tocar nada, `--tono`, nuevo (debug) frente a baseline (release), uno tras otro

| Par | system nuevo / base | dif. | mic nuevo / base | dif. | apertura_ms mic / sist. (nuevo) |
|---|---|---|---|---|---|
| 1 | 60258 / 60279 | 0,02 s | 60989 / 60661 | **0,33 s** | 1266 / 732 |
| 2 | 60114 / 60146 | 0,03 s | 60232 / 60210 | 0,02 s | 414 / 50 |
| 3 | 60174 / 60103 | 0,07 s | 60218 / 60181 | 0,04 s | 305 / 34 |

Todos los ejecutables salieron con exit 0 y no hubo ninguna línea de reapertura, cambio de
dispositivo ni fallo de lectura en los logs nuevos. Criterio ±0,2 s: **cumplido en los pares 2 y 3;
el par 1 lo incumple en el micrófono (0,33 s)**. Ese par es también el de mayor carga de la máquina
(huecos entre tramas de 3,4 s en el nuevo y 2,0 s en el baseline, aperturas de 1,3 s y 0,7 s); en
rondas anteriores el baseline ya variaba 0,2–0,5 s entre ejecuciones en esa situación. No puedo
afirmar que el búfer de 3 s no influya en esa diferencia con una sola repetición cargada; lo declaro
como incumplimiento observado, no como ruido demostrado.

Mediciones de apertura acumuladas (rondas 2 y 3, todas de arranque): micrófono 305, 414, 694, 1266,
2472 ms; sistema 34, 50, 62, 188, 732 ms. Dependen mucho de la carga. Ninguna reapertura real se ha
medido ni provocado (no se intentó por medios no permitidos).

### Sigue sin verificar

Que con el búfer de 3 s una reapertura real del micrófono no pierda datos del sistema (requiere
provocar una reapertura: Paso 0, (a), (b), (c) siguen pendientes del guion del PO).

## Ronda 4 (correcciones del REVIEW consolidado)

Archivos: `sincronia.rs` y `wasapi_src.rs` (ambos del plan). `CARGO_TARGET_DIR=D:\dev_v1\dictar_ia\target`,
`CARGO_INCREMENTAL=0`, sin targets nuevos, sin rodear nada.

### Qué se hizo

- **B-1 (fechar por captura).**
  - `GetBuffer` recibe ahora `pu64QPCPosition`. Nuevo `sincronia::RelojCaptura::fechar(qpc, tramas,
    frecuencia, leido_ms)`: fin del bloque = QPC (100 ns → ms, `qpc_a_ms`) + duración del bloque + el
    **mínimo desfase observado** «lectura − captura» (compartido por las dos pistas y las
    reaperturas), recortado a ≤ instante de lectura. Elegí calibrar con el mínimo porque
    `Win32_System_Performance` (`QueryPerformanceCounter`) no está en las features de `Cargo.toml` y
    no podía tocarlo. Es una heurística: el primer bloque de la sesión se fecha en su lectura (sin
    historial) y, si el QPC de WASAPI no fuera coherente con el reloj de lectura, el efecto sería un
    desplazamiento constante de a lo sumo la latencia mínima de lectura.
  - Respaldo cuando `pu64QPCPosition == 0` o la bandera `AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR` (0x4):
    `fecha_por_pendientes(leido_ms, GetCurrentPadding − tramas_del_bloque, frecuencia)`.
  - **Orden de arranque restaurado**: abrir las dos pistas (`abrir_cliente`, sin `Start`) →
    `Start` de las dos (`arrancar`) → `inicio`. El fallo de `Start` sigue siendo transitorio y se
    registra (`registrar_arranque`). La reapertura sigue abriendo y arrancando en un paso.
  - Al reabrir, el hueco se rellena de inmediato con `PistaCapturada::rellenar_hasta` (tramas de
    1 s, hasta el instante en que empezó el intento), de modo que el primer paquete solo cubre la
    apertura.
- **I-1.** Corregidos los cuatro comentarios (búfer de 3 s y fechado por captura; el `sleep` de
  10 ms frente a una ventana de 3 s; doc de `reabrir` con «ha llegado a tardar 2,5 s»; doc de
  `relleno_final`: una pista viva no se rellena y un loopback en silencio sale corto).
- **I-2.** `procesar_paquetes` entrega la trama (`tx.send`) **antes** de propagar el error de
  `ReleaseBuffer`.
- **I-3.** `empezar_reapertura` trata como intento fallido (`ultimo_intento = Some(ahora)`) el fallo
  de una pista abierta hace menos de 5 s; una que servía desde hace rato sigue reintentando de
  inmediato.
- **I-4.** `reabrir` pasa `inicio.elapsed()` **posterior** al intento a `intento_fallido`.
- **I-5.** `relleno_final` devuelve `Vec<AudioFrame>` en tramas de ≤ 1 s (`silencio_en_tramas`,
  `TRAMA_DE_RELLENO_MAX`); `wasapi_src` las envía una a una.

### Pruebas nuevas o cambiadas (todas llaman a producción)

- B-1: `la_posicion_qpc_de_wasapi_viene_en_cien_nanosegundos`,
  `sin_qpc_la_fecha_resta_lo_que_aun_esta_pendiente_en_el_bufer`,
  `el_primer_bloque_se_fecha_en_el_instante_de_su_lectura`,
  `un_bloque_leido_tarde_se_fecha_en_su_captura_y_no_en_su_lectura` y
  `una_rafaga_tras_un_bloqueo_no_adelanta_la_pista` (ráfaga de 2,5 s con `RelojCaptura` +
  `PistaCapturada::procesar_paquete`; antes de B-1 la misma ráfaga con la fecha de lectura dejaba la
  pista ≈2,48 s adelantada, medido por el revisor).
- I-5: `el_relleno_final_sale_en_tramas_de_como_mucho_un_segundo`,
  `el_relleno_en_tramas_conserva_la_cola_corta`,
  `rellenar_hasta_al_reabrir_avanza_el_contador_y_acota_las_tramas` (con 10 min en vez de 2 h para no
  reservar cientos de MB en la prueba); las de relleno final existentes ahora suman las tramas.
- I-3: `una_apertura_que_falla_al_leer_enseguida_no_reintenta_de_inmediato`,
  `una_pista_que_servia_desde_hace_rato_reintenta_enseguida_al_fallar`.
- Superviviente 1 del verificador:
  `una_pista_ausente_puede_volver_dentro_de_los_cinco_segundos_de_su_ultima_apertura`.
- Tolerancia de 400: la prueba del primer paquete sin salida ya no usa rango; fija que el segundo
  paquete trae exactamente 339 muestras (valor medido; 1 024/3 menos el retardo del filtro) y que el
  contador es 16 000 + 8 000 + 339, con el razonamiento escrito en la prueba.
- **No cubierto por pruebas**: I-2 (orden de `ReleaseBuffer`), I-4 y el cableado de B-1 en
  `procesar_paquetes` (todo COM). Solo lo ha visto el compilador y el (d) de abajo.

### Resultados (cada comando de pruebas, dos veces)

- `cargo test -p dictar-audio`: ejecución 1 **exit 0**, 91 passed; ejecución 2 **exit 0**, 91 passed.
- `rustfmt --check` (`sincronia.rs`, `wasapi_src.rs`, `aaudio_src.rs`): exit 0. Líneas > 100
  caracteres: solo `sincronia.rs:609` (122), preexistente (nota del revisor).
- `cargo check --target x86_64-unknown-freebsd -p dictar-audio --all-targets`: **exit 0**.
- `cargo clippy -p dictar-audio --all-targets -- -D warnings`: **exit 101**, único error el
  preexistente de `lib.rs:304`.

### Prueba (d), 3 pares de 60 s, nuevo (depuración, `cargo run`) frente a baseline (release de 73e141e); todos exit 0

| Par | system nuevo / base (dif.) | mic nuevo / base (dif.) | apertura_ms mic / sist. (nuevo) | log |
|---|---|---|---|---|
| 1 | 60093 / 60114 (0,02 s) | 60349 / 60200 (0,15 s) | 484 / 24 | sin reaperturas |
| 2 | 60065 / 60154 (0,09 s) | 60136 / 60245 (0,11 s) | 287 / 24 | sin reaperturas |
| 3 | 60065 / 60098 (0,03 s) | 60298 / 60164 (0,13 s) | 1874 / 26 | sin reaperturas |

Criterio ±0,2 s: **cumplido en los tres pares y las dos pistas**. `buffer_ms=3000` en todas.
Matices: (1) el micrófono nuevo sale siempre 0,11–0,15 s más largo que el baseline; no sé de dónde
sale ese margen constante (el baseline mide el micrófono contra la lectura, el nuevo contra la
captura), pero cabe en el criterio. (2) No reproduje la condición exacta de B-1 (apertura lenta del
**sistema** con el micrófono ya arrancado): ahora no hay ningún `Start` anterior a esa apertura, y
la única apertura lenta (micrófono, 1,9 s, par 3) tampoco se pasa. Cobertura real del nuevo
fechado en ejecución: solo el camino QPC en lecturas normales; no vi cuándo se usa el respaldo ni
si algún bloque llegó con `TIMESTAMP_ERROR`.

### Sigue sin verificar

Paso 0, (a), (b), (c): un solo dispositivo, sin administrador (guion del PO en la sección anterior).
Ninguna reapertura real se ha ejecutado, ni el relleno `rellenar_hasta` en ejecución, ni
`RelojCaptura` frente a un bloqueo real del hilo (solo en la prueba de ráfaga simulada). Verificación
por mutación de lo nuevo: no hecha.
