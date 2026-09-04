# REVIEW-codigo — HU-05 «Claves de API en el llavero del SO»

Revisor: `revisor-codigo`. Fuente: `git diff` sobre los cuatro archivos declarados en
`PLAN.md` (`core/providers/Cargo.toml`, `core/providers/src/secretos.rs`,
`core/api/src/puente.rs`, `app/lib/datos/repositorio_rust.dart`), leidos completos, no solo el
diff. `HANDOFF.md` se trato como declaracion, no como evidencia: cada afirmacion suya que se cita
abajo fue comprobada de forma independiente (grep del repositorio, lectura de los archivos
reales, conteo de caracteres por linea).

## 1. Veredicto

**NO VERIFICABLE por B-1** (no hay `cargo`/`rustc`/`rustfmt`; nada de lo que depende de compilar,
formatear o ejecutar se pudo comprobar). En revision estatica: **3 hallazgos Importantes**
(uno de ellos con escenario reproducible verificado cruzando cinco archivos, no solo sospecha) y
**0 Bloqueantes**. Ningun `tracing::` nuevo ni ningun camino hacia `puente.rs` expone el valor de
una clave, truncada o completa, hasta donde la lectura estatica permite afirmarlo.

## 2. Hallazgos

### Importante

**H1 - Guardar una clave nueva no la activa si ya habia una vieja en `.env`; la interfaz dice lo
contrario de lo que realmente pasa.**

`core/providers/src/secretos.rs:439-446` (`guardar_clave`) intenta el llavero primero y, si
tiene exito, devuelve `Ok(Origen::Llavero)` sin tocar el `.env` (ver `resultado_de_guardar`,
lineas 415-424: la rama `respaldo()`, la unica que escribe el archivo, solo se invoca si
`en_llavero` es `false`). Pero `resolver_por_defecto()` (lineas 390-401) resuelve el `.env`
antes que el llavero. Combinacion:

1. Instalacion existente: antes de esta HU el `.env` era el unico mecanismo, asi que
   practicamente todo usuario con una clave configurada la tiene en
   `~/.config/dictar_ia/.env`.
2. El usuario abre Ajustes, escribe una clave nueva (por ejemplo para rotar una filtrada) y
   guarda.
3. `escribir_en_llavero` tiene exito, `guardar_clave` devuelve `Origen::Llavero`,
   `core/api/src/puente.rs:399-405` lo convierte en el texto "llavero del sistema", y
   `app/lib/pantallas/ajustes.dart:167` muestra un SnackBar con "Guardada en llavero del
   sistema".
4. `widget.onCambio()` (linea 162 de `ajustes.dart`) dispara `_recargar()`
   (`ajustes.dart:37`), que vuelve a pedir `proveedoresInfo()`, que llama a
   `listarProveedores()`, que en `puente.rs:379-390` (`listar_proveedores`) llama a
   `core/api/src/lib.rs:221-236` (`estado_proveedores`), que usa el mismo
   `resolver_por_defecto()` y sigue devolviendo el valor del `.env`, porque ese eslabon se
   consulta antes que el llavero.
5. La tarjeta del proveedor (`ajustes.dart:264`, `Clave configurada · ${p.origenClave}`) sigue
   mostrando la ruta del `.env`, y todas las peticiones siguientes (`router()` en
   `core/api/src/lib.rs:129-131`, reconstruido en cada uso) siguen usando la clave vieja del
   `.env`, no la que el usuario acaba de guardar.

El usuario ve "Guardada en llavero del sistema" y, en la misma pantalla, "Clave configurada ·
/ruta/al/.env" con el valor antiguo activo. Si la clave que se esta reemplazando estaba
comprometida, sigue en uso sin que nada lo indique. Esto no es un caso limite exotico: es el
camino de migracion por defecto para cualquier instalacion que ya exista, exactamente la
poblacion a la que la HU ("que mis claves de API no esten en texto plano") mas deberia proteger.

