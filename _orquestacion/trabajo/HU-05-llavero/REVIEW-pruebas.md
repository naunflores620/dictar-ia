# REVIEW-pruebas — HU-05 «Claves de API en el llavero del SO»

## 1. Veredicto

**NO VERIFICABLE por B-1** — no hay `cargo`/`rustc` en esta máquina, ni en Windows ni en la
única distribución WSL con shell (`Ubuntu-26.04`). No pude ejecutar ni una sola prueba.

Del análisis estático: las **6 pruebas de la tabla del plan están presentes** con el nombre
exacto, y las **5 preexistentes «adaptadas»** conservan su fuerza (nada se debilitó al pasar de
`PathBuf` a `Origen`). Pero encontré **dos hallazgos Importantes** que ninguna ejecución hacía
falta para ver, leyendo el código con cuidado:

- `el_llavero_va_despues_del_entorno_y_del_env` nunca llama a `resolver_por_defecto()` — la
  función que el propio plan señala como el sitio a mutar (líneas 397-401) —, así que la
  mutación que el plan da por buena («reordenar la cadena para poner el llavero primero») **no
  la pondría en rojo**: la prueba arma su propia cadena, ya en el orden correcto, con
  `MapResolver` en los tres eslabones.
- `ningun_evento_de_tracing_contiene_el_valor_de_la_clave` solo tiene un evento de `tracing` que
  capturar en todo el archivo: el `warn!` del *branch* de error de `escribir_en_llavero`
  (línea 313-315). Si el llavero escribe con éxito —Windows con Credential Manager, un GNOME de
  escritorio, o cualquier entorno donde `set_password` no falle—, **no se emite ni un solo
  evento** y la prueba pasa con el bucle vacío. Es exactamente el patrón «afirma algo que el
  vacío ya cumple». En el CI real (`ubuntu-24.04`, sin sesión D-Bus ni `gnome-keyring-daemon`)
  esto no se nota porque la escritura siempre falla ahí — pero eso es un accidente del entorno de
  CI, no una garantía del diseño de la prueba.

Además: **`resolver_por_defecto()` y `guardar_clave()` —los dos puntos de entrada reales que usa
la aplicación (`core/api/src/lib.rs`, `cli/src/main.rs`)— no los llama ninguna prueba**, ni antes
ni después de esta HU. Se prueban sus piezas por separado, nunca la composición.

Los criterios 1-5 de `docs/06-historias-de-usuario.md#hu-05` están todos representados por al
menos una prueba, pero el AC 1 (que exista un resolver que **lea** de verdad del llavero) no
tiene ninguna prueba de éxito — el HANDOFF lo declara, y lo confirmo: es un hueco real, aunque
razonable dado el entorno.

## 2. Qué pude ejecutar y qué no

```
$ command -v cargo rustc flutter
/c/dev/flutter/bin/flutter

$ cargo --version
/usr/bin/bash: line 1: cargo: command not found

$ where cargo
INFORMACIÓN: no se pudo encontrar ningún archivo para los patrones dados.

$ where rustc
INFORMACIÓN: no se pudo encontrar ningún archivo para los patrones dados.

$ wsl -l -v
    NAME                    STATE           VERSION
  * docker-desktop          Stopped         2
    Ubuntu-26.04            Stopped         2

$ wsl -d Ubuntu-26.04 -- bash -lc "command -v cargo; cargo --version"
bash: line 1: cargo: command not found
```

No hay `cargo` ni `rustc` en Windows ni dentro de la distribución WSL con shell
(`docker-desktop` no tiene una). Confirmado el bloqueo **B-1**. No corrí `cargo test`, no
apliqué ninguna mutación al árbol (aplicar una mutación sin poder compilarla no genera ninguna
señal), y no toqué `core/providers/src/secretos.rs`. Hice copia de seguridad íntegra en el
scratchpad (`...\scratchpad\secretos.rs.bak`, verificada byte a byte contra el original con
`diff`) por si hiciera falta mutar, pero no fue necesario tocar nada porque no había forma de
leer una señal de verde/rojo.

Lo que sí hice: lectura completa de `core/providers/src/secretos.rs` (923 líneas), `git diff`
línea por línea de los cuatro archivos tocados, y trazado manual de qué código ejecuta cada
prueba, para razonar el resultado de cada mutación propuesta sin poder correrla.

## 3. Tabla de mutaciones

Las seis pruebas nuevas de la tabla del plan, más las cinco preexistentes adaptadas. Todas
marcadas `no ejecutado (B-1)` porque no hay forma de compilar; la columna «Análisis estático»
da mi conclusión razonada, que en dos filas contradice lo que el plan esperaba (ver sección 4).

| Prueba | Archivo:línea mutado | Mutación | Resultado | Análisis estático |
|---|---|---|---|---|
| `el_llavero_va_despues_del_entorno_y_del_env` | `secretos.rs:397-401` (`resolver_por_defecto`) | Poner `.con(Origen::Llavero, ...)` primero en la cadena, antes de `Origen::Entorno` | no ejecutado (B-1) | **No la detectaría.** La prueba nunca llama a `resolver_por_defecto()` — arma su propia `CadenaResolvers` ya en orden correcto con `MapResolver`. Ver hallazgo 4.1 |
| `el_resolutor_real_del_llavero_no_entra_en_panico_sin_sesion` | `secretos.rs:269-270` (`LlaveroResolver::resolver`, rama desktop) | Cambiar `.get_password().ok()` por `.get_password().unwrap()` | no ejecutado (B-1) | La detectaría: sin llavero (CI) o con una referencia inexistente en un llavero real, `get_password()` devuelve `Err`, y `.unwrap()` entra en pánico -> la prueba falla por pánico en los dos entornos |
| `un_error_del_llavero_se_convierte_en_none` | `secretos.rs:267-268` (`LlaveroResolver::resolver`, rama desktop) | Cambiar `.ok()?` de `keyring::Entry::new(...)` por `.unwrap()` | no ejecutado (B-1) | La detectaría si `Entry::new` llega a fallar con la referencia de 5000 caracteres (plausible, aunque no verificado empíricamente ni por el implementador ni por mí — ver «Premisas que cuestiono»); si no falla ahí, el fallo real ocurre en `get_password()`, no cubierto por esta mutación concreta. La mutación del plan («propagar con `?`») no tiene un punto literal en el código: la función devuelve `Option`, no `Result`, así que no hay un `?` que mutar tal cual |
| `guardar_en_el_llavero_informa_de_su_origen_no_de_una_ruta` | `secretos.rs:420` (`resultado_de_guardar`) | Cambiar `Ok(Origen::Llavero)` por `Ok(Origen::Archivo(PathBuf::from("llavero del sistema")))` | no ejecutado (B-1) | La detectaría: `assert!(matches!(origen, Ok(Origen::Llavero)))` es específico a esa variante, no un `matches!` laxo |
| `el_texto_del_error_no_contiene_la_clave` | `secretos.rs:465` (`guardar_clave_en`, antes de `create_dir_all`) | Envolver el error con `.map_err(|e| std::io::Error::other(format!("fallo guardando {}: {e}", valor.unwrap_or(""))))` | no ejecutado (B-1) | La detectaría: el secreto quedaría en `error.to_string()` y el `assert!(!error.to_string().contains(secreto))` fallaría. Nota: no hay ninguna línea *existente* que filtre la clave hoy — esta prueba blinda contra una regresión futura, no contra un comportamiento actual con un punto de fuga real (ver sección 4) |
| `ningun_evento_de_tracing_contiene_el_valor_de_la_clave` | `secretos.rs:309` (`escribir_en_llavero`, brazo `Some(v) => entrada.set_password(v)`) | Añadir `tracing::warn!(valor = %v, "clave guardada en el llavero")` justo tras el `set_password(v)` exitoso | no ejecutado (B-1) | **No la detectaría en el CI real** (`ubuntu-24.04`, sin sesión D-Bus: `set_password` siempre falla ahí, ese brazo nunca se ejecuta). Sí la detectaría en una máquina con llavero funcional (Windows, GNOME de escritorio). Ver hallazgo 4.2 |
| `guardar_una_clave_la_deja_legible_para_el_resolutor` (adaptada) | `secretos.rs:498` (`guardar_clave_en`) | Cambiar `lineas.push(format!("{nombre}={v}"))` por `lineas.push(format!("{nombre}=OTRO-VALOR"))` | no ejecutado (B-1) | La detectaría: el resolutor leería `"OTRO-VALOR"` en vez de `"sk-prueba-123"` |
| `guardar_no_pisa_las_claves_de_otros_proveedores` (adaptada) | `secretos.rs:482` (`guardar_clave_en`) | Cambiar `if clave_linea == Some(nombre.as_str())` por `if true` | no ejecutado (B-1) | La detectaría: la línea de `DEEPSEEK_API_KEY` se trataría como si fuera la que se está sustituyendo y desaparecería al escribir `GEMINI_API_KEY` |
| `guardar_dos_veces_sustituye_en_vez_de_acumular` (adaptada) | `secretos.rs:487` (`guardar_clave_en`) | Quitar `sustituida = true;` | no ejecutado (B-1) | La detectaría: `!sustituida` sería siempre verdadero y la segunda escritura añadiría una línea duplicada en vez de sustituir; `texto.matches("DEEPSEEK_API_KEY").count()` daría 2, no 1 |
| `borrar_una_clave_la_quita_del_archivo` (adaptada) | `secretos.rs:483-487` (`guardar_clave_en`) | Al encontrar la línea a borrar con `valor = None`, empujarla igual (`lineas.push(linea.to_owned())`) en vez de omitirla | no ejecutado (B-1) | La detectaría: `r.resolver("keyring:deepseek").is_none()` fallaría, la clave seguiría en el archivo |
| `el_archivo_de_claves_queda_ilegible_para_los_demas` (adaptada, `cfg(unix)`) | `secretos.rs:509` (`guardar_clave_en`) | Cambiar `0o600` por `0o644` | no ejecutado (B-1) | La detectaría: `assert_eq!(modo, 0o600, ...)` fallaría con `modo = 0o644` |

## 4. Pruebas que no protegen (o protegen menos de lo que el plan asume)

### 4.1 `el_llavero_va_despues_del_entorno_y_del_env` no ejercita `resolver_por_defecto()`

`secretos.rs:730-776`. El nombre y el comentario de la prueba dicen que protege el orden **de la
cadena por defecto de la aplicación** (AC 2: «va el último de la cadena, después del entorno y
del `.env`»). Pero el cuerpo de la prueba construye su propia `CadenaResolvers` desde cero, con
`MapResolver` en los tres eslabones, **ya en el orden correcto**:

```rust
let cadena = CadenaResolvers::nueva()
    .con(Origen::Entorno, Box::new(MapResolver::default()...))
    .con(Origen::Archivo(...), Box::new(MapResolver::default()...))
    .con(Origen::Llavero, Box::new(MapResolver::default()...));
```

En ningún momento llama a `resolver_por_defecto()` (`secretos.rs:390-401`), que es la función
real que usa la aplicación (`core/api/src/lib.rs:130,222,266`; `cli/src/main.rs:159,260`) y que
el propio plan señala como el sitio de la mutación («Reordenar la cadena para poner el llavero
primero»). Si alguien invirtiera el orden dentro de `resolver_por_defecto()` — el bug real que
AC 2 quiere prevenir — **esta prueba seguiría en verde**, porque nunca toca esa función: solo
demuestra que `CadenaResolvers` (el mecanismo genérico) respeta el orden de inserción, algo que
ya cubren `la_cadena_prefiere_el_primer_eslabon` (`secretos.rs:609-623`) y
`la_cadena_sigue_buscando_si_el_primero_no_la_tiene` (`secretos.rs:626-637`) — ninguna de las
dos nueva de esta HU.

