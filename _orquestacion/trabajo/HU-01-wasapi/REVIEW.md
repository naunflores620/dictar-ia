# REVIEW — HU-01 «Grabar en Windows (WASAPI loopback)»

Consolidado por el orquestador. Ninguno de los tres revisores es quien implementó.

Reportes de origen, en esta misma carpeta: `REVIEW-codigo.md`, `REVIEW-pruebas.md`,
`REVIEW-plataforma.md`. No se resumen ni se suavizan aquí: se referencian y se decide.

## 1. Hallazgos consolidados

| # | Hallazgo | Origen | Severidad | Estado |
|---|---|---|---|---|
| 1 | `wasapi_src.rs:219-225` — el bucle usa `?` sobre `procesar_paquetes` para **las dos pistas dentro de un único bucle compartido**. Un error COM transitorio en una sola pista termina la sesión entera **sin llamar a `Remuestreador::vaciar()` ni a `Stop()`**: ese bloque de limpieza (235-258) solo es alcanzable por la vía normal de parada. Se pierde el resto de la grabación en ambas pistas | `revisor-codigo` | **Bloqueante** | Vuelve a implementación |
| 2 | `sincronia.rs:76-79` — la fórmula de relleno no descuenta las muestras del propio paquete antes de comparar con lo ya emitido, así que **cuenta dos veces el paquete que reanuda tras un silencio**: 1600 muestras (100 ms) de exceso en **cada** transición silencio→audio del loopback, no una sola vez | `revisor-codigo` | **Bloqueante** | Vuelve a implementación |
| 3 | `sincronia.rs:307-317` — `dos_horas_de_muestras_no_desbordan_el_contador` **fallaría con código correcto**. El ida y vuelta ms→muestras→ms trunca porque `u32::MAX` no es múltiplo de 16: el valor correcto es `31_985`, no `32_000` | `verificador-pruebas` | **Bloqueante** | Vuelve a implementación |
| 4 | `sincronia.rs:103-121` — el helper `Plataforma`/`tiene_backend` es una **réplica escrita a mano**, desconectada de los `#[cfg(target_os)]` reales de `lib.rs:166-200`. `fuera_de_windows_y_linux_sigue_devolviendo_no_soportada` seguiría en verde con el `cfg` real roto, incluido el caso de confundir Android con «no-Linux». El enum no se usa en ningún otro sitio | `auditor-plataforma` (H1) y `verificador-pruebas` (4), por separado | **Importante** | Vuelve a implementación |
| 5 | `un_silencio_en_loopback_no_adelanta_lo_que_viene_despues` rellena con muestras de valor `0.0`, **indistinguibles del silencio insertado**: comprueba la longitud del hueco, no que el silencio vaya antes del audio. Y codifica como esperado el resultado erróneo del hallazgo 2 | `verificador-pruebas` (3) y `revisor-codigo` (2) | **Importante** | Vuelve a implementación |
| 6 | `el_relleno_no_se_calcula_a_partir_de_las_muestras_recibidas` **no puede** blindar la decisión de diseño del reloj: esa decisión vive en `wasapi_src.rs`, que no tiene ninguna prueba. La función que prueba es pura y agnóstica del origen del timestamp | `verificador-pruebas` (2) | **Importante** | Vuelve a implementación |
| 7 | `wasapi_src.rs:418-447` — `dispositivos()` devuelve solo el par por defecto. **Incumple el AC 4** de la HU, que pide enumerar los dispositivos reales del sistema. El implementador lo declaró como desviación | `revisor-codigo` | **Importante** | Vuelve a implementación |
| 8 | `wasapi_src.rs:455-457` — fuga de un `PWSTR` reservado por COM si `to_string()` falla en el camino de error de `id_de_endpoint_por_defecto` | `revisor-codigo` | **Importante** | Vuelve a implementación |
| 9 | Ninguna de las 9 pruebas ejercita `esperadas < muestras_ya_emitidas`: quitar el `saturating_sub` de `sincronia.rs:78` no lo detectaría nadie | `verificador-pruebas` (5) | **Menor** | Se corrige en la vuelta |
| 10 | `Cargo.lock` no incluye la arista `dictar-audio → windows` | `auditor-plataforma` (H2) | **Nota** | Se regenera cuando haya toolchain |
| 11 | El comentario de cabecera de `release.yml:12-18` queda falso al cerrar la HU | `auditor-plataforma` (H3) | **Nota** | Deuda de cierre, no del implementador. Ya prevista en el PLAN |

