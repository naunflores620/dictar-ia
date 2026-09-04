# REVIEW-pruebas — T-15 (item c: v3-1 y v3-2 de HU-01, `sincronia.rs`)

## 1. Veredicto en una línea

**NO VERIFICABLE con `cargo`/`rustc` (B-1 persiste, comprobado de nuevo hoy)**: análisis
exhaustivo por simulación en Python — concluyente para funciones puras de 2 entradas booleanas —
confirma que **v3-1 cierra las 4 combinaciones y ninguna de las 8 mutaciones probadas sobrevive**;
**v3-2 no aporta ninguna detección nueva**: la prueba sigue siendo, tras el "arreglo", 100%
redundante con otras dos pruebas ya existentes — el mismo defecto que el hallazgo original
describía, solo que reubicado.

## 2. Qué pude ejecutar y qué no

No pude ejecutar `cargo test` — ni nada compilado — porque no hay compilador disponible,
comprobado hoy por tres vías independientes:

```
$ command -v cargo rustc flutter
/c/dev/flutter/bin/flutter
$ cargo --version
bash: cargo: command not found
$ rustc --version
bash: rustc: command not found
```

```
PS> Get-Command cargo,rustc -ErrorAction SilentlyContinue    (sin salida: ninguno existe)
PS> where.exe cargo
INFORMACION: no se pudo encontrar ningun archivo para los patrones dados.
PS> where.exe rustc
INFORMACION: no se pudo encontrar ningun archivo para los patrones dados.
```

```
$ wsl -d Ubuntu-26.04 -- bash -lc "command -v cargo rustc; cargo --version; rustc --version"
wsl: Clave 'wsl2.autoMemoryReclaim' desconocida en C:\Users\naunf\.wslconfig:16
La operacion supero el tiempo de espera... HCS_E_CONNECTION_TIMEOUT
$ wsl -l -v
  NAME              STATE      VERSION
* docker-desktop     Stopped    2
  Ubuntu-26.04        Stopped    2
```

Contrario a lo que se esperaba, el PO no instaló `cargo` hoy: ni siquiera hay `rustc` suelto (no
es solo que falte el *wrapper* `cargo`), y la VM de WSL sigue sin arrancar — el mismo
HCS_E_CONNECTION_TIMEOUT que ya había reportado el verificador-pruebas en la tercera vuelta de
HU-01. B-1 sigue exactamente igual. No edité `sincronia.rs` en el árbol real: sin compilador,
mutar el archivo y no poder correr nada contra él no verifica nada, así que hice el trabajo por
simulación en Python (sancionado explícitamente para este caso: función pura de dos entradas
booleanas, dominio exhaustivo de 4 combinaciones).

Lo que sí hice, y es la base de este informe:

- Copié `core/audio-capture/src/sincronia.rs` (no versionado) a
  `...\scratchpad\sincronia.rs.orig` antes de tocar nada.
- Encontré en el propio scratchpad un `sincronia.rs.bak` de una sesión anterior (15:23 de hoy) que
  resultó ser la versión previa a la corrección de v3-1/v3-2, y lo usé como base real de
  comparación (`diff -u`) en vez de reconstruir el "antes" de memoria — más fuerte que lo que pude
  hacer en la propia tercera vuelta, donde ese "antes" se declaró como lectura comparada sin diff
  real.
- Escribí y corrí dos scripts Python que reproducen fielmente la lógica pura de `sincronia.rs`
  (`alguna_pista_sigue_viva`, `pistas_a_grabar`, y el cuerpo de las pruebas con sus `assert!` /
  `assert_eq!`), en `...\scratchpad\T-15\mutaciones_v3_1.py` y
  `...\scratchpad\T-15\encadenamiento_v3_2.py`. Salida completa abajo.

## 3. Tabla de mutaciones

Leyenda de "Resultado": `rojo (protege)` / `verde (NO protege)`, con **[Python]** cuando se
verificó por simulación exhaustiva (no hay `cargo test` real que ejecutar, B-1). No lo marco como
"no ejecutado (B-1)" a secas porque, para una función pura de 2 entradas booleanas, la enumeración
de las 4 combinaciones es matemáticamente exhaustiva: de las 16 funciones booleanas de 2 entradas
posibles, solo una coincide con `||` en las 4 filas de la tabla de verdad, así que cualquier
mutación que cambie el comportamiento la detecta necesariamente alguna de las 4 pruebas. Eso no
sustituye a `cargo test` (no confirma que el crate compile) — ver §7.

### v3-1 — `alguna_pista_sigue_viva`, `sincronia.rs:130` (`microfono_vivo || sistema_vivo`)