Confirmé por `grep` que **ninguna prueba en todo el repositorio llama a `resolver_por_defecto()`
con paréntesis** (solo aparece en `use` y en la propia definición). El criterio de aceptación 2
queda, en la práctica, sin una sola prueba que ejercite el código de producción que lo implementa.

Esto es un hallazgo **Importante**: el plan (revisión 2, sección «Dos precisiones») da por buena
esta prueba explícitamente («eso es composición de cadena, y da igual qué resolutor concreto
ocupe cada eslabón»), pero esa justificación pasa por alto que el propósito de la prueba, según
su propio nombre y el AC que dice cubrir, es comprobar el ensamblado real, no el mecanismo
genérico ya probado.

### 4.2 `ningun_evento_de_tracing_contiene_el_valor_de_la_clave` puede pasar con cero eventos

`secretos.rs:896-922`, apoyada en `CapturaEventos` (`secretos.rs:850-894`).

El subscriptor sí es real (implementa `tracing::Subscriber` a mano, captura los campos vía
`Visit::record_debug` — que en `tracing` 0.1 es el único método obligatorio del trait, y todos
los demás (`record_str`, `record_i64`, …) delegan en él por defecto, así que también capturan los
valores con `%` como el `error = %e` existente). Y está activo durante las tres llamadas, dentro
de `tracing::subscriber::with_default(captura.clone(), || { ... })`. Hasta ahí, bien construida.

El problema es dónde están, en todo el archivo, las líneas que emiten un evento de `tracing`
durante esas tres llamadas:

- `escribir_en_llavero` (`secretos.rs:304-317`): **un único** `tracing::warn!` en el brazo de
  error (`secretos.rs:313-315`, dentro de `if let Err(e) = &resultado`). El brazo de éxito
  (`Some(v) => entrada.set_password(v)`, línea 309) no emite nada.
- `LlaveroResolver::resolver` (`secretos.rs:259-272`): ningún `tracing::` en absoluto, ni en
  éxito ni en error.
- `guardar_clave_en` (`secretos.rs:460-513`): ningún `tracing::` en absoluto.

Es decir: **el único evento que esta prueba puede llegar a capturar, en todo el archivo, es ese
`warn!` de error**, y solo se emite si `escribir_en_llavero` **falla** al escribir. Si la
escritura tiene éxito —Windows con Credential Manager (que el propio HANDOFF, decisión 1, dice
que «está siempre disponible, sin necesitar sesión gráfica ni demonio»), o un escritorio Linux
con `gnome-keyring` corriendo—, `captura.textos()` queda **vacío**, el
`for texto in captura.textos() { assert!(...) }` no itera ni una vez, y la prueba pasa
trivialmente. Es el patrón exacto que se pidió vigilar: «si pasa con cero eventos emitidos, no
protege».

Comprobé además dónde corre esta prueba de verdad: `.github/workflows/ci.yml:23,65` ejecuta
`cargo test --workspace` en `runs-on: ubuntu-24.04`, sin ningún paso que arranque un bus de
sesión D-Bus ni un demonio de secretos (`grep -i "dbus|keyring|secret|gnome"` solo encuentra el
paquete `libsecret-1-dev` en la lista de dependencias del sistema, no un servicio corriendo).
Ahí, `escribir_en_llavero` va a fallar casi con toda seguridad, así que el CI real sí llega a
capturar ese evento de error — pero por un accidente del entorno de CI, no porque la prueba lo
garantice. Con esa mecánica, un desarrollador corriendo `cargo test` en su máquina Windows (el
propio entorno de esta sesión) **no tiene ninguna garantía de que la prueba proteja nada**, y
—más grave todavía— **una fuga de la clave introducida en el brazo de éxito de
`escribir_en_llavero` (línea 309) nunca podría detectarla ni siquiera el CI**, porque ese brazo
jamás se ejecuta en `ubuntu-24.04` sin sesión D-Bus.

Hallazgo **Importante**: la prueba protege el brazo de error de una sola función, y solo en los
entornos donde ese brazo es el que se ejecuta. No protege el brazo de éxito de
`escribir_en_llavero`, ni ninguna línea de `LlaveroResolver::resolver` o `guardar_clave_en`,
porque ninguna de esas rutas emite jamás un evento que capturar.

## 5. Criterios de aceptación de `docs/06` sin prueba que los cubra

De `docs/06-historias-de-usuario.md`, sección HU-05:

- **AC 1** («Existe un `KeyResolver` que lee del llavero: Secret Service en Linux, Credential
  Manager en Windows»): las dos pruebas que el HANDOFF le asigna
  (`el_resolutor_real_del_llavero_no_entra_en_panico_sin_sesion`,
  `un_error_del_llavero_se_convierte_en_none`) **solo ejercitan el camino de fallo** — ninguna
  hace una lectura que tenga éxito y devuelva un valor real. El propio HANDOFF lo declara
  («Escrito, no ejecutado. Ver deuda: no hay una prueba de ida y vuelta»). Lo confirmo: es
  cierto, no hay ninguna prueba de éxito para AC 1. Es un hueco real. Si acepto la razón que da
  el HANDOFF (que forzar un llavero real en la máquina de pruebas no es viable ni en CI ni en un
  test determinista), el hueco es razonable *dado ese diseño concreto* — pero cuestiono la
  premisa de que no había alternativa; ver sección 6.

- **AC 2** («Va el último de la cadena, después del entorno y del `.env`, sin alterar ese
  orden»): sin prueba real, por el hallazgo 4.1. La prueba que existe cubre el mecanismo
  genérico de `CadenaResolvers`, no el ensamblado de `resolver_por_defecto()`.

- **AC 3** («Guardar una clave desde ajustes la escribe en el llavero, no en un archivo»): hay
  prueba para la traducción booleano-a-`Origen` (`resultado_de_guardar`), pero **ninguna prueba
  ejercita `guardar_clave()`** (`secretos.rs:439-446`), la función pública real que usa
  `core/api/src/puente.rs`. Nadie comprueba que `guardar_clave` de verdad llama primero a
  `escribir_en_llavero` antes de caer al `.env` — se prueban las dos piezas por separado, nunca
  la composición. Mismo patrón que el hallazgo 4.1, en la mitad de escritura de la cadena. No es
  un hueco nuevo de esta HU (la función `guardar_clave_en` tampoco se probaba integrada con
  `guardar_clave` antes), pero con el llavero de por medio el riesgo de que la composición esté
  mal es mayor que antes, cuando `guardar_clave` era una línea trivial.

AC 4 y AC 5 sí tienen prueba que ejercita el código real (con los matices de las secciones 3 y 4
para AC 5).

## 6. Premisas que cuestiono

- **«No se puede probar el llavero sin uno real en la máquina» (HANDOFF, «Lo que NO pude
  verificar», último punto).** Es la premisa detrás de por qué no hay una prueba de ida y vuelta
  para AC 1, y también la raíz del hallazgo 4.2. `keyring-core` (la base de `keyring` 4.2.0)
  incorpora, según mi conocimiento de versiones anteriores del crate, un backend de prueba en
  memoria (mock) pensado exactamente para este caso: dependencia inyectable, sin tocar ningún
  almacén real. **No pude confirmarlo** para la versión 4.2.0 exacta — no hay `cargo` ni acceso
  al código fuente descargado en esta sesión para comprobarlo, y no quiero afirmar como hecho
  algo que no verifiqué—, pero si existe (cosa que el HANDOFF tampoco descarta explícitamente:
  solo dice que leyó `Cargo.toml.orig`, `src/v1.rs` y los crates de backend de Windows y Linux,
  no menciona haber buscado un backend de prueba), sería la forma de escribir tanto el
  ida-y-vuelta de AC 1 como una prueba de `tracing` que no dependa de qué sistema operativo la
  ejecuta. **Conclusión: quien retome esta deuda debería comprobar primero si `keyring-core` trae
  un backend de prueba inyectable antes de asumir que la única alternativa es "no se puede
  probar".**

- **«El doble `MapResolver` sirve para `el_llavero_va_despues_del_entorno_y_del_env` porque da
  igual qué resolutor concreto ocupe cada eslabón» (PLAN.md, «Dos precisiones»).** Es cierto para
  probar el mecanismo de `CadenaResolvers`, pero la conclusión que el plan saca de ahí —que esta
  prueba cubre el AC 2— no se sostiene, porque el AC 2 habla del ensamblado real
  (`resolver_por_defecto`), no del mecanismo genérico. **Conclusión: la premisa mezcla "probar
  que el tipo `CadenaResolvers` funciona" con "probar que `resolver_por_defecto` lo usa bien", y
  son dos cosas distintas.** Falta una prueba que llame a `resolver_por_defecto()` de verdad y
  compruebe, aunque sea con variables de entorno controladas y un `.env` en un directorio
  temporal, que el llavero queda al final.

- **«En Windows el Credential Manager está siempre disponible, sin necesitar sesión gráfica ni
  demonio» (HANDOFF, decisión 1).** La doy por cierta —es consistente con lo que se sabe de
  Windows—, pero el HANDOFF la usa solo para justificar por qué `guardar_clave_en` no debía tocar
  el llavero (para no ensuciar un almacén real en cada corrida), y no advierte la implicación
  contraria: que esa misma disponibilidad hace que la prueba de `tracing` quede sin ningún evento
  que inspeccionar en cualquier máquina Windows. **Conclusión: la premisa es correcta, pero su
  consecuencia sobre la prueba de `tracing` no se documentó ni se mitigó.**

## 7. Qué verifiqué y no marqué

- No verifiqué que `keyring = "4.2.0"` compile de verdad con las *features* que declara el
  `Cargo.toml` (B-1). Confío en la lectura del HANDOFF sobre `Cargo.toml.orig` de `crates.io`,
  pero no la repetí yo mismo: no tengo forma confirmada de acceso a Internet desde este entorno
  de verificación ni una copia local de esos manifiestos.
- No verifiqué el valor real de `CRED_MAX_USERNAME_LENGTH` en Windows, así que no puedo confirmar
  que la referencia de 5000 caracteres de `un_error_del_llavero_se_convierte_en_none` falle de
  verdad en el Credential Manager. El propio HANDOFF ya declara esto como no verificado.
- No verifiqué el comportamiento de `zbus`/D-Bus sin sesión (¿falla rápido o espera un
  *timeout*?). Si esperara, dos pruebas (`el_resolutor_real...`, `un_error_del_llavero...`)
  seguirían siendo correctas pero podrían volverse lentas en CI — no pude medirlo.
- No revisé `core/providers/Cargo.toml`, `core/api/src/puente.rs` ni
  `app/lib/datos/repositorio_rust.dart` más allá de leer su `git diff` para entender qué código
  ejercitan las pruebas; su corrección de estilo y arquitectura es tarea de `revisor-codigo`, no
  mía.
