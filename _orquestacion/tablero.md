# Tablero

Backlog real en [`docs/06-historias-de-usuario.md`](../docs/06-historias-de-usuario.md); esto es
el estado operativo. Última actualización: 2026-09-03.

Sin límites WIP numéricos. El límite es otro: **varias tareas a la vez solo si sus `PLAN.md`
declaran archivos disjuntos.**

## Bloqueos del entorno

Afectan a todo lo demás, así que van primero.

| # | Bloqueo | Efecto | Sale con |
|---|---|---|---|
| B-1 | **No hay toolchain de Rust.** Verificado el 03/09: no existen `cargo`, `rustc` ni `rustfmt`, ni en Windows ni en `Ubuntu-26.04` de WSL | Ninguna compuerta de Rust se puede ejecutar. Todo cambio en el núcleo se entrega sin comprobar y todo cierre queda condicionado | Instalar Rust 1.75+ (lo que exige el workspace) |
| B-2 | **Flutter desactualizado.** Dart 3.11.4 contra el `sdk: ^3.12.2` de `app/pubspec.yaml` | `flutter pub get` falla, y con él `analyze` y `test`. Además el analizador del IDE marca como indefinidos hasta `Size` y `debugPrint`: **ese ruido no es un hallazgo** | `flutter upgrade` |
| B-3 | **No hay NDK de Android ni `cargo-ndk`** | El cruce a Android no se puede probar. `cargo ndk build -p dictar-api` nunca se ha ejecutado | Depende de B-1, más el NDK |
| B-4 | El `target/` de 21 GB tiene artefactos de Linux (`.so`), de otra máquina | No sirve de evidencia de nada acá | — |
| B-5 | **No está `flutter_rust_bridge_codegen`** | Ninguna función ni campo nuevo puede cruzar el puente. Bloquea T-5 entera y los criterios 2 y 3 de HU-04 | `cargo install flutter_rust_bridge_codegen`; depende de B-1 |

**Consecuencia operativa:** hoy solo se puede cerrar de verdad trabajo que no dependa de
ejecutar nada — documentación, estructura de archivos, y comprobaciones con Python. Todo lo
demás sale como *APROBADA, cierre condicionado*. Resolver B-1 es la acción de mayor palanca del
proyecto ahora mismo.

## Regla suspendida

**Rama por tarea, suspendida.** El protocolo la exige, pero el árbol tiene 24 archivos
modificados sin commitear de trabajo anterior, y crear ramas ahora los arrastraría a todas.
Vuelve a estar en vigor en cuanto el PO autorice el primer commit. Hasta entonces todas las
tareas trabajan sobre el árbol de `main`, sin commit, y el aislamiento lo da la regla de
archivos disjuntos.

Consecuencia añadida: `verificador-pruebas` **no puede usar `isolation: "worktree"`**, porque un
worktree se crea desde un commit y no llevaría nada que mutar. Se lanza sin aislamiento, con
instrucción explícita de revertir cada mutación y de pegar `git status` al empezar y al terminar.

## El orquestador ya no implementa — decisión del PO, 2026-09-03

El PO revirtió el permiso que este protocolo daba al orquestador para implementar tareas chicas.
Está razonado en `protocolo.md`, en el rol «Orquestador». Efecto inmediato: **toda implementación
va a un `implementador`, incluida la corrección de un `assert`.**

Deja una deuda concreta, porque bajo la regla vieja el orquestador implementó tres cosas hoy y
**ninguna ha sido revisada por nadie**. Es T-15.

## Segundo corte por límite de cuota — 2026-09-03

Los tres revisores de la tercera vuelta de HU-05 murieron con `rate_limit` (HTTP 429, «resets
7:40pm»). Uno alcanzó a anunciar que iba a escribir su informe.

**Esta vez sí se comprobó bien**, aplicando la lección del primer corte —comparar contenido, no
solo integridad—: `grep "Tercera vuelta"` sobre los tres `REVIEW-*.md` da **cero** en los tres.
Ninguno escribió. Los informes se quedan en la segunda vuelta y hay que relanzar los tres.

## Corte por límite de cuota — 2026-09-03

**Los cinco revisores en vuelo murieron a la vez** con `rate_limit` (HTTP 429, «session limit,
resets 2:10pm»). No es un fallo del trabajo ni de los prompts: se agotó la cuota de la sesión.
Dos alcanzaron a anunciar que iban a escribir su informe. El orquestador comprobó los doce
archivos y concluyó que ninguno lo había hecho — **y se equivocó**: el `verificador-pruebas` de
HU-05 **sí** había terminado su sección antes de caer. Terminar en frases completas no distingue
«no escribió» de «escribió entero», y no se comprobó la fecha ni el contenido. El agente
relanzado lo detectó y lo resolvió bien: hizo su pasada independiente **sin leer la anterior** y
comparó después. Lección: al retomar tras un corte, comparar contenido, no solo integridad.

