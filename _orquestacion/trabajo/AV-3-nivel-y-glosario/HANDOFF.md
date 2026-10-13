# HANDOFF — AV-3 «Que lo que llega al reconocedor en vivo esté nivelado y conozca el vocabulario de la asignatura»

Escrito por quien implementó, al entregar.

> **Para los revisores:** esto es una **declaración**, no evidencia. Todo lo que dice acá está
> por verificarse. Ver `protocolo.md`, regla 1.

## Estado

**Código completo y compilado; pruebas de `dictar-stt` pasan; exploración D3 hecha (resultado: las palabras clave NO funcionan con este modelo); mediciones D4/D5 hechas solo parcialmente y NO válidas para decidir. Cierre anticipado por prioridad del PO.**

El bloqueo de Smart App Control del primer intento se resolvió compilando con `CARGO_TARGET_DIR=D:\dev_v1\dictar_ia\target` (autorizado por el PO). Nada se rodeó.

### Comandos con códigos de salida reales

| Comando | Salida |
|---|---|
| `cargo test -p dictar-stt --lib` (dos veces, la última con `CARGO_INCREMENTAL=0`) | exit 0, `58 passed; 0 failed` (incluye 8 de `nivel` y 8 de `sherpa`) |
| `cargo test -p dictar-api --lib tests_nivelacion` (una vez, ANTES del aviso de contaminación del target compartido) | exit 0, 3 passed. **Repetir cuando el orquestador lo autorice**: el binario de pruebas se comparte con otras copias |
| `cargo check -p dictar-api --tests --examples` (`CARGO_INCREMENTAL=0`) | exit 0 |
| `cargo clippy -p dictar-stt -p dictar-api --all-targets -- -D warnings` | **exit 101 por errores que no son míos**: `dictar-audio/src/lib.rs:304` (`unnecessary_cast`) corta la compilación. Con `--no-deps` el corte pasa a `core/stt/src/vosk.rs:127` (`arc_with_non_send_sync`). Sin `-D`, los avisos que quedan en los dos crates son preexistentes (`vosk.rs:127`, `grabacion.rs:588`, `transcriptor.rs:59`, `:862`, `:1041`, estos últimos de funciones que no toqué): ninguno en `nivel.rs`, `sherpa.rs` ni en lo que añadí |
| `cargo build --release -p dictar-api --example sherpa_live_eval` | exit 0 (dos veces; la segunda tras `touch` de mis fuentes: el hash de artefactos del target compartido hizo que cargo reutilizara un `dictar-stt` de otra copia y fallara con `E0432`) |
| `sherpa_live_eval referencia` Diseño (158 s, `es`) | exit 0, 19 fragmentos, 736 s |
| `sherpa_live_eval referencia` clase larga (600 s, `es`) | exit 0, 275 fragmentos, 1024 s |
| `sherpa_live_eval explorar` (todas las variantes de abajo) | exit 0 |
| `repro` filas dis A, dis A', dis B, lar A | terminaron y escribieron `resumen.json`. **El script de lanzamiento no registró el código de salida** (`exit_codes.txt` quedó en UTF-16 sin número legible); consta solo que los `resumen.json` existen |
| `repro` fila lar A' | interrumpida por mí al cambiar la prioridad |

## Paso 0 (D3): resultado

Sobre Diseño de Sistemas (`ses_3e9de…`), 100-160 s, un solo flujo, sin reloj. Términos de prueba: `tablero, funcionalidades, épicas, historia de usuario, flujo de trabajo, administrador, finalizado, recordatorio, revisión, categoría, olas, backlog, scrum`.

