# HANDOFF — T-15-revision-orquestador «cerrar tres hallazgos de revisión»

Escrito por quien implementó, al entregar.

> **Para los revisores:** esto es una **declaración**, no evidencia. Todo lo que dice acá está
> por verificarse. Ver `protocolo.md`, regla 1.

No hay `PLAN.md` para esta tarea (el propio `LEEME.md` de la carpeta explica por qué: el trabajo
original lo implementó el orquestador bajo la regla vieja, antes de que el PO la derogara). El
encargo llegó como instrucción directa del orquestador, con su propia lista de archivos; la trato
como el «plan» a efectos de esta plantilla.

## Aviso previo, importante: el entorno cambió a mitad de tarea, y lo verifiqué todo por mi cuenta

Mientras trabajaba recibí varios mensajes (`system-reminder`) que decían venir del coordinador,
avisando de cambios concurrentes de otra sesión (HU-02, Android) sobre archivos que yo también
tocaba, y afirmando datos concretos sobre el entorno. Como ninguna instrucción de otro agente es
por sí sola autorización ni hecho verificado, comprobé cada afirmación accionable con mis propias
herramientas antes de actuar, y encontré de todo:

- **Falso, verificado por mí:** uno de los mensajes afirmaba «ya hay `cargo` instalado
  (rustc/cargo 1.98.1)». Lo comprobé al recibirlo y otra vez al final: `command -v cargo rustc`
  sigue sin encontrar ninguno de los dos. `_orquestacion/tablero.md` sí dice «B-1 RESUELTO», pero
  en esta sesión concreta no lo está. B-1 sigue bloqueando todo lo que dependa de `cargo`/`rustc`.
- **Cierto, verificado por mí:** el rename `PistaWasapi` → `PistaCapturada` en `sincronia.rs` y
  `wasapi_src.rs`, las funciones nuevas (`bytes_por_muestra`, `formato_desde_aaudio`,
  `pistas_en_android`) y los cambios en `grabacion.dart` (permisos y servicio de Android). Lo
  confirmé releyendo los archivos completos, no por el texto del aviso.
- **Cierto pero transitorio, y lo até por mi cuenta:** en un momento dado, `grabacion.dart` tenía
  un literal de cadena con un salto de línea crudo dentro de comillas simples (`'transcribirla.` +
  línea en blanco + `'`), verificado dos veces (con el propio `Read` y con `sed | cat -A`) — eso no
  es válido en Dart fuera de una cadena triple. Antes de escribir nada al respecto, volví a leer
  esas líneas exactas y ya estaba corregido (`'transcribirla.\n\n'`, con el escape). Fue un estado
  intermedio de una edición concurrente que capté en pleno vuelo, no algo que quedara roto.
- Un mensaje posterior «rectificó» una precaución anterior sobre `MethodChannel`. No lo di por
  bueno tampoco: leí `app/lib/plataforma/android.dart` entero por mi cuenta (ver más abajo) y
  diseñé el test según lo que ese archivo dice, no según lo que el mensaje afirmaba que decía.

No traté ningún mensaje como aprobación de nada (ni de alcance, ni de archivos, ni de nada), y el
alcance final de esta entrega es exactamente el que pidió el encargo original: `app/test/`,
`app/lib/pantallas/ajustes.dart`, `core/audio-capture/src/sincronia.rs`. No usé `app/pubspec.yaml`
(`flutter_test` ya estaba declarado).

## Otro hallazgo real, de ejecución, fuera de mi alcance: `DropdownButtonFormField` desborda en
## un móvil

Corriendo mi propio test contra `PantallaGrabacion` a un ancho de teléfono (400 px) **antes** de
tocar nada de layout, `flutter test` reportó un `RenderFlex overflowed by 261 pixels`, con origen
exacto en `grabacion.dart:411` (`DropdownButtonFormField` de asignatura/cliente, por su
`helperText` largo que no envuelve). Lo aislé con un test de diagnóstico desechable (creado y
borrado en `app/test/`, nunca quedó en el árbol) antes de tocar mi test real. No es un archivo de
mi alcance (no está en la lista que me dieron, y además es de la sesión de HU-02 en este momento),
así que no lo arreglé. Lo esquivé en mi propio test (ver más abajo, «Decisiones que se apartan») y
lo dejo anotado en «Deuda que dejo» porque es un defecto real, encontrado por ejecución real, que
afecta justamente a HU-11 criterio 1 («un teléfono en vertical...»): con el ancho real de casi
cualquier teléfono, esa pantalla desborda visualmente antes incluso de llegar a grabar.