Quedaron sin entregar:

| Tarea | Revisor | Encargo |
|---|---|---|
| HU-01, 3.ª vuelta | `revisor-codigo` | v2-1, v2-2, v2-3 |
| HU-01, 3.ª vuelta | `verificador-pruebas` | Las cuatro combinaciones de `alguna_pista_sigue_viva` |
| HU-05, 2.ª vuelta | `revisor-codigo` | Los diez hallazgos, y `purgar_del_env` en particular |
| HU-05, 2.ª vuelta | `verificador-pruebas` | Si el test de `tracing` es ya determinista |
| HU-05, 2.ª vuelta | `auditor-plataforma` | La asimetría de `#[cfg]` |

### Dos comprobaciones del orquestador, que NO son una revisión

Mientras dure el corte, y **solo** porque son verificables de forma exhaustiva sin compilador.
No sustituyen a ningún `REVIEW-*.md`: son un adelanto, y los revisores deben rehacerlas.

1. **HU-01, v2-1: el `break` sí cierra el canal.** Todo el arreglo del Bloqueante descansaba en
   que `bucle()` recibe `tx` por valor y no hay clones. Verificado: la firma lo toma por valor
   (`wasapi_src.rs:160`), **no hay ni un `.clone()` en todo el archivo**, y el único
   `spawn(move ||)` es el que crea el hilo y mueve el `Sender` dentro. Al retornar no queda
   ningún `Sender` vivo. La afirmación del implementador se sostiene.
2. **HU-05: la asimetría de `#[cfg]` es legítima.** Siete atributos positivos contra cuatro
   negativos, pero son **cuatro pares bien emparejados** (`LlaveroResolver` y sus dos `impl`, y
   `escribir_en_llavero`) más tres que solo pueden existir en escritorio: la constante
   `SERVICIO_LLAVERO`, `registrar_resultado_de_llavero` —que recibe un `keyring::Result`, un
   tipo que en Android no existe— y un test. Comprobado que **todas** las referencias a
   `keyring::` (líneas 267, 308, 312, 327 y las del test) caen dentro de ramas positivas:
   Android no ve el crate por ningún camino.

## En curso

