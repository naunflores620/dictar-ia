# PLAN: AV-3 «Que lo que llega al reconocedor en vivo esté nivelado y conozca el vocabulario de la asignatura»

Origen: [docs/investigacion-notion-reuniones-2026-10-12.md §6 bis](../../../docs/investigacion-notion-reuniones-2026-10-12.md), puntos 3 y 4 · HU-16 y HU-18 del [plan de Windows](../../../docs/plan-correccion-calidad-windows.md).

**Revisión 2**, tras el contradictor. Los cambios están en «Respuesta al contradictor», al final.

## Problema

El camino en vivo de Sherpa (español) entrega a `motor.aceptar(&frame.pcm)` (`core/api/src/transcriptor.rs:908`) el PCM tal cual sale de la captura. Tiene dos limitaciones:

1. **Nivel.** Las clases medidas el 08/10 van de un RMS de 0,039 a 0,19 según la sesión y la pista. Una ganancia *g* suma `2·ln g` a cada coeficiente log-mel, así que el modelo ve entradas distintas según el volumen. No sabemos si Kroko se entrenó cubriendo niveles bajos. **Solo la medición decide**: el objetivo de −20 dBFS es una hipótesis.
2. **Vocabulario.** El glosario de la asignatura (`prompt_glosario`: términos separados por `", "`, con un máximo de 800 caracteres, en `core/storage/src/notes_store.rs:235-257`) solo llega a Whisper (`transcriptor.rs:368`). Sherpa decodifica con `greedy_search`, sin palabras clave.

## Hechos verificados que condicionan el diseño

- **El reconocedor de Sherpa es único por proceso.** `modelo_es_compartido()` lo guarda en caché (`sherpa.rs:14,63-77`) y el método de decodificación se fija al cargar (`sherpa.rs:42`), no por flujo.
- **El modelo instalado** está en `%LOCALAPPDATA%\Programs\dictar_ia\models\sherpa-onnx-streaming-zipformer-es-kroko-2025-08-06`:
  - `tokens.txt` tiene 652 piezas con prefijo `▁` y distingue mayúsculas de minúsculas;
  - **no hay `bpe.vocab`**.
- **sherpa-onnx 1.13.8** (crate y DLL) ofrece:
  - `hotwords_file`, `hotwords_buf`, `hotwords_score`, `max_active_paths`, `modeling_unit` y `bpe_vocab`, además de `create_stream_with_hotwords(&str)`;
  - según las cadenas de la DLL: «modeling_unit should be one of bpe, cjkchar or cjkchar+bpe», «Please provide --bpe-vocab», «Encode hotwords failed, skipping», «Please use --decoding-method=modified_beam_search if you provide --hotwords-file».
- **Un término que no se puede codificar se omite con un registro, sin dar error.**
- **`create_stream_with_hotwords` hace `CString::new(...).unwrap()`**: un `\0` dentro de un término haría que el programa abortara.
- **Los replays del 08/10 van a ritmo real** (con `sleep`), así que el RTF de 1,026 refleja el reloj y no la carga. Los datos útiles de ese replay:
  - inferencia por bloque p95 de 231 ms;
  - edad del audio p95 de 167 ms;
  - cola máxima de 3;
  - **una sola pista**. En producción hay dos flujos en un hilo.

## Decisiones de diseño (las toma el orquestador)

### D1. Se nivela solo la copia que va a Sherpa, y solo la pista de sistema

- El WAV queda crudo.
- **Solo se nivela `Track::System` cuando el motor es `ReconocedorVivo::Sherpa`.** El micrófono no se nivela, porque amplificaría el eco de los altavoces (correlación de 0,71–0,74 medida). Vosk tampoco, porque no hay medición.

### D2. Control de ganancia con estado entre bloques

`core/stt/src/nivel.rs`, como módulo puro:

- Ventanas de 30 ms (480 muestras) **con estado entre llamadas**: los bloques reales son de 1024 muestras y no se alinean con las ventanas.
- La ganancia solo se actualiza con ventanas activas (RMS > 0,003).
- Objetivo de −20 dBFS, rango de −6 a +20 dB, subida ≤ 3 dB/s y bajada ≤ 20 dB/s.
- **Limitador suave**: lineal hasta 0,9 y compresión continua por encima, sin superar nunca 0,98. Es un limitador, no un recorte duro.
- **La ganancia se conserva** en un reinicio por hueco (`reiniciar_por_hueco`, `transcriptor.rs:413-426`). Solo se reinicia al empezar la sesión.

