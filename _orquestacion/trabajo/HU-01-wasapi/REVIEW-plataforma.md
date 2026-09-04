# REVIEW-plataforma — HU-01 «Grabar en Windows (WASAPI loopback)»

Revisor: `auditor-plataforma`. Contra `PLAN.md` (revisión 2) y `HANDOFF.md`, ambos declaraciones a
verificar. Alcance: `core/audio-capture/src/{sincronia.rs,wasapi_src.rs,lib.rs}`,
`core/audio-capture/Cargo.toml`, `.github/workflows/ci.yml`. No toqué `core/providers`,
`core/api` ni `app/lib/datos` (territorio de HU-05/T-3, en curso en paralelo). No hice commit.

## 1. Veredicto

**El contrato de `cfg` y de empaquetado está bien construido y es estáticamente consistente en
las tres plataformas — con una reserva seria: la prueba que dice cubrir la rama `NoSoportada`
para Android/macOS no verifica el `#[cfg]` real de `lib.rs`, sino una copia manual y
desconectada de esa política, así que la garantía que el PLAN y el HANDOFF dan por resuelta
sigue, en la práctica, dependiendo solo de lectura estática como la mía.** Nada de lo que depende
de compilar se pudo comprobar: no hay `cargo` en esta máquina (confirmado abajo).

## 2. Matriz de plataformas

| Plataforma | ¿Compila? | Qué lo impediría / qué lo respalda |
|---|---|---|
| **Linux** | NO VERIFICABLE (sin `cargo`). Por lectura: sin obstáculo nuevo. `sincronia.rs` no tiene dependencias de plataforma y compila con lo que ya está en `[dependencies]`; la sección `[target.'cfg(target_os = "linux")'.dependencies]` de `Cargo.toml` queda intacta (confirmado parseando, ver §5); `wasapi_src` no se declara en este target (`core/audio-capture/src/lib.rs:20-21`) | Ninguno detectado por lectura |
| **Windows** | NO VERIFICABLE (sin `cargo`, sin caché de registro, sin máquina con tarjeta de sonido). Es el mayor riesgo real de esta entrega, y coincide con lo que el propio implementador señala en `HANDOFF.md:145-173`: la API exacta de `windows 0.58` (firma de `CoInitializeEx`, parámetro de `CoTaskMemFree`, orden de `IAudioClient::Initialize`/`IAudioCaptureClient::GetBuffer`, existencia de `PWSTR::to_string()`) no se pudo cotejar contra el crate real | Cualquier desajuste de firma en `windows` 0.58 rompería la compilación; ni las *features* declaradas (`Win32_Media_Audio`, `Win32_System_Com`, `Win32_Foundation` en `core/audio-capture/Cargo.toml:22-26`) se pudieron confirmar como suficientes |
| **Android** | NO VERIFICABLE (sin NDK, sin `cargo-ndk`, bloqueo B-3). Por lectura: sin obstáculo nuevo introducido por esta HU. `windows` vive bajo `[target.'cfg(windows)'.dependencies]` (no en `[dependencies]`), así que Cargo no la resuelve para `aarch64-linux-android`; `lib.rs::iniciar`/`dispositivos` caen en la rama `#[cfg(not(any(target_os = "linux", target_os = "windows")))]` (`lib.rs:177`, `196`), que sí incluye Android porque Android es `target_os = "android"`, no `"linux"` ni `"windows"` | Ninguno detectado por lectura. El llamador (`core/api/src/grabacion.rs:111`) invoca `dictar_audio::iniciar(cfg)` sin `#[cfg]` propio, así que el contrato está donde debe estar: dentro del crate que sufre la condición |
| **macOS** | Fuera de la matriz del proyecto (ni `ci.yml` ni `release.yml` construyen para macOS). Por lectura: cae en la misma rama `NoSoportada` que Android, sin fisura | No aplica |

## 3. Hallazgos

### Alto

**H1 — La prueba que debía cubrir la rama `NoSoportada` de una tercera plataforma no ejercita el
`#[cfg]` real; ejercita una copia manual y desconectada de la política.**
`core/audio-capture/src/sincronia.rs:111-121` define `enum Plataforma { Windows, Linux, Otra }` y
`fn tiene_backend(p: Plataforma) -> bool`, con el comentario explícito: «No es un
`#[cfg(target_os)]`: es la misma política expresada como dato» (línea 105). El único uso de estos
dos elementos, fuera de su propia definición, es el test de las líneas 340-344
(`fuera_de_windows_y_linux_sigue_devolviendo_no_soportada`), que llama a `tiene_backend` con
valores del enum **escritos a mano** (`Plataforma::Windows`, `Plataforma::Linux`,
`Plataforma::Otra`), no derivados de `cfg!(target_os = ...)` ni de ningún otro mecanismo que los
ate a los atributos reales de `core/audio-capture/src/lib.rs:167,172,177,186,191,196`. Confirmé
con `grep` que `Plataforma`/`tiene_backend` no se usan en ningún otro sitio del crate.

Consecuencia concreta: si alguien debilita el `cfg` real de `lib.rs` — por ejemplo, cambia
`#[cfg(not(any(target_os = "linux", target_os = "windows")))]` por
`#[cfg(not(target_os = "linux"))]`, que es **exactamente** el error histórico que este rol existe
para atrapar (Android confundido con «no-Linux» cuando en realidad hay que distinguirlo de
Windows también, o cualquier variante que reintroduzca una laguna) — este test seguiría en verde,
porque no lee `lib.rs` en absoluto. La objeción 2 del contradictor, que el PLAN dice «Aceptada»
(`PLAN.md:164`), pedía que las pruebas de esta HU **se ejecuten de verdad**; lo consiguen para la
aritmética de `sincronia.rs`, pero no para la política de enrutado de plataformas, que sigue sin
tener ninguna prueba automatizada real — solo la lectura que hago yo, hoy, y la que haga el
próximo auditor la próxima vez que alguien toque ese `cfg`.

