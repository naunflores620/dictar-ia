# REVIEW-plataforma — HU-05 «Claves de API en el llavero del SO»

Auditor de plataforma. Objeto: el contrato multiplataforma del cambio declarado en `HANDOFF.md`
sobre `core/providers/Cargo.toml`, `core/providers/src/secretos.rs`, `core/api/src/puente.rs` y
`app/lib/datos/repositorio_rust.dart`. Todo lo que sigue viene de lectura y parseo estático; nada
se compiló.

## 1. Veredicto en una línea

El contrato `#[cfg]` de esta HU está bien construido y protege a Android por construcción
(evidencia estructural, no de confianza en el crate externo); pero cierro con reserva porque no
pude ejecutar nada (B-1/B-3) y porque encontré dos riesgos de empaquetado fuera del `HANDOFF`
—uno de ellos capaz de romper Linux en silencio si alguien cierra mal la deuda que este plan dejó
abierta a propósito.

## 2. Matriz de plataformas

| Plataforma | ¿Compila, según lectura? | Qué lo verificaría de verdad | Qué lo impediría |
|---|---|---|---|
| **Linux** | Probable. `keyring = "4.2.0"` entra vía `target_os = "linux"` (`core/providers/Cargo.toml:31-32`), la rama `#[cfg]` correspondiente en `secretos.rs` usa `keyring::Entry` solo dentro de su gate (líneas 259-272, 304-317). No hay síntoma de rotura de sintaxis o de cfg mal puesto. | `cargo build -p dictar-providers -p dictar-api` en `ubuntu-24.04`. También: si se toca la deuda de `libsecret-1-dev`, verificar que `pkg-config --exists dbus-1` sigue resolviendo (ver hallazgo I-2) | Nada identificado en el diff mismo. Riesgo indirecto: que alguien borre `libsecret-1-dev` de `ci.yml`/`release.yml` sin comprobar si arrastraba `libdbus-1-dev` (necesario para `xcap`, no para esta HU) |
| **Windows** | Probable. Mismo `any(...)` incluye `target_os = "windows"`; la rama usa `keyring` sin nada específico de Windows en este archivo (el backend `windows-native-keyring-store` es interno al crate). No se toca ningún CMake para esto: no hay librería nativa nueva que empaquetar, `keyring` se enlaza estáticamente dentro de `dictar_api.dll` igual que hoy hace el resto de `core/providers` | `cargo build -p dictar-providers -p dictar-api` en Windows, y confirmar que `nucleo-windows` de `ci.yml` (que hoy solo corre `cargo check --workspace --all-targets`, línea 110) sigue en verde | Nada identificado en el diff. No verificable: esta máquina Windows tampoco tiene `cargo` (ver §5) |
| **Android** | Protegida **por construcción**, no por confianza en el crate: `target_os = "android"` no aparece en `cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))` (`Cargo.toml:31`), así que Cargo **nunca añade `keyring` al grafo de dependencias de ese target**, compile o no compile `keyring` en Android por su cuenta. La rama sin backend (`secretos.rs:274-322`) no referencia `keyring::` en ningún punto (grep completo del archivo, ver §5) y tiene la misma firma que la rama de escritorio | `cargo tree -p dictar-api --target aarch64-linux-android -e normal` filtrando `keyring` (debe no imprimir nada) sin necesidad siquiera de que compile; y luego `cargo ndk build -p dictar-api` completo | No identificado en el diff. B-3 (sin NDK ni `cargo-ndk`) impide comprobarlo incluso con `cargo` resuelto |

Ninguna de las tres tiene, en este diff, el patrón de las tres roturas históricas del proyecto
(función tras `cfg` sin su contraria, `xcap`-style sin rama Android, paquete sin núcleo dentro).

## 3. Hallazgos

### Importante

**I-1 — La condición "MSRV Bloqueante" del encargo no se cumple hoy, pero la promesa del
workspace ya es falsa.**
`Cargo.toml:20` (raíz) declara `rust-version = "1.75"` en `[workspace.package]`. Ningún workflow
fija una versión de `rustc`: `dtolnay/rust-toolchain@stable` se usa sin `with: toolchain:` en
`.github/workflows/ci.yml:27,92,121` y en `.github/workflows/release.yml:34,121` — instala lo que
sea "stable" en el momento de correr, hoy muy por encima de cualquier MSRV que declare `keyring`.
Tampoco hay `--locked` ni `--frozen` en ningún `cargo build`/`check`/`test` de esos dos archivos
(grep completo, cero resultados), así que un `Cargo.lock` sin `keyring` (ver M-2) tampoco rompe
nada ahí. Conclusión: el CI no se ve afectado. Pero la cifra `1.75` del `Cargo.toml` raíz queda
incumplida en cuanto alguien la tome en serio: un desarrollador que fije ese `rustc` exacto (vía
`rust-toolchain.toml`, que este repositorio no tiene, o vía su distro) verá fallar la compilación
de `core/providers` en Linux/Windows con el error estándar de Cargo (paquete que exige un `rustc`
más nuevo del activo), no un fallo silencioso, pero sí una promesa rota que nadie corrige. No
verificable con certeza: no tengo herramienta de red para confirmar el MSRV real de `keyring`
4.2.0 que el `HANDOFF.md:219-226` declara (1.88); lo trato como declaración no verificada, no
como hecho.

**I-2 — Riesgo no visto por el HANDOFF: `libsecret-1-dev` podría estar sosteniendo
`libdbus-1-dev` por accidente, y nadie lo comprobó.**
`Cargo.lock` (sin condicionar, tal como pide el protocolo) muestra que `xcap` — dependencia ya
existente, ajena a esta HU — trae `dbus` 0.9.12 entre sus dependencias (`Cargo.lock:3666-3681`,
entrada `dbus` en la línea 3673), y `dbus` 0.9.12 depende a su vez de `libdbus-sys`
(`Cargo.lock:571-577`), que en Linux enlaza contra el `libdbus-1` del sistema vía `pkg-config`.
Ni `ci.yml`, ni `release.yml`, ni `INSTALL.md`, ni `README.md`, ni `docs/05-empaquetado.md`
mencionan `libdbus-1-dev` en ningún punto (grep de `libdbus`/`dbus-1` en todo el repo: solo
aparece dentro de `Cargo.lock` y en la propia declaración del `HANDOFF`). Que el job `nucleo` de
`ci.yml` compile hoy en verde con solo `libsecret-1-dev` en su lista de `apt-get install` (línea
45) significa que `libdbus-1-dev` llega de otro lado: la imagen base del runner, o el propio
`apt` resolviendo `libdbus-1-dev` como dependencia transitiva de `libsecret-1-dev`. Esto no lo
comprobé con certeza (no hay `apt`/Debian real disponible en esta máquina), pero es exactamente
el tipo de suposición que ya rompió este proyecto tres veces: si es lo segundo, borrar
`libsecret-1-dev` de `ci.yml`/`release.yml` al cerrar la deuda —sin este aviso— rompería la
compilación de `core/screen-capture` en Linux, por una razón que no tiene nada que ver con el
llavero. El `PLAN.md:37-40` y el `HANDOFF.md:38-39,190-202` acotan correctamente que esa deuda no
se toca en esta HU; el punto de este hallazgo es para quien la cierre después.

