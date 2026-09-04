# REVIEW-pruebas — HU-01 «Grabar en Windows (WASAPI loopback)»

Verificador de pruebas. Ver método en `.claude/agents/verificador-pruebas.md`.

## 1. Veredicto

**NO VERIFICABLE por B-1** (no hay `cargo`/`rustc` en esta máquina ni en WSL): 9 mutaciones
propuestas y verificadas a mano (aritmética emulada, no ejecución), de las cuales **2 pruebas no
protegen lo que dicen proteger** y **1 tiene un error aritmético propio que la haría fallar con
código perfectamente correcto** el día que haya `cargo`. Las 7 pruebas que pedía el PLAN están
las 7; el implementador entregó 2 más, legítimas.

## 2. Qué pude ejecutar y qué no

```
$ command -v cargo rustc flutter
/c/dev/flutter/bin/flutter

$ cargo --version
/usr/bin/bash: line 1: cargo: command not found   (exit 127)

$ rustc --version
/usr/bin/bash: line 1: rustc: command not found   (exit 127)

$ ls ~/.cargo/bin ; ls /c/Users/naunf/.cargo/bin
No such file or directory (ambas)

$ where.exe cargo ; where.exe rustc
INFORMACIÓN: no se pudo encontrar ningún archivo para los patrones dados. (ambas)

$ wsl -l -v
docker-desktop   Stopped   2
Ubuntu-26.04     Stopped   2

$ wsl -d Ubuntu-26.04 -- bash -lc "command -v cargo rustc; cargo --version; rustc --version"
bash: cargo: command not found
bash: rustc: command not found
```

B-1 confirmado de primera mano en Windows nativo y en WSL Ubuntu-26.04 (la única distro con
shell). No pude correr `cargo test -p dictar-audio` ni ningún otro comando de Cargo. Todo lo que
sigue es lectura del código y **verificación aritmética manual/emulada** (con Python, donde la
lógica es aritmética entera o de punto flotante pura) de qué haría cada mutación — no es
ejecución de la prueba real, y no sustituye a `cargo test`.

No toqué ningún archivo del árbol: no apliqué ninguna mutación de verdad (no hay compilador con
el que confirmar rojo/verde), así que no hizo falta revertir nada ni respaldar
`core/audio-capture/src/sincronia.rs` en el scratchpad. `git status --short` es idéntico al
principio y al final (ver sección 7).

## 3. Tabla de mutaciones

Las 9 pruebas viven en `core/audio-capture/src/sincronia.rs`. Todas compilan sin `#[cfg]` de
plataforma y correrían tanto en el job Linux (`nucleo`) como en el nuevo paso `tests de
dictar-audio` del job `nucleo-windows` (`.github/workflows/ci.yml:116-119`).

| Prueba | Archivo:línea mutada | Mutación | Resultado |
|---|---|---|---|
| `las_muestras_i16_del_dispositivo_se_normalizan_a_f32` | `sincronia.rs:59` — `(i16::MAX as f32 + 1.0)` a `i16::MAX as f32` | Divide por 32767 en vez de 32768 | no ejecutado (B-1); verificado a mano: rojo (protege). `f32s[0]` pasaría de 32767/32768≈0.999969 a 1.0 (diff 3.05e-5 mayor que la tolerancia 1e-6); `f32s[1]` pasaría de −1.0 exacto a ≈−1.00003, falla el `assert_eq!` exacto |
| `el_f32_del_dispositivo_pasa_sin_tocarse` | `sincronia.rs:55` — `from_le_bytes` a `from_be_bytes` | Bug de endianness en la rama F32 | no ejecutado (B-1); verificado a mano: rojo (protege). Los bytes LE de 0.25/−0.5 reinterpretados como BE dan valores completamente distintos; `assert_eq!` falla |
| `el_i32_del_dispositivo_se_normaliza_igual_que_el_i16` | `sincronia.rs:63` — divisor `(i32::MAX as f32 + 1.0)` sustituido por el divisor de i16 (32768.0) | Copiar el divisor equivocado entre ramas del `match` | no ejecutado (B-1); verificado a mano: rojo (protege). 2147483648.0 vs 32768.0 da una diferencia de órdenes de magnitud, el `assert!` de tolerancia 1e-6 falla con margen enorme |
| `un_silencio_en_loopback_no_adelanta_lo_que_viene_despues` | `sincronia.rs:76-78` — `muestras_de_relleno` devuelve `0` siempre | Mutación literal del PLAN | no ejecutado (B-1); verificado a mano: rojo (protege). `segundo.pcm.len()` pasaría de 32000 a 1600, falla el primer `assert_eq!` |
| `el_relleno_no_se_calcula_a_partir_de_las_muestras_recibidas` | `sincronia.rs:76-79` — invertir los operandos: `muestras_ya_emitidas.saturating_sub(esperadas)` | Intercambiar orden de la resta | no ejecutado (B-1); verificado a mano: rojo (protege) para esta mutación puntual, pero ver hallazgo 4.2: no protege lo que dice proteger |
| `dos_horas_de_muestras_no_desbordan_el_contador` | `sincronia.rs:307-317` (la propia prueba, no el código) | Ninguna hace falta: la prueba ya está mal | verificado a mano: la prueba falla con código correcto (ver 4.1, hallazgo crítico) |
| `sin_dispositivo_de_salida_se_graba_solo_el_microfono` | `sincronia.rs:94-96` — `if !sistema_abierto { return Err(...) }` incondicional | Mutación literal del PLAN | no ejecutado (B-1); verificado a mano: rojo (protege). `.unwrap()` entraría en pánico |
| `sin_ninguna_pista_disponible_es_un_error_explicito` | `sincronia.rs:97-99` — quitar el `if pistas.is_empty() { return Err(...) }` | Mutación literal del PLAN | no ejecutado (B-1); verificado a mano: rojo (protege). `.unwrap_err()` entraría en pánico |
| `fuera_de_windows_y_linux_sigue_devolviendo_no_soportada` | `sincronia.rs:120` — `matches!(p, Plataforma::Windows \| Plataforma::Linux)` a `true` | Quitar la exclusión de `Plataforma::Otra` | no ejecutado (B-1); verificado a mano: rojo (protege) para `tiene_backend` en sí, pero ver hallazgo 4.4: no protege el código real de `lib.rs` |

## 4. Pruebas que no protegen (o protegen menos de lo que declaran)

### 4.1 CRÍTICO — dos_horas_de_muestras_no_desbordan_el_contador tiene un error aritmético propio

El HANDOFF afirma: «Reconstruí la aritmética de cada una al escribirlas (en particular
`un_silencio_en_loopback_no_adelanta_lo_que_viene_despues` y
`dos_horas_de_muestras_no_desbordan_el_contador`, que son las de cálculo más delicado) y no
encontré ningún error en ese repaso». Repetí ese cálculo a mano (y con aritmética de precisión
arbitraria para no arrastrar error de redondeo yo mismo) y sí hay un error:

```rust
let muestras_previas: u64 = u32::MAX as u64 + 1_000_000;              // 4 295 967 295
let timestamp_correspondiente =
    ((muestras_previas + 32_000) as i64 * 1000) / SAMPLE_RATE as i64;  // división entera
let relleno = muestras_de_relleno(timestamp_correspondiente, muestras_previas);
assert_eq!(relleno, 32_000);
```

`SAMPLE_RATE = 16_000`, y `1000 / 16_000 = 1/16`, así que
`timestamp_correspondiente = floor((muestras_previas + 32_000) / 16)`. El problema:
`u32::MAX = 2^32 − 1` es congruente con 15 módulo 16, y `1_000_000` y `32_000` son ambos
múltiplos de 16, así que `(muestras_previas + 32_000) mod 16 = 15` — se pierde ese resto en la
división entera.

Cálculo exacto:

- `muestras_previas + 32_000 = 4 295 999 295`
- `timestamp_correspondiente = 4 295 999 295 div 16 = 268 499 955` (resto 15, perdido)
- Al reconvertir dentro de `muestras_de_relleno`: `esperadas = 268 499 955 x 16 = 4 295 999 280`
- `relleno = esperadas − muestras_previas = 4 295 999 280 − 4 295 967 295 = 31 985`

No 32 000. El `assert_eq!(relleno, 32_000)` fallaría con `muestras_de_relleno` perfectamente
correcta, porque el propio fixture de la prueba pierde 15 muestras al hacer el redondeo
ida-y-vuelta ms→muestras→ms sobre un valor (`u32::MAX`) que no es múltiplo de 16. Esto es más
grave que «no protege»: es un falso rojo que bloqueará el CI el primer día que exista `cargo`,
sin que exista ningún defecto real en el código de producción. Contradice directamente la
afirmación del HANDOFF de que la aritmética fue revisada a mano sin encontrar error.

Corrección sugerida (no la aplico, no es mi rol): usar un valor base múltiplo de 16, por ejemplo
`(u32::MAX as u64 / 16 + 1) * 16 + 1_000_000`, o construir `timestamp_correspondiente` de forma
que el redondeo no pierda resto, o simplemente calcular el `assert_eq!` con la misma fórmula que
usa el código en vez de con un literal de mano.

### 4.2 Importante — el_relleno_no_se_calcula_a_partir_de_las_muestras_recibidas no puede blindar, por construcción, la decisión que dice blindar

Pregunta del encargo: «¿Distinguiría de verdad entre un relleno derivado del reloj y uno derivado
del conteo de muestras, o pasaría con ambos?» — Respuesta: pasaría con ambos, porque no puede
hacer otra cosa.