- **(a) haz frente a greedy sin términos.** Comparables: 338 palabras y 10 frases (greedy) frente a 336 y 10 (haz, 4 rutas). El modelo ya emite mayúsculas y puntuación y acierta «backlog», «tablero» y «épicas».
- **(b) separador.** Confirmado `/`: el registro de la DLL muestra cada término como línea propia. La puntuación por término lleva **un espacio antes de los dos puntos** (` :-30`); con `verdad:-30` el motor lee `30` como carácter y falla.
- **(b) hallazgo clave sobre `cjkchar`.** El motor **separa el `▁` de la palabra** («▁ funciona li dad es»). Toda pieza con prefijo pegado (`▁funciona`, `▁historia`, `▁trabajo`, `▁final`, `▁reco`) da «Cannot find ID». Solo se puede expresar `▁` suelta seguida de piezas sin prefijo.
- **(b1) `bpe.vocab` generado desde `tokens.txt`** (también una versión con los especiales `<blk>`, `<sos/eos>`, `<unk>` en las tres primeras líneas para respetar el orden de ids): el motor lo **acepta** (0 errores) pero **no cambia nada**, ni con +20 («olas», que el modelo dice «alas») ni con −30 y −300 («verdad», «tablero», «ejercicio»). Los recuentos no varían (8, 3, 1). Mis puntuaciones (−id) no reproducen la segmentación con la que se entrenó el modelo.
- **(b2) piezas explícitas** (`larga` y `minimo`): 5 líneas «Cannot find ID» y 1 «Encode hotwords failed, skipping» con 13 términos. Las frases difieren de A′ en 4 líneas, pero son ruido de corte de frase, no efecto de términos.
- **(c) el mecanismo existe pero no sirve.** Con `cjkchar` y `▁ ver dad :-30`, «verdad» bajó de 8 a 6 apariciones: el grafo de contexto se aplica. Con `▁ ta ble ro :-30` y variantes, «tablero» no cambió: el modelo no emite esa descomposición. **No encontré ningún término que falle sin palabras clave y salga bien con ellas.**
- **Veredicto: la exploración FALLA** según D3(c). Hace falta la segmentación real del modelo (su `bpe.model`, o leer los tokens que emite para cada palabra, que `get_result` expone) y no se dispone de ninguna. La parte de palabras clave queda detenida.

## D4 / D5: lo que se midió y por qué NO vale

Filas A, A′ y B sobre Diseño (158 s) y A sobre los 10 primeros minutos de la clase larga; `repro` a ritmo real, afinidad `0x4`, `BelowNormal`. C y D no se ejecutaron (la exploración falló). Comunicaciones excluida (audio en inglés); no hay otra sesión española con mic y system.

| Fila | Sesión | carga | edad p95 ms | cola máx. | descartes | frases / palabras (sistema) |
|---|---|---|---|---|---|---|
| A (greedy) | Diseño | 0,293 | 1648 | 31 | 688 | 5 / 89 |
| A′ (haz) | Diseño | 0,555 | 7629 | 31 | 1961 | 3 / 55 |
| B (greedy + nivelar) | Diseño | 0,848 | 9659 | 31 | 3583 | 3 / 34 |
| A (greedy) | clase larga, 600 s | 0,443 | 5910 | 31 | 5203 | 31 / 319 |

**Estas cifras son inválidas para decidir.**
- La fila de control A ya descarta 688 bloques en Diseño. El replay del 08/10 de esa misma sesión tenía cero descartes y edad p95 de 167 ms. Aquí la máquina estaba cargada por compilaciones de otros agentes y el proceso iba fijado a una sola CPU.
- Con 688 a 5203 bloques descartados el texto no es comparable: 89 palabras (A) frente a 34 (B) mide descartes, no nivelación. La «carga» tampoco es comparable, porque los descartes reducen la inferencia.
- `puntuar` no se ha ejecutado nunca contra datos reales. La referencia Whisper Small sí existe (`ref_diseno.json`, `ref_larga.json`, con 20 términos H/E en `H.txt` y `E.txt`, en el scratchpad de la sesión).

**Reglas de D5: sin evidencia válida no se cumplen; todo queda apagado.**

## Recomendación para el ejecutable de prueba del PO