### D3. Las palabras clave, primero como exploración

**Paso 0, la exploración. Si falla, se detiene la parte de palabras clave; la nivelación sigue adelante.**

- **(a)** Cargar el modelo con `modified_beam_search` y comprobar que da un texto comparable a `greedy_search` sobre un WAV de clase (fila A′ de D5).
- **(b)** Encontrar una forma de codificar los términos que **el motor no descarte**. Probar en este orden y quedarse con la primera que funcione:
  - **(b1)** generar un `bpe.vocab` a partir de `tokens.txt` (piezas con su puntuación por orden de id), y configurar `modeling_unit = "bpe"` y `bpe_vocab`;
  - **(b2)** `modeling_unit` fijado explícitamente, con los términos ya tokenizados con piezas de `tokens.txt`. Probar al menos dos segmentaciones: la de coincidencia más larga y otra alternativa.
  - Antes de nada, comprobar en el código C++ de sherpa-onnx 1.13.8 (el `sherpa-onnx-sys` del registro, o la documentación de la versión) qué separador usa `create_stream_with_hotwords` entre términos.
- **(c)** Distinguir «palabras clave activas» de «sin efecto»:
  - capturar el registro de la DLL, contar los «Encode hotwords failed» y los términos rechazados por la tokenización (caracteres ausentes en `tokens.txt`: dígitos, guiones, siglas);
  - demostrar con un término concreto, que el modelo falla sin palabras clave sobre un tramo real, que **con** ellas sale bien;
  - si todos los términos se descartan o ninguno cambia nada, la exploración **falla**.
- **(d)** Normalizar los términos al uso de mayúsculas que emite el modelo, quitar `\0` y separadores, y descartar los que no se pueden tokenizar o tienen menos de 4 caracteres.

Si la exploración funciona:

- `SherpaModelo::cargar` usa `modified_beam_search` **siempre** (un solo reconocedor; no hay dos instancias de 155 MB) y `max_active_paths = 4`;
- `nuevo_flujo_con_terminos` crea los flujos de las dos pistas;
- con el glosario vacío se llama a `create_stream()` sobre ese mismo reconocedor de haz. Por eso hay que **medir la fila A′** (haz sin palabras clave);
- `hotwords_score` empieza en 1,5 y se ajusta midiendo **inserciones** (D5).

### D4. Topes de carga medidos de verdad

El replay es de **dos pistas a la vez** (`system.wav` y `mic.wav` de la misma sesión), a una CPU, sobre **≥ 10 min** de la clase larga. Topes:

- **Carga de cómputo** = suma de la inferencia de las dos pistas / duración del audio ≤ **0,6**;
- **la cola no crece**: la cola máxima de los últimos 5 min no supera la de los primeros 5 min en más de 2 trabajos;
- **edad del audio p95 ≤ 400 ms**.

Se mide con el glosario a tamaño real: **todos** los términos válidos de un prompt de 800 caracteres, no 10. Si no se cumplen los topes con haz, se prueba `max_active_paths = 2`. Si aún así no cumple, se vuelve a `greedy_search` sin palabras clave y se informa.

### D5. Medición que no se engañe a sí misma

Sobre los pares `mic.wav`/`system.wav` de `%APPDATA%\dictar_ia\audio\ses_*`: Comunicaciones (132 s, eco 0,744), Diseño de Sistemas (158 s) y los 10 primeros minutos de la clase de 57,7 min. Cinco configuraciones:

| Fila | Decodificación | Nivelación (sistema) | Palabras clave |
|---|---|---|---|
| A | greedy | no | no |
| A′ | haz | no | no |
| B | greedy | sí | no |
| C | haz | no | sí |
| D | haz | sí | sí |

**Referencia de términos:**

- de la pasada Whisper Small de esas sesiones se extraen ~20 términos técnicos;
- se dividen en **H** (10, se dan como palabras clave) y **E** (10, **no** se dan; sirven de control).

**Por fila se reporta:**

- carga de D4, cola y edad;
- número de frases y palabras, con su diferencia frente a A;
- **aciertos** de H y de E: apariciones bien escritas en una ventana de ±5 s respecto de Whisper;
- **inserciones** de H: el término aparece en Sherpa y no en Whisper dentro de ±5 s;
- **duplicados entre pistas**: frases de la pista de micrófono que repiten texto de la de sistema en ±3 s. Es un control: no debería cambiar, porque el micrófono no se nivela;
- tres fragmentos de 30 s comparados lado a lado, A frente a D.

**Reglas de decisión:**

