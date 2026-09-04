# PLAN — HU-05 «Claves de API en el llavero del SO»

HU de origen: [`docs/06-historias-de-usuario.md#hu-05`](../../../docs/06-historias-de-usuario.md)
· Tarea T-3 del [tablero](../../tablero.md)

**Revisión 2**, tras el contradictor. Los cambios respecto de la revisión 1 están justificados
en «Respuesta al contradictor», al final.

## Análisis

`core/providers/src/secretos.rs` ya está construido como una **cadena de resolutores**:
`resolver_por_defecto()` (líneas 271-281) compone hoy dos eslabones —entorno y archivo `.env`—
con un patrón *builder* al que añadir un tercero es trivial. El comentario de cabecera lo dice
literalmente: *«Pendiente de `libsecret`; el hueco ya está previsto»*.

Y ya existe el tipo que hace falta para el retorno: `Origen` (líneas 216-232) distingue
`Llavero => "llavero del sistema"` de `Archivo(p) => p.display()`. No hay que inventarlo.

**Fuera de alcance:** cifrar la base de datos (HU-06, que depende de esta) y migrar claves que
ya estén en un `.env` — se resuelve solo, porque el `.env` conserva prioridad sobre el llavero.

## Verificación previa, y es bloqueante

El paso 1 no es una formalidad: **si sale mal, el plan cambia entero.** El implementador no
escribe una línea antes de resolver esto y anotarlo en el `HANDOFF.md`:

1. **Qué crate y qué versión**, y **qué *features* exactas**. El contradictor comprobó que
   `keyring` 4.2.0 no es un backend monolítico sino una arquitectura conectable, y que su
   conjunto por defecto resuelve Linux con `zbus-secret-service-keyring-store` — **cliente D-Bus
   en Rust puro, sin enlazar `libsecret`**.
2. **Que Android quede fuera por construcción.** `android-native-keyring-store` no está en el
   conjunto por defecto, y `core/providers` se enlaza en `dictar-api`, que compila a Android.
   La dependencia se condiciona con el patrón que ya usa el repositorio:
   `core/screen-capture/Cargo.toml` condiciona `xcap` a
   `cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))`. Ese es el
   precedente a copiar, literalmente.
3. **Si `libsecret-1-dev` sobra en el CI y en `INSTALL.md`.** Aparece desde el commit inicial
   del repositorio, antes de que existiera `secretos.rs`: es una suposición de apertura, nunca
   verificada. Si sobra, **se anota como deuda y no se toca aquí** — esos archivos pertenecen a
   HU-01, que está en curso en paralelo.

## Archivos y paquetes afectados

Lista cerrada. Comparada con [`HU-01-wasapi/PLAN.md`](../HU-01-wasapi/PLAN.md): sin
solapamiento.

| Ruta | Cambio esperado |
|---|---|
| `core/providers/Cargo.toml` | Dependencia del llavero, condicionada a escritorio |
| `core/providers/src/secretos.rs` | `LlaveroResolver` nuevo, alta al final de la cadena, y `guardar_clave` devolviendo `Origen` en vez de `PathBuf` |
| `core/api/src/puente.rs` | **Una línea**: el `.map(|r| r.to_string_lossy()…)` de `guardar_clave` pasa a formatear el `Origen`. La firma pública del puente **no cambia**, así que no dispara B-5 |
| `app/lib/datos/repositorio_rust.dart` | El comentario de las líneas 293-296 dice «Devuelve el archivo donde quedó»; deja de ser cierto |

**No se toca** ningún workflow, ni `INSTALL.md`, ni `app/lib/src/rust/puente.dart` (es generado;
ver la deuda de abajo).

## Dependencias

- HU previas: ninguna. **HU-06 depende de esta.**
- Bloqueos: **B-1** (no hay `cargo`), así que no se puede compilar ni probar; el cierre será
  condicionado. **B-5** (no hay `flutter_rust_bridge_codegen`) no bloquea esta HU porque la
  firma del puente no cambia, pero sí tiene una consecuencia — ver deuda.

## Casos del dominio que hay que cubrir

- [x] **Sin llavero disponible** — WSL sin D-Bus, un contenedor, un servidor sin sesión gráfica.
      Es el criterio 4 y el caso más probable en la práctica. La aplicación **sigue funcionando**
      con entorno y `.env`; no falla al arrancar.