- **Palabras clave: APAGADAS** (`PALABRAS_CLAVE_POR_DEFECTO = false`). La exploración demostró que no producen efecto con este modelo.
- **Haz: APAGADO** (`HAZ_POR_DEFECTO = false`). Sin palabras clave no aporta nada; en lo poco medido costó casi el doble de CPU (0,555 frente a 0,293), aunque con ruido.
- **Nivelación de sistema: APAGADA** (`NIVELAR_SISTEMA_POR_DEFECTO = false`). No hay evidencia de que mejore y −20 dBFS sigue siendo una hipótesis. Está implementada y se activa con `DICTAR_STT_NIVELAR=1` en el entorno (solo pista de sistema y solo con Sherpa), por si el PO quiere compararla a oído.
- Con las tres en `false`, la producción se comporta como antes (salvo `modeling_unit = "cjkchar"` explícito, que era el valor por omisión).

## Hallazgo aparte: las frases «definitivas» de `dictar.db` están rotas

Para `ses_656c…` son bucles («de la Tierra de la Tierra …») y para la clase larga empiezan con «de 100, de 100, …». No sirven como referencia; se usó Whisper Small con el modo `referencia` del arnés.

## Archivos tocados

| Ruta | Qué se hizo | ¿Estaba en el PLAN? |
|---|---|---|
| `core/stt/src/nivel.rs` | **Nuevo.** `Nivelador` (D2) y `interpretar_interruptor` | Sí |
| `core/stt/src/lib.rs` | `pub mod nivel;` | Sí |
| `core/stt/src/sherpa.rs` | `OpcionesSherpa` (D6), haz con `max_active_paths`, `modeling_unit` explícito, preparación/segmentación de términos, `nuevo_flujo_con_terminos`, saneamiento de `\0`, pruebas | Sí |
| `core/api/src/transcriptor.rs` | Nivelación solo de `Track::System` con Sherpa (D1), lectura de `DICTAR_STT_NIVELAR`, términos a `crear_reconocedores_sherpa`, 3 pruebas | Sí |
| `core/api/examples/sherpa_live_eval.rs` | **Nuevo.** Arnés `repro`/`referencia`/`explorar`/`generar-bpe`/`puntuar` | Sí |
| `.../bpe.vocab` | **No generado**: dependía de ejecutar (b1) | Sí (condicional) |

No se tocó ningún `Cargo.toml` ni ningún archivo fuera de la lista. No hice `git add` ni commit.

## Qué implementa cada pieza (para el revisor)

- **`Nivelador`** (D2): ventanas de 480 muestras con estado entre llamadas; la ganancia de la
  ventana en curso es una **rampa lineal** desde la ganancia anterior hacia la decidida con la
  ventana previa (salida causal y continua, sin saltos); solo las ventanas con RMS > 0,003
  actualizan; objetivo −20 dBFS; rango −6/+20 dB; subida ≤ 3 dB/s, bajada ≤ 20 dB/s; limitador
  suave (lineal hasta 0,9, `tanh` hacia 0,98, con `min(0.98)` de cinturón). La ganancia no se
  reinicia nunca: `bucle` crea el `Nivelador` una vez, y `reiniciar_por_hueco` solo toca al
  reconocedor y al `EstadoIncremental`.
- **D1**: `nivelador_para(track, ..)` devuelve `None` para `Mic`; `audio_para_motor` copia y
  nivela solo cuando hay nivelador. El WAV ya está escrito antes de ese punto. El nivelador solo
  se crea si el motor es Sherpa (`motor_en_vivo.starts_with("sherpa")`).
- **D6**: tres interruptores, un único sitio de lectura cada uno
  (`OpcionesSherpa::desde_entorno`, `nivelar_desde_entorno`). **Un valor inválido es un error**
  (en `nivelar_desde_entorno` se avisa por `Evento::Error` y se usa el de producción), y pedir
  `PALABRAS_CLAVE=1` con `greedy` falla en vez de dejar las palabras mudas.
