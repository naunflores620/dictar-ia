# PLAN: AV-2 «Que la app me avise durante la grabación si el audio está mal»

Origen: [docs/investigacion-notion-reuniones-2026-10-12.md §6 bis](../../../docs/investigacion-notion-reuniones-2026-10-12.md) · HU-23 propuesta en ese documento (§7; todavía no está en `docs/06`) · HU-14 del [plan de Windows](../../../docs/plan-correccion-calidad-windows.md).

**Revisión 2**, tras el contradictor. Los cambios están en «Respuesta al contradictor», al final.

**Se integra junto con AV-1**: ninguna de las dos se cierra sin la otra (ver el plan de AV-1).

## Problema

Las métricas de calidad de captura existen, pero **después** de la clase:

- `dictar metricas`;
- `VentanaCaptura` en `core/api/src/grabacion.rs:509-560`;
- `analizar_retorno_acustico` en `core/audio-capture/src/lib.rs:282-343`.

Durante la grabación la interfaz solo pinta «hablando sí o no» (`repositorio_rust.dart:411-418`). Que el loopback estaba mudo, que el volumen era bajísimo o que había eco se descubre tarde, en una clase que ocurre una sola vez.

## Decisiones de diseño (las toma el orquestador)

### D1. Lógica pura en un módulo nuevo, alimentada por tramas y por ticks

`core/api/src/salud_audio.rs` expone `SaludAudio::nueva(inicio: Instant, capturar_sistema: bool)` con dos entradas:

- `trama(&AudioFrame, ahora)`;
- `tick(ahora)`, porque varios avisos dependen justamente de que **no** lleguen tramas.

Las dos devuelven `Vec<CambioAviso>` (un aviso aparece o desaparece).

En `bucle_escritura` (`grabacion.rs`, camino principal, ~327-430) se llama a `trama` tras escribir a disco y registrar métricas, y a `tick` en **cada vuelta**, incluido el brazo `Timeout` de `recv_timeout` (cada 200 ms). El vaciado final tras detener (`grabacion.rs:442-489`) **no** alimenta a `SaludAudio`: ya no hay interfaz que avisar. El reloj de «sin tramas» empieza en `inicio`, que es el inicio de la sesión, no la primera trama.

### D2. La línea de tiempo se alinea por `timestamp_ms`, no por orden de llegada

Cada pista guarda un búfer circular de **15 s a 16 kHz**, indexado por la posición absoluta `timestamp_ms × 16`, con los huecos sin tramas a cero.

El loopback no entrega paquetes en silencio y `sincronia.rs` rellena al volver, así que el orden de llegada **no** sirve como reloj. Toda evaluación conjunta (el eco) se hace sobre la misma ventana absoluta `[ahora − 15 s, ahora − 0,5 s]` de las dos pistas. El medio segundo final evita la parte que puede no haber llegado todavía.

### D3. Los avisos

Los umbrales son iniciales y quedan como `const` documentadas. «Ventana activa» significa una ventana de 30 ms con RMS > 0,003, el umbral de casi silencio de `VentanaCaptura`. Se usa para medir **actividad**, no voz.

| Código | Pista | Se activa | Se desactiva | Texto |
|---|---|---|---|---|
| `sin_senal_inicio` | sistema (si `capturar_sistema`) | Desde el inicio **nunca** hubo una ventana activa en sistema y pasaron **30 s** | Primera ventana activa | «No llega audio del sistema. Comprueba que la clase suena en este equipo y por la salida predeterminada.» |
| `sin_senal_pausa` | sistema | Hubo actividad, y luego **3 min** seguidos sin ventanas activas | Primera ventana activa | «Hace 3 minutos que no suena nada en el equipo. Si la clase sigue, revisa la salida de audio.» |
| `mic_sin_tramas` | micrófono | **10 s** sin recibir tramas de micrófono. Un micrófono abierto entrega tramas siempre, aunque sea silencio, así que esto significa dispositivo perdido o reabriendo (AV-1) | Llega una trama | «El micrófono dejó de enviar audio. Revisa que siga conectado.» |
| `nivel_bajo` | **solo sistema** | En los últimos 10 s hay ≥ **3 s** de ventanas activas y su **RMS global** (raíz de la media de los cuadrados de esas muestras) es < **−38 dBFS** (0,0126) | ≥ −34 dBFS con ≥ 3 s activos | «El volumen de la clase es muy bajo; súbelo en la aplicación que la reproduce.» |
| `recorte` | cualquiera | > **0,1 %** de muestras con \|x\| ≥ 0,99 en los últimos 5 s | 10 s sin recorte | «{Pista} satura; baja el volumen.» |
| `eco` | micrófono | **Dos evaluaciones seguidas** (cada 5 s) con correlación ≥ **0,65** en `analizar_retorno_acustico`, sobre la ventana alineada de D2, **y** con ≥ **5 s** de ventanas activas en sistema **y** ≥ 3 s en micrófono dentro de esa ventana. Este mínimo lo exige `salud_audio`; el de la función (30 bloques ≈ 0,6 s) es insuficiente | Dos evaluaciones seguidas < 0,55, o falta de datos durante 30 s | «El micrófono recoge lo que suena por los altavoces. Usa auriculares para evitar eco.» |