**I-3 — Inventario de "siete archivos" del HANDOFF: el conteo no cuadra y falta uno real.**
`HANDOFF.md:190-202` enumera, sumados a los tres del plan (`ci.yml`, `release.yml`,
`INSTALL.md`): `packaging/linux/build_deb.sh:101`, `docs/05-empaquetado.md` (tres apariciones, un
archivo) y `README.md:133`. Contados como archivos distintos son seis, no siete (confirmé cada
cita con lectura directa: `INSTALL.md:9-14,24`, `packaging/linux/build_deb.sh:101`,
`docs/05-empaquetado.md:168,289,338`, `README.md:133`, más los dos del plan, `ci.yml:45` y
`release.yml:49`). Y falta uno real que sí encontré por mi cuenta: **`docs/01-arquitectura.md:591`**
— "Claves de API en el llavero del SO: Credential Manager (Windows), **libsecret** (Linux)" — una
afirmación arquitectónica, no de instalación, que queda tan falsa como la del `README.md:78-80`
que el propio `HANDOFF.md:204-207` sí anotó por separado. No cambia el veredicto de esta HU
(documentación, fuera de la lista de archivos), pero corrige el material que el HANDOFF deja para
el cierre de la deuda.

### Menor

**M-1 — `docs/05-empaquetado.md` está desactualizado en más ejes que `libsecret`.**
El bloque de ejemplo de `release.yml` que cita en las líneas 307-343 usa `ubuntu-22.04`,
`flutter_distributor` y las claves `platform`/`targets`, que no corresponden al pipeline real
(`ubuntu-24.04`, `dpkg-deb` vía `packaging/linux/build_deb.sh`, Inno Setup). No es un hallazgo
nuevo de esta HU ni algo que el implementador debiera haber tocado; lo anoto porque estaba leyendo
el mismo archivo para verificar la cita de `libsecret` y contextualiza que ese documento ya era
ejemplo ilustrativo, no la fuente de verdad operativa, desde antes de esta HU.

**M-2 — `Cargo.lock` no tiene `keyring` (esperado, consecuencia de B-1, no un defecto del
cambio).** Confirmado por grep: cero apariciones de `keyring` en `Cargo.lock`. Como ya se
estableció en I-1, ningún workflow usa `--locked`/`--frozen`, así que esto no rompe el CI: el
primer `cargo build` real lo resuelve y actualiza el lock solo. Se anota porque el encargo pedía
explícitamente comprobarlo.

### Sin hallazgos (verificado y conforme)

- `core/providers/Cargo.toml` parseado con `tomllib`: la tabla
  `[target.'cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))'.dependencies]`
  quedó bien anidada. `dictar-domain`, `serde`, `serde_json`, `schemars`, `reqwest`, `tokio`,
  `futures`, `async-trait`, `thiserror`, `tracing` y `toml` siguen en `[dependencies]`, y
  `[dev-dependencies]` conserva `tokio` (con `features = ["rt", "macros"]`) y `tempfile = "3"`.
  No hay ninguna entrada perdida ni desplazada — el error clásico de TOML que este repositorio ya
  cometió no se repite aquí.
- Las dos ramas de `LlaveroResolver` (`secretos.rs:239-272` vs. `274-290`) y de
  `escribir_en_llavero` (`304-317` vs. `319-322`) tienen firma idéntica: mismos tipos de parámetro
  y mismo tipo de retorno en cada par (el nombre del parámetro cambia a `_referencia`/`_valor` en
  la rama sin uso, válido en Rust y sin efecto sobre la firma).
- Invariante 4 respetada: `resolver_por_defecto()` (`secretos.rs:390-401`) y `guardar_clave`
  (`secretos.rs:439-446`, `puente.rs:399-406`) no llevan ningún `#[cfg]` propio. Seguí también la
  cadena de re-exportación: `core/api/src/lib.rs:27,130,222,266` importa `resolver_por_defecto`
  de `dictar_providers::router`, que a su vez es un simple
  `pub use crate::secretos::{resolver_por_defecto, ...}` (`core/providers/src/router.rs:19-20`) —
  no hay una segunda definición divergente escondida en `router.rs`, que es lo primero que sospeché
  al ver el import por ese camino y no por `secretos` directamente.
- Rama Android limpia: grep de `keyring` en todo `secretos.rs` (54 coincidencias) confirma que
  cada uso del crate real (`keyring::Entry::new`, líneas 267 y 308) cae dentro de una función ya
  gateada a `cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))`. Ningún
  `use keyring::...` suelto, ninguna referencia fuera de su `cfg`.
- `puente.rs:399-406`: la firma pública `guardar_clave(proveedor: String, clave: Option<String>)
  -> Result<String, String>` no cambió, tal como decía el plan; solo cambió el cuerpo
  (`.map(|origen| origen.to_string())`).
- `app/lib/datos/repositorio_rust.dart:293-303`: el comentario ya no afirma sin condición que se
  devuelve "el archivo donde quedó"; ahora distingue llavero de archivo, coherente con `Origen`.

## 4. Premisas que cuestiono

1. **"Si el MSRV se queda corto, sería un hallazgo Bloqueante" (encargo, punto 4).** La cuestiono:
   verifiqué que ningún workflow fija ninguna versión de `rustc` (`@stable` flotante en los cuatro
   jobs que usan Rust) y que ninguno usa `--locked`/`--frozen`. Con esas dos condiciones, un MSRV
   real más alto que `1.75` no rompe el CI de este repositorio hoy. Conclusión: es **Importante**,
   no Bloqueante — la promesa del `Cargo.toml` raíz queda falsa, pero no hay ninguna compuerta
   automatizada que dependa de ella todavía.

2. **"`libsecret-1-dev` sobra" (plan y HANDOFF, dado por bueno para el llavero).** La comparto
   *para lo que pide `secretos.rs`* — la evidencia de que `keyring` usa `zbus` en Linux es
   coherente con el resto del diseño del cambio. Pero la cuestiono *como conclusión de
   empaquetado completa*: nadie comprobó si `libsecret-1-dev` es hoy, por accidente de resolución
   de `apt`, lo que trae `libdbus-1-dev` a la imagen — necesario para `xcap`, no para el llavero.
   Conclusión: correcto no tocarlo en esta HU (como decidieron plan y HANDOFF), pero la nota de
   deuda debería decir explícitamente "verificar `libdbus-1-dev` antes de borrar", no solo
   "sobra". Ver I-2.

3. **"El HANDOFF encontró siete archivos con la suposición obsoleta de `libsecret`" (encargo,
   punto 6, citando al implementador).** La cuestiono en el conteo: son seis archivos distintos en
   la lista del HANDOFF, no siete. Conclusión: el número correcto de archivos afectados sí es al
   menos siete, pero por una composición distinta — falta `docs/01-arquitectura.md:591`, que el
   HANDOFF no vio.