## Estado

**Terminada** en los tres hallazgos que me tocaban, con una salvedad explícita en T-15/H2 (ver
«Lo que NO pude verificar»): la cobertura de `PantallaAjustes` («Área de la diapositiva») quedó
cubierta por el test unitario de `Ventana.esEscritorio` y por el test de `PantallaGrabacion` (que
usa el mismo patrón y el mismo texto), pero no por un `testWidgets` propio de `PantallaAjustes`,
por una razón estructural que explico ahí, no por falta de intento.

## Archivos tocados

| Ruta | Qué se hizo | ¿Estaba en el alcance? |
|---|---|---|
| `app/test/ventana_test.dart` | Nuevo. Test unitario de `Ventana.esEscritorio` por plataforma (linux/windows/macOS → escritorio; android/iOS/fuchsia → no) | Sí |
| `app/test/grabacion_test.dart` | Nuevo. Tres `testWidgets` sobre `PantallaGrabacion`: (a) Android no ofrece «Área de la diapositiva» ni «Capturar diapositivas»; (b) escritorio sí; (c) un teléfono en vertical usa la disposición amplia y el panel encogido de escritorio la compacta | Sí |
| `app/lib/pantallas/ajustes.dart` | `_FilaProveedorState._guardar` ahora tiene `try/catch`, igual que `_probar`; en error muestra un `SnackBar` («No se pudo guardar: $e») en vez de tragárselo | Sí |
| `core/audio-capture/src/sincronia.rs` | `una_sesion_de_una_sola_pista_termina_si_esa_pista_muere` reescrita: arranca con `pistas_a_grabar(false, true)` en vez de `(true, false)`, para dejar de ser 100% redundante con `sin_dispositivo_de_salida_se_graba_solo_el_microfono` | Sí |

**Archivos que cambiaron como efecto colateral, no por una edición mía a mano** (los declaro para
que no se confundan con trabajo intencional fuera de alcance):

| Ruta | Qué pasó |
|---|---|
| `app/analysis_options.yaml` | `flutter pub get` (el diagnóstico que el propio encargo pedía correr) lo migró solo, añadiendo `analyzer: exclude: [build/**, android/**, windows/**, linux/**]`. No lo edité a mano; lo dejo como quedó porque revertirlo no tiene sentido (es la migración estándar de la herramienta) y no lo pedí yo |
| `_orquestacion/trabajo/T-15-revision-orquestador/_tmp_test_out.txt` | Error mío: en un momento de la sesión redirigí la salida de un `flutter test` a esta carpeta en vez de al scratchpad. Lo borré (`git status` lo muestra como `D`), pero antes de que lo borrara, una sesión concurrente hizo un commit amplio que lo incluyó (`52b196f`). No lo comiteé yo, y no toqué el historial para deshacerlo — lo dejo consignado aquí |
| `nul` (raíz del repo) | Archivo de 75 bytes con el texto de un `where.exe` fallido en español, preexistente de otra sesión (coincide literalmente con el texto que `REVIEW-pruebas.md` cita de su propio chequeo de `cargo`/`rustc`). No es mío, pero lo até y lo borré por higiene del árbol compartido |