No es un defecto que rompa la compilación en ninguna plataforma (por lectura, el `cfg` real de
`lib.rs` está bien, ver §2). Es un defecto de **verificación**: da una falsa sensación de
cobertura automatizada donde no la hay, en el módulo que el propio PLAN describe como «la razón
de que exista» esta separación (`PLAN.md:69-71`).

### Medio

**H2 — `Cargo.lock` todavía no tiene la arista `dictar-audio → windows`.**
`grep -n '^name = "dictar-audio"' -A 11 Cargo.lock` muestra las dependencias actuales de
`dictar-audio` en el lockfile: `dictar-domain, hound, libspa, pipewire, rubato, tempfile,
thiserror, tracing` — sin `windows`. Es la consecuencia esperada de B-1 (nadie corrió `cargo` para
regenerarlo) y no atribuible al implementador; lo marco para que quien lo verifique con un
`cargo` real sepa que el primer `cargo build`/`check` en Windows va a tener que resolver esa
arista por primera vez. No hay `--locked` ni `--frozen` en `ci.yml` ni en `release.yml` (grep sin
resultados), así que debería resolverse solo — pero es NO VERIFICABLE sin `cargo`. `windows
0.58.0` ya está en el árbol global vía `xcap` (`Cargo.lock:3450-3452`, y el bloque de `xcap` la
lista entre sus dependencias), lo que respalda el comentario de
`core/audio-capture/Cargo.toml:18-20` («no añade una segunda versión del crate»), pero esa
segunda versión (`windows 0.57.0`, vía `sysinfo`, `Cargo.lock:3440-3442`) coexiste igual en el
árbol — no es un conflicto para `dictar-audio` porque `0.58` es la que se pide explícitamente, y
Cargo puede resolver ambas versiones simultáneamente para crates distintos; lo señalo solo para
que quede constancia de que **sí hay dos versiones de `windows` en el árbol global**, aunque
ninguna la vaya a compartir con la otra.

**H3 — El comentario de cabecera de `release.yml:12-18` queda desactualizado por esta HU.**
Dice literalmente: «La captura de audio solo está implementada para PipeWire
(`core/audio-capture/src/lib.rs`: `iniciar()` devuelve `NoSoportada` fuera de Linux). Falta WASAPI
*loopback* en Windows». Ya no es cierto tras esta HU. Tanto `PLAN.md:148-149` como
`HANDOFF.md:217-221` lo declaran deuda de quien **cierre** la HU, no del implementador — no está
en su lista de archivos — así que no es un hallazgo contra la entrega, pero si nadie lo actualiza
al cerrar, el propio pipeline de release queda mintiendo sobre su propio estado, que es la
categoría exacta de fallo silencioso que motivó la falla histórica #3 de este proyecto.

### Bajo / informativo

**H4 — Atribución de `ci.yml` no reconstruible desde `git diff`.**
`git diff -- .github/workflows/ci.yml` muestra el job `nucleo-windows` **completo** (checkout,
toolchain, `cargo check --workspace --all-targets` y el paso nuevo `cargo test -p dictar-audio`)
como adición nueva contra `HEAD`, no solo el paso de tests que el PLAN asume preexistente
(`PLAN.md:65`: «`cargo test` en el job de Windows, **que hoy solo hace `cargo check`**»). Esto es
consistente con lo que ya declara `_orquestacion/tablero.md`: «Rama por tarea, suspendida […] el
árbol tiene 24 archivos modificados sin commitear de trabajo anterior» — no hay forma de separar,
con las herramientas de este repositorio hoy, qué fracción del job es de esta HU y cuál es trabajo
previo no commiteado. Lo que sí pude validar es el **estado final** (§5), que es correcto y
válido. Lo mismo ocurre con `core/audio-capture/src/lib.rs`, cuyo diff contra `HEAD` incluye,
además del enrutado a tres ramas de esta HU, la función `mezclar()` y el `#[cfg]` de
`reproductor_stub.rs`, que no pertenecen a esta HU según su propio `HANDOFF.md:40-42` — y en
efecto no lo son: son trabajo anterior, sin commitear, que ya estaba en el árbol.

**H5 — Riesgo autoconfesado, sin nada que yo pueda añadir por lectura.**
`wasapi_src.rs` es código `unsafe` contra COM escrito «de memoria», sin poder verificar el crate
`windows` 0.58 real (`HANDOFF.md:145-173`). No encontré ninguna inconsistencia estructural nueva
en una lectura línea a línea (el `unsafe` está contenido en llamadas COM puntuales, no en bloques
grandes; `ComGuard` desempareja `CoInitializeEx`/`CoUninitialize` en `Drop`,
`wasapi_src.rs:464-479`; `CoTaskMemFree` se llama en todos los caminos de salida de
`abrir_cliente`, incluido el de error, `wasapi_src.rs:335-357`), pero confirmar que **compila**
exige el compilador que no está disponible aquí. Ninguna prueba en el repositorio invoca
`wasapi_src::iniciar` ni `wasapi_src::dispositivos`: la cobertura de este archivo es cero en
tiempo de ejecución, tal como el propio `HANDOFF.md:212-216` reconoce sin rodeos.