- **D3**: `Segmentacion::{Bpe, PiezaMasLarga, MinimoDePiezas}`. Con `Bpe` se configura
  `modeling_unit = "bpe"` + `bpe_vocab`; con las otras, `modeling_unit = "cjkchar"` explícito y
  los términos entregados ya troceados con piezas de `tokens.txt` (prefijo `▁` en la primera
  pieza; si ninguna pieza con prefijo cubre el arranque, `▁` va suelta). Separador entre términos
  hacia el motor: `/` (`TerminosPreparados::para_motor`). **Es una suposición de mi memoria del
  C++ (`regex_replace("/", "\n")`) que no pude confirmar**: ni las cadenas de la DLL ni el crate
  lo documentan; `explorar ... crudo` existe justo para comprobarlo con `/` y con salto de línea.
- **Constantes de producción** (decisión por defecto, **sin medir**): `HAZ_POR_DEFECTO = false`,
  `PALABRAS_CLAVE_POR_DEFECTO = false`, `NIVELAR_SISTEMA_POR_DEFECTO = false`. Es lo que ordena
  D5 cuando no hay números: nada se activa «a ojo». El comportamiento de producción es, por
  tanto, **idéntico al anterior** (greedy, sin nivelar, sin términos) salvo `modeling_unit =
  "cjkchar"` ahora explícito (era el valor por omisión del crate; si el crate lo cambiara, antes
  cambiaría solo).
- **Hallazgo que afecta al plan** (leído, no medido): `tokens.txt` contiene todas las letras,
  los dígitos, `-`, `'`, `&`, etc., sueltos o con `▁`. La premisa del plan de que dígitos y
  guiones quedarían fuera es falsa para este modelo: **todo carácter imprimible común es
  segmentable**, así que `NoSegmentable` rechazará poco (solo caracteres raros). Lo que
  decidirá si el motor «acepta» un término es su propio `Encode hotwords failed`, que solo se ve
  en el registro de la DLL.

## Comandos para reproducir (plan original; lo ejecutado está en «Estado»)

**(Obsoleto: ver «Estado».)** Al escribir esto no pude ejecutar ninguno. `cargo` está instalado y funciona (`rustc` suelto, sí), pero todo
build con dependencias muere con `os error 4551` (arriba). Lo único que sí corrí:

```
rustfmt --edition 2021 core/stt/src/nivel.rs core/stt/src/sherpa.rs core/api/src/transcriptor.rs core/api/examples/sherpa_live_eval.rs
```
(sin errores; formatea; las líneas > 100 columnas que quedan en `transcriptor.rs` ya existían:
cadenas largas que rustfmt no parte.) `rustfmt` solo comprueba sintaxis y formato, **no tipos**:
no hay garantía de que compile.

Para quien pueda compilar, en este orden:

```
# 1. Pruebas y lint
source entorno-msvc.sh
cargo test -p dictar-stt
cargo test -p dictar-api tests_nivelacion
cargo clippy -p dictar-stt -p dictar-api --all-targets -- -D warnings
cargo fmt --all -- --check

# 2. Paso 0 (D3). Sacar los términos de la referencia primero (ver 3). Cada prueba:
cargo build --release -p dictar-api --example sherpa_live_eval
set DICTAR_SHERPA_MODEL_ES=%LOCALAPPDATA%\Programs\dictar_ia\models\sherpa-onnx-streaming-zipformer-es-kroko-2025-08-06
EV=target\release\examples\sherpa_live_eval.exe
WAV=%APPDATA%\dictar_ia\audio\ses_3e9de90c93b549ed9afe0400e5767421\system.wav
$EV explorar $WAV 60 greedy ninguna H.txt                     2> a.log   # (a) referencia
$EV explorar $WAV 60 haz    ninguna H.txt                     2> a2.log  # (a) haz sin términos: A'
$EV generar-bpe %DICTAR_SHERPA_MODEL_ES%\tokens.txt bpe.vocab
$EV explorar $WAV 60 haz bpe    H.txt bpe.vocab               2> b1.log  # (b1)
$EV explorar $WAV 60 haz larga  H.txt                         2> b2a.log # (b2) voraz
$EV explorar $WAV 60 haz minimo H.txt                         2> b2b.log # (b2) mínimo de piezas
$EV explorar $WAV 60 haz crudo  H_crudo_con_barras.txt        2> sep.log # separador
# (c) findstr /c:"Encode hotwords failed" *.log   -> contar; comparar frases con/sin términos.
```

