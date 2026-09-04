# REVIEW — HU-05 «Claves de API en el llavero del SO»

Consolidado por el orquestador. Ninguno de los tres revisores es quien implementó.

Reportes de origen, en esta misma carpeta: `REVIEW-codigo.md`, `REVIEW-pruebas.md`,
`REVIEW-plataforma.md`. No se resumen ni se suavizan aquí: se referencian y se decide.

## 1. Hallazgos consolidados

| # | Hallazgo | Origen | Severidad | Estado |
|---|---|---|---|---|
| 1 | `secretos.rs:439-446` — `guardar_clave` escribe en el llavero y, si tiene éxito, no toca el `.env`. Pero `resolver_por_defecto()` (390-401) resuelve el `.env` **antes** que el llavero. En cualquier instalación existente —donde toda clave vive hoy en `.env`, que era el único mecanismo— cambiar una clave desde Ajustes muestra «Guardada en llavero del sistema» **mientras la aplicación sigue usando la clave vieja en cada petición**. Trazado por el revisor a lo largo de cinco archivos, de `ajustes.dart` a `secretos.rs` | `revisor-codigo` (H1) | **Bloqueante** (elevado; ver §2) | Vuelve a implementación |
| 2 | `secretos.rs:730-776` — `el_llavero_va_despues_del_entorno_y_del_env` **nunca llama a `resolver_por_defecto()`**: construye su propia `CadenaResolvers` ya en el orden correcto. La mutación que el propio plan esperaba —reordenar la cadena real— no la pondría en rojo. Ninguna prueba del repositorio invoca esa función | `verificador-pruebas` (1) | **Importante** | Vuelve a implementación |
| 3 | `secretos.rs:896-922` — `ningun_evento_de_tracing_contiene_el_valor_de_la_clave` solo puede capturar el `tracing::warn!` de la **rama de error**. Ni la rama de éxito, ni `LlaveroResolver::resolver`, ni `guardar_clave_en` emiten evento alguno. En el CI de Linux la escritura falla siempre porque no hay D-Bus, así que la prueba entra por la única rama que vigila: **la salva el accidente**. Una fuga en la rama de éxito no se detectaría nunca | `verificador-pruebas` (2) | **Importante** | Vuelve a implementación |
| 4 | Esa misma prueba llama a `escribir_en_llavero` real, así que **escribe de verdad en el Credential Manager / Keychain al correr `cargo test`** — contradiciendo la «Decisión #1» que el propio `HANDOFF` declara en otro punto | `revisor-codigo` (H2) | **Importante** | Vuelve a implementación |
| 5 | `guardar_clave()`, el punto de entrada real del AC 3, no se ejercita nunca como composición completa: solo sus piezas por separado. Mismo patrón que el hallazgo 2 | `verificador-pruebas` | **Importante** | Vuelve a implementación |
| 6 | `Cargo.toml:20` — `rust-version = "1.75"` es una promesa falsa. **No es bloqueante**: ningún workflow fija `rustc` (`dtolnay/rust-toolchain@stable` sin pin) ni usa `--locked`/`--frozen`, así que el CI no se rompe | `auditor-plataforma` (I-1) | **Importante** | A deuda: T-12 |
| 7 | `xcap` —ajeno a esta HU— arrastra `dbus` → `libdbus-sys`, que necesita `libdbus-1-dev` en Linux, **y eso no está declarado en ningún sitio**. Si hoy llega por accidente vía `apt` como transitiva de `libsecret-1-dev`, quitar `libsecret` al cerrar la deuda rompería `core/screen-capture` por un motivo ajeno al llavero | `auditor-plataforma` (I-2) | **Importante** | A deuda: T-11, con el aviso escrito |
| 8 | El `HANDOFF` dice «siete archivos» con la suposición obsoleta de `libsecret` y enumera seis. Falta `docs/01-arquitectura.md:591` | `auditor-plataforma` (I-3) | **Menor** | A deuda: T-11 |
| 9 | `secretos.rs:883` — posible `clippy::uninlined_format_args` | `revisor-codigo` (H3) | **Menor** | Se corrige en la vuelta |
| 10 | Falta una prueba de ida y vuelta para el AC 1. El `HANDOFF` lo declara; el `verificador-pruebas` apunta una vía no explorada: el backend `mock` de `keyring-core` | Ambos | **Nota** | Se evalúa en la vuelta |