## 4. Premisas que cuestiono

1. **Premisa (PLAN, respuesta al contradictor #2): «`sincronia.rs` sin `#[cfg]` de plataforma
   resuelve que las pruebas de esta HU se ejecuten de verdad».**
   Cierto para las 5 pruebas de aritmética/política pura (`normalizar_a_f32`, `muestras_de_relleno`,
   `pistas_a_grabar`), falso para la sexta: `fuera_de_windows_y_linux_sigue_devolviendo_no_soportada`
   prueba un enum paralelo (`Plataforma`/`tiene_backend`), no el `#[cfg]` real de `lib.rs` (H1).
   **Conclusión:** la cobertura de la política de enrutado de plataformas para Android/macOS
   sigue sin verificación automatizada; el HANDOFF debería decirlo así en vez de dar a entender
   que el test la cubre.

2. **Premisa (comentario en `Cargo.toml:18-20`): «0.58 y no otra: es la que ya fija `Cargo.lock`
   […] así que no añade una segunda versión del crate al árbol».**
   Verifiqué que es cierta en el sentido estricto (no añade una *tercera*), pero hay que matizarla:
   el árbol global **ya tenía dos versiones** de `windows` antes de esta HU (0.57.0 vía `sysinfo`,
   0.58.0 vía `xcap`), y `Cargo.lock` **todavía no incluye** la arista `dictar-audio → windows`
   porque nadie lo regeneró (H2). **Conclusión:** el comentario es una intención correcta, no una
   evidencia comprobada; la evidencia real solo existirá tras el primer `cargo build`/`check` en
   Windows.

3. **Premisa implícita del HANDOFF: «acotar `cargo test` a `-p dictar-audio` en el job de Windows
   basta para cumplir el objetivo de que las pruebas de esta HU corran».**
   Cierto para las pruebas de `sincronia.rs` (compilan y se ejecutan tanto en `nucleo` vía
   `cargo test --workspace` en Linux, como en `nucleo-windows` vía `cargo test -p dictar-audio`
   en Windows — confirmado leyendo ambos jobs, §5). **Falso** si se lee como «esto ejercita
   `wasapi_src.rs`»: ese archivo se **compila** en ambos pasos de Windows (`check` y `test`), pero
   ninguna prueba lo **invoca** (H5). **Conclusión:** «las pruebas corren en Windows» es cierto
   solo para la parte pura; la cobertura del código COM real sigue siendo cero, y conviene que el
   HANDOFF no deje ambigüedad ahí (de hecho no la deja — lo reconoce en su sección «Deuda que
   dejo» — pero vale remarcarlo porque es justo el tipo de matiz que se pierde al leer rápido).

## 5. Qué verifiqué y no marqué

**Seguido, `#[cfg]` por `#[cfg]`:**
- `core/audio-capture/src/lib.rs:17-21` (declaración de módulos `pipewire_src`/`wasapi_src`) y
  `:166-200` (`iniciar()`/`dispositivos()`): confirmé que las tres ramas —
  `#[cfg(target_os = "linux")]`, `#[cfg(target_os = "windows")]`,
  `#[cfg(not(any(target_os = "linux", target_os = "windows")))]` — son exhaustivas y no se
  solapan, y que la rama de reserva cubre Android correctamente (Android es `target_os =
  "android"`, no coincide con ninguna de las dos primeras condiciones).
- Comparé firma por firma `pipewire_src::iniciar`/`dispositivos()` (`pipewire_src.rs:62`, `:356`)
  contra `wasapi_src::iniciar`/`dispositivos()` (`wasapi_src.rs:59`, `:418`): mismo parámetro
  (`cfg: CaptureConfig`, o ninguno) y mismo tipo de retorno
  (`Result<(Receiver<AudioFrame>, Box<dyn CaptureSession>)>` y `Result<Vec<DeviceInfo>>`
  respectivamente) en ambas. La función pública de `lib.rs` no cambia de firma con la plataforma:
  el `#[cfg]` está dentro del cuerpo, en bloques, no en la firma de la función.
- Confirmé con `grep -n 'cfg(' core/audio-capture/src/sincronia.rs` que el único `cfg` del archivo
  es `#[cfg(test)]` (línea 216); ningún `#[cfg]` de plataforma, y ningún `use crate::wasapi_src`.
- Revisé el helper `Plataforma`/`tiene_backend` (`sincronia.rs:103-121,339-344`) y confirmé por
  `grep` que no se usa fuera de su propio test (H1).

**`Cargo.toml`, parseado, no leído:**

    python -c "import tomllib,json;print(json.dumps(tomllib.load(open('core/audio-capture/Cargo.toml','rb')),indent=1,default=str))"

Confirmé: `dictar-domain`, `thiserror`, `tracing`, `hound`, `rubato` en `[dependencies]`;
`pipewire`, `libspa` en `[target.'cfg(target_os = "linux")'.dependencies]`; `windows` (con sus
tres *features*) en `[target.'cfg(windows)'.dependencies]`, no en `[dependencies]`;
`[dev-dependencies]` con `tempfile` intacto detrás de la tabla nueva. `git diff` confirma que la
inserción no se llevó nada por delante: la única adición es el bloque `[target.'cfg(windows)'...]`
entre las dependencias de Linux y `[dev-dependencies]`.

**`Cargo.lock`, tratado como no-evidencia salvo para lo puntual:**
`grep -n '^name = "dictar-audio"' -A 11 Cargo.lock` (sin `windows` todavía, H2);
`grep -n '"windows"' Cargo.lock` y lectura del bloque `[[package]] name = "xcap"` para confirmar
que `windows 0.58.0` entra vía `xcap` y que además convive con `windows 0.57.0` (vía `sysinfo`) en
el árbol global — ninguna de las dos cosas prueba nada sobre lo que resolverá `dictar-audio`.

**Workflow, parseado, no leído:**

    python -c "import yaml;d=yaml.safe_load(open('.github/workflows/ci.yml',encoding='utf-8'));print(list(d['jobs']))"

→ `['nucleo', 'nucleo-windows', 'app']`, sin error de sintaxis. Listé los pasos de ambos jobs
Rust. Confirmé: `nucleo` corre `cargo test --workspace` en `ubuntu-24.04` (compila y ejecuta
`sincronia.rs`, ya que no tiene `#[cfg]`); `nucleo-windows` corre `cargo check --workspace
--all-targets` y luego `cargo test -p dictar-audio` en `windows-latest` (compila `wasapi_src.rs`
y ejecuta las pruebas de `sincronia.rs` otra vez, ahí sí compilado para Windows). `grep` sin
resultados para `--locked|--frozen|--offline` en `ci.yml`.

**Cruce a Android:**
Parseé `core/api/Cargo.toml` (`dictar-audio = { path = "../audio-capture" }`, sin condición) y
hice `grep` de `dictar_audio::` en `core/api/src/`: la llamada en `core/api/src/grabacion.rs:111`
(`dictar_audio::iniciar(cfg)`) no lleva `#[cfg]` propio — el contrato vive dentro del crate que
sufre la condición, como debe ser. Leí el job `android` de `release.yml:116-193`: no invoca
`cargo` directamente sobre `dictar-audio`; lo hace transitivamente vía `cargo-ndk` desde Gradle al
construir el APK, y ya tiene un paso que falla si `libdictar_api.so` no queda dentro del APK
(`release.yml:179-187`) — anterior a esta HU, sin tocar por ella.

**Qué NO marqué (depende de compilar; confirmado sin `cargo` disponible):**

    $ cargo --version && rustc --version
    bash: cargo: command not found

(confirmado en esta sesión, coincide con lo que ya reporta `HANDOFF.md:49-57` y el bloqueo B-1 de
`_orquestacion/tablero.md:15`). No verificable, por tanto:
- Que `wasapi_src.rs` compile contra `windows` 0.58 real (H5).
- Que las *features* `Win32_Media_Audio`, `Win32_System_Com`, `Win32_Foundation` alcancen para
  `IMMDeviceEnumerator`, `IMMDevice`, `WAVEFORMATEXTENSIBLE`, etc.
- Que `cargo fmt --all -- --check` y `cargo clippy -p dictar-audio -- -D warnings` pasen (el
  propio HANDOFF admite no haberlos podido correr, `HANDOFF.md:186-192`).
- Que `cargo build --release --workspace` (el paso real de `release.yml:64`, sin `--target-dir`
  ni matriz de targets Windows explícita más allá del runner nativo) compile de punta a punta en
  `windows-latest` — es la primera vez que ese paso, para esta HU, tendría que enlazar código COM
  real, no solo la rama `NoSoportada`.
- Cualquier comportamiento en tiempo de ejecución de WASAPI (AC 1, 2, 5, 6 de la HU): no hay
  Windows con tarjeta de sonido en este entorno.
- Que `cargo ndk build -p dictar-api` compile para `aarch64-linux-android` sin que `dictar-audio`
  introduzca un error nuevo (bloqueo B-3).

No revisé en profundidad `core/audio-capture/src/mezcla.rs`, `wav.rs`, `reproductor.rs` ni
`reproductor_stub.rs`: el propio `HANDOFF.md:40-42` declara que esta HU no los tocó, y el `git
diff` de esos archivos contra `HEAD` (156 líneas movidas entre los tres) es trabajo previo sin
commitear, no de esta HU (H4). Sí confirmé el punto de contacto necesario: `sincronia.rs:200-201`
llama a `self.remuestreador.vaciar()`, y el `Remuestreador::vaciar(&mut self) -> Result<Vec<f32>>`
que existe hoy en el árbol de trabajo de `mezcla.rs` (diff, no commiteado) tiene esa firma exacta
— la dependencia cruzada entre ambos archivos es consistente, aunque el archivo en sí no sea mío
para auditar en detalle.

## 6. Qué haría falta para verificarlo de verdad

1. **Resolver B-1**: instalar Rust 1.75+ (lo exige `rust-version` del workspace) en una máquina
   con acceso al registro de `crates.io`, y correr, en este orden, exactamente lo que pide
   `HANDOFF.md:65-73`: `cargo fmt --all -- --check`, luego
   `cargo check -p dictar-audio --target x86_64-pc-windows-msvc` (el paso que más importa, porque
   ahí aparece cualquier error de firma en `wasapi_src.rs` sin pagar el resto del build),
   `cargo clippy -p dictar-audio --all-targets -- -D warnings`, `cargo test -p dictar-audio`.
2. **Regenerar `Cargo.lock`** (con `cargo generate-lockfile` o el primer `cargo build`) y volver a
   parsear la arista `dictar-audio → windows` para confirmar que resuelve `0.58.0` sin traer una
   segunda copia solo para este crate.
3. Una **máquina Windows con tarjeta de sonido real** (o virtual, tipo Steinberg/VB-Cable) para
   ejercitar AC 1, 2, 5 y 6, que ni el PLAN ni el HANDOFF dan por comprobables aquí.
4. Resolver B-3 (NDK + `cargo-ndk`) para correr `cargo ndk build -p dictar-api` y confirmar que el
   cruce a Android sigue compilando tras esta HU, más allá de la lectura estática de §2.
5. Cerrar H1 con una prueba real: la única forma honesta de probar el `#[cfg]` de `lib.rs` para
   una tercera plataforma sin compilar tres veces es, o bien un job de CI que compile para un
   `--target` no soportado (p. ej. `x86_64-apple-darwin` con `cargo check`, sin runner, solo para
   forzar la rama), o bien reformular la política como una función que reciba el resultado de
   `cfg!(target_os = "linux")`/`cfg!(target_os = "windows")` como parámetros en vez de un enum
   escrito a mano — así el test sí quedaría atado a los `cfg!` reales, aunque siga sin poder
   probar la rama `wasapi_src`/`pipewire_src` en sí.


---

## Segunda vuelta (2026-09-03)

Re-auditoría acotada, no desde cero: contra `HANDOFF.md` (sección «Segunda vuelta») y los nueve
hallazgos consolidados en `REVIEW.md` de esta carpeta, donde mi H1 fue uno de los que provocó la
vuelta. Alcance: `core/audio-capture/src/{sincronia.rs, wasapi_src.rs, lib.rs}`. Confirmé con `git
diff` que `core/audio-capture/Cargo.toml` y `.github/workflows/ci.yml` no cambiaron respecto de lo
ya auditado en la primera vuelta — mismo diff, línea por línea (ver §5). No toqué `core/providers`,
`core/api` ni `app/lib` (territorio de HU-05, en su segunda vuelta en paralelo). No hice commit.

### 1. Veredicto de esta vuelta

**H1 está resuelto en su núcleo: la réplica desconectada desapareció sin dejar rastro, y en su
lugar hay dos pruebas que ejercitan `iniciar()`/`dispositivos()` reales bajo el complemento exacto
—carácter por carácter— de las ramas reales de `lib.rs`, con Android cayendo donde debe. La reserva
que queda no es un defecto nuevo: es la misma limitación estructural que ya señalaba como pendiente
en mi informe anterior — esa prueba sigue sin ejecutarse en ningún job de `ci.yml`, y el propio
equipo lo declara sin rodeos.** La superficie COM nueva de `dispositivos()` no introduce, por
lectura, ninguna dependencia de plataforma mal condicionada. Nada de esto se pudo compilar: sigue
sin haber `cargo` ni NDK en esta máquina (reverificado de primera mano, §5).

### 2. Matriz de plataformas (segunda vuelta)

| Plataforma | ¿Compila? | Qué cambió desde la primera vuelta / qué lo respalda |
|---|---|---|
| **Linux** | NO VERIFICABLE (sin `cargo`). Por lectura: sin obstáculo nuevo. `sincronia.rs` sigue sin ningún `#[cfg]` de plataforma (único `cfg` del archivo: `#[cfg(test)]`, `sincronia.rs:211`); `wasapi_src` no se declara en este target (`lib.rs:20-21`). La superficie COM nueva de `wasapi_src.rs` es irrelevante aquí: ese archivo entero sigue sin compilarse en Linux | Ninguno |
| **Windows** | NO VERIFICABLE (sin `cargo`, sin máquina con tarjeta de sonido). Mismo riesgo estructural de la primera vuelta (API real de `windows` 0.58 sin cotejar) más la superficie COM nueva de `dispositivos()`/`enumerar_flujo` (`wasapi_src.rs:489-566`): `EnumAudioEndpoints`, `IMMDeviceCollection::{GetCount, Item}`. Por lectura, todos los símbolos nuevos se importan del mismo módulo `windows::Win32::Media::Audio` (`wasapi_src.rs:30-34`) que ya usaban, sin cuestionamiento en la primera vuelta, `IMMDeviceEnumerator`/`IMMDevice` — mismo patrón de traducción DWORD→`u32`, out-param→`Result<T>` que el resto del archivo. `Cargo.toml` no se tocó (confirmado, §5) y sigue declarando exactamente las mismas tres *features* | Ni las firmas exactas de `EnumAudioEndpoints`/`IMMDeviceCollection` en `windows` 0.58 real, ni que `Win32_Media_Audio` alcance sin necesitar una *feature* adicional |
| **Android** | NO VERIFICABLE (sin NDK, bloqueo B-3). Por lectura: sin obstáculo nuevo. `lib.rs:177` (`iniciar`) y `lib.rs:196` (`dispositivos`) siguen enrutando Android (`target_os = "android"`, no `"linux"` ni `"windows"`) a `NoSoportada`. Novedad: dos pruebas reales (`lib.rs:415-420`, `426-431`) ejercitarían esa rama si se compilaran para una tercera plataforma — antes no había ninguna que pudiera. Confirmado: ningún job de `ci.yml` compila para un `target_os` distinto de Linux o Windows (§3). El job `android` de `release.yml:116-193` sí compila, transitivamente vía `cargo-ndk`, el código de *producción* de esa misma rama — pero no el test (§3, H6) | Ninguno detectado por lectura |
| **macOS** | Sin cambios: fuera de la matriz del proyecto, cae en la misma rama `NoSoportada` sin fisura | No aplica |

### 3. Hallazgos de esta vuelta

**H1 (revisado) — RESUELTO: la prueba de la rama `NoSoportada` ya ejercita el `#[cfg]` real.**
Confirmé con `grep` sobre todo el árbol que ni `enum Plataforma` ni `fn tiene_backend` existen ya
en ningún archivo de código: las únicas coincidencias que quedan son comentarios que explican, en
pasado, por qué se borraron (`sincronia.rs:389-396`, `lib.rs:397-407`) y menciones en documentos de
revisión o en la definición del agente `implementador` (`.claude/agents/implementador.md:65`) que
citan el error histórico como ejemplo. No queda código muerto.

En su lugar, `lib.rs:415-420` y `lib.rs:426-431` definen dos pruebas que llaman a `iniciar(...)` y
`dispositivos()` a secas (resuelven al mismo símbolo que `crate::iniciar`/`crate::dispositivos`,
por el `use super::*;` de `lib.rs:236`), cada una gateada con
`#[cfg(not(any(target_os = "linux", target_os = "windows")))]`. Comparé ese `#[cfg]`, carácter por
carácter, contra las ramas reales que verifican: es idéntico al de `lib.rs:177` (dentro de
`iniciar()`) y `lib.rs:196` (dentro de `dispositivos()`). No es un cfg "parecido": es el mismo
texto. Esto cierra lo que el hallazgo original describía — debilitar el `#[cfg]` real (el error
histórico de este proyecto) ya no puede dejar el test en verde sin que este lo note, porque el test
llama a la función real, no a una copia.

**Matiz que sigue abierto, y que el propio equipo declara sin rodeos** (`lib.rs:409-411`,
`HANDOFF.md:145-149`): con la matriz de `ci.yml` de hoy, ningún job compila para una plataforma
donde `target_os` no sea `"linux"` ni `"windows"`, así que este test no se ejecuta en ningún sitio
todavía. Confirmé leyendo `.github/workflows/ci.yml` completo: tres jobs — `nucleo` (`ci.yml:12`,
`ubuntu-24.04`), `nucleo-windows` (`ci.yml:69`, `windows-latest`), `app` (`ci.yml:121`, Flutter,
`ubuntu-24.04`) — ninguno con un tercer `target_os`. No es un hallazgo nuevo: es la confirmación de
que el punto 5 de mi «qué haría falta» de la primera vuelta sigue vigente.

Severidad: **cerrado** (era Importante). Sin regresión.

**H6 (nuevo, Bajo/informativo) — "no compila en ningún job" es impreciso si se lee sin acotar a
`ci.yml`.** El comentario de `lib.rs:409-411` y el `HANDOFF.md:145-149` dicen, con distintas
palabras, que esa rama "no compila en ningún job [existente]". Leído junto a `ci.yml` es exacto.
Leído en sentido amplio —todo el repositorio— no lo es del todo: el job `android` de
`release.yml:116-193` construye el APK con `flutter build apk --release --split-per-abi
--target-platform android-arm64` (`release.yml:174`), que invoca Gradle y este, a su vez,
`cargo-ndk` (instalado en `release.yml:145-148`, con el target `aarch64-linux-android` declarado en
`release.yml:126`) para compilar `dictar-api`, que depende de `dictar-audio` sin condición
(confirmado en la primera vuelta). Eso **sí** compila, para Android de verdad, el código de
*producción* de la rama `#[cfg(not(any(target_os = "linux", target_os = "windows")))]` de
`iniciar()`/`dispositivos()` — cuando ese job corre.

Lo que ese job **no** hace es compilar ni ejecutar el módulo `#[cfg(test)] mod tests { ... }` de
`lib.rs:234`: `flutter build apk` termina en un `cargo ndk build` (modo *build*, no *test*), y
`#[cfg(test)]` excluye ese módulo entero de cualquier compilación que no sea explícitamente de
pruebas. La conclusión práctica del HANDOFF sigue siendo correcta —ningún `assert!` de esas dos
pruebas se ejecuta hoy en ninguna parte—, pero por una razón más precisa que "no compila para
Android": sí compila (el código real, no el test), y además solo en tags `v*` o disparo manual
(`release.yml:3-6`), no en cada *push* o *pull request* como `ci.yml`. No cambia ninguna decisión ni
exige acción; lo dejo para que ese comentario, si se vuelve a tocar, no dé a entender más de lo que
es cierto.

**Reconfirmación sin cambios — H2 y H5 originales.** `Cargo.lock` sigue sin la arista
`dictar-audio → windows`: `grep -n '^name = "dictar-audio"' -A 12 Cargo.lock` da la misma lista que
en la primera vuelta (`dictar-domain, hound, libspa, pipewire, rubato, tempfile, thiserror,
tracing`, sin `windows`), y `windows` 0.57.0/0.58.0 siguen coexistiendo sin cambios en el árbol
global (`Cargo.lock:3440-3453`). Coherente con que `Cargo.toml` no se tocó esta vuelta. El riesgo
autoconfesado de COM sin verificar (H5) crece en superficie, no en naturaleza: `enumerar_flujo`
(`wasapi_src.rs:520-566`) añade `EnumAudioEndpoints`/`IMMDeviceCollection::GetCount`/`Item` al mismo
tipo de riesgo ya aceptado, con la misma disciplina de manejo de errores que el resto del archivo —
el `PWSTR` de `GetId()` se libera con `CoTaskMemFree` en todos los caminos, incluido el de error de
`to_string()` (`wasapi_src.rs:542-544`, mismo patrón que la corrección del hallazgo 8 original en
`id_de_endpoint_por_defecto`, `wasapi_src.rs:579-581`), y un fallo al enumerar una dirección no
vacía la lista de la otra (`wasapi_src.rs:503-505`, `510-512`). No encontré ninguna inconsistencia
estructural nueva por lectura línea a línea.

### 4. Premisas que cuestiono

1. **Premisa (`lib.rs:409-411`, `HANDOFF.md` §4): "con la matriz de CI actual [...] esta rama no
   compila en ningún job existente".** Cierta para `.github/workflows/ci.yml`. Incompleta si se lee
   sin acotar: el job `android` de `release.yml` sí compila, transitivamente, el código de
   *producción* de esa rama cuando corre (H6). **Conclusión:** la urgencia no cambia —ninguna
   prueba automatizada ejecuta el `assert!` hoy, en ningún lado—, pero conviene que, si se vuelve a
   tocar ese comentario, diga "en `ci.yml`" y no "en ningún job": alguien podría leerlo como que el
   enrutado de Android no se comprueba en absoluto, cuando sí se comprueba, parcialmente y de forma
   esporádica.

2. **Premisa (`HANDOFF.md` §7, `wasapi_src.rs:481-488`): "ambos tipos [`EnumAudioEndpoints`,
   `IMMDeviceCollection`] están bajo `Win32_Media_Audio` [...] así que no hace falta tocar
   `Cargo.toml`".** Coherente con lo único verificable sin compilador: los cuatro símbolos se
   importan del mismo `use windows::Win32::Media::Audio::{...}` (`wasapi_src.rs:30-34`), y el `git
   diff` de `Cargo.toml` confirma que no se tocó (§5). **Conclusión:** intención razonable y
   consistente con el único patrón disponible por lectura, no evidencia comprobada — igual que mi
   premisa 2 de la primera vuelta sobre la versión de `windows`. Es la afirmación concreta que más
   vale comprobar primero en cuanto haya `cargo`, antes que el resto del archivo.