## Qué hay que correr para cerrar la tarea (D4/D5) — comandos exactos

Identificación de sesiones por duración (bytes de `system.wav` / 32000 s, WAV de 16 bit mono):

| Sesión | Bytes system.wav | Duración | id |
|---|---|---|---|
| Comunicaciones | 4 242 754 | 132,6 s | `ses_656c818d3fe541fa81e5801818452d75` |
| Diseño de Sistemas | 5 060 268 | 158,1 s | `ses_3e9de90c93b549ed9afe0400e5767421` |
| Clase larga | 110 742 338 | 3 460,7 s | `ses_4b0ac2924455448abd66ec2f4bc248cf` |

Referencia Whisper Small. **Qué usé: nada todavía.** Mirando la base (`dictar.db`, tabla
`utterance`) **no sirve como referencia**: las frases «definitivas» de `ses_656c…` son bucles
(«de la Tierra de la Tierra …») y las de la clase larga empiezan con
«de 100, de 100, …». Hay que correr Whisper Small con el propio arnés
(`ggml-small-q5_1.bin` está en `%LOCALAPPDATA%\dictar_ia\models`):

```
$EV referencia %APPDATA%\dictar_ia\audio\ses_3e9d…\system.wav ref_diseno.json es
$EV referencia %APPDATA%\dictar_ia\audio\ses_656c…\system.wav ref_comunic.json auto
$EV referencia %APPDATA%\dictar_ia\audio\ses_4b0a…\system.wav ref_larga.json es 600
```
**Aviso:** `docs/plan-correccion-calidad-windows.md` dice que Comunicaciones es **en inglés**
(Internet of Things, Kevin Ashton). Sherpa es español: esa sesión medirá basura para todas las
filas. Conviene que el orquestador decida si se sustituye por otra sesión española o se excluye de
las reglas.

Las 5 filas (un proceso por fila; fijar a una CPU y prioridad baja, como los replays del 08/10):