`muestras_de_relleno(timestamp_ms: i64, muestras_ya_emitidas: u64) -> u64` es una función pura:
recibe `timestamp_ms` ya calculado y no sabe ni puede saber si ese número vino de
`Instant::now()` (la decisión correcta, documentada en el PLAN) o de
`(muestras_ya_emitidas * 1000) / SAMPLE_RATE` (el error de copiar `pipewire_src`, que el PLAN
describe explícitamente como el fallo a evitar). Esa decisión —qué se le pasa como
`timestamp_ms`— se toma enteramente en `wasapi_src.rs` (el bucle real de captura, en la línea que
llama a `PistaWasapi::procesar_paquete`), que no tiene ninguna prueba y no puede tenerla sin
hardware/compilador (confirmado: `wasapi_src.rs` no tiene `#[cfg(test)]` en absoluto).

Lo que la prueba verifica de verdad es que `muestras_de_relleno` resta correctamente
(`esperadas − emitidas`, con saturación) para dos pares de números concretos — una comprobación
válida, pero redundante con lo que ya cubren `un_silencio_en_loopback_no_adelanta_lo_que_viene_
despues` y `dos_horas_de_muestras_no_desbordan_el_contador`. El comentario de la prueba («Simula
el error que cometería copiar el mecanismo de pipewire_src») describe una intención correcta que
el cuerpo de la prueba no puede cumplir: no hay ninguna línea en `sincronia.rs` que la mutación
«derivar el timestamp del conteo de muestras» pueda romper, porque esa lógica no vive aquí. El
nombre de la prueba promete blindar la decisión de diseño más importante del PLAN (sección
«Decisión de diseño: el reloj y el silencio»); en realidad la deja exactamente igual de
descubierta que antes de escribirla.

### 4.3 Importante — un_silencio_en_loopback_no_adelanta_lo_que_viene_despues verifica la longitud del hueco, no que el silencio vaya antes del audio real

Pregunta del encargo: «¿de verdad ejercita un hueco temporal, o solo comprueba que la función
devuelve algo?» — Respuesta intermedia: sí ejercita el hueco temporal en longitud (la mutación
«relleno = 0 siempre», la del PLAN, sí la pondría en rojo, ver §3), pero no verifica el orden,
que es la otra mitad de lo que su propio nombre promete («no adelanta lo que viene después»).

El fixture usa como «audio real» `vec![0u8; 1600 * 4]` — bytes todos en cero, que interpretados
como `f32` dan muestras de valor 0.0, exactamente el mismo valor que el silencio de relleno. Los
dos asserts son:

```rust
assert_eq!(segundo.pcm.len(), 30_400 + 1_600);
assert!(segundo.pcm[..30_400].iter().all(|&m| m == 0.0), "relleno de silencio");
```

Una mutación que intercambie el orden de construcción del `pcm` en `procesar_paquete`
(`sincronia.rs:181-182`) — por ejemplo, anteponer las muestras reales y añadir el silencio
después en vez de antes, algo tan plausible como un `extend`/`prepend` invertido en un refactor —
produciría exactamente el mismo `pcm.len() == 32_000` y el mismo contenido (32 000 ceros, porque
tanto el relleno como el «audio real» de este fixture son cero). Ningún assert de esta prueba lo
detectaría. Y esa mutación es justo el escenario que el nombre de la prueba dice cubrir: si el
silencio queda después en vez de antes, el audio real de este paquete «se adelanta» en el WAV
exactamente igual que si no se hubiera insertado el hueco en absoluto.

Corrección sugerida: usar un valor distinto de cero para las muestras «reales» (por ejemplo 0.5) y
comprobar que las últimas 1600 muestras de `segundo.pcm` son exactamente ese valor — así el
assert de longitud y el de posición dejan de coincidir accidentalmente.

### 4.4 Importante — fuera_de_windows_y_linux_sigue_devolviendo_no_soportada prueba una réplica paralela, no el código real de lib.rs

Confirmado leyendo `core/audio-capture/src/lib.rs:166-200`: `iniciar()` y `dispositivos()`
enrutan con tres bloques `#[cfg(target_os = "linux")]` / `#[cfg(target_os = "windows")]` /
`#[cfg(not(any(target_os = "linux", target_os = "windows")))]`, y ninguno de los tres llama a
`sincronia::tiene_backend` ni a `sincronia::Plataforma`. Son dos piezas de código
estructuralmente desconectadas, tal como el propio doc-comment de `sincronia.rs:103-110`
reconoce con honestidad: «esto solo prueba que la lista de plataformas soportadas es la que se
pretende. El enrutado real, con #[cfg], sigue en lib.rs».

Consecuencia: si alguien rompe la rama real de `lib.rs` (por ejemplo, borra el bloque
`#[cfg(not(any(...)))]` o cambia mal su condición, dejando sin gestionar una tercera plataforma),
esta prueba sigue en verde, porque nunca toca esa función. Y esto no es solo un defecto de esta
prueba: con el diseño actual de CI (cada job compila para un único `target_os`), ninguna prueba
automatizada, en ningún job, puede ejercitar hoy esa rama real de `lib.rs` — es estructuralmente
intocable sin compilar para una tercera plataforma, cosa que ningún job hace. `tiene_backend` es
el mejor sustituto posible dado ese límite, y está declarado como tal, pero sigue siendo cierto
que el Invariante 4 de HU-01 («ninguna firma pública cambia según la plataforma») no tiene, en la
práctica, ninguna prueba que cubra su implementación real, solo su intención.

### 4.5 Menor — ninguna de las 9 pruebas ejercita el underflow que saturating_sub existe para evitar

En las 9 pruebas, cada llamada a `muestras_de_relleno` tiene `esperadas >= muestras_ya_emitidas`
(nunca al revés). Quitar el `.saturating_sub` de `sincronia.rs:78` (usar resta normal) no lo
detectaría ninguna de las 9: en `debug` provocaría un pánico solo si algún día se ejecuta con
`esperadas < emitidas`, cosa que ningún test hace; en `release` envolvería silenciosamente. Es una
mutación pequeña y del tipo exacto que el propio método pide comprobar («quitar un
`saturating_`»), y hoy no la detecta ninguna prueba.

## 5. Criterios de aceptación de docs/06 sin prueba que los cubra

De `docs/06-historias-de-usuario.md#hu-01`:

| AC | Cubierto por prueba | Estado |
|---|---|---|
| 1. iniciar() en Windows devuelve Receiver real | Ninguna | No comprobable sin hardware/compilador — correctamente declarado así en PLAN y HANDOFF |
| 2. Dos pistas, loopback con AUDCLNT_STREAMFLAGS_LOOPBACK sobre salida por defecto | `sin_dispositivo_de_salida_se_graba_solo_el_microfono`, `sin_ninguna_pista_disponible_es_un_error_explicito` cubren solo la política (qué hacer con lo que se abrió) | La apertura COM real no comprobable sin hardware — declarado |
| 3. 16 kHz mono f32, timestamp_ms de reloj monótono común | Conversión y relleno cubiertos en sincronia.rs; el hecho de que el reloj sea realmente monótono y común a las dos pistas depende de wasapi_src.rs (Instant::now() en el bucle real) | La parte de wasapi_src no comprobable sin hardware — declarado. Adicional: ni siquiera a nivel de diseño puro hay una prueba que distinga «reloj real» de «reloj derivado de muestras» (ver 4.2) — eso no estaba declarado como hueco, y debería estarlo |
| 4. dispositivos() enumera los dispositivos reales | Ninguna | No comprobable sin hardware, y además la implementación es declarada parcial por el propio implementador (solo el par por defecto, no EnumAudioEndpoints completo) — es deuda explícita, no solo falta de prueba |
| 5. Al soltar CaptureSession la captura se detiene | Ninguna | No comprobable sin hardware — declarado |
| 6. Deriva imperceptible en 30 min | Ninguna | No comprobable sin hardware ni tiempo real — declarado, y es inherentemente así (no hay forma de medir deriva real con pruebas unitarias puras) |
| 7. Al menos un test sin tarjeta de sonido | Las 9 de sincronia.rs | Cumplido — con holgura en cantidad, pero ver 4.1 (una de las 9 está rota) antes de darlo por cumplido con calidad |
| Invariante 4 — ninguna firma pública cambia según plataforma | fuera_de_windows_y_linux_sigue_devolviendo_no_soportada (declarado como cobertura en HANDOFF) | Cubre una réplica (tiene_backend), no el código real de lib.rs (ver 4.4). No estaba declarado este matiz en HANDOFF ni PLAN |

Distinción pedida: los AC 1, 2 (apertura COM), 4 (enumeración), 5 y 6 son genuinamente no
comprobables sin hardware — el PLAN lo dice y estoy de acuerdo, no hay forma razonable de
simularlos sin envolver COM entero (objeción 3 del contradictor, aceptada con buen criterio). El
AC 3 (parte del reloj) y el Invariante 4 son distintos: sí se intentó cubrirlos con pruebas puras
(`el_relleno_no_se_calcula_a_partir_de_las_muestras_recibidas` y
`fuera_de_windows_y_linux_sigue_devolviendo_no_soportada`), pero el intento no logra su objetivo
por construcción — eso es «se intentó y no alcanzó», no «nadie lo intentó», y ninguno de los dos
documentos (PLAN, HANDOFF) lo señala.

## 6. Premisas que cuestiono