3. **Premisa implícita: "borrar el enum `Plataforma` y sustituirlo por pruebas gateadas con el
   mismo `#[cfg]` que la función real cierra el hallazgo del todo".** Cierto para lo que el hallazgo
   pedía —que la prueba deje de ser una réplica desconectada— y lo verifiqué de forma directa
   (§3). **Falso** si se lee como "ya hay cobertura automatizada de esa rama": sigue sin haberla,
   por la matriz de CI, no por el diseño de la prueba. El propio HANDOFF no cae en esta lectura
   optimista (lo dice explícito en «Deuda que dejo»); esto es una confirmación de que el matiz está
   bien puesto, no una corrección.

### 5. Qué verifiqué y no marqué

**`cfg` seguidos, en esta vuelta:**
- `sincronia.rs` completo (397 líneas): único `cfg(` es `#[cfg(test)]` en la línea 211 (`grep -n
  'cfg(' core/audio-capture/src/sincronia.rs`). Ningún `#[cfg]` de plataforma, ninguna referencia a
  `wasapi_src`. Sin regresión.
- `lib.rs:17-21` (declaración de módulos) y `:166-200` (`iniciar()`/`dispositivos()`): las tres
  ramas — `target_os = "linux"` (167, 186), `target_os = "windows"` (172, 191),
  `not(any(target_os = "linux", target_os = "windows"))` (177, 196) — siguen exhaustivas, sin
  solapar, sin cambios.