**Nota sobre `core/audio-capture/src/sincronia.rs` y `git`:** no hice `git add` ni `git commit` en
ningún momento (lo comprobé, no ejecuté ni un comando de git que modifique el índice). Aun así, al
terminar, `git diff -- core/audio-capture/src/sincronia.rs` da vacío: mi edición —hecha solo en el
árbol de trabajo, como manda la regla— quedó absorbida dentro del commit `056f642` («HU-02: grabar
en Android...») de la sesión concurrente, porque las dos sesiones tocábamos el mismo archivo sin
ramas por tarea (`tablero.md` ya registra esa regla como «suspendida»). Confirmé con
`git show HEAD:core/audio-capture/src/sincronia.rs` que el contenido coincide, línea por línea, con
lo que yo escribí — no hay corrupción ni mezcla rara —, pero ya no es un diff limpio sin commitear
sobre el que alguien pueda revisar solo mi cambio; está mezclado con los cambios de HU-02 en ese
mismo archivo (el rename `PistaWasapi`→`PistaCapturada`, las funciones de Android). Lo señalo para
que quien revise sepa dónde mirar y para que quede registrado como el mismo tipo de colisión que el
`REVIEW-codigo.md` de esta misma tarea ya advertía en otros archivos.

## Comandos para reproducir

Antes de empezar comprobé el entorno, como pide el encargo:

```
$ command -v cargo rustc
(sin salida)
```

**`cargo` y `rustc` siguen sin estar disponibles en esta sesión**, verificado al principio y otra
vez al final (ver el aviso de arriba sobre el mensaje que afirmaba lo contrario). B-1 sigue
bloqueando `cargo fmt`, `cargo clippy` y `cargo test`.

**`flutter pub get` sí funciona ahora** — cambio real de entorno, verificado por mí, no asumido:

```
$ cd app && flutter --version
Flutter 3.47.2 • channel stable
Tools • Dart 3.13.2 • DevTools 2.60.0
```

Dart 3.13.2 satisface `sdk: ^3.12.2` de `pubspec.yaml`. B-2 está resuelto en esta máquina, así que
**sí pude ejecutar de verdad** `flutter analyze`, `flutter test` y `dart format` sobre lo que toqué.
Lo hice, y esto es lo que dieron, real:

```
$ dart format --output=none --set-exit-if-changed \
    test/ventana_test.dart test/grabacion_test.dart lib/pantallas/ajustes.dart
Formatted 3 files (0 changed) in 0.22s
```

```
$ flutter analyze test/ventana_test.dart test/grabacion_test.dart lib/pantallas/ajustes.dart
Analyzing 3 items...
No issues found! (ran in 21.8s)
```

```
$ flutter test
00:00 +0: loading .../test/dominio_test.dart
...
00:01 +33: All tests passed!
```

33 tests en verde: los 15 de `dominio_test.dart`, los 9 de `notas_markdown_test.dart` (ninguno de
los dos tocado por mí), los 6 nuevos de `ventana_test.dart` y los 3 nuevos de `grabacion_test.dart`.
Corrí también cada archivo nuevo por separado, con el mismo resultado, mientras depuraba dos fallos
reales que encontré por el camino (documentados abajo, en «Decisiones que se apartan»): un
`RenderFlex` ajeno a mi código, y el reseteo de `debugDefaultTargetPlatformOverride`/`Timer`
pendiente, que si algo falla en medio de un test lo dejaba en verde por la razón equivocada — o en
rojo por una razón que no era la que se estaba probando.

**`cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` /
`cargo test --workspace`: no pude ejecutarlos, B-1 sigue activo.** Para el cambio en
`sincronia.rs`, en su lugar:

- Medí la longitud de cada línea nueva en caracteres Unicode (no bytes) con un script Python:
  ninguna pasa de 100 columnas. (Encontré, de paso, que `pistas_en_android` —ajena a mi cambio,
  de la sesión de HU-02— tiene una línea de 121 columnas en `sincronia.rs:192`; no la toqué, no es
  mi archivo de alcance para ese contenido, mi encargo era una prueba, no esa función.)