## 2. Desacuerdos

**Con el implementador, sobre el hallazgo 2.** Su `HANDOFF` lo caracteriza como «una posible
imprecisión menor en el relleno de silencio del primer paquete de cada pista». El
`revisor-codigo` reconstruyó la aritmética con los valores exactos del test y demostró que
**recurre en cada transición silencio→audio**, no una sola vez al principio. Decide el
orquestador: vale la reconstrucción aritmética, no la caracterización. Es Bloqueante — desalinea
el audio de forma acumulativa, que es justo lo que la decisión de diseño del plan existía para
evitar.

**Con el implementador, sobre el hallazgo 3.** El `HANDOFF` afirma haber verificado la
aritmética a mano sin errores. El `verificador-pruebas` la reprodujo con Python y le salió otra
cifra; el orquestador la reprodujo por tercera vía y confirma `31_985`. La afirmación del
`HANDOFF` es falsa.

**Entre revisores no hubo contradicciones.** Al contrario: el `auditor-plataforma` y el
`verificador-pruebas` llegaron por separado al mismo hallazgo 4, y el `revisor-codigo` y el
`verificador-pruebas` al mismo 5 desde ángulos distintos. Que dos revisores independientes
converjan es señal de que el hallazgo es real, no de que sobre uno de los dos.

## 3. Qué quedó sin verificar

**Todo lo que depende de compilar.** Los tres confirmaron B-1 de primera mano, cada uno por su
cuenta, incluido el WSL `Ubuntu-26.04`. En concreto:

- Las 483 líneas de `wasapi_src.rs` tienen **cobertura de ejecución cero** y están escritas
  contra una API que nadie pudo abrir. Los 33 bloques `unsafe` se revisaron por lectura: el
  emparejamiento `CoInitializeEx`/`CoUninitialize` está bien en todos los caminos, incluido el
  de pánico, pero las firmas de la API de `windows 0.58` **no están verificadas**.
- Los AC 1, 2, 5 y 6 de la HU siguen siendo no comprobables sin un Windows con tarjeta de sonido.

Lo que sí se verificó y está conforme: las tres ramas de `iniciar()`/`dispositivos()` tienen
firmas idénticas; `windows = "0.58"` está bajo `[target.'cfg(windows)'.dependencies]` sin
arrastrar las entradas siguientes (parseado con `tomllib`, no leído); el YAML del CI es válido;
**las 9 pruebas de `sincronia.rs` se ejecutan en los dos jobs**, así que la desviación del
`cargo test -p dictar-audio` resulta inocua; el cruce a Android no se ve afectado; se reutiliza
`mezcla.rs` en vez de reimplementarlo; los contadores son `u64`; y el ancho máximo de línea real
es 99 caracteres.

## 4. Checklist de cierre