1. «Los cálculos están verificados a mano... no encontré ningún error en ese repaso» (HANDOFF).
   Falso para `dos_horas_de_muestras_no_desbordan_el_contador`: hay un error de 15 muestras por
   pérdida de resto en una división entera. Conclusión: la revisión manual del implementador no
   sustituye una ejecución real, ni siquiera cuando se declara hecha con cuidado — es exactamente
   la razón de ser de este rol.

2. «Cumplido con holgura: son 7, no 1, y ninguna toca COM» (HANDOFF, sobre AC 7). Contar pruebas
   no es lo mismo que tener pruebas correctas. Con una de las 9 rota por aritmética propia y dos
   que no cubren lo que su nombre promete, la holgura numérica es real pero engañosa como señal
   de calidad.

3. La premisa del PLAN de que sincronia.rs es «lo único verificable» y por tanto suficiente para
   blindar «la decisión de diseño más importante del plan» (el reloj y el silencio). Cuestiono
   esto: la decisión de diseño real —qué se pasa como timestamp_ms— no vive en sincronia.rs, vive
   en wasapi_src.rs. sincronia.rs solo puede probar que, dado un timestamp correcto, el cálculo
   del hueco es correcto. Eso es necesario pero no es lo mismo que blindar la decisión, y el PLAN
   (y el HANDOFF, siguiéndolo) hablan como si lo fuera.

4. ¿Son las 2 pruebas añadidas (el_f32_del_dispositivo_pasa_sin_tocarse,
   el_i32_del_dispositivo_se_normaliza_igual_que_el_i16) una desviación del PLAN? Concluyo que no:
   cubren ramas de la misma función (normalizar_a_f32) que la prueba del PLAN
   (las_muestras_i16_...) no alcanza, y la propia HANDOFF lo declara como cambio menor de
   alcance, no oculto. Las considero una mejora legítima, no un hallazgo.

## 7. Qué verifiqué y no marqué

- PistaWasapi::nueva con frecuencia == SAMPLE_RATE: confirmé leyendo
  mezcla.rs::Remuestreador::nuevo (líneas 50-61) que en ese caso interno = None y procesar() hace
  pass-through directo (entrada.to_vec()), sin el bloqueo de 1024 muestras. Esto es relevante para
  que el fixture de un_silencio_en_loopback_no_adelanta_lo_que_viene_despues (que usa
  FormatoNativo::F32, SAMPLE_RATE) no se vea afectado por el buffering del remuestreador. No lo
  marco como hallazgo: el comportamiento es correcto y coherente con el resto de pruebas de
  mezcla.rs.
- Cargo.toml de core/audio-capture: confirmé la dependencia windows = 0.58 bajo el bloque de
  dependencias condicionales para Windows, con las tres features declaradas en HANDOFF. Coincide.
  No es mi rol evaluar si esas features son las correctas para el código de wasapi_src.rs
  (HANDOFF ya declara esa API como no verificada; es aviso, no hallazgo de pruebas).
- .github/workflows/ci.yml, job nucleo-windows: confirmé el paso nuevo tests de dictar-audio
  (cargo test -p dictar-audio, líneas 116-119) y que el job Linux (nucleo) también correría estos
  tests al no tener #[cfg] de plataforma. Coincide con lo declarado. Relevante para el hallazgo
  4.1: el test roto fallaría en ambos jobs el día que haya cargo, no solo en uno.
- La firma de AudioError::SinDispositivo(&static str) usada en pistas_a_grabar (sincronia.rs:98)
  contra su definición real en lib.rs:52-53: coincide. No lo marco.
- normalizar_a_f32 con bytes que no completan un chunk exacto (por ejemplo 3 bytes para I16):
  chunks_exact descarta el resto, igual patrón que a_mono en mezcla.rs (ya probado ahí con
  una_trama_incompleta_no_rompe_la_conversion). No hay una prueba equivalente específica para
  normalizar_a_f32, pero por ser el mismo patrón ya cubierto en el mismo crate, no lo elevo a
  hallazgo — lo dejo anotado como gap menor, no crítico.
- No revisé wasapi_src.rs en busca de pruebas: no tiene ninguna, por diseño explícito del PLAN, y
  no puede tenerlas sin hardware. No es un hallazgo de esta revisión, coincide con lo declarado
  por ambos documentos.
- No evalué estilo, nombres, organización del código ni arquitectura — corresponde a
  revisor-codigo, fuera de mi encargo.
- No corrí cargo fmt ni cargo clippy — no puedo (B-1), y tampoco es mi rol evaluarlos.
- No apliqué ninguna mutación real sobre el árbol de trabajo: todas las mutaciones de este
  informe están razonadas y, donde la lógica es aritmética pura, verificadas con cálculo manual o
  con Python (documentado en 4.1), no con cargo test. El árbol de git es idéntico al inicio y al
  final (ver git status --short abajo).

---

git status --short al empezar y al terminar (idéntico, no hice ningún cambio):

```
 M .github/workflows/ci.yml
 M .github/workflows/release.yml
 M .gitignore
 M README.md
 M app/android/app/build.gradle.kts
 M app/android/app/src/main/AndroidManifest.xml
 M app/lib/datos/repositorio_rust.dart
 M app/lib/pantallas/grabacion.dart
 M app/lib/pantallas/region.dart
 M app/lib/ventana.dart
 M app/pubspec.yaml
 M app/windows/CMakeLists.txt
 M app/windows/runner/resources/app_icon.ico
 M cli/src/sesion.rs
 M cli/src/transcribir.rs
 M cli/src/transcripcion.rs
 M core/api/src/puente.rs
 M core/audio-capture/Cargo.toml
 M core/audio-capture/src/lib.rs
 M core/audio-capture/src/mezcla.rs
 M core/audio-capture/src/reproductor.rs
 M core/audio-capture/src/wav.rs
 M core/providers/Cargo.toml
 M core/providers/src/secretos.rs
 M core/screen-capture/Cargo.toml
 M core/screen-capture/src/lib.rs
 M core/storage/src/migrations.rs
 M core/stt/src/limpieza.rs
 M packaging/icono.py
?? .claude/
?? .gitattributes
?? _orquestacion/
?? core/audio-capture/src/reproductor_stub.rs
?? core/audio-capture/src/sincronia.rs
?? core/audio-capture/src/wasapi_src.rs
?? docs/06-historias-de-usuario.md
```

(Los cambios en core/providers, core/api/src/puente.rs y app/lib/datos/repositorio_rust.dart son
de HU-05, en paralelo — no los toqué.)

---

## Segunda vuelta — tras la corrección de los 9 hallazgos (`REVIEW.md`, 2026-09-03)

Encargo acotado: re-verificar solo los 4 cambios sobre pruebas que ya había señalado (el falso
rojo de `dos_horas_de_muestras_no_desbordan_el_contador`, la réplica eliminada, el test del
silencio y el test renombrado) más 2 piezas nuevas (el parámetro `muestras_del_paquete` y el test
del `saturating_sub`). No re-audito `wasapi_src.rs` (hallazgos 1, 7, 8, de `revisor-codigo`, sin
pruebas propias por diseño) ni toco nada de HU-05.

### 1. Veredicto

**Sigue NO VERIFICABLE por B-1**, reconfirmado de primera mano en los tres entornos con shell
disponibles (Windows nativo, WSL `Ubuntu-26.04`, WSL `docker-desktop`). De los 6 puntos del
encargo: **los 6 quedan cerrados**. Verifiqué con lectura + aritmética en Python (nunca a mano)
las 6 pruebas tocadas o nuevas, con **8 mutaciones propuestas** (una prueba recibe dos), y en
ninguna encontré el patrón que busco — código roto que la prueba no detectaría. En particular:

- El error crítico de la primera vuelta (hallazgo 3 del `REVIEW.md`: falso rojo con código
  correcto) está corregido de verdad — el nuevo valor da `32_000` exacto, verificado con Python,
  no es un artefacto distinto ni una casualidad: la elección de base (`2^32 + 1_000_000`) es suma
  de tres múltiplos de 16, razonada, no tanteada.
- Los tres hallazgos Importantes de la familia «regla de la réplica» (4, 5, 6 del `REVIEW.md`)
  están cerrados: la réplica desapareció por completo (no solo del código, también verifiqué que
  no quedó mención fuera de un comentario explicativo), el test de silencio ahora distingue de
  verdad el silencio del audio real, y el test renombrado ya no promete blindar algo que no puede.
- El hallazgo Menor (9) está cerrado y con una cobertura mejor de la que pedía: el test nuevo
  ejercita el primer `saturating_sub`, y de paso el primer paquete de
  `un_silencio_en_loopback_no_adelanta_lo_que_viene_despues` (ya existente, no tocado a propósito
  para esto) resulta que ejercita el segundo — los dos `saturating_sub` de la función quedan
  cubiertos, no solo uno.
- Persiste, ya declarada desde la primera vuelta y no un hallazgo nuevo de esta ronda: los dos
  tests que sustituyen a la réplica no corren en ningún job de CI existente. Ver sección 4 para mi
  valoración de si eso es aceptable.
- Encontré una discrepancia menor entre lo declarado y lo real (el conteo de pruebas del HANDOFF),
  sin impacto en la calidad de las pruebas — ver «Premisas que cuestiono».

### 2. Qué pude ejecutar y qué no

```
$ command -v cargo rustc; cargo --version; rustc --version
/usr/bin/bash: line 1: cargo: command not found   (exit 127)
/usr/bin/bash: line 1: rustc: command not found   (exit 127)

$ wsl -d Ubuntu-26.04 -- bash -lc "command -v cargo rustc; cargo --version; rustc --version"
bash: cargo: command not found
bash: rustc: command not found

$ wsl -d docker-desktop -- bash -lc "command -v cargo rustc; cargo --version"
/bin/sh: bash: not found   (exit 127 — ni siquiera hay bash en esta distro)
```