4. **Que la exclusión de Android dependa de que `keyring` "se degrade solo" en ese target (una
   posibilidad que el propio HANDOFF descarta explícitamente, líneas 32-37).** Coincido con el
   implementador en que confiar en eso hubiera sido más frágil, y confirmé por qué: mi lectura no
   depende de creerle a `keyring` — el `cfg(any(...))` del propio `Cargo.toml` de este repositorio
   ya excluye `target_os = "android"` del grafo de dependencias, así que aunque la lectura del
   implementador sobre el comportamiento interno de `keyring` en Android estuviera equivocada, no
   importaría: Android nunca ve el crate.

## 5. Qué verifiqué y no marqué

**Parseado, no solo leído:**
- `core/providers/Cargo.toml` con `python -c "import tomllib..."` (comando y salida completa en
  la sesión) — confirmé la estructura de tablas y que ninguna clave de `[dependencies]` o
  `[dev-dependencies]` se perdió.
- No apliqué el mismo tratamiento (parseo con `pyyaml`) a `ci.yml`/`release.yml`: el encargo
  prohíbe tocar `.github/workflows` porque hay otro auditor trabajando ahí en paralelo sobre
  HU-01, así que me limité a **leerlos completos** para las comprobaciones de MSRV,
  `--locked`/`--frozen` y paquetes de sistema — sin editarlos ni ejecutar el validador de YAML
  sobre ellos, por no ser mi alcance en esta HU.

**`#[cfg]` seguidos, línea por línea:**
- `core/providers/src/secretos.rs:239-322` (las dos ramas de `LlaveroResolver` y
  `escribir_en_llavero`, con `SERVICIO_LLAVERO` como const auxiliar gateada).
- `core/providers/src/secretos.rs:174-186` (`dir_configuracion`, `cfg(windows)`/
  `cfg(not(windows))` — preexistente, no tocado por esta HU, revisado solo para contexto de
  `guardar_clave`).
- `core/api/src/lib.rs:107,112` (los únicos `#[cfg]` de ese archivo — preexistentes, ninguno
  nuevo alrededor de `router()` ni de las llamadas a `resolver_por_defecto`).
- Grep completo de `#[cfg` en `core/api/src/lib.rs` y de `keyring` en todo `secretos.rs` (54
  coincidencias, todas dentro de tests o de las dos ramas gateadas).

**Rutas de empaquetado comparadas:**
- Confirmé que esta HU no añade ninguna librería nativa nueva que empaquetar (a diferencia de
  `xcap` o `whisper.cpp`): `keyring` se enlaza en tiempo de compilación dentro de `dictar-api`
  (`.dll`/`.so`), igual que el resto de `core/providers`. Por eso no comparé
  `app/windows/CMakeLists.txt` contra `app/linux/CMakeLists.txt` en detalle — esta HU no les pide
  nada, y tocarlos no está en su lista de archivos.
- Sí comparé, dentro de `Cargo.lock` (sin condicionar, como pide el protocolo), qué paquete real
  trae `dbus`/`libdbus-sys` al árbol (`xcap`, no `keyring`), con un script `awk` que recorre cada
  bloque `[[package]]` y su lista de `dependencies` — no hay forma de hacer esa pregunta leyendo
  un solo archivo, y `Cargo.lock` por sí solo no distingue plataformas, así que el resultado se lee
  como "esto existe en el árbol de alguna plataforma", no como "esto compila en Windows".

**Otros archivos leídos para contrastar el HANDOFF:**
`INSTALL.md:1-30`, `README.md:70-140`, `packaging/linux/build_deb.sh:90-110`,
`docs/05-empaquetado.md:160-345`, `docs/01-arquitectura.md:585-594`, `.github/workflows/ci.yml`
completo, `.github/workflows/release.yml` completo, `core/api/Cargo.toml`,
`core/providers/src/router.rs` (la línea de re-exportación), `_orquestacion/tablero.md`,
`_orquestacion/historial/decisiones.md:50-76`.

**Confirmé personalmente B-1** (no me fío solo del tablero): `cargo`/`rustc` no están en el
`PATH` de esta sesión de Windows, y `wsl.exe -d Ubuntu-26.04 -- bash -lc "cargo --version"`
devuelve `command not found`. No verifiqué B-3 de forma independiente (no intenté instalar NDK);
lo doy por bueno según `tablero.md:17` y `HANDOFF.md:175-178`.

**No marqué como hallazgo** (ruido explícitamente descartado por el propio tablero): B-2 (Flutter
desactualizado) no aplica a este diff — no toqué nada de Dart más allá del comentario ya revisado.

## 6. Qué haría falta para verificarlo de verdad

- Resolver B-1 (instalar Rust ≥ lo que exija `keyring` de verdad) y correr, en Linux y Windows:
  `cargo build -p dictar-providers -p dictar-api --all-targets`,
  `cargo test -p dictar-providers`, `cargo clippy --workspace --all-targets -- -D warnings`.
- Con `cargo` resuelto pero sin necesitar NDK todavía: filtrar `keyring` en
  `cargo tree -p dictar-api --target aarch64-linux-android -e normal` — si no aparece, confirma en
  segundos, sin compilar, que el `cfg` excluye a `keyring` del grafo de Android. Es la
  comprobación más barata y la más directa para el riesgo central de esta HU.
- Resolver B-3 (NDK + `cargo-ndk`) y correr `cargo ndk -t arm64-v8a build -p dictar-api`, y luego
  el chequeo de `release.yml:179-187` a mano (que el `.so` del APK exista y el build no haya
  arrastrado nada de `keyring`).
- En una máquina Ubuntu real (no esta, que es Windows sin `apt`): `apt-cache depends
  libsecret-1-dev` y, en una rama de prueba, quitar `libsecret-1-dev` del `apt-get install` de
  `ci.yml`/`release.yml` para comprobar si `pkg-config --exists dbus-1` sigue resolviendo — antes
  de que alguien cierre la deuda de I-2 asumiendo que sobra sin más.
- Confirmar contra crates.io (con acceso de red real, que yo no tengo en esta sesión) el MSRV
  exacto de `keyring` 4.2.0 y sus backends activos por defecto, y actualizar `rust-version` en el
  `Cargo.toml` raíz (`Cargo.toml:20`) para que deje de ser una promesa falsa — no bloquea el
  cierre de esta HU, pero es deuda real que hoy nadie más anotó con la cifra concreta a corregir.
- El round-trip real de AC 1 que el propio `HANDOFF.md:184-186,231-233` admite que falta: guardar
  una clave y releerla con `LlaveroResolver` real, a mano, en un escritorio con GNOME/KDE y en
  Windows.

---

# Segunda vuelta

Objeto: `HANDOFF.md`, sección «Segunda vuelta» (líneas 8-266). El implementador declara haber
tocado únicamente `core/providers/src/secretos.rs`. Todo lo que sigue es lectura, `grep` y
parseo estático — nada se compiló. B-1 confirmado de nuevo por mi cuenta: no hay `cargo` en el
`PATH` de esta sesión. No toqué `core/audio-capture` (HU-01, en revisión aparte).