| Prueba | Archivo:línea mutado | Mutación | Resultado |
|---|---|---|---|
| `con_las_dos_pistas_vivas_la_sesion_sigue` (línea 419) | sincronia.rs:130 | `||` a `!=` (xor) | rojo (protege) [Python] — es la única de las 4 pruebas que la detecta |
| `una_pista_muerta_no_frena_a_la_otra` (línea 408) | sincronia.rs:130 | `||` a `&&` | rojo (protege) [Python] |
| suite de 4 pruebas | sincronia.rs:130 | resultado invertido, `!(m \|\| s)` | rojo (protege) [Python] — las 4 la detectan |
| `una_pista_muerta_no_frena_a_la_otra` (mitad `(false,true)`) | sincronia.rs:130 | ignora `sistema_vivo`, devuelve solo `microfono_vivo` | rojo (protege) [Python] |
| `una_pista_muerta_no_frena_a_la_otra` (mitad `(true,false)`) | sincronia.rs:130 | ignora `microfono_vivo`, devuelve solo `sistema_vivo` | rojo (protege) [Python] |
| `sin_ninguna_pista_viva_el_bucle_termina` y v3-2 | sincronia.rs:130 | siempre `true` | rojo (protege) [Python] |
| `una_pista_muerta_no_frena_a_la_otra` y v3-1 | sincronia.rs:130 | siempre `false` | rojo (protege) [Python] |
| 4 de las 5 aserciones | sincronia.rs:130 | `||` a `==` (equivalencia) | rojo (protege) [Python] |

Ninguna mutación sobrevive. Con las 4 combinaciones cubiertas por las 4 pruebas, no puede quedar
ninguna: es una consecuencia matemática, no un hallazgo empírico que dependa de qué mutaciones se
me ocurrió probar (ver la prueba combinatoria en la salida del script, §3.1).

### v3-2 — `una_sesion_de_una_sola_pista_termina_si_esa_pista_muere` (línea 438)

| Prueba | Archivo:línea mutado | Mutación | Resultado |
|---|---|---|---|
| `una_sesion_de_una_sola_pista_termina_si_esa_pista_muere` | `pistas_a_grabar`, sincronia.rs:108 (`if sistema_abierto`) | `if sistema_abierto` a `if !sistema_abierto` | rojo (protege) [Python] — pero por el `assert_eq!` de la línea 447, que ya existía antes del fix, no por la derivación nueva de la línea 455 (detalle en §4) |
| ídem, revirtiendo solo la derivación (`sistema_vivo` de vuelta a literal `false`) | sincronia.rs:455 | `pistas_al_arrancar.contains(&Track::System)` a literal `false` | mismo veredicto que con la derivación, para las 4 variantes de `pistas_a_grabar` probadas — la derivación no cambia ni un solo caso |

### 3.1 Salida completa de los scripts

`mutaciones_v3_1.py` (tabulación + barrido de 8 mutaciones + prueba combinatoria):

```
== 1. Cobertura de las 4 combinaciones ==
  (False, False): cubierta
  (False, True): cubierta
  (True, False): cubierta
  (True, True): cubierta
  -> Las 4 combinaciones estan cubiertas.

== 3. Barrido de mutaciones contra la suite completa (4 pruebas) ==
  || -> &&: ROJO -> detectada por una_pista_muerta_no_frena_a_la_otra [1] y [2]
  || -> != (xor): ROJO -> detectada solo por con_las_dos_pistas_vivas_la_sesion_sigue
  resultado invertido: ROJO -> detectada por las 5 aserciones
  ignora sistema_vivo (devuelve m): ROJO -> detectada por una_pista_muerta_no_frena_a_la_otra [2]
  ignora microfono_vivo (devuelve s): ROJO -> detectada por una_pista_muerta_no_frena_a_la_otra [1]
  siempre true: ROJO -> detectada por sin_ninguna_pista_viva_el_bucle_termina y v3-2
  siempre false: ROJO -> detectada por una_pista_muerta_no_frena_a_la_otra [1][2] y con_las_dos_pistas_vivas_la_sesion_sigue
  || -> == (equivalencia): ROJO -> detectada por 4 de las 5 aserciones

Alguna mutacion sobrevive: False

== 4. Prueba matematica combinatoria ==
  Funciones de 2 entradas booleanas posibles: 16
  Tabla de 'or': (False, True, True, True)
  Funciones que coinciden en las 4 combinaciones: 1 (debe ser 1: la propia 'or')
```

`encadenamiento_v3_2.py` (compara el veredicto de la versión ANTES —literal, rescatada del
`.bak`— contra la versión DESPUÉS —derivada— para 4 variantes de `pistas_a_grabar`, incluida la
que el propio comentario del fix cita como la que detectaría):