B-1 reconfirmado en los tres entornos, de primera mano, no heredado del HANDOFF. No corrí
`cargo test -p dictar-audio`. Todo lo que sigue es lectura de
`core/audio-capture/src/sincronia.rs` y `core/audio-capture/src/lib.rs` tal como están hoy en el
árbol de trabajo (no versionados salvo `lib.rs`), más aritmética y simulación de estructuras de
datos **ejecutadas con Python** (scripts reproducidos abajo, no resumidos de memoria) — no es
`cargo test` real.

No apliqué ninguna mutación sobre el árbol: no usé `Edit`/`Write` sobre ningún `.rs` del
repositorio, solo `Read`/`Grep`/`Bash` de solo lectura y `python3` fuera del árbol. `git status
--short` es idéntico al principio y al final de esta segunda pasada (ver el bloque al final de
esta sección).

### 3. Tabla de mutaciones (segunda vuelta)

Los números de línea son los del archivo actual (`sincronia.rs` tiene hoy **10** `#[test]`, no 9
— ver «Premisas que cuestiono»).

| Prueba | Archivo:línea mutado | Mutación | Resultado |
|---|---|---|---|
| `dos_horas_de_muestras_no_desbordan_el_contador` | `sincronia.rs:90-91` — invertir operandos: `muestras_ya_emitidas.saturating_sub(esperadas)` | Intercambiar la resta | no ejecutado (B-1); verificado con Python: rojo (protege). `esperadas` (4 295 999 296) > `muestras_ya_emitidas` (4 295 967 296): la resta invertida satura a 0; `assert_eq!(relleno, 32_000)` falla |
| `fuera_de_windows_y_linux_iniciar_devuelve_no_soportada` | `lib.rs:180` — `Err(AudioError::NoSoportada)` a `Err(AudioError::SinDispositivo("otra"))` | Cambiar la variante de error de la rama de la tercera plataforma | no ejecutado (B-1) **y estructuralmente no ejecutable en ningún job de CI de hoy**, con o sin `cargo`: la matriz solo compila para `target_os = "linux"`/`"windows"`, así que este `#[cfg]` no se selecciona en ningún job. Razonado: si compilara, `assert!(matches!(err, AudioError::NoSoportada))` fallaría — rojo en teoría |
| `fuera_de_windows_y_linux_dispositivos_devuelve_no_soportada` | `lib.rs:198` — `Err(AudioError::NoSoportada)` a `Ok(Vec::new())` | Devolver una lista vacía en vez de propagar el error | mismo matiz que la fila anterior: no ejecutado (B-1) y fuera de todo job de CI actual. Razonado: `.unwrap_err()` entraría en pánico (`unwrap_err()` sobre un `Ok`) — rojo en teoría |
| `un_silencio_en_loopback_no_adelanta_lo_que_viene_despues` | `sincronia.rs:176-177` — anteponer `reales` y añadir el silencio después, no antes | Invertir el orden de construcción del `pcm` | no ejecutado (B-1); verificado con **simulación en Python** de ambas listas y los 3 `assert`: el primero (`pcm.len()`) sigue en verde (no detecta longitud), pero el segundo y el tercero (posición del silencio y del audio real) fallan los dos — rojo (protege). Con la versión de la primera vuelta (relleno de `0.0`) esta misma mutación no la detectaba ningún `assert` |
| `un_silencio_en_loopback_no_adelanta_lo_que_viene_despues` (segunda mutación, mismo test) | `sincronia.rs:92` — quitar `.saturating_sub(muestras_del_paquete)` entero | Revertir a la fórmula de antes del hallazgo 2 (el bug del doble conteo) | no ejecutado (B-1); verificado con Python: rojo (protege). El segundo paquete pasa de `pcm.len() == 30_400` a `pcm.len() == 32_000` (cuenta el paquete de reanudación dos veces); `assert_eq!(segundo.pcm.len(), 28_800 + 1_600)` falla |
| `el_relleno_resta_lo_ya_emitido_del_timestamp_esperado` | `sincronia.rs:90-91` — mismo intercambio de operandos que la primera fila | Intercambiar la resta | no ejecutado (B-1); verificado con Python: rojo (protege) en las dos aserciones del test (`0` deja de dar `0`; `30_400` deja de dar `30_400`) |
| `el_relleno_tambien_descuenta_las_muestras_del_propio_paquete` | `sincronia.rs:92` — quitar `.saturating_sub(muestras_del_paquete)` entero | Revertir a la fórmula de antes del hallazgo 2 | no ejecutado (B-1); verificado con Python: rojo (protege). `muestras_de_relleno(2_000, 1_600, 1_600)` pasaría de `28_800` a `30_400`; `assert_eq!` con `32_000 - 1_600 - 1_600` falla |
| `el_relleno_nunca_es_negativo_si_ya_se_emitio_de_mas` | `sincronia.rs:90-91` — quitar el primer `.saturating_sub`, resta normal | Quitar la saturación de la resta que puede ir negativa | no ejecutado (B-1); verificado con Python en los dos modos: en `debug`, `1_600u64 - 10_000u64` entra en pánico («attempt to subtract with overflow»); en `release`, envuelve a `18_446_744_073_709_543_216`. En ambos, `assert_eq!(relleno, 0)` falla — rojo (protege) |

Scripts de Python usados (reproducibles, no resumidos):

```python
SAMPLE_RATE = 16_000

def muestras_de_relleno(timestamp_ms, muestras_ya_emitidas, muestras_del_paquete):
    t = max(timestamp_ms, 0)
    esperadas = (t * SAMPLE_RATE) // 1000
    return max(max(esperadas - muestras_ya_emitidas, 0) - muestras_del_paquete, 0)

# 1) dos_horas_de_muestras_no_desbordan_el_contador, valor NUEVO
muestras_previas = (2**32 - 1) + 1 + 1_000_000          # 4 295 967 296 == 2**32 + 1_000_000
suma = muestras_previas + 32_000                         # 4 295 999 296
assert suma % 16 == 0                                    # ya no se pierde resto (antes: resto 15)
timestamp = (suma * 1000) // SAMPLE_RATE                  # 268 499 956
assert (suma * 1000) % SAMPLE_RATE == 0                   # ida y vuelta exacto
assert muestras_de_relleno(timestamp, muestras_previas, 0) == 32_000   # EXACTO, no aproximado

# contraste con el valor viejo (roto), para confirmar que no es casualidad
vieja = (2**32 - 1) + 1_000_000                            # u32::MAX + 1_000_000, sin el +1
suma_vieja = vieja + 32_000
timestamp_vieja = (suma_vieja * 1000) // SAMPLE_RATE
assert muestras_de_relleno(timestamp_vieja, vieja, 0) == 31_985   # el falso rojo original

# 2) las 4 llamadas con el parámetro nuevo, valores esperados
assert muestras_de_relleno(100, 1_600, 0) == 0
assert muestras_de_relleno(2_000, 1_600, 0) == 30_400
assert muestras_de_relleno(2_000, 1_600, 1_600) == 28_800
assert muestras_de_relleno(100, 10_000, 0) == 0

# 3) mutación: quitar el segundo saturating_sub, detectada por el PRIMER
#    paquete de un_silencio_en_loopback_no_adelanta_lo_que_viene_despues
#    (timestamp=0, emitidas=0, paquete=1600): esperadas=0, intermedio=0,
#    0 - 1600 sin saturar = -1600 -> underflow de u64 -> pánico/envoltura,
#    nunca 0. Con saturating_sub da 0 (verde, correcto).
```

Salida real de estos scripts (`python3`, ejecutado dos veces: valores nuevos y contraste con los
viejos):

```
muestras_previas = 4295967296
suma = 4295999296  mod 16 = 0
timestamp_correspondiente = 268499956  resto perdido en esta division = 0
relleno = 32000
OK: relleno == 32_000 exacto
[contraste con el valor VIEJO, roto, u32::MAX puro]
relleno_vieja = 31985  (coincide con el hallazgo 3 de la primera vuelta)
```

### 4. Pruebas que no protegen (segunda vuelta)

**Ninguna de las 6 pruebas tocadas o nuevas cae en este caso ahora.** Cierro explícitamente cada
hallazgo de pruebas que había abierto:

- **Hallazgo 3 del `REVIEW.md` (mi 4.1, Bloqueante) — CERRADO.** `dos_horas_de_muestras_no_
  desbordan_el_contador` ya no es un falso rojo. El comentario nuevo del test documenta la cifra
  exacta del error anterior (31 985) y el porqué de la nueva base — verificado con Python, cifra
  por cifra, coincide con lo que el propio comentario afirma.

- **Hallazgo 6 del `REVIEW.md` (mi 4.2, Importante) — CERRADO, por la vía de la honestidad, no de
  la conexión real.** `el_relleno_resta_lo_ya_emitido_del_timestamp_esperado` ya no se llama como
  si blindara la decisión de diseño del reloj. El nombre nuevo describe exactamente lo que la
  función hace (resta lo emitido de lo esperado) y nada más; el comentario dice explícitamente qué
  protege y qué no puede proteger por construcción (el origen real de `timestamp_ms` vive en
  `wasapi_src.rs`, sin pruebas posibles sin hardware). Verifiqué que las dos aserciones son
  aritméticamente correctas (con Python) y que ninguna frase del comentario afirma más de lo que
  el cuerpo del test ejercita. Sigue siendo cierto que ninguna prueba pura puede cubrir esa
  decisión — pero eso ya no se le atribuye falsamente a esta prueba.