- [ ] `cargo fmt --all -- --check` — **no ejecutable** (B-1)
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` — **no ejecutable** (B-1)
- [ ] `cargo test --workspace` — **no ejecutable** (B-1)
- [ ] `flutter analyze` / `flutter test` — no aplica, esta HU no toca Flutter
- [ ] Cada AC tiene su prueba, nombrada — **no**: el AC 4 no se cumple (hallazgo 7), y los AC 3
      y 2 tienen pruebas que no protegen (hallazgos 4, 5, 6)
- [ ] Cada prueba nueva se puso en rojo al mutar — **no ejecutable** (B-1); tres de ellas no
      podrían ponerse en rojo ni con toolchain
- [x] Los nombres de prueba describen el fallo que previenen, en español
- [ ] Invariantes 1, 2 y 3 — el 1 **se rompe** por el hallazgo 1: se pierde audio ya escrito
- [x] `auditor-plataforma` lo cotejó, con archivo y línea
- [x] Documentación afectada actualizada en el mismo cambio

## Veredicto

**VUELVE A IMPLEMENTACIÓN.**

Tres hallazgos Bloqueantes, y ninguno depende de que haya compilador: se ven leyendo. El 1
pierde audio, que es lo único que este proyecto declara irreparable. El 2 desalinea la pista de
sistema de forma acumulativa, justo lo que la decisión de diseño del plan existía para evitar. El
3 es un test que fallaría con código correcto y que habría hecho perder tiempo persiguiendo un
bug inexistente el día que haya toolchain.

Lo que **no** hay que rehacer: la arquitectura es correcta. La separación `sincronia.rs` /
`wasapi_src.rs` cumple su propósito —las pruebas se ejecutan en los dos jobs—, el contrato
multiplataforma está bien construido, y la decisión del reloj está bien entendida. Lo que falla
es la ejecución en cuatro puntos concretos y el hecho de que las pruebas no protegen sus
criterios.

Para la vuelta, por orden: hallazgos 1, 2 y 3; después 4, 5 y 6, que son la misma familia —
pruebas que no ejercitan el código de producción, la «regla de la réplica» de `protocolo.md`;
después 7, 8 y 9.

Orquestador · 2026-09-03

---

# Segunda vuelta — 2026-09-03

Los tres revisores volvieron a pasar, acotados a los deltas. Sus informes tienen ahora una
sección «Segunda vuelta» cada uno.

## Lo que quedó cerrado

Los **nueve hallazgos** de la primera vuelta están corregidos, y esta vez con evidencia propia
de cada revisor, no citando el `HANDOFF`:

- **Bloqueante 1**: `cerrar_pista` corre en todos los caminos de salida del fallo por pista;
  confirmado recorriendo `bucle()` entera. Cada pista muere sola y la otra sigue.
- **Bloqueantes 2 y 3**: la aritmética se verificó con Python por partida doble
  (`revisor-codigo` y `verificador-pruebas`, por separado). `muestras_de_relleno` con el
  parámetro nuevo da el resultado correcto, y el test de las dos horas da `32_000` exacto. La
  base nueva no es un tanteo: `2³²`, `1 000 000` y `32 000` son los tres múltiplos de 16.
- **Hallazgo 4, la réplica**: `enum Plataforma`/`fn tiene_backend` borrados sin rastro. Los dos
  tests nuevos de `lib.rs` llaman a `iniciar()`/`dispositivos()` reales, y el
  `auditor-plataforma` comparó sus `#[cfg]` **carácter por carácter** contra las ramas reales:
  son idénticos, y Android cae donde debe.
- **Hallazgos 5 y 6**: el test del silencio detecta ahora la mutación de orden (antes, con
  `0.0`, no); el test renombrado dice la verdad sobre qué protege y qué no.
- **Hallazgos 7, 8 y 9**: `enumerar_flujo` enumera de verdad, libera recursos y maneja el caso
  de cero dispositivos; la fuga de `PWSTR` está corregida y el mismo cuidado se aplicó al código
  nuevo; el caso del `saturating_sub` tiene prueba.

**Dos decisiones del implementador, aceptadas.** La ampliación del aislamiento a los `Start()`
—que se salía del hallazgo literal y él declaró— es correcta; el `revisor-codigo` razonó el
patrón de préstamo. Y los nombres genéricos de dispositivo cumplen la letra del AC 4; la
ambigüedad sobre su espíritu queda declarada, y el selector de dispositivo en la interfaz ya
estaba fuera del alcance de esta HU.

## Hallazgos nuevos, todos en el código que cierra el Bloqueante 1