## 1. Veredicto en una línea (segunda vuelta)

Confirmo, con verificación propia e independiente —no por confiar en el adelanto—, que la
asimetría de siete `cfg` positivos y cuatro negativos en `secretos.rs` es correcta y no
compromete Android; que el único archivo tocado en esta vuelta es `secretos.rs`, tal como
declara el `HANDOFF`; y que T-11 y T-12 siguen intactos. Misma reserva que la primera vuelta,
sin novedad: nada de esto se compiló (B-1/B-3).

## 2. Matriz de plataformas (sin cambios de fondo respecto a la primera vuelta)

| Plataforma | ¿Compila, según lectura? | Qué lo impediría |
|---|---|---|
| Linux | Probable. `keyring` entra por `target_os = linux` (`core/providers/Cargo.toml:31`); las funciones nuevas de esta vuelta (`purgar_del_env`, `guardar_clave_orquestada`, `ensamblar_cadena_por_defecto`, `resultado_de_guardar`) no llevan `cfg` propio y no referencian `keyring` (ver más abajo) | Nada identificado en el diff. Sigue en pie I-2 (deuda T-11, no tocada, ver más abajo) |
| Windows | Probable. Mismo `any(...)`. Ninguna de las funciones nuevas usa rutas ni permisos específicos de Windows; `purgar_del_env` delega en `guardar_clave_en`, que ya distinguía por plataforma antes de esta HU | Nada identificado. No verificable: esta máquina Windows tampoco tiene `cargo` |
| Android | Protegida por construcción, igual que en la primera vuelta: `target_os = android` no está en el `any(...)` de `Cargo.toml:31`, así que `keyring` no entra al grafo de dependencias de ese target. Las cuatro funciones nuevas de esta vuelta son de composición pura (resolutores en caja, closures, `std::fs`, `std::path`) y no tocan `keyring` en ningún punto | No identificado en el diff. B-3 sigue impidiendo comprobarlo con `cargo ndk` |

macOS sigue sin runner en `ci.yml`/`release.yml` (ya era así antes de esta HU); el `cfg` la
incluye por paridad con el patrón de `xcap` (`core/screen-capture/Cargo.toml:17`), no porque el
proyecto la empaquete.

## 3. Hallazgos

### Comprobación principal — verificada, conforme (no confío en el adelanto, lo repetí)

Emparejé uno por uno los once `cfg` de plataforma de `secretos.rs` (grep completo de `#[cfg` con
número de línea). Resultado: siete positivos, cuatro negativos, tal como adelantó el
orquestador, y el reparto es exactamente el que propuso, sin huecos:

- Cuatro pares completos, firma idéntica en cada uno (mismos tipos de parámetro y retorno; el
  guion bajo delante del nombre de parámetro en la rama Android no cambia la firma):
  - `pub struct LlaveroResolver;` — línea 250 (positiva, cfg en la línea 249) contra línea 275
    (negativa, cfg en la línea 274).
  - `impl LlaveroResolver` con `pub fn nuevo() -> Self` — líneas 253 a 257 (cfg 252) contra
    líneas 278 a 282 (cfg 277).
  - `impl KeyResolver for LlaveroResolver` — líneas 260 a 272 (cfg 259), con
    `fn resolver(&self, referencia: &str) -> Option<String>`, contra líneas 285 a 290 (cfg 284),
    con `fn resolver(&self, _referencia: &str) -> Option<String>`.
  - `fn escribir_en_llavero(referencia: &str, valor: Option<&str>) -> bool` — líneas 305 a 313
    (cfg 304) contra líneas 335 a 337 (cfg 334), con
    `fn escribir_en_llavero(_referencia: &str, _valor: Option<&str>) -> bool`.
- Tres positivos sin contraria, los tres correctamente sin necesitarla porque ninguno se
  referencia jamás desde código sin `cfg` ni desde la rama Android:
  - `const SERVICIO_LLAVERO` (líneas 239 y 240) — solo se lee en las líneas 267 y 308, ambas ya
    dentro de ramas positivas. Sin otras referencias en todo el archivo, según el grep.
  - `fn registrar_resultado_de_llavero(resultado: keyring::Result<()>) -> bool` (líneas 326 a
    332) — función privada; sus dos únicos llamadores son la línea 312 (dentro de
    `escribir_en_llavero`, positiva) y las líneas 1139 y 1146 (dentro del test de la línea
    siguiente, también gateado). Al ser privada, ningún módulo externo puede llamarla sin pasar
    por esas dos rutas, así que no hace falta una rama Android que la sustituya.
  - El test `ningun_evento_de_tracing_contiene_el_valor_de_la_clave` (líneas 1112 a 1165, cfg en
    la 1112) — un test no necesita contraparte: en Android simplemente no se compila ni corre,
    sin que eso rompa nada más.

Grep de `keyring` en todo el archivo, 54 apariciones: descartadas las que son el texto de
convención de referencia (el prefijo `keyring:` que usan `nombres_candidatos` y `resolver`, no
el crate), las únicas cuatro referencias reales al crate —`keyring::Entry::new` en las líneas
267 y 308, el tipo `keyring::Result` en la firma de la línea 327, y `keyring::Error::NoEntry` en
la línea 1146— caen, las cuatro, dentro de una rama ya gateada a la condición positiva. Ninguna
fuga hacia código sin `cfg` ni hacia la rama negativa. No encontré nada que se le escapara al
adelanto: coincido con él, con evidencia propia.

### Verificado y conforme (sin hallazgo)

Archivo único, según lo que el diff puede probar.
`git diff --stat -- core/providers/` muestra solo dos archivos: `Cargo.toml` (+14/-0) y
`secretos.rs` (+668/-34). No hay ningún commit entre la primera y la segunda vuelta (`git log
--oneline` sobre los cuatro archivos de la lista del `PLAN.md` no devuelve nada), así que el
diff es la suma de ambas vueltas por construcción: no puede, por sí solo, aislar qué cambió solo
en la segunda. Lo que sí hice: leer el contenido completo de los otros tres diffs (`Cargo.toml`,
`core/api/src/puente.rs`, `app/lib/datos/repositorio_rust.dart`) y confirmar que ninguno contiene
una sola línea que no esté ya descrita en la tabla de archivos tocados de la primera vuelta del
`HANDOFF` (líneas 312 a 321): el `Cargo.toml` solo trae el bloque de dependencia condicionada a
`keyring`; `puente.rs` solo el cambio del mapeo a texto más el doc-comment de tres líneas;
`repositorio_rust.dart` solo el comentario de `guardarClave`. Ningún rastro de purgar, 0600 ni
orquestada en esos tres diffs. Es la mejor evidencia disponible sin un commit que marque el
límite entre vueltas; no descarta matemáticamente un cambio revertido a lo idéntico, pero no hay
motivo para sospechar ese escenario aquí.