- **Hallazgo 5 del `REVIEW.md` (mi 4.3, Importante) — CERRADO.** `un_silencio_en_loopback_no_
  adelanta_lo_que_viene_despues` usa `0.5` para el audio real y `0.0` para el relleno: ya no son
  indistinguibles. Confirmé con una simulación en Python (construí el `pcm` con la lógica correcta
  y con la mutación de orden invertido, y evalué los tres `assert` contra ambas) que el `assert!`
  de posición del audio real (`segundo.pcm[28_800..].iter().all(|&m| m == 0.5)`) es una aserción
  real: detecta la mutación que la versión anterior (con `0.0`) dejaba pasar en verde.

- **Hallazgo 4 del `REVIEW.md` (mi 4.4, Importante) — la réplica desapareció; matiz que persiste,
  no es hallazgo nuevo.** Confirmé con `grep -rn "enum Plataforma|fn tiene_backend"` en todo
  `core/audio-capture/src` que no queda ningún rastro de código real (solo la mención, como texto
  explicativo, en el comentario de `lib.rs` que documenta por qué se reemplazó). Los dos tests
  nuevos (`fuera_de_windows_y_linux_iniciar_devuelve_no_soportada`,
  `...dispositivos_devuelve_no_soportada`) llaman a `iniciar()` y `dispositivos()` reales de
  `lib.rs:166` y `lib.rs:185` — las mismas funciones públicas que expone el crate, no una
  indirección — y están gateados exactamente por el mismo `#[cfg(not(any(target_os = "linux",
  target_os = "windows")))]` que la rama de producción que verifican (confirmado línea por línea:
  `lib.rs:177`/`416` para `iniciar`, `lib.rs:196`/`427` para `dispositivos`). Esto cumple lo que el
  hallazgo 4 pedía (dejar de ser una réplica desconectada), y las mutaciones de la tabla de arriba
  muestran que, si corrieran, sí se pondrían en rojo.

  **La trampa que pregunta el encargo — ¿es aceptable que no corran en ningún job de CI hoy?**
  Mi valoración: **sí es aceptable para el alcance de esta HU**, porque el hallazgo 4 tal como está
  redactado en `REVIEW.md` pide «dejar de ser una réplica», no «conseguir que el test se ejecute
  en CI» — eso último es una decisión de infraestructura (tocar `ci.yml`) que ni el PLAN ni el
  `REVIEW.md` pusieron dentro del alcance de esta vuelta, y ya estaba señalada como límite
  estructural desde la primera vuelta (mi 4.4, `auditor-plataforma` H1). No es un hallazgo nuevo de
  esta ronda: es la misma deuda, ahora con una prueba que sí protegería si algún día se ejecutara,
  en vez de una que nunca lo haría. Dicho eso, no lo minimizo: la protección efectiva **hoy**
  sigue siendo cero para el invariante 4, y el propio código (comentario en `lib.rs:409-414`) y el
  HANDOFF lo declaran sin ocultarlo, lo cual es lo correcto.
  **Qué haría falta, más preciso que lo que dice el HANDOFF:** el HANDOFF y el comentario hablan de
  «un job que compile para un tercer `--target`» — pero *compilar* cruzado (`cargo check --target
  X`) no basta para *ejecutar* un `#[test]`: hace falta correrlo en una plataforma nativa de esa
  familia, o emularla. La opción más simple, disponible hoy sin trabajo de infraestructura nuevo,
  es un job con `runs-on: macos-latest` en GitHub Actions (macOS cae de forma natural en
  `not(any(linux, windows))`) que corra `cargo test -p dictar-audio` — no necesita cross-compile
  ni emulación. No verifiqué el costo/disponibilidad de minutos de runner macOS en el plan de este
  repositorio, así que lo dejo como observación, no como exigencia.

- **Hallazgo 9 del `REVIEW.md` (mi 4.5, Menor) — CERRADO, con mejor cobertura de la pedida.**
  `el_relleno_nunca_es_negativo_si_ya_se_emitio_de_mas` ejercita el primer `saturating_sub`
  (`esperadas.saturating_sub(muestras_ya_emitidas)`, `sincronia.rs:91`) con `esperadas <
  muestras_ya_emitidas`. Verifiqué además, simulando la mutación de quitar el *segundo*
  `saturating_sub` (el de `muestras_del_paquete`, línea 92), que **ese** caso ya lo cubre el primer
  paquete de `un_silencio_en_loopback_no_adelanta_lo_que_viene_despues` (existente, no escrito para
  esto): con `timestamp=0, emitidas=0, paquete=1600`, `esperadas=0` y `0 - 1600` sin saturar
  desborda igual. Los dos `saturating_sub` de la función quedan cubiertos, no solo el que pedía el
  hallazgo.

### 5. Criterios de aceptación de docs/06 (actualización de esta vuelta)

Encargo acotado a lo que tocan los 6 puntos de esta pasada; el resto de la tabla de la primera
vuelta (AC 1, 2, 4, 5, 6) no cambia y no la repito aquí.

| AC / invariante | Qué cambió esta vuelta | Estado |
|---|---|---|
| AC 3 — 16 kHz mono `f32`, `timestamp_ms` de reloj monótono común | La fórmula de relleno ya no cuenta dos veces el paquete de reanudación (hallazgo 2 de `REVIEW.md`, de `revisor-codigo`, no mío, pero verifiqué que las pruebas puras reflejan la fórmula corregida); el test que intentaba blindar la decisión del reloj ahora es honesto sobre sus límites en vez de prometer de más | Igual que antes: la parte pura, bien probada; el origen real del `timestamp_ms` (`wasapi_src.rs`) sigue sin poder probarse sin hardware, y ahora eso está reconocido en el propio nombre/comentario del test, no solo en este informe |
| Invariante 4 — ninguna firma pública cambia según la plataforma | La prueba que lo blinda ejercita ahora el `#[cfg]` real de `lib.rs`, no una réplica | Mejor que antes (protegería si corriera), pero sin ejecución en ningún job de CI existente — ver 4.4 arriba para el detalle y mi valoración |

### 6. Premisas que cuestiono

1. **HANDOFF: «9 pruebas en sincronia.rs (antes 7; +1 por el hallazgo 6, +1 por el hallazgo 9)».**
   Conté con `grep -c "#\[test\]" sincronia.rs`: son **10**, no 9. La aritmética que sí cuadra es
   la del archivo completo antes y después: la primera vuelta tenía 9 pruebas en `sincronia.rs`
   (incluida `fuera_de_windows_y_linux_sigue_devolviendo_no_soportada`, que mi propia tabla de la
   primera vuelta contaba); esta vuelta la quita de aquí (se movió, mejorada, a `lib.rs`) y añade
   dos (`el_relleno_tambien_descuenta_las_muestras_del_propio_paquete`,
   `el_relleno_nunca_es_negativo_si_ya_se_emitio_de_mas`): 9 − 1 + 2 = 10. El HANDOFF parece haber
   olvidado restar la que se fue. **Conclusión: es un desliz de conteo, no un problema de calidad
   de las pruebas** — las 10 existen, están bien, y el número real (10) es mejor que el declarado
   (9), no peor. Lo señalo porque el propio HANDOFF de esta vuelta insiste en que «esta vez sí»
   verificó todo con cuidado (con Python, no a mano): un desliz aritmético trivial, aunque inocuo,
   es precisamente el tipo de cosa que mi rol existe para no dar por buena sin comprobar.

2. **HANDOFF / comentario de `lib.rs:409-414`: el test del invariante 4 «protege de verdad en
   cuanto exista un job que compile para una tercera plataforma».** Cuestiono la precisión, no la
   intención: compilar cruzado no ejecuta un test. Hace falta un *runner* nativo de esa plataforma
   (o un emulador), no solo que el `#[cfg]` se seleccione en un `cargo check`. La afirmación no es
   falsa —de hecho compilar es condición necesaria— pero es incompleta sobre qué haría falta en la
   práctica. Ver mi propuesta concreta (`runs-on: macos-latest`) en la sección 4.

### 7. Qué verifiqué y no marqué

- `CaptureConfig` implementa `Default` (`lib.rs:104-120`, con `impl Default for CaptureConfig`
  explícito): `CaptureConfig::default()`, usado en el test nuevo de `iniciar()`, tiene de dónde
  salir. No es hallazgo, es una comprobación de que mi mutación propuesta (y el propio test) son
  sintácticamente plausibles.
- `AudioError::SinDispositivo(&'static str)` existe como variante real (`lib.rs:52-53`) y ya se
  usa con éxito en `pistas_a_grabar` (`sincronia.rs:112`): la usé como base de una de mis
  mutaciones propuestas para que fuera código que compilaría, no solo aritméticamente plausible.
- El único call site de producción de `muestras_de_relleno` (`sincronia.rs:171`, dentro de
  `PistaWasapi::procesar_paquete`) pasa `reales.len() as u64` — el valor real que acaba de calcular
  el remuestreador, no un valor fijo ni simulado — como tercer parámetro. Coincide con lo que el
  HANDOFF declara para el hallazgo 2. No hallazgo.
- `grep -rn "enum Plataforma|fn tiene_backend" core/audio-capture/src`: cero coincidencias de
  código real en todo el crate (solo el comentario explicativo ya citado). Confirma que la réplica
  se eliminó de raíz, no que se movió o se ocultó tras otro nombre.
