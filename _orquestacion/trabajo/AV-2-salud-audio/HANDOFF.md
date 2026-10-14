# HANDOFF — AV-2 «Que la app me avise durante la grabación si el audio está mal»

Escrito por quien implementó, al entregar.

> **Para los revisores:** esto es una **declaración**, no evidencia. Todo lo que dice acá está
> por verificarse. Ver `protocolo.md`, regla 1.

## Estado

**Parcial.** Código, pruebas Rust (lib), pruebas Flutter y D7 hechos. Quedan sin cumplir:

- **D4 en Release** (criterio 7): Smart App Control bloqueó con `os error 4551` el build script de
  `ring` en `target/release`. No lo rodeé. Solo hay medición en Debug (abajo), que no sirve para
  decidir contra los 20 ms.
- **Criterio 6 (pruebas manuales)**: ninguna se pudo hacer sin una persona; van como guion, NO
  VERIFICABLE.
- **Criterio 3**: clippy de `dictar-api` sigue rojo por 3 errores en `transcriptor.rs`, archivo
  que no es de este plan (ver abajo).

## Archivos tocados

| Ruta | Qué se hizo | ¿Estaba en el PLAN? |
|---|---|---|
| `core/api/src/salud_audio.rs` | Nuevo. `SaludAudio` (`nueva`, `trama`, `tick`), anillos por `timestamp_ms`, 6 avisos, bloqueo de 15 s, 27 pruebas | Sí |
| `core/api/src/lib.rs` | `pub mod salud_audio;` (pública porque el ejemplo de D7 la usa) | Sí |
| `core/api/src/eventos.rs` | `Evento::Salud { pista, codigo, activo, valor, mensaje }` | Sí |
| `core/api/src/grabacion.rs` | `bucle_escritura` recibe un `SaludAudio`; `trama` tras escribir a disco y registrar métricas, `tick` en cada vuelta (incluido el timeout); el vaciado final no alimenta. `#[allow(clippy::too_many_arguments)]` (precedente en `transcriptor.rs`). Además cambié `io::Error::new(Other, e)` por `io::Error::other` en `volcar_metricas_captura` (código preexistente que clippy rechazaba) | Sí (el cambio de `io::Error::other` es colateral, dentro del mismo archivo) |
| `core/api/src/puente.rs` | Mapeo D5 + comentario del campo `tipo` + prueba | Sí |
| `core/api/examples/salud_audio_eval.rs` | Nuevo, D7 | Sí |
| `app/lib/modelos/dominio.dart` | `AvisoAudio` | Sí |
| `app/lib/datos/repositorio.dart` | `avisosAudio` en la interfaz y la demo (nunca emite) | Sí |
| `app/lib/datos/repositorio_rust.dart` | `case 'salud'` y su `StreamController` | Sí |
| `app/lib/pantallas/grabacion.dart` | Banda con cerrar (amplia), punto + tooltip (compacta), limpieza al empezar y al detener | Sí |
| `app/test/grabacion_test.dart` | D8 y dos pruebas nuevas con subclase de `RepositorioDemo` | Sí |

No toqué `frb_generated.rs`, `app/lib/src/rust/**`, ningún `Cargo.toml` ni `core/audio-capture`.

## Comandos para reproducir

En Git Bash con `source entorno-msvc.sh` (MSVC delante del `link` de coreutils).

```
cargo test -p dictar-api                                  # lib: 68 passed; ver ICE de doctest
cargo clippy -p dictar-api --all-targets --no-deps -- -D warnings
rustfmt --edition 2021 --check core/api/src/{salud_audio,grabacion,eventos,puente}.rs core/api/examples/salud_audio_eval.rs
cargo build -p dictar-api --example salud_audio_eval && target/debug/examples/salud_audio_eval.exe
cd app && flutter analyze && flutter test
```

**Resultados reales:**