## 2. Desacuerdos

**Con el `revisor-codigo`, sobre la severidad del hallazgo 1.** Él lo clasifica como Importante;
el orquestador lo eleva a **Bloqueante**. Razón: viola el invariante 5 del protocolo —un fallo no
se traga— y además le da al usuario información falsa sobre el estado de una credencial. Que no
encaje literalmente en la tabla de severidad («pierde audio, corrompe una sesión, rompe la
compilación, o produce un paquete con datos de demostración») es un defecto de esa tabla, no del
hallazgo. Se anota para revisarla.

**Con el orquestador, sobre el MSRV.** Al lanzar al `auditor-plataforma` le dije que si el MSRV
se quedaba corto sería Bloqueante. Fue a comprobarlo en vez de aceptarlo y demostró que no lo es:
ningún workflow fija `rustc` ni usa `--locked`. **Mi premisa era mala y queda corregida.** Es
exactamente lo que se le pide a un revisor.

**Con el PLAN, y la premisa es mía.** El plan afirmaba que no hacía falta migrar las claves que
ya estuvieran en un `.env` porque «se resuelve solo, ya que el `.env` conserva prioridad sobre el
llavero». El `revisor-codigo` atacó esa premisa: no se resuelve solo, **se rompe solo**. Esa
prioridad, que defendí como virtud, es la causa directa del hallazgo 1. Error del plan, no del
implementador.

**Entre revisores no hubo contradicciones.**

## 3. Qué quedó sin verificar

Los tres confirmaron B-1 de primera mano, cada uno por su cuenta, incluido el WSL. Sin
compilador no se ejecutó nada. Además:

- El texto exacto de `keyring::Error`: si alguna variante incluye el secreto en su `Display`, la
  política «error → `None`» no basta. Nadie pudo comprobarlo.
- El comportamiento de `zbus` sin sesión D-Bus, que es el caso del criterio 4.
- La compilación cruzada a Android (B-3).

Lo que sí se verificó y está conforme: la tabla `[target...]` del `Cargo.toml` no se llevó por
delante ninguna entrada (parseado con `tomllib`); las dos ramas `#[cfg]` de `LlaveroResolver` y
`escribir_en_llavero` tienen firma idéntica; `resolver_por_defecto()` y `guardar_clave` no llevan
`#[cfg]` propio; **Android queda excluido por construcción** —no por confiar en el crate—, porque
`target_os = "android"` no está en el `any(...)` y Cargo nunca añade `keyring` a ese grafo; no
hay `unwrap`/`expect`/pánico en el resolutor; los cinco tests preexistentes conservan sus
aserciones originales, adaptados con un helper que entra en pánico en vez de aceptar cualquier
variante; `puente.rs` solo cambió la línea autorizada más el doc-comment; y el ancho máximo real
de línea es 97 caracteres.

**Mención aparte, a favor del implementador:** resolvió la verificación previa bloqueante
descargando y leyendo las fuentes reales de `keyring` 4.2.0 y sus backends desde crates.io, en
vez de asumir. Confirmó que el backend de Linux por defecto es D-Bus puro y no enlaza `libsecret`,
y que Android está excluido del conjunto por defecto. Es el estándar de evidencia que este
protocolo pide y casi nunca se alcanza.

## 4. Checklist de cierre

- [ ] `cargo fmt` / `clippy` / `test` — **no ejecutables** (B-1)
- [ ] Cada AC tiene su prueba, nombrada — **no**: el AC 2 tiene una que no ejercita la función
      real (hallazgo 2), el AC 5 una que solo cubre media rama (3), el AC 3 no se prueba como
      composición (5), y el AC 1 no tiene ninguna (10)
