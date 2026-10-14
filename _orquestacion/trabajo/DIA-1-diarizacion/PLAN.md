# PLAN: DIA-1 «Saber quién dijo qué en una reunión con varias personas»

**Origen:** pedido del PO (13/10), rama `diarizacion`, salida de `audio-vivo` (`e71e880`).

**Revisión 2**, tras el contradictor. Ver «Respuesta al contradictor» al final.

## Problema

Hoy el hablante sale del **canal**:

- el micrófono se etiqueta **«Yo»** (`proceso.rs:461`, `transcriptor.rs:687/707/1287`, `transcript.rs:201`);
- el sistema recibe un único nombre, el de `nombre_interlocutor` (`proceso.rs:659`): la persona de la asignatura o cliente, o «Cliente», «Profesor» o «Interlocutor».

Socio en la sala, socio en remoto, dos socios en remoto o reunión presencial: en todos los casos hay personas distintas bajo la misma etiqueta.

## Alcance: dos partes

**Parte A, el núcleo.** No depende del generador del puente. Diariza al cerrar, etiqueta frases y hace que los apuntes entiendan las etiquetas.

**Parte B, la interfaz.** Renombrar participantes y conectar Regenerar. **Depende de `flutter_rust_bridge_codegen` (B-5).**

Fuera de alcance, declarado:

- diarización en vivo (DIA-2);
- memoria de voces entre reuniones (DIA-3);
- diarizar sesiones ya procesadas, es decir, un botón «Detectar participantes» (DIA-4);
- clases (`Class`) y notas de voz (`VoiceNote`).

## Parte A: decisiones de diseño

### A1. Motor y parámetros

`sherpa_onnx::OfflineSpeakerDiarization` (crate 1.13.8, enlazada como DLL compartida).

- **Modelos:** segmentación pyannote 3.0 y un extractor de embeddings de 3D-Speaker; el implementador elige el más pequeño que funcione y lo justifica.
- **Comprobación en el Paso 1:** que la DLL empaquetada expone la diarización.
- **Formato:** `leer_wav` ya da mono a 16 kHz. El envoltorio compara con `sample_rate()` del diarizador y falla con un mensaje claro si no coincide.
- **Hilos:** `num_threads = min(4, núcleos lógicos / 2)`.
- **Ventana:** `window_shift_ratio` por defecto, salvo que la medición de A7 obligue a cambiarlo.
- **Agrupado:** `num_clusters = -1` y `threshold` calibrado en A7.

### A2. Cuándo y sobre qué

- **Tipos de sesión:** solo `ClientMeeting` y `TeamMeeting`, sea cual sea la fuente (también importadas).
- **Momento:** en `procesar_sesion`, justo antes de `solo_notas`. Es el punto al que llegan todos los caminos (`proceso.rs:205,222`) y queda después de `suprimir_diafonia`, que conserva `speaker`.
- **Pista de sistema:** se diariza `system.wav` tal cual.
- **Pista de micrófono:** solo si tiene voz propia, y **siempre sobre el audio con el retorno atenuado**, con el mismo `detectar_retorno_en_wav` y `atenuar_microfono` de `transcribir_lote`. Si no se detecta retorno (auriculares, o sin loopback), se diariza tal cual.
- **Clústeres sin frases** asignadas tras A3 (eco residual, ruido): se descartan. No aparecen como participantes.
- **Idempotencia:** si la sesión ya tiene etiquetas de diarización (por ejemplo un marcador `diarizacion.ok` en su carpeta de audio, o una columna de estado), no se rehace. Si una pasada completa reescribe las frases, porque `reemplazar_por_definitiva` las borra, el marcador se invalida y se vuelve a diarizar. **Se pierden entonces los renombres de esa sesión. Se acepta y se declara.**
- **Sin bloquear la sesión:**
  - si faltan modelos o la diarización devuelve error, la sesión sigue con las etiquetas de hoy y se registra el motivo;
  - **no hay tiempo límite**, porque la API no se puede cancelar. A cambio, `FaseProceso` nueva («Identificando participantes»), o un texto de progreso, para que no parezca colgada;
  - el inyectable para probar el fallo es la resolución de rutas de modelos: una función que devuelve `Option<Rutas>` y que la prueba sustituye.