- `lib.rs:415-420` y `:426-431`: los dos `#[cfg]` de las pruebas nuevas, comparados carácter por
  carácter contra `lib.rs:177` y `:196` (idénticos).
- `wasapi_src.rs` completo (606 líneas, antes 483): `grep -n 'cfg('` sin resultados — el archivo
  entero sigue dependiendo únicamente del `#[cfg(target_os = "windows")]` de `lib.rs:20`, sin gateo
  interno propio.
- `core/api/src/grabacion.rs:111` (`dictar_audio::iniciar(cfg)`) y `grep` de
  `dictar_audio::(iniciar|dispositivos)` en `core/api/src/`: sigue sin `#[cfg]` propio del lado del
  llamador. El único `#[cfg(windows)]`/`#[cfg(not(windows))]` de `core/api` (`core/api/src/lib.rs:
  107,112`) resuelve la carpeta de datos por defecto (`APPDATA` vs `XDG_DATA_HOME`), sin relación
  con audio ni con esta HU, y es una pareja exhaustiva correcta en sí misma; no lo audité más allá
  de descartar que fuera relevante aquí.

**`Cargo.toml`, parseado, no leído, para confirmar el estado final (no solo el diff):**

    python -c "import tomllib,json;print(json.dumps(tomllib.load(open('core/audio-capture/Cargo.toml','rb')),indent=1,default=str))"