- [ ] Cada prueba nueva se puso en rojo al mutar — **no ejecutable** (B-1); dos no podrían
      ponerse en rojo ni con toolchain
- [x] Los nombres de prueba describen el fallo que previenen, en español
- [x] `auditor-plataforma` lo cotejó, con archivo y línea
- [x] Documentación afectada actualizada en el mismo cambio

## Veredicto

**VUELVE A IMPLEMENTACIÓN.**

Un Bloqueante y cuatro Importantes, ninguno dependiente de que haya compilador.

El hallazgo 1 es el que decide: la funcionalidad **no hace lo que dice hacer** en el único
escenario que importa, que es el de un usuario que ya tiene claves. Y falla en silencio,
informando de lo contrario. La corrección natural, que respeta el criterio 2 de la HU sin dejar
la trampa: al escribir en el llavero con éxito, **retirar esa clave del `.env`**, para que no
queden dos fuentes en conflicto. Que es, además, lo que el usuario espera al «mover» una clave al
llavero.

Lo que **no** hay que rehacer: la elección del crate y su condicionamiento por plataforma están
bien resueltos y bien verificados, el contrato `#[cfg]` es correcto, Android queda protegido por
construcción, y el cambio de `PathBuf` a `Origen` es la solución acertada.

Para la vuelta, por orden: hallazgo 1; después 2, 3, 4 y 5, que son la misma familia —pruebas
que no ejercitan el código de producción, la «regla de la réplica» de `protocolo.md`—; el 9 al
paso. Los hallazgos 6, 7 y 8 salen de esta tarea y van al tablero.

Orquestador · 2026-09-03

---

# Segunda vuelta — 2026-09-03

## Lo que quedó cerrado

- **Hallazgos 2, 3, 4 y 5 (la familia de la réplica).** Se extrajeron `ensamblar_cadena_por_defecto`,
  `registrar_resultado_de_llavero` y `guardar_clave_orquestada`. El `verificador-pruebas` confirmó
  que `guardar_clave` es literalmente `guardar_clave_orquestada(...)` sin lógica propia añadida, y
  que el test de `tracing` ya no toca el llavero real y es determinista en cualquier plataforma.
- **La asimetría de `#[cfg]`.** El `auditor-plataforma` emparejó los once uno por uno y rastreó las
  54 apariciones de `keyring` en el archivo: cuatro pares completos con firma idéntica y tres
  positivos que no necesitan contraria. **Ninguna de las cuatro referencias reales al crate escapa
  de su rama**: Android sigue protegido por construcción.
- **Los permisos 0600.** `purgar_del_env` no duplica esa lógica: delega en `guardar_clave_en`, que
  ya tenía el único bloque `cfg(unix)` desde antes de esta HU. Resuelto por reutilización.
- **El mock de `keyring-core`, descartado con razón.** Los dos revisores lo verificaron por
  separado leyendo las fuentes reales del crate: el `LazyLock` de `keyring-4.2.0/src/v1.rs:107`
  con su corte temprano en `Entry::new`, y que `zbus-secret-service-keyring-store` conecta a D-Bus
  de forma síncrona y eager. Una sugerencia del propio `verificador-pruebas`, descartada con
  evidencia por el implementador y ratificada por él mismo: es el resultado que este mecanismo
  busca.

## Hallazgos nuevos