### A3. Asignar frases a hablantes

Función pura, con pruebas que fijan **qué hablante gana y con qué números**:

- cada frase recibe el clúster con **mayor solape** dentro de su pista;
- **sin solape:** el clúster del segmento más cercano si está a ≤ 1 s; si no, la etiqueta por defecto del canal;
- **empate exacto:** gana el clúster con más tiempo total en la sesión.

### A4. Etiquetas

- **Micrófono:**
  - una sola voz: **«Yo»**, como hoy;
  - varias: «Yo» para la de más tiempo y **«Persona en sala 1, 2…»** para las demás.
- **Sistema:**
  - una sola voz: `nombre_interlocutor`, como hoy;
  - varias: **«Participante 1, 2…»**, ordenados por aparición.

### A5. Que los apuntes entiendan las etiquetas

- `core/notes/src/prompts.rs:134` y `core/notes/src/chunking.rs:81` hoy dicen «"Yo" soy yo; "Interlocutor" es el cliente».
- Se generaliza: **«"Yo" es el usuario; el resto de etiquetas son otros participantes. Si una etiqueta es un nombre propio, úsalo.»** En reuniones con cliente, si hay varias etiquetas de sistema, el prompt no debe suponer cuál es el cliente.
- Con una sola voz por canal, el texto resultante sigue funcionando igual que hoy. Una prueba lo fija.

### A6. Renombrar sin tabla de alias (lo usa la Parte B)

En vez de `speaker_alias`, renombrar **reescribe `utterance.speaker`** de esa sesión y pista: de «Participante 2» a «Carlos (socio)».

- Es estable frente a la numeración de clústeres.
- Renombrar dos etiquetas con el mismo nombre las **fusiona**, lo que resuelve la sobresegmentación.
- La lista de participantes se deriva de las frases: se agrupa `speaker` en `obtener_transcripcion`, que ya devuelve `speaker` en `FraseDto`.
- La función de núcleo `Nucleo::renombrar_hablante(sesion, pista, de, a)`, con prueba, va en la Parte A.

### A7. Medición antes de integrar

`core/api/examples/diarizacion_eval.rs`, sobre audio real de `%APPDATA%\dictar_ia\audio\ses_*`:

1. **Una clase:** 1 voz en el sistema.
2. **Mezcla controlada** de fragmentos de dos sesiones con profesores distintos: 2 voces, con ≥ 85 % del tiempo de cada uno bien asignado.
3. **Pista larga sintética** de 1 h (audio repetido): RTF, **pico de RAM** e hilos usados. Tope: **RTF ≤ 0,1 por canal**. Si no se cumple, se informa **antes** de integrar, sin decidir solo.
4. **Deriva:** fin de la última frase frente a duración del WAV, en 2 sesiones largas.
5. **Micrófono con altavoces** (la sesión de Comunicaciones, retorno 0,744): con atenuación no aparecen clústeres fantasma que acaben con frases.

Se documentan el umbral elegido y las limitaciones observadas: habla simultánea (una frase solo va a uno), voces parecidas, sobresegmentación e intervenciones de menos de 1 s.

## Parte B: decisiones de diseño

### B0. Paso previo

`cargo install flutter_rust_bridge_codegen --version 2.12.0 --locked --target-dir D:\dev_v1\dictar_ia\target`. Así se reutilizan los build scripts ya aceptados de las dependencias compartidas.

**Si Smart App Control o el clasificador de permisos lo bloquean:** se detiene la Parte B, se informa y **no se rodea**. La Parte A se entrega igual: las etiquetas se ven en la transcripción y en los apuntes, y renombrar queda pendiente.

### B1. Puente

Dos funciones nuevas:

- `renombrar_hablante(sesion, pista, de, a)`;
- `regenerar_notas(sesion)`, que llama **solo** a `solo_notas` (hoy privada), nunca a `procesar_sesion`.

Hay que comprobar si `guardar_notas` con `kind: "final"` reemplaza o acumula, y hacer que **reemplace**. El coste en LLM es el de una generación; se muestra como lo hace hoy el procesado.

### B2. Interfaz

En `sesion.dart`:

- apartado **«Participantes»**, derivado de las frases, con canal, tiempo hablado y campo para renombrar;
- **Regenerar** conectado a `regenerar_notas`.

## Archivos

| Ruta | Parte |
|---|---|
| `core/stt/src/diarizacion.rs` (nuevo: envoltorio, rutas, asignación pura) | A |
| `core/api/src/proceso.rs` (llamada, idempotencia, etiquetas, fase), `core/api/src/eventos.rs` (fase) | A |
| `core/api/src/lib.rs` (`renombrar_hablante`) y `core/storage/src/transcript.rs` (reescritura de `speaker`) | A |
| `core/notes/src/prompts.rs`, `core/notes/src/chunking.rs` | A |
| `core/api/examples/diarizacion_eval.rs` | A |
| `packaging/windows/*` (modelos y licencias) | A |
| `core/api/src/puente.rs`, `frb_generated.rs`, `app/lib/src/rust/**`, `app/lib/pantallas/sesion.dart`, `app/lib/datos/*.dart` | B |

## Criterios de aceptación

### Parte A

1. A7 completo en HANDOFF, con números.
2. Pruebas puras de A3: partida, sin solape a ≤ 1 s y a > 1 s, y empate, cada una con su ganador concreto.
3. Pruebas de A4 (una o varias voces por canal) y de A5 (el prompt con una sola voz sigue igual que hoy).
4. Prueba de que un fallo de modelos (sustituto de A2) deja la sesión procesada con las etiquetas de hoy.
5. Prueba de idempotencia y de que el marcador se invalida tras una pasada completa.
6. `cargo test` de los crates tocados en verde.
7. Instalador con modelos; anotar su tamaño.

### Parte B

8. Prueba de widget de «Participantes» y renombrado.
9. Regenerar no vuelve a transcribir: prueba en Rust de que `regenerar_notas` no llama a la transcripción.
10. Prueba manual del PO en una reunión real con 2 o más personas. **Expectativa declarada:** las intervenciones muy cortas, el habla simultánea y las voces parecidas pueden atribuirse mal.

## Respuesta al contradictor (revisión 1 → 2)

| Objeción | Respuesta |
|---|---|
| 1. Hablantes fantasma por eco en el micrófono | Aceptada. Se diariza el micrófono con el retorno atenuado; los clústeres sin frases se descartan; criterio A7.5 |
| 2. «Tú» frente a «Yo»; el prompt depende de las etiquetas | Aceptada. Se conserva «Yo»; A5 generaliza el prompt; `prompts.rs` y `chunking.rs` están en la lista |
| 3. Alias inestables y Regenerar | Aceptada. Sin tabla de alias: se reescribe `speaker`. Idempotencia con marcador e invalidación. `regenerar_notas` solo llama a `solo_notas`; las notas se reemplazan |
| 4. Dependencia del generador sin alternativa | Aceptada. Se parte en A (independiente) y B (bloqueable); B0 con `--target-dir` compartido |
| 5. Deriva de tiempos y desempate | Aceptada. A7.4 mide la deriva; A3 fija el desempate |
| 6. Coste, memoria y cancelación | Aceptada. Hilos fijados, pista de 1 h con RAM, RTF por canal; sin tiempo límite, con fase de progreso |
| 7. Sesiones ya procesadas | Fuera de alcance, DIA-4 |
| 8. Limitaciones del motor | Declaradas en A7 y en el criterio 10; renombrar dos etiquetas igual las fusiona |
| `VoiceNote` y «importadas» | Precisado en A2 |