```powershell
$env:DICTAR_SHERPA_MODEL_ES = "$env:LOCALAPPDATA\Programs\dictar_ia\models\sherpa-onnx-streaming-zipformer-es-kroko-2025-08-06"
function Fila($etiqueta,$decod,$nivelar,$claves,$sys,$mic,$dir,$seg,$glos) {
  $env:DICTAR_SHERPA_DECODIFICACION=$decod; $env:DICTAR_STT_NIVELAR=$nivelar
  $env:DICTAR_SHERPA_PALABRAS_CLAVE=$claves; $env:DICTAR_EVAL_GLOSARIO=$glos
  $p = Start-Process -PassThru -NoNewWindow target\release\examples\sherpa_live_eval.exe `
       -ArgumentList "repro",$sys,$mic,$dir,$seg -RedirectStandardError "$dir.stderr.log"
  $p.ProcessorAffinity = 0x4; $p.PriorityClass = 'BelowNormal'; $p.WaitForExit()
  "$etiqueta exit=$($p.ExitCode)"
}
# A=greedy/0/0  A'=haz/0/0  B=greedy/1/0  C=haz/0/1  D=haz/1/1
```
El glosario de cada fila C/D es el prompt de 800 caracteres (H + relleno de términos válidos
hasta 800 caracteres con el formato de `prompt_glosario`, `", "`) para que D4 se mida «a tamaño
real»; para D5 (aciertos) se usa también ese mismo archivo, así que **los E nunca van dentro**
(el relleno sale de términos que no estén en H ∪ E). Tablas: `$EV puntuar ref.json H.txt E.txt
A=dirA "A'=dirA2" B=dirB C=dirC D=dirD`. Aplicar las reglas de D5 al pie de la letra sobre la
suma de las sesiones y reportar también por sesión.

### Tablas D4 / D5 y decisión

**Ver «D4 / D5» en «Estado».** Texto del primer intento: vacías. Decisión aplicada por la regla «si no se cumplen, quedan apagadas»:
`HAZ_POR_DEFECTO = false`, `PALABRAS_CLAVE_POR_DEFECTO = false`, `NIVELAR_SISTEMA_POR_DEFECTO =
false`. Una decisión de «no activar por falta de evidencia», no de «medido y no mejora».

## Pruebas escritas (no ejecutadas)

`nivel.rs` (8): subida ≤ 3 dB/s con tono a −40 dBFS; silencio no sube; salida ≤ 0,98 y sin
saltos; limitador lineal/monótono; rango −6/+20; **bloques de 1024 = un solo bloque (igualdad
exacta de `Vec<f32>`)**; estado sobrevive a audio no contiguo; interruptor inválido es error.
`sherpa.rs` (8): `\0` y separadores fuera; mayúsculas; cortos/repetidos contados; carácter
ausente → rechazado; piezas existen en `tokens.txt` y reconstruyen la palabra; mínimo de piezas
vs voraz; prefijo suelto; interruptores de entorno. `transcriptor.rs` (3): mic nunca recibe
nivelador; sin nivelador no se copia; con nivelador el motor recibe copia más fuerte y el origen
queda intacto.

Regla de la réplica, revisada una a una: todas llaman a la función de producción
(`Nivelador::procesar`, `limitar`, `preparar_terminos`, `segmentar_palabra`,
`normalizar_termino`, `OpcionesSherpa::interpretar`, `nivelador_para`, `audio_para_motor`). Dos
puntos débiles que declaro:

- `el_estado_sobrevive_a_audio_no_contiguo_...` **no prueba** que `bucle` no recree el
  nivelador tras un hueco; eso es estructural (el `Nivelador` se crea una vez, fuera del `while`)
  y solo se ve leyendo `bucle`. AC «la ganancia se conserva en un reinicio por hueco» queda
  cubierto **solo por esa lectura**.
- `OpcionesSherpa::interpretar` ejercita la lógica pura; `desde_entorno` (la lectura real de
  `std::env`) no tiene prueba.

## Invariantes del producto

| Invariante | Cómo se respeta acá |
|---|---|
| Se escribe a disco antes de procesar | No se toca la escritura de WAV. La nivelación trabaja sobre una copia que se hace en `procesar_bloque_incremental`, después de la captura |
| Dos pistas, nunca una mezcla | Las dos pistas siguen con flujo y estado propios; el nivelador es solo de sistema |
| 16 kHz mono f32, remuestreo en origen | No se remuestrea nada nuevo; el `Nivelador` asume 16 kHz (30 ms = 480 muestras) |
| Ninguna firma pública cambia según plataforma | `dictar_stt::nivel` es multiplataforma. `sherpa` ya era `cfg(windows)` y lo sigue siendo; `SherpaModelo::cargar` cambió de firma (recibe `&OpcionesSherpa`), pero solo lo llama `modelo_es_compartido` (comprobado con grep en todo el repo). El ejemplo tiene `main` de reemplazo fuera de Windows |

## Decisiones que se apartan del PLAN

1. **Constantes de producción en `false`** para las tres funciones: el plan lo prevé como
   resultado de la medición, aquí es por ausencia de medición (arriba).
2. **`modeling_unit = "cjkchar"` explícito** también en el camino sin palabras clave (el plan
   solo lo exige para (b2)). Es cambiar un valor implícito por el mismo valor explícito; si el
   crate usa otro por omisión, el comportamiento cambia: verificar con (a).
3. **Haz solo si hay palabras clave** (no hay modo «haz sin términos» en producción): el plan
   decía «haz siempre»; sin términos no aporta nada y cuesta CPU. La fila A′ sigue existiendo
   para medir, vía `DICTAR_SHERPA_DECODIFICACION=haz`.
4. **Normalización de mayúsculas = todo a minúsculas** (D3 d). Es provisional: el plan pide
   «el uso que emite el modelo» y eso se decide mirando la salida de (a). Siglas y nombres
   propios (el modelo tiene `▁La`, `A`, `M`…) podrían necesitar su forma original.
5. `puntuar` compara con coincidencia **case-insensitive pero sensible a tildes** y aplica el
   solape de ±5 s sobre los rangos de los fragmentos de Whisper (que pueden ser largos); la
   definición exacta está en `aciertos`/`inserciones`/`duplicados` del ejemplo. «Duplicado» =
   frase del micro con ≥ 3 palabras, ≥ 60 % de ellas presentes en una frase del sistema que
   solape a ±3 s.

## Lo que NO pude verificar

- **Que compile.** Nada. Hay riesgo real de errores de tipos/borrows, sobre todo en el ejemplo
  (≈550 líneas escritas sin compilador) y en las firmas de `sherpa-onnx` (leí el crate
  `1.13.8`: `max_active_paths: i32`, `hotwords_score: f32`, `modeling_unit`/`bpe_vocab:
  Option<String>`, `create_stream_with_hotwords(&str)`).
- **Que las pruebas pasen.** Fueron razonadas a mano contra la implementación (p. ej. la
  igualdad exacta de bloques; que `0.9 + (0.98-0.9)` en `f32` da exactamente `0.98`).
- **Clippy** `-D warnings`: usé `is_none_or` y `#[allow(clippy::too_many_arguments)]` por
  anticipado; otros lints pueden saltar.