- `cargo test -p dictar-api`: unit tests `68 passed; 0 failed` (27 de `salud_audio`, 1 del mapeo
  `puente::tests::un_aviso_de_salud_cruza_con_cada_dato_en_su_campo_reutilizado`). **Código de
  salida 101** por el paso de doctests: `rustc` entra en pánico («the compiler unexpectedly
  panicked», `decode error: Expected header tag ... found [0,0,0,0]`, `T-rustdoc`, rustc 1.98.1).
  Parece un artefacto corrupto del `target/` compartido entre tres agentes; el crate no tiene
  doctests (`running 0 tests`). No lo investigué. Los tests de `salud_audio` también los corrí
  solos en una ejecución anterior: `27 passed`, rc=0.
- `cargo clippy -p dictar-api --all-targets -- -D warnings` (sin `--no-deps`): **falla en
  `dictar-audio`** (`core/audio-capture/src/lib.rs:304`, `unnecessary_cast`), crate que no puedo
  tocar. Con `--no-deps`: rc=101 con **3 errores, todos en `core/api/src/transcriptor.rs`**
  (líneas 58 `too_many_arguments`, 848 `arc_with_non_send_sync`, 966 `too_many_arguments`),
  preexistentes y fuera del plan. En mis archivos: 0. Dos errores míos/de mi archivo
  (`chunks_exact_to_as_chunks` en `salud_audio.rs` y `io_other_error` en `grabacion.rs`) los
  corregí. Nota: `as_chunks` no sirve porque el `rust-version` es 1.75; usé `chunks` con
  comprobación de longitud.
- `rustfmt --check` de los archivos Rust míos: rc=0.
- `flutter test`: `All tests passed!` (43 pruebas), incluida la prueba de D8 que estaba rota.
- `flutter analyze`: 1 info preexistente (`ajustes.dart:184`, `deprecated_member_use`), ningún
  error nuevo.

## Criterios de aceptación

| Criterio | Prueba | Estado |
|---|---|---|
| 1. Pruebas por fila de D3 | `salud_audio::tests` (ver lista abajo) | Cumplido |
| 2. Mapeo D5 | `puente::tests::un_aviso_de_salud_cruza_con_cada_dato_en_su_campo_reutilizado` | Cumplido |
| 3. Clippy limpio | — | **No**: 3 errores ajenos en `transcriptor.rs` y 1 en `dictar-audio` |
| 4. Flutter | analyze sin errores nuevos; test todo en verde | Cumplido |
| 5. D7 | Salida abajo (en Debug) | Cumplido |
| 6. Manual en Windows | Guion abajo | NO VERIFICABLE |
| 7. Medición D4 | Solo Debug | **No cumplido** (Release bloqueado) |

Cobertura del criterio 1 (nombres de prueba): `sin_senal_inicio_salta_a_los_treinta_segundos...`,
`sin_senal_inicio_se_va_con_la_primera_ventana_activa`, `sin_senal_pausa_salta_a_los_tres_minutos_y_no_antes`
(caso vecino: sin actividad previa no hay pausa, con actividad previa no hay «inicio»),
`el_silencio_digital_del_sistema_no_cuenta_como_senal`, `mic_sin_tramas_necesita_el_tick...`,
`un_microfono_en_silencio_pero_abierto_no_dispara_mic_sin_tramas`,
`un_aviso_desactivado_no_se_reactiva_antes_de_quince_segundos`, `nivel_bajo_*` (4),
`el_ruido_de_sala_a_menos_45_dbfs_en_el_microfono_no_dispara_nada`, `recorte_*` (3),
`el_eco_se_detecta_...` (positivo), `dos_voces_distintas_con_ritmo_silabico_independiente_no_son_eco`
(negativo), `el_eco_necesita_dos_evaluaciones_seguidas...`, `el_eco_se_apaga_*` (2),
`un_eco_sin_suficiente_sistema_activo_no_se_evalua`, `el_eco_se_alinea_por_timestamp_y_no_por_orden_de_llegada`
y tres del anillo. Todas llaman a `SaludAudio::trama/tick` o a `Pista` de producción; los
generadores de señal sintética solo producen entradas. No hice pruebas de mutación (no rompí a
propósito la alineación para ver el rojo): no puedo afirmar que cada prueba falle con su bug.

Pruebas de widget nuevas (`grabacion_test.dart`): la banda aparece, se cierra, no reaparece al
repetirse el aviso, reaparece al apagarse y reactivarse, y se vacía al pulsar Detener (con el
núcleo todavía parando); en compacta, punto con tooltip y sin banda.