```
-- condicion de System invertida --
   ANTES   : FALLA (assert_eq! fallo: ['Mic', 'System'] != ['Mic'])
   DESPUES : FALLA (assert_eq! fallo: ['Mic', 'System'] != ['Mic'])
   Mismo veredicto (PASA/FALLA): True

-- siempre incluye System --
   ANTES   : FALLA (assert_eq! fallo: ['Mic', 'System'] != ['Mic'])
   DESPUES : FALLA (assert_eq! fallo: ['Mic', 'System'] != ['Mic'])
   Mismo veredicto (PASA/FALLA): True

Existe alguna mutacion donde ANTES y DESPUES difieran en su veredicto: False
```

## 4. Pruebas que no protegen

Ninguna de las dos pruebas de este encargo está en el caso extremo de "sigue en verde con el
código roto" — ambas se ponen en rojo ante las mutaciones plausibles. Pero v3-2 no protege nada
que no protegieran ya otras dos pruebas, y eso es precisamente lo que el hallazgo original (v3-2,
tercera vuelta de HU-01) decía, y lo que el "arreglo" declaraba haber resuelto. No lo resolvió:

- Recuperé la versión anterior al fix desde `sincronia.rs.bak` (scratchpad, ver §2). El único
  cambio real es: `let sistema_vivo = false;` (literal) a `let sistema_vivo =
  pistas_al_arrancar.contains(&Track::System);` (derivado). La línea `assert_eq!(pistas_al_arrancar,
  vec![Track::Mic])` (447) no cambió: ya estaba antes del fix, palabra por palabra.