| Tarea | Qué es | Estado | Desde |
|---|---|---|---|
| T-4 | [HU-01](../docs/06-historias-de-usuario.md#hu-01--grabar-en-windows) · WASAPI | **Segunda vuelta entregada, en re-revisión.** Los nueve hallazgos del [REVIEW.md](trabajo/HU-01-wasapi/REVIEW.md) corregidos. `verificador-pruebas` ✅ cierra sus seis puntos; `revisor-codigo` y `auditor-plataforma` en curso | 03/09 |
| T-3 | [HU-05](../docs/06-historias-de-usuario.md#hu-05--que-mis-claves-de-api-no-estén-en-texto-plano) · Llavero | **Condición de cierre, hallazgo del `auditor-plataforma`:** `README.md:78-80` afirma que el llavero «todavía no está implementado (pendiente de `libsecret`)». Las dos cosas serán falsas al cerrar esta HU, pero **no se toca antes**: si la HU volviera a implementación, el README mentiría en la otra dirección. Es distinto de T-11 —esa es la dependencia `libsecret-1-dev` en el empaquetado; esto es la afirmación de estado, y pertenece a HU-13—. Comprobado de paso que la referencia `docs/01-arquitectura.md:591` que cita T-11 sigue siendo correcta. · **Segunda vuelta en implementación.** Un Bloqueante ([REVIEW.md](trabajo/HU-05-llavero/REVIEW.md)): guardar una clave dice «Guardada en el llavero» mientras la aplicación sigue usando la vieja del `.env`. Causa raíz: una premisa equivocada del propio PLAN | 03/09 |

## Por hacer

Ordenado por lo que costaría más caro si se queda como está.

| # | Tarea | HU | Por qué | Agentes |
|---|---|---|---|---|
| T-2 | **Búsqueda en la interfaz** | [HU-04](../docs/06-historias-de-usuario.md#hu-04--buscar-en-todo-lo-que-he-grabado) | El backend existe y el puente expone `buscar`, pero `FraseDto` **no lleva `session_id`**: los criterios 2 y 3 no se pueden cumplir sin regenerar el puente. **Parcialmente bloqueada por B-5** | `contradictor`, `revisor-codigo` |
| T-5 | Importar audio desde la interfaz | [HU-03](../docs/06-historias-de-usuario.md#hu-03--importar-un-audio-que-grabé-con-otra-cosa) | Botón maqueta con backend completo detrás. **Bloqueada por B-5** | Los tres |
| T-6b | **Resto de T-6**: el botón de captura manual durante la grabación | HU-11 §4 | La parte previa a grabar ya está resuelta (ver Terminadas, T-6). Queda «Capturar diapositiva» **durante** la sesión, en `grabacion.dart`. No es una tarea chica: en la disposición compacta ese botón es el elemento principal y quitarlo obliga a repensarla. Necesita su `PLAN.md` | `contradictor` antes, luego `revisor-codigo` |
| T-7 | **El chequeo de «el núcleo va dentro» no corre en los PR** | HU-10 §3 | Hallazgo de `qa`. El paso que verifica que el `.exe` y el APK llevan la librería nativa solo vive en `release.yml`, que dispara con `tags: v*`. Una regresión que reintroduzca el fallo original del proyecto —paquete sin núcleo, aplicación con datos de demostración— **no se vería en el PR que la introduce**, solo al cortar un release. Misma clase de fallo silencioso, movido más tarde en el pipeline | `auditor-plataforma` |
| T-8 | `pipewire_src` no vacía el remuestreador al cerrar | Deuda de HU-01 | Hallazgo del `contradictor`. Pierde hasta ~21 ms por pista en **cada** cierre de sesión, hoy, en silencio. WASAPI sí lo hará, así que Linux queda inconsistente con Windows | `revisor-codigo` |
| T-9 | El cambio de dispositivo por defecto a mitad de sesión no se soporta | Deuda de HU-01 | Conectar unos auriculares durante la clase termina esa pista. Decidido fuera de alcance en HU-01, sin resolver en ninguna plataforma | `contradictor` sobre el alcance, antes de planear |
| T-11 | **Limpiar `libsecret-1-dev` — y NO hacerlo a ciegas** | Deuda de HU-05 | `libsecret-1-dev` está en el CI, en `release.yml`, en `INSTALL.md`, en el README y en `docs/01-arquitectura.md:591` desde el commit inicial, y el backend de `keyring` que se usa no lo necesita. **Pero el `auditor-plataforma` encontró la trampa:** `xcap` —dependencia de `core/screen-capture`, ajena a esta HU— arrastra `dbus` → `libdbus-sys`, que sí necesita `libdbus-1-dev` en Linux, y **eso no está declarado en ningún sitio**. Si hoy llega por accidente como dependencia transitiva de `libsecret-1-dev` vía `apt`, quitarlo rompería la captura de pantalla en Linux por un motivo que no tiene nada que ver con el llavero, y el error señalaría al sitio equivocado. Declarar `libdbus-1-dev` explícitamente **antes** de tocar nada | `auditor-plataforma` |
| T-16 | **`una_sesion_de_una_sola_pista_termina_si_esa_pista_muere` sigue sin proteger nada** | HU-01 | Hallazgo de T-15. El arreglo del orquestador (derivar `sistema_vivo` de `pistas_al_arrancar` en vez de escribirlo a mano) **no cambió qué mutaciones detecta la prueba**: el `assert_eq!(pistas_al_arrancar, vec![Track::Mic])` ya existía antes y es una igualdad exacta del vector, así que si pasa, `.contains(&Track::System)` solo puede dar `false`. Demostrado con `diff -u` contra la versión previa y simulando cuatro variantes. La prueba sigue siendo redundante al 100 % con `sin_dispositivo_de_salida_se_graba_solo_el_microfono` y `sin_ninguna_pista_viva_el_bucle_termina`. **Decidir: hacerla útil de verdad o borrarla.** Tres pruebas donde dos bastan es ruido, y el comentario afirma una protección que no existe | `implementador`, con `verificador-pruebas` detrás |
| T-15 | **Revisar lo que implementó el orquestador bajo la regla vieja** | HU-01 · HU-11 · HU-13 | Tres cambios sin revisar por nadie, hechos antes de que el PO prohibiera al orquestador implementar. (a) `README.md:93`, la afirmación falsa sobre el `.exe` desactivado. (b) **T-6**: `Ventana.esEscritorio` nuevo en `ventana.dart`, y con él se condicionan la tarjeta de región en `ajustes.dart` y el área más el interruptor de captura en `grabacion.dart`; además `_capturarPantalla` pasó a arrancar según plataforma. (c) **v3-1 y v3-2 de HU-01**: la prueba `con_las_dos_pistas_vivas_la_sesion_sigue` y el encadenamiento real en `una_sesion_de_una_sola_pista_termina_si_esa_pista_muere`, ambos en `sincronia.rs`. Ninguno pasó por `revisor-codigo` ni por `verificador-pruebas` | `revisor-codigo` + `verificador-pruebas` |
| T-14 | El fin del canal no dice por qué terminó | Deuda de HU-01 | Declarada por el implementador en la tercera vuelta. Al cerrarse el canal de audio, el consumidor no puede distinguir «terminó porque no queda ninguna pista viva» de «terminó porque se llamó a `detener()`». Hoy no rompe nada, pero una interfaz que quiera avisar al usuario de que la grabación murió sola necesita más que «el `Receiver` dejó de entregar» | `contradictor` sobre el alcance |
| T-13 | **Los tests de la rama `NoSoportada` no los ejecuta ningún job** | Deuda de HU-01 · HU-12 | Están gateados a «ni Linux ni Windows», y ningún job de `ci.yml` compila para un `target_os` así, de modo que **el test nunca corre**. Dos precisiones que acotan la urgencia, y conviene no perderlas: el `verificador-pruebas` señaló que **compilar cruzado no es ejecutar**, así que añadir un target no basta —haría falta un runner, probablemente `macos-latest`—; y el `auditor-plataforma` corrigió el alcance del problema: el job `android` de `release.yml` **sí compila el código de producción** de esa rama vía `cargo-ndk`, así que el riesgo de que la rama no compile ya está cubierto. Lo que falta es solo la ejecución del test. Coste de CI recurrente por un invariante verificable leyendo: lo decide el PO | `contradictor` sobre el alcance, antes de planear |
| T-12 | `rust-version = "1.75"` es una promesa falsa | Deuda de HU-05 | Hallazgo I-1. No es bloqueante —ningún workflow fija `rustc` ni usa `--locked`, así que el CI no se rompe—, pero el número del workspace no se corresponde con lo que las dependencias exigen de verdad | `revisor-codigo` |

## En revisión

| Tarea | Revisores | Desde |
|---|---|---|
| — | Los seis revisores entregaron. Ver veredictos en «En curso» | — |

## Terminadas

| Tarea | Qué se hizo | Veredicto | Cerrada |
|---|---|---|---|
| T-0 | Se crea el mecanismo: `_orquestacion/`, seis subagentes en `.claude/agents/`, y `docs/06-historias-de-usuario.md` con las 13 HU y su tablero | **TERMINADA.** Documentación y configuración; no depende de B-1 | 03/09 |
| T-1 | Auditoría de las cuatro HU en 🟡 por `qa`. Ninguna sube ni baja de color: HU-10 no es comprobable en absoluto sin toolchain; HU-11 tiene tres criterios cumplidos y el cuarto confirmado como incumplido (T-6); HU-12 cumple 3 y 4, y 1 y 2 quedan sin comprobar; HU-13 cumple 1, 2 y 3, y el 4 falló en un punto concreto | **TERMINADA.** Estados actualizados en `docs/06` con evidencia archivo+línea por criterio. Dio dos hallazgos nuevos: el error de `README.md:93` (corregido en el acto) y T-7 | 03/09 |
| T-6 | Las opciones de diapositivas ya no se ofrecen en móvil. `Ventana.esEscritorio` nuevo —distinto de `soportado`, que es si el gestor de ventanas respondió—, y con él se condicionan la tarjeta «Área de la diapositiva» en Ajustes y, antes de grabar, el área y el interruptor «Capturar diapositivas». Además `_capturarPantalla` ya no arranca en `true` en móvil, donde pedía una captura que solo podía devolver `NoSoportada` | **TERMINADA parcialmente**, la implementó el orquestador (que pierde el derecho a revisarla, `protocolo.md`). Queda T-6b. Sin compilar: B-1 y B-2 | 03/09 |
| T-10 | `README.md:93` afirmaba que el `.exe` de Inno Setup «está escrito pero desactivado hasta que Windows pueda grabar». Falso: `release.yml` lo compila y empaqueta en cada tag, sin condición que lo desactive. La frase se escribió antes de restaurar la pata de Windows y nadie la volvió a mirar | **TERMINADA.** Corregida. Cambio de una línea de documentación, no depende de B-1 | 03/09 |