- No evalué el caso documentado en HANDOFF, punto 2 de «Decisiones que se apartan del PLAN» (que
  un `NoEntry` al borrar cuenta como fallo, no como éxito): el propio HANDOFF dice que no hay
  prueba dedicada a ese caso y que no amplía el alcance de pruebas del plan. Como no hay prueba
  que evaluar ahí, no la incluyo en la tabla de mutaciones, pero la señalo aquí porque es
  comportamiento sin verificar, tal como el HANDOFF lo deja escrito.
- No toqué `.github/workflows/ci.yml` ni `core/audio-capture` (tarea paralela de HU-01); lo leí
  únicamente para confirmar en qué runner corre `cargo test --workspace` y si había algún paso
  de arranque de D-Bus, dato que necesitaba para el hallazgo 4.2.
- No apliqué ninguna mutación real al árbol de trabajo: sin `cargo`, mutar sin poder compilar no
  produce ninguna señal verde/rojo, así que hacerlo solo hubiera arriesgado el trabajo del
  implementador sin ninguna ganancia. Hice la copia de respaldo en el scratchpad de todos modos,
  como indicaba el protocolo, por si a mitad de la verificación aparecía algún camino para
  ejecutar algo (no apareció).
- `git status --short` al empezar y al terminar mi trabajo es idéntico; no dejé cambios en el
  árbol.

---

## Segunda vuelta (2026-09-03)