| # | Hallazgo | Origen | Severidad | Estado |
|---|---|---|---|---|
| v2-1 | `wasapi_src.rs:232-279` — si mueren **las dos** pistas, el bucle no tiene ningún `break`: el hilo queda vivo indefinidamente, sin producir frames y **sin cerrar el canal**. La sesión aparenta seguir grabando y no graba nada, y el contrato de `iniciar()` no tiene forma de comunicarlo. Una clase de dos horas que pierda las dos pistas en el minuto diez se queda en diez minutos, en silencio | `revisor-codigo` (v2-I2) | **Bloqueante** (elevado; ver §2) | Tercera vuelta |
| v2-2 | `wasapi_src.rs:307-314` — `cerrar_pista` descarta en silencio el error de `vaciar()`, mientras que `procesar_paquetes` sí lo registra. Invariante 5: un fallo no se traga | `revisor-codigo` (v2-I1) | **Importante** | Tercera vuelta |
| v2-3 | El comportamiento que corrige el Bloqueante 1 —una pista muere, la otra sigue— **no tiene ninguna prueba**, a diferencia de `pistas_a_grabar`, que sí se extrajo a lógica pura testeable | `revisor-codigo` (v2-I3) | **Importante** | Tercera vuelta |
| v2-4 | El job `android` de `release.yml` **sí compila** el código de producción de la rama `NoSoportada` vía `cargo-ndk`; lo que no se ejecuta nunca es el *test* | `auditor-plataforma` (H6) | **Nota** | Acota T-13 en el tablero |

## Desacuerdos

**Con el `revisor-codigo`, sobre v2-1.** Él lo clasifica como Importante; el orquestador lo
eleva a **Bloqueante**. Un hilo que sigue vivo sin producir nada y sin cerrar el canal deja la
sesión aparentando grabar mientras no graba: es pérdida de audio de facto, y además silenciosa,
que es la combinación exacta que este proyecto declara inaceptable. Que el audio anterior ya
esté en disco no lo salva — lo que se pierde es todo lo que venía después.

**Con mi propio tablero.** Escribí en T-13 que la protección del invariante 4 «es cero». El
`auditor-plataforma` lo corrigió: el código de producción de esa rama sí se compila en el job de
Android, así que solo falta la ejecución del test. Corregido en el tablero.

## Veredicto de la segunda vuelta

**VUELVE A IMPLEMENTACIÓN**, tercera vuelta, acotada a v2-1, v2-2 y v2-3.

Los nueve hallazgos originales están cerrados y la calidad subió de forma medible: esta vez las
cuentas se verificaron ejecutando, no afirmando. Los tres hallazgos nuevos están todos en el
código que se escribió para cerrar el Bloqueante 1 — que es lo normal cuando se arregla un
camino de error: aparecen los caminos de error del arreglo.

v2-3 es el que más valor tiene a largo plazo: la política «una pista muere, la otra sigue» tiene
que salir del bucle COM y vivir en `sincronia.rs`, donde sí se puede probar. Es el mismo
movimiento que ya se hizo con `pistas_a_grabar`, y la arquitectura de esta HU está pensada
justo para eso.

Orquestador · 2026-09-03

---

# Tercera vuelta — 2026-09-03

Dos revisores, según la tabla de proporcionalidad de `agentes.md`: una corrección con su prueba
de regresión no lleva `auditor-plataforma`, y en esta vuelta no se tocó ningún `#[cfg]`,
`Cargo.toml` ni workflow.

## Los tres hallazgos de la segunda vuelta, cerrados

- **v2-1 (Bloqueante).** El `revisor-codigo` no se limitó a confirmar que hay un `break`: recorrió
  las **cuatro** reasignaciones de `mic`/`sistema` a `None` (líneas 210, 216, 257, 268) contra los
  **cuatro** sitios donde se llama a `cerrar_pista` (256, 267, 302, 305), y demostró que no hay un
  quinto camino. Los dos que no pasan por `cerrar_pista` nunca capturaron audio; los dos que sí
  capturaron pasan por él dentro del mismo brazo del `match`, antes de la reasignación. Y remató
  con el dato que lo cierra: `procesar_paquetes` agota `GetNextPacketSize()` hasta cero en cada
  llamada, así que ninguna pista viva puede tener paquetes sin leer en el momento del chequeo.
  La hipótesis del orquestador sobre el `Sender` la verificó él por su cuenta, no la citó.
- **v2-2.** `cerrar_pista` comparado brazo por brazo contra `procesar_paquetes:385-391`: los brazos
  `Ok(Some(_))`/`Ok(None)` no cambiaron; solo `Err(_)` pasó de descartarse a registrarse.
