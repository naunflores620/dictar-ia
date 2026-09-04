# 06 — Historias de usuario y seguimiento

Lo que falta, escrito como lo que el usuario quiere hacer y no como lo que hay que programar.
Cada historia lleva sus criterios de aceptación, y cada criterio está redactado para que se
pueda **comprobar**: si no se puede decir «esto pasa» o «esto no pasa» mirando el código o
ejecutando algo, no es un criterio, es un deseo.

El estado lo mantiene al día el agente de QA (`.claude/agents/qa.md`), que contrasta cada
criterio contra el repositorio. Nadie marca una historia como validada por su cuenta.

| Estado | Significado |
|---|---|
| 🔴 | Pendiente. Nadie ha empezado. |
| 🟡 | Implementada pero **sin verificar**: el código está, no se ha compilado ni ejecutado. |
| 🟢 | Verificada por QA en el código, con `cargo test` / `flutter test` en verde. |
| ✅ | Validada de extremo a extremo contra una clase real. |

---

## Tablero

| HU | Título | Prioridad | Estado |
|---|---|---|---|
| [HU-01](#hu-01--grabar-en-windows) | Grabar en Windows | Alta | 🔴 |
| [HU-02](#hu-02--grabar-en-android) | Grabar en Android | Media | 🔴 |
| [HU-03](#hu-03--importar-un-audio-que-grabé-con-otra-cosa) | Importar un audio | Alta | 🔴 |
| [HU-04](#hu-04--buscar-en-todo-lo-que-he-grabado) | Buscar en todo lo grabado | Alta | 🔴 |
| [HU-05](#hu-05--que-mis-claves-de-api-no-estén-en-texto-plano) | Claves en el llavero del SO | Alta | 🔴 |
| [HU-06](#hu-06--que-mis-clases-estén-cifradas-en-el-disco) | Cifrado en reposo | Media | 🔴 |
| [HU-07](#hu-07--escuchar-una-sesión-en-windows) | Reproducir en Windows | Media | 🔴 |
| [HU-08](#hu-08--preguntarle-a-la-asignatura) | Búsqueda semántica y chat | Baja | 🔴 |
| [HU-09](#hu-09--mandarle-el-acta-a-un-cliente) | Exportar a PDF | Baja | 🔴 |
| [HU-10](#hu-10--que-el-instalador-lleve-el-núcleo-dentro) | El paquete lleva el núcleo | Alta | 🟡 |
| [HU-11](#hu-11--que-la-interfaz-se-comporte-en-un-móvil) | La interfaz en móvil | Media | 🟡 |
| [HU-12](#hu-12--que-el-núcleo-compile-fuera-de-linux) | El núcleo compila fuera de Linux | Alta | 🟡 |
| [HU-13](#hu-13--que-el-readme-no-prometa-lo-que-no-hay) | El README dice la verdad | Media | 🟡 |

Las cuatro últimas están en 🟡 y no en 🟢 por un motivo concreto: se escribieron en una máquina
sin `cargo`, sin NDK y con un Flutter demasiado viejo para resolver el `pubspec`. **No se ha
compilado ni una línea.** Validarlas es el primer trabajo del agente de QA, antes que cualquier
historia nueva.

---

## HU-01 · Grabar en Windows

**Como** estudiante que usa Windows,
**quiero** que la aplicación capture el audio del sistema y el de mi micrófono en dos pistas,
**para** grabar mis clases sin tener que cambiarme de sistema operativo.

Es la historia que más valor desbloquea: hoy el `.exe` se instala, arranca y no graba. Todo lo
demás del producto —transcribir, resumir, buscar— ya funciona en Windows.

**Criterios de aceptación**

1. `dictar_audio::iniciar` en Windows devuelve un `Receiver` de verdad, no `NoSoportada`.
2. Se capturan **dos pistas separadas**: micrófono y *loopback* del dispositivo de salida por
   defecto, usando `AUDCLNT_STREAMFLAGS_LOOPBACK`. Nunca una mezcla.
3. Ambas pistas llegan a 16 kHz mono en `f32`, con `timestamp_ms` de un reloj monótono común,
   igual que hace `pipewire_src`.
4. `dictar_audio::dispositivos()` enumera los dispositivos reales del sistema.
5. Al soltar la `CaptureSession` la captura se detiene y los hilos terminan.
6. Grabando 30 minutos, la deriva entre las dos pistas no es perceptible.
7. Existe al menos un test que no necesite tarjeta de sonido (conversión de formato, cálculo de
   marcas de tiempo, manejo del cambio de dispositivo por defecto).

**Alcance técnico** — `core/audio-capture/src/wasapi_src.rs` nuevo, la rama de `iniciar()` y
`dispositivos()` en `lib.rs`, y las dependencias de Windows en su `Cargo.toml`. El espejo exacto
de `pipewire_src.rs`, que es la referencia de cómo debe comportarse.

**Ojo con** el formato: WASAPI entrega lo que tenga el dispositivo (normalmente 48 kHz estéreo
en `f32` o `i16`), así que hay que pasar por `mezcla::a_mono` y `mezcla::Remuestreador` antes de
emitir. Y el *loopback* necesita un `IAudioClient` en modo captura sobre el **dispositivo de
salida**, que es la parte que más se equivoca quien lo hace por primera vez.

---

## HU-02 · Grabar en Android

**Como** persona que va a una reunión presencial,
**quiero** grabar con el micrófono del móvil y que la grabación sobreviva a que apague la
pantalla,
**para** no tener que llevar el portátil a una cafetería.

En Android **no** se persigue el *loopback*: no hay forma de capturar el audio de una
videollamada ajena, y el caso de uso aquí es la reunión presencial, no la clase online.

**Criterios de aceptación**

1. `dictar_audio::iniciar` en Android captura el micrófono y emite `AudioFrame` con `Track::Mic`.
2. La captura continúa con la pantalla apagada y con la aplicación en segundo plano, sostenida
   por un servicio en primer plano con su notificación.
3. La aplicación pide `RECORD_AUDIO` en tiempo de ejecución y explica para qué antes de pedirlo.
4. Si el usuario deniega el permiso, la aplicación lo dice y no se queda en una pantalla muerta.
5. Una grabación de una hora no se corta.

**Alcance técnico** — un backend AAudio (u Oboe) en `core/audio-capture`, el servicio en primer
plano en Kotlin bajo `app/android/`, y la petición de permisos en Dart. Los permisos del
manifiesto ya están declarados; falta todo lo demás.

**Depende de** HU-10 (que el APK lleve el núcleo dentro).

---

## HU-03 · Importar un audio que grabé con otra cosa

**Como** usuario que grabó una reunión con la grabadora del móvil,
**quiero** arrastrar ese archivo a la aplicación y que salga con sus apuntes,
**para** aprovechar todo el pipeline sin haber grabado desde aquí.

El botón ya existe en la pantalla de inicio y **no hace nada**: llama a `_avisar()`, que muestra
«pendiente de conectar con el núcleo». El backend está entero (`leer_wav` remuestrea cualquier
frecuencia, y `procesar_sesion` hace el resto).

**Criterios de aceptación**

1. El botón «Importar audio de una reunión presencial» abre un selector de archivos.
2. Se acepta al menos WAV; si el formato no se puede leer, se dice por qué y no se crea una
   sesión a medias.
3. El archivo importado se convierte en una sesión con su `topic`, su tipo y su fecha, igual que
   una grabada.
4. Al terminar, la sesión aparece en el listado y se puede procesar como cualquier otra.
5. Un archivo a 48 kHz sale con la duración correcta, no a triple velocidad.

**Alcance técnico** — una función nueva en `core/api/src/puente.rs`, **regenerar el puente** con
`flutter_rust_bridge_codegen generate`, y conectar el botón en `app/lib/pantallas/inicio.dart`.

---

## HU-04 · Buscar en todo lo que he grabado

**Como** estudiante a mitad de semestre,
**quiero** escribir «transformada de Laplace» y ver en qué clases se dijo,
**para** encontrar algo sin recordar de qué día era.

Igual que la anterior: el botón es una maqueta y el backend existe. `db.buscar()` usa FTS5, está
expuesto en el puente como `buscar`, y el índice se repuebla al migrar.

**Criterios de aceptación**

1. El icono de búsqueda abre una pantalla de búsqueda, no un aviso.
2. Al escribir, se listan las frases que coinciden, cada una con su sesión y su `ts_ms`.
3. Tocar un resultado abre la sesión posicionada en ese momento.
4. Sin resultados se dice «no hay nada», no una lista vacía sin explicación.
5. La búsqueda no bloquea la interfaz mientras escribe el usuario.

**Alcance técnico** — el puente ya expone `buscar`, pero **no basta**: `FraseDto`
(`core/api/src/puente.rs`) devuelve `track`, `speaker`, `inicio_ms`, `fin_ms`, `texto` y
`definitiva`, y **deja fuera el `session_id`** que sí trae `Utterance`. Sin él no se puede
cumplir el criterio 2 («con su sesión») ni el 3 («abre la sesión»). Añadirlo obliga a regenerar
el puente, y `flutter_rust_bridge_codegen` no está instalado (bloqueo B-5).

Los criterios 1, 4 y 5 sí se pueden cumplir solo con Dart. Los criterios 2 y 3 esperan a B-5.

---

## HU-05 · Que mis claves de API no estén en texto plano

**Como** usuario que guarda su clave de Gemini en el equipo,
**quiero** que viva en el llavero del sistema operativo,
**para** que no esté en un `.env` legible por cualquier proceso.

El README lo promete desde el primer día. La cadena de resolución ya está diseñada para esto
—variables de entorno, luego `.env`, luego llavero— y el tercer eslabón está sin escribir:
`secretos.rs` lo dice en su propio comentario de cabecera.

**Criterios de aceptación**

1. Existe un `KeyResolver` que lee del llavero: Secret Service en Linux, Credential Manager en
   Windows.
2. Va el último de la cadena, después del entorno y del `.env`, sin alterar ese orden.
3. Guardar una clave desde la pantalla de ajustes la escribe en el llavero, no en un archivo.
4. Si no hay llavero disponible —un servidor sin sesión gráfica—, la aplicación sigue
   funcionando con las otras dos fuentes en vez de fallar al arrancar.
5. Ninguna clave aparece en los logs, ni siquiera truncada.

**Alcance técnico** — `core/providers/src/secretos.rs` y su `Cargo.toml`. La pantalla de ajustes
ya llama a `guardar_clave`; el cambio es de dónde sale y a dónde va.

---

## HU-06 · Que mis clases estén cifradas en el disco

**Como** usuario que graba reuniones con clientes,
**quiero** que la base de datos esté cifrada,
**para** que perder el portátil no signifique entregar las transcripciones.

La feature `cifrado` (SQLCipher) ya existe en `core/storage/Cargo.toml`, apagada por una razón
escrita allí mismo: vendoriza OpenSSL y multiplica el tiempo de compilación. Esa razón vale
mientras se itera sobre el esquema; deja de valer cuando el esquema se estabiliza.

**Criterios de aceptación**

1. Con la feature activada, la base se abre con clave y un `sqlite3` normal no puede leerla.
2. La clave se genera sola la primera vez y se guarda en el llavero, nunca junto a la base.
3. Una base existente sin cifrar se migra sin perder nada, y se avisa antes.
4. Si la clave no aparece, se dice claramente qué pasa; no se crea una base vacía encima.

**Depende de** HU-05: sin llavero no hay dónde poner la clave, y guardarla al lado de la base
cifrada no cifra nada.

---

## HU-07 · Escuchar una sesión en Windows

**Como** usuario de Windows repasando una clase,
**quiero** darle al play y oír la grabación,
**para** contrastar lo que dicen los apuntes con lo que se dijo.

Hoy `Reproductor::iniciar` fuera de Linux devuelve `NoSoportada`: es el sustituto que se escribió
para que el núcleo compilara, no una implementación.

**Criterios de aceptación**

1. Reproducir una sesión en Windows suena, con las dos pistas mezcladas.
2. Pausar, saltar y consultar la posición funcionan igual que en Linux.
3. Al cerrar la sesión o salir, no queda ningún hilo de audio vivo.

**Alcance técnico** — `core/audio-capture/src/reproductor_stub.rs` deja de ser un sustituto para
Windows. `mezclar` ya es común a todas las plataformas.

---

## HU-08 · Preguntarle a la asignatura

**Como** estudiante preparando un examen,
**quiero** preguntar «¿qué dijo sobre transformadas?» y que me responda con citas de mis clases,
**para** no repasar quince grabaciones a mano.

Es la fase 3 del roadmap y el valor que ninguna grabadora tiene. `sqlite-vec` figura en el stack
del README pero **no está en el repositorio**: no hay ni una línea que lo use.

**Criterios de aceptación**

1. Las frases se indexan con sus vectores al procesar una sesión.
2. Una consulta en lenguaje natural devuelve fragmentos de varias sesiones del mismo `topic`.
3. Cada respuesta lleva su cita, con enlace al `ts_ms` y a la diapositiva.
4. Funciona sin conexión si el proveedor configurado es local.

---

## HU-09 · Mandarle el acta a un cliente

**Como** profesional que acaba una reunión,
**quiero** exportar el acta a PDF con un formato presentable,
**para** enviarla sin tener que maquetarla.

Hoy se exporta a Markdown, que sirve para uno mismo y no para un cliente.

**Criterios de aceptación**

1. Se exporta a PDF desde la pantalla de la sesión.
2. El PDF lleva las diapositivas intercaladas en su sitio.
3. Se abre bien en un lector normal, sin fuentes que falten.

---

## HU-10 · Que el instalador lleve el núcleo dentro

**Como** persona que instala la aplicación desde un paquete,
**quiero** que funcione al abrirla,
**para** no encontrarme una aplicación llena de datos de ejemplo.

Es la historia que explica el fallo más silencioso que ha tenido este proyecto: el `.exe` y el
APK se construían sin la librería nativa, la aplicación no la encontraba, caía en su `catch` y
arrancaba con datos de demostración. Parecía funcionar.

**Criterios de aceptación**

1. `flutter build windows` deja `dictar_api.dll` junto al `.exe`.
2. `flutter build apk` deja `libdictar_api.so` dentro del APK, en su ABI.
3. El CI **falla** si cualquiera de las dos cosas no ocurre.
4. El `.deb` sigue funcionando igual que antes.

**Estado 🟡** — auditado por QA (03/09), sigue en 🟡: los cuatro criterios son **no
comprobables aquí**. No hay `cargo` (B-1) ni NDK/`cargo-ndk` (B-3), y `flutter pub get` falla por
versión del SDK (B-2) antes de llegar siquiera a invocar el CMake o el Gradle que compilan el
núcleo — comprobado ejecutando `flutter build windows`, que aborta en la resolución de paquetes
con el mismo error que `flutter pub get`. Lectura estática, sin poder ejecutarla: `dictar_api.dll`
(criterio 1, `app/windows/CMakeLists.txt:90-132`) y `libdictar_api.so` (criterio 2,
`app/android/app/build.gradle.kts`) se generan con el nombre y en la ruta que esperan
`packaging/windows/inno_setup.iss` (empaqueta toda la carpeta `Release` por glob) y el chequeo de
`release.yml`; no se encontró ninguna discrepancia de nombres o rutas. El criterio 3 (el CI falla
si falta el núcleo) está en `.github/workflows/release.yml:100-108` y `:179-187` (`throw` /
`exit 1`), YAML válido (`python3 -c "yaml.safe_load(...)"`), pero solo corre al etiquetar un
release (`tags: v*` o `workflow_dispatch`) — no en cada PR, a diferencia del `cargo check` de
HU-12. El criterio 4 (`.deb`) no tiene cambios en este diff (`git diff` vacío para
`packaging/linux/` y `app/linux/CMakeLists.txt`): no hay indicio de regresión, pero tampoco se
pudo construir para confirmarlo.

---

## HU-11 · Que la interfaz se comporte en un móvil

**Como** usuario que abre la aplicación en el teléfono,
**quiero** una interfaz pensada para esa pantalla,
**para** que no sea la de escritorio encogida.

**Criterios de aceptación**

1. Un teléfono en vertical muestra la transcripción en vivo, no la disposición del panel
   compacto de escritorio.
2. Ninguna pantalla llama a `window_manager` donde no existe.
3. La navegación cambia de riel lateral a barra inferior según el ancho.
4. Las funciones que no tienen sentido en un móvil no se ofrecen como si funcionaran.

**Estado 🟡** — auditado por QA (03/09). 1, 2 y 3 cumplen; **4 sigue sin cumplirse**.
1: `_esCompacto` en `app/lib/pantallas/grabacion.dart:432-434` decide solo por alto (antes entraba
también por ancho, lo que mandaba a todos los móviles a la disposición compacta por error de
ancho, no de alto). 2: en toda `app/lib` solo dos archivos llaman a `windowManager` —
`ventana.dart` (se autoprotege con `_soportado`) y `pantallas/region.dart`, que ahora usa
`Ventana.soportado` en las tres llamadas (líneas 60, 67 y 79), incluida la del `catch` (línea 79):
antes de este cambio, ese `catch` volvía a llamar a `windowManager.show()` sin protección y
relanzaba `MissingPluginException` desde dentro del propio manejador de errores en Android —
verificado corregido. 3: sin cambios en este diff; ya funcionaba (`app/lib/main.dart:114-168`,
`esAncho = ancho >= 720`). 4: **no cumple** — `app/lib/pantallas/ajustes.dart:645-654` navega a
`PantallaRegion` sin ninguna comprobación de plataforma. En un móvil no falla en silencio
(`dictar_screen::captura_para_seleccion` devuelve `NoSoportada` y la pantalla muestra el error),
pero la opción se sigue ofreciendo donde no tiene sentido. No verificado en un dispositivo real ni
con `flutter test`: B-2 bloquea `flutter pub get` antes de llegar a correr nada.

---

## HU-12 · Que el núcleo compile fuera de Linux

**Como** desarrollador,
**quiero** que `cargo check` pase en Windows y en el objetivo de Android,
**para** que una ruptura multiplataforma se vea en el pull request y no en la release.

**Criterios de aceptación**

1. `cargo check --workspace --all-targets` pasa en `windows-latest`.
2. `cargo ndk -t arm64-v8a build -p dictar-api` compila.
3. El CI comprueba lo primero en cada pull request.
4. Ninguna función pública cambia de firma según la plataforma: quien la llama no escribe
   `#[cfg]`.

**Estado 🟡** — auditado por QA (03/09). 3 y 4 cumplen; 1 y 2 **no comprobables aquí**,
nunca ejecutados. Confirmado de nuevo que no hay `cargo`/`rustc`/`rustfmt` ni en Windows ni en
`Ubuntu-26.04` de WSL (única distribución con shell; `docker-desktop` no tiene ninguna), y que no
hay NDK ni `cargo-ndk` instalados. 3: el job `nucleo-windows` (nuevo en este diff,
`.github/workflows/ci.yml:69-105`) corre `cargo check --workspace --all-targets` en
`windows-latest`, dentro del `on: pull_request` del workflow; YAML válido. 4: verificado firma por
firma, no solo por inspección superficial — `core/audio-capture/src/reproductor.rs` (Linux) contra
`reproductor_stub.rs` (resto): `iniciar(&Path, i64) -> Result<Self>`, `posicion_ms(&self) -> i64`,
`duracion_ms(&self) -> i64`, `pausar(&self, bool)`, `pausado(&self) -> bool`,
`terminado(&self) -> bool`, `saltar(&self, i64)` — idénticas en ambos archivos. En
`core/screen-capture/src/lib.rs`, las cuatro funciones (`iniciar`, `captura_para_seleccion`,
`capturar_ahora`, `pantallas`) tienen la misma firma en la rama
`#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]` que en su
`#[cfg(not(...))]`. `core/api/src/lib.rs` y `core/api/src/grabacion.rs` llaman a ambos módulos sin
ningún `#[cfg]` propio (`grep` sin resultados de `target_os` en esos dos archivos). Se repasaron
también el resto de `#[cfg(windows)]` / `#[cfg(unix)]` preexistentes del núcleo
(`core/api/src/lib.rs:107`, `core/stt/src/modelos.rs:63`,
`core/providers/src/secretos.rs:153,358,543`): ninguno cambia una firma pública, son ramas dentro
del cuerpo de la función con el mismo tipo de retorno.

---

## HU-13 · Que el README no prometa lo que no hay

**Como** persona que llega al repositorio,
**quiero** que lo que dice coincida con lo que hace,
**para** saber en qué me estoy metiendo.

**Criterios de aceptación**

1. Las plataformas que dice soportar son las que graban de verdad.
2. El cifrado y el llavero se describen por su estado real, no por el previsto.
3. Los recuentos de tests coinciden con los del repositorio.
4. Lo que se anuncia en el stack existe en el código.

**Estado 🟡** — auditado por QA (03/09). 1, 2 y 3 cumplen; **4 falla en un punto
concreto**. 1: coincide con el código — `dictar_audio::iniciar` solo tiene implementación real en
Linux (`pipewire_src`); Windows y Android devuelven `NoSoportada`. 2: coincide —
`core/storage/Cargo.toml:16` (`default = []`, feature `cifrado` apagada) y
`core/providers/src/secretos.rs:1-16` (llavero «pendiente de `libsecret`»). 3: recontado con
`grep -rn '#\[test\]\|#\[tokio::test\]' core/*/src cli/src | wc -l` → 307, más 24 en
`app/test` (`test(` / `testWidgets(`), total 331 — exacto contra «331 tests en verde» y contra
cada fila de la tabla, crate por crate. El recuento del puente también cuadra: 33 `pub fn` en
`core/api/src/puente.rs` contra «33 funciones». Ninguno de estos recuentos confirma que los tests
*pasen*: no se pudo ejecutar `cargo test` ni `flutter test` (B-1, B-2), así que «en verde» queda
sin verificar — solo el recuento coincide, que es lo que pide el criterio 3 literalmente. 4:
**no cumple en un punto** — `README.md:93` dice «El `.exe` de Inno Setup está escrito pero
desactivado hasta que Windows pueda grabar», y es falso: `.github/workflows/release.yml` compila
y empaqueta el `.exe` de Windows en cada tag `v*` o `workflow_dispatch`, sin ninguna condición que
lo desactive (la matriz incluye `windows-latest` exactamente igual que antes de este diff; los
`if: matrix.plataforma == 'windows'` solo separan pasos entre plataformas, no deshabilitan
ninguna). El resto del stack sí coincide: `sqlite-vec` no aparece en ningún `.rs`/`.toml` del
repositorio, y el `.deb` sigue activo sin condición.