- Leí el doc-comment de `muestras_de_relleno` (`sincronia.rs:68-83`, reescrito en esta vuelta):
  describe con precisión el porqué del tercer parámetro (el paquete que reanuda tras un silencio
  se contaba dos veces) y no promete más de lo que la función hace. No hallazgo.
- No re-audité `wasapi_src.rs` (hallazgos 1, 7, 8 del `REVIEW.md`, todos de `revisor-codigo`): sin
  pruebas propias por diseño explícito del PLAN, sin cambios respecto de mi primera pasada, y
  fuera de los 6 puntos de este encargo. Le corresponde a `revisor-codigo` reverificarlos en su
  propia segunda vuelta.
- No toqué ni leí `core/providers`, `core/api/src/puente.rs` ni `app/lib/datos/repositorio_rust.dart`
  (HU-05, en paralelo) — ni siquiera para descartar, tal como se me indicó explícitamente.
- No corrí `cargo fmt` ni `cargo clippy` — B-1, y tampoco es mi rol evaluarlos.
- No apliqué ninguna mutación real sobre el árbol de trabajo en esta segunda pasada tampoco: todas
  las de la tabla de la sección 3 están razonadas por lectura y verificadas con Python (aritmética
  pura) o con simulación de estructuras de datos equivalentes (el caso del orden del `pcm`), nunca
  con `cargo test`. No hizo falta ni siquiera copiar `sincronia.rs` al scratchpad: no lo edité en
  ningún momento, así que no hubo nada que restaurar.
- No verifiqué con un compilador que mis propias mutaciones propuestas sean válidas en Rust
  (tipos, *borrow checker*, etc.) más allá de razonarlas contra el resto del archivo (variantes de
  enum existentes, firmas de función existentes) — no puedo, por B-1. Son plausibles, no
  confirmadas.

`git status --short` al empezar y al terminar esta segunda pasada — idéntico, no hice ningún
cambio (comparado línea por línea con el bloque de la sección 7 original; los archivos que
aparecen de más aquí respecto de esa lista son los que HU-05 tocó *entre* mi primera y esta
segunda pasada, no algo que yo haya causado):

```
 M .github/workflows/ci.yml
 M .github/workflows/release.yml
 M .gitignore
 M README.md
 M app/android/app/build.gradle.kts
 M app/android/app/src/main/AndroidManifest.xml
 M app/lib/datos/repositorio_rust.dart
 M app/lib/pantallas/ajustes.dart
 M app/lib/pantallas/grabacion.dart
 M app/lib/pantallas/region.dart
 M app/lib/ventana.dart
 M app/pubspec.yaml
 M app/windows/CMakeLists.txt
 M app/windows/runner/resources/app_icon.ico
 M cli/src/sesion.rs
 M cli/src/transcribir.rs
 M cli/src/transcripcion.rs
 M core/api/src/puente.rs
 M core/audio-capture/Cargo.toml
 M core/audio-capture/src/lib.rs
 M core/audio-capture/src/mezcla.rs
 M core/audio-capture/src/reproductor.rs
 M core/audio-capture/src/wav.rs
 M core/providers/Cargo.toml
 M core/providers/src/secretos.rs
 M core/screen-capture/Cargo.toml
 M core/screen-capture/src/lib.rs
 M core/storage/src/migrations.rs
 M core/stt/src/limpieza.rs
 M packaging/icono.py
?? .claude/
?? .gitattributes
?? _orquestacion/
?? core/audio-capture/src/reproductor_stub.rs
?? core/audio-capture/src/sincronia.rs
?? core/audio-capture/src/wasapi_src.rs
?? docs/06-historias-de-usuario.md
```

(`core/providers`, `core/api/src/puente.rs`, `app/lib/pantallas/ajustes.dart` y
`app/lib/datos/repositorio_rust.dart` son de HU-05, en paralelo — no los toqué, ni siquiera para
leerlos.)

---

## Tercera vuelta — v2-3, `alguna_pista_sigue_viva` (`REVIEW.md` / `HANDOFF.md`, sección «Tercera vuelta», 2026-09-03)

Encargo acotado a las tres pruebas nuevas de `sincronia.rs:407-442`, que ejercitan
`alguna_pista_sigue_viva` (`sincronia.rs:117-131`, hallazgo v2-3). No reaudito `wasapi_src.rs`
más allá de lo necesario para responder al punto 2 del encargo (v2-1/v2-2 son control de flujo y
manejo de error en código sin pruebas propias por diseño — terreno de `revisor-codigo` en su
propia re-revisión), y no toco `core/providers` ni nada de HU-05 (en re-revisión aparte).

### 1. Veredicto

**NO VERIFICABLE por B-1** (reconfirmado de primera mano, Windows nativo y WSL `Ubuntu-26.04`;
detalle en §2). Es un booleano de dos entradas: el análisis de las 4 combinaciones **sí es
exhaustivo sin compilador**, y lo tabulé con Python. Resultado: **las tres pruebas cubren solo 3
de las 4 combinaciones** — falta `(true, true)`, que además es razonablemente el estado más
frecuente en producción (las dos pistas vivas, grabando con normalidad; las otras tres
combinaciones son todas casos de fallo). Con ese hueco, una mutación de una sola línea
(`sincronia.rs:130`, cambiar `||` por `!=`) pasa las tres pruebas en verde: **hallazgo
Importante**. Las mutaciones más directas del encargo (`||`→`&&`, resultado invertido, ignorar un
parámetro) sí quedan atrapadas, las tres, solo con la primera prueba. Además: la tercera prueba
no encadena de verdad `pistas_a_grabar` con `alguna_pista_sigue_viva` pese a que su propio
`HANDOFF` dice que sí — son dos aserciones independientes con literales escritos a mano, no un
valor que fluya de una llamada a la otra —, y medida por mutación no aporta ninguna detección que
las otras dos pruebas del archivo no den ya: **hallazgo Menor**, no llega a ser una réplica en el
sentido estricto de `protocolo.md` porque las dos mitades sí llaman código de producción real.

### 2. Qué pude ejecutar y qué no

```
$ command -v cargo rustc flutter
/c/dev/flutter/bin/flutter

$ cargo --version
/usr/bin/bash: line 1: cargo: command not found   (exit 127)

$ rustc --version
/usr/bin/bash: line 1: rustc: command not found   (exit 127)

$ wsl -l -v
docker-desktop   Stopped   2
Ubuntu-26.04     Stopped   2

$ wsl -d Ubuntu-26.04 -- bash -lc "command -v cargo rustc; cargo --version; rustc --version"
# primer intento: la VM todavía no había arrancado, timeout de HCS
#   (clave wsl2.autoMemoryReclaim desconocida en .wslconfig + HCS_E_CONNECTION_TIMEOUT)
# segundo intento, ya con la VM arriba: se completó
bash: cargo: command not found   (exit 127)
bash: rustc: command not found   (exit 127)

$ git diff -- core/audio-capture/src/sincronia.rs
(sin salida: archivo sin versionar, git no tiene base con la que comparar — ver §8.3)
```

Coincide con `_orquestacion/tablero.md` (bloqueo B-1, «verificado el 03/09: no existen cargo,
rustc ni rustfmt, ni en Windows ni en Ubuntu-26.04 de WSL») y con las dos vueltas anteriores de
este mismo documento. No repetí la comprobación en `docker-desktop`: la segunda vuelta ya dejó
constancia de que esa distro no tiene `bash`, y el propio encargo de hoy lo da por sabido.

No apliqué ninguna mutación real sobre el árbol: sin compilador no hay forma de confirmar
rojo/verde ejecutando, así que todo lo de abajo es lectura más verificación lógica/aritmética con
Python — no sustituye a `cargo test`. Por la instrucción explícita del encargo (el archivo no está
versionado, `git checkout` no lo restauraría), copié `core/audio-capture/src/sincronia.rs` al
scratchpad antes de leerlo (`sincronia.rs.bak`); no llegué a editarlo, así que no hubo nada que
restaurar. `git status --short` es idéntico al empezar y al terminar (bloque al final de esta
sección).

### 3. Tabulación exhaustiva de las 4 combinaciones (Python)

`alguna_pista_sigue_viva(microfono_vivo, sistema_vivo) -> bool` es, línea por línea,
`microfono_vivo || sistema_vivo` (`sincronia.rs:130`). Cuatro combinaciones posibles; los puntos
que cada prueba ejercita, leyendo `sincronia.rs:407-442`:

- `una_pista_muerta_no_frena_a_la_otra` (líneas 414-415): `(true, false)` y `(false, true)`,
  esperado `true` las dos veces.
- `sin_ninguna_pista_viva_el_bucle_termina` (línea 424): `(false, false)`, esperado `false`.
- `una_sesion_de_una_sola_pista_termina_si_esa_pista_muere` (línea 441): `(false, false)` otra
  vez — el mismo punto, no uno nuevo (desarrollo en §6).

Script (`tabulacion_v3.py`, en el scratchpad de esta sesión, ejecutado dos veces sin diferencias):

```python
combos = [(False, False), (False, True), (True, False), (True, True)]
def real(a, b): return a or b
mutaciones = {
    "&& (or -> and)":                 lambda a, b: a and b,
    "resultado invertido":            lambda a, b: not (a or b),
    "ignora sistema_vivo (a sola)":   lambda a, b: a,
    "ignora microfono_vivo (b sola)": lambda a, b: b,
    "XOR (!=)":                       lambda a, b: a != b,
    "siempre True":                   lambda a, b: True,
    "siempre False":                  lambda a, b: False,
}
puntos_testeados = [(True, False), (False, True), (False, False)]
# sobrevive en verde si coincide con la funcion real en los 3 puntos testeados,
# sin que importe si coincide en el punto sin testear
```