Confirmé: `[dependencies]` sin `windows`; `[target.'cfg(target_os = "linux")'.dependencies]` con
pipewire/libspa; `[target.'cfg(windows)'.dependencies]` con `windows = "0.58"` y exactamente las
mismas tres *features* de la primera vuelta; `[dev-dependencies]` con tempfile intacto. Idéntico a
la primera vuelta.

**`git diff`, para verificar la declaración del implementador sobre `Cargo.toml`/`ci.yml`:**

    git diff -- core/audio-capture/Cargo.toml   # 10 líneas insertadas, 0 eliminadas
    git diff -- .github/workflows/ci.yml         # 52 líneas insertadas, 0 eliminadas

Comparé el contenido de ambos diffs, línea por línea, contra lo ya documentado en la primera vuelta
(el bloque `[target.'cfg(windows)'.dependencies]` completo y el job `nucleo-windows` completo): es
el mismo texto. Confirmo la declaración del `HANDOFF.md` — no hay ningún cambio oculto en ninguno
de los dos archivos respecto de lo ya auditado.

**`Cargo.lock`, tratado como no evidencia salvo para lo puntual:**
`grep -n '^name = "dictar-audio"' -A 12 Cargo.lock` (sin `windows`, igual que en la primera vuelta)
y `grep -n '^name = "windows"$' -A 3 Cargo.lock` (0.57.0 y 0.58.0, ambas sin cambios). No indica
nada sobre lo que resolvería `dictar-audio` con un `cargo` real.