`purgar_del_env` no reinventa los permisos: los hereda, correctamente.
`purgar_del_env` (`secretos.rs`, líneas 606 a 627) no tiene ningún `cfg` propio ni escribe el
archivo directamente: solo lee con `std::fs::read_to_string` (línea 608, portable) y, si hay
algo que limpiar, llama a `guardar_clave_en(dir, referencia, None)` (línea 624), la misma
función que ya existía antes de esta HU y que contiene el único bloque `cfg(unix)` del archivo
relacionado con permisos (líneas 583 a 588, fijando el modo 0600, sin rama para Windows porque
ahí no hay equivalente — eso ya era así antes de esta vuelta). No se olvidó nada: al reusar la
función en vez de duplicar la escritura, `purgar_del_env` hereda automáticamente el mismo
comportamiento por plataforma que ya tenía `guardar_clave_en`, sin que el propio `purgar_del_env`
tenga que saber nada de `cfg`. Es el patrón que pide el checklist: la condición vive dentro del
crate que la sufre, no repartida. Nota aparte, no un defecto: `cfg(unix)` incluye Android, así
que el `.env` de respaldo en Android también quedaría con permisos 0600 si alguna vez se escribe
— coherente con el resto del archivo, no es nuevo de esta vuelta.

`core/providers/Cargo.toml` reparseado con `tomllib`: sigue intacto.
Comando: `python -c`, cargando el archivo con `tomllib.load` en modo binario. La tabla de
dependencias condicionada a `cfg(any(target_os = linux, target_os = windows, target_os =
macos))` contiene únicamente `keyring = 4.2.0`. La tabla `[dependencies]` conserva sus once
entradas (`dictar-domain`, `serde`, `serde_json`, `schemars`, `reqwest`, `tokio`, `futures`,
`async-trait`, `thiserror`, `tracing`, `toml`) y `[dev-dependencies]` conserva `tokio`, con las
características `rt` y `macros`, y `tempfile = 3`. Nada perdido ni desplazado. Coincide con lo
que ya había confirmado la primera vuelta — esperable, porque el `HANDOFF` declara, y el diff
confirma en el punto anterior, que este archivo no se tocó en la segunda. El patrón de la tabla
condicionada es textualmente idéntico al que ya usa `core/screen-capture/Cargo.toml`, línea 17,
para `xcap`.

T-11 y T-12 no se tocaron.
El `Cargo.toml` raíz, donde vive la versión mínima de Rust declarada del workspace (I-1/T-12),
no aparece en `git status`: cero cambios. `docs/01-arquitectura.md` e `INSTALL.md` tampoco
aparecen en ningún diff. En `.github/workflows/ci.yml` y `.github/workflows/release.yml`, la
línea con `libsecret-1-dev` (I-2/T-11) aparece solo como línea de contexto sin tocar en ambos
diffs, confirmado revisando los caracteres de cada línea del diff: sin marca de línea añadida ni
eliminada. `packaging/linux/build_deb.sh` no tiene diff en absoluto. Los cuatro archivos que
sostienen T-11 y T-12 están intactos.

### Nota, fuera de esta HU y de T-11 — para quien retome la deuda

`README.md` sí tiene diff, pero no por esta HU: no toca `core/providers` ni conceptos de
llavero; su contenido, sobre qué plataformas graban audio hoy, es ajeno, y corresponde a HU-01.
Pero el texto que I-3 ya había señalado como falso sigue ahí, reescrito con una frase todavía
más incorrecta: `README.md`, líneas 79 y 80 actuales, dice hoy que el llavero del sistema es el
tercer eslabón previsto y todavía no está implementado, pendiente de `libsecret`, así que hoy
las claves quedan en texto plano en el `.env` — las tres afirmaciones son falsas después de esta
HU: está implementado, nunca dependió de `libsecret`, y con éxito en el llavero ya no queda en
texto plano. No es un hallazgo contra el implementador de HU-05 (no tocó `README.md`, y no debía
hacerlo) ni contra T-11 (nadie lo tocó por T-11). Lo anoto porque el número de línea que T-11
trae anotado en `_orquestacion/tablero.md`, línea 93, puede quedar desalineado si alguien más
sigue tocando este párrafo antes de que se cierre esa deuda.

## 4. Premisas que cuestiono

«La asimetría de siete contra cuatro es legítima» (adelanto del orquestador, con la cuota
agotada). La comparto, pero no por confianza: la verifiqué emparejando los once `cfg` uno por
uno (ver Comprobación principal, arriba) y confirmé que los tres positivos sin contraria son,
los tres, símbolos privados cuyos únicos llamadores ya están gateados. Conclusión igual a la del
adelanto, con evidencia propia añadida: el detalle línea por línea de a qué llama cada uno de
los tres sueltos, que el adelanto no traía.

## 5. Qué verifiqué y no marqué

Parseado, no solo leído: `core/providers/Cargo.toml` con `tomllib` — reparseo independiente, no
heredado de la primera vuelta.

`Cfg` seguidos, línea por línea: los once de plataforma en `secretos.rs` (líneas 239, 249, 252,
259, 274, 277, 284, 304, 326, 334 y 1112; detalle arriba en Comprobación principal), más los dos
preexistentes de `dir_configuracion` (líneas 174 y 179, sin cambios en esta vuelta, releídos
solo para confirmar que `purgar_del_env` no necesita duplicarlos) y los dos `cfg(unix)` de la
sección de escritura y de tests de permisos (líneas 583 y 817, tampoco tocados: confirmé con
`git diff` que la región de la línea 583 no aparece en ningún bloque modificado, es decir, es
idéntica byte a byte a la primera vuelta).

Rutas de empaquetado comparadas: ninguna nueva. Confirmé, buscando las palabras keyring,
secretos, llavero y providers dentro del diff de `app/windows/CMakeLists.txt`, cero
coincidencias — el único cambio a ese archivo en el árbol de trabajo es ajeno a esta HU (HU-01).
Sigue sin haber ninguna librería nativa nueva que empaquetar: `keyring` se enlaza en tiempo de
compilación dentro de `dictar-api`, igual que describía la primera vuelta.

No verificable, mismo motivo que la primera vuelta: que `secretos.rs` compile de verdad (B-1);
un `cargo tree` con destino Android para confirmar en segundos que `keyring` no entra al grafo
de ese target sin necesitar el NDK; que `keyring::Result` y `keyring::Error::NoEntry` sean la
API real de `keyring` 4.2.0 (heredo la lectura de fuentes que declara el `HANDOFF`, sin acceso
de red propio para confirmarla).

## 6. Qué haría falta para verificarlo de verdad

Lo mismo que ya pedía la primera vuelta, sin novedad: resolver B-1 y correr `cargo build` sobre
`dictar-providers` y `dictar-api` con todos los targets, y `cargo test` sobre
`dictar-providers`, en Linux y en Windows; con solo `cargo` resuelto y sin NDK, un `cargo tree`
acotado al target de Android filtrando `keyring` — sigue siendo la comprobación más barata y más
directa para el riesgo central de esta HU, y ahora cubre también las cuatro funciones nuevas de
esta vuelta, no solo las de la primera.