- [x] **Sin clave de API y sin conexión** — el flujo local con Ollama sigue sirviendo.
- [x] **La clave no aparece en ningún log.** Ni truncada, ni en un `Debug` derivado, ni en un
      mensaje de error, ni en un campo de `tracing`. Es el criterio 5, y el vector real es
      `tracing`, no el `Result` — ver pruebas.
- [x] **El llavero se consulta en cada petición**, no una vez al arrancar: `resolver_por_defecto()`
      se reconstruye en cada uso a propósito (`core/api/src/lib.rs:129-131`), para que una clave
      añadida al `.env` surta efecto sin reiniciar. Con el llavero eso son consultas reales a
      D-Bus por cada carga de ajustes y cada `procesar_sesion`. **Decisión: se acepta sin
      caché.** Una lectura de llavero son pocos milisegundos, y meter caché rompería justo la
      propiedad que ese comentario defiende. Si un D-Bus lento lo hace notar, se revisa con
      datos.

## Riesgos

| Riesgo | Mitigación |
|---|---|
| El crate arrastra dependencias de sistema que complican el `.deb` y el `.exe` | La verificación previa lo resuelve antes de escribir código. `docs/05-empaquetado.md` manda |
| En Android no hay llavero, y `dictar-api` compila a Android | Dependencia condicionada a escritorio, y el resolutor devuelve «sin llavero». Invariante 4 |
| Un fallo del llavero rompe el arranque | El resolutor devuelve `None` ante cualquier error, nunca propaga. Es una cadena: el eslabón que no sabe, calla |
| El texto de un `keyring::Error` podría incluir el valor del secreto | No verificado por nadie todavía. El implementador lo comprueba antes de dar por buena la política «error → `None`» |

## Estrategia

1. La verificación previa de arriba. Bloqueante.
2. Añadir la dependencia, condicionada a escritorio.
3. `LlaveroResolver` implementando `KeyResolver`, con la política «error → `None`».
4. Darlo de alta **al final** de la cadena, sin alterar el orden de los dos primeros.
5. `guardar_clave` devuelve `Origen`; escribe en el llavero, con respaldo al `.env` si no hay.
6. Ajustar la línea de `puente.rs` y el comentario de `repositorio_rust.dart`.
7. Pruebas.

## Pruebas necesarias

| Prueba | Cubre | Mutación esperada que la pone en rojo |
|---|---|---|
| `el_llavero_va_despues_del_entorno_y_del_env` | AC 2 | Reordenar la cadena para poner el llavero primero |
| `el_resolutor_real_del_llavero_no_entra_en_panico_sin_sesion` | AC 4 | Cambiar el `.ok()` por un `.unwrap()` |
| `un_error_del_llavero_se_convierte_en_none` | AC 4 | Propagar el error con `?` en vez de devolver `None` |
| `ningun_evento_de_tracing_contiene_el_valor_de_la_clave` | AC 5 | Añadir `tracing::warn!(valor = %clave, …)` |
| `el_texto_del_error_no_contiene_la_clave` | AC 5 | Incluir la clave en el mensaje del error |
| `guardar_en_el_llavero_informa_de_su_origen_no_de_una_ruta` | AC 3 | Devolver un `Origen::Archivo` fabricado |

Dos precisiones que la revisión 1 tenía mal:

- **Las pruebas de AC 4 ejecutan el `LlaveroResolver` real, no un doble.** Un doble de
  `KeyResolver` que devuelve `None` no ejecuta ni una línea del resolutor real: probaría otra
  vez que la cadena sigue buscando, que ya está cubierto por
  `la_cadena_sigue_buscando_si_el_primero_no_la_tiene` (`secretos.rs:478-490`).
- **Y no pueden depender de que el llavero esté ausente.** El CI de Linux no tiene sesión
  gráfica, pero la máquina de un desarrollador con GNOME sí tiene llavero. Una prueba que
  asuma una de las dos cosas es frágil en la otra. Así que
  `el_resolutor_real_del_llavero_no_entra_en_panico_sin_sesion` afirma **solo** que devuelve un
  `Option` sin entrar en pánico, sea cual sea el entorno; y
  `un_error_del_llavero_se_convierte_en_none` fuerza el error con un nombre de servicio
  imposible, que falla en los dos entornos por igual.