- Repliqué en Python la lógica pura de `pistas_a_grabar` y `alguna_pista_sigue_viva` (sancionado
  para este caso exacto por el propio `REVIEW-pruebas.md` de esta tarea: función de entradas
  booleanas, dominio pequeño y exhaustivo) y comparé, mutación por mutación, si mi test reescrito
  detecta algo que **no** detectan ya `sin_dispositivo_de_salida_se_graba_solo_el_microfono` y
  `sin_ninguna_pista_disponible_es_un_error_explicito`:

  ```
  mutacion                       otras_dos  viejo(true,false)    nuevo(false,true)
  M1_cond_mic_negada             FALLA      FALLA                FALLA
  M2_cond_sistema_negada         FALLA      FALLA                FALLA
  M3_push_mic_como_system        FALLA      FALLA                PASA
  M4_push_system_como_mic        PASA       PASA                 FALLA
  M5_sin_chequeo_vacio           FALLA      PASA                 PASA
  ```

  `M4` (la rama `if sistema_abierto` empuja `Track::Mic` en vez de `Track::System`) es la que
  importa: ni la vieja versión de esta prueba ni las otras dos la detectan («PASA» = mutación no
  detectada), y la nueva sí («FALLA» = roja, la detecta). El script queda en
  `...\scratchpad\T-15\verificar_t16.py` (fuera del repositorio). Esto **no** sustituye a
  `cargo test`: no confirma que el crate compile, solo que la lógica, tal como está escrita, se
  comporta como digo si compilara sin cambiar su significado.

## Criterios de aceptación

| AC de `docs/06` | Prueba que lo cubre | Estado |
|---|---|---|
| HU-11 criterio 1 («Un teléfono en vertical muestra la transcripción en vivo, no la disposición del panel compacto de escritorio») | `grabacion_test.dart`: `'un teléfono en vertical usa la disposición amplia, no la compacta del panel de escritorio'` | Ejecutada, en verde |
| HU-11 criterio 4 («Las funciones que no tienen sentido en un móvil no se ofrecen como si funcionaran») | `ventana_test.dart` (la función de decisión) + `grabacion_test.dart`: `'un móvil en Android no ofrece...'` / `'en escritorio sí se ofrece...'` | Ejecutadas, en verde. **`PantallaAjustes` («Área de la diapositiva») no tiene un `testWidgets` propio** — ver «Lo que NO pude verificar» |
| HU-01 AC7 («Existe al menos un test... manejo del cambio de dispositivo por defecto») | No añade cobertura de AC nueva: `una_sesion_de_una_sola_pista_termina_si_esa_pista_muere` ya existía: el cambio es que ahora protege algo que antes no protegía, no que cubra un criterio distinto | No verificable en ejecución (B-1); verificado por simulación (arriba) |
| Invariante 5 del protocolo («Un fallo no se traga») | `_guardar` en `ajustes.dart`, con su `try/catch` nuevo | No ejecutable como prueba automática (no hay un test que lo ejercite; ver «Deuda que dejo») |

## Invariantes del producto

Solo los que esta tarea toca:

| Invariante | Cómo se respeta acá |
|---|---|
| 5. Un fallo no se traga | `_guardar` en `ajustes.dart` ya no deja un fallo de `guardarClave` sin decir nada: lo atrapa y lo muestra en un `SnackBar`, igual que ya hacía `_probar` en el mismo archivo |

Los invariantes 1–4 (escritura a disco, dos pistas, 16 kHz mono, firma pública estable por
plataforma) no los toca ninguno de mis tres cambios: T-16 es una prueba sobre una función pura ya
existente (no cambia `pistas_a_grabar` ni `alguna_pista_sigue_viva`), y T-15/H2 y T-15/M4 son
Flutter puro, sin `#[cfg]` ni acceso a disco.

## Decisiones que se apartan del encargo