- **El micrófono queda fuera de `nivel_bajo` en esta tarea.** Sin un detector de voz propio, su ruido de sala entre −50 y −38 dBFS lo marcaría bajo toda la clase, y no hay ninguna medida de voz propia a distancia normal para calibrarlo. Queda anotado como **T-19**.
- **El aviso de eco no promete detectar todo eco audible.** Diseño de Sistemas tiene eco con 0,552 global y ventanas de 10 s de hasta 0,691, así que el aviso puede aparecer de forma intermitente. Es aceptable si la prueba sin conexión de D7 lo documenta.
- **Para no parpadear:** un aviso que se desactiva no puede reactivarse antes de **15 s**.

### D4. Coste

El contradictor estimó la correlación sobre 15 s en microsegundos o pocos milisegundos, y es asumible. La ventana se copia una vez por evaluación, no en cada tick. El implementador mide el tiempo de una evaluación en Release y lo pone en HANDOFF. Si supera **20 ms**, informa y no cambia de hilo por su cuenta.

### D5. Cruzar el puente sin regenerarlo

Se añade `Evento::Salud { pista, codigo, activo, valor, mensaje }` y se mapea en `From<Evento> for EventoDto` así:

| Campo de `EventoDto` | Contenido |
|---|---|
| `tipo` | `"salud"` |
| `track` | `pista` |
| `fase` | `codigo` |
| `definitiva` | `activo` |
| `fraccion` | `valor` |
| `mensaje` | `mensaje` |

- **No cambia ningún tipo del puente** y no hace falta `flutter_rust_bridge_codegen` (B-5).
- El comentario del campo `tipo` (`puente.rs:117`) documenta este uso de campos con otro significado.
- **Prueba Rust obligatoria del mapeo**, junto a la de `Niveles` (`puente.rs:~752`).
- Las versiones viejas de la app ignoran `tipo` desconocido (verificado por el contradictor).

### D6. Interfaz y ciclo de vida de los avisos

- **`Repositorio`:** `Stream<AvisoAudio> get avisosAudio`. `RepositorioDemo` no emite nunca. `RepositorioRust` emite en `case 'salud'`.
- **`grabacion.dart`:**
  - en la disposición amplia, una banda con los avisos activos (icono, texto y cerrar, que lo oculta hasta que se reactive);
  - en la compacta, un punto de color sobre el indicador de la pista, con el texto en *tooltip*.
- **Limpieza:** el estado de avisos de la pantalla se vacía al **empezar** una sesión y al **detenerla**. El núcleo no emite `activo=false` al parar: es la interfaz la que lo limpia.

### D7. Prueba sin conexión con audio real

`core/api/examples/salud_audio_eval.rs` alimenta `SaludAudio` con los pares `mic.wav`/`system.wav` de `%APPDATA%\dictar_ia\audio\ses_*`:

- en bloques de 20 ms con `timestamp_ms` sintético;
- sin huecos, salvo el relleno que ya está en el WAV;
- incluidas Comunicaciones (132,6 s, eco 0,744), Diseño de Sistemas (158,1 s, 0,552 global) y la clase de 57,7 min.

Imprime cuántas veces y en qué segundos se activa cada aviso. Es la única comprobación de falsos positivos y negativos con datos reales.

**Esperado:**

- `eco` aparece en Comunicaciones y en la clase larga;
- en Diseño de Sistemas puede aparecer o no, y se documenta lo que pase;
- `nivel_bajo` **no** aparece en ninguna de las tres, que tienen RMS de sistema de 0,05 a 0,16;
- `sin_senal_*` no aparece salvo en silencios reales de más de 3 min, que se identifican.

Si los resultados contradicen esto, se informa con los números **antes** de tocar umbrales.

### D8. Arreglar la prueba de widget rota

En `app/test/grabacion_test.dart`, la pantalla de configuración actual en escritorio muestra «Área para captura manual» y el botón «Elegir» (`grabacion.dart:464-478`). «Área de la diapositiva» ya solo existe en `ajustes.dart:831`.

- Rama **escritorio**: espera «Área para captura manual».
- Rama **Android**: `findsNothing` de **esa misma cadena**, para que el contraste no quede vacío y siga protegiendo `Ventana.esEscritorio`.
- No se reintroduce nada de lo quitado.

## Fuera de alcance

- Arreglar las causas (AV-1, AV-3, AV-4).
- `nivel_bajo` del micrófono (T-19).
- Ventana de diagnóstico fuera de la grabación.
- Android, salvo que compile.

## Archivos (lista cerrada)