- Esa `assert_eq!` compara el vector entero por igualdad exacta. Si pasa, `pistas_al_arrancar` es
  literalmente `[Track::Mic]`, así que `.contains(&Track::System)` no puede dar otra cosa que
  `false` — no hay ninguna mutación plausible de `pistas_a_grabar` que haga pasar el `assert_eq!`
  y a la vez cambiar el resultado de `.contains()`. Lo confirmé con 4 variantes de
  `pistas_a_grabar` (incluida la que el comentario del fix cita textualmente: "empezara a incluir
  `System` por error"): en las 4, el veredicto de la prueba —roja o verde— es idéntico con la
  derivación y con el literal viejo.
- Conclusión: el comentario nuevo (`sincronia.rs:449-454`) es cierto en lo literal —la prueba sí
  se pondría en rojo si `pistas_a_grabar(true, false)` incluyera `System`— pero atribuye esa
  detección a la línea equivocada. La detecta la `assert_eq!` de la 447, que ya estaba antes del
  fix y que además es idéntica, llamada por llamada y aserción por aserción, a la de
  `sin_dispositivo_de_salida_se_graba_solo_el_microfono` (línea 391-394):
  `pistas_a_grabar(true, false).unwrap()` seguido de `assert_eq!(_, vec![Track::Mic])`. Y la
  aserción final, `assert!(!alguna_pista_sigue_viva(false, false))`, es la misma llamada —con los
  mismos dos argumentos— que hace `sin_ninguna_pista_viva_el_bucle_termina` (línea 434). Medida por
  mutación, `una_sesion_de_una_sola_pista_termina_si_esa_pista_muere` sigue sin aportar, tras el
  fix, ninguna detección que esas dos pruebas no den ya por separado. Es exactamente lo que decía
  el hallazgo original; el fix cambió dónde vive la redundancia, no si existe.
- Severidad: no la subo a "Importante" porque no es una prueba que quede en verde con código roto
  (si se rompe, sí se entera, vía las otras dos pruebas y también vía esta). La dejo como lo que
  es: una corrección que no logra lo que su propio comentario afirma, y que por tanto no debería
  figurar como "Corregido" en el sentido de aportar protección nueva — ver §6.

## 5. Criterios de aceptación de `docs/06` sin prueba que los cubra

Acotado al código que toca este encargo (`alguna_pista_sigue_viva` / `pistas_a_grabar`, HU-01 AC
7: "Existe al menos un test que no necesite tarjeta de sonido... manejo del cambio de dispositivo
por defecto"):

- El sub-criterio "cambio de dispositivo por defecto" sigue cubierto solo por analogía: estas
  pruebas modelan "una pista muere y la otra sigue" / "la única pista muere y la sesión termina",
  no "Windows cambia el dispositivo de salida por defecto y hay que reabrir sobre el nuevo". Es la
  misma interpretación que ya aceptaron el revisor-codigo y el orquestador en rondas anteriores
  (comentario de `una_pista_muerta_no_frena_a_la_otra`, línea 411-413); no es un hallazgo nuevo, lo
  dejo señalado porque toca justo la función que revisé.
- No encontré ningún AC de HU-01 nuevo sin cobertura a partir de este diff concreto: v3-1 y v3-2
  son correcciones acotadas a pruebas existentes, no añaden superficie de producción.

## 6. Premisas que cuestiono

**"El fix de v3-2 cierra el hallazgo"** (REVIEW.md, tercera vuelta: "v3-2 ... Corregido").
Cuestionada y refutada por simulación: el defecto que describía el hallazgo original —"medido por
mutación no aportaba ninguna detección que no dieran ya las otras dos pruebas"— sigue siendo
cierto después del fix, tal como muestro en §3 y §4. El diff es real (no es una réplica, llama a
código de producción real, como decía ya el hallazgo original), pero no cambia qué mutaciones
detecta la prueba. Conclusión: la fila de la tabla de la tercera vuelta debería decir algo más
preciso que "Corregido" — el diff se aplicó, la protección no cambió. No es algo que yo arregle;
lo dejo consignado para quien cierre el tablero.

**Que "el orquestador pierde el derecho a revisar su propia corrección, así que queda pendiente de
que `qa` la valide" sea suficiente salvaguarda contra el sesgo de autor.** La regla es correcta en
su diseño, pero noto que el propio `REVIEW.md` ya incluye una autovalidación de hecho antes de que
llegara nadie más a mirar ("Verificado por él: las cuatro combinaciones quedan cubiertas... y
`sincronia.rs` queda con 14 pruebas") — cifras que confirmé exactas (recuento de `#[test]` = 14,
ninguna línea pasa de 99 columnas), pero que no incluían ningún análisis de mutación de v3-2, solo
el recuento de v3-1. Es exactamente el punto ciego que este encargo pedía comprobar, y es
exactamente donde apareció el hallazgo de la §4: contar combinaciones y líneas es necesario pero
no basta para saber si una prueba protege algo.

**Que el caso booleano de 2 entradas hace que Python sea concluyente.** No la cuestiono, la
comparto y la demostré por combinatoria (16 funciones posibles, 1 sola coincide con `||` en las 4
filas), no solo por la lista de mutaciones que se me ocurrió probar. Pero la acoto: es concluyente
sobre qué mutaciones de comportamiento detecta la suite, no sobre si el archivo compila. Ver §7.

## 7. Qué verifiqué y no marqué

- **Que el crate compile.** No lo sé. Sin `rustc` ni `cargo` no hay forma de confirmarlo hoy;
  ningún resultado de este informe implica que `sincronia.rs` compile, solo que su lógica —tal
  como está escrita, leída línea por línea— se comporta como describo si compilara sin cambios de
  significado.
- **El estilo de `una_pista_muerta_no_frena_a_la_otra` (línea 408-416): dos `assert!`
  independientes en el mismo `#[test]`.** Si el primero falla, el segundo no llega a ejecutarse en
  una corrida real de `cargo test` (a diferencia de mi simulación en Python, que evalúa las dos
  ramas por separado). Lo comprobé y no cambia ningún veredicto rojo/verde de la tabla de §3 —para
  toda mutación que hace fallar la primera mitad, la prueba ya da rojo ahí mismo—, así que no lo
  marco como hallazgo de protección. Es un asunto de estilo (¿por qué no dos `#[test]` separados?),
  fuera de mi encargo.
- **Cobertura propia de `pistas_a_grabar`.** Solo tiene pruebas para `(true, false)` y `(false,
  false)`; no hay ninguna para `(false, true)` ni `(true, true)`. No lo marco como hallazgo de esta
  ronda porque esas dos combinaciones no se tocaron en v3-1/v3-2 ni están en el alcance que me
  dieron — pero lo dejo anotado porque pasé por esa función de todos modos.
- **El único call site de producción de `alguna_pista_sigue_viva`** (`wasapi_src.rs:281`, según el
  propio `REVIEW.md`). No lo reabrí: no cambió en este diff, y ya lo verificó el revisor-codigo en
  el cierre de v2-3 de la tercera vuelta. Mi encargo era las pruebas, no el punto de llamada.
- **`core/providers/` no lo toqué ni lo leí** más allá de verlo listado en `git status` — HU-05
  está en revisión ahí, tal como se me indicó.
- **Los otros dos ítems de T-15** (README.md, `Ventana.esEscritorio`) no los revisé: son de
  revisor-codigo según el propio `LEEME.md` de esta tarea; mi encargo es el ítem (c).
- **`git status --short` antes y después de este informe es idéntico** al que tenía al empezar
  (mismos archivos listados); no edité `sincronia.rs` en el árbol real ni ningún otro archivo del
  repositorio — solo escribí en el scratchpad y en este `REVIEW-pruebas.md`.