## Invariantes del producto

| Invariante | Cómo se respeta |
|---|---|
| Escribir a disco antes de procesar | `salud.trama` va después de `escritor.escribir` en el bucle; el tick no escribe |
| Dos pistas, nunca mezcla | Un anillo por pista; solo se comparan, no se suman |
| 16 kHz mono f32 | Se asume `SAMPLE_RATE`; no se remuestrea nada |
| Ninguna firma pública cambia según plataforma | `salud_audio` y `Evento::Salud` no llevan `#[cfg]`; el puente no cambió de tipos |

## Decisiones que se apartan del PLAN

1. **Anillo de 16 s, no 15.** Con 15 exactos el inicio de la ventana de eco (`ahora − 15 s`) caería
   siempre fuera si la última trama va algo por delante de `ahora`. Comentado en el código.
2. **D8: además de «Área para captura manual», cambié la expectativa de `'Capturar diapositivas'`.**
   Esa cadena ya no existe en ninguna pantalla (la captura automática se quitó), así que
   `findsOneWidget` fallaba también. En escritorio ahora espera «Área para captura manual» y
   «Elegir», y `Capturar diapositivas` pasa a `findsNothing` en las dos ramas ("no se reintroduce
   lo quitado"). Android: `findsNothing` de «Área para captura manual».
3. **Limpieza al detener antes de esperar al núcleo, y síncrona** (`_subAvisos.cancel()` sin
   `await`): con `await` el vaciado tardaba más que la prueba. Además se limpia al empezar.
4. **`AvisoAudio.esGrave`** (sin_senal*, mic_sin_tramas → color de error; el resto, advertencia):
   añadido por mí para el estilo, no estaba en el plan.
5. **Recorte por pista**: «recorte» se activa por separado en mic y sistema; la clave en Dart es
   `pista/codigo`. El texto dice «El micrófono satura…» / «El audio del sistema satura…».
6. `#[allow(clippy::too_many_arguments)]` en `bucle_escritura` (8 parámetros).

## Salida de D7 (Debug, `target/debug/examples/salud_audio_eval.exe`, rc=0)

Identificación por duración (los tres tienen `mic.wav` y `system.wav`; el resto de `ses_*` solo
tiene mic y el ejemplo los salta):

- Comunicaciones (132,6 s): `ses_656c818d3fe541fa81e5801818452d75`
- Diseño de Sistemas (158,1 s): `ses_3e9de90c93b549ed9afe0400e5767421`
- Clase larga (3460,7 s = 57,7 min): `ses_4b0ac2924455448abd66ec2f4bc248cf`

```
== ses_3e9de90c93b549ed9afe0400e5767421 ==
  mic 158.1 s (RMS 0.1866), sistema 158.1 s (RMS 0.1554)
  (ningún aviso)
== ses_4b0ac2924455448abd66ec2f4bc248cf ==
  mic 3460.7 s (RMS 0.1503), sistema 3460.7 s (RMS 0.1007)
      50.0 s  ACTIVA   eco               mic     valor 0.715
    2355.0 s  apaga    eco               mic     valor 0.527
    2370.0 s  ACTIVA   eco               mic     valor 0.738
    3400.0 s  apaga    eco               mic     valor 0.000
    3415.0 s  ACTIVA   eco               mic     valor 0.660
== ses_656c818d3fe541fa81e5801818452d75 ==
  mic 132.6 s (RMS 0.0389), sistema 132.6 s (RMS 0.0518)
      25.0 s  ACTIVA   eco               mic     valor 0.806
```

Lectura frente a lo esperado:

- `eco` en Comunicaciones (a los 25 s) y en la clase larga (a los 50 s): **cumple**.
- Diseño de Sistemas: **no aparece eco** (permitido por el plan; su correlación global es 0,552,
  por debajo de 0,65 sostenido). Documentado.
- Clase larga: el eco es intermitente: se apaga a los 2355 s (dos evaluaciones < 0,55) y vuelve
  a los 2370 s (15 s después, justo el bloqueo); se apaga a los 3400 s con valor 0.000, que es la
  rama «30 s sin datos suficientes» (no he mirado qué pasaba en el audio ahí: puede ser un
  silencio real de la clase), y vuelve a los 3415 s. No es un falso positivo evidente pero no lo
  verifiqué escuchando.
- `nivel_bajo`: **no aparece** en ninguna. `sin_senal_*`: **no aparece** en ninguna.
- No hubo resultados que contradigan lo esperado, así que no toqué umbrales.

## Medición de D4

**Release: no medido** (build script de `ring` bloqueado, `os error 4551`, en `target/release`).
En **Debug** (sin optimizar, no comparable con los 20 ms), costo por `tick` de los ticks que
tardaron > 100 µs (las evaluaciones de 1 s y de eco cada 5 s):

| Sesión | Evaluaciones > 100 µs | Máx | Media |
|---|---|---|---|
| Diseño (158 s) | 158 | 126,9 ms | 29,4 ms |
| Clase larga (3461 s) | 3633 | 2209,9 ms | 80,7 ms |
| Comunicaciones (133 s) | 138 | 558,7 ms | 120,9 ms |

No sé qué sale en Release. El plan dice informar si supera 20 ms: **no puedo decir que no los
supere**; el máximo de 2,2 s en Debug (probablemente un pico por el sistema, no por el algoritmo)
justifica medirlo antes de cerrar. Lo más caro es `extraer` (copia elemento a elemento con
módulo) y `analizar_retorno_acustico` sobre 14,5 s.

## Lo que NO pude verificar

- **D4 en Release**, ver arriba.
- **Criterio 6 completo.** Nada de esto lo hice: requiere altavoces, micrófono y una persona.
  Guion exacto para el PO (cada punto: grabar con `dictar_ia`, ventana amplia visible):
  - **(a)** Nueva sesión con sistema; no reproducir nada. Esperado: banda «No llega audio del
    sistema…» a los ~30 s.
  - **(b)** Altavoces (no auriculares), micrófono abierto, reproducir un vídeo con voz de forma
    continua. Esperado: «El micrófono recoge lo que suena por los altavoces…» antes de 40 s.
  - **(c)** Con voz sonando por auriculares o altavoces, bajar el volumen **de la aplicación
    de origen** en el Mezclador de volumen hasta casi inaudible. Esperado: «El volumen de la
    clase es muy bajo…» pasados unos segundos. Anotar aparte si bajar solo el volumen maestro
    cambia o no lo que captura el loopback.
  - **(d)** 3 min de clase normal con auriculares. Esperado: **ningún** aviso (caso de falsos
    positivos; es el más importante).
  - **(e)** Con un aviso activo, pulsar Detener. Esperado: la banda desaparece al instante.
  Estado de los cinco: **NO VERIFICABLE**.
- Que el evento `salud` llegue de verdad por el puente real a Dart (no hay pantalla ni
  núcleo real en las pruebas; sí está probado el mapeo Rust y el `case` queda sin prueba de
  Dart: la traducción `track == 'mic'` / `definitiva` del `RepositorioRust` no tiene test).
- La disposición compacta con el tooltip en un monitor real.
- Que `timestamp_ms` marque el **inicio** de la trama en todas las plataformas (lo asumí; en D7
  lo generé yo mismo así).
- Que el hilo de escritura aguante el coste del eco sin retrasar la escritura a disco (ligado a
  D4).
- Doctests: el ICE de rustdoc impidió ver rc=0 en `cargo test -p dictar-api` completo.

## Deuda que dejo

- Medir D4 en Release cuando SAC lo permita; si > 20 ms, mover la evaluación de eco a otro hilo
  (decisión de quien coordine) o reducir el coste de `extraer`/copia.
- `transcriptor.rs` (3 clippy) y `audio-capture/src/lib.rs:304` (1 clippy) bloquean el
  criterio 3 para cualquiera.
- T-19: `nivel_bajo` del micrófono.
- Prueba Dart de `RepositorioRust._procesarEvento` para `salud`.
- Pruebas de mutación de `salud_audio` (romper la alineación, el bloqueo de 15 s, etc., y
  comprobar el rojo).
- Los umbrales salen de tres sesiones; el aviso `sin_senal_pausa` y `recorte` no tienen ningún
  dato real que los ejercite en D7.

## D4 con opt-level=3

Medido en el tercer intento (los dos primeros fallaron por un pánico transitorio de `rustc` al leer
un `.rmeta`; ya no se reproduce). Comando, desde Git Bash con `source entorno-msvc.sh`, rc=0, sin
`os error 4551`:

```
cargo run --example salud_audio_eval -p dictar-api --config profile.dev.opt-level=3
```

Es el perfil `dev` con `opt-level=3` (conserva `debug-assertions` y `debuginfo`; no es Release puro,
así que puede ser algo pesimista). El "tick caro" son los ticks de más de 100 µs, que mezclan la
evaluación de 1 s (nivel, recorte) y la de eco cada 5 s. El ejemplo no imprime p95.

| Sesión | Ticks > 100 µs | Media | Máximo |
|---|---|---|---|
| Diseño de Sistemas (158 s) | 157 | 1,04 ms | 3,80 ms |
| Clase larga (3461 s) | 3491 | 5,48 ms | **127,01 ms** |
| Comunicaciones (133 s) | 133 | 2,80 ms | 10,66 ms |

**Cumple en media (1 a 5,5 ms, bajo 20 ms), pero NO cumple en el máximo**: la clase larga tuvo un
tick de 127 ms. No sé si es un pico del algoritmo o del sistema (otros agentes comparten la máquina
y un proceso de fondo puede haber quitado la CPU); el ejemplo no dice cuántos ticks superan 20 ms.
Según D4 hay que informar y no cambiar de hilo por mi cuenta. Esa evaluación corre en el hilo de
escritura: un tick de 127 ms retrasa 127 ms la lectura del canal de audio, que es holgado frente a
su capacidad, pero no lo comprobé. Para decidir haría falta que el ejemplo imprima p95/p99 y el
número de ticks > 20 ms (no toqué el código). La salida de avisos de esta ejecución es idéntica a la
de Debug (mismos instantes de eco).

## Ronda 2

Correcciones de `REVIEW-codigo.md`. Sin commit.

- **H-1.** `Pista::escribir` (`salud_audio.rs`) ya no lee `timestamp_ms` como inicio: usa
  `ini = max(fin_de_la_pista, ts*16 - pcm.len())`, documentado en la función. Vale para WASAPI
  (final de bloque) y PipeWire (inicio) sin `cfg`. Consecuencias: se eliminó la rama que descartaba
  tramas «demasiado antiguas» (con el máximo ya no puede ocurrir) y una trama atrasada se pega a
  continuación en vez de pisar. Las pruebas del anillo y el helper `trama` de las pruebas de
  comportamiento pasan a usar timestamp de final. Prueba nueva:
  `el_relleno_de_wasapi_tras_un_hueco_largo_no_deja_ciega_la_pista` (paquete de 45 s de ceros más
  audio con ts de su final, y la trama siguiente es visible). Sustituí
  `una_trama_demasiado_antigua...` por `una_trama_con_timestamp_atrasado_se_pega_a_continuacion_sin_pisar`.
- **H-2.** `el_eco_se_alinea_por_timestamp_y_no_por_orden_de_llegada` rehecha: el sistema suena 4 s,
  calla 7 s sin entregar nada (sin relleno) y vuelve; comentario corregido. **Mutación hecha**: con
  `let ini = self.fin;` fallan 3 pruebas, entre ellas esa (antes pasaba). Restaurado el código.
- **M-6.** `_detener` vuelve a suscribirse a los avisos y relanza si `detenerGrabacion` lanza.
- **M-4.** El texto ahora dice «Hace más de 3 minutos que no suena nada…».
- También ajusté el comentario de `CAPACIDAD_ANILLO` (M-1) y la doc de `trama` (M-3).
- No toqué M-2, M-5, M-7, M-8, M-9 ni N-*; quedan como deuda.

Resultados reales: `CARGO_INCREMENTAL=0 cargo test -p dictar-api --lib salud_audio`: `28 passed; 0
failed`, rc=0. `flutter test`: `All tests passed!` (43). `rustfmt` aplicado a `salud_audio.rs`. No
corrí clippy, D4 ni D7 de nuevo; D7 usaba ts de inicio con WAV ya rellenados, así que el cambio de
semántica no lo afecta, pero no lo reejecuté.