Re-verificación acotada a lo que provocó la vuelta: mis dos hallazgos (1 y 2 de esta tabla, = #2
y #3 del `REVIEW.md` consolidado), el hallazgo 5 (composición de `guardar_clave`), las pruebas
nuevas del hallazgo 1 Bloqueante (`purgar_del_env`), los tests adaptados, y una evaluación
expresa de la decisión de no usar el mock de `keyring-core`. Único archivo de código tocado en
esta vuelta: `core/providers/src/secretos.rs` (confirmado leyendo `HANDOFF.md`).

### Veredicto

**NO VERIFICABLE por B-1 (persiste, confirmado de nuevo)** — de mis dos hallazgos: el **2**
(`tracing`) queda **cerrado del todo** en la parte que una prueba automática puede alcanzar; el
**1** (el orden) cierra el **mecanismo** pero no la **función real** — `resolver_por_defecto()`
sigue sin que la llame ninguna prueba, ni antes ni después de esta vuelta, y le quedan **dos**
decisiones propias sin cubrir (el cableado de qué resolutor va a qué posición, y la traducción a
`Origen` de si hubo o no `.env`, esta última preexistente a la HU) — así que **no está cerrado del
todo**. El hallazgo 5 cierra bien: `guardar_clave_orquestada` se prueba como
composición real, no como réplica. De los cuatro casos de borde de `purgar_del_env`, tres tienen
prueba con aserciones reales; el cuarto (fallo al reescribir) no tiene ninguna, tal como el
HANDOFF ya declara. Encontré **un hallazgo nuevo, Importante**: una de las pruebas de
`purgar_del_env` no protege la comprobación que dice proteger — aunque, y esto importa para
calibrar la urgencia, el resultado que ve el usuario queda a salvo por otra prueba (ver §
«Pruebas que no protegen»). La decisión de **no usar el mock de `keyring-core`** se sostiene:
verifiqué las cinco afirmaciones técnicas en que se apoya contra el código fuente real (que el
propio implementador dejó descargado en esta máquina) y las cinco resultaron correctas.

### Qué pude ejecutar y qué no

```
$ command -v cargo rustc flutter
/c/dev/flutter/bin/flutter

$ cargo --version / rustc --version
/usr/bin/bash: line 1: cargo: command not found
/usr/bin/bash: line 1: rustc: command not found

$ where cargo / where rustc / where rustup
INFORMACIÓN: no se pudo encontrar ningún archivo para los patrones dados. (las tres)

$ ls "$USERPROFILE/.cargo/bin"  → no existe
$ wsl -l -v
    NAME                    STATE           VERSION
  * docker-desktop          Stopped         2
    Ubuntu-26.04            Stopped         2

$ wsl -d Ubuntu-26.04 -- bash -lc "command -v cargo; cargo --version; rustc --version"
bash: line 1: cargo: command not found
bash: line 1: rustc: command not found
$ wsl -d Ubuntu-26.04 -- bash -lc "ls -la ~/.cargo/bin"
ls: cannot access '/home/naunflores/.cargo/bin': No such file or directory
```

`docker-desktop` no tiene shell (regla ya conocida). Repetí la comprobación completa que hizo mi
primera pasada más una búsqueda adicional en rutas de instalación habituales
(`~/.cargo/bin`, `rustup`), en Windows y en WSL: mismo resultado, B-1 confirmado sin ambigüedad.
No corrí `cargo test`, no apliqué ninguna mutación al árbol de trabajo (mutar sin poder compilar
no genera ninguna señal), y no toqué `core/providers/src/secretos.rs`.

`D:\dictar_ia\Cargo.lock` (fecha 11 ago, anterior a esta HU) no tiene entradas para `keyring` ni
`keyring-core`: el lockfile nunca se regeneró para esta dependencia, otra consecuencia visible de
B-1, no algo nuevo que reportar.

Lo que sí hice: lectura completa de `core/providers/src/secretos.rs` (1166 líneas, creció desde
las 923 de la primera pasada), `git diff -- core/providers/src/secretos.rs` completo (780 líneas,
acumula las dos vueltas — confirmado: `el_llavero_va_despues_del_entorno_y_del_env` aparece como
línea `+` pura, nunca como `-`/`+` pareados, señal de que el diff corre contra el estado
anterior a la HU-05, no contra la primera vuelta), lectura de `REVIEW.md`, `HANDOFF.md` completo
y trazado manual de cada prueba nueva o tocada contra el código que ejercita.

Además, y esto no estaba disponible en mi primera pasada: encontré que el implementador dejó las
fuentes reales de `keyring-core` 1.0.0, `keyring` 4.2.0, `zbus-secret-service-keyring-store`
1.0.0 y `windows-native-keyring-store` 1.1.0 descargadas en
`C:\Users\naunf\AppData\Local\Temp\{kc-src,keyring-src,zbus-src,wnks11-src}`. Las leí
directamente para verificar, contra el código fuente y no solo contra la prosa del `HANDOFF`,
las afirmaciones técnicas de la sección «Decisión: no usar el mock» (ver más abajo). No las
descargué yo mismo — no tengo acceso a Internet confirmado en esta sesión —, así que confío en
que son genuinas por su huella de `cargo package` (`Cargo.toml.orig`, `.cargo_vcs_info.json`,
licencias, estilo interno consistente con el ecosistema), pero no comparé byte a byte contra
crates.io. Lo dejo anotado en «Qué verifiqué y no marqué».

### Hallazgo 1 (el orden): el mecanismo se cerró, la función real no

Ahora existe `ensamblar_cadena_por_defecto` (`secretos.rs:410-420`), que recibe los tres
resolutores ya construidos y decide únicamente el orden de registro:

```rust
fn ensamblar_cadena_por_defecto(
    entorno: Box<dyn KeyResolver>,
    origen_archivo: Origen,
    archivo: Box<dyn KeyResolver>,
    llavero: Box<dyn KeyResolver>,
) -> CadenaResolvers {
    CadenaResolvers::nueva()
        .con(Origen::Entorno, entorno)
        .con(origen_archivo, archivo)
        .con(Origen::Llavero, llavero)
}
```

Y `el_llavero_va_despues_del_entorno_y_del_env` (`secretos.rs:844-882`) **sí llama a esta
función**, con `MapResolver` distinguibles en cada posición, y comprueba las dos propiedades que
importan: que gana el primero que responde, y que si ni el entorno ni el archivo tienen la clave,
se llega hasta el llavero. Confirmé leyendo el cuerpo de la prueba (no solo el comentario) que
esto es cierto. **Primera parte de la comprobación pedida: sí, el test llama a la función
extraída.**

**Segunda parte: `resolver_por_defecto` sí conserva una decisión propia, y esa parte sigue sin
prueba.** Su cuerpo completo, `secretos.rs:430-443`:

```rust
pub fn resolver_por_defecto() -> CadenaResolvers {
    let dotenv = DotEnvResolver::buscar();
    let origen = dotenv.origen().map(|p| Origen::Archivo(p.to_path_buf())).unwrap_or(Origen::Memoria);

    ensamblar_cadena_por_defecto(
        Box::new(EnvResolver),               // línea 438
        origen,                                // línea 439
        Box::new(dotenv),                      // línea 440 — ¿va en la posición "archivo"?
        Box::new(LlaveroResolver::nuevo()),    // línea 441 — ¿va en la posición "llavero"?
    )
}
```

El HANDOFF dice: «`resolver_por_defecto` no hace nada más que llamarla con las piezas reales — no
le queda ninguna otra línea de lógica propia». Es cierto que no le queda lógica de *orden*: el
orden vive entero en `ensamblar_cadena_por_defecto`. Pero **sí le queda una decisión**: qué objeto
concreto ocupa cada parámetro. Las líneas 438-441 pasan cuatro valores posicionales, tres de ellos
del mismo tipo exacto (`Box<dyn KeyResolver>`) — el compilador no distingue `Box::new(dotenv)` de
`Box::new(LlaveroResolver::nuevo())`, así que un error de cableado ahí (intercambiar cuál va como
`archivo` y cuál como `llavero`) compila sin ningún aviso.

**Hay una segunda decisión propia, distinta del cableado, en la misma función:** qué *valor* de
`Origen` corresponde a la posición «archivo». La línea 435
(`.map(|p| Origen::Archivo(p.to_path_buf())).unwrap_or(Origen::Memoria)`) no la calcula
`ensamblar_cadena_por_defecto` —que solo recibe `origen_archivo` ya resuelto como parámetro y lo
reenvía sin mirarlo, tal como muestra su firma—, la calcula `resolver_por_defecto` misma. No es
código nuevo de esta vuelta: confirmé en el `git diff` que esas tres líneas ya estaban, sin tocar,
antes de que existiera `ensamblar_cadena_por_defecto` (el hunk las marca como líneas de contexto,
no como `+`). Es observable por el usuario en el caso más común de todos: un `.env` que sí existe
y sí contiene la clave pedida. `Origen` no tiene otro propósito que ese, según su propio doc
(«se muestra al usuario para que pueda depurar por qué su clave no se coge», `secretos.rs:343-344`
sin cambios). Invertir esa traducción —que encontrar un `.env` etiquete `Origen::Memoria` y no
encontrarlo etiquete `Origen::Archivo` con una ruta inventada— haría que una clave leída de un
archivo real se reportara como si hubiera salido de la nada, sin que ninguna prueba lo note, por
la misma razón que el punto anterior: `resolver_por_defecto()` no tiene un solo llamador de
prueba, así que ninguna de sus dos decisiones propias —cableado y traducción— queda cubierta.

Confirmé por `grep` (`resolver_por_defecto\(\)` en todo el repositorio) que **ninguna prueba
llama a `resolver_por_defecto()`** — ni antes ni después de esta vuelta. Sus únicas tres llamadas
en todo el repositorio son de producción: `cli/src/main.rs:159,260` y
`core/api/src/lib.rs:130,222,266`. Si alguien intercambiara las líneas 440 y 441 —el error de
copiar/pegar más plausible en código con varios parámetros del mismo tipo, y estructuralmente el
mismo tipo de bug que motivó el hallazgo original—, la aplicación real serviría el llavero en
segundo lugar y el `.env` al final, invirtiendo justo la prioridad que el criterio 2 exige, y
**ninguna prueba lo notaría**. Es un hueco más angosto que el original —una función de seis
líneas, fácil de auditar a simple vista, y ya no todo el mecanismo de ensamblado— pero es un
hueco real y del mismo tipo. Ver fila 2 de la tabla de mutaciones; la traducción de `Origen` es la
fila 2b, hermana de esta y con el mismo diagnóstico.

**Conclusión: hallazgo 1 cerrado en la parte que provocó la vuelta (el mecanismo de orden ahora
tiene una prueba real y efectiva), no cerrado del todo.** Cerrarlo exigiría, o bien una prueba que
llame a `resolver_por_defecto()` de verdad con entorno/`.env` controlados (la vía que el HANDOFF
descartó por el riesgo de tests en paralelo pisándose variables de entorno globales — razón que
comparto), o bien extraer también el cableado a una función auxiliar inyectable, del mismo modo
que `guardar_clave_orquestada` hizo con `guardar_clave` (ver siguiente sección). Lo segundo parece
el camino más consistente con el patrón que esta misma vuelta ya usó dos veces.

Nota aparte, de menor riesgo por contraste: `guardar_clave` (`secretos.rs:486-488`) tiene el mismo
tipo de cableado no probado (`guardar_clave_orquestada(referencia, valor, escribir_en_llavero,
dir_configuracion)`), pero ahí los dos parámetros sustituidos tienen firmas *distintas*
(`impl FnOnce(&str, Option<&str>) -> bool` vs. `impl Fn() -> Option<PathBuf>`), así que el
compilador sí impediría un intercambio accidental. No es un hallazgo — lo anoto solo para que
quede claro por qué el caso de `resolver_por_defecto` es el que importa y no ambos por igual.

### Hallazgo 2 (tracing): cerrado

El HANDOFF hace dos afirmaciones fuertes: que el test **ya no escribe en el Credential Manager
real**, y que **cubre las dos ramas de forma determinista en cualquier plataforma**. Comprobé las
dos, leyendo el código, no la prosa.

**¿Ya no toca el llavero real?** Sí. `ningun_evento_de_tracing_contiene_el_valor_de_la_clave`
(`secretos.rs:1111-1165`) llama a `registrar_resultado_de_llavero` (`secretos.rs:327-332`), que
recibe un `keyring::Result<()>` ya resuelto y no contiene ninguna llamada a `keyring::Entry`:

```rust
fn registrar_resultado_de_llavero(resultado: keyring::Result<()>) -> bool {
    if let Err(e) = &resultado {
        tracing::warn!(error = %e, "no se pudo usar el llavero del sistema, se recurre al .env");
    }
    resultado.is_ok()
}
```

Confirmé por `grep` que `escribir_en_llavero` —la única función del archivo que sí llama a
`keyring::Entry::new` (`secretos.rs:308`)— **no aparece en el módulo de tests salvo en
comentarios** (líneas 1119, 1130); su única invocación real está en `guardar_clave`
(`secretos.rs:487`), código de producción. La otra llamada que hace la prueba,
`guardar_clave_en(dir.path(), ...)`, tampoco toca el llavero por diseño (Decisión #1 del propio
archivo). **El test no escribe en ningún almacén real.**

**¿Cubre las dos ramas de forma determinista?** Sí, con aserciones explícitas en cada dirección:

```rust
assert!(registrar_resultado_de_llavero(Ok(())));
assert!(captura.textos().is_empty(), "la rama de éxito no debía emitir ningún evento de tracing");

assert!(!registrar_resultado_de_llavero(Err(keyring::Error::NoEntry)));
assert!(!captura.textos().is_empty(), "la rama de error debía emitir al menos un evento");
```

Respondiendo las dos preguntas puntuales del encargo:

- **¿Sigue habiendo algún camino en que el resultado dependa del entorno?** No, para lo que esta
  prueba ejercita. `registrar_resultado_de_llavero` no llama a ninguna API del sistema operativo;
  con `Ok(())`/`Err(keyring::Error::NoEntry)` sintéticos el resultado es el mismo en Windows,
  Linux con o sin D-Bus, y macOS. El único código que sí depende del entorno
  —`entrada.set_password(v)` dentro de `escribir_en_llavero`, línea 309— queda deliberadamente
  fuera del alcance de esta prueba (ver siguiente punto), no metido de matute dentro de ella.
- **¿El test seguiría pasando si no se emitiera ningún evento?** No. A diferencia de la versión
  que revisé en la primera pasada —un `for texto in captura.textos() { assert!(...) }` sin
  ninguna comprobación de que la colección no estuviera vacía, el patrón exacto que califiqué de
  «afirma algo que el vacío ya cumple»—, esta versión tiene `assert!(!captura.textos().is_empty(),
  ...)` inmediatamente después de la llamada de error. Si alguien quitara el `tracing::warn!` de
  la línea 329, esa aserción fallaría de inmediato. Verifiqué esto con una mutación razonada (fila
  3 de la tabla) y con la mutación inversa —emitir también en la rama de éxito— (fila 4): las dos
  direcciones quedan protegidas.

Sobre la construcción `keyring::Error::NoEntry` en la prueba —el punto que el propio HANDOFF marca
como el de mayor riesgo sin compilador, porque `keyring_core::Error` es `#[non_exhaustive]`—: leí
`keyring-core-1.0.0/src/error.rs` (`NoEntry` es una variante unitaria, sin campos, del enum
`#[non_exhaustive] pub enum Error`) y `keyring-4.2.0/src/v1.rs:27`
(`pub use keyring_core::{Error, Result};`, un *re-export* literal, no un envoltorio propio). Un
`#[non_exhaustive]` en un enum bloquea el *matching* exhaustivo sin comodín y la construcción por
sintaxis de struct con campos con nombre; **no** bloquea construir una variante unitaria ya
existente desde fuera del crate —el ejemplo canónico de la biblioteca estándar es
`std::io::ErrorKind::NotFound`, que cualquier crate externo escribe sin problema pese a que
`ErrorKind` también es `#[non_exhaustive]`—. Y lo confirmé además de forma empírica, no solo por
la regla: `zbus-secret-service-keyring-store` (un crate externo a `keyring-core`, dependencia
`keyring-core = { version = "1" }` en su propio `Cargo.toml.orig`) construye `Error::NoEntry`
directamente en `src/cred.rs:86` y `src/service.rs:213`; `windows-native-keyring-store` lo hace en
`src/utils.rs:396`. La construcción que usa la prueba es sintácticamente idéntica a un patrón que
ya vive, y compila, en dos dependencias transitivas de este mismo `Cargo.toml`. Riesgo de
compilación descartado con evidencia razonable, aunque —como todo en esta vuelta— sin poder
confirmarlo con `rustc` de verdad.

**Lo que queda fuera, declarado y aceptado:** `entrada.set_password(v)` (`secretos.rs:309`), la
única línea del archivo que sigue tocando el almacén real, no la ejercita ninguna prueba. Una fuga
de `tracing` insertada justo ahí no la detectaría nada automático. El HANDOFF lo declara
explícitamente como deuda nueva, con una explicación de por qué cerrarlo (inyectar el `Entry`
como parámetro) es un cambio de diseño mayor, no una corrección de esta vuelta. Coincido: es
deuda razonable, acotada a una expresión de una línea, y con el mismo tipo de mitigación
—auditoría por lectura, no por prueba— que ya se usa en otros puntos de este archivo. Ver fila 5
de la tabla.

**Conclusión: hallazgo 2 cerrado.** El hallazgo 4 del `revisor-codigo` (escribe de verdad en el
Credential Manager al correr `cargo test`) también cierra, por el mismo cambio.

### Hallazgo 5 — `guardar_clave()` como composición

Tres pruebas nuevas llaman a `guardar_clave_orquestada` (`secretos.rs:502-524`) directamente, no a
una reconstrucción de su lógica:

- `guardar_una_clave_en_el_llavero_no_deja_dos_copias_activas` (`secretos.rs:930-964`): éxito en
  el llavero, con una copia vieja en el `.env` que debe retirarse y una clave de otro proveedor
  que debe sobrevivir.
- `guardar_en_el_llavero_sin_copia_vieja_no_crea_el_env` (`secretos.rs:966-984`): éxito en el
  llavero, sin `.env` previo.
- `si_el_llavero_no_esta_disponible_la_clave_no_se_pierde` (`secretos.rs:986-1009`): fallo en el
  llavero, caída al `.env`.

Verifiqué que esto es composición real y no una réplica de dos maneras. Primero, `guardar_clave`
(`secretos.rs:486-488`) es literalmente `guardar_clave_orquestada(referencia, valor,
escribir_en_llavero, dir_configuracion)` — mismo cuerpo de función, con `intentar_llavero` y
`carpeta_config` sustituidos por closures deterministas en la prueba en vez de por las funciones
reales que tocan el sistema operativo. No hay una segunda implementación en ningún lado de «qué
pasa si el llavero tiene éxito». Segundo, dentro de `guardar_clave_orquestada`, cuando la prueba
simula éxito (`|_r, _v| true`), la función real llama de verdad a `purgar_del_env` y, en la rama
de respaldo, a `guardar_clave_en` — ambas funciones de producción sin sustituir. Las aserciones
usan además `DotEnvResolver::desde_archivo(...).resolver(...)` para leer de vuelta el valor, no
una comparación de texto ad hoc, lo que las hace depender de otro código ya probado por separado
(`DotEnvResolver`) en vez de solo de lo que el propio test escribió.

**Conclusión: hallazgo 5 cerrado.** Es exactamente el patrón de inyección de dependencias que ya
usaba `resultado_de_guardar` desde antes de esta vuelta, llevado un nivel más arriba.

### `purgar_del_env`: los cuatro casos de borde del Bloqueante

`purgar_del_env` (`secretos.rs:606-627`) es la función más destructiva de la HU: reescribe el
`.env` real del usuario. Los cuatro casos de borde que pedía el REVIEW:

| Caso | ¿Tiene prueba? | Cuál | Aserción real o de construcción |
|---|---|---|---|
| Archivo inexistente | Sí, directa | `purgar_del_env_no_crea_el_archivo_si_no_existia` (`:1011-1019`) | `assert!(!dir.path().join(".env").exists())` |
| Clave ausente | Sí, directa | `purgar_del_env_no_toca_el_archivo_si_la_clave_no_esta` (`:1021-1037`) | `assert_eq!(antes, despues)` — **contenido completo, no cuenta líneas**: confirmado leyendo el código, compara dos `String` completos leídos con `std::fs::read_to_string` antes y después |
| Otras claves preservadas | Sí, indirecta | `guardar_una_clave_en_el_llavero_no_deja_dos_copias_activas` (`:930-964`), vía `guardar_clave_orquestada` → `purgar_del_env` | `r.resolver("keyring:gemini").as_deref() == Some("de-gemini")` tras purgar `deepseek` — valor real leído con `DotEnvResolver`, no conteo |
| Fallo al reescribir | **No** | — | Solo el argumento estructural de que `?` en la línea 514 propaga cualquier `Err` |

Sobre «otras claves preservadas»: no hay una prueba que llame a `purgar_del_env` *directamente*
con dos claves en el archivo y purgue una — el HANDOFF eligió no duplicar esa comprobación porque
`purgar_del_env` delega en `guardar_clave_en`, que ya tiene su propia prueba dedicada
(`guardar_no_pisa_las_claves_de_otros_proveedores`). Es una decisión razonable —no repetir una
garantía ya probada en la función que se reutiliza—, y la prueba de composición
(`guardar_una_clave_en_el_llavero_no_deja_dos_copias_activas`) sí la ejercita de punta a punta con
el escenario que de verdad importa (llavero con éxito, dos proveedores). No lo cuento como hueco.

Sobre «fallo al reescribir»: el HANDOFF lo declara sin prueba, con la razón de que forzarlo de
forma determinista y portable habría exigido dejar un archivo sin permiso de escritura —la misma
clase de manipulación de permisos que sí usa `el_archivo_de_claves_queda_ilegible_para_los_demas`,
pero solo en Unix (`#[cfg(unix)]`); en Windows habría hecho falta una API de permisos distinta, que
el HANDOFF no puede verificar sin compilador—. El argumento tiene mérito, pero a diferencia del
caso del mock (que es *imposible* en el CI de Linux, no solo laborioso), este caso **sí seria
alcanzable** con más trabajo: no es lo mismo «no se puede» que «no se hizo». Lo trato como hueco
real, no como deuda cerrada por argumento estructural — ver fila 10 de la tabla y § «Criterios de
aceptación sin prueba».

### Tests adaptados: sin debilitamiento

Revisé `git diff -- core/providers/src/secretos.rs` completo (780 líneas) buscando específicamente
si alguno de los cinco tests que mi primera pasada ya había calificado de «adaptados sin
debilitarse» (`guardar_una_clave_la_deja_legible_para_el_resolutor`,
`guardar_no_pisa_las_claves_de_otros_proveedores`, `guardar_dos_veces_sustituye_en_vez_de_acumular`,
`borrar_una_clave_la_quita_del_archivo`, `el_archivo_de_claves_queda_ilegible_para_los_demas`) se
tocó de nuevo en esta segunda vuelta. Los leí en su forma actual (`secretos.rs:763-828`) y son
byte a byte los mismos que documentó mi primera pasada: mismas aserciones, mismo helper `ruta_de`
(que sigue entrando en pánico ante cualquier variante que no sea `Origen::Archivo`, en vez de
aceptarla con un `matches!` laxo). El único cambio visible en las líneas removidas del diff que
las toca es la adaptación de tipo original (`PathBuf` → `Origen` vía `ruta_de`), ya evaluada. No
encontré ninguna aserción debilitada, ni en estos cinco ni en el resto del archivo — repasé
también todas las líneas `+` que contienen `assert` (17 en total) una por una; ninguna es trivial
(`assert!(true)`, un `matches!` sin variante específica, una tolerancia amplia).

### La sugerencia del mock, descartada: evaluación

El REVIEW proponía el backend `mock` de `keyring-core` para probar la rama de éxito sin tocar el
llavero real. El HANDOFF lo investigó y decidió no usarlo, con cinco afirmaciones técnicas
concretas. Las verifiqué las cinco contra el código fuente real, no contra la prosa:

1. **«`keyring-core` sí trae un backend mock, sin *feature flag*».** Confirmado:
   `keyring-core-1.0.0/src/lib.rs:39` declara `pub mod mock;` sin ningún `#[cfg(feature = ...)]`
   encima (a diferencia de `sample`, que sí lo tiene en la línea 41). `Cargo.toml.orig` no lista
   `mock` entre las features condicionales.
2. **«`keyring::Entry` fija el backend nativo en un `LazyLock` evaluado una sola vez por
   proceso».** Confirmado literalmente: `keyring-4.2.0/src/v1.rs:107`,
   `static SET_CREDENTIAL_STORE_RESULT: LazyLock<Result<()>> = LazyLock::new(set_credential_store);`.
3. **«En el CI de Linux sin D-Bus queda en `NoDefaultStore` para siempre, y ningún mock instalado
   después puede intervenir».** Confirmado por lectura directa del control de flujo:
   `Entry::new` (`v1.rs:47-53`) comprueba `if SET_CREDENTIAL_STORE_RESULT.is_err() { return
   Err(Error::NoDefaultStore); }` **antes** de llegar a `keyring_core::Entry::new(...)`, que es la
   única línea que de verdad consulta el store por defecto registrado en `keyring_core` (mock o
   no). Y `set_credential_store()` (`v1.rs:109-129`), en la rama Unix no-Apple, llama a
   `zbus_secret_service_keyring_store::Store::new()?` con `?` — si eso falla (sin D-Bus, que es
   exactamente el caso del CI: confirmé en `zbus-secret-service-keyring-store-1.0.0/src/store.rs`
   que `Store::new()` llama a `Service::new()?` de inmediato, sin ninguna espera perezosa
   visible), la función devuelve `Err` **sin llegar nunca** a `keyring_core::set_default_store(...)`.
   Es decir: aunque un test hubiera sembrado un mock en `keyring_core` *antes* de la primera
   llamada a `keyring::Entry::new`, esa primera llamada dispara el `LazyLock`, que falla al
   construir el backend nativo, y `Entry::new` corta camino con `NoDefaultStore` sin mirar jamás
   qué store está registrado. La cadena de razonamiento es correcta de punta a punta.
4. **Riesgo de estado global mutable entre tests paralelos.** Es cierto en términos generales
   —`keyring_core::set_default_store` es, por su propia naturaleza, un `static` de proceso— y es
   consistente con el mismo criterio que este archivo ya aplica en otros puntos (no mutar
   `XDG_CONFIG_HOME` en tests paralelos). No necesita verificación de código fuente, es una
   propiedad estructural de cualquier *setter* de un `static` compartido.

**El argumento se sostiene en las cinco partes.** Es exactamente el caso que se pidió reconocer:
una sugerencia mía, descartada con evidencia que aguanta la lectura del código fuente real, no
solo la palabra de quien la descartó. Buen resultado, no una derrota.

**Sobre la deuda que queda como consecuencia** —la prueba de ida y vuelta del criterio 1 sigue sin
existir—: la considero **aceptable**, con matices. El `REVIEW.md` ya la clasificó como «Nota», la
severidad más baja de la tabla, y pidió explícitamente «evaluarla en la vuelta», no resolverla —
eso se cumplió, y con un estándar de evidencia más alto que el habitual (código fuente descargado
y leído, no solo inferido). Cerrarla de verdad exigiría un cambio de diseño —inyectar la
construcción del `Entry`/store en `LlaveroResolver` y `escribir_en_llavero`, tal como el propio
HANDOFF esboza en «Deuda que dejo»—, que es desproporcionado para una corrección hecha sin
compilador para verificarla. Queda mitigada, no eliminada, por la recomendación explícita de
probarlo a mano en un escritorio real antes de dar la HU por cerrada con confianza total. Mi única
adición: dado que ahora existe una razón técnica concreta y verificada (el `LazyLock`), convendría
que quedara anotada en el tablero con esa razón, no solo repetida como «pendiente» — para que
nadie proponga el mismo mock otra vez sin leer esta investigación primero. El propio HANDOFF ya lo
deja escrito con el detalle suficiente, así que esto es más una sugerencia de dónde apuntar el
enlace que un hallazgo.

### Tabla de mutaciones

Todas `no ejecutado (B-1)`; la columna «Análisis estático» da la conclusión razonada.

| Prueba | Archivo:línea mutado | Mutación | Resultado | Análisis estático |
|---|---|---|---|---|
| `el_llavero_va_despues_del_entorno_y_del_env` | `secretos.rs:416-419` (`ensamblar_cadena_por_defecto`) | Invertir el orden: `.con(Origen::Llavero, llavero)` primero, `.con(Origen::Entorno, entorno)` al final | no ejecutado (B-1) | **La detectaría.** La prueba llama a esta función directamente con `MapResolver` distinguibles en cada posición y compara el valor devuelto contra el eslabón esperado |
| *(ninguna — hueco residual del hallazgo 1)* | `secretos.rs:440-441` (`resolver_por_defecto`) | Intercambiar `Box::new(dotenv)` y `Box::new(LlaveroResolver::nuevo())` en la llamada a `ensamblar_cadena_por_defecto` | no ejecutado (B-1) | **No la detectaría ninguna prueba.** Ninguna prueba del repositorio llama a `resolver_por_defecto()` (confirmado por `grep`); solo la usan `cli/src/main.rs` y `core/api/src/lib.rs`, código de producción |
| *(ninguna — hueco 2b, hermano del anterior)* | `secretos.rs:435` (`resolver_por_defecto`) | Invertir la traducción a `Origen`: que encontrar un `.env` etiquete `Origen::Memoria` y no encontrarlo etiquete `Origen::Archivo` con una ruta inventada | no ejecutado (B-1) | **No la detectaría ninguna prueba**, por la misma razón que la fila anterior. Lógica preexistente a esta HU (no la tocó ninguna de las dos vueltas), y sigue sin que ninguna prueba llame a la función que la contiene |
| `ningun_evento_de_tracing_contiene_el_valor_de_la_clave` (rama de error) | `secretos.rs:328-330` (`registrar_resultado_de_llavero`) | Quitar el `tracing::warn!` del brazo `Err` | no ejecutado (B-1) | **La detectaría.** `assert!(!captura.textos().is_empty(), "la rama de error debía emitir al menos un evento")` fallaría con la colección vacía |
| `ningun_evento_de_tracing_contiene_el_valor_de_la_clave` (rama de éxito) | `secretos.rs:328` (`registrar_resultado_de_llavero`) | Emitir `tracing::warn!(...)` también cuando `resultado` es `Ok` | no ejecutado (B-1) | **La detectaría.** `assert!(captura.textos().is_empty(), "la rama de éxito no debía emitir ningún evento de tracing")` fallaría |
| *(ninguna — deuda ya declarada por el HANDOFF)* | `secretos.rs:309` (`escribir_en_llavero`) | Insertar `tracing::warn!(valor = %v, "...")` en el brazo `Some(v) => entrada.set_password(v)` | no ejecutado (B-1) | **No la detectaría ninguna prueba.** `escribir_en_llavero` no aparece invocada en el módulo de tests, solo en comentarios |
| `guardar_una_clave_en_el_llavero_no_deja_dos_copias_activas` | `secretos.rs:510-515` (`guardar_clave_orquestada`) | Quitar todo el bloque `if en_llavero { if let Some(dir) = carpeta_config() { purgar_del_env(&dir, referencia)?; } }` | no ejecutado (B-1) | **La detectaría.** Reproduce el hallazgo 1 Bloqueante exacto: `r.resolver("keyring:deepseek").is_none()` fallaría, la copia vieja seguiría en el `.env` |
| `purgar_del_env_no_crea_el_archivo_si_no_existia` y `guardar_en_el_llavero_sin_copia_vieja_no_crea_el_env` | `secretos.rs:608-610` (`purgar_del_env`) | En el `else` de `let Ok(previo) = ... else { ... }`, crear un `.env` vacío antes de `return Ok(())` | no ejecutado (B-1) | **Las detectaría, las dos.** `assert!(!dir.path().join(".env").exists())` fallaría en ambas |
| `purgar_del_env_no_toca_el_archivo_si_la_clave_no_esta` | `secretos.rs:623-625` (`purgar_del_env`) | Quitar el `if ya_estaba`: llamar siempre a `guardar_clave_en(dir, referencia, None)` | no ejecutado (B-1) | **No la detectaría.** Ver «Pruebas que no protegen» — `guardar_clave_en` con una clave ausente es idempotente en contenido, así que `assert_eq!(antes, despues)` sigue pasando |
| `si_el_llavero_no_esta_disponible_la_clave_no_se_pierde` | `secretos.rs:518` (`guardar_clave_orquestada`) | Cambiar `resultado_de_guardar(en_llavero, ...)` por `resultado_de_guardar(!en_llavero, ...)` | no ejecutado (B-1) | **La detectaría.** Con el llavero simulado en fallo, la mutación devolvería `Ok(Origen::Llavero)`; el helper `ruta_de` entra en pánico ante cualquier variante que no sea `Origen::Archivo` |
| *(ninguna — cuarto caso de borde sin prueba)* | `secretos.rs:514` (`guardar_clave_orquestada`) | Cambiar `purgar_del_env(&dir, referencia)?;` por `let _ = purgar_del_env(&dir, referencia);` (tragar el error) | no ejecutado (B-1) | **No la detectaría ninguna prueba.** Ninguna prueba fuerza que `purgar_del_env` devuelva `Err` a través de la composición completa |

### Pruebas que no protegen (o protegen menos de lo que el HANDOFF asume)

**`purgar_del_env_no_toca_el_archivo_si_la_clave_no_esta` no protege el `if ya_estaba` que dice
proteger.** El HANDOFF la describe así: «se comprueba primero con un `.any(...)` de solo lectura;
si no aparece, no se reescribe nada» — atribuyéndole la protección de esa comprobación
(`secretos.rs:613-625`). Rastreé qué pasaría si se quitara: con el escenario exacto de la prueba
(un `.env` con solo `GEMINI_API_KEY=de-gemini`, purgando `deepseek`, que no está), llamar a
`guardar_clave_en(dir, "keyring:deepseek", None)` sin la protección del `if ya_estaba` reescribe
el archivo — pero con el **mismo contenido byte a byte**: el bucle de sustitución de
`guardar_clave_en` (`secretos.rs:552-569`) ya compara por nombre canónico exacto, ninguna línea
coincide con `DEEPSEEK_API_KEY`, así que cada línea se copia tal cual y no se añade nada porque
`valor` es `None`. `assert_eq!(antes, despues)` sigue pasando con el `if ya_estaba` roto.

Esto no es el patrón de «afirma algo que el vacío ya cumple» de mi primera pasada, es distinto: la
propiedad que la prueba de verdad puede observar —el contenido final del archivo es correcto— está
garantizada dos veces, por dos mecanismos independientes (el `if ya_estaba` de `purgar_del_env`, y
la selectividad exacta de `guardar_clave_en`, esta última ya probada aparte por
`guardar_no_pisa_las_claves_de_otros_proveedores`). Mutar el primero no cambia el resultado
observable porque el segundo ya lo cubre. Es un hallazgo real —la prueba no protege la línea que
el HANDOFF dice que protege— pero de **riesgo práctico bajo**: el propósito explícito del
`if ya_estaba`, según el propio comentario del código (`secretos.rs:601-605`), es evitar el efecto
secundario de *crear* el archivo cuando no había nada que limpiar — y **eso sí** lo protegen, por
partida doble, `purgar_del_env_no_crea_el_archivo_si_no_existia` y
`guardar_en_el_llavero_sin_copia_vieja_no_crea_el_env` (fila 7 de la tabla). Lo que no protege
ninguna prueba es la optimización de "evitar una reescritura innecesaria de un archivo que sí
existe", que no tiene ningún efecto observable distinto de no hacerla, dado que `guardar_clave_en`
es seguro de invocar de más. Clasifico esto **Importante** por el criterio literal del protocolo
(la prueba sigue en verde con esa línea rota), con la salvedad explícita de que el comportamiento
que el usuario final observa no está en riesgo.

### Criterios de aceptación de `docs/06` sin prueba que los cubra

Actualización de mi tabla de la primera pasada, solo lo que cambió:

- **AC 2** («Va el último de la cadena…»): pasa de «sin prueba real» a **«con prueba del
  mecanismo, sin prueba del ensamblado real»** — ver «Hallazgo 1» arriba. Ya no es un hueco total,
  pero tampoco está cerrado.
- **AC 3** («Guardar una clave… la escribe en el llavero, no en un archivo»): cierra con las tres
  pruebas de `guardar_clave_orquestada` (hallazgo 5). El único resto sin cubrir es el cuarto caso
  de borde de `purgar_del_env` (fallo al reescribir), que es parte del AC 3 en su lectura estricta
  («un fallo no debe informar éxito»).
- **AC 5** («Ninguna clave aparece en los logs…»): cierra para `registrar_resultado_de_llavero`;
  sigue sin prueba la línea `entrada.set_password(v)` de `escribir_en_llavero`, deuda declarada y
  aceptada.
- **AC 1** sigue igual que en la primera pasada: sin prueba de éxito, deuda evaluada esta vuelta y
  aceptada con matices (ver «La sugerencia del mock»).

AC 4 no cambió en esta vuelta.

### Premisas que cuestiono

- **«`resolver_por_defecto` no hace nada más que llamar a [`ensamblar_cadena_por_defecto`] con las
  piezas reales» (HANDOFF, hallazgo 2 de la segunda vuelta).** Es cierta para la lógica de
  *orden*, pero no para otras dos cosas que la misma función sigue decidiendo por su cuenta: el
  *cableado* (qué objeto concreto llega a cada parámetro posicional) y la *traducción* a `Origen`
  del resultado de `dotenv.origen()` (línea 435, preexistente a esta HU). Ninguna de las dos la
  ejercita ninguna prueba. **Conclusión: la premisa mezcla "ya no decide el orden" con "ya no
  decide nada", y son cosas distintas — ver hallazgo 1 arriba. El hallazgo 2 del `REVIEW.md` queda
  cerrado en el mecanismo, no en la función pública real.**

- **«`purgar_del_env` cubre los cuatro casos de borde que pedía el REVIEW» (HANDOFF, hallazgo 1 de
  la segunda vuelta).** Cierto para tres; el cuarto («fallo al reescribir») se cubre solo con un
  argumento estructural sobre el operador `?`, no con una prueba. El propio HANDOFF es honesto al
  respecto en «Lo que NO pude verificar», pero la frase resumen del hallazgo 1 no distingue los
  tres casos probados del cuarto razonado. **Conclusión: la cobertura real es 3 de 4; el cuarto es
  deuda alcanzable (no imposible, a diferencia del caso del mock), y debería quedar anotado como
  tal en vez de implícito dentro de "cubre los cuatro".**

- **«Las dos ramas quedan cubiertas de verdad, de forma determinista, en cualquier plataforma»
  (HANDOFF, sobre la prueba de `tracing`).** La comprobé y se sostiene para
  `registrar_resultado_de_llavero`, que es todo lo que la prueba ejercita. **Conclusión: la
  premisa es correcta, pero solo alcanza a la función que aísla la decisión de log — no a
  `escribir_en_llavero` completa, algo que el propio HANDOFF ya distingue con cuidado al decir que
  resuelve el hallazgo 3 "parcialmente". Confirmo que esa calibración propia es precisa, ni
  optimista ni insuficiente.**

- **«`guardar_una_clave_en_el_llavero_no_deja_dos_copias_activas`... comprueba que solo uno se
  retira» como evidencia de que `purgar_del_env` "conserva las demás claves"» (HANDOFF, hallazgo 1
  de la segunda vuelta).** La comprobé y es cierta, pero vale la pena decir explícitamente que es
  una prueba de la *composición* (`guardar_clave_orquestada`), no de `purgar_del_env` en
  aislamiento — una distinción que no cambia la conclusión (el caso está cubierto) pero sí
  importa para quien busque después una prueba unitaria de `purgar_del_env` y no la encuentre.
  **Conclusión: cobertura real, ubicación distinta de la que su descripción sugiere a primera
  lectura.**

### Qué verifiqué y no marqué

- No verifiqué que el archivo compile. Sigue sin haber `cargo`/`rustc`, y esta vez confirmé
  además que no hay `rustup` ni una carpeta `~/.cargo/bin` en ninguna de las dos ubicaciones
  (Windows, WSL `Ubuntu-26.04`) — antes solo había confirmado la ausencia del binario en el PATH.
- Leí las fuentes de `keyring-core`, `keyring`, `zbus-secret-service-keyring-store` y
  `windows-native-keyring-store` que encontré ya descargadas en el `Temp` de esta máquina
  (`C:\Users\naunf\AppData\Local\Temp\{kc-src,keyring-src,zbus-src,wnks11-src}`), dejadas ahí por
  el propio implementador según su HANDOFF. No las descargué yo, no confirmé que esta sesión tenga
  acceso a Internet, y no comparé estos archivos byte a byte contra una copia fresca de
  crates.io — confío en su autenticidad por su estructura interna (`Cargo.toml.orig`,
  `.cargo_vcs_info.json`, que `cargo package` genera y que no es trivial de falsificar a mano) y
  por ser consistentes entre sí y con lo que el HANDOFF describe, pero es una confianza razonada,
  no una verificación independiente de origen.
- Dentro de esas fuentes, leí completos `error.rs`, `mock.rs` y `lib.rs` de `keyring-core`, y
  `v1.rs`/`lib.rs` de `keyring`. De `zbus-secret-service-keyring-store` y
  `windows-native-keyring-store` solo grepeé construcciones de `Error::NoEntry` y leí `store.rs`
  del primero (para el arranque de `Service::new()`); no leí `service.rs`, `cred.rs` ni
  `utils.rs` completos, así que no puedo certificar que la conexión a D-Bus sea síncrona de punta
  a punta dentro de `Service::new()` — solo que se invoca sin ninguna espera diferida visible
  desde `Store::new_internal()`.
- No re-verifiqué el valor exacto de `CRED_MAX_USERNAME_LENGTH` en Windows pese a tener ahora
  acceso a `windows-native-keyring-store-1.1.0/src/utils.rs`: confirmé que la constante se importa
  desde fuera del crate (probablemente `windows-sys`, no declarada en este código), pero no bajé
  esa dependencia. Es el mismo punto que la primera pasada y el propio HANDOFF ya dejan sin
  verificar; no estaba en el alcance que se me pidió para esta vuelta (el test que la usa,
  `un_error_del_llavero_se_convierte_en_none`, no se tocó en esta vuelta).
- No revisé de nuevo `core/providers/Cargo.toml`, `core/api/src/puente.rs` ni
  `app/lib/datos/repositorio_rust.dart`: el HANDOFF declara que ninguno de los tres se tocó en
  esta vuelta, y lo confirmé por `git diff --stat` (solo `core/providers/src/secretos.rs` aparece
  con cambios desde mi primera pasada — de hecho el `--stat` de esta vuelta ya solo lista ese
  archivo, coherente con la declaración).
- No toqué `core/audio-capture` ni nada relacionado con HU-01, como se me indicó explícitamente.
- No apliqué ninguna mutación real al árbol de trabajo: sin `cargo`, mutar sin poder compilar no
  produce ninguna señal verde/rojo. Todas las filas de la tabla son análisis estático razonado,
  no ejecución.
- `git status --short` al empezar y al terminar esta segunda vuelta es idéntico (la única
  diferencia es el contenido de este mismo archivo, que no cambia la línea `?? _orquestacion/`
  en el resumen corto de `git status`, ya que toda la carpeta figura como no rastreada de por
  sí). No muté `core/providers/src/secretos.rs`, así que no hubo necesidad de copiarlo al
  scratchpad ni de restaurarlo.

### Auditoría de esta re-lectura (2026-09-03, pasada final)

Encontré el archivo con la sección «Segunda vuelta» ya escrita de punta a punta — la sesión previa
alcanzó a completarla antes de quedarse sin cuota, no solo a empezarla. En vez de reescribirla,
hice mi propia pasada independiente **desde cero, sin leer este archivo primero**: releí
`secretos.rs` completo, tracé a mano cada prueba nueva o tocada, y solo después comparé mis
conclusiones contra lo ya escrito. Coincidieron en todo salvo un punto (el «hueco 2b» de
`resolver_por_defecto`, añadido arriba). Donde pude, no me limité a confiar en la prosa:

- Repetí la comprobación de B-1 en Windows y en WSL `Ubuntu-26.04` — la máquina virtual ya ni
  arranca (`HCS_E_CONNECTION_TIMEOUT`), un síntoma nuevo que ni mi primera pasada ni esta segunda
  vuelta habían visto. Mismo veredicto: sin `cargo`.
- Leí yo mismo, directamente y sin intermediarios, las tres piezas de código fuente en las que se
  apoya «La sugerencia del mock»: `keyring-4.2.0/src/v1.rs` (el `LazyLock` en la línea 107 y el
  corte temprano en `Entry::new`, líneas 48-49), y `zbus-secret-service-keyring-store-1.0.0/src/store.rs`
  más `src/service.rs` (`Store::new` → `Store::new_internal` → `Service::new` →
  `SecretService::connect(...)`, sin ninguna espera diferida entre medio). Las tres citas de línea
  y las tres afirmaciones de esta sección quedan confirmadas por mí, no solo heredadas.
- Comprobé con `git diff` que ninguna de las cinco pruebas preexistentes readaptadas perdió una
  aserción, revisando el diff completo línea por línea, no solo los fragmentos ya citados.

No repetí desde cero la lectura de `mock.rs`/`lib.rs` de `keyring-core` ni de `cred.rs`/`utils.rs`
de los dos backends nativos: confié en lo que ya dejó escrito la pasada anterior sobre esos
archivos, con la misma reserva que ella ya anotó (no descargué nada yo mismo, no confirmé acceso a
Internet en esta sesión — lo intenté con `curl` contra `crates.io` y devolvió `403`).

---

## Tercera vuelta (2026-09-03)

Acotada al encargo: los ocho tests nuevos de H1-bis/H3-bis/H5-bis/H7-bis/H8-bis/H9-bis, la
reproducción de los dos scripts de Python del `HANDOFF`, y la comprobación de que nada preexistente
se debilitó. Único archivo de código tocado en esta vuelta: `core/providers/src/secretos.rs`
(confirmado leyendo `HANDOFF.md` y con `git diff --stat`, que solo lista ese archivo).

### 1. Veredicto

**NO VERIFICABLE por B-1 en Rust** (sin cambios: `cargo`/`rustc` ausentes, y esta vez la propia VM
de WSL ni arranca) — **pero, por primera vez en esta HU, sí hay verificación por ejecución real**:
reproduje los dos scripts de Python del `HANDOFF` tal cual están transcritos y los dos corren y
imprimen «todas las aserciones pasaron», confirmando que la traducción a Python es fiel al Rust
real (la comparé línea por línea contra `secretos.rs`, y contra el `git diff` para la lógica
vieja). Además escribí y corrí un tercer script propio para cerrar un hueco que el script del
`HANDOFF` **no** cubre pese a que su prosa dice que sí (ver «Premisas que cuestiono»): con él,
confirmo por cálculo que H8-bis y H9-bis sí detectan sus mutaciones.

Dos hallazgos **Importantes** nuevos, los dos confirmados por ejecución (Python) o por `grep`, no
solo por lectura:

1. **El cableado de `resolver_por_defecto()` sigue sin ninguna prueba.** H5-bis/H7-bis cerraron el
   cableado *dentro* de `cadena_con` y la traducción a `Origen`, pero `resolver_por_defecto()`
   —la función que usa la aplicación real— le pasa a `cadena_con` dos argumentos del mismo tipo
   exacto (`entorno` y `llavero`, ambos `Box<dyn KeyResolver>`) en un intercambio que compila sin
   aviso, y **ninguna prueba llama a `resolver_por_defecto()`** (confirmado por `grep` en todo el
   repositorio). Es el mismo hallazgo de la Segunda vuelta, no cerrado: solo se movió un nivel más
   adentro.
2. **Las dos pruebas nuevas de H1-bis cubren 2 de las 3 formas de `nombres_candidatos`, no las
   tres.** Confirmé por mutación ejecutada (Python) que una `linea_declara` que ignorara la
   primera forma (minúscula, sin transformar) dejaría las dos pruebas nuevas en verde, porque las
   dos usan el escenario `"GEMINI=sk-vieja"` (la forma intermedia).

Ninguna de las ocho pruebas nuevas falla por construcción. `git diff -U0` (solo líneas borradas,
51 en total, acumuladas desde antes de la HU) no muestra ninguna aserción debilitada; de paso
confirma, contra el código real y no solo contra la prosa del `HANDOFF`, que la lógica vieja de
`linea_declara` comparaba en efecto solo contra `nombre_canonico`.

### 2. Qué pude ejecutar y qué no

```
$ command -v cargo rustc flutter
/c/dev/flutter/bin/flutter

$ cargo --version
/usr/bin/bash: line 1: cargo: command not found
$ rustc --version
/usr/bin/bash: line 1: rustc: command not found

$ ls "$HOME/.cargo/bin"; ls "$HOME/.rustup"
No such file or directory (las dos)

$ wsl -e bash -lc "command -v cargo rustc; cargo --version"
wsl: Clave 'wsl2.autoMemoryReclaim' desconocida en .wslconfig:16
La operación superó el tiempo de espera porque no se recibió ninguna respuesta de la máquina
virtual ni del contenedor. Código de error: Wsl/Service/CreateInstance/CreateVm/HCS_E_CONNECTION_TIMEOUT
```

B-1 confirmado de nuevo, por cuarta vez consecutiva en esta HU. La VM de WSL, que en las dos
vueltas anteriores al menos arrancaba lo suficiente para decir «cargo: command not found» dentro de
`Ubuntu-26.04`, esta vez ni siquiera arranca (mismo síntoma nuevo que ya vio la pasada final de la
segunda vuelta, `HCS_E_CONNECTION_TIMEOUT` — persiste, no se resolvió solo). No corrí `cargo test`,
no apliqué ninguna mutación a `secretos.rs` (nada que compilar no genera ninguna señal), y no lo
toqué.

Lo que sí corrí, con `python3` (3.14.3, confirmado con `python3 --version`):

- Transcribí los dos scripts del `HANDOFF` (`verificar_h1bis.py`, `verificar_cadena_con.py`) **byte
  a byte** al scratchpad y los corrí: `python3 verificar_h1bis.py` → `verificar_h1bis.py: todas las
  aserciones pasaron`; `python3 verificar_cadena_con.py` → `verificar_cadena_con.py: todas las
  aserciones pasaron`. Los dos, tal cual el `HANDOFF` dice haber hecho.
- Antes de confiar en el resultado, comparé cada función de esos scripts contra el `secretos.rs`
  real, línea por línea: `nombres_candidatos`, `nombre_canonico`, `nombre_de_linea`,
  `linea_declara`, el bucle de `guardar_clave_en` y el guard de `purgar_del_env_con` están
  traducidos con fidelidad (mismo orden de operaciones, mismos casos borde). La única
  simplificación real es `dotenv_resolver_sim`, que omite el manejo de comillas y comentarios de
  fin de línea de `parsear()` (`secretos.rs:188-228`) — no afecta a ninguna aserción del script,
  porque ningún escenario probado usa comillas ni comentarios en línea, pero no es una réplica 1:1
  de `parsear`, solo de lo que hacía falta para el escenario.
- Escribí y corrí un tercer script, propio (no del `HANDOFF`): `verificar_h8h9_propio.py`, que
  modela `purgar_del_env_con` (`secretos.rs:740-766`) con el parámetro `escribir` inyectado —igual
  que el Rust real, no una réplica del test— y un espía que panica o que falla, para confirmar por
  cálculo lo que el script del `HANDOFF` describe en prosa pero no implementa (ver «Premisas que
  cuestiono»). Salida completa:
  ```
  1) código real, clave ausente: el espía NO se llamó (correcto, sin excepción)
  2) mutación H8-bis: el espía SÍ se llamó y panicó (...) -> la prueba real se pondría en rojo
  3) código real, clave presente + escribir_fn que falla: el error SÍ se propaga (correcto)
  4) mutación H9-bis: el error se traga sin excepción -> resultado.is_err() sería falso -> prueba en rojo
  ```
- Escribí un cuarto bloque (dentro del mismo `python3 -c`) con una `linea_declara_SKIP1` que
  descarta la primera forma de `nombres_candidatos`, para responder la pregunta explícita sobre
  cobertura de las tres formas — resultado en la tabla, fila 4.
- `git diff --stat -- core/providers/src/secretos.rs`: `986 insertions(+), 51 deletions(-)`
  (acumulado desde antes de la HU). `git diff -U0 -- core/providers/src/secretos.rs | grep
  '^-[^-]'` para ver solo las 51 líneas borradas: ninguna es una aserción debilitada; son
  refactors legítimos (el `guardar_clave` viejo que devolvía `PathBuf`, el comentario viejo sobre
  «no se usa el llavero», y la comparación vieja `clave_linea == Some(nombre.as_str())` que
  confirma, contra el código y no solo contra la palabra del `HANDOFF`, cómo era `linea_declara`
  antes de esta vuelta).
- `grep -rn "resolver_por_defecto()"` en todo el repositorio: cinco llamadas, las cinco en código
  de producción (`cli/src/main.rs:159,260`; `core/api/src/lib.rs:130,222,266`). Cero en el módulo
  de tests, antes y después de esta vuelta.
- `grep` de `gemini=`, `deepseek=`, `openai=` (minúsculas, sin prefijo) en `secretos.rs`: cero
  resultados dentro de contenido de prueba — ninguna prueba del archivo usa la primera forma de
  `nombres_candidatos` en un `.env` real.
- Leí `core/providers/src/secretos.rs` completo (1499 líneas, las 800 primeras y las últimas 700 en
  dos pasadas) y las secciones «Segunda vuelta»/«Tercera vuelta» de `HANDOFF.md` y de este mismo
  archivo.
- `git status --short` idéntico al inicio y al final de esta vuelta (diff de los dos guardados en
  el scratchpad, vacío).

### 3. Tabla de mutaciones

| Prueba | Archivo:línea mutado | Mutación | Resultado |
|---|---|---|---|
| `purgar_del_env_no_reescribe_si_la_clave_no_esta` | `secretos.rs:762` (`purgar_del_env_con`), quitar el `if ya_estaba` | reescribir sin condición | **rojo (protege)** — confirmado por cálculo (Python, script propio, paso 2): el espía inyectado panica |
| `purgar_del_env_propaga_el_error_si_falla_al_reescribir` | `secretos.rs:763`, tragar el error (`let _ = escribir(...)` en vez de `escribir(...)?`) | swallow del `Err` | **rojo (protege)** — confirmado por cálculo (Python, script propio, paso 4) |
| `guardar_en_el_llavero_purga_una_copia_vieja_escrita_con_nombre_corto` + `purgar_del_env_reconoce_una_clave_escrita_con_un_nombre_corto` | `secretos.rs:601-604` (`linea_declara`), volver a comparar solo contra `nombre_canonico` (la lógica vieja, confirmada por `git diff`) | revertir a una sola forma | **rojo (protege)** — confirmado ejecutando `verificar_h1bis.py` del `HANDOFF` tal cual |
| Mismo par | `secretos.rs:602`, `linea_declara` usando `nombres_candidatos(referencia)[1..]` (se salta la forma en minúsculas, deja las otras dos intactas) | omitir solo el primer candidato | **verde (NO protege)** — confirmado por cálculo (Python, script propio): con el escenario real de las dos pruebas (`"GEMINI=sk-vieja"`, índice 1) el resultado no cambia |
| `cadena_con_no_intercambia_el_archivo_con_el_llavero` | `secretos.rs:474` (`cadena_con`), intercambiar `Box::new(dotenv)` y `llavero` en la llamada a `ensamblar_cadena_por_defecto` | swap posicional | **rojo (protege)** — confirmado ejecutando `verificar_cadena_con.py` del `HANDOFF` tal cual, en las dos mitades del test (valor y `Origen`) |
| `un_env_encontrado_se_reporta_como_origen_archivo_no_como_memoria` + `sin_env_encontrado_el_origen_es_memoria_no_una_ruta_inventada` | `secretos.rs:469-472` (`cadena_con`), invertir qué rama de `.map(...).unwrap_or(...)` produce `Origen::Archivo` y cuál `Origen::Memoria` | invertir la traducción | rojo por trazado manual (no hay script Python para esta parte; lógica de dos líneas, directa de seguir) |
| `purgar_del_env_propaga_un_error_de_lectura_que_no_es_archivo_ausente` | `secretos.rs:756`, tratar cualquier error de lectura como `NotFound` | ensanchar el `match` | rojo por trazado manual — mismo mecanismo que H3-bis |
| *(ninguna — hueco residual, no cerrado desde la Segunda vuelta)* | `secretos.rs:486-490` (`resolver_por_defecto`), intercambiar `Box::new(EnvResolver)` y `Box::new(LlaveroResolver::nuevo())` | swap posicional en el llamador real | **no la detectaría ninguna prueba** — confirmado por `grep`: `resolver_por_defecto()` no tiene ningún llamador de prueba |

### 4. Pruebas que no protegen

**Las dos pruebas nuevas de H1-bis protegen el escenario reportado, no las tres formas que
`nombres_candidatos` define.** El `HANDOFF` describe el arreglo como «consultar el contrato
completo, no una versión reducida» y usa el escenario `"GEMINI=sk-vieja"` (la forma intermedia,
mayúscula sin `_API_KEY`) para las dos pruebas nuevas
(`guardar_en_el_llavero_purga_una_copia_vieja_escrita_con_nombre_corto`,
`purgar_del_env_reconoce_una_clave_escrita_con_un_nombre_corto`). Ninguna prueba del archivo —ni
estas dos, ni ninguna preexistente— ejercita la **primera** forma (minúscula, tal cual `referencia`
sin transformar, p. ej. `"gemini=sk-vieja"`) contra `linea_declara`/`guardar_clave_en`/
`purgar_del_env`: confirmé con `grep` que esa forma no aparece en ningún contenido de prueba, y con
una mutación ejecutada (tabla, fila 4) que las dos pruebas nuevas siguen en verde si `linea_declara`
dejara de reconocerla. La forma canónica (`_API_KEY`) sí queda cubierta, pero por pruebas
preexistentes a esta vuelta (`guardar_no_pisa_las_claves_de_otros_proveedores`, etc.), no por las
dos nuevas. Clasifico esto **Importante** por el criterio literal (una mutación plausible en
`linea_declara` pasa desapercibida), con una salvedad que baja la urgencia práctica: `nombres_
candidatos` en sí ya está probada de forma directa
(`una_referencia_prueba_varios_nombres_de_variable`) para incluir las tres formas como conjunto, y
`linea_declara` hoy es un `.iter().any(...)` de una sola línea sobre ese conjunto completo, sin
ningún lugar natural donde una regresión parcial pudiera colarse sin querer — el riesgo es sobre
todo hacia futuros refactors de `linea_declara`, no sobre el código actual.

**`purgar_del_env_no_toca_el_archivo_si_la_clave_no_esta` sigue sin proteger el `if ya_estaba` en
aislamiento** (hallazgo ya reportado en la Segunda vuelta, sin cambios: la prueba no se tocó). Ya
no importa en la práctica: `purgar_del_env_no_reescribe_si_la_clave_no_esta`, nueva en esta vuelta,
cubre exactamente esa comprobación con el mecanismo correcto (espía inyectado, confirmado en la
tabla). Lo dejo anotado para que quede explícito que la prueba vieja no se «arregló», se
complementó.

### 5. Criterios de aceptación de `docs/06` sin prueba que los cubra

Actualización de la tabla de la Segunda vuelta, solo lo que cambió:

- **AC 2** («Va el último de la cadena…»): mejora, no cierra. `cadena_con` ahora tiene las dos
  decisiones que le tocaban (cableado interno y traducción a `Origen`) cubiertas con pruebas
  reales. Pero **`resolver_por_defecto()` —la función pública que usa la aplicación— sigue sin que
  la llame ninguna prueba**, y conserva una decisión propia sin cubrir: qué objeto concreto ocupa
  `entorno` y cuál `llavero` al llamar a `cadena_con` (ambos del mismo tipo `Box<dyn KeyResolver>`,
  intercambiables sin aviso del compilador). Ver hallazgo 1 del veredicto y fila residual de la
  tabla.
- **AC 5** («Ninguna clave aparece en los logs…»): sin cambios en lo que ya cubre
  `registrar_resultado_de_llavero`. La línea nueva de H2-bis (`entrada.get_password()` justo
  después de `entrada.set_password(v)`, dentro de `escribir_en_llavero`) queda sin prueba
  automática, por el mismo criterio ya aceptado para el resto de esa función (no tocar el almacén
  real) — no es un hueco nuevo de diseño, es la misma deuda ya declarada, aplicada a una línea más.
- AC 1, AC 3, AC 4 no cambiaron en esta vuelta: siguen como los dejó la Segunda vuelta (AC 1 sin
  prueba de ida y vuelta, deuda declarada y sin tocar; AC 3 y AC 4 cerrados).

### 6. Premisas que cuestiono

- **«[El script de Python] modela también `purgar_del_env_con` con un `escribir_fn` inyectado:
  con el `if ya_estaba` intacto, el contador de llamadas queda en cero... con el guard quitado...
  el `escribir_fn` inyectado sí se invoca» (`HANDOFF.md`, sección H8-bis).** Comprobé el script
  transcrito (`verificar_h1bis.py`) tal cual aparece en el `HANDOFF`, y **no contiene ningún
  `escribir_fn` inyectado ni ningún contador de llamadas**: la parte que el `HANDOFF` etiqueta como
  «H8-bis» en ese script solo compara **contenido** (`antes == despues_con_guard ==
  despues_sin_guard`), que es exactamente la comparación que la prueba **vieja**
  (`purgar_del_env_no_toca_el_archivo_si_la_clave_no_esta`) ya hacía y que no detectaba la
  mutación — el propio punto de partida de H8-bis, no una confirmación de que el mecanismo nuevo
  (el espía) funcione. La prosa describe una verificación que el código pegado no realiza.
  **Conclusión: la afirmación específica de que se «modeló y ejecutó» el mecanismo del espía
  inyectado no se sostiene con la evidencia que el propio `HANDOFF` aporta.** No cuestiono la
  conclusión final —construí y corrí yo mismo el modelo correcto (`verificar_h8h9_propio.py`,
  tabla filas 1 y 2) y confirma que la prueba real sí protege—, cuestiono que el `HANDOFF` haya
  presentado como «ejecutado» algo que, para esta pieza puntual, no lo estaba.
- **«`resolver_por_defecto` ... ya no importa: no le queda ninguna decisión propia sin cubrir,
  todas viven en `cadena_con`, que sí se prueba» (`HANDOFF.md`, sección H5-bis/H7-bis).** Falsa en
  el sentido estricto: `resolver_por_defecto` sí conserva una decisión propia —qué objeto pasa
  como `entorno` y cuál como `llavero` en su llamada a `cadena_con`— y ningún test la ejercita,
  porque ningún test llama a `resolver_por_defecto()`. **Conclusión: es el mismo hallazgo de la
  Segunda vuelta (hueco de cableado), reducido en superficie (antes eran intercambiables tres
  parámetros, ahora dos) pero no cerrado.**
- **«Ninguna prueba preexistente se reescribió en esta vuelta —solo se añadieron estas ocho—»
  (`HANDOFF.md`, «Lo que NO pude verificar»).** La comprobé con `git diff -U0` (solo las líneas
  borradas del diff acumulado) y con lectura directa de las cinco pruebas «adaptadas» y de
  `el_llavero_va_despues_del_entorno_y_del_env`/la prueba de `tracing`: coincide, ninguna se tocó.
  **Conclusión: cierta.**

### 7. Qué verifiqué y no marqué

- No repetí la investigación del mock de `keyring-core` ni releí las fuentes descargadas de
  `keyring`/`zbus-secret-service-keyring-store`/`windows-native-keyring-store`: nada de eso cambió
  en esta vuelta (el `HANDOFF` no las tocó), y ya está verificado en la Segunda vuelta contra el
  código fuente real, no solo contra la prosa.
- No profundicé de nuevo en H2-bis (`get_password` tras `set_password`), H3-bis (distinguir
  `NotFound`) ni H4-bis (escritura atómica con `rename`) más allá de lo que hacía falta para las
  pruebas de la tabla: son hallazgos del `revisor-codigo`, y su diseño es tarea de esa revisión, no
  de esta. Sí verifiqué que la prueba de H3-bis (`purgar_del_env_propaga_un_error_de_lectura_que_
  no_es_archivo_ausente`) ejercita código real con una aserción no trivial (fila 7 de la tabla).
- No verifiqué el comportamiento real de `entrada.get_password()` inmediatamente después de
  `set_password()` en un backend nativo (Credential Manager/Secret Service/Keychain): sigue sin
  poder comprobarse sin compilador ni llavero real, y el propio `HANDOFF` ya lo declara como
  decisión no verificada, no como hecho.
- No revisé `core/audio-capture` ni `app/lib`, como pedía el encargo explícitamente (tarea T-15 en
  revisión aparte).
- No apliqué ninguna mutación al árbol de trabajo real (`secretos.rs`): sin `cargo`, mutar sin
  poder compilar no genera ninguna señal de verde/rojo en Rust — por eso todas las mutaciones de la
  tabla contra código Rust real quedan como trazado manual o, donde la lógica lo permitía, como
  cálculo ejecutado en Python (columna «Resultado» de cada fila lo distingue). No hizo falta copiar
  `secretos.rs` al scratchpad porque no lo muté.
- `git status --short` al empezar y al terminar esta vuelta es idéntico (diff vacío entre las dos
  capturas guardadas en el scratchpad de esta sesión).