El `PLAN.md` (seccion "Analisis") ya anticipaba el mecanismo -"migrar claves que ya esten en un
.env ... se resuelve solo, porque el .env conserva prioridad sobre el llavero"- pero lo trato
como algo neutro. No lo es: "se resuelve solo" describe el comportamiento, no arregla el hecho
de que la interfaz confirma una accion (guardar/rotar) que no tiene efecto, y no hay ninguna
prueba que cubra esta interaccion entre `guardar_clave` y el `.env` preexistente (las pruebas
nuevas de `secretos.rs` prueban `resolver_por_defecto` con dobles aislados, o
`escribir_en_llavero` en aislamiento, pero ninguna combina "ya hay algo en el .env" + "se guarda
algo distinto en el llavero" + "que devuelve la cadena completa despues").

No lo marco Bloqueante porque no encaja en ninguna de las cuatro categorias del protocolo
(audio, sesion, compilacion, paquete con datos de demo), pero el efecto practico es que ningun
usuario migrando desde una instalacion existente puede sacar su clave del texto plano usando la
interfaz, que es el proposito declarado de la historia.

**H2 - La prueba nueva de `tracing` escribe de verdad en el llavero real de la maquina que corre
`cargo test`, exactamente lo que el propio HANDOFF dice haber evitado en otro punto.**

`core/providers/src/secretos.rs:896-921`
(`ningun_evento_de_tracing_contiene_el_valor_de_la_clave`) llama directamente a
`escribir_en_llavero("keyring:prueba-tracing", Some(secreto))` (linea 909), que en Windows y
macOS habla con el backend real (Credential Manager / Keychain): no hay ningun doble ni
aislamiento. La propia "Decision que se aparta del PLAN" numero 1 del `HANDOFF.md` explica por
que `guardar_clave_en` se diseno para no tocar el llavero en los tests: "si lo hiciera, los tests
... escribirian de verdad en el llavero real de la maquina que ejecuta cargo test -en Windows, el
Credential Manager esta siempre disponible- ... y dejarian de comprobar lo que dicen comprobar,
ademas de ensuciar un almacen real en cada corrida". Esa misma razon se aplica sin excepcion a
esta prueba nueva, que si llama a la funcion real. Cada `cargo test -p dictar-providers` en un
Windows con Credential Manager (o un Linux de escritorio con sesion y demonio de secretos -el
propio comentario de las lineas 782-786 del archivo admite que esto pasa en la maquina de un
desarrollador con GNOME-) deja o sobrescribe una entrada llamada `PRUEBA-TRACING_API_KEY` bajo el
servicio `dictar_ia` en el almacen real del sistema operativo, sin limpiarla despues. No es una
fuga de una clave real (el valor es un literal de prueba), pero si es exactamente la clase de
contaminacion de un almacen real, y de estado global mutable compartido entre ejecuciones
paralelas de `cargo test`, que el propio archivo dice evitar en el comentario de
`guardar_clave_en` (lineas 451-456: "los tests corren en paralelo, y mutar XDG_CONFIG_HOME desde
varios a la vez hace que se pisen entre ellos"). Aqui el mecanismo es el mismo, solo que sobre el
llavero en vez del entorno, y no se evito.

**H3 - Sospecha fundada, no ejecutable: posible aviso de `clippy::uninlined_format_args` en
`secretos.rs:883`.**

`write!(self.0, " {}={:?}", field.name(), value)`: el segundo `{:?}` corresponde a `value`, un
identificador simple del parametro (`value: &dyn std::fmt::Debug`), inlineable a `{value:?}`. El
resto del codigo nuevo y el preexistente en este mismo modulo (`router.rs:133`, con
`format!("{id}: {e}")`; `router.rs:98`, con el formato de `"no tiene clave configurada"`) sigue
sistematicamente el estilo interpolado, lo que sugiere que este lint (parte del grupo `style`,
activo por defecto en clippy desde hace varias versiones estables) esta efectivamente encendido
en este workspace. Con `cargo clippy --workspace --all-targets -- -D warnings` en CI (leido, no
tocado), si el lint dispara aqui la compuerta de clippy falla. No pude ejecutar clippy (B-1), asi
que esto queda como sospecha fundada con una linea y una correccion concretas
(`write!(self.0, " {}={value:?}", field.name())`), no como hecho comprobado.

## 3. Premisas que cuestiono

**Premisa 1 (del `PLAN.md`, seccion "Analisis"): "migrar claves que ya esten en un .env ... se
resuelve solo, porque el .env conserva prioridad sobre el llavero".**

Conclusion: la premisa describe correctamente el mecanismo pero es enganosa sobre su efecto. "Se
resuelve solo" sugiere que no hace falta ninguna accion; lo que realmente ocurre es que la accion
de guardar desde la interfaz queda sin efecto para cualquier clave que ya viviera en el `.env`, y
la interfaz lo confirma como si hubiera funcionado (ver H1). El patron "la fuente de mayor
prioridad enmascara una escritura nueva en una de menor prioridad" ya existia antes de esta HU
(entorno por encima de archivo), pero afectaba solo a quien exportara variables de entorno a
mano, un caso raro. Con esta HU, la fuente que enmascara pasa a ser el `.env`, que es el unico
lugar donde vivia cualquier clave anterior a esta HU. El radio de impacto pasa de "casi nadie" a
"cualquiera que actualice desde antes de esta HU", y el plan no distingue esa diferencia de
escala al descartar el caso.

**Premisa 2 (del `PLAN.md`, "Respuesta al contradictor" 2.4, repetida en el `HANDOFF.md`):
"una lectura de llavero son pocos milisegundos".**

Conclusion: no verificada, y el propio caso que el plan marca como "el mas probable en la
practica" (seccion "Casos del dominio", "Sin llavero disponible") es justo el que mas la pone en
duda. `resolver_por_defecto()` se reconstruye en cada `router()` y cada `estado_proveedores()`
(`core/api/src/lib.rs:129-131,221-222`), asi que sin sesion grafica el `LlaveroResolver` real se
invoca en cada arranque de proveedor sin clave en entorno o `.env`. Si `zbus` sin
`DBUS_SESSION_BUS_ADDRESS` falla rapido (conexion rechazada), la afirmacion se sostiene; si
intenta autolanzar un bus o espera algun timeout de protocolo, no. El propio `HANDOFF.md` ("Lo
que NO pude verificar") lo senala como abierto y no lo resuelve. La decision "se acepta sin
cache" descansa sobre un supuesto de latencia que nadie midio, en el escenario que el propio plan
considera mas comun. No propongo revertir la decision -cachear romperia la propiedad que defiende
el comentario de `core/api/src/lib.rs:127-128`- pero la premisa de que es gratis necesita una
medicion antes de darla por buena.

## 4. Que verifique y no marque

- **Cadena de resolucion y su orden** (criterio 2): lei `resolver_por_defecto()` completo
  (`secretos.rs:390-401`) y confirme que el orden de alta es `Entorno`, luego `origen` (`.env`),
  luego `Origen::Llavero`: el llavero es el tercer `.con(...)`, sin reordenar los dos primeros
  respecto del codigo anterior a esta HU (comparado contra el `git diff`, que no toca esas dos
  lineas salvo para anadir la tercera).
- **Panico en el resolutor** (criterio 4): grep de `.unwrap()`, `.expect(`, `panic!` en
  `secretos.rs` limitado a las lineas 1-534 (todo lo anterior a `#[cfg(test)] mod tests`, que
  empieza en la 535): cero resultados. `LlaveroResolver::resolver` usa `.ok()?` seguido de
  `.get_password().ok()`; no hay ninguna rama que propague con `?` sobre un `Result` ni que
  desenvuelva con `.unwrap()`/`.expect()`. `escribir_en_llavero` usa `if let Err(e) = &resultado`
  y `resultado.is_ok()`, nunca desenvuelve.
- **Fuga de la clave por `tracing`**: grep de `tracing::` en `core/providers`, `core/api`,
  `core/stt` completos. El unico `tracing::` nuevo es `secretos.rs:314`
  (`tracing::warn!(error = %e, ...)`), con el mismo patron que ya usa `router.rs:204`
  (`error = %e`, nunca el valor que causo el error). No hay ningun `{:?}` ni `{}`, en codigo
  nuevo o afectado, sobre una variable que contenga el valor de la clave (`v`, `valor`, `clave`,
  `secreto`); si hay `{otro:?}` sobre `Origen` (linea 645, en un test), que solo puede contener
  una ruta de archivo o una variante sin datos, nunca el secreto.
- **Camino hasta Flutter** (`Result<String, String>` de `puente.rs`): trace que el unico `Err`
  posible de `dictar_providers::secretos::guardar_clave` es un `std::io::Error` originado en
  `guardar_clave_en` (creacion de directorio, escritura del archivo, permisos), nunca un
  `keyring::Error`, porque `escribir_en_llavero` convierte su `Result` en `bool` internamente y
  jamas lo propaga. Confirmado tambien por el test existente `el_texto_del_error_no_contiene_la_
  clave` (`secretos.rs:824-841`), que fuerza el error por la via del sistema de archivos, no del
  llavero.
- **`Debug`/`Display` sobre tipos con la clave**: `Origen` (`derive(Debug, Clone, PartialEq,
  Eq)`) solo guarda `PathBuf` para el caso `Archivo`; nunca el valor. `LlaveroResolver` es una
  unidad sin campos. Ningun tipo nuevo deriva `Debug` sobre algo que retenga el secreto.
- **Consistencia `PathBuf` a `Origen`**: lei `guardar_clave` y `guardar_clave_en` completos.
  `guardar_clave` intenta el llavero y cae al `.env` solo si falla (`resultado_de_guardar`);
  `guardar_clave_en` nunca toca el llavero (decision 1 del `HANDOFF.md`, coherente con el
  codigo). El respaldo al `.env` sin llavero sigue funcionando exactamente igual que antes de
  esta HU: la unica diferencia es que `Ok(ruta)` paso a `Ok(Origen::Archivo(ruta))`.
- **Los 5 tests preexistentes adaptados**: compare cada uno contra su version anterior en el
  diff. El unico cambio es envolver la llamada en el helper `ruta_de(...)`
  (`secretos.rs:639-647`), que extrae el `PathBuf` de `Origen::Archivo` o entra en panico si sale
  otra variante. Las aserciones originales (contenido del archivo, permisos 0600, no
  duplicacion, borrado) estan intactas.
- **`puente.rs`: una linea, mas el doc-comment**: el diff completo de `puente.rs` tiene un unico
  bloque de cambios en `guardar_clave`: la linea del `.map` y el doc-comment de 2 a 4 lineas. No
  hay ningun otro cambio en el archivo. Verifique ademas, con `grep -rn "guardar_clave\b"` sobre
  todo el arbol `.rs`, que `puente.rs` es el unico llamador externo de
  `dictar_providers::secretos::guardar_clave`, asi que no hay ningun otro sitio que dependa del
  tipo de retorno anterior (`PathBuf`) y que este cambio pudiera romper en silencio.
- **Comentarios nuevos, uno por uno**: cabecera de `secretos.rs` (lineas 10-18), Cargo.toml
  (lineas 50-60), doc de `resolver_por_defecto` (382-389), doc de `guardar_clave` (426-438), doc
  de `guardar_clave_en` (448-459), doc de `resultado_de_guardar` (407-414), doc de
  `escribir_en_llavero` (292-303), doc de `guardar_clave` en `puente.rs` y el de `guardarClave`
  en `repositorio_rust.dart`. Todos describen con precision la linea que acompanan, con la
  salvedad de que el doc de `guardar_clave` (`secretos.rs:435-438`) es preciso y hasta
  transparente sobre el comportamiento que genera H1, pero ni el `PLAN.md` ni el `HANDOFF.md` le
  dieron la gravedad que amerita.
- **Anchos de linea, contando caracteres**: no confie en la afirmacion del `HANDOFF.md` de haber
  contado bytes; medi yo mismo con un script Python que decodifica UTF-8 y usa `len()` sobre cada
  linea de `secretos.rs`, `puente.rs` y `Cargo.toml`. Maximo real: 97 caracteres en los tres
  archivos. Cero lineas por encima de 100.
- **Firmas identicas entre las dos ramas `#[cfg]`**: lei las cuatro definiciones
  (`LlaveroResolver` y `escribir_en_llavero`, escritorio y no-escritorio, `secretos.rs:239-322`).
  Los tipos de parametros y de retorno son identicos en ambas ramas; solo cambian los nombres de
  parametro (con guion bajo en la rama no usada), lo que no afecta el tipo de la firma.
- **Archivos tocados, sin sorpresas**: `git status --porcelain` limitado a los cuatro archivos
  del plan confirmo que son exactamente esos cuatro, ni uno mas, incluyendo que `Cargo.lock` no
  se toco (ni contiene `keyring`, evidencia adicional de que nadie corrio `cargo` sobre este
  cambio, consistente con B-1).
- **Riesgo de `keyring::Error` filtrando el secreto por `tracing::warn!` (linea 314)**: revise
  que errores puede producir el camino de escritura (`set_password`/`delete_credential`). Por mi
  conocimiento de versiones anteriores de este crate, las variantes de error alcanzables desde
  ahi (fallo de plataforma, sin acceso al almacen, nombre o valor demasiado largo, invalido,
  ambiguo) no suelen llevar el contenido del secreto, a diferencia de una variante de
  decodificacion que si podria llevar bytes crudos, pero esa solo se alcanza leyendo
  (`get_password`), y esa lectura se descarta con `.ok()` sin pasar nunca por `tracing`. No es
  una confirmacion de fuente; queda en la seccion siguiente.

## 5. Que no pude verificar y que haria falta

- **Todo lo que depende de compilar, formatear o ejecutar** (B-1): `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Nada de esto
  corrio. Necesita Rust 1.75+ instalado (y en la practica, dado que `keyring` 4.2.0 declara
  edicion 2024, probablemente un `rustc` bastante mas nuevo que eso: ver la deuda de MSRV que ya
  declaro el `HANDOFF.md`).
- **El texto exacto de `keyring::Error`**: no encontre ninguna copia local de las fuentes de
  `keyring` 4.2.0, `keyring-core` 1.0.0 ni `zbus-secret-service-keyring-store` 1.0.0 en esta
  maquina (sin `~/.cargo/registry`, sin `Cargo.lock` que las liste). Mi lectura de H1/H2 aparte,
  no pude confirmar con la fuente real si `Error::PlatformFailure` envuelve algun error de bajo
  nivel (D-Bus o Win32) cuyo `Display` pudiera, en algun caso limite, citar parte del valor que
  se intentaba escribir. Haria falta descargar esas tres fuentes (como dice haber hecho el
  implementador) y leer los `impl Display for Error` de cada una, no solo confiar en que "no
  suelen" hacerlo.
- **`clippy::uninlined_format_args` en `secretos.rs:883`** (H3): necesita `cargo clippy` real.
- **El comportamiento de `zbus` sin sesion D-Bus** (Premisa 2): si falla rapido o espera un
  timeout. Necesita ejecutar el resolutor real en una maquina Linux sin
  `DBUS_SESSION_BUS_ADDRESS`, con reloj en mano.
- **Round-trip real del llavero** (AC 1): ninguna prueba automatica escribe y vuelve a leer
  contra un backend real de forma aislada (ya declarado como deuda en el propio `HANDOFF.md`).
  Haria falta correrlo a mano en un Windows con Credential Manager y en un Linux de escritorio
  con Secret Service, y confirmar ademas que H2 no deja basura permanente ahi.
- **Compilacion cruzada a Android** (B-3, sin NDK): no pude confirmar que `dictar-api` siga
  compilando para `aarch64-linux-android` con la nueva seccion `[target.'cfg(...)']` del
  `Cargo.toml`. La lectura estatica dice que la seccion no aplica a `target_os = "android"`, pero
  eso es lectura de TOML, no una compilacion real.
- **Que las 7 funciones de `tracing::Subscriber` y el metodo de `tracing::field::Visit`
  implementados a mano (`secretos.rs:861-893`) coincidan exactamente con la version de `tracing`
  0.1 que resuelve este workspace.** Coincide con mi conocimiento de esa API, pero es exactamente
  el tipo de afirmacion que solo un compilador cierra.

---

# Segunda vuelta (2026-09-03)

Alcance: re-revisión acotada a lo que provocó la vuelta anterior (el Bloqueante H1 y los
hallazgos 2-5 del REVIEW.md consolidado). Unico archivo de código tocado, confirmado por dos
vias independientes -no solo por la declaración del HANDOFF.md-: git diff --stat desde la
raiz solo lista core/providers/src/secretos.rs con cambios de contenido dentro de esta HU, y la
fecha de modificacion en disco de los otros tres archivos del PLAN.md
(core/providers/Cargo.toml, core/api/src/puente.rs, app/lib/datos/repositorio_rust.dart,
las tres a las 09:11-09:12 del 3 de septiembre) es anterior en horas a la de secretos.rs
(13:35 del mismo día), consistente con que solo este último se toco en la sesión de la segunda
vuelta. No toque core/audio-capture (HU-01 en revisión en paralelo). No audité el contrato
#[cfg] (asimetria 7/4 mencionada en el encargo): es tarea de auditor-plataforma en paralelo y
el encargo pide explicitamente no duplicarla; la doy por pendiente de esa revisión, no por buena.

HANDOFF.md, sección "Segunda vuelta", tratado como declaración, no evidencia: cada afirmación
citada abajo se verificó de forma independiente -lectura completa del archivo actual (1166
líneas), trazado manual de tipos y control de flujo función por función, y lectura directa del
código fuente real de keyring 4.2.0, keyring-core 1.0.0, zbus-secret-service-keyring-store
1.0.0 y windows-native-keyring-store 1.1.0, que siguen descargados en el directorio temporal de
esta maquina (no los descargue yo; confio en su autenticidad por la misma razón que ya
razonaron los otros dos revisores: estructura de cargo package consistente, sin forma de
confirmarlo byte a byte contra crates.io sin acceso a red confirmado en esta sesión).

## 1. Veredicto

**NO VERIFICABLE por B-1** (confirmado de nuevo en esta sesión: cargo/rustc no están en el
PATH, ni where cargo/where rustc encuentran nada). En revisión estatica: el Bloqueante H1
se corrige para la grafía de clave que la propia aplicación siempre ha escrito
(NOMBRE_API_KEY), pero encontre una forma concreta y determinista de reproducir el mismo
Bloqueante para una grafía que el resolutor de lectura acepta desde antes de esta HU y que
purgar_del_env no reconoce (H1-bis, abajo) - no es una variación hipotética: la trace línea por
línea y el resultado es el mismo síntoma exacto que motivó la vuelta anterior, en la misma
sesión, sin condición de entorno. Ademas, 6 hallazgos Importantes adicionales y 3
Menores/Notas, todos en purgar_del_env y en la familia de extracción 2-5. Ninguno encaja
literalmente en las cuatro categorías de la tabla de Bloqueante, pero H1-bis es funcionalmente
idéntico al hallazgo que si encajó ahi en la vuelta anterior, y lo marco para que el orquestador
aplique el mismo criterio de elevacion.

## 2. Hallazgos

### Importante

**H1-bis -- purgar_del_env (secretos.rs:606-627) solo reconoce la grafía canonica
_API_KEY; una clave preexistente escrita a mano con una de las otras dos grafías que el propio
resolutor acepta no se purga, y el Bloqueante H1 se reproduce igual de determinista.**

nombre_canonico(referencia) (línea 612) siempre da la forma _API_KEY (última de
nombres_candidatos, líneas 36-52: para "keyring:gemini" son ["gemini", "GEMINI",
"GEMINI_API_KEY"], confirmado también por el test existente
una_referencia_prueba_varios_nombres_de_variable). El chequeo any(...) de purgar_del_env
(líneas 613-621) compara solo contra esa última forma. Pero DotEnvResolver::resolver
(línea 142-148, sin tocar en esta HU) prueba las tres al leer. Escenario, trazado
completo: un .env con la línea GEMINI=sk-vieja (grafía que el propio comentario de
nombres_candidatos documenta como aceptada, aunque README.md:136 solo enseña
GEMINI_API_KEY; nadie tiene que hacer nada exótico, basta con haber escrito la variable corta a
mano, como es habitual al exportarla en una terminal). El usuario guarda una clave nueva desde
Ajustes: escribir_en_llavero tiene éxito, purgar_del_env busca GEMINI_API_KEY en el
archivo, no encuentra ninguna línea con esa clave exacta (la única línea dice GEMINI), así
que ya_estaba queda en false y no purga nada, y guardar_clave_orquestada devuelve
Ok(Origen::Llavero): la interfaz dice "Guardada en llavero del sistema". La siguiente recarga
(misma sesión, mismo proceso) llama a resolver_por_defecto(): el .env sigue teniendo
GEMINI=sk-vieja, DotEnvResolver::resolver("keyring:gemini") prueba "gemini" (no esta), luego
"GEMINI" (esta) y devuelve "sk-vieja", con Origen::Archivo(...), que gana por estar antes que
Origen::Llavero en la cadena. Es el hallazgo 1 original, letra por letra, disparado por una
grafía en vez de por "cualquier .env preexistente". Deberia reconocer cualquiera de las tres
formas de nombres_candidatos, no solo la última, al decidir si hay algo que purgar.

**H2-bis -- La purga de H1 retira la única red de seguridad que el criterio 4 de la HU promete
para la clave recien movida: si el llavero deja de estar disponible en una sesión posterior, la
clave desaparece sin aviso, indistinguible de "nunca configurada".**

Verificado con lectura de fuente, no solo con la prosa del HANDOFF:
zbus-secret-service-keyring-store-1.0.0/src/service.rs:24-27 conecta al bus con
SecretService::connect(...) de forma síncrona, sin espera diferida; y
keyring-4.2.0/src/v1.rs:47-53,107 fija el resultado en un LazyLock evaluado una vez por
proceso, así que el primer fallo (sin sesión D-Bus) deja Err(NoDefaultStore) para el resto de
ese proceso. En esas condiciones, LlaveroResolver::resolver (línea 259-272) devuelve None para
toda referencia, en silencio y por diseño (comentario de la línea 262-265: el que no sabe,
calla). Antes de esta vuelta, ese silencio era inofensivo para una clave que ya vivía en el
.env: el eslabón anterior la tenía. Despues de purgar, si la única copia vive en el llavero y
esa sesión no puede alcanzarlo (CLI via cron o SSH sin bus de sesión reenviado, un contenedor
mínimo, un demonio de secretos bloqueado tras reanudar de suspensión -- el propio
cli/src/main.rs usa resolver_por_defecto() en las mismas líneas que core/api, y es la pieza
pensada para automatizarse), la clave queda invisible en esa sesión: el proveedor se ve "sin
configurar", sin ningún diagnóstico que apunte a la causa real. Confirme además, leyendo
windows-native-keyring-store-1.1.0/src/store.rs:35-40, que en Windows Store::new() es
Ok(...) incondicional, sin depender de ninguna sesión -- el riesgo es específicamente
Linux/headless, no Windows. Esto no estaba en los diez hallazgos del REVIEW.md, ni lo
menciona el HANDOFF, ni lo cubren los otros dos informes de esta vuelta hasta donde los lei.
No es "perder la clave para siempre" en el almacen subyacente, pero si lo es en la práctica,
para cualquier entorno que hoy dependa del .env como respaldo universal -- que es exactamente
lo que el criterio 4 pide que seguir funcionando signifique.

**H3-bis -- purgar_del_env (líneas 608-610) trata cualquier error de lectura del .env, no
solo "no existe", como "nada que limpiar", con un comentario que solo describe el caso más
común.**

```rust
let Ok(previo) = std::fs::read_to_string(&ruta) else {
    return Ok(()); // No hay archivo: nada que limpiar.
};
```

std::fs::read_to_string también devuelve Err por permiso denegado, contenido no UTF-8, o un
error de E/S transitorio (el archivo bloqueado un instante por un antivirus o una herramienta de
sincronizacion sobre la carpeta de configuración en Windows, plausible en el entorno que este
proyecto prioriza). Si la causa es transitoria y el archivo vuelve a ser legible momentos
después, purgar_del_env ya devolvio Ok(()) sin purgar nada, guardar_clave_orquestada siguio
a Ok(Origen::Llavero), y la siguiente resolucion, con el archivo otra vez legible, encuentra la
copia vieja intacta y la antepone al llavero: mismo síntoma que H1 y H1-bis, por una tercera via.
El comentario solo describe el caso de archivo ausente; el código absorbe los demas casos bajo
la misma rama sin decirlo.

**H4-bis -- La reescritura del .env (guardar_clave_en:580-581, std::fs::write, heredada sin
cambios de antes de esta HU) sigue sin ser atómica, y purgar_del_env le da un disparador
automático nuevo que antes no existia.**

std::fs::write trunca y escribe en el mismo archivo, sin archivo temporal ni operacion de
renombrado. No es una regresion de esta vuelta: el patron es idéntico al de antes de HU-05,
comparado contra la versión de secretos.rs en HEAD. Pero antes de esta vuelta esa escritura solo
ocurria cuando el usuario pulsaba explicitamente guardar o borrar esa clave. Ahora,
purgar_del_env (línea 624, dentro de una llamada a guardar_clave_en con valor None) la dispara
también como efecto colateral de cualquier guardado exitoso en el llavero, sin que el usuario
haya pedido tocar el archivo en ese instante. Un fallo del proceso (corte de energia, cierre
forzado) exactamente entre el truncado y el final de la escritura deja el archivo vacio o a
medias, perdiendo todas las claves que contenia, no solo la que se purgaba, en una operacion que
el usuario no inicio directamente y no sabe que esta ocurriendo.

**H5-bis -- El cableado posicional de resolver_por_defecto hacia ensamblar_cadena_por_defecto
(líneas 437-442) no esta protegido ni por tipo ni por prueba.**

Tres de los cuatro argumentos son del mismo tipo exacto, Box<dyn KeyResolver>. Intercambiar
Box::new(dotenv) (línea 440) y Box::new(LlaveroResolver::nuevo()) (línea 441) compila sin
ningún aviso e invierte la prioridad real del criterio 2: el .env pasaria a resolverse en la
posición que la cadena etiqueta como "llavero", y viceversa, del mismo modo que el hallazgo
original de esta HU. Ninguna prueba lo detectaria: confirme por busqueda en todo el repositorio
que resolver_por_defecto() no la llama ninguna prueba, solo cli/src/main.rs y
core/api/src/lib.rs, código de produccion. Es cierto, como dice el HANDOFF, que a
resolver_por_defecto no le queda ninguna otra línea de lógica propia después de la
llamada, y verifique que es literalmente así, sin sentencias tras el retorno implicito, pero si
queda una decisión sin proteger dentro de la propia llamada: que objeto concreto ocupa cada
posición. Contraste, a favor del implementador: en guardar_clave (línea 486-488), el mismo
patron de cableado hacia guardar_clave_orquestada si esta protegido, porque ahi los dos
parametros sustituidos tienen firmas distintas (una función de dos argumentos que devuelve bool,
frente a una función sin argumentos que devuelve Option<PathBuf>) y un intercambio no
compilaria.

**H6-bis -- La fuga residual de tracing en escribir_en_llavero, dentro del brazo que llama a
entrada.set_password(v), línea 309, es un hallazgo abierto, no deuda aceptable sin más.**

El HANDOFF documenta el riesgo con precision y explica por que cerrarlo con una prueba exige
o bien el mock, descartado con buena razón (ver más abajo), o bien una inversion de dependencias
que no es proporcionada para esta correccion: coincido con esa parte. Pero no deja ninguna
mitigacion de costo casi nulo en el propio código, ni siquiera un comentario en la línea 309
advirtiendo no anadir ahi un aviso de tracing con el valor de la clave, pese a que el propio
archivo usa ese recurso en otros puntos (el comentario de registrar_resultado_de_llavero,
líneas 315-325, explica por que esa función si es segura para loguear). Anadir esa advertencia
no depende de compilar y no tiene costo de diseño. Mientras no este, cualquiera que simetrice el
aviso de error anadiendo uno de éxito, una tentacion real dado el patron ya existente en la
rama de error, filtraria la clave sin que nada, ni prueba ni comentario, lo advirtiera antes de
fusionar el cambio.

### Menor

**H7-bis -- purgar_del_env duplica, en vez de reutilizar, el predicado de coincidencia de clave
que ya vive en guardar_clave_en.**

La secuencia que recorta la línea al nombre de la clave, quitar espacios, quitar un posible
prefijo de exportacion, partir por el signo igual y quedarse con la primera mitad recortada,
aparece dos veces, casi identica: en el chequeo any(...) de purgar_del_env (líneas 613-621) y en
el bucle de guardar_clave_en (líneas 552-560). Las compare línea por línea: hoy son
equivalentes, sin divergencia. Pero es lógica duplicada que dos funciones deben mantener
sincronizada a mano; un cambio futuro al formato aceptado (otro separador, otro prefijo) en un
sitio sin el otro reabre H1-bis o H3-bis por una cuarta via.

### Nota

- Cualquier reescritura del .env, incluida la que dispara purgar_del_env, normaliza los finales
  de línea: la forma en que Rust separa un texto en líneas no conserva el retorno de carro de un
  .env con CRLF, y la reconstruccion posterior siempre usa salto de línea simple. Heredado de
  antes de esta HU, cosmetico (el archivo se sigue leyendo bien), pero es perder el formato en
  sentido literal si algo externo al parser de este archivo depende de los finales de línea
  originales.
- Si purgar_del_env falla (el cuarto caso de borde, sin prueba, ya señalado en REVIEW.md), el
  mensaje que llega a Flutter es el texto genérico de un error de entrada/salida convertido a
  cadena en puente.rs:405; no dice que el llavero ya tiene el valor nuevo mientras el .env sigue
  con el viejo. No es incorrecto, no reclama éxito, pero tampoco ayuda a diagnosticar el estado
  resultante.
- El criterio de aceptacion 1 sigue sin ninguna prueba que lea con éxito un valor real del
  llavero, solo caminos de fallo. Ya declarado por el HANDOFF y confirmado por
  verificador-pruebas; lo repito aquí solo porque el encargo pide señalar que criterio de
  docs/06 se quedo sin ninguna prueba, y este sigue siendo ese criterio.

## 3. Premisas que cuestiono

**Premisa 1 (implicita en el HANDOFF, sección Hallazgo 1): si el llavero tiene éxito, purgar la
copia vieja del .env es una mejora estrictamente segura.**

Conclusion: la premisa asume que la única alternativa a "dos copias en conflicto" es "una copia,
en el sitio correcto". Pero cuando el llavero y el .env respondian distinto en sesiones
distintas, el propio invariante que el criterio 4 pide sostener, la copia en el .env, aunque
incorrecta cuando ambas coincidian, funcionaba como respaldo en cualquier entorno, incluidos los
que no pueden alcanzar el llavero. Purgarla resuelve la inconsistencia cuando ambos eslabones
están disponibles, pero cambia una degradacion con valor (dato desactualizado pero presente) por
una sin el (ausencia total) en cualquier sesión donde el llavero falle después de la purga.
H2-bis es la consecuencia concreta. No propongo deshacer la purga, el hallazgo 1 original la
necesita y sin ella el criterio 3 no se cumple, pero "es una mejora sin más" no se sostiene sin
matizar que se pierde a cambio.

**Premisa 2 (HANDOFF, sección Hallazgo 1, último punto): purgar_del_env cubre los cuatro casos
de borde que pedia el REVIEW.**

Conclusion: cierto para los cuatro tal como el REVIEW los enumero literalmente (archivo
inexistente, clave ausente, otras claves preservadas, fallo al reescribir). Pero la frase se lee
como "el Bloqueante 1 queda cerrado", y no lo esta: la deteccion de si la clave ya estaba
descansa en un supuesto no declarado, que toda clave preexistente en el .env use la grafía
canonica, que el propio resolutor de este mismo archivo nunca exigio (ver H1-bis). Cubrir los
cuatro casos de borde nombrados no es lo mismo que cubrir todas las formas en que el bug
original puede reaparecer.

## 4. Que verifique y no marque

- Permisos 0600 tras la purga: guardar_clave_en reaplica el modo 0600 (líneas 583-588)
  incondicionalmente después de cada escritura, incluida la que dispara purgar_del_env; no hay
  ninguna ruta que escriba sin pasar por esas líneas. Verificado leyendo el cuerpo completo, no
  solo la firma.
- Alcance de la purga: guardar_clave (línea 486-488) pasa dir_configuracion como único
  carpeta_config en produccion; purgar_del_env nunca ve ninguna de las otras rutas de
  rutas_dotenv() (directorio de trabajo o su padre). Coincide con lo que el HANDOFF declara en
  Precision de alcance, y lo confirme por lectura directa de la llamada, no por la prosa.
- Propagacion de errores de purgar_del_env: el operador de propagación de errores en la línea
  514 corta la ejecución de guardar_clave_orquestada antes de llegar a resultado_de_guardar si
  purgar_del_env devuelve un error. Es una garantía del propio lenguaje Rust, no del compilador
  de esta maquina en particular, así que la doy por buena sin necesitar ejecutarla: un error ahi
  nunca puede convertirse en un resultado que informe éxito en el llavero.
- La familia 2-5 (extracción real): guardar_clave y resolver_por_defecto no tienen, ninguno de
  los dos, una sola sentencia después de la llamada a la función extraida, confirmado leyendo el
  cuerpo completo de ambos. El hueco que queda no es lógica después, sino cableado dentro de la
  llamada, ver H5-bis.
- La decisión de no usar el mock de keyring-core: la evalue con fuente propia, no con la prosa
  del HANDOFF. Confirme, leyendo keyring-core-1.0.0/src/lib.rs línea 39 (el modulo mock se
  declara sin feature flag), keyring-4.2.0/src/v1.rs líneas 47 a 53 y 107 (el LazyLock corta con
  un error fijo antes de consultar el store registrado en keyring_core, en cualquier plataforma)
  y zbus-secret-service-keyring-store-1.0.0/src/service.rs líneas 24 a 27 (la conexion se hace
  de forma síncrona, sin espera perezosa) que el argumento se sostiene: en el CI de Linux sin
  D-Bus, ningún mock instalado después puede intervenir, porque el primer fallo ya quedo fijado
  para el resto del proceso. Coincido con la decisión.
- keyring::Error::NoEntry construible desde fuera del crate: error.rs de keyring-core 1.0.0
  confirma que NoEntry es una variante sin campos de un enum marcado como no exhaustivo. Ese
  atributo bloquea el emparejamiento exhaustivo sin comodin y la construcción por sintaxis de
  estructura de variantes con campos con nombre; no bloquea referenciar una variante unitaria ya
  existente, y lo confirme también de forma empirica: zbus-secret-service-keyring-store y
  windows-native-keyring-store construyen esa misma variante así, ambos crates externos a
  keyring-core. Sin riesgo de compilación.
- Los parametros de función generica (impl Fn / impl FnOnce) de guardar_clave_orquestada:
  razonado por reglas de préstamo, no por compilador. carpeta_config se referencia dos veces en
  el cuerpo de la función, línea 513 y dentro del cierre pasado a resultado_de_guardar en la
  línea 519, aunque en tiempo de ejecución solo una de las dos ramas corre. Para que el
  analizador de préstamos acepte esa doble referencia textual necesita el rasgo que permite
  llamar por referencia compartida sin consumir el valor; el rasgo que solo permite una llamada
  total no compilaria ahi. intentar_llavero se usa una sola vez, línea 508, así que ese segundo
  rasgo es exacto y suficiente. La eleccion no es solo estilo: es la única combinacion que puede
  compilar dada la estructura del cuerpo de la función.
- La coercion de una referencia a PathBuf hacia una referencia a Path (purgar_del_env recibe
  &dir en la línea 514, con dir de tipo PathBuf): coercion por deref estandar de la biblioteca
  estandar, el mismo mecanismo que convierte una referencia a String en una referencia a str. No
  depende de nada especifico de este workspace.
- Anchos de línea: recontados con un script propio (Python, contando caracteres del texto
  decodificado como UTF-8, no bytes) sobre las 1166 líneas actuales del archivo, no solo las
  nuevas de esta vuelta. Maximo real: 97 caracteres. Cero líneas por encima de 100.
- Panico o desenvolvimiento sin control en código de produccion: busqueda limitada a las líneas
  1 a 648 (todo lo anterior al modulo de pruebas, que empieza en la 649). Cero resultados, igual
  que en la primera pasada.
- Extension del nuevo uso de tracing: única aparicion en código de produccion es la línea 329,
  dentro de registrar_resultado_de_llavero; ninguna otra fuera del modulo de test. Confirmado
  por busqueda sobre el archivo completo.
- B-1: reproducido de nuevo en esta sesión (la versión de cargo no se pudo obtener, comando no
  encontrado; la busqueda de cargo y de rustc en el sistema no encontro nada), no solo aceptado
  de la declaración del HANDOFF.

## 5. Que no pude verificar y que haria falta

- Todo lo que depende de compilar o ejecutar (B-1, sin cambios): H1-bis, H2-bis, H3-bis y
  H4-bis están razonados por trazado manual de tipos y control de flujo, no por una mutacion
  roja con pruebas reales. Necesitan Rust instalado para confirmarse con una prueba de verdad;
  en el caso de H1-bis es trivial de escribir en cuanto haya compilador: sembrar el .env con una
  línea corta tipo GEMINI=sk-vieja (en vez de con guardar_clave_en, que solo escribe la forma
  canonica) y comprobar que guardar_clave_orquestada, con éxito simulado en el llavero, no deja
  esa línea intacta.
- Frecuencia real de un fallo transitorio de lectura del .env (H3-bis): plausible por el
  mecanismo (antivirus, sincronizacion de archivos), no medido. Haria falta instrumentar la
  lectura del archivo en una maquina Windows real con OneDrive u otro sincronizador activo sobre
  la carpeta de configuración.
- Si la escritura del archivo se resuelve en una sola llamada al sistema para un archivo de
  este tamano (unos pocos cientos de bytes), lo que acotaria, sin eliminar, la ventana de riesgo
  de H4-bis: no medido, haria falta un trazador de llamadas al sistema sobre una escritura real.
- El contrato de compilación condicional y su asimetria (7 ramas positivas, 4 negativas): fuera
  de mi alcance por indicación explicita del encargo; lo audita auditor-plataforma en paralelo.
  No lo doy por bueno ni por malo.
- Todo lo que la primera pasada ya declaraba sin verificar y que esta vuelta no cambia
  (compilación cruzada a Android, formateo real, el comportamiento medido de D-Bus sin sesión,
  el valor exacto del limite de longitud de usuario en Windows) sigue igual: ver la sección
  original más arriba.

# Tercera vuelta (2026-09-03)

Alcance: el encargo de esta vuelta fue verificar H1-bis (mi propio Bloqueante de la segunda
vuelta) y de paso H2-bis a H7-bis. Unico archivo de codigo: `core/providers/src/secretos.rs`
(1499 lineas), confirmado con `git status --porcelain` sobre ese archivo -- sin cambios en
`Cargo.toml`, `puente.rs` ni `repositorio_rust.dart`. Lei el archivo completo, no el diff, en
tramos (1-500, 280-500, 500-780, 780-1130, 1130-1499). No toque `core/audio-capture` ni edite
nada en `app/lib` (T-15 en revision ahi); si lei dos archivos fuera de ese arbol -- `puente.rs`
y `ajustes.dart` -- porque el propio encargo de H3-bis exige comprobar si la propagacion "llega
al usuario", y esa pregunta no se puede responder sin salir de `secretos.rs`; no cambie nada en
ninguno de los dos.

`HANDOFF.md`, seccion "Tercera vuelta", tratado como declaracion: cada afirmacion que se cita
abajo fue verificada leyendo el codigo real en la linea indicada, no copiada del texto. Ademas,
donde el `HANDOFF` afirma haber ejecutado algo (los dos scripts de Python), yo tambien ejecute
mis propias verificaciones independientes, no las suyas: conteo de caracteres por linea con un
script propio y balance de llaves/parentesis/corchetes con otro script propio.

## 1. Veredicto

**NO VERIFICABLE por B-1** (sin cambios: no hay `cargo`/`rustc`/`rustfmt` en el PATH). En
revision estatica: **H1-bis, el Bloqueante que motivo esta vuelta, queda confirmado corregido**
-- verifique con lectura de codigo, no acepte la declaracion -- en sus cuatro sub-preguntas: los
dos consumidores (`guardar_clave_en` y `purgar_del_env_con`) usan `linea_declara`, no hay un
tercer sitio que reimplemente la nocion en todo el repositorio, acepta las tres formas y el
prefijo `export `, y no encontre un escenario de borrado de mas con los proveedores reales del
repositorio (`gemini`, `deepseek`, `openai`). H3-bis, H5-bis/H7-bis y H6-bis tambien quedan
confirmados segun lo declarado (detalle en la seccion 4). Pero el codigo nuevo de esta vuelta
introduce **2 hallazgos Importantes propios**, que responden a la pregunta abierta del encargo
("el arreglo introdujo algo nuevo?"): una ventana de permisos abiertos en la escritura atomica
del `.env` (H4-ter) y una relectura del llavero que certifica legibilidad pero no coincidencia
de valor, con un camino concreto a clave duplicada en dos sitios (H2-ter). Mas 2 notas marcadas
explicitamente como sospecha, sin proveedor ni escenario real hoy que las dispare.

## 2. Hallazgos

### Importante

**H4-ter -- el archivo temporal de la escritura atomica pasa por permisos abiertos antes de
cerrarse en 0600, y en cada guardado, no solo en el primero.**

`secretos.rs:680-691`:

```rust
let temporal = dir.join(format!(".env.tmp.{}", std::process::id()));
std::fs::write(&temporal, contenido)?;

#[cfg(unix)]
{
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&temporal, std::fs::Permissions::from_mode(0o600))?;
}

std::fs::rename(&temporal, &ruta)?;
```

`std::fs::write` crea el archivo con el modo por defecto de `File::create` (0o666 antes de
aplicar la umask del proceso; 0o644 con la umask 022 mas comun, legible por grupo y por otros) y
recien despues se restringe a 0600. Entre esas dos lineas el `.env.tmp.<pid>` contiene el `.env`
completo en claro -- incluida la clave que se acaba de guardar -- con permisos que cualquier
otro usuario local puede leer, en un directorio (`dir_configuracion()`, creado con
`create_dir_all` sin `mode` propio, linea 627) que por la misma razon suele ser listable por
cualquiera. Es exactamente la exposicion que la HU existe para cerrar (comentario del propio
archivo, lineas 20-22: "una clave en un .env esta en texto plano en el disco... por eso el
llavero es lo suyo"), reabierta por un instante en cada guardado. Antes de esta vuelta la
escritura era directa sobre `.env` (`std::fs::write` seguido de `chmod`): esa misma ventana solo
existia la primera vez que el archivo se creaba, porque truncar un archivo ya existente no
reinicia sus permisos. Con un temporal nuevo en cada guardado, la ventana se abre en **cada**
guardado, no solo en el primero -- es una ampliacion de superficie que introduce esta misma
vuelta, no una deuda heredada. La correccion habitual es crear el archivo ya con el modo
restringido (`OpenOptions` con `mode(0o600)` antes de escribir, no `set_permissions` despues),
no solo "0600 antes de renombrar" como dice el comentario de la linea 686 -- que es cierto en lo
que afirma (antes del rename), pero no cubre la ventana entre la creacion y el chmod, que es la
que importa.

Segunda parte, la que pedia el encargo ("que no quede huerfano si falla la escritura"): si
`std::fs::set_permissions` (linea 688) falla -- un sistema de archivos que no soporta chmod
arbitrario, un punto de montaje restringido para atributos extendidos -- la funcion devuelve el
error con `?` sin haber intentado borrar el temporal. Ese `.env.tmp.<pid>` ya contiene el secreto
en claro, con permisos abiertos, y queda huerfano indefinidamente: no hay ningun
`std::fs::remove_file` sobre esa ruta en todo el archivo (comprobado con `grep` de
`remove_file`/`.tmp.` sobre `secretos.rs` completo: la unica aparicion de `.env.tmp.` es la
linea 680, donde se crea). El `HANDOFF` documenta el riesgo de huerfano solo para el caso "el
proceso muere entre escribir y renombrar" (deuda declarada); no menciona este camino -- fallo
del propio chmod, sin que el proceso muera -- que dejaria exactamente el mismo residuo pero de
forma reproducible sin necesitar un corte de proceso.

**H2-ter -- la relectura de `escribir_en_llavero` certifica que la lectura no falla, no que el
valor leido coincida con el escrito; y si falla tras un `set_password` que si tuvo exito, la
clave termina duplicada, no protegida.**

`secretos.rs:328-331`:

```rust
Some(v) => {
    entrada.set_password(v)?;
    entrada.get_password().map(|_| ())
}
```

`.map(|_| ())` descarta el valor que devuelve `get_password()`: nunca se compara contra `v`. La
propia decision (comentario lineas 305-315) dice que "escribir con exito" debe significar "de
verdad disponible para la proxima lectura", pero lo que el codigo comprueba es solo que la
lectura no devuelva `Err`, no que devuelva el mismo valor. Con los tres backends reales
(sincronos, sobre la misma entrada, mismo proceso) esto es poco probable que diverja en la
practica -- lo cual coincide con lo que ya admite el propio `HANDOFF` en "Lo que NO pude
verificar" --, asi que dejo esta mitad como sospecha razonada, no como hallazgo confirmado.

La segunda mitad si la trace completa contra el codigo, no es sospecha: si `set_password` tiene
exito real (el secreto queda en el llavero) pero la relectura inmediata falla -- un hipo
transitorio de D-Bus, una coleccion de Secret Service bloqueada un instante --,
`escribir_en_llavero` devuelve `false` (linea 335, via `registrar_resultado_de_llavero` sobre un
`Err`). En `guardar_clave_orquestada` (`secretos.rs:550-572`), `en_llavero` queda en `false`, asi
que el bloque `if en_llavero { ... purgar_del_env ... }` (linea 558) no se ejecuta -- correcto,
no se pierde el respaldo -- pero `resultado_de_guardar(false, respaldo)` si ejecuta el respaldo,
que llama a `guardar_clave_en(&dir, referencia, valor)` con el **mismo** `valor` (linea 570): la
clave se escribe en el `.env`. Resultado neto: el secreto queda de verdad en el llavero -- nadie
lo sabe, porque el retorno fue `Err` para esa rama -- y tambien en el `.env`, y `guardar_clave`
le informa al usuario `Origen::Archivo`, como si el llavero hubiera fallado del todo. No hay
corrupcion de dato (mismo valor en los dos sitios) ni el bloqueante original (el `.env` sigue
mandando y tiene el valor correcto), pero si una copia en texto plano que la HU existe para
evitar, dejada sin aviso ni intento de retirarla en un guardado posterior con mas suerte.

### Nota

**Nombre del temporal no distingue llamadas concurrentes del mismo proceso.** `secretos.rs:680`:
`.env.tmp.<pid>` varia solo por PID, no por hilo ni tarea. La aplicacion es un proceso unico de
larga duracion; si dos guardados sobre el mismo directorio se disparan solapados dentro de ese
proceso (dos proveedores guardados casi a la vez desde la pantalla de Ajustes, por ejemplo),
ambas llamadas usarian el mismo nombre de archivo temporal. No pude establecer si el puente
flutter_rust_bridge serializa las llamadas a `guardar_clave` o las despacha en paralelo -- no lo
investigue, es un limite de mi revision, no una conclusion --, asi que esto queda como sospecha
marcada, no como hallazgo con severidad.

**Colision de `nombres_candidatos` entre dos proveedores, si uno tuviera el nombre canonico del
otro.** La comparacion en `linea_declara` es por igualdad exacta contra la lista completa de
`nombres_candidatos(referencia)` (`secretos.rs:602-603`), no acotada por proveedor: un proveedor
hipotetico con id `gemini_api_key` compartiria el candidato `GEMINI_API_KEY` con el proveedor
`gemini` real. Con los tres proveedores reales de `config/providers.toml` (`gemini`, `deepseek`,
`openai`) no hay solapamiento -- lo comprobe calculando los tres conjuntos a mano -- asi que no
hay hoy ningun escenario que lo dispare; queda como sospecha de diseno, no como hallazgo.

## 3. Premisas que cuestiono

**"Es exactamente el patron habitual (temporal + rename)"** (`HANDOFF.md`, seccion H4-bis,
justificacion del patron elegido). La premisa describe bien la atomicidad del reemplazo final,
pero el patron "temporal + rename" habitual para un archivo sin datos sensibles no necesita
crear el temporal con permisos restringidos desde el origen; para un archivo cuyo proposito
completo es contener credenciales, si. Aplicar la variante generica del patron a un archivo de
secretos es la causa exacta de H4-ter. **Conclusion: la premisa no se sostiene para este caso de
uso** -- el patron elegido es el correcto para atomicidad de contenido, incorrecto para
confidencialidad, y son dos propiedades distintas que el comentario del codigo (linea 686, "0600
antes de renombrar") trata como si fueran la misma.

**"`resolver_por_defecto` no le queda ninguna decision propia sin cubrir"** (`HANDOFF.md`,
seccion H5-bis/H7-bis). La ataque buscando cualquier logica residual: `secretos.rs:485-491` es
literalmente `cadena_con(Box::new(EnvResolver), DotEnvResolver::buscar(),
Box::new(LlaveroResolver::nuevo()))`, sin una sola linea de control de flujo propia. Fui mas
alla de lo que pedia el encargo y confirme que esta funcion no es un adorno: `router.rs:19-22`
la reexporta, y `core/api/src/lib.rs:130,222,266` y `cli/src/main.rs:159,260` la consumen como el
resolutor real que arma el `Router` de produccion. **Conclusion: la premisa se sostiene**, y
ademas la funcion que ya no tiene logica propia es la que de verdad usan el CLI y el API, no una
ruta muerta.

## 4. Que verifique y no marque

- **H1-bis, los cuatro sub-puntos del encargo.** Lei `linea_declara` (601-604) y confirme sus
  dos unicos llamadores en todo el archivo con `grep`: `guardar_clave_en:637` y
  `purgar_del_env_con:760`. Repeti el `grep` sobre `nombre_de_linea`/`linea_declara` en
  `cli/src`, `core/api/src` y `app/lib` (lectura, no edicion) buscando cualquier
  `strip_prefix("export`/`split_once`/`_API_KEY` propio: el unico hallazgo fuera de
  `secretos.rs` es una linea de texto de ayuda en `cli/src/main.rs:79`, no logica. Trace
  `nombres_candidatos("keyring:gemini")` a mano: `gemini`, `GEMINI`, `GEMINI_API_KEY`, las tres
  formas que pide el encargo. `nombre_de_linea` usa `strip_prefix("export ")` antes de partir por
  el signo igual, igual que `parsear` (linea 199): confirmado que reconoce el prefijo. Para
  "borra de mas": trace `guardar_clave_en` con un `.env` que trae `GEMINI=x` y `GEMINI_API_KEY=y`
  a la vez (linea por linea, con el bucle de 636-654): ambas lineas coinciden con
  `linea_declara`, la primera en orden de archivo se sustituye por la forma canonica con el valor
  nuevo, la segunda se descarta sin duplicar ni resucitar -- un solo resultado, sin restos. Para
  "clave de otro proveedor cuyo nombre es prefijo": confirme que la comparacion es de igualdad
  exacta de la cadena completa devuelta por `nombre_de_linea` contra cada elemento de la lista
  (`c.as_str() == k`, linea 603), no `starts_with`, asi que un prefijo real entre dos proveedores
  reales no produce coincidencia -- solo la colision exacta que documento como Nota, y que no se
  da con los proveedores reales.
- **H3-bis, la propagacion dentro de `secretos.rs`.** Trace la cadena completa:
  `purgar_del_env_con` (746-758) distingue `NotFound` del resto y devuelve `Err` en cualquier
  otro caso; `purgar_del_env` (727-729) no envuelve nada, delega directo; `guardar_clave_orquestada`
  (561-563) usa `purgar_del_env(&dir, referencia)?`, asi que el error sale de la funcion entera;
  `guardar_clave` (534-536) es una llamada directa sin capturar nada. Para "llega al usuario"
  sali del archivo: `puente.rs:399-406` mapea `Err` a `Err(e.to_string())`, que
  `flutter_rust_bridge` convierte en una excepcion del lado Dart. Lei `ajustes.dart:161-179`
  (`_guardar`, la funcion que llama a `guardarClave`): no tiene `try/catch`, a diferencia de
  `_probar` (181-198, mismo archivo) que si lo tiene. No es un hallazgo de este archivo ni de
  esta vuelta -- `ajustes.dart` no esta en el `PLAN.md`, no lo edite, y antes de esta vuelta esa
  ruta de error casi nunca se alcanzaba porque el `let-else` viejo convertia casi cualquier fallo
  de lectura en `Ok(())` -- pero registro que "llega al usuario" solo se puede confirmar hasta el
  limite de la excepcion Dart, no hasta una pantalla legible.
- **H5-bis/H7-bis.** Ver seccion 3: confirmado por lectura directa que `resolver_por_defecto` no
  retiene logica propia, y que es la funcion real de produccion, no solo la que prueban los
  tests. Repeti el `grep` de `resolver_por_defecto()` (con parentesis, para excluir menciones en
  comentarios) sobre el modulo de tests: cero resultados, coincide con lo declarado.
- **H6-bis.** Compare el comentario aplicado (`secretos.rs:321-327`) contra el texto exacto de mi
  propio hallazgo de la segunda vuelta (este mismo archivo, mas arriba, seccion "Segunda vuelta"):
  pedi "un comentario... advirtiendo no anadir ahi un aviso de tracing con el valor de la clave";
  el comentario aplicado dice, sobre el mismo brazo `Some(v)`, "si algun dia hace falta
  diagnosticar un fallo aqui con tracing... pero NUNCA v". Es lo que pedi, no una version
  reducida.
- **Ancho de linea y balance de delimitadores, con scripts propios, no los del `HANDOFF`.** Un
  script de Python que cuenta caracteres Unicode por linea (no bytes) sobre las 1499 lineas:
  maximo real 97, en la linea 213 -- coincide con lo declarado, pero lo comprobe con mi propio
  script, no acepte el numero. Balance de llaves/parentesis/corchetes sobre el archivo completo:
  170/170, 900/900, 90/90 -- senal debil (no distingue literales de cadena) pero sin ninguna
  asimetria.
- **Que el archivo modificado sea el unico, y que sea de verdad el de esta sesion.**
  `git status --porcelain -- core/providers/src/secretos.rs` da solo ese archivo; ningun otro
  archivo de `core/providers/`, `core/api/` ni `app/lib/datos/` aparece modificado.

## 5. Que no pude verificar y que haria falta

- **Todo lo que depende de compilar o ejecutar (B-1, sin cambios).** H4-ter y H2-ter estan
  razonados por trazado manual de tipos y del orden de las llamadas al sistema documentado por
  `std::fs`, no por una prueba real. Para H4-ter en concreto haria falta, con `cargo` disponible,
  una prueba que capture el modo del archivo durante la ventana entre `write` y
  `set_permissions` -- por ejemplo, inyectando un punto de observacion entre ambas llamadas que
  consulte `metadata().permissions()` -- y en una maquina real, confirmar la umask heredada por
  el proceso de la aplicacion empaquetada.
- **Si `flutter_rust_bridge` serializa las llamadas entrantes a `guardar_clave` o las despacha en
  paralelo sobre el mismo proceso.** Necesario para saber si la sospecha del nombre de temporal
  compartido (Nota, junto a H4-ter) es alcanzable o no; no lo investigue, ni en el codigo de
  `puente.rs` ni en la configuracion de `frb_generated.rs` (que no se edita a mano y no lei en
  detalle esta vez).
- **El comportamiento real de `get_password()` inmediatamente despues de un `set_password()`
  exitoso, contra un backend real** (Credential Manager, Secret Service, Keychain): sigue sin
  poder confirmarse sin compilar y sin un llavero real disponible en esta sesion -- mismo limite
  que ya declaraban las dos vueltas anteriores para esta misma linea.
- **`std::fs::rename` en Windows** (atomicidad real, comportamiento si el destino esta abierto
  por un antivirus o sincronizador): fuera de mi alcance por indicacion explicita del encargo --
  lo audita `auditor-plataforma` en paralelo. No lo doy por bueno ni por malo.
- **Frecuencia real de un fallo de `set_permissions`** en un sistema de archivos concreto (la
  causa que dispararia el huerfano de H4-ter sin necesitar que el proceso muera): no medido,
  haria falta probar sobre un punto de montaje que efectivamente rechace `chmod` (algunos
  sistemas de archivos de red, contenedores con ciertas politicas de seguridad).
- Todo lo que las vueltas anteriores ya declaraban sin verificar y que esta vuelta no toca
  (compilacion cruzada a Android, formateo real, el texto exacto de cada variante de
  `keyring::Error`, el valor exacto de `CRED_MAX_USERNAME_LENGTH`) sigue igual: ver las secciones
  originales mas arriba.

---

# Cuarta vuelta (2026-09-04)

Alcance: el encargo de esta vuelta fue verificar H4-ter, H2-ter, P1, V1, V2, M1 y responder si el
arreglo introdujo algo nuevo. Único archivo de código: `core/providers/src/secretos.rs`. Primera
vuelta de esta HU con `cargo`/`rustc` instalados (1.98.1) y, tras resolver el PATH (`~/.cargo/bin`
no estaba en el PATH por defecto de esta sesión de Git Bash), **ejecutables**. Compuertas corridas
de verdad: `cargo fmt --all -- --check`, `cargo clippy -p dictar-providers --all-targets -- -D
warnings`, `cargo test -p dictar-providers`, más un test de integración desechable (creado y
borrado dentro de esta sesión) para confirmar un hallazgo por ejecución en vez de por lectura. Es
la primera evidencia ejecutada de HU-05 en cuatro vueltas.

## 0. Advertencia: el archivo se modificó por sí solo durante esta revisión

Esto pesa sobre todo lo que sigue y hay que leerlo antes que los hallazgos.

Capturé el diff de esta vuelta al empezar (`git diff -- core/providers/src/secretos.rs`, contra
`c08abdf7` — el commit con el que cierra la tercera vuelta —, 347 inserciones/52 borrados, hash
del archivo `9dc5f74b...`). Corrí las tres compuertas contra ese estado: pasaron (detalle abajo).
Después, sin que yo tocara el archivo, lo volví a leer y encontré un bloque nuevo dentro de
`linea_declara` que no estaba en mi diff original:

```rust
fn linea_declara(linea: &str, referencia: &str) -> bool {
    // MUTACION V2 (verificador-pruebas, cuarta vuelta): se salta la primera
    // forma (minuscula, sin transformar) de nombres_candidatos.
    let candidatos = nombres_candidatos(referencia)[1..].to_vec();
```

Es, letra por letra, la mutación que V2 dice haber verificado que las pruebas nuevas detectan —
alguien (casi con certeza un proceso de mutación automática corriendo en paralelo sobre el mismo
árbol de trabajo, a juzgar por el propio texto del comentario) la estaba aplicando en vivo. Un rato
después volví a comprobar: la mutación ya no estaba, el archivo había vuelto exactamente a mi diff
original (mismo hash `9dc5f74b...`). Corrí las tres compuertas otra vez contra ese estado estable
(hash idéntico antes y después de la corrida completa, ver salida en la sección 1) y pasaron limpio.

En el medio, el orquestador me envió tres correcciones sucesivas y contradictorias sobre el
entorno: primero que `cargo` estaba bloqueado por Smart App Control (con `cargo check -p
dictar-domain` colgado como evidencia); después una retractación completa ("cargo SÍ funciona",
con `cargo check -p dictar-domain` en 1,74 s como evidencia); después una tercera afirmación, con
salida de compilador literal, de que `resolver_por_defecto` tenía `ResolverDeEntorno` y
`ResolverDeLlavero` intercambiados (`E0308`, línea 549) y el crate no compilaba. Verifiqué esto
último de inmediato, por mi cuenta, en vez de citarlo: leí las líneas 545-551 en el momento (sin
intercambio, orden correcto), y corrí `cargo check -p dictar-providers --all-targets` de nuevo, con
fecha (`Fri Sep 4 01:06:34`-`01:06:48`): terminó en 14 s con éxito. No pude reproducir el error que
describe el orquestador en ningún momento en que yo mismo miré esa función. La explicación más
simple, dado lo que ya había observado con `linea_declara`, es que ese intercambio también fue una
mutación en vivo del mismo proceso concurrente, capturada por el orquestador en una ventana distinta
a las mías — de hecho, un poco más tarde encontré una tercera mutación transitoria, esta vez dos
`eprintln!("DIAGNOSTICO verificador-pruebas: ...")` insertados dentro de las dos ramas de limpieza
de `guardar_clave_en` (las que añadió H4-ter), que también desaparecieron en la siguiente lectura.

No lo presento como acusación — no sé qué proceso es ni si es intencional (mutación controlada de
`verificador-pruebas`, muy probablemente, a juzgar por el texto de los propios comentarios
insertados) — sino como una limitación real de esta vuelta: **el archivo bajo revisión no fue
estable durante toda la sesión**, y cualquier lectura puntual —la mía, la del orquestador, la de
cualquiera— puede estar describiendo un estado transitorio en vez del que realmente queda en el
árbol de trabajo. Todo lo que reporto abajo lo verifiqué contra el estado que, en las dos ocasiones
en que lo comprobé con hash, coincidía exactamente con el diff original de 347/52 líneas (hash
`9dc5f74bc34d98a30ace6cf7ea9fe1f389db71c20463da97562c2810ae9b2e61`), no contra cualquier lectura
aislada. Recomiendo, antes de dar por buena esta revisión o cualquier otra, confirmar con
`sha256sum core/providers/src/secretos.rs` que sigue siendo ese hash, y averiguar qué proceso está
mutando el archivo y detenerlo si sigue corriendo — de lo contrario ninguna verificación posterior,
tampoco la mía, es de fiar sin repetirla.

## 1. Veredicto

**Compila, `clippy -p dictar-providers -- -D warnings` sale limpio, y las 87 pruebas de
`dictar-providers` pasan — ejecutado de verdad, no razonado.** Es la primera vez en esta HU. Pero
no es una compuerta completamente verde: `cargo fmt --all -- --check` **falla** sobre código nuevo
de esta vuelta (dos de las seis pruebas nuevas no están formateadas), y leyendo el archivo completo
(no solo el diff) encontré y **confirmé por ejecución** un hallazgo **Bloqueante** preexistente, no
introducido esta vuelta y no señalado en ninguna de las tres vueltas anteriores: `guardar_clave_en`
pierde en silencio las claves de otros proveedores si la lectura del `.env` previo falla por
cualquier causa que no sea "no existe" (detalle en la sección 2). Los seis puntos puntuales del
encargo — H4-ter, H2-ter, P1, V1, V2, M1 — verifican correctos tal como los describe el `HANDOFF`,
con un matiz en H4-ter (gap teórico de baja explotabilidad, no mencionado) y uno en la deuda
declarada (una vía parcial de prueba que no se exploró). Detalle punto por punto abajo.

**Salida real de las tres compuertas**, corrida contra el estado estable (hash
`9dc5f74bc34d9...` antes y después, idéntico):

```
$ export PATH="$HOME/.cargo/bin:$PATH"; cd /d/dictar_ia

$ cargo fmt --all -- --check          # (recorte: solo lo que toca secretos.rs)
Diff in \\?\D:\dictar_ia\core\providers\src\secretos.rs:1112:
         let dir = tempfile::tempdir().unwrap();
         let a = nombre_temporal(dir.path());
         let b = nombre_temporal(dir.path());
-        assert_ne!(a, b, "dos guardados en el mismo proceso no deben compartir temporal");
+        assert_ne!(
+            a, b,
+            "dos guardados en el mismo proceso no deben compartir temporal"
+        );
     }
Diff in \\?\D:\dictar_ia\core\providers\src\secretos.rs:1131:
         let secreto = "sk-no-debe-quedar-huerfano-en-disco-2f6c";
         let resultado = guardar_clave_en(dir.path(), "keyring:prueba-huerfano", Some(secreto));
-        assert!(resultado.is_err(), "un rename imposible debía propagarse como error");
+        assert!(
+            resultado.is_err(),
+            "un rename imposible debía propagarse como error"
+        );
Diff in \\?\D:\dictar_ia\core\providers\src\secretos.rs:1771:      # preexistente, ver nota abajo
-            assert!(!registrar_resultado_de_llavero(Err(keyring::Error::NoEntry)));
+            assert!(!registrar_resultado_de_llavero(Err(
+                keyring::Error::NoEntry
+            )));
$ echo $?
1

$ cargo clippy -p dictar-providers --all-targets -- -D warnings
    Checking dictar-providers v0.1.0 (D:\dictar_ia\core\providers)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 56s
$ echo $?
0

$ cargo test -p dictar-providers
running 87 tests
...
test secretos::tests::dos_guardados_seguidos_no_comparten_nombre_de_temporal ... ok
test secretos::tests::si_falla_el_renombrado_no_queda_un_temporal_huerfano_con_el_secreto ... ok
test secretos::tests::resolver_por_defecto_no_entra_en_panico ... ok
test secretos::tests::guardar_en_el_llavero_purga_una_copia_vieja_escrita_en_minuscula ... ok
test secretos::tests::purgar_del_env_reconoce_una_clave_escrita_en_minuscula ... ok
test secretos::tests::si_la_purga_falla_tras_un_exito_real_en_el_llavero_el_error_lo_dice ... ok
...
test result: ok. 87 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.63s
$ echo $?
0
```

`cargo fmt --all` (workspace completo, no solo el paquete) también marca varios archivos de
`core/audio-capture`. No los reporto ni los cuento contra esta vuelta: ese árbol lo está tocando
otra sesión ahora mismo (`git status` de apertura), fuera del alcance de este encargo.

La línea 1771 (el `assert!` sobre `registrar_resultado_de_llavero`) **no es de esta vuelta** —
confirmado contra `git show c08abdf7 -- core/providers/src/secretos.rs`, línea 1479 de esa versión,
idéntica—: es deuda de formato preexistente, invisible en las tres vueltas anteriores porque nunca
hubo `cargo fmt` para encontrarla. La anoto por transparencia, no la cuento como hallazgo de esta
vuelta.

No corrí `cargo build --workspace --all-targets` ni `cargo clippy --workspace` (el paso real de
CI): el encargo pidió explícitamente las versiones acotadas a `dictar-providers`, y `--workspace`
tocaría `core/audio-capture` y `app/**`, fuera de mi alcance esta vuelta.

## 2. Hallazgos

### Bloqueante

**`guardar_clave_en` pierde en silencio las claves de otros proveedores si la lectura del `.env`
previo falla por cualquier causa que no sea "el archivo no existe" — confirmado por ejecución, no
solo por lectura.**

`secretos.rs:770`:

```rust
let previo = std::fs::read_to_string(&ruta).unwrap_or_default();
```

`Result::unwrap_or_default()` descarta el `Err` y da `String::new()` sin importar la causa: archivo
ausente (el único caso que tiene sentido tratar como "vacío"), pero también permiso denegado,
contenido no UTF-8, o el mismo bloqueo transitorio de antivirus/sincronizador que el propio archivo
ya acepta como plausible y digno de manejo explícito en otro punto — es el escenario exacto que
motivó H3-bis en la segunda vuelta, aceptado entonces por el orquestador. La diferencia es que
H3-bis arregló la lectura *de `purgar_del_env_con`* (línea 896-906 actual, correcta: distingue
`NotFound` de cualquier otro error y propaga el segundo). Esta es una **lectura distinta**, dentro
de `guardar_clave_en`, la función a la que `purgar_del_env_con` delega la reescritura real — y
nunca recibió el mismo tratamiento.

Efecto: si `previo` "falla" para un `.env` que en realidad tiene contenido (claves de otros
proveedores), el bucle que sigue (774-792) itera cero líneas, `sustituida` queda en `false`, y el
bloque de la línea 794 escribe un archivo **nuevo** con únicamente la clave que se está guardando
en ese momento — perdiendo cualquier otra que hubiera. La función devuelve `Ok(Origen::Archivo(...))`:
éxito, sin ningún indicio de la pérdida.

**Lo confirmé por ejecución**, no solo por lectura del código: escribí un test de integración
desechable (`core/providers/tests/zz_temp_verificacion_revisor.rs`, fuera de `secretos.rs` para no
arriesgar el archivo bajo revisión mientras algo más lo mutaba — ver sección 0), lo corrí, y lo
borré después (`git status` limpio, confirmado). Reproduce el escenario: un `.env` con
`OPENAI_API_KEY=sk-openai-real` y `DEEPSEEK_API_KEY=sk-deepseek-real`, corrompido después a bytes
no UTF-8 (determinista y portable, sin depender de permisos de archivo — el mismo criterio que ya
usa este archivo para sus propios trucos de prueba). Salida real:

```
RESULTADO DE guardar_clave_en: Ok(Archivo("...\\.tmpNF1Eve\\.env"))
CONTENIDO FINAL DEL .env: "# Claves de API de dictar_ia. Permisos 0600.\nGEMINI_API_KEY=sk-nueva-gemini\n"

thread '...' panicked at core\providers\tests\zz_temp_verificacion_revisor.rs:51:5:
BUG CONFIRMADO: las claves de otros proveedores se perdieron. Contenido final: "# Claves de API de
dictar_ia. Permisos 0600.\nGEMINI_API_KEY=sk-nueva-gemini\n"
```

`OPENAI_API_KEY` y `DEEPSEEK_API_KEY` desaparecieron. `guardar_clave` reportaría éxito ("Guardada en
llavero del sistema" o "Guardada en `.env`", según el camino) mientras borra en silencio las claves
de los otros proveedores configurados — exactamente "le dice al usuario algo falso sobre el estado
de sus datos o sus claves", la categoría que el propio protocolo de esta HU amplió explícitamente en
la tercera vuelta para H4-ter.

**Aclaración importante: no es código de esta vuelta.** Confirmado contra `git show c08abdf7 --
core/providers/src/secretos.rs`: la línea 632 de esa versión (tercera vuelta, ya cerrada) es
idéntica, byte a byte. Existe desde antes de que `purgar_del_env` existiera como función separada.
Ninguna de las tres vueltas anteriores de esta HU lo señaló — lo encuentro ahora porque el encargo
de esta vuelta me llevó a leer `guardar_clave_en` completa (la función que H4-ter reescribió para
la parte de escritura atómica) y la lectura del contenido previo, dos líneas más arriba en esa misma
función, quedó fuera del alcance de esa corrección. Lo marco Bloqueante igual, no como deuda: el
protocolo no distingue "cuándo se introdujo" para la severidad, y el caso concreto (un usuario con
tres proveedores configurados que guarda o rota una clave y ve las otras dos desaparecer sin aviso)
encaja de lleno en la tabla.

### Importante

**`cargo fmt --all -- --check` falla sobre dos de las seis pruebas nuevas de esta vuelta —
confirmado por ejecución repetida, no sospecha.**

`secretos.rs:1105-1116` (`dos_guardados_seguidos_no_comparten_nombre_de_temporal`) y
`secretos.rs:1119-1144` (`si_falla_el_renombrado_no_queda_un_temporal_huerfano_con_el_secreto`)
tienen cada una una línea (`assert_ne!`/`assert!` con su mensaje) más larga de lo que `rustfmt`
acepta en una sola línea con el ancho configurado; `rustfmt` las quiere partidas en varias líneas
(salida completa en la sección 1). No cambia comportamiento — es solo formato — pero
`.github/workflows/ci.yml` corre `formato` como paso **separado y sin `continue-on-error`**, antes
de `compilar`, `clippy` y `tests`, con el comentario explícito "Pasos separados para que el fallo
diga cuál de los tres fue": tal como está este diff, el job `nucleo` (Linux, el único que compila
`core/providers` en CI hoy) fallaría en el primer paso, y ninguno de los siguientes llegaría a
correr — ni siquiera para confirmar que compila, aunque yo ya lo confirmé aparte. No lo marco
Bloqueante porque no es una falla de compilación ni de comportamiento, es trivial de corregir
(`cargo fmt`), y a diferencia de H4-ter no hay dato ni credencial en juego — pero señalo la
consecuencia práctica en CI con el mismo criterio que ya se aplicó una vez en esta HU ("si un
hallazgo obliga a discutir si la tabla lo cubre, la tabla está incompleta, no el hallazgo"), para
que el orquestador decida con el precedente ya sentado.

### Nota

**El cierre de la ventana de permisos en Unix (H4-ter) no cubre un archivo o symlink preexistente
en la ruta exacta del temporal — sospecha razonada, explotabilidad baja bajo los permisos
habituales.**

`escribir_temporal_restringido` (`secretos.rs:720-732`) usa `OpenOptions::new().write(true)
.create(true).truncate(true).mode(0o600)`, **sin** `create_new(true)`. Es correcto que esto cierra
la ventana temporal entre crear y proteger que motivó el hallazgo (verificado por razonamiento
POSIX, no solo por confiar en el comentario: `mode` en `open(2)` con `O_CREAT` se aplica de forma
atómica en la misma llamada al sistema, y `umask` solo puede *restar* bits de un modo pedido
explícitamente, nunca añadirlos, así que pedir `0o600` da como mucho `0o600` sin importar la umask
del proceso). Pero `mode` **solo se aplica cuando el `open` efectivamente crea el inodo**: si ya
existe un archivo (o un symlink) en `dir.join(".env.tmp.<pid>.<contador>")` — nombre predecible por
cualquiera que conozca el PID del proceso, ya que el contador arranca en 0 en cada proceso —
`escribir_temporal_restringido` lo abre, lo trunca y escribe el secreto ahí, pero **conserva los
permisos que ese archivo ya tenía**, no los restringe a 0600. Esto sí dejaría el secreto expuesto,
sin que nada lo detecte. No lo marco con severidad porque el escenario que lo dispara — otro usuario
u otro proceso plantando de antemano un archivo o symlink dentro de `dir_configuracion()` — exige
que ese tercero ya tenga permiso de escritura en el directorio de configuración *del propio usuario*
que corre `dictar_ia`, algo que las convenciones de permisos habituales (`$HOME/.config` o
`%APPDATA%`, no escribibles por otros por defecto) ya impiden; no es un vector de ataque
independiente. Pero es una precisión real sobre el alcance de "no queda ninguna ventana": no queda
ninguna ventana *temporal*, pero sí queda un supuesto no verificado (que nadie más puede escribir
ahí) del que depende la garantía completa, y ni el `HANDOFF` ni el comentario del código lo
mencionan.

## 3. Premisas que cuestiono

**Premisa (implícita en el propio módulo, consecuencia de cómo H3-bis se presentó y se cerró en la
segunda vuelta): que el archivo maneja correctamente los errores de lectura del `.env`, porque
`purgar_del_env_con` ya distingue "no existe" de cualquier otro error.**

Conclusión: la premisa es cierta para *una* de al menos dos lecturas independientes del mismo
archivo, y falsa para la otra. `purgar_del_env_con` (línea 896) sí distingue `NotFound` del resto,
correcto desde H3-bis. Pero `guardar_clave_en` — la función a la que `purgar_del_env_con` delega la
reescritura real, y que además es el camino directo de cualquier guardado que cae al `.env` sin
pasar por `purgar_del_env` — hace su **propia** lectura del mismo archivo (línea 770) con
`unwrap_or_default()`, sin ninguna de las precauciones de H3-bis. H3-bis cerró el síntoma en el
lugar donde el hallazgo original lo señaló, pero no en la función vecina que comparte el mismo
riesgo. Ver el hallazgo Bloqueante de arriba: no es un caso hipotético, lo reproduje.

**Premisa (`HANDOFF`, sección "Lo que no llegué a cubrir con una prueba, y por qué", sobre el
camino de error de la escritura del temporal en H4-ter): que forzarlo de forma determinista y
portable "exige... depender de permisos de sistema de archivos... o bien predecir de antemano el
nombre exacto que va a generar `nombre_temporal`".**

Conclusión: la premisa es cierta para probar el camino **completo**, de punta a punta, a través de
`guardar_clave_en` — ahí sí hace falta predecir el nombre exacto, y el contador global compartido
por tests en paralelo lo hace impráctico, coincido. Pero no es la única forma de obtener cobertura
real: `escribir_temporal_restringido` (`secretos.rs:720-732` en Unix, `739-742` en el resto) es una
función privada del mismo módulo, así que `mod tests` puede llamarla **directamente**, con una ruta
elegida a mano — sin pasar por `nombre_temporal` ni por el contador. El mismo truco que ya usa
`si_falla_el_renombrado_no_queda_un_temporal_huerfano_con_el_secreto` para el `rename` (un directorio
en vez de un archivo en la ruta de destino) funciona igual aquí, sin ninguna dependencia del
contador: `escribir_temporal_restringido(&dir.join("obstaculo"), "contenido")` con `"obstaculo"` ya
creado como directorio falla de forma determinista y portable en cualquier plataforma. Esto no
cerraría la cobertura de punta a punta del *wrapper* de limpieza dentro de `guardar_clave_en` para
esta rama específica — para eso sí hace falta el contador, y ahí la premisa se sostiene —, pero sí
hubiera dado cobertura real de que la función en sí devuelve `Err` cuando la escritura falla, algo
que hoy no verifica ningún test. La premisa "no lo encontré" es cierta para el camino completo, pero
había una vía parcial, alcanzable, que no se exploró.

## 4. Qué verifiqué y no marqué

- **H4-ter, la ventana en Unix.** Leí `escribir_temporal_restringido` completa y razoné la
  atomicidad de `OpenOptions::mode` con `O_CREAT` contra la semántica POSIX de `open(2)` y `umask`
  (no solo acepté el comentario del código): confirmado que no hay ventana temporal. El gap de
  archivo/symlink preexistente que sí encontré queda en la sección 2 como Nota, no invalida esto.
- **H4-ter, la limpieza en los dos caminos falibles.** Leí `guardar_clave_en` completa
  (`secretos.rs:760-844`): los dos `if let Err(e) = ... { let _ = std::fs::remove_file(&temporal);
  return Err(e); }` (escritura y `rename`) están, cada uno limpia antes de propagar, y el error que
  se propaga es siempre el original, nunca el de la propia limpieza (`let _`, no `?`, confirmado
  línea por línea). Confirmado además por ejecución: `si_falla_el_renombrado_no_queda_un_temporal_
  huerfano_con_el_secreto` corrió y pasó (`ok`) en mi corrida real de `cargo test`, no solo leída.
- **H4-ter, Windows.** Comparé `escribir_temporal_restringido` bajo `#[cfg(not(unix))]`
  (`secretos.rs:739-742`, `std::fs::write(temporal, contenido)` sin ningún endurecimiento) contra
  el código de Windows anterior a esta vuelta (`git show c08abdf7`, línea 674 de esa versión: la
  misma llamada, sin ningún `set_permissions` porque ese bloque siempre estuvo bajo `#[cfg(unix)]`).
  Idénticas en sustancia: en Windows, el temporal nunca tuvo ni tiene ningún endurecimiento de
  permisos, ni antes ni después de esta vuelta. **No es regresión**, es el mismo estado preexistente,
  ya trazado como M2/P3 (nota, al tablero) en la tercera vuelta. Confirmado por lectura de ambas
  versiones, no solo por la caracterización del `HANDOFF`.
- **H2-ter.** Leí el docblock completo de `escribir_en_llavero` (`secretos.rs:293-339`) contra el
  código real (340-372): el párrafo "Decisión sobre qué pasa si..." describe con precisión lo que
  el código hace — no purga, sí escribe en el `.env`, informa `Origen::Archivo` — y nombra el riesgo
  (duplicación) sin suavizarlo. No encuentro que esconda nada más allá de lo que ya dice. La
  comparación de valor (`Ok(releido) if releido == v`) está, y usa `keyring::Error::Invalid`, una
  variante real del crate (confirmado en vueltas anteriores contra el código fuente descargado; no
  volví a descargarlo esta vuelta, lo doy por bueno porque el tipo ya se usaba sin objeciones en la
  tercera vuelta y nada de esta vuelta lo toca).
- **P1, si el mensaje llega al usuario.** Trazé la cadena completa: `guardar_clave_orquestada`
  (`secretos.rs:613-653`) envuelve el error con `.map_err(...)` (625-643) preservando `e.kind()` y
  el texto original; `puente.rs:403-406` (`guardar_clave`) hace
  `.map_err(|e| e.to_string())` sobre el `Result` completo — leído directo del archivo, no citado
  del `HANDOFF` — así que el texto armado en `secretos.rs` (con "ya se guardó en el llavero...")
  llega intacto hasta el límite de `flutter_rust_bridge`. De ahí a una pantalla legible depende de
  `app/lib/pantallas/ajustes.dart`, que no tiene `try/catch` en `_guardar` — ya señalado en la
  tercera vuelta (M4, al tablero, fuera de los archivos de esta vuelta) — así que "llega al usuario"
  es cierto hasta la excepción Dart, no hasta una notificación visible. No es un hallazgo nuevo, es
  la confirmación exacta del límite que ya se había declarado.
- **V1, mi juicio sobre tipos vs. prueba.** Confirmado por lectura de `ResolverDeEntorno`/
  `ResolverDeLlavero` (`secretos.rs:481-497`), `cadena_con` (524-535) y `resolver_por_defecto`
  (545-551): el intercambio que preocupaba en `resolver_por_defecto` ya no compila. Mi conclusión:
  **una protección de tipos es más fuerte que una prueba** para esta clase de error, no como regla
  general sino para este caso concreto — no depende de que alguien recuerde correrla, no se puede
  desactivar con un `#[ignore]` accidental, y falla en el momento más temprano posible. El hueco que
  el propio `HANDOFF` declara sin cerrar — dentro de `cadena_con`, línea 534,
  `ensamblar_cadena_por_defecto(entorno.0, origen_archivo, Box::new(dotenv), llavero.0)`, donde
  `entorno.0` y `llavero.0` vuelven a ser `Box<dyn KeyResolver>` sueltos — es real, lo confirmé
  leyendo esa línea, y ahí sí solo protege la prueba (`cadena_con_no_intercambia_el_archivo_con_
  el_llavero`, línea 1214, confirmada pasando en mi corrida). Dato curioso, no planeado: durante
  esta sesión observé (sección 0) que algo externo efectivamente intentó el intercambio en
  `resolver_por_defecto` — y el síntoma que le llegó al orquestador fue exactamente un error de
  compilación, no un test rojo. Es, por accidente, una confirmación en vivo de que la protección
  funciona como se documenta.
- **V2.** `guardar_en_el_llavero_purga_una_copia_vieja_escrita_en_minuscula` (línea 1434) y
  `purgar_del_env_reconoce_una_clave_escrita_en_minuscula` (línea 1589): confirmadas `ok` en mi
  corrida real de `cargo test`, no solo leídas.
- **M1.** `CONTADOR_TEMPORALES_ENV` (línea 691) y `nombre_temporal` (701-707): confirmado que usa
  `AtomicU64::fetch_add` (incremento real, no una lectura sin incrementar). Prueba
  `dos_guardados_seguidos_no_comparten_nombre_de_temporal` (línea 1105), confirmada `ok`.
- **Que ninguna prueba toque el llavero real** (H2, primera vuelta, para no perder la pista con
  tanto código nuevo alrededor). `grep` de `escribir_en_llavero` en todo el archivo: como
  identificador solo aparece en su propia definición (dos veces, por plataforma) y como valor
  pasado a `guardar_clave_orquestada` dentro de `guardar_clave` (línea 598, código de producción);
  cero apariciones dentro de `mod tests`. Confirmado además por ejecución, no solo por `grep`: tras
  correr `cargo test -p dictar-providers` completo, listé las credenciales de Windows
  (`cmdkey /list` vía PowerShell, ya que Git Bash traduce mal el `/list`) y busqué "dictar" en las
  14 entradas: cero coincidencias.
- **Anchos de línea, con mi propio script, no citando a nadie.** Python, decodificado UTF-8,
  `len()` por línea sobre las 1794 líneas actuales: máximo real 97 caracteres (línea 214), cero por
  encima de 100. Coincide con lo que declaran el `HANDOFF` y las vueltas anteriores, pero lo conté
  yo.
- **Que el único archivo tocado sea `secretos.rs`.** `git status --porcelain` sobre los cuatro
  archivos de `PLAN.md`: solo `secretos.rs` aparece; `Cargo.toml`, `puente.rs` y
  `repositorio_rust.dart` están limpios. Confirmado también que el diff final (347 inserciones/52
  borrados contra `c08abdf7`) es el mismo que capturé al principio de la sesión, pese a las
  mutaciones transitorias de la sección 0.

## 5. Qué no pude verificar y qué haría falta

- **El comportamiento real de `entrada.get_password()` contra un backend real** (Credential
  Manager, Secret Service, Keychain) comparado con el valor recién escrito: sigue sin cubrirse por
  ninguna prueba automática, y esta máquina (Windows, sin ejercitar el llavero real en ningún test)
  no lo confirma. Mismo límite que las cuatro vueltas anteriores.
- **Compilación cruzada a Android y comportamiento en Linux/macOS real** (D-Bus sin sesión,
  `zbus`): no verificable desde esta máquina Windows. `cargo test -p dictar-providers` solo ejercitó
  la rama de Windows del `#[cfg]`.
- **El resto del workspace** (`cargo build --workspace`, `cargo clippy --workspace`, incluidos
  `core/audio-capture` y `app/**`): fuera de mi alcance por indicación explícita del encargo, y
  tocado ahora mismo por otra sesión.
- **Qué proceso está mutando `secretos.rs` y si sigue activo.** No lo investigué más allá de
  documentar los síntomas (sección 0): no tengo forma de identificar el proceso concreto desde esta
  sesión sin arriesgarme a interferir con lo que sea que esté haciendo. Haría falta que el
  orquestador confirme si hay una `verificador-pruebas` corriendo en paralelo sobre este mismo
  archivo y, si es así, que coordine el orden en vez de dejarlos concurrentes.
- **La versión final, verdaderamente estable, de `secretos.rs`.** Todo lo que reporto arriba lo
  verifiqué contra el hash `9dc5f74bc34d98a30ace6cf7ea9fe1f389db71c20463da97562c2810ae9b2e61`, que
  coincide con el diff de 347/52 líneas contra `c08abdf7`. Si al leer esto el archivo tiene otro
  hash, esta revisión describe un estado que ya no es el actual y hay que repetirla contra el nuevo.