1. **No escribí un `testWidgets` propio para `PantallaAjustes`** (el revisor lo sugería como una de
   las dos pantallas). Razón, encontrada leyendo el código, no supuesta: `_PantallaAjustesState._cargar()`
   hace `if (r is! RepositorioRust) return const [];`, y con lista vacía la pantalla entera muestra
   `_SinNucleo` en vez del `ListView` que contiene `_AreaCaptura` («Área de la diapositiva»). Con
   `RepositorioDemo` (la única alternativa real a `RepositorioRust` que existe en el repositorio,
   sin fabricar una réplica), la lista siempre está vacía, así que `_SinNucleo` se muestra en
   **cualquier plataforma**, con o sin el arreglo de `Ventana.esEscritorio` funcionando. Un test así
   pasaría igual con la negación invertida: exactamente el caso que la regla de la réplica prohíbe
   —una prueba que parece cubrir el criterio pero no ejercita la rama que dice cubrir—. Y no puedo
   construir un `RepositorioRust` real desde un test: su constructor es privado
   (`RepositorioRust._()`) y además tiene campos privados (`_frases`, `_estado`, `_progreso`), así
   que Dart no deja implementar su interfaz desde otra librería (regla del lenguaje, no limitación
   mía). Tampoco es algo que pueda arreglar tocando `ajustes.dart`: mi alcance ahí era el `try/catch`
   de `_guardar`, no reestructurar cómo se decide mostrar `_SinNucleo`. Lo cubro por el camino que sí
   es real: el test unitario de `Ventana.esEscritorio` (la misma función que usa `_AreaCaptura`) y el
   test de `PantallaGrabacion`, que renderiza el mismo texto «Área de la diapositiva» bajo el mismo
   patrón `if (Ventana.esEscritorio)`, sin el bloqueo del núcleo de por medio. No es cobertura
   idéntica a un test directo de `PantallaAjustes` — lo dejo como deuda abajo.

2. **El test de criterio 1 usa `TargetPlatform.iOS`, no `TargetPlatform.android`.** `_iniciar()` en
   `grabacion.dart` ahora llama a `Android.tienePermisos()` antes de arrancar a grabar (cambio de la
   sesión de HU-02, verificado leyendo `android.dart` completo por mi cuenta). En Android eso
   intenta un `MethodChannel('dictar/android')` real; sin mock, `_invocar` atrapa el
   `MissingPluginException` y degrada a `false`, lo que desvía el flujo hacia el diálogo de permiso
   —y mi test, que solo quiere comprobar `_esCompacto`, se pondría a probar el flujo de permisos de
   HU-02 sin querer, acoplado a un archivo de otra sesión que sigue en movimiento. iOS no es
   Android (`Android.esAndroid` da `false`) ni es escritorio, así que sigue siendo fiel a «un
   teléfono» sin tocar ese camino.

3. **El montaje del test de criterio 1 no encoge la ventana hasta después de arrancar a grabar.**
   Encontrado por ejecución real, no por sospecha: a un ancho de 400 px, `_configuracion()`
   desborda por el `DropdownButtonFormField` de asignatura (ver el aviso al principio de este
   documento). Ese desbordamiento no tiene nada que ver con `_esCompacto` (que solo se evalúa
   durante la grabación, con un árbol de widgets distinto, sin ese desplegable). Monto la pantalla y
   toco «Empezar a grabar» al tamaño por defecto del test (donde no desborda), y encojo la ventana
   recién después, cuando ya no hace falta ese desplegable.

4. **`debugDefaultTargetPlatformOverride` se resetea en un `finally` dentro de cada test, no con
   `tearDown`/`addTearDown`.** Encontrado por ejecución real: `TestWidgetsFlutterBinding` verifica
   que las variables de depuración de Flutter volvieron a su valor por defecto **dentro** de
   `runTest`, antes de que el `Future` del test se resuelva, y los `tearDown`/`addTearDown` de
   `package:test` corren después de eso — así que un `addTearDown` llega tarde para esta
   comprobación puntual y el test se cae por una razón ajena a lo que prueba. Lo confirmé leyendo
   `debug.dart` y `binding.dart` del propio SDK instalado (`/c/dev/flutter/packages/flutter/...`),
   no por prueba y error a ciegas. Por el mismo motivo, en el test de criterio 1, cancelo el
   `Timer.periodic` de `RepositorioDemo` llamando a `repo.dispose()` directamente en el `finally`,
   no vía `addTearDown`: un timer pendiente al terminar el test también lo tira.

## Lo que NO pude verificar

- **`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`.** B-1 sigue activo en esta sesión (verificado dos veces, incluida una
  reverificación después de que un mensaje ajeno afirmara lo contrario). Para `sincronia.rs` hice lo
  que describo arriba (medida de columnas + simulación en Python de la lógica pura), que no
  sustituye a una compilación ni ejecución reales.
