# HANDOFF — DIA-1 «Saber quién dijo qué en una reunión con varias personas»

Escrito por quien implementó (Sonnet 5.5), al entregar. Rama `diarizacion`, sin commit ni `git add`.

> **Para los revisores:** esto es una **declaración**, no evidencia. Todo lo que dice acá está
> por verificarse. Ver `protocolo.md`, regla 1.

## Estado

**Parcial en un punto y completa en el resto.**

- **Parte A: hecha y con pruebas, pero el criterio A7.5 NO se cumple** (hay "Persona en sala N"
  con frases en el micrófono de la sesión de Comunicaciones aunque se atenúe el retorno; ver la
  tabla de A7.5 y «Decisiones que necesitan al orquestador»). El resto de A7 pasa, con una
  condición: **el RTF y la separación de la mezcla solo pasan tras cambiar `window_shift_ratio`
  de 0,1 a 0,5**, cosa que el plan permite («salvo que la medición de A7 obligue»). Con el valor
  por omisión de Sherpa el RTF era 0,355 y la mezcla controlada no separaba (A 67,2 %): ahí la
  regla de parada ("parar y reportar") se habría disparado. Seguí porque el plan autoriza ese
  ajuste y con él ambos criterios pasan; lo dejo dicho para que lo decida quien corresponda.
- **Parte B: hecha** (B0 funcionó, sin Smart App Control ni bloqueo del clasificador). Puente
  regenerado, `renombrar_hablante` y `regenerar_notas` en Rust y Dart, apartado «Participantes»
  y «Regenerar» conectado, con pruebas.
- **Empaquetado:** `preparar_vosk.ps1` copia los modelos y sus licencias. **No se construyó el
  instalador** (no hay `ISCC` en esta máquina): el tamaño es una estimación, ver criterio 7.
- **No hice la prueba manual del PO (criterio 10).**

## Archivos tocados

| Ruta | Qué se hizo | ¿Estaba en el PLAN? |
|---|---|---|
| `core/stt/src/diarizacion.rs` (nuevo) | Envoltorio de `OfflineSpeakerDiarization`, rutas de modelos, opciones, asignación frase→voz (A3), etiquetas (A4), descarte de voces residuales. 19 pruebas (la última solo corre fuera de Windows: en Windows se ejecutan 18) | Sí |
| `core/stt/src/lib.rs` | `pub mod diarizacion;` y variante `SttError::NoSoportada(String)` | **No** (necesario: el módulo hay que declararlo, y la invariante 4 pide `NoSoportada` fuera de Windows) |
| `core/api/src/proceso.rs` | Llamada a la diarización antes de `solo_notas` en los dos caminos, marcador, invalidación (`guardar_pasada_completa`/`guardar_pasada_de_microfono`), `transcripcion_sin_eco` (extraído de `solo_notas`), `regenerar_notas`, `diarizar_participantes` (pública, para el arnés). 3 módulos de pruebas | Sí |
| `core/api/src/eventos.rs` | `FaseProceso::Identificando` | Sí |
| `core/api/src/lib.rs` | `Nucleo::renombrar_hablante` + prueba | Sí |
| `core/storage/src/transcript.rs` | `asignar_hablantes` y `renombrar_hablante` + 4 pruebas | Sí |
| `core/notes/src/prompts.rs` | Aviso de etiquetas común (macro `aviso_etiquetas!`) en `MAP`, `REDUCE_CLIENTE` y `REDUCE_EQUIPO`; `VERSION` v2 → v3; 2 pruebas | Sí |
| `core/notes/src/chunking.rs` | Solo 2 pruebas nuevas (`formatear` no cambia) | Sí |
| `core/api/examples/diarizacion_eval.rs` (nuevo) | Arnés de A7 | Sí |
| `packaging/windows/preparar_vosk.ps1` | Descarga con SHA-256 y copia de modelos y licencias | Sí (`packaging/windows/*`) |
| `core/api/src/puente.rs` | `renombrar_hablante`, `regenerar_notas`, `pista_desde_texto` + prueba | Sí |
| `core/api/src/frb_generated.rs`, `app/lib/src/rust/frb_generated.dart`, `app/lib/src/rust/puente.dart` | Regenerados con `flutter_rust_bridge_codegen 2.12.0` | Sí |
| `app/lib/datos/participantes.dart` (nuevo) | `Participante` y `participantesDe(frases)` | Sí (`app/lib/datos/*.dart`). Lo puse acá y no en `modelos/dominio.dart` porque ese no estaba declarado |
| `app/lib/datos/repositorio.dart`, `repositorio_rust.dart` | `renombrarParticipante`, `regenerarNotas` (contrato, demo y Rust) | Sí |
| `app/lib/pantallas/sesion.dart` | Panel «Participantes», diálogo de renombrado, «Regenerar» conectado | Sí |
| `app/test/participantes_test.dart` (nuevo) | 8 pruebas | El plan pide la prueba (criterio 8) pero no nombra el archivo |