---

# Tercera vuelta

Objeto: `HANDOFF.md`, sección «Tercera vuelta» (líneas 8-560), y el único archivo que declara
haber tocado: `core/providers/src/secretos.rs`. El encargo de esta pasada no es el contrato
`#[cfg]` ni `Cargo.toml` —no cambiaron— sino dos operaciones que se comportan distinto según el
sistema operativo aunque no lleven ni un `#[cfg]`: la escritura atómica del `.env` (H4-bis) y la
relectura del llavero tras escribir (H2-bis). Todo lo que sigue es lectura, `grep`, `diff` de
fuentes de terceros y trazado manual — nada se compiló.

## 1. Veredicto en una línea (tercera vuelta)

El contrato de plataforma sigue intacto —cfg, `Cargo.toml`, empaquetado y T-11/T-12/`README.md`
sin tocar, verificado de nuevo por mi cuenta, no heredado— y la relectura del llavero (H2-bis) es
fiable por diseño en Linux y Windows, sin caché en ningún eslabón de la cadena hasta el backend
nativo; pero la escritura atómica (H4-bis) tiene una consecuencia específica de Windows que nadie
había trazado hasta el final: un `.env` bloqueado por otro proceso puede hacer que `guardar_clave`
devuelva error **después** de que el llavero ya guardó la clave con éxito, dejando llavero y
`.env` en desacuerdo (de forma segura, sin mentir, pero sin diagnóstico). `cargo` sigue sin estar
instalado —comprobado de nuevo en esta sesión, B-1 vigente— así que el veredicto no cambia por
esa vía.

## 2. Matriz de plataformas (sin cambios estructurales; matiz nuevo en Windows)

| Plataforma | ¿Compila, según lectura? | Qué cambió esta vuelta | Qué lo impediría |
|---|---|---|---|
| **Linux** | Sin cambios: probable, mismo `cfg`/`Cargo.toml` que la segunda vuelta | Nada de plataforma: las cuatro funciones nuevas (`nombre_de_linea`, `linea_declara`, `cadena_con`, `purgar_del_env_con`) son cadenas y `std::fs` portables, sin `keyring` (grep, ver §5). La relectura de `escribir_en_llavero` es una segunda consulta síncrona real al Secret Service, serializada por `Mutex<SecretService>` (`zbus-secret-service-keyring-store-1.0.0/src/service.rs:19-21`), no una caché | Nada identificado en el diff |
| **Windows** | Sin cambios en compilación; **nuevo matiz de runtime, no de compilación**: la escritura atómica (`secretos.rs:680-691`) puede fallar limpio si otro proceso tiene el `.env` o el temporal abiertos sin `FILE_SHARE_DELETE` (editor, antivirus indexando) — ver hallazgo P1. La relectura del llavero (`Cred::set_secret`/`get_password`, `windows-native-keyring-store-1.1.0/src/cred.rs:91-118`) es `CredWriteW`/`CredReadW` directas y locales (`utils.rs:168,238`), sin demonio ni caché: más fiable, si acaso, que en Linux | Nada identificado para compilar. **No verificable en runtime**: sin `cargo`, no se pudo forzar el escenario de archivo bloqueado ni confirmar empíricamente que `std::fs::rename` mapea a `MoveFileExW`+`MOVEFILE_REPLACE_EXISTING` (documentado, no verificado localmente: sin `rustc` ni fuente de `std` en esta máquina) |
| **Android** | Sin cambios: protegido por construcción | Confirmado de nuevo, esta vuelta: cero referencias a `keyring` fuera de rama gateada, incluidas las cuatro funciones nuevas | B-3 sigue sin resolverse para probarlo con NDK |

macOS: sin cambios respecto a la segunda vuelta (incluida por paridad de `cfg`, sin runner). Esta
vuelta no aporta evidencia nueva sobre `apple-native-keyring-store`: no está descargado en esta
máquina (ver §5).

## 3. Hallazgos

### Importante

**P1 — Windows: un `.env` bloqueado por otro proceso puede dejar el llavero y el archivo en
desacuerdo, con un error que no dice por qué.**
`guardar_clave_en` (`secretos.rs:622-694`) escribe el temporal (línea 681), opcionalmente le
aplica 0600 (líneas 683-689, solo Unix) y reemplaza el `.env` con `std::fs::rename` (línea 691).
Si el `rename` falla —en Windows, la causa más plausible es que otro proceso tenga el `.env` o el
`.env.tmp.<pid>` recién creado abiertos sin permiso de compartir borrado— el error se propaga con
`?` hasta `guardar_clave_orquestada` (`secretos.rs:550-572`). Ahí hay dos caminos:

- Si el llavero **ya tuvo éxito** y la purga es la que falla (`purgar_del_env(&dir, referencia)?`
  en la línea 562), `guardar_clave` devuelve `Err` sin llegar nunca a
  `resultado_de_guardar(en_llavero, ...)` (línea 566): **el llavero ya guardó el valor nuevo, pero
  la función reporta fallo**, y el `.env` conserva el valor viejo. Como `resolver_por_defecto()`
  antepone el `.env` al llavero, la aplicación sigue usando el valor viejo pese al error: no es
  una mentira (invariante 5 se respeta, se informa `Err`), pero es un estado inconsistente sin
  diagnóstico — el `std::io::Error` que llega es el crudo del sistema operativo.
- Si el llavero **no estaba disponible** y falla el respaldo (`guardar_clave_en` en la línea 570,
  dentro del cierre de `resultado_de_guardar`), la clave que el usuario escribió no queda guardada
  en ningún sitio — también reportado como `Err`, también sin diagnóstico específico.

Ninguna prueba de esta vuelta ejercita un `rename` que falle de verdad: las nuevas
(`purgar_del_env_propaga_el_error_si_falla_al_reescribir`, `secretos.rs:1330-1347`) inyectan el
cierre de escritura y lo hacen fallar directamente, sin pasar por `std::fs::rename`. **No
verificable sin `cargo` y sin poder abrir un `.env` real desde otro proceso en esta sesión.**

### Menor

**P2 — Los permisos 0600 se aplican después de escribir, no al crear: la ventana existe.**
`std::fs::write(&temporal, contenido)` (`secretos.rs:681`) crea y llena el temporal con el modo
por defecto del proceso (sujeto a `umask`, típicamente legible por el resto de cuentas locales)
**antes** de que `std::fs::set_permissions(&temporal, ..., 0o600)` (`secretos.rs:688`, dentro de
`#[cfg(unix)]`, líneas 683-689) lo proteja. Entre esas dos líneas la clave en texto plano es
legible por cualquier proceso local con acceso al directorio. No es una regresión de esta vuelta
—el patrón "escribir, luego proteger" ya regía sobre el `.env` directamente antes de que existiera
el archivo temporal— pero es exactamente lo que pregunta el encargo, y sigue sin resolverse.