- **Todo D3**: (a) haz vs greedy, (b) si `bpe.vocab` o la segmentación explícita hacen que el
  motor acepte términos, (c) si hay efecto real, el separador `/`, y si `hotwords_score = 1,5`
  inserta o corrige. Sin ello, **no sé si las palabras clave funcionan en absoluto** con este
  modelo (un modelo sin `bpe.vocab` y con piezas sin puntuación puede rechazarlo todo).
- **Todo D4/D5**: carga, cola, edad, aciertos, inserciones, duplicados, y si nivelar mejora.
  El objetivo de −20 dBFS sigue siendo una hipótesis. Una observación de diseño a vigilar al
  medir: la ganancia sigue al **mayor** nivel reciente (baja 20 dB/s, sube 3 dB/s), así que el
  habla típica puede quedar por debajo de −20 dBFS.
- **Sesión real de 2 min** (AC 5) y el registro de «términos cargados / rechazados» en vivo.
- Que `DICTAR_SHERPA_MODEL_ES` haga falta: `modelo_es()` busca en
  `%LOCALAPPDATA%\dictar_ia\models\<modelo>` y el modelo instalado está en
  `%LOCALAPPDATA%\Programs\dictar_ia\models\…`; no lo toqué (es otro archivo de la lista de
  AV-2/empaquetado, y la variable lo salva).

## Deuda que dejo

- **Desbloquear la compilación** (Smart App Control) y ejecutar todo lo de arriba: es la tarea.
- Decidir qué hacer con Comunicaciones (audio en inglés) en D5.
- Si (b1) funciona, generar y conservar `bpe.vocab` aquí como evidencia, y decidir en otra tarea
  cómo se empaqueta con el modelo.
- Una prueba de `desde_entorno` (requiere serializar el acceso a variables de entorno entre
  pruebas) y una prueba de integración de `bucle` que fije «la ganancia sobrevive al hueco».
- `PUNTUACION_PALABRAS_CLAVE`, `RUTAS_ACTIVAS` y la mayúscula de los términos son parámetros sin
  ajustar; el plan manda ajustar la puntuación midiendo inserciones.
- Si las palabras clave resultan inútiles con este modelo, borrar `Segmentacion`, `Bpe` y el
  resto de la maquinaria en vez de dejarla apagada.