| # | Hallazgo | Origen | Severidad | Estado |
|---|---|---|---|---|
| H1-bis | **El Bloqueante original, reproducido por otra vía.** `purgar_del_env` (`secretos.rs:606-627`) compara solo contra `nombre_canonico(referencia)`, una forma. Pero `nombres_candidatos` (36-52) devuelve **tres** —`gemini`, `GEMINI`, `GEMINI_API_KEY`— y el resolutor del `.env` busca las tres. Un `.env` preexistente con `GEMINI=sk-vieja` sobrevive a la purga, y la cadena lo sigue devolviendo porque el `.env` antecede al llavero. La interfaz dice «Guardada en llavero del sistema» y la aplicación sigue usando la vieja: **el H1 original, letra por letra, sin ninguna condición de entorno especial** | `revisor-codigo` | **Bloqueante** | Tercera vuelta |
| H2-bis | La purga retira la única red de seguridad de la clave migrada: si el llavero deja de estar disponible en una sesión posterior, la clave no está en ningún sitio. Verificado contra las fuentes del crate que el riesgo es específicamente Linux/headless, no Windows | `revisor-codigo` | **Importante** | Tercera vuelta |
| H3-bis | `let Ok(previo) = read_to_string(&ruta) else { return Ok(()) }` conflaciona «archivo ausente» con **cualquier** error de lectura —permisos denegados incluidos— y lo trata como éxito. Invariante 5 | `revisor-codigo` | **Importante** | Tercera vuelta |
| H4-bis | La reescritura no atómica del `.env` es heredada, pero ahora tiene un **disparador automático nuevo**: antes solo ocurría cuando el usuario guardaba; ahora, en cada guardado con llavero disponible | `revisor-codigo` | **Importante** | Tercera vuelta |
| H5-bis | El cableado posicional entre `resolver_por_defecto` y `ensamblar_cadena_por_defecto` sigue sin proteger | `revisor-codigo` | **Importante** | Tercera vuelta |
| H6-bis | La fuga residual de `tracing` es **hallazgo abierto, no deuda aceptable**: existe una mitigación de coste casi nulo | `revisor-codigo` | **Importante** | Tercera vuelta |
| H7-bis | `resolver_por_defecto` conserva **dos** decisiones sin prueba, no una: el cableado posicional **y** la traducción de `dotenv.origen()` a `Origen::Archivo`/`Origen::Memoria` (línea 435), esta preexistente. Ninguna prueba del repositorio llama a `resolver_por_defecto()` | `verificador-pruebas` | **Importante** | Tercera vuelta |
| H8-bis | `purgar_del_env_no_toca_el_archivo_si_la_clave_no_esta` **no protege el `if ya_estaba` que dice proteger**: quitar el guard produce una reescritura idempotente byte a byte en ese escenario. El comportamiento visible queda a salvo por otra prueba, pero esta no detecta su propia mutación | `verificador-pruebas` | **Importante** | Tercera vuelta |
| H9-bis | El cuarto caso de borde de `purgar_del_env` —fallo al reescribir— sigue sin ninguna prueba | `verificador-pruebas` | **Importante** | Tercera vuelta |
| H10-bis | `README.md:78-80` afirma que el llavero «todavía no está implementado (pendiente de `libsecret`)». Será falso al cerrar esta HU. **No se toca antes de cerrar**: si la HU volviera a implementación, mentiría en la otra dirección | `auditor-plataforma` | **Nota** | Condición de cierre |

## Desacuerdos

**Con el implementador, sobre H6-bis.** Él declaró la fuga de `tracing` como deuda aceptable y lo
hizo con honestidad. El `revisor-codigo` sostiene que es hallazgo abierto porque hay una
mitigación de coste casi nulo. Decide el orquestador: **vale el revisor**. Una deuda se acepta
cuando cerrarla es caro; si es barata, se cierra.

**Entre revisores, ninguno.** El `verificador-pruebas` y el `revisor-codigo` llegaron por caminos
distintos a la misma conclusión sobre `resolver_por_defecto` (H5-bis y H7-bis son la misma raíz).

## Veredicto

**VUELVE A IMPLEMENTACIÓN.**

H1-bis lo decide solo: es el mismo Bloqueante que provocó la vuelta anterior, corregido para una
grafía y abierto para las otras dos. Es una lección concreta y vale anotarla: **la corrección se
hizo contra el ejemplo, no contra el contrato**. `nombres_candidatos` es la función que define qué
nombres cuentan como «esta clave», y `purgar_del_env` tenía que consultarla a ella, no reimplementar
una versión reducida. Es la regla de la réplica otra vez, esta vez en código de producción y no en
una prueba.