Salida real:

```
(mic,sist)      real   testeado   &&     invertido  ignora_a  ignora_b  XOR    siempreT  siempreF
(False,False)   False  SI         False  True       False     False     False  True      False
(False,True )   True   SI         False  False      False     True      True   True      False
(True ,False)   True   SI         False  False      True      False     True   True      False
(True ,True )   True   NO <-      True   False      True      True      False  True      False

&& (or -> and)                    -> detectada (rojo)
resultado invertido                -> detectada (rojo)
ignora sistema_vivo (a sola)      -> detectada (rojo)
ignora microfono_vivo (b sola)    -> detectada (rojo)
XOR (!=)                          -> SOBREVIVE EN VERDE -- las 3 pruebas no la detectan
siempre True                      -> detectada (rojo)
siempre False                     -> detectada (rojo)

Combinacion sin ningun test que la ejercite: (True, True)
```

### 4. Tabla de mutaciones

| Prueba | Archivo:línea mutado | Mutación | Resultado |
|---|---|---|---|
| `una_pista_muerta_no_frena_a_la_otra` | `sincronia.rs:130`, cuerpo de la función a `microfono_vivo && sistema_vivo` | OR por AND | no ejecutado (B-1); verificado con Python: rojo (protege). En `(true, false)` la mutación da `false`; el `assert!` de la línea 414 falla |
| `una_pista_muerta_no_frena_a_la_otra` | `sincronia.rs:130`, cuerpo a solo `microfono_vivo` | Ignorar `sistema_vivo` | no ejecutado (B-1); verificado con Python: rojo (protege). En `(false, true)` la mutación da `false`; el `assert!` de la línea 415 falla |
| `una_pista_muerta_no_frena_a_la_otra` | `sincronia.rs:130`, cuerpo a solo `sistema_vivo` | Ignorar `microfono_vivo` | no ejecutado (B-1); verificado con Python: rojo (protege). En `(true, false)` la mutación da `false`; el `assert!` de la línea 414 falla |
| `sin_ninguna_pista_viva_el_bucle_termina` | `sincronia.rs:130`, cuerpo a `!(microfono_vivo \|\| sistema_vivo)` | Resultado invertido | no ejecutado (B-1); verificado con Python: rojo (protege) — y ya detectada antes por `una_pista_muerta_no_frena_a_la_otra`, que falla en sus dos combinaciones |
| `una_sesion_de_una_sola_pista_termina_si_esa_pista_muere` (mitad 1, líneas 436-437) | `sincronia.rs:105`, `if microfono_abierto` a `if !microfono_abierto` | Invertir la condición de `pistas_a_grabar` | no ejecutado (B-1); verificado con lectura/Python: rojo (protege) — pero detectada de forma idéntica por la prueba preexistente `sin_dispositivo_de_salida_se_graba_solo_el_microfono` (`sincronia.rs:390-394`): misma llamada `pistas_a_grabar(true, false)`, misma aserción sobre el vector resultante |
| `una_sesion_de_una_sola_pista_termina_si_esa_pista_muere` (mitad 2, línea 441) | `sincronia.rs:130`, cualquiera de las mutaciones de arriba que falle en `(false, false)` | Ej. «siempre True» | no ejecutado (B-1); rojo (protege) — pero exactamente el mismo punto `(false, false)` que ya cubre `sin_ninguna_pista_viva_el_bucle_termina`: no añade ninguna combinación nueva |
| Las tres pruebas juntas | `sincronia.rs:130`, cuerpo de la función a `microfono_vivo != sistema_vivo` | OR por XOR | no ejecutado (B-1); verificado con Python: **verde (NO protege)**. Coincide con la función real en las tres combinaciones que las pruebas ejercitan y solo difiere en `(true, true)`, que ninguna de las tres toca |
| Las tres pruebas juntas (fuera de su alcance por diseño) | `wasapi_src.rs:281-283`, quitar el `break` condicional entero, o invertir su condición | Reintroducir el defecto que v2-1 corrigió, en el llamador | **no ejecutable por estas pruebas, con o sin B-1**: `wasapi_src.rs` es `#[cfg(target_os = "windows")]` sin ningún `#[cfg(test)]` propio; las tres pruebas de `sincronia.rs` no compilan ni ejecutan ni un carácter de ese archivo. Desarrollo en §5 |

### 5. Política vs. bucle — mi juicio (punto 2 del encargo)

Leí `wasapi_src.rs:230-292` para confirmarlo de primera mano, no solo citando el `HANDOFF`: la
llamada de producción existe, en la línea que se declara (`wasapi_src.rs:281`, la condición es
`!crate::sincronia::alguna_pista_sigue_viva(mic.is_some(), sistema.is_some())`), y el `break` está
en la línea siguiente (283).

**Lo que las tres pruebas protegen:** que `alguna_pista_sigue_viva`, la función pura, devuelva el
booleano correcto para tres de las cuatro combinaciones de entrada — que la política en sí esté
bien escrita.

**Lo que no protegen, y no pueden proteger en este entorno:**

- Que `wasapi_src.rs:281` llame a esa función con los argumentos correctos y en el orden correcto
  (estado del micrófono, estado del sistema) en cada vuelta del bucle real. Si mañana alguien
  intercambiara los dos argumentos, o llamara a la función una sola vez fuera del bucle y
  cacheara el resultado, estas tres pruebas seguirían en verde sin enterarse.
- Que el `break` condicional de verdad se ejecute cuando la política dice que no queda ninguna
  pista viva. Si ese `break` desapareciera —el defecto original de v2-1—, las tres pruebas
  seguirían en verde: no tocan `wasapi_src.rs` en absoluto.
- Que las variables que representan cada pista reflejen de verdad, en cada vuelta, si esa pista
  murió. Eso depende de que la limpieza de pista se llame y de que la reasignación ocurra en el
  sitio correcto dentro de `bucle()`, que es control de flujo de COM, no de `sincronia.rs`.
- Que soltar el `Sender` al retornar cierre de verdad el canal. Esto lo verificó el orquestador
  por lectura (el `Sender` se toma por valor en la firma, sin ningún `.clone()` en todo el
  archivo — ver `tablero.md`, «Dos comprobaciones del orquestador»), no ninguna prueba
  automática, y sigue sin poder verificarse en ejecución sin un Windows real.

En una frase: v2-1 era un hallazgo sobre el bucle (le faltaba un `break`), y lo que esta vuelta
corrigió y probó es la política que ese `break` debería consultar, no el `break` en sí. La
distancia entre «la política es correcta» y «el bucle la consulta y actúa en consecuencia» sigue
exactamente tan sin probar como antes de esta vuelta. La diferencia real es que ahora hay una
pieza menos que pueda estar mal (la política, ya extraída y probada), y estructuralmente no puede
haber más pruebas sin tocar el entorno: `wasapi_src.rs` no tiene ni puede tener un `#[cfg(test)]`
que corra sin Windows.

Esto no es un defecto oculto de esta vuelta: es el límite que ya declara el propio doc-comment de
`alguna_pista_sigue_viva` (`sincronia.rs:123-125`, «la apertura real y el sondeo COM viven en
`wasapi_src.rs` y no se pueden probar aquí sin envolver COM entero») y que el `HANDOFF` reconoce
sin ocultarlo en «Deuda que dejo» («sigue sin ninguna cobertura... que el break se ejecute, y que
`tx` se suelte al retornar»). Mi juicio: la entrega es honesta sobre este límite. No lo elevo a
hallazgo nuevo — es la confirmación, con lectura propia de `wasapi_src.rs:281-283`, de un límite
ya declarado por quien implementó.

### 6. La tercera prueba no encadena — son dos aserciones independientes (punto 3 del encargo)

El `HANDOFF` (líneas 376-384) describe `una_sesion_de_una_sola_pista_termina_si_esa_pista_muere`
así: «encadena `pistas_a_grabar(true, false)`... con `alguna_pista_sigue_viva(false, false)`...
encadenar la función real de arranque con la de en-vivo deja constancia de que las dos
políticas... dan la misma respuesta correcta para este caso, sin que nadie tuviera que decidirlo a
mano». Leí el cuerpo del test (`sincronia.rs:428-442`) buscando esa cadena:

```rust
let pistas_al_arrancar = pistas_a_grabar(true, false).unwrap();
assert_eq!(pistas_al_arrancar, vec![Track::Mic]);

let microfono_vivo = false; // la única pista pedida, y murió
let sistema_vivo = false; // nunca se pidió, no es que fallara
assert!(!alguna_pista_sigue_viva(microfono_vivo, sistema_vivo));
```

No hay encadenamiento de valores. `microfono_vivo` y `sistema_vivo` son literales `false`
escritos a mano, no una expresión que dependa de `pistas_al_arrancar` (por ejemplo, algo que
mirara si `Track::Mic` sigue en ese vector). El test hace dos aserciones independientes, una
detrás de otra, y las llama «encadenar» en el comentario porque narrativamente describen el mismo
escenario (una sesión que arrancó solo con micrófono, y ese micrófono muere) — pero en código no
hay ningún dato que fluya de la primera llamada a la segunda.