- **v2-3.** `alguna_pista_sigue_viva` tiene una sola definición, un solo call site de producción
  (`wasapi_src.rs:281`) y ninguna decisión duplicada en el bucle. El único otro punto de salida es
  `rx_parar`, que es otra política.

## Hallazgos nuevos, y su corrección

| # | Hallazgo | Origen | Severidad | Estado |
|---|---|---|---|---|
| v3-1 | Las tres pruebas de `alguna_pista_sigue_viva` cubrían **3 de las 4** combinaciones: faltaba `(true, true)`, que es el estado normal de una grabación sana. La tabulación exhaustiva encontró una mutación concreta que **sobrevivía en verde**: `\|\|` → `!=`. Ambos operadores coinciden en las tres combinaciones probadas y solo difieren en la que faltaba, así que el bucle habría hecho `break` de inmediato justo cuando todo va bien | `verificador-pruebas` | **Importante** | **Corregido** |
| v3-2 | `una_sesion_de_una_sola_pista_termina_si_esa_pista_muere` no encadenaba de verdad `pistas_a_grabar` con `alguna_pista_sigue_viva`, pese a que el `HANDOFF` lo describía así: los dos booleanos eran literales escritos a mano. No es una réplica en sentido estricto —ambas mitades llaman a código real—, pero medido por mutación no aportaba ninguna detección que no dieran ya las otras dos pruebas | `verificador-pruebas` | **Menor** | **Corregido** |

**Los corrigió el orquestador**, por proporcionalidad: v3-1 era añadir una prueba y v3-2 derivar
un booleano en vez de escribirlo. Lanzar una cuarta vuelta con su implementador para dos ediciones
de esa talla habría sido teatro. Consecuencia según `protocolo.md`: **el orquestador pierde el
derecho a revisar esa corrección**, y queda pendiente de que `qa` la valide al cerrar el tablero.
Verificado por él: las cuatro combinaciones quedan cubiertas, ninguna línea pasa de 100 columnas,
y `sincronia.rs` queda con 14 pruebas.

## Qué quedó sin verificar

Lo de siempre, y sin novedad: **nada se ha compilado** (B-1, reconfirmado de primera mano por los
dos revisores). Dato nuevo del `verificador-pruebas`: la VM `Ubuntu-26.04` de WSL ya ni arranca
(`HCS_E_CONNECTION_TIMEOUT`), así que esa vía tampoco está disponible.

Además, dos límites declarados y aceptados, no ocultos:

- Las pruebas cubren la **política**, no el **bucle**. Que la política sea correcta no garantiza
  que el bucle la consulte con los argumentos correctos, ni que el `break` se ejecute, ni que el
  canal se cierre. `wasapi_src.rs` es Windows-only y no se puede probar sin hardware.
- Las 483 líneas de COM siguen con cobertura de ejecución cero.
- `sincronia.rs` nunca estuvo versionado, así que `git diff` no tiene base contra la que comparar:
  la comprobación de que las pruebas previas siguen intactas se hizo por lectura comparada contra
  la sección anterior del propio informe, no por diff real. El `verificador-pruebas` lo declaró en
  vez de disimularlo.

## Veredicto

**APROBADA, CIERRE CONDICIONADO.**

Condicionado a B-1: no se cierra del todo hasta que `cargo fmt`, `clippy` y `test` pasen en
verde, y hasta que alguien la ejecute en un Windows con tarjeta de sonido. Los AC 1, 2, 5 y 6 de
la HU siguen siendo no comprobables aquí.

Lo que sí se puede afirmar: tras tres vueltas, **no queda ningún hallazgo Bloqueante ni Importante
abierto**. Los tres bloqueantes de la primera vuelta —pérdida de audio, relleno duplicado, y un
test que fallaba con código correcto— están cerrados con evidencia; los tres que introdujo el
arreglo del primero, también; y el último Importante se corrigió en el acto.

Deuda que sale de esta HU al tablero: T-8, T-9, T-13 y T-14.

Orquestador · 2026-09-03