- La nivelación se activa por defecto solo si B mejora a A en aciertos (H ∪ E) **sin** aumentar duplicados ni reducir palabras más de un 5 %.
- Las palabras clave se activan solo si C mejora los aciertos de H frente a A′ con **inserciones ≤ 2 por cada 10 min**, y E no empeora.
- Si no se cumplen, quedan apagadas tras su constante y se informa con los números.

### D6. Interruptores de evaluación, inequívocos

Se leen de variables de entorno **solo** para el arnés, con un único sitio de lectura en `sherpa.rs`/`transcriptor.rs`:

- `DICTAR_SHERPA_DECODIFICACION=greedy|haz`
- `DICTAR_STT_NIVELAR=0|1`
- `DICTAR_SHERPA_PALABRAS_CLAVE=0|1`

Los valores de producción son `const` y se usan cuando la variable falta. El arnés imprime la configuración efectiva al empezar.

## Fuera de alcance

- Vosk (inglés).
- La nivelación del micrófono (va con AV-4, que quita el eco).
- El camino Whisper en vivo.
- La pasada final.
- AV-6 (ruido) y AV-7 (corte de frases y puntuación).

## Archivos (lista cerrada)

| Ruta | Cambio |
|---|---|
| `core/stt/src/nivel.rs` | **Nuevo**, D2, con pruebas |
| `core/stt/src/lib.rs` | `pub mod nivel;` |
| `core/stt/src/sherpa.rs` | Haz y palabras clave (D3), preparación y tokenización de términos con pruebas, lectura de D6, saneamiento de `\0` |
| `core/api/src/transcriptor.rs` | Nivelación solo de sistema con Sherpa (D1); pasar los términos a `crear_reconocedores_sherpa`; lectura de D6 |
| `core/api/examples/sherpa_live_eval.rs` | **Nuevo**: arnés de D4/D5 con las dos pistas, a una CPU, con métricas de cola y salida de las tablas |
| `_orquestacion/trabajo/AV-3-nivel-y-glosario/bpe.vocab` | Solo si (b1) funciona: el archivo generado, como evidencia. Lo que se empaquete con el modelo se decidirá en otra tarea |

**Disjunta** de AV-1 y de AV-2. No toca ningún `Cargo.toml`.

## Criterios de aceptación

1. **`cargo test -p dictar-stt`.** Cubre:
   - nivel: tono a −40 dBFS que sube sin superar el límite de subida;
   - el silencio no sube la ganancia;
   - la salida nunca pasa de 0,98 y es continua;
   - el rango de −6/+20 dB se respeta;
   - bloques de 1024 con ventanas de 30 ms dan el mismo resultado que un único bloque;
   - la ganancia se conserva en un reinicio por hueco;
   - preparación de términos: rechazados contados, normalización de mayúsculas, `\0` eliminado.
2. `cargo clippy -p dictar-stt -p dictar-api --all-targets -- -D warnings` limpio.
3. **Exploración D3** documentada en HANDOFF, con el resultado de (a) a (d).
4. **Tablas de D4 y D5** en HANDOFF, con los comandos exactos y la decisión aplicada según las reglas.
5. **Sesión real de 2 min** con la app o con `dictar sesion`: arranca, transcribe en vivo, guarda frases y no duplica entre pistas. Si las palabras clave quedaron activas, el registro muestra cuántos términos se cargaron y cuántos se rechazaron.

## Respuesta al contradictor (revisión 1 → 2)

| Objeción | Respuesta |
|---|---|
| 1. La métrica premia el sesgo | Aceptada. Conjuntos H y E separados, inserciones y diferencias de palabras y frases, con reglas de decisión |
| 2. Reconocedor único; «flujo normal» falso; falta la fila del haz | Aceptada. Haz siempre (si la exploración funciona), fila A′ |
| 3. Interruptores sin definir | Aceptada. D6 |
| 4. Los topes no miden la carga; una sola pista | Aceptada. D4 con carga, pendiente de la cola, dos pistas y 10 min |
| 5. Nivelar el micrófono amplifica el eco | Aceptada. Solo sistema |
| 6. Vosk contradictorio | Aceptada. Fuera, sin nivelación |
| 7. Detalles de las palabras clave | Aceptada. D3 (b1)/(b2), separador, mayúsculas, rechazos, criterio de «sin efecto» y glosario a tamaño real |
| 8. Limitador, ventanas con estado, ganancia en el hueco, clon | Aceptada. D2 |
| Ruta del modelo equivocada | Corregida |