Aplico la pregunta de la regla de la réplica (`protocolo.md`: «¿qué línea del código de
producción ejecuta esta prueba?»): en las dos mitades, la respuesta es una línea real
(`pistas_a_grabar` en `sincronia.rs:103` y `alguna_pista_sigue_viva` en `sincronia.rs:130`), no
una versión hecha a mano — así que no es una réplica en el sentido estricto que define
`protocolo.md` (los dos ejemplos que la originaron, un enum paralelo y una cadena de resolutores
reconstruida a mano, sustituían la función de producción por una copia; aquí no se sustituye
nada). Es un defecto más leve y distinto: la prueba afirma una relación entre dos llamadas que el
código no expresa, y esa afirmación no se sostiene si se le pide evidencia con mutación — la tabla
de §4 lo muestra en las dos mitades: cada una, por separado, es indetectable de una prueba
preexistente o hermana. Clasifico esto como **Menor**: no protege menos de lo que su nombre
promete en el sentido de dejar pasar un defecto real (si el código se rompe, el test sí se pone en
rojo, en las dos mitades), pero su justificación como prueba separada —«dejar constancia de que
encadenar dos políticas da la misma respuesta»— no está respaldada por lo que el cuerpo del test
ejercita.

### 7. Criterios de aceptación de `docs/06` (actualización de esta vuelta)

Encargo acotado a lo que tocan las tres pruebas nuevas; el resto de la tabla de la primera vuelta
(AC 1, 4, 5, 6) no cambia y no la repito aquí.

| AC / invariante | Qué cambió esta vuelta | Estado |
|---|---|---|
| AC 2 — dos pistas separadas; política de qué grabar «en vivo» | Nuevas `una_pista_muerta_no_frena_a_la_otra`, `sin_ninguna_pista_viva_el_bucle_termina`, `una_sesion_de_una_sola_pista_termina_si_esa_pista_muere` cubren 3 de las 4 combinaciones de `alguna_pista_sigue_viva` | Mejor que antes (la política «en vivo» no tenía ninguna prueba). Incompleto: falta `(true, true)` — el caso normal de grabación con las dos pistas vivas —, y la parte de `wasapi_src.rs` que consulta la política y hace `break` sigue sin ninguna prueba, por diseño (§5) |
| AC 7 — al menos un test sin tarjeta de sonido, ejemplo explícito «manejo del cambio de dispositivo por defecto» | Las tres pruebas nuevas son las primeras de todo el archivo que apuntan directamente a ese ejemplo del AC (antes solo `pistas_a_grabar`, la política de arranque, lo cubría) | Cumplido en la letra, con el matiz de cobertura de §3-4 |
| Invariante 1 — no se pierde audio, ni siquiera de forma silenciosa | v2-1 (el `break` que faltaba) queda protegido en su parte de política, no en su parte de bucle | La corrección en sí —que el hilo deje de "grabar en silencio" sin cerrar el canal— sigue sin ninguna prueba automatizada (§5). No es un hallazgo nuevo: ya lo declara el propio `HANDOFF` en «Deuda que dejo» |

### 8. Premisas que cuestiono

1. **`HANDOFF`: «Tres pruebas, las tres pedidas»**, y que la tercera «encadena... para cubrir el
   tercer caso pedido explícitamente». Cuestiono el «encadena»: ver §6. No cuestiono que sean tres
   pruebas legítimas (las tres llaman código de producción real), pero la tercera no agrega
   protección medible más allá de lo que ya dan las otras dos combinadas.
2. **`HANDOFF`, sobre v2-3, en «No pude verificar»: son aserciones sobre una función OR de dos
   booleanos, sin ningún camino oculto, así que el riesgo de que fallen por un error de tipeo es
   bajo.** De acuerdo en que compilarían sin ambigüedad de tipos — eso no lo cuestiono. Pero «sin
   ningún camino oculto» no es lo mismo que «sin ningún hueco de cobertura»: falta un cuarto de
   las combinaciones posibles de la propia función, y es precisamente el camino que un error de
   tipeo plausible (OR por XOR, o un operador mal puesto en un refactor) explotaría sin que
   ninguna de las tres pruebas lo note (§3-4).
3. **El método pedido en el encargo: usar `git diff` para confirmar que las 10 pruebas previas de
   `sincronia.rs` siguen intactas.** Cuestiono el método, no la conclusión a la que llegué por
   otra vía. `sincronia.rs` nunca estuvo versionado en ningún punto de esta HU (`??` desde la
   primera vuelta), así que `git diff` no tiene ninguna base con la que comparar: corrí
   `git diff -- core/audio-capture/src/sincronia.rs` y da salida vacía (§2). Sustituí el método:
   comparé línea por línea el contenido actual de las 10 pruebas preexistentes
   (`sincronia.rs:233-403`) contra lo transcrito y verificado —cuerpos casi completos, con sus
   valores exactos— en la sección «Segunda vuelta» de este mismo documento. Coinciden nombre por
   nombre y línea por línea: las 10 siguen intactas. Pero quiero que quede explícito que esa es
   una comparación de lectura contra un documento propio anterior, no la garantía que da un diff
   real contra un commit — la diferencia importa porque es exactamente el tipo de sustitución
   («verificar por lectura en vez de por ejecución») que este rol existe para no dar por buena
   sin señalar.

### 9. Qué verifiqué y no marqué

- Conté con `grep -c "#\[test\]" sincronia.rs`: **13** pruebas (10 preexistentes + 3 nuevas),
  coincide exactamente con la tabla de AC 7 del `HANDOFF` («13... 10 al empezar esta vuelta, +3
  por v2-3»). A diferencia de la segunda vuelta (donde el `HANDOFF` se equivocó en el conteo por
  1), esta vez el número declarado es correcto.
- Leí `wasapi_src.rs:230-292` para confirmar por mí mismo, no solo citando el `HANDOFF`, la línea
  exacta de la llamada de producción a `alguna_pista_sigue_viva` (281) y del `break` (283) — la
  evidencia que sostiene §5. No es una auditoría de ese archivo (v2-1/v2-2 son terreno de
  `revisor-codigo` en su propia re-revisión), solo la confirmación puntual necesaria para
  responder al punto 2 del encargo con lectura propia.
- Comparé las 10 pruebas preexistentes de `sincronia.rs` (líneas 233-403) contra lo ya verificado
  en la sección «Segunda vuelta» de este documento: idénticas. No repetí la aritmética de
  `muestras_de_relleno` (ya verificada dos veces, en dos vueltas anteriores) porque el código y
  los tests que la ejercitan no cambiaron en esta vuelta.
- Confirmé que `alguna_pista_sigue_viva` no lleva `#[cfg]` de plataforma (`sincronia.rs:129`) y
  que `sincronia` es un módulo público sin `#[cfg]` (`lib.rs:14`), así que las tres pruebas nuevas
  correrían en el job de Linux del CI igual que las 10 preexistentes — mismo argumento que ya
  vale para `pistas_a_grabar`. No hallazgo.
- No toqué `core/providers`, `core/api/src/puente.rs` ni nada de HU-05 — en re-revisión, fuera de
  este encargo, ni siquiera para leerlos.
- No evalué estilo, nombres, organización ni arquitectura del código nuevo — corresponde a
  `revisor-codigo`.
- No corrí `cargo fmt` ni `cargo clippy` — B-1, y tampoco es mi rol evaluarlos.
- No apliqué ninguna mutación real sobre el árbol de trabajo: todas las de la tabla de §4 están
  razonadas por lectura y verificadas con Python (tabulación booleana exhaustiva), nunca con
  `cargo test`. `sincronia.rs.bak` en el scratchpad quedó sin usar — no hizo falta restaurar nada
  porque no llegué a editar el original.

---

`git status --short` al empezar y al terminar esta tercera pasada — idéntico:

```
 M .github/workflows/ci.yml
 M .github/workflows/release.yml
 M .gitignore
 M README.md
 M app/android/app/build.gradle.kts
 M app/android/app/src/main/AndroidManifest.xml
 M app/lib/datos/repositorio_rust.dart
 M app/lib/pantallas/ajustes.dart
 M app/lib/pantallas/grabacion.dart
 M app/lib/pantallas/region.dart
 M app/lib/ventana.dart
 M app/pubspec.yaml
 M app/windows/CMakeLists.txt
 M app/windows/runner/resources/app_icon.ico
 M cli/src/sesion.rs
 M cli/src/transcribir.rs
 M cli/src/transcripcion.rs
 M core/api/src/puente.rs
 M core/audio-capture/Cargo.toml
 M core/audio-capture/src/lib.rs
 M core/audio-capture/src/mezcla.rs
 M core/audio-capture/src/reproductor.rs
 M core/audio-capture/src/wav.rs
 M core/providers/Cargo.toml
 M core/providers/src/secretos.rs
 M core/screen-capture/Cargo.toml
 M core/screen-capture/src/lib.rs
 M core/storage/src/migrations.rs
 M core/stt/src/limpieza.rs
 M packaging/icono.py
?? .claude/
?? .gitattributes
?? _orquestacion/
?? core/audio-capture/src/reproductor_stub.rs
?? core/audio-capture/src/sincronia.rs
?? core/audio-capture/src/wasapi_src.rs
?? docs/06-historias-de-usuario.md
```

(Igual que en las dos vueltas anteriores: `core/providers`, `core/api/src/puente.rs`,
`app/lib/pantallas/ajustes.dart` y `app/lib/datos/repositorio_rust.dart` son de HU-05, en
re-revisión aparte — no los toqué, ni siquiera para leerlos. `wasapi_src.rs` lo leí en esta
vuelta, sin editarlo, para el punto 2 del encargo — sigue `??`, sin cambios míos.)