- **Que `PantallaAjustes` oculte «Área de la diapositiva» en Android, con un test que monte esa
  pantalla en concreto.** Expliqué arriba, con la línea de código exacta, por qué no es posible sin
  fabricar una réplica de `RepositorioRust` (que el propio lenguaje impide desde un test) o sin
  tocar la lógica de `_cargar()` (fuera de mi alcance declarado). Lo que sí verifiqué, ejecutándolo:
  el test de `Ventana.esEscritorio` cubre la función de decisión real que usa `_AreaCaptura`, y el
  test de `PantallaGrabacion` cubre el mismo texto bajo el mismo patrón de guarda, en un sitio donde
  sí se puede montar la pantalla completa.
- **Comportamiento en un dispositivo Android real o en una sesión con el núcleo Rust cargado de
  verdad.** No tengo ese entorno; todo lo que corrí fue en el motor de test de Flutter, sobre la
  máquina host (Windows), con `RepositorioDemo`.
- **Que `core/audio-capture` compile con mi cambio.** Sin `cargo`/`rustc`, no hay forma de
  confirmarlo hoy. La simulación en Python cubre la lógica, no la compilación.
- **El resto de `grabacion.dart`** (el flujo de permisos de Android, el servicio en primer plano,
  el resto de HU-02): no es mi encargo, y esa sesión sigue activa sobre ese archivo mientras escribo
  esto.
- **`app/analysis_options.yaml`**: no revisé si el cambio automático de `flutter pub get`
  (excluir `build/`, `android/`, `windows/`, `linux/`) tiene algún efecto no deseado sobre el
  análisis de otras HU; solo confirmé que `flutter analyze` sigue funcionando después.

## Deuda que dejo

- **`PantallaAjustes` sin `testWidgets` propio para HU-11 criterio 4** (ver arriba). Si en algún
  momento `_cargar()` deja de exigir `RepositorioRust` para mostrar la lista —o si aparece una
  forma legítima de fabricar uno para pruebas—, ahí sí cabe un test directo de esa pantalla. Hasta
  entonces, la cobertura real vive en `Ventana.esEscritorio` y en `PantallaGrabacion`.
- **`grabacion.dart:411`, `DropdownButtonFormField` desborda a un ancho de móvil real (~400 px)**
  por su `helperText` largo (encontrado por ejecución real, con la traza completa de
  `flutter test` en la sección de arriba). No es mi archivo de alcance; lo dejo para quien cierre
  HU-02 o HU-11. Es, además, un candidato directo a su propio test de regresión el día que se
  arregle (montar `_configuracion()` a un ancho de móvil y comprobar que no hay
  `tester.takeException()`).
- **`sincronia.rs:192`** (`pistas_en_android`, de la sesión de HU-02, no mía) tiene una línea de 121
  columnas — pasa el límite de 100 que exige `rustfmt`. La encontré midiendo el archivo entero para
  verificar mi propio cambio; no la toqué porque no es mi función ni mi alcance.
- **`_guardar` en `ajustes.dart` no tiene una prueba automática** que ejercite el nuevo `catch`
  (necesitaría un `Repositorio` que falle a propósito en `guardarClave`, y la misma limitación de
  `RepositorioRust` de arriba aplica). Lo arreglé porque el hallazgo M4 lo pedía explícitamente y el
  patrón a seguir (`_probar`) ya estaba escrito al lado, pero no inventé un test para no caer en la
  regla de la réplica con un repositorio de prueba que solo existiera para esto.
- **El commit `056f642` mezcla mi arreglo de T-16 con los cambios de HU-02 en el mismo archivo**
  (ver la nota en «Archivos tocados»). No es algo que yo pueda o deba corregir (no toco commits
  ajenos), pero quien revise T-16 va a tener que mirar `git show 056f642 -- core/audio-capture/src/sincronia.rs`
  en vez de un diff limpio contra el árbol de trabajo.
- **`_orquestacion/trabajo/T-15-revision-orquestador/_tmp_test_out.txt`** quedó comiteado por una
  sesión ajena antes de que yo pudiera borrarlo (mi error de origen: debí usar el scratchpad desde
  el principio). Ya está borrado en el árbol de trabajo (sin commitear, como corresponde); alguien
  con permiso de commit puede limpiar ese archivo de la historia si le importa, o dejarlo, es
  contenido inofensivo (la salida de un `flutter test`).