No toqué `core/storage/src/notes_store.rs` (ver «Decisiones que se apartan»). Efectos fuera del
árbol: modelos en `%LOCALAPPDATA%\dictar_ia\models\diarizacion\` y `flutter_rust_bridge_codegen` y
`cargo-expand` instalados en `~/.cargo/bin` (el generador instaló `cargo-expand` por su cuenta).

## Paso de medición A7 (resultados reales)

Máquina: 8 núcleos lógicos. Audio real de `%APPDATA%\dictar_ia\audio\ses_*`. Binario
`target\release\examples\diarizacion_eval.exe`. Hilos: 4 por modelo (`min(4, 8/2)`).

### Modelos

| Modelo | Versión / origen | Tamaño | SHA-256 | Licencia |
|---|---|---|---|---|
| Segmentación pyannote 3.0 | `github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2`; se usa `model.onnx` | tar 6.958.444 B; `model.onnx` 5.992.913 B (hay `model.int8.onnx` de 1.540.506 B, **no evaluado**) | tar `24615ee8…6488`; onnx `220ad67c…1079` | MIT (CNRS), `LICENSE` dentro del tar |
| Embeddings 3D-Speaker CAM++ | `github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_zh-cn_16k-common.onnx` | 28.281.138 B | `f682b514…d11` | Apache-2.0 (repositorio 3D-Speaker; descargué su `LICENSE`) |

Elección: CAM++ es el más pequeño de los tres que publica sherpa (ERes2Net-base 39.593.761 B,
ERes2Net 220.623.485 B). **No comparé la exactitud con los otros dos**: con el más pequeño la
mezcla controlada ya pasa. La licencia de CAM++ la tomé del repositorio 3D-Speaker, no de un
archivo dentro del `.onnx`.

La DLL empaquetada (`target\release\sherpa-onnx-c-api.dll`) expone la diarización: contiene
`SherpaOnnxOfflineSpeakerDiarizationProcess` y todo el arnés corrió contra ella.

### Parámetros elegidos (y por qué)

| Parámetro | Valor | Evidencia |
|---|---|---|
| `window_shift_ratio` | **0,5** (Sherpa: 0,1) | Clip de 158 s, 4 hilos: 0,1 → RTF 0,355 (8 hilos: 0,574); 0,25 → 0,126 (8 hilos: 0,289); 0,5 → 0,0725 (8 hilos: 0,113). Con 0,1 la mezcla da A 67,2 % (no llega a 85 %); con 0,25 da A 45,7 % / B 63,6 % (sobresegmenta); con 0,5 pasa |
| Hilos | 4 | Con 8 hilos va más lento (RTF 0,113 vs 0,0725): los núcleos eficientes no ayudan |
| `threshold` | **0,7** | Con ventana 0,5 y umbrales 0,4/0,5/0,6/0,7/0,8 la mezcla A/B da lo mismo (A 98,3–99,1 %, B 90,6–91,0 %); cambia el nº de clústeres: 6/6/6/5/3. 0,7 deja pocos clústeres espurios sin acercarse a fundir voces |
| `HABLA_MINIMA_VOZ_MS` | **3000** (regla nueva, ver desviaciones) | Clústeres de 0,7–1,1 s aparecían como «Participante 3» en clases de una sola voz |

### A7.1 — una voz en el sistema

| Sesión | Resultado (config. final) |
|---|---|
| `ses_3e9de…` (2,6 min) | 1 voz en el sistema (21 frases «Cliente»), 1 en el micrófono |
| `ses_656c…` (2,2 min) | 1 voz en el sistema; el micrófono: Sherpa imprime «No speakers found» y devuelve 0 voces (las 7 frases quedan «Yo») |
| `ses_96292…` (12 min, clase) | **3 voces con frases** en el sistema: una de 586,7 s (44 frases) y otras dos de 2,1 s y 33,2 s |
| `ses_4b0ac…` (57,7 min, clase) | **5 voces con frases**: 486 frases/4821 s la principal; las otras 9, 1, 6 y 2 frases |
| `ses_46801…` (17,8 min) | 3 voces con frases |

En clips cortos, una voz → una voz. **En las clases largas aparecen voces extra que no puedo
confirmar ni descartar** sin oír el audio (puede haber alumnos preguntando).

### A7.2 — mezcla controlada (`ses_4b0ac` Comunicaciones + `ses_96292` Análisis Financiero; 10 fragmentos de 30 s por voz, 10 min en total)

Definición de «bien asignado»: del tiempo de habla diarizado dentro de los fragmentos de una
voz, la parte que cae en el clúster de mayor solape.

| | Voz A | Voz B |
|---|---|---|
| Config. final (0,7 / 0,5) | **98,3 %** (257,6 s de 262,0 s) | **90,6 %** (247,4 s de 273,0 s) |

Nota sobre B: un clúster de 24,0 s (8,8 % de B) cae casi entero en B y es estable con todos los
umbrales; puede ser otra persona en la grabación de B y no un error. **Control con la misma
voz** (el mismo audio, principio vs. +1800 s): 1 clúster dominante, **99,7 % y 99,9 %**; no
parte una voz. Control con otra sesión de la misma materia: se separó 99,0 % (eran voces
distintas).

### A7.3 — pista sintética de 1 h (audio de `ses_4b0ac` repetido)

| Medida | Valor |
|---|---|
| Reloj de la diarización | 89,0 s → **RTF 0,0247** (tope 0,1) |
| CPU | 285,4 s → 3,21 núcleos efectivos de 4 configurados |
| Pico de RAM del proceso | **750 MiB** (715 MiB antes de diarizar). Incluye los búferes del propio arnés (el WAV leído más la pista de 1 h, ~460 MB), así que el motor añade del orden de 35 MiB sobre eso; **la cifra aislada del motor no está medida** |
| Clústeres/segmentos | 18 / 693 |

RTF por canal en sesiones reales (config. final): sistema 0,0259–0,0306 y micrófono 0,0092–0,0142
(`ses_96292`, `ses_46801`, `ses_4b0ac`). Diarizar las dos pistas de una hora son ~2,2 min.

### A7.4 — deriva de tiempos

| Sesión | WAV | Fin de la última frase | Último segmento diarizado |
|---|---|---|---|
| `ses_96292` sistema (12,0 min) | 721.261 ms | +1.301 ms | −1.629 ms |
| `ses_46801` sistema (17,8 min) | 1.066.858 ms | −39.648 ms (sesión que quedó `transcribing`; el audio terminó antes que el texto) | −49.298 ms |
| `ses_4b0ac` sistema (57,7 min) | 3.460.696 ms | **+21.832 ms** | +27 ms |

El +21,8 s de `ses_4b0ac` no es deriva: es la última frase, de una ventana fija de 30 s que
empieza dentro del audio y acaba fuera (inicio de esa frase: 3.452.528 ms, antes del fin del
WAV). Conclusión honesta: **no veo deriva grande, pero sin una verdad de referencia no puedo
medir desfases de menos de unos segundos**. El 1,3 s de `ses_96292` está bajo la tolerancia de
2 s que ya usa `proceso.rs`.

### A7.5 — micrófono con altavoces (`ses_4b0ac`, Comunicaciones): **NO CUMPLIDO**

El retorno detectado en esa sesión fue de **correlación 0,709** (el plan hablaba de 0,744; es la
misma sesión, lo que mide `detectar_retorno_en_wav` hoy). Medido por el camino de producción
(`Nucleo::diarizar_participantes` sobre una copia de los datos, marcando la sesión como reunión):

| | Voces del micrófono con frases |
|---|---|
| Con retorno atenuado (producción) | **14**: «Yo» (159 frases) + «Persona en sala 1…13» |
| Sin atenuar (control) | 12 |

Lo que sé y lo que no:

- La atenuación **no elimina** los clústeres; los reduce poco (14 → 12 es incluso al revés: hay
  *más* con atenuación).
- Las frases de esa sesión vienen de un transcriptor antiguo con ventanas fijas de 30 s y bucles
  alucinados («de 100, de 100, …»): frases de minutos que abarcan varios clústeres. Eso empeora
  la asignación, pero no la causa por sí solo.
- Probé si los clústeres suenan a la vez que el sistema (eco): 17–88 %, pero en esa clase el
  sistema suena el ~69 % del tiempo, así que **la medida no discrimina** y no la usé.
- No puedo decir cuántas de esas 13 voces son eco y cuántas personas reales.

Propuesta (no implementada, para decidir): si se detecta retorno (hay altavoces), **no partir
el micrófono**: todo «Yo». Cuesta poco y elimina el caso observado; a cambio, no se detecta a una
persona en la sala cuando se usan altavoces.

### Limitaciones observadas

Habla simultánea (una frase va a un solo clúster), voces parecidas (no probado), sobresegmentación
(5 voces en una clase de 57 min con un solo profesor probable), intervenciones de menos de 1 s
(se heredan del clúster vecino o quedan con la etiqueta del canal), y micrófono con altavoces
(A7.5). Los clústeres de menos de 3 s de habla en toda la pista se descartan: una intervención
real tan corta se atribuye a la voz más cercana.

## Comandos para reproducir

Todo en Git Bash, desde `D:\dev_v1\dictar_ia`, con `source entorno-msvc.sh; export CARGO_INCREMENTAL=0`
(target compartido).

```
cargo test -p dictar-stt -p dictar-storage -p dictar-notes -p dictar-api --lib --tests
cargo clippy -p dictar-stt -p dictar-storage -p dictar-notes -p dictar-api --all-targets --no-deps -- -D warnings -A clippy::arc_with_non_send_sync -A clippy::too_many_arguments
cargo fmt --all -- --check
cargo check --workspace --all-targets
cd app && flutter test && flutter analyze
flutter_rust_bridge_codegen generate --config-file D:\dev_v1\dictar_ia\flutter_rust_bridge.yaml
cargo build -p dictar-api --release --example diarizacion_eval
target\release\examples\diarizacion_eval.exe sesion|mic-sin-atenuar|mezcla|larga|integrada ...   (ver la cabecera del ejemplo)
```

**Resultados reales** (todos los comandos se ejecutaron):

| Comando | Resultado |
|---|---|
| `cargo test -p dictar-stt -p dictar-storage -p dictar-notes -p dictar-api --lib --tests` | exit 0. api 109 ok, notes 37 ok, storage 33 ok, stt 76 ok, 0 fallos |
| `cargo clippy … --no-deps -- -D warnings -A arc_with_non_send_sync -A too_many_arguments` | exit 0. **Con `-D warnings` a secas falla**, por código que no toqué: `vosk.rs:127` (`arc_with_non_send_sync`), `transcriptor.rs:59` y `:1262` (`too_many_arguments`) y, sin `--no-deps`, `audio-capture/src/lib.rs:304` (`unnecessary_cast`). El único lint propio (`explicit_counter_loop` en `etiquetar_pista`) lo corregí |
| `cargo fmt --all -- --check` | exit 1, **ya fallaba antes** (comprobado con `git stash`): varios archivos preexistentes (ejemplos, `vosk.rs`, `frb_generated.rs`, imports); el rustfmt de esta máquina ordena los `use` distinto de como está el repo. Mis archivos nuevos pasan; en los tocados solo quedan los diferencias de imports preexistentes (`proceso.rs:13` y `:22`, `lib.rs` de api y stt) |
| `cargo check --workspace --all-targets` | `Finished`, sin errores |
| `flutter test` | `All tests passed!` (51; 43 eran los previos) |
| `flutter analyze` | 1 `info` previo (`ajustes.dart:184`, `value` obsoleto); nada en lo mío |
| `flutter_rust_bridge_codegen generate --config-file <ruta absoluta>` | `Done!` (8 min; ver desviaciones) |
| `cargo test … doctests` | **No ejecutados** (ICE conocido) |

## Criterios de aceptación

| # | Criterio | Prueba que lo cubre | Estado |
|---|---|---|---|
| 1 | A7 completo con números | Sección «Paso de medición A7» | **Parcial**: A7.5 no se cumple (reportado) |
| 2 | A3: partida, sin solape ≤ 1 s y > 1 s, empate | `diarizacion::tests::una_frase_partida_…`, `sin_solape_y_a_menos_de_un_segundo_…`, `sin_solape_y_a_mas_de_un_segundo_…` (incluye el límite exacto de 1 s), `en_un_empate_exacto_…`, `si_el_empate_persiste_…` — todas ejecutan `hablante_de_frase` de producción con ganador concreto | Cubierto |
| 3 | A4 y A5 | A4: `una_voz_por_canal_conserva_…`, `varias_voces_en_el_sistema_…`, `en_el_microfono_yo_es_quien_mas_habla_…`, `etiquetar_frases_…`; en `proceso`: `varias_voces_por_canal_…`, `una_voz_por_canal_conserva_…`. A5: `prompts::los_prompts_que_muestran_la_transcripcion_explican_las_etiquetas`, `chunking::con_una_voz_por_canal_la_transcripcion_sale_igual_…`, `las_etiquetas_de_varias_voces_llegan_tal_cual_…` | Cubierto. Nota: A5 «el prompt con una sola voz sigue igual» lo interpreté como que `formatear` no cambia con una voz por canal y que el aviso es el mismo texto; los prompts son estáticos |
| 4 | Fallo de modelos deja las etiquetas de hoy | `proceso::tests_diarizacion::sin_modelos_la_sesion_sigue_…` (sustituye la resolución de rutas, como pide A2), `un_motor_que_no_carga_…`, `un_fallo_en_el_microfono_no_deja_la_sesion_a_medias`. **No** prueban `procesar_sesion` entero (necesita un LLM); prueban `diarizar_sesion_con`, que es lo que `procesar_sesion` llama | Cubierto con esa salvedad |
| 5 | Idempotencia e invalidación | `una_sesion_ya_diarizada_no_se_rehace` (conserva un nombre puesto a mano), `una_pasada_completa_invalida_el_marcador_…`, `la_pasada_que_solo_rehace_el_microfono_…` — las dos últimas llaman a `guardar_pasada_completa`/`_de_microfono`, que son las que usa `procesar_sesion` | Cubierto |
| 6 | `cargo test` de los crates tocados en verde | Tabla de resultados | Cubierto |
| 7 | Instalador con modelos; tamaño | `preparar_vosk.ps1` ejecutado **por trozos** (el bloque de diarización aislado, contra la red): descarga, hashes OK, extrae con `tar.exe`, deja `models\diarizacion\*.onnx` y las 3 licencias. **El instalador no se construyó.** Instalador actual: 246.927.597 B. Los dos modelos pesan 34.274.051 B; comprimidos con LZMA preset 6, ~30.912.748 B → estimo **~278 MB (+12,5 %)** | **Parcial** |
| 8 | Prueba de widget de «Participantes» y renombrado | `app/test/participantes_test.dart` (panel, renombrado, cancelar, regenerar, doble toque, `participantesDe`). Una de ellas destapó un fallo real mío (`setState` con un `Future`) | Cubierto |
| 9 | Regenerar no vuelve a transcribir | Rust: `tests_regenerar::regenerar_las_notas_no_transcribe_no_diariza_…` (única fase emitida: `GenerandoNotas`; frases, nombres y marcador intactos; el proveedor apunta a un puerto cerrado para no gastar dinero). Dart: `Regenerar vuelve a generar … sin volver a procesar` (`procesar` no se llama) | Cubierto |
| 10 | Prueba manual del PO | — | **Pendiente** |

## Invariantes del producto

| Invariante | Cómo se respeta acá |
|---|---|
| Escribir a disco antes de procesar | No se toca la captura. La diarización lee los WAV ya escritos y solo reescribe `utterance.speaker` |
| Dos pistas, nunca una mezcla | Se diariza cada WAV por separado y los clústeres no se comparan entre pistas. La mezcla de A7.2 es solo del arnés de medición |
| 16 kHz mono `f32` | `leer_wav` ya entrega eso; el envoltorio compara con `sample_rate()` del motor y falla con mensaje claro si no coincide |
| Firmas públicas iguales en todas las plataformas | `Diarizador::cargar/frecuencia/diarizar` existen siempre; fuera de Windows devuelven `SttError::NoSoportada`. **Esa rama no la pude compilar** (ver abajo) |

## Decisiones que se apartan del PLAN

1. **`window_shift_ratio` 0,5** en vez del defecto. El plan lo permite si A7 lo obliga; con el
   defecto el RTF era 0,355 y la mezcla fallaba. Ver la tabla de parámetros.
2. **Regla nueva no prevista: voces residuales** (`HABLA_MINIMA_VOZ_MS = 3000`,
   `quitar_voces_residuales`). Los clústeres con menos de 3 s de habla en toda la pista se
   descartan antes de asignar frases. Razón: en clases de una voz aparecían «Participante 3» de
   0,7 s. Efecto colateral declarado: una intervención real de menos de 3 s en toda la reunión
   se atribuye a la voz más cercana.
3. **Recorte de eco antes de diarizar** (`transcripcion_sin_eco`, extraído de `solo_notas`).
   El plan dice «clústeres sin frases se descartan»; para que el eco no cree voces que luego
   desaparecen, el filtro de diafonía corre antes. `solo_notas` llama a la misma función y no
   cambia de comportamiento.
4. **Todo o nada**: las etiquetas se aplican solo si las dos pistas se analizaron bien.
5. **El fallo se avisa a la interfaz** con un `Evento::Error` recuperable (sin modelos, o fallo
   del motor), además del registro. El plan decía «se registra el motivo»; el proyecto ya ha
   fallado en silencio.
6. **`guardar_notas` con `kind: "final"` ACUMULA, y no lo cambié.** El plan (B1) pide
   comprobarlo y hacer que reemplace; hacerlo exige tocar `core/storage/src/notes_store.rs`, que
   no está en la lista. Hoy `notas_finales` devuelve la más reciente por `created_at DESC`, y la
   prueba `las_notas_regeneradas_sustituyen_a_las_anteriores_al_leerlas` lo fija. Dos
   consecuencias: (a) con el mismo `created_at` el orden es ambiguo (Dart manda milisegundos, es
   poco probable); (b) `coste_sesion` suma todas las generaciones, que es lo correcto como gasto.
   **Necesita decisión**: añadir `, id DESC` al orden (una palabra) o borrar las `final`
   anteriores (perdería el histórico de coste).
7. **`flutter_rust_bridge_codegen generate` falló con el `yaml` por defecto** («prefix not found»,
   error de rutas de Windows: compara `D:\…` con `\\?\D:\…`). Se resuelve pasando
   `--config-file` con la **ruta absoluta del mismo yaml**: no cambié ningún valor. Además
   restauré a mano las 3 líneas de `use` que el generador reordena, para no meter ruido en
   `frb_generated.rs`. `frb_generated.dart` trae reformateo de `dart format` en trozos que no
   son míos.
8. `Nucleo::diarizar_participantes` es pública (para el arnés) aunque no se exponga al puente.

## Lo que NO pude verificar

- **El instalador**: no hay `ISCC` ni se hizo `flutter build windows`. El tamaño es una
  estimación (LZMA a preset 6, el instalador usa `lzma2/max`).
- **La rama no-Windows de `diarizacion.rs`** (`Diarizador` vacío + `NoSoportada`) y la prueba
  `fuera_de_windows_cargar_devuelve_no_soportada…`: `cargo check --target x86_64-unknown-linux-gnu`
  falla en el build script de `target-lexicon` (target dir nuevo, el bloqueo de binarios nuevos
  documentado en la memoria del equipo). La revisé a ojo, no compilada.
- **Una reunión real con 2 o más personas** (criterio 10). Toda la validación de voces separadas
  es con mezclas de clases grabadas. Las sesiones de que dispongo son clases con un profesor.
- **Qué voces extra son personas reales y cuáles ruido** en las clases largas (sin oírlas).
- **Pico de RAM aislado del motor** (solo tengo el del proceso entero con el arnés).
- **Que la nueva interfaz se vea bien**: solo pruebas de widget, no la aplicación en marcha, y no
  probé `RepositorioRust` de punta a punta (el puente se compila y el Dart analiza, nada más).
- `procesar_sesion` completo con diarización (necesita un LLM): probé las piezas, no el flujo.
- `release.yml` y `docs/05-empaquetado.md` no se actualizaron (no estaban en la lista). El
  `release.yml` solo comprueba la atribución de Sherpa; el `.ps1` ya falla si faltan los
  modelos o las licencias de diarización.
- Comparación con ERes2Net y con la segmentación `int8`: no medidas.

## Deuda que dejo

- **Decidir A7.5**: la propuesta de «sin partir el micrófono si hay retorno» (o endurecer el
  criterio para «Persona en sala»). Hoy el caso de uso principal (reunión con altavoces) puede
  llenar el micrófono de personas inexistentes.
- **`notes_store.rs`**: hacer que las notas regeneradas reemplacen de verdad (ver desviación 6).
- **`core/api/src/transcriptor.rs`** produce frases de 30 s con bucles («de 100, de 100…») en el
  micrófono de sesiones viejas; empeora cualquier asignación frase→voz. No es de esta tarea.
- **Baseline rojo de CI** que ya estaba: `cargo clippy -D warnings` (4 lints) y
  `cargo fmt --check` (orden de `use`). Convendría arreglarlo o fijar la versión de rustfmt.
- **DIA-4** (diarizar sesiones ya procesadas): el marcador y `diarizar_participantes` ya lo
  permiten; falta el botón y puentearla.
- Versión `int8` de la segmentación (−4,4 MB) y ERes2Net-base por si la exactitud lo pidiera.
- Un `Evento::Error` por cada reunión sin modelos puede resultar ruidoso si alguien instala sin
  ellos a propósito.

## Ronda 2

Decisiones del orquestador aplicadas.

1. **A7.5, regla nueva.** `Nucleo::microfono_con_altavoces(dir)` (usa `detectar_retorno_en_wav`,
   el mismo detector de la transcripción por lotes) decide en `intentar_diarizar`: con retorno,
   el micrófono **no se diariza** y todas sus frases quedan «Yo» (se reescriben si tenían otra
   etiqueta); sin retorno, o si el análisis falla, se diariza como antes. Se quitó la
   atenuación del audio (ya no hace falta: o se diariza sin retorno, o no se diariza).
   Pruebas, ambas sobre el punto de decisión de producción y WAV con envolventes sintéticas
   (el micrófono es una copia al 50 % retrasada 120 ms del sistema, o una secuencia
   independiente): `con_altavoces_el_microfono_no_se_diariza_y_todo_queda_como_yo` y
   `sin_altavoces_el_microfono_se_diariza_como_siempre`.
   **Arnés sobre `ses_4b0ac` (camino de producción, `integrada`):** `Hecha { voces_sistema: 5,
   voces_microfono: 1 }`; micrófono: «Yo», 277 frases (antes 14 voces, 13 «Persona en sala»);
   el sistema igual que antes (5 voces). Tarda 96,5 s en total.
2. **`window_shift_ratio` = 0,5**: comentario con las cifras en `DESPLAZAMIENTO_VENTANA`
   (`core/stt/src/diarizacion.rs`). Nota: 0,355 → 0,025 compara clip de 158 s con pista de 1 h;
   en el mismo clip la cifra es 0,355 → 0,0725.
3. **`guardar_notas`, sin cambios.** `notes.created_at` es `INTEGER NOT NULL` y `procesar_sesion`/
   `regenerar_notas` reciben `ahora` en milisegundos (Dart manda
   `DateTime.now().millisecondsSinceEpoch`). Una regeneración segundos después tiene un
   `created_at` mayor y `notas_finales` (`ORDER BY created_at DESC`) la devuelve. La resolución
   alcanza; solo habría empate con dos generaciones en el mismo milisegundo.

Resultado: `cargo test -p dictar-api --lib --tests` → `test result: ok. 111 passed; 0 failed`
(exit 0). Ojo: la primera ejecución tras compilar la dio bloqueada por Control de aplicaciones
(`os error 4551`, también un reintento); tras `touch` de `proceso.rs` y recompilar, se ejecutó.
No lo rodeé: es el bloqueo ya conocido de binarios recién compilados.

**Aviso final de la ronda 2:** después de la corrida en verde (111 pruebas) hice un último
cambio solo de formato en una prueba de `proceso.rs` (salto de línea de una llamada a `frase(…)`
para `cargo fmt`). Tres intentos de volver a correr `cargo test -p dictar-api --lib --tests` tras
ese cambio terminaron en `os error 4551` (Control de aplicaciones bloquea el ejecutable
recién compilado), así que **la versión final del archivo no se ejecutó**; el código es el mismo
que pasó, salvo el formato. `cargo clippy` (mismos flags de arriba) sí dio exit 0 con ese
archivo antes del reformateo; tras él compila (`cargo test` llega a enlazar), pero no se ejecutó.