Lo que **no** hay que rehacer: la extracción de las tres funciones, el contrato `#[cfg]`, la
protección de Android, los permisos por reutilización y la decisión sobre el mock.

Orquestador · 2026-09-03

---

# Tercera vuelta — 2026-09-03

## Lo que quedó cerrado

- **H1-bis, el Bloqueante.** Confirmado corregido con evidencia propia del `revisor-codigo`: ambos
  consumidores usan `linea_declara` (`guardar_clave_en:637`, `purgar_del_env_con:760`), no hay un
  tercer sitio que reimplemente la noción, y —lo que nadie había mirado— **no borra de más**: con
  `GEMINI=x` y `GEMINI_API_KEY=y` simultáneos colapsa a una línea sin restos, y la comparación es
  por igualdad exacta, así que un proveedor cuyo nombre sea prefijo de otro no da falso positivo.
- **La relectura del llavero es fiable por diseño.** El `auditor-plataforma` reconstruyó la cadena
  entera —`keyring::Entry` → `keyring_core::Entry` → backend nativo— extrayendo `keyring-core` del
  `.crate` con `tar` porque no estaba desempaquetado. **Ningún eslabón cachea.** En Windows
  (`CredWriteW`/`CredReadW` directas) es incluso más fiable que en Linux, que depende de un demonio.
- **H3-bis, H5-bis y H6-bis** confirmados. Para H6-bis el revisor comparó el comentario aplicado
  contra el texto exacto de su propio pedido: coincide.
- **Verificación ejecutada, por primera vez en esta HU.** El `verificador-pruebas` reprodujo los dos
  scripts de Python del `HANDOFF` — pero antes los comparó línea por línea contra el Rust real,
  incluida la lógica vieja recuperada con `git diff`, para no confiar en una traducción infiel. Y
  escribió un tercero propio para un hueco que el `HANDOFF` describe en prosa pero no implementa.
  Con eso confirmó que H8-bis y H9-bis sí detectan sus mutaciones y que ninguna prueba previa se
  debilitó.

## Hallazgos nuevos