El doble `MapResolver` (`secretos.rs:462-476`) sí sirve, y se reutiliza, para
`el_llavero_va_despues_del_entorno_y_del_env`: eso es composición de cadena, y da igual qué
resolutor concreto ocupe cada eslabón.

## Deuda que este plan abre a propósito

- `app/lib/src/rust/puente.dart` (generado) repite el comentario «Escribe en
  `~/.config/dictar_ia/.env` con permisos 0600», que queda obsoleto. **No se puede corregir
  mientras dure B-5**: el archivo se regenera, no se edita. El `HANDOFF.md` lo declara.
- Si `libsecret-1-dev` sobra en `ci.yml`, `release.yml` e `INSTALL.md`, se anota y lo hace quien
  cierre — esos archivos son de HU-01.
- `nombres_candidatos()` (`secretos.rs:29-45`) añade el sufijo `_API_KEY`, que no tiene sentido
  para la clave de cifrado de HU-06. El primitivo de bajo nivel sirve; la envoltura
  `guardar_clave` está más acoplada a «clave de API» de lo que HU-06 necesitará. Lo decide HU-06.

## Agentes a lanzar

`implementador`, y a la vuelta `revisor-codigo` + `verificador-pruebas`.
`auditor-plataforma` **sí**: se condiciona una dependencia por plataforma y afecta al
empaquetado y al cruce a Android.

---

## Respuesta al contradictor

| # | Objeción | Veredicto del orquestador |
|---|---|---|
| 2.1 | El retorno de `guardar_clave` obliga a tocar `puente.rs`, y la lista «cerrada» lo prohibía sin decir cómo evitarlo. Además `repositorio_rust.dart` tiene un comentario que queda falso, y el generado `puente.dart` también | **Aceptada entera.** Se usa `Origen`, que ya existe y ya distingue llavero de archivo — la alternativa que el contradictor describía (fabricar un `PathBuf` con el texto «llavero del sistema») queda descartada por escrito: es exactamente la clase de mentira de tipos que este repositorio evita. `puente.rs` y `repositorio_rust.dart` entran en la lista. Se confirma el dato que lo hace viable: la firma pública del puente no cambia, así que **B-5 no bloquea esta HU** |
| 2.2 | Dos de las cuatro pruebas no protegen lo que dicen: un doble de `KeyResolver` no ejecuta el resolutor real, y la prueba de AC 5 solo cubre el texto del error cuando el vector real es un campo de `tracing` | **Aceptada.** Las de AC 4 pasan a ejecutar el resolutor real. Y **corrijo la propuesta del contradictor en un punto**: sugería apoyarse en que el CI de Linux no tiene sesión gráfica, pero eso hace la prueba frágil en la máquina de un desarrollador con GNOME, donde sí hay llavero. Se replantean para que valgan en ambos entornos. Se añade la prueba de captura de `tracing`, que era el agujero real |
| 2.3 | El supuesto central seguía sin verificar, y la evidencia apunta a que `libsecret-1-dev` —presente desde el commit inicial— es una suposición errónea: el backend Linux por defecto de `keyring` 4.2.0 es `zbus`, en Rust puro. Y Android no está en el conjunto por defecto | **Aceptada, y ascendida.** La verificación pasa de ser el paso 1 de la estrategia a una sección propia y **bloqueante**, con los tres puntos concretos y el precedente exacto a copiar (`core/screen-capture/Cargo.toml`). Lo de `libsecret-1-dev` va a deuda y **no** se toca aquí: `ci.yml` pertenece a HU-01, que corre en paralelo, y tocarlo rompería la regla de archivos disjuntos |
| 2.4 | Nota: la cadena se reconstruye en cada uso, así que habrá una consulta real al llavero por cada carga de ajustes y cada proceso | **Aceptada como caso del dominio, con decisión explícita:** se acepta sin caché, y queda escrito el porqué. Meter caché rompería la propiedad que defiende el comentario de `core/api/src/lib.rs:129-131` |
| 2.5 | Nota: HU-06 reutilizaría una convención (`_API_KEY`) pensada para claves de API | **Aceptada como deuda**, a decidir por HU-06. El primitivo de bajo nivel sirve; la envoltura no del todo |
| — | No verificado por el contradictor y trasladado al implementador: que `keyring` 4.2.0 compile para `aarch64-linux-android`, y si algún `keyring::Error` incluye el valor del secreto | Ambos en Riesgos, y el segundo condiciona la política «error → `None`» |