| Ruta | Cambio |
|---|---|
| `core/api/src/salud_audio.rs` | **Nuevo.** D1–D4, con pruebas |
| `core/api/src/lib.rs` | `mod salud_audio;` (o `pub mod`, si el ejemplo de D7 lo necesita) |
| `core/api/src/eventos.rs` | `Evento::Salud` |
| `core/api/src/grabacion.rs` | Alimentar `trama` y `tick` en `bucle_escritura`; pasarle `capturar_sistema` |
| `core/api/src/puente.rs` | Mapeo D5 y su prueba |
| `core/api/examples/salud_audio_eval.rs` | **Nuevo**, D7 |
| `app/lib/modelos/dominio.dart` | `AvisoAudio` |
| `app/lib/datos/repositorio.dart` | `avisosAudio` en la interfaz y en la demo |
| `app/lib/datos/repositorio_rust.dart` | `case 'salud'` |
| `app/lib/pantallas/grabacion.dart` | Banda, punto en la compacta y limpieza (D6) |
| `app/test/grabacion_test.dart` | D8, y una prueba con una subclase de `RepositorioDemo` que emite un aviso (como `sesion_test.dart:24`) y comprueba que se pinta y se limpia al detener |

No toca `frb_generated.rs`, `app/lib/src/rust/**`, ningún `Cargo.toml` ni `core/audio-capture`. **Disjunta** de AV-1 y AV-3.

## Criterios de aceptación

1. **`cargo test -p dictar-api salud_audio`.** Por cada fila de D3, una prueba de activación, otra de desactivación y otra de que no se dispara en el caso vecino. En concreto:
   - `sin_senal_inicio` frente a `sin_senal_pausa`;
   - ruido de sala a −45 dBFS en el micrófono no dispara nada;
   - `tick` sin tramas sí dispara;
   - el bloqueo de 15 s para reactivar;
   - **alineación** (D2): sistema con huecos sin tramas y relleno posterior, frente a micrófono continuo, no produce un falso eco.
   - **Eco positivo:** sistema con envolvente silábica sintética (ruido modulado a 3–6 Hz) y micrófono igual a ese sistema retrasado 200 ms y atenuado.
   - **Eco negativo:** dos «voces» sintéticas con modulación **independiente** de 3–6 Hz. Ruido blanco no vale como caso negativo.
2. Prueba del mapeo D5 en `puente.rs`.
3. `cargo clippy -p dictar-api --all-targets -- -D warnings` limpio.
4. **Flutter:** `flutter analyze` sin errores nuevos y `flutter test` con **todas** las pruebas en verde.
5. **D7** ejecutado y su salida en HANDOFF.
6. **Prueba manual en Windows**, registrada en HANDOFF:
   - **(a)** grabar sin reproducir nada: `sin_senal_inicio` a los ~30 s;
   - **(b)** reproducir un vídeo con voz por altavoces con el micrófono abierto: `eco` antes de 40 s;
   - **(c)** bajar el volumen **de la aplicación de origen** en el Mezclador de volumen: `nivel_bajo`. Registrar también si el volumen maestro afecta a la captura loopback. Si (c) no se consigue, se dice por qué;
   - **(d)** **3 min de clase normal con auriculares, sin que aparezca ningún aviso.** Es el caso de falsos positivos;
   - **(e)** detener y comprobar que la banda se vacía.
7. Medición de D4 en HANDOFF.

## Respuesta al contradictor (revisión 1 → 2)

| Objeción | Respuesta |
|---|---|
| 1. `nivel_bajo` del micrófono por umbral de silencio | Aceptada. `nivel_bajo` solo en sistema, con ≥ 3 s activos y RMS global definido; micrófono en T-19. Prueba manual con el volumen de la app de origen |
| 2. Eco sin alinear y con mínimo estadístico pobre; prueba con ruido blanco | Aceptada. Alineación por `timestamp_ms` (D2), mínimos propios de 5 s y 3 s, dos evaluaciones seguidas, y pruebas con envolventes silábicas independientes |
| 3. `sin_senal` a 20 s da falsos positivos | Aceptada. Se separa en `sin_senal_inicio` (30 s) y `sin_senal_pausa` (3 min) con textos distintos |
| 4. Falta `tick` | Aceptada. D1 |
| 5. `bucle_escritura` y vaciado final | Aceptada. Se nombra, y el vaciado no alimenta |
| 6. Limpieza de la banda | Aceptada. D6 y criterio 6(e) |
| 7. D6 imprecisa, contraste de Android vacío | Aceptada. D8 |
| 8. Repositorio controlable en la prueba | Aceptada. Subclase de `RepositorioDemo` |
| 9. Falta prueba con audio real | Aceptada. D7 |
| 10. Umbral de eco intermitente | Aceptada. Se declara el comportamiento esperado |
| Coordinación con AV-1: ausencia de tramas | `mic_sin_tramas` (10 s). En sistema la ausencia de tramas es normal en silencio, así que se cubre con `sin_senal_*` |