| # | Hallazgo | Origen | Severidad | Estado |
|---|---|---|---|---|
| H4-ter | **La escritura atómica empeoró la seguridad.** El temporal se crea con `std::fs::write` (permisos por defecto, típicamente legibles por grupo y otros bajo umask 022) y el `chmod 0600` se aplica **después** (`secretos.rs:681` vs `688`). Hay una ventana con el `.env` entero en claro bajo permisos abiertos, **y ahora en cada guardado**: antes la escritura era directa sobre un archivo que, tras la primera vez, ya conservaba 0600 al truncarse. Además, si `set_permissions` falla, el temporal **queda huérfano con el secreto en claro** — no hay ningún `remove_file` en todo el archivo | `revisor-codigo` · `auditor-plataforma` (P2, P4) | **Bloqueante** (elevado; ver §2) | Cuarta vuelta |
| H2-ter | `secretos.rs:328-331` — `.map(\|_\| ())` descarta el valor releído: certifica que la lectura no falla, no que coincida con lo escrito. Y el caso trazado completo: si `set_password` tiene éxito real pero la relectura falla, `escribir_en_llavero` devuelve `false`, no se purga (correcto) **pero sí se escribe la clave en el `.env`** — queda duplicada en ambos sitios, e informa `Origen::Archivo` como si el llavero hubiera fallado del todo | `revisor-codigo` | **Importante** | Cuarta vuelta |
| P1 | En Windows, si `std::fs::rename` (`secretos.rs:691`) falla porque otro proceso tiene el `.env` abierto sin `FILE_SHARE_DELETE`, y ocurre en la purga posterior a un guardado **ya exitoso** en el llavero (`secretos.rs:562`), `guardar_clave` devuelve `Err` **aunque la clave ya esté guardada**. No miente, pero deja llavero y `.env` en desacuerdo sin ningún diagnóstico que apunte a la causa | `auditor-plataforma` | **Importante** | Cuarta vuelta |
| V1 | **`resolver_por_defecto()` sigue sin ninguna prueba** (`secretos.rs:485-491`). Se extrajo `cadena_con` para cubrir el cableado, pero `resolver_por_defecto` le pasa `entorno` y `llavero` —**el mismo tipo exacto**, `Box<dyn KeyResolver>`— así que intercambiarlos compila sin aviso. Ninguna prueba del repositorio la invoca. **El hueco no se cerró: se movió un nivel más adentro**, y contradice la afirmación del `HANDOFF` de que «ya no le queda ninguna decisión propia sin cubrir» | `verificador-pruebas` | **Importante** | Cuarta vuelta |
| V2 | Las dos pruebas nuevas de H1-bis cubren **2 de las 3** formas de `nombres_candidatos`: la forma en minúsculas (`gemini`) no aparece en ningún `.env` de prueba del archivo. Demostrado **por mutación ejecutada**: una `linea_declara` que la ignorase dejaría ambas pruebas en verde. Es el Bloqueante original otra vez, un tercio más pequeño | `verificador-pruebas` | **Importante** | Cuarta vuelta |
| M1 | El nombre del temporal (`.env.tmp.<pid>`) no distingue llamadas concurrentes del mismo proceso | `revisor-codigo` | **Menor** | Cuarta vuelta |
| M2 | En Windows no hay ningún endurecimiento de permisos, ni antes ni después. Preexistente, no regresión de esta vuelta | `auditor-plataforma` (P3) | **Nota** | Al tablero |
| M3 | El `HANDOFF` (líneas 144-146, 554-556) dice que el temporal huérfano depende de que el proceso muera. Es falso: cualquier error de `rename`/`set_permissions` lo deja igual, con el proceso vivo | `auditor-plataforma` | **Menor** | Cuarta vuelta |
| M4 | `ajustes.dart:_guardar` no tiene `try/catch`, a diferencia de `_probar`, justo cuando `purgar_del_env` ha empezado a propagar errores que antes se tragaba | `revisor-codigo` | **Importante** | Al tablero: es `app/lib`, fuera del alcance de esta HU |

## Desacuerdos

**Con el `revisor-codigo`, sobre H4-ter.** Él lo clasifica como Importante; el orquestador lo eleva
a **Bloqueante**. Una fuga de credenciales en disco, recurrente, introducida por la historia que
existe precisamente para evitarlas. **Y obligó a arreglar el protocolo**: su tabla de severidad se
quedó corta dos veces el mismo día, las dos en esta HU. Ahora «expone una credencial» y «le dice al
usuario algo falso sobre el estado de sus datos o sus claves» son causales explícitas, con la regla
general escrita al lado: *si un hallazgo obliga a discutir si la tabla lo cubre, la tabla está
incompleta, no el hallazgo*.

**Entre revisores, ninguno.** El `revisor-codigo` y el `auditor-plataforma` llegaron por separado a
la ventana de permisos (H4-ter y P2/P4 son el mismo defecto visto desde dos ángulos).

## Veredicto

**VUELVE A IMPLEMENTACIÓN.** Cuarta vuelta.

Dos patrones atraviesan esta vuelta y conviene nombrarlos, porque se repiten:

1. **Arreglar un camino de error abre los caminos de error del arreglo.** H4-ter, H2-ter y P1 son
   los tres consecuencia directa de las correcciones de la vuelta anterior, no defectos originales.
   Es normal; lo que no sería normal es no revisarlos.
2. **La corrección desplaza el problema en vez de eliminarlo.** V1 es el hueco de cableado de la
   segunda vuelta, movido un nivel más adentro. V2 es el Bloqueante original, reducido a un tercio.
   En ambos casos el `HANDOFF` los da por cerrados.

Orquestador · 2026-09-03