**Workflows, releídos completos, no solo el diff:**
`.github/workflows/ci.yml` completo (136 líneas): tres jobs, `nucleo` (Linux), `nucleo-windows`
(Windows), `app` (Flutter/Linux); ninguno con un tercer `target_os`.
`.github/workflows/release.yml` completo (194 líneas): confirmé el disparador (`push: tags: ["v*"]`
o `workflow_dispatch`, líneas 3-6, no en cada *push*/PR), el job `android` (116-193) y que compila
`dictar-audio` transitivamente vía `cargo-ndk`/Gradle (145-148, 174) sin ningún paso `cargo test` en
todo el job (H6).

**Ausencia de herramientas, reverificada de primera mano en esta sesión:**

    $ cargo --version
    bash: cargo: command not found
    $ rustc --version
    bash: rustc: command not found
    $ which cargo-ndk
    (sin resultado)

Sin `~/.cargo` ni `CARGO_HOME` en esta máquina (confirmado con `ls`), así que tampoco hay caché de
registro de `crates.io` para cotejar la API real de `windows` 0.58 por ningún otro medio que leer
el propio código y razonar sobre el IDL, como ya hacía el HANDOFF. No verificable, por tanto, lo
mismo que en la primera vuelta más lo nuevo de esta:
- Que `wasapi_src.rs` compile contra `windows` 0.58 real, ahora incluida la superficie de
  `enumerar_flujo`.
