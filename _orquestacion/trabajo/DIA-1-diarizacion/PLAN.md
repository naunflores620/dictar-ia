# PLAN: DIA-1 «Saber quién dijo qué en una reunión con varias personas»

**Origen:** pedido del PO (13/10), rama `diarizacion`, salida de `audio-vivo` (`e71e880`). Contexto en [investigacion-notion-reuniones-2026-10-12.md](../../../docs/investigacion-notion-reuniones-2026-10-12.md): Notion solo etiqueta bien en llamadas 1:1 y en inglés.

**Revisión 1.** Pendiente del contradictor.

## Problema

Hoy el hablante sale del **canal**: todo lo del micrófono es «Tú» y todo lo de los altavoces es un único nombre (`nombre_interlocutor`, `core/api/src/proceso.rs:659`: la persona de la asignatura o cliente, o «Cliente», «Profesor» o «Interlocutor»). Casos reales del PO:

1. Él y su socio en la misma sala, con una llamada a un cliente: el socio sale como «Tú».
2. El socio en remoto: socio y cliente salen con el mismo nombre.
3. Dos socios en remoto más el cliente: los tres salen igual.
4. Reunión presencial sin llamada: todos salen como «Tú».

## Alcance de esta tarea

Solo el punto 1 de la propuesta al PO: **diarización al terminar la reunión, dentro de cada canal, y renombrado por el usuario**. La diarización en vivo y la memoria de voces entre reuniones quedan fuera (DIA-2 y DIA-3).

## Decisiones de diseño (las toma el orquestador)

### D1. Motor

`sherpa_onnx::OfflineSpeakerDiarization` (crate 1.13.8, ya en el proyecto) con:

- **segmentación:** pyannote (`sherpa-onnx-pyannote-segmentation-3-0`, unos 6 MB);
- **huellas de voz:** un extractor de embeddings de los que publica sherpa-onnx (por ejemplo 3D-Speaker `eres2net` o `campplus`, unos 30 MB). El implementador elige el más pequeño que funcione bien en español y lo justifica;
- **agrupado:** `FastClusteringConfig` con `num_clusters = -1` (número desconocido) y un `threshold` que se calibra en D7.

No se añade ninguna dependencia nueva de Rust.

### D2. Cuándo y sobre qué

- En `procesar_sesion`, **después** de confirmar la transcripción y **antes** de generar las notas, para que el LLM reciba los hablantes.
- **Por canal:** `system.wav` y `mic.wav` por separado. Sobre el micrófono, solo si tiene voz propia (casos 1 y 4).
- **Por tipo de sesión:** se aplica a `ClientMeeting`, a las reuniones de equipo y a las sesiones importadas. **No** a `Class`.
- Si la diarización falla o tarda más de lo previsto, la sesión sigue sin ella y se registra el motivo. No debe romper nunca el procesado.

### D3. Asignar frases a hablantes

Cada frase (`utterance`, que ya tiene la columna `speaker`, `migrations.rs:83`) recibe el hablante cuyo segmento se **solapa más** con su intervalo, dentro de su canal. Etiquetas por defecto:

- en el sistema, «Persona 1», «Persona 2»… El nombre de la asignatura o cliente se mantiene si solo se detecta **una** voz;
- en el micrófono, «Tú» para la voz con más tiempo y «Persona en sala 1», «Persona en sala 2»… para las demás.

Si en un canal se detecta una sola voz, la etiqueta es la de hoy, sin cambios visibles.

### D4. Renombrar

- Tabla nueva `speaker_alias(session_id, track, etiqueta, nombre)`, en una migración nueva.
- Renombrar «Persona 2» → «Carlos (socio)» actualiza cómo se muestran **todas** sus frases de esa sesión.
- La exportación a Markdown y las notas usan el nombre.
- Las notas ya generadas no se rehacen solas. Conectar el botón «Regenerar» (`sesion.dart:283`, hoy sin acción) para regenerarlas con los nombres nuevos **entra en esta tarea**, porque si no renombrar sirve a medias.

### D5. Puente e interfaz