**P3 — En Windows no hay ningún endurecimiento de permisos, ni antes ni después.**
El único bloque que ajusta permisos (`#[cfg(unix)]`, `secretos.rs:683-689`) no tiene contraparte
`cfg(windows)`. El temporal y el `.env` final quedan con la ACL heredada de
`%APPDATA%\dictar_ia`, igual que ya ocurría con el `.env` antes de esta HU: no es una regresión de
la escritura atómica, es una ausencia preexistente que la nueva mecánica no corrige ni empeora.

**P4 — El `HANDOFF` acota mal la causa del huérfano `.env.tmp.<pid>`.**
`HANDOFF.md:144-146` y `:554-556` describen el riesgo como "si el proceso muere entre escribir el
temporal y renombrarlo". `secretos.rs:680-691` tiene tres operaciones falibles con `?` (`write`,
línea 681; `set_permissions`, línea 688, solo Unix; `rename`, línea 691) y ninguna limpia el
temporal en su rama de error. Un `rename` que falla por el motivo de P1 —proceso del sistema vivo,
sin ninguna caída— deja exactamente el mismo huérfano. No cambia la severidad que el propio
`HANDOFF` ya le asignó (deuda de bajo riesgo, sin limpieza automática al arrancar); corrijo el
alcance de la causa, no el veredicto sobre si hace falta resolverla ahora.

### Sin hallazgos (verificado y conforme)

**H2-bis — la relectura del llavero es fiable por diseño, no por casualidad, en los dos backends
que se empaquetan.** Recompuse la cadena completa de delegación desde `secretos.rs:328-330`
(`entrada.set_password(v)?; entrada.get_password()`) hasta el backend nativo, con evidencia de
fuente en los dos casos, no por confianza en lo que ya declaraba el `HANDOFF`:

- `keyring::Entry::set_password`/`get_password` (`keyring-4.2.0/src/v1.rs:74-90`) delegan en una
  sola línea sobre `self.inner`.
- `keyring_core::Entry::set_password`/`get_password` (`keyring-core-1.0.0/src/lib.rs:212-215,
  261-264`) delegan en una sola línea sobre `self.inner: Arc<Credential>`, con un `debug!` de por
  medio y nada más. **Esta capa no estaba extraída en el disco** (solo el `.crate` empaquetado,
  `C:\Users\naunf\AppData\Local\Temp\keyring-core.crate`); la descomprimí al `scratchpad` de esta
  sesión con `tar -xzf` para leerla — evidencia nueva de esta vuelta, no heredada.
- Linux: `Specifier::set_secret`/`get_secret` (`zbus-secret-service-keyring-store-1.0.0/src/
  cred.rs:115-132`) hacen, cada uno, su propia búsqueda D-Bus por atributos
  (`get_unique_item`→`find_matching_items`) seguida de su propia operación; `Service` serializa
  todo con un único `Mutex<SecretService>` (`service.rs:19-21`, `.lock()` en cada método: líneas
  34, 59, 108, 118). Es una segunda consulta síncrona real al demonio, no una lectura de caché.
- Windows: `Cred::set_secret`/`get_password` (`windows-native-keyring-store-1.1.0/src/
  cred.rs:91-118`) llaman a `save_credential`/`extract_from_credential`, que son `CredWriteW`
  (`utils.rs:168`) y `CredReadW` (`utils.rs:238`) directas — API local y síncrona, sin demonio ni
  IPC de por medio. Confirmé además, con `diff` entre `windows-native-keyring-store` 1.0.0 y
  1.1.0 (ambas versiones satisfacen `version = "1"` de `keyring-4.2.0/Cargo.toml`, y sin
  `Cargo.lock` cualquiera de las dos podría resolverse), que `cred.rs` es byte a byte idéntico y
  `utils.rs` solo difiere en gatear la enumeración tras `#[cfg(feature = "search")]` —irrelevante
  aquí—: la conclusión vale para las dos versiones posibles.
- Conclusión: no hay caché en ningún punto de la cadena, en ninguno de los dos backends. El único
  vector de falso negativo que encontré y que H2-bis no cubre ya es, en Linux, que el propio bus
  de sesión o el demonio de secretos caigan en la ventana de milisegundos entre las dos llamadas
  D-Bus —estructural, no defecto de este código, y no verificable sin un Secret Service real—; en
  Windows no encontré ningún vector plausible. macOS queda sin verificar (no descargado, sin
  runner en CI de todas formas).

**El contrato `#[cfg]` no cambió.** Grep repetido de `#[cfg` en `secretos.rs`: once atributos de
plataforma en total, mismos cuatro pares y tres solitarios que confirmó la segunda vuelta,
desplazados de línea por las inserciones pero idénticos en estructura (`239, 249, 252, 259` /
`274, 277, 284` / `316, 349` / `357` / `1445`). Las cuatro referencias reales a `keyring::` en todo
el archivo (`267, 319, 350, 1479`) siguen, las cuatro, dentro de rama gateada. Las cuatro funciones
nuevas de esta vuelta —`nombre_de_linea` (`580-587`), `linea_declara` (`601-604`), `cadena_con`
(`464-475`), `purgar_del_env_con` (`740-766`)— no llevan ningún `#[cfg]` propio y no mencionan
`keyring` en ninguna línea de código (solo aparece la palabra en prosa de comentarios ajenos a
ellas). Android sigue protegido por construcción, sin cambios.

**`core/providers/Cargo.toml` reparseado con `tomllib`: idéntico a la segunda vuelta.** Mismas
tablas, mismas entradas, nada perdido. Confirmado también por `git status`/mtime: `09:11:30`,
anterior a esta vuelta (`secretos.rs` a `16:36:44` el mismo día).

**T-11 y T-12 no se tocaron, verificado por dos vías independientes de la del `HANDOFF`.**
`Cargo.toml` raíz no aparece en `git status --porcelain`; `rust-version = "1.75"` sigue en
`Cargo.toml:20`, sin cambios. `docs/01-arquitectura.md`, `INSTALL.md`,
`packaging/linux/build_deb.sh` y `docs/05-empaquetado.md` tampoco aparecen en `git status`.
`.github/workflows/ci.yml` y `release.yml` sí tienen diff —de otra tarea en curso (T-4/HU-01,
WASAPI/LLVM para Windows, sin relación con esta HU)—, pero comprobé el contenido, no solo que el
archivo esté marcado "M": la línea con `libsecret-1-dev` (`release.yml`, contexto sin marca `+`/
`-`) no fue tocada, y `ci.yml` no menciona "libsecret" en ningún punto de su diff.

**`README.md` no fue tocado por esta vuelta.** `mtime` `03/09 08:39:55` — anterior incluso a los
archivos de la segunda vuelta (`09:11-09:12`) y muy anterior a `secretos.rs` de esta vuelta
(`16:36:44`). Verificación independiente de la que hizo el propio `HANDOFF` (que aplicó el mismo
método sobre otros tres archivos, no sobre `README.md`). El párrafo que la segunda vuelta ya marcó
como falso (H10-bis: "pendiente de `libsecret`") sigue idéntico, palabra por palabra, dentro de un
diff de `README.md` que sí existe pero es enteramente de otra tarea (HU-01: estado de grabación en
Windows/Android, `SQLCipher`). Pertenece a T-3 (condición de cierre del orquestador), no a esta
tarea.