- Que la *feature* `Win32_Media_Audio` alcance para `EnumAudioEndpoints`/`IMMDeviceCollection` sin
  necesitar una adicional (premisa 2, §4).
- Que las dos pruebas nuevas de `lib.rs` compilen y pasen para una plataforma real donde
  `target_os` no sea `"linux"` ni `"windows"` (nadie las ha podido ejecutar, ni yo).
- Todo lo demás ya listado como no verificable en la primera vuelta (AC 1, 2, 5, 6; `cargo
  fmt`/`clippy`; el cruce a Android con `cargo ndk build -p dictar-api`), sin cambios.

No repetí la auditoría de `mezcla.rs`, `wav.rs`, `reproductor.rs`/`reproductor_stub.rs`,
`core/providers` ni `core/api` más allá del punto de contacto puntual de arriba: no están en la
lista de archivos de esta vuelta (`HANDOFF.md`, «Archivos tocados») y, para
`core/providers`/`core/api`/`app/lib`, el encargo de esta ronda los excluye explícitamente por ser
territorio de HU-05 en su propia segunda vuelta, en paralelo.

### 6. Qué haría falta para verificarlo de verdad (actualizado)

Sin cambios de fondo respecto de la primera vuelta (sigue haciendo falta B-1 y, para Android, B-3),
con una precisión nueva en el punto 4:

1. **Resolver B-1** e ir directo a `cargo check -p dictar-audio --target x86_64-pc-windows-msvc`:
   con la superficie COM nueva de `enumerar_flujo`, es el paso que más vale correr primero, antes
   que el resto del archivo (ya revisado en la primera vuelta).
2. **Regenerar `Cargo.lock`** y confirmar la arista `dictar-audio → windows` (sin cambios, H2 sigue
   abierto).
3. **Una máquina Windows con tarjeta de sonido** para los AC que siguen sin poder probarse aquí,
   ahora incluido si `dispositivos()` de verdad enumera más de un dispositivo cuando los hay (AC 4).
4. **Para que las dos pruebas nuevas de `lib.rs` se ejecuten de verdad**, hace falta que algún job
   compile con `cargo test`/`cargo check --all-targets` (no `build`) para un `target_os` que no sea
   `"linux"` ni `"windows"`. Dos vías, de menor a mayor costo:
   - **Más barata:** un paso de `cargo check -p dictar-audio --tests --target x86_64-apple-darwin`
     (o cualquier *target* instalable con `rustup target add` sin necesitar un runner de esa
     plataforma) en el job `nucleo` existente. `cargo check` no enlaza el binario final, así que no
     debería necesitar un *linker* nativo de esa plataforma para un crate sin `build.rs` como
     `dictar-audio` — expectativa razonada, no confirmada. Compilaría el módulo de tests (incluidas
     las dos pruebas nuevas), aunque no lo ejecutaría: sigue sin correr el `assert!`, solo confirma
     que compila para una tercera plataforma.
   - **Más cara, y la única que ejecuta el `assert!` de verdad:** un runner nativo de una tercera
     plataforma (`macos-latest`, fuera de la matriz de plataformas soportadas por el producto, o un
     emulador Android vía alguna acción de emulador, que además exige resolver B-3 primero) con
     `cargo test -p dictar-audio --target <ese-target>`.
   Ninguna de las dos está hecha hoy, y ninguna es exigible a esta HU en concreto — misma conclusión
   de mi informe anterior, con la vía más barata ahora precisada.
5. **Resolver B-3** para `cargo ndk build -p dictar-api`, confirmando que el `.so` resultante para
   `aarch64-linux-android` no cambió de forma anómala por la superficie COM nueva de Windows (no
   debería, al vivir bajo `cfg(target_os = "windows")`, pero es del tipo de cosa que solo un build
   real confirma).