- Hacen falta funciones nuevas en el puente: listar hablantes de una sesión, renombrar y regenerar notas. **Requiere `flutter_rust_bridge_codegen` 2.12.0, que no está instalado (B-5).**
- **Paso 0:** instalarlo con `cargo install flutter_rust_bridge_codegen --version 2.12.0 --locked`. Si Smart App Control bloquea sus build scripts (`os error 4551`, ver la memoria del proyecto), **no se rodea**: se detiene y se informa. En ese caso el orquestador decidirá una vía sin regenerar el puente.
- **Interfaz:** en la pantalla de sesión, un apartado «Participantes» con cada etiqueta detectada, su canal, tiempo hablado y un campo para renombrar, más el botón Regenerar conectado.

### D6. Modelos y empaquetado

- Los modelos de D1 se descargan y empaquetan igual que los de Vosk y Sherpa: ampliar `packaging/windows/preparar_vosk.ps1` (o un script hermano) y que el instalador los lleve.
- Las licencias y atribuciones van junto a las existentes.
- Se buscan en el directorio del ejecutable y en `%LOCALAPPDATA%\dictar_ia\models`, como el resto.

### D7. Medición

**Paso 1.** Antes de integrar en el procesado, un arnés (`core/api/examples/diarizacion_eval.rs`) corre la diarización sobre audio real y saca tiempo, RTF y número de hablantes detectados.

- **Audio:** las sesiones del PO en `%APPDATA%\dictar_ia\audio\ses_*`. Una clase debería dar 1 hablante en el sistema, y conviene buscar alguna sesión con varias voces.
- **Prueba controlada:** si no hay ninguna sesión con varias voces, concatenar fragmentos de dos sesiones distintas, cuyos profesores son voces distintas, para tener una verdad conocida.
- **Umbral:** se elige el `threshold` que acierta el número de voces en esos casos y se documenta.
- **Tope de coste:** RTF ≤ 0,1 en esta máquina, es decir, 1 h de reunión en ≤ 6 min. Si no se cumple, se informa antes de integrar.

## Fuera de alcance

- Diarización en vivo (DIA-2).
- Memoria de voces entre reuniones (DIA-3).
- Android y Linux más allá de que compilen.
- Clases.

## Archivos (orientativo; el implementador declara la lista final en HANDOFF)

| Ruta | Cambio |
|---|---|
| `core/stt/src/diarizacion.rs` | **Nuevo:** envoltorio de sherpa, rutas de modelos, asignación de frases (pura y probada) |
| `core/api/src/proceso.rs` | Llamada a la diarización entre la transcripción y las notas (D2), etiquetas de D3 |
| `core/storage/src/migrations.rs`, `core/storage/src/transcript.rs` (o un `speakers.rs` nuevo) | `speaker_alias`, lectura de frases con su nombre |
| `core/notes/…` | Que el contexto del LLM use el nombre del hablante si ya no lo hace |
| `core/api/src/puente.rs`, `frb_generated.rs`, `app/lib/src/rust/**` | Funciones nuevas (D5), regenerado con codegen |
| `app/lib/pantallas/sesion.dart`, `app/lib/datos/*.dart` | «Participantes» y Regenerar |
| `packaging/windows/*` | Modelos de D6 |
| `core/api/examples/diarizacion_eval.rs` | **Nuevo:** D7 |

## Criterios de aceptación

1. **Arnés D7:** en una clase, 1 voz en el sistema. En la mezcla controlada de dos profesores, 2 voces, con ≥ 85 % del tiempo de cada uno asignado a su etiqueta. RTF y umbral documentados.
2. **Pruebas puras de asignación por solapamiento:** frase partida entre dos hablantes, frase sin solape, canal con una sola voz que conserva su etiqueta de hoy.
3. **Migración:** prueba de `speaker_alias` y de la lectura con nombres.
4. **Procesado:** si falla la diarización (por ejemplo sin modelos), la sesión se procesa igual y las etiquetas quedan como hoy.
5. `cargo test` de los crates tocados y `flutter test`, en verde.
6. Instalador con los modelos, y una prueba manual del PO con una reunión real de 2 o más personas.