## 4. Premisas que cuestiono

1. **El comentario de `secretos.rs:677-678` ("`rename`... en Unix y en Windows sustituye el
   destino de una sola vez") es correcto pero incompleto para quien lo lea como garantía de
   éxito.** Es cierto que, cuando el `rename` tiene éxito, reemplaza el destino de una sola vez en
   los dos sistemas. Lo que el comentario no dice —y hace falta decir, porque motiva P1— es que en
   Windows el `rename` puede **fallar** si el destino o el origen están abiertos por otro proceso
   sin compartir borrado, y ese fallo, aunque limpio, puede dejar el llavero y el `.env` en
   desacuerdo. Conclusión: el comentario no miente, pero documentar solo el caso de éxito deja a
   quien lo lea sin la mitad del cuadro.

2. **`HANDOFF.md:144-146,554-556`: "el temporal puede quedar huérfano si el proceso muere".** La
   cuestiono en la causa, no en la conclusión: el huérfano no depende de que el proceso muera —
   cualquier error en `set_permissions` o `rename` lo deja igual, con el proceso vivo y
   funcionando (el escenario exacto de P1). La severidad que el `HANDOFF` le asignó (deuda de bajo
   riesgo, sin resolver ahora) sigue pareciéndome correcta; corrijo el alcance de cuándo ocurre.

3. **El encargo pedía evaluar si la relectura de H2-bis "puede dar un falso negativo" en alguno de
   los backends.** Tras trazar la cadena completa de delegación con fuente en mano —incluida
   `keyring-core`, que no estaba extraída en esta máquina hasta esta vuelta— concluyo que no: la
   relectura es fiable por diseño en Linux y en Windows, sin caché en ningún punto. La duda que
   motivaba el encargo queda resuelta a favor de la decisión que tomó el implementador en H2-bis,
   con más evidencia de la que él mismo pudo reunir sin acceso a `keyring-core` desempaquetado.

## 5. Qué verifiqué y no marqué

**Parseado, no solo leído:** `core/providers/Cargo.toml` con `tomllib` (reparseo independiente,
tercera vez sobre este archivo entre las tres vueltas, mismo resultado las tres).

**`#[cfg]` seguidos, línea por línea:** los once de plataforma en `secretos.rs` (líneas 239, 249,
252, 259, 274, 277, 284, 316, 349, 357, 1445), reconfirmados contra la segunda vuelta con grep
propio, no heredado. Verifiqué explícitamente que las cuatro funciones nuevas de esta vuelta
(`580-587`, `601-604`, `464-475`, `740-766`) están fuera de cualquier bloque `#[cfg]` y no
mencionan `keyring` en código (grep de "keyring" acotado a esos rangos: cero).

**Fuentes de terceros leídas esta vuelta, con ruta exacta:**
- `keyring-src\keyring-4.2.0\src\v1.rs` (completo) y su `Cargo.toml` (completo, para confirmar
  qué versión de cada backend fija).
- `zbus-src\zbus-secret-service-keyring-store-1.0.0\src\cred.rs`, `store.rs` y `service.rs` (los
  tres completos).
- `wnks11-src\windows-native-keyring-store-1.1.0\src\cred.rs`, `utils.rs` y `store.rs` (los tres
  completos); y `diff` contra la versión `1.0.0` ya presente en `wnks-src\...-1.0.0\src\`
  (`cred.rs` idéntico; `utils.rs` solo difiere en el gateo de `search`, irrelevante aquí).
- `keyring-core.crate`: **no estaba desempaquetado** en ninguna vuelta anterior (a diferencia de
  los otros tres, que sí tenían un directorio `-src`). Lo extraje yo (`tar -xzf ...
  keyring-core-1.0.0/src/lib.rs keyring-core-1.0.0/src/api.rs`) al `scratchpad` de esta sesión y
  leí `lib.rs` para confirmar que `Entry::set_password`/`get_password` no cachean nada. No leí
  `api.rs` con el mismo detalle (solo lo extraje); no encontré motivo para sospechar una capa de
  caché ahí dado que `lib.rs` ya resuelve la pregunta completa.
- No descargué `apple-native-keyring-store` (macOS): fuera del alcance operativo del proyecto
  (sin runner) y no lo pidió el encargo de forma prioritaria.

**Rutas de empaquetado comparadas:** ninguna nueva — confirmé, con `git diff --stat -- core/
providers/` y con `git status`, que esta vuelta no toca ningún `CMakeLists.txt` ni introduce
ninguna librería nativa nueva.

**Confirmé personalmente, de nuevo, que no hay `cargo`:** `cargo --version` y `rustc --version`
devuelven "command not found" en esta sesión de Bash; `$PATH` no contiene ningún directorio con
"cargo"; busqué `cargo.exe` en `$USERPROFILE/.cargo/bin` (no existe) y en todo `C:\Users` con
`find -iname cargo.exe` (cero resultados); `$CARGO_HOME`/`$RUSTUP_HOME` no están definidas. No
reintenté arrancar WSL (el `HANDOFF` ya reportó que esta vez no arranca). Coincide con
`_orquestacion/tablero.md` (bloqueo B-1, "verificado el 03/09").

**No verificable, esta vuelta:** el escenario de P1 (rename fallando por archivo bloqueado) de
forma empírica; que `std::fs::rename` de Rust mapee de verdad a `MoveFileExW` con
`MOVEFILE_REPLACE_EXISTING` en Windows (conocimiento documentado ampliamente, no confirmado
localmente: sin `rustc` ni fuente de `std` en esta máquina); el comportamiento real de
`Specifier::set_secret`/`get_secret` contra un Secret Service real (gnome-keyring, KWallet).

## 6. Qué haría falta para verificarlo de verdad

- Resolver B-1 y, en una máquina Windows real: reproducir P1 abriendo el `.env` del usuario de
  prueba con un editor (o simulando un antivirus con un `FileStream` sin `FILE_SHARE_DELETE`) y
  disparando `guardar_clave` con el llavero disponible, para confirmar que el error se propaga
  limpio y medir si el llavero queda con el valor nuevo mientras el `.env` conserva el viejo.
- En un Linux de escritorio con Secret Service real (gnome-keyring o KWallet corriendo): ejercitar
  `escribir_en_llavero` de verdad y confirmar con logging temporal que la búsqueda de
  `get_unique_item()` tras el `set_secret` encuentra el ítem sin reintentos ni demoras.
- Descargar y leer `apple-native-keyring-store` para completar el tercer backend, aunque el
  proyecto no lo empaquete hoy — el `cfg(any(...))` de `Cargo.toml` sí lo incluye.
- Lo que ya pedían las dos vueltas anteriores, sin cambios: `cargo build`/`test`/`clippy` en
  Linux y Windows sobre `dictar-providers`/`dictar-api`, y `cargo tree` acotado a Android
  filtrando `keyring`.
