# Tablero

Backlog real en [`docs/06-historias-de-usuario.md`](../docs/06-historias-de-usuario.md); esto es
el estado operativo. Última actualización: 2026-09-03.

Sin límites WIP numéricos. El límite es otro: **varias tareas a la vez solo si sus `PLAN.md`
declaran archivos disjuntos.**

## Bloqueos del entorno

Afectan a todo lo demás, así que van primero.

| # | Bloqueo | Efecto | Sale con |
|---|---|---|---|
| ~~B-1~~ | **RESUELTO el 04/09.** La sesión hermana `dictar-ia-b2` instaló rustc/cargo 1.98.1 y las VS Build Tools (MSVC 14.44). Confirmado: `~/.cargo/bin` completo y `Cargo.lock` con 718 líneas nuevas resolviendo `keyring` | **Es el cambio de mayor impacto del proyecto.** Catorce revisiones cerradas como `NO VERIFICABLE` pasan a ser ejecutables, y ninguna HU seguía en *cierre condicionado* por otro motivo | ✅ |
| B-1b | **El SDK de Windows sigue instalándose**, así que `cargo test` en el host todavía no enlaza. Y en Git Bash el `link` de coreutils **ensombrece al `link.exe` de MSVC**: un error de enlazado raro es eso, no el código | Compilar sí, enlazar todavía no | Que termine la instalación; usar PowerShell o anteponer MSVC en el `PATH` |
| B-2 | **Flutter desactualizado.** Dart 3.11.4 contra el `sdk: ^3.12.2` de `app/pubspec.yaml` | `flutter pub get` falla, y con él `analyze` y `test`. Además el analizador del IDE marca como indefinidos hasta `Size` y `debugPrint`: **ese ruido no es un hallazgo** | `flutter upgrade` |
| B-3 | **No hay NDK de Android ni `cargo-ndk`** | El cruce a Android no se puede probar. `cargo ndk build -p dictar-api` nunca se ha ejecutado | Depende de B-1, más el NDK |
| B-4 | El `target/` de 21 GB tiene artefactos de Linux (`.so`), de otra máquina | No sirve de evidencia de nada acá | — |
| B-5 | **No está `flutter_rust_bridge_codegen`** | Ninguna función ni campo nuevo puede cruzar el puente. Bloquea T-5 entera y los criterios 2 y 3 de HU-04 | `cargo install flutter_rust_bridge_codegen`; depende de B-1 |

**Consecuencia operativa:** hoy solo se puede cerrar de verdad trabajo que no dependa de
ejecutar nada — documentación, estructura de archivos, y comprobaciones con Python. Todo lo
demás sale como *APROBADA, cierre condicionado*. Resolver B-1 es la acción de mayor palanca del
proyecto ahora mismo.

## Regla restaurada — 2026-09-04

**Rama por tarea, otra vez en vigor.** El PO autorizó commitear y la sesión hermana lo hizo:
cuatro commits en `main`, de `849e95e` a `a210562` — el mecanismo, HU-01, HU-05 y el resto.
Desaparece el motivo por el que estaba suspendida (un árbol con 24 archivos sin commitear que
cualquier rama habría arrastrado).

Dos consecuencias, y las dos importan:

- **`verificador-pruebas` vuelve a poder usar `isolation: "worktree"`.** Era imposible mientras
  no hubiera un commit desde el que crearlo, y le obligaba a copiar archivos al scratchpad a mano
  para no destruir el trabajo del implementador.
- **Y es la salida limpia al problema que la regla de archivos disjuntos no cubre.** Esa regla
  protege colisiones *dentro* del mecanismo; no sirvió cuando apareció otra sesión trabajando
  sobre el mismo árbol, y estuvo a punto de costar un rename global a medio aplicar. Con ramas,
  el aislamiento deja de depender de que dos sesiones se pongan de acuerdo por mensaje.

## B-6 — el CI sigue sin estrenarse, y ahora por otro motivo

Durante todo el 03/09 la excusa fue que nada estaba commiteado. Ya lo está y **empujado**, pero el
CI **sigue sin haber ejecutado ni uno solo de los arreglos de hoy**: el PR quedó **cerrado**, su
cabeza congelada en `2181f1c`, y un PR cerrado no recibe eventos `synchronize`.

Importa más de lo que parece, porque este tablero lleva el día entero apoyándose en «la compuerta
que vale es el CI» —está escrito incluso en el README— mientras el CI no veía nada. Y la prueba
de que no era retórica: en cuanto hubo compilador local aparecieron **cuatro errores de
compilación** en código que había pasado tres vueltas y tres revisores.

**Hasta que se reabra el PR, «el CI lo validará» no es una afirmación utilizable.** Es un botón, y
ya se le pidió al PO.

## Dos sesiones sobre el mismo repositorio — 2026-09-04

El PO puso a una segunda sesión, **`dictar-ia-b2`**, a implementar HU-02 (Android) más cuatro
puntos de saneamiento, en paralelo a esta. Se detectó por un cambio inesperado en `sincronia.rs`,
no por aviso previo, y se coordinó por `SendMessage`.

**El riesgo concreto que se evitó:** había renombrado `PistaWasapi` → `PistaCapturada` en
`sincronia.rs` **y** en `wasapi_src.rs` (10 ocurrencias). Un implementador de esta sesión tenía
encargo de editar el primero; si hubiera escrito sobre la copia que leyó al empezar, habría
deshecho el rename en un archivo y no en el otro, dejando el crate sin compilar. Se le avisó a
tiempo con instrucción de releer antes de escribir.

**Reparto acordado.** Suyos: `core/audio-capture/src/{aaudio_src.rs, lib.rs}`, todo
`app/android/**`, `app/lib/plataforma/android.dart`, `app/lib/pantallas/grabacion.dart` y
`.github/workflows/ci.yml`. Nuestros: `core/providers/src/secretos.rs`,
`app/lib/pantallas/ajustes.dart`, `app/test/` y `core/audio-capture/src/sincronia.rs` (ya
liberado por ella).

**HU-02 no ha pasado por el mecanismo.** Ella misma lo declaró en su `PLAN.md`: el PO se la
delegó directamente, sin `contradictor` ni los tres revisores. Queda como deuda de revisión
explícita, igual que T-15, y **no se da por cerrada**. Ofreció expresamente que se revise, «cuanto
más hostil mejor».

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

## Tanda «audio en vivo» — 2026-10-13

> **Estado a las 04:20:**
>
> - AV-1, AV-2 y AV-3 están **integradas** en la rama `audio-vivo` (`4f777ba`, `682f4cc` y `51f929f`).
> - El PO pidió priorizar un ejecutable: se cortó la verificación por mutación y AV-3 no pasó revisores.
> - El instalador está en `dist/`, y las pruebas para el PO en [`trabajo/PRUEBAS-PO-2026-10-14.md`](trabajo/PRUEBAS-PO-2026-10-14.md).
> - **Ninguna HU de la tanda se da por cerrada** hasta esas pruebas.
> - Las copias `../dictar_ia-av1`, `-av3`, `-av1-mut` y `-av2-mut` siguen existiendo; se borran tras la prueba del PO.

Prioridad del PO: mejorar la transcripción en vivo, a partir de [la investigación de Notion](../docs/investigacion-notion-reuniones-2026-10-12.md) §6 bis. Las tres tareas declaran archivos disjuntos y pueden ir en paralelo.

**Respaldo previo:** antes de empezar, el árbol de trabajo (72 archivos de octubre sin commitear) se guardó en `refs/respaldo/antes-audio-vivo-2026-10-13` (`73e141e`), sin tocar ramas ni índice. Para recuperar un archivo: `git show refs/respaldo/antes-audio-vivo-2026-10-13:<ruta>`.

| Tarea | Qué es | Estado | Desde |
|---|---|---|---|
| [AV-1](trabajo/AV-1-seguir-dispositivo/PLAN.md) | Reabrir la captura WASAPI cuando cambia o se invalida el dispositivo (cierra T-9 en Windows) | PLAN rev. 2 más adenda (el contradictor tumbó la rev. 1) · **ronda 3 del implementador** en `../dictar_ia-av1`. La ronda 2 dejó `cargo test -p dictar-audio` con 80 en verde, la prueba (d) cumplida contra la línea base y la sesión que ya no termina sola. Ronda 3: búfer WASAPI de 3 s, porque abrir el micrófono tarda hasta 2,5 s y una reapertura dejaría sin leer la pista del sistema. Pendiente: el camino de reapertura nunca se ejecutó (un solo dispositivo y sin privilegios de administrador); guion para el PO | 13/10 |
| [AV-2](trabajo/AV-2-salud-audio/PLAN.md) | Avisos en vivo de sin señal, nivel bajo, recorte y eco. Arregla además la prueba de widget rota | PLAN rev. 2 (rev. 1: umbral de voz igual al de silencio, eco sin alinear) · **entregada (parcial), en revisión**: el implementador declara 27 pruebas Rust y 43 de Flutter en verde, y D7 con eco en Comunicaciones y en la clase larga. Faltan D4 con optimización y las pruebas manuales (guion para el PO). Revisores: `revisor-codigo`, `verificador-pruebas` (en la copia `../dictar_ia-av2-mut`, ref `refs/revision/av-2`) y `auditor-plataforma` · **se integra con AV-1** | 13/10 |
| [AV-3](trabajo/AV-3-nivel-y-glosario/PLAN.md) | Nivelar el audio de sistema que va a Sherpa y glosario como palabras clave, con exploración de viabilidad primero | PLAN rev. 2 (rev. 1: métrica circular, reconocedor compartido, nivelar el micrófono amplifica el eco) · **implementador en curso**, en la copia aislada `../dictar_ia-av3` | 13/10 |

**Corte del 13/10:**

- **AV-1 y AV-2** murieron por `rate_limit` («resets 1:50am») a mitad de implementación. No dejaron HANDOFF. Su código está escrito en parte y **no se ha verificado que compile**.
- **AV-3** terminó la escritura, pero **no pudo compilar**: Smart App Control (Enforce) bloquea con `os error 4551` los *build scripts* que se compilan por primera vez en un `target/` nuevo. Dejó HANDOFF con las tres constantes de producción en `false`.
- **Comprobado tras el corte:** en el árbol principal solo cambiaron los archivos de AV-2. El resto es idéntico al respaldo, salvo finales de línea.

**Incidente de entorno, 13/10 ~04:00: el `target/` compartido contamina las pruebas.**

- **Qué pasó:** cargo calcula el hash de los crates del workspace relativo a la raíz, así que todas las copias generan el mismo ejecutable de pruebas y se pisan. El `auditor-plataforma` de AV-1 vio 8 fallos falsos con el `target/` compartido y 80/80 con uno propio. La suposición del orquestador de que «la ruta entra en el hash» era falsa.
- **Medidas:**
  - para `dictar-audio`, `target/` propio por copia (sí compila);
  - para `dictar-api`, serializar y repetir los resultados sorprendentes;
  - se avisó a los cuatro agentes activos;
  - todo resultado de prueba obtenido antes de esto con el `target/` compartido mientras otra copia compilaba el mismo crate queda **en duda** y se repite.

**Aislamiento:** todos los crates dependen de `dictar-audio`, que edita AV-1. Para que nadie compile código a medio editar de otro, AV-1 y AV-3 trabajan en copias aisladas (git worktree) creadas desde el respaldo `73e141e`. El orquestador integra cada diff (`git diff 73e141e`) en el árbol principal al cerrarlas.

Deudas nuevas que salieron de los contradictores:

| # | Deuda | Origen |
|---|---|---|
| T-18 | `cli/src/grabar.rs` gira al 100 % de CPU cuando el canal se cierra (`continue` ante `Disconnected`) y no drena `rx` al parar, así que pierde las últimas tramas | Contradictor de AV-1 |
| T-20 | **Clippy en rojo en `audio-vivo` por código de octubre** (`b7f93eb`): 3 errores en `core/api/src/transcriptor.rs` y 1 en `core/audio-capture/src/lib.rs:304`. El CI (`-D warnings`) fallaría. También: `cargo test -p dictar-api` sale 101 por un ICE de rustdoc en los doctests, así que se usa `--lib --tests`. Y Smart App Control bloquea el build script de `ring` en `target/release`; para medir con optimización, `--config profile.dev.opt-level=3` | Implementador de AV-2 |
| T-21 | **Huecos de 1,3 a 2,1 s entre tramas en WASAPI, también con el código anterior** (medidos por AV-1 en 60 s con tono, en las dos pistas). Con un búfer de 200 ms, un hueco de ese tamaño puede significar audio perdido, no solo relleno. Causa desconocida. Las aperturas del micrófono miden de 0,7 a 2,5 s, quizá relacionado | Implementador de AV-1, ronda 2 |
| T-19 | El micrófono no tiene aviso de `nivel_bajo`: hace falta un detector de voz propio y una medida de voz real a distancia normal para calibrarlo | Contradictor de AV-2 |

Siguientes, sin plan todavía:

- **AV-4:** micrófono en vivo sin eco.
- **AV-5:** captura por proceso.
- **AV-6:** reducción de ruido.
- **AV-7:** corte de frases y puntuación.

Dependen de una **referencia humana de 10 min**, que tiene que dar el PO.

## En curso

| Tarea | Qué es | Estado | Desde |
|---|---|---|---|
| T-4 | [HU-01](../docs/06-historias-de-usuario.md#hu-01--grabar-en-windows) · WASAPI | **Segunda vuelta entregada, en re-revisión.** Los nueve hallazgos del [REVIEW.md](trabajo/HU-01-wasapi/REVIEW.md) corregidos. `verificador-pruebas` ✅ cierra sus seis puntos; `revisor-codigo` y `auditor-plataforma` en curso | 03/09 |
| T-3 | [HU-05](../docs/06-historias-de-usuario.md#hu-05--que-mis-claves-de-api-no-estén-en-texto-plano) · Llavero | **Condición de cierre, hallazgo del `auditor-plataforma`:** `README.md:78-80` afirma que el llavero «todavía no está implementado (pendiente de `libsecret`)». Las dos cosas serán falsas al cerrar esta HU, pero **no se toca antes**: si la HU volviera a implementación, el README mentiría en la otra dirección. Es distinto de T-11 —esa es la dependencia `libsecret-1-dev` en el empaquetado; esto es la afirmación de estado, y pertenece a HU-13—. Comprobado de paso que la referencia `docs/01-arquitectura.md:591` que cita T-11 sigue siendo correcta. · **Segunda vuelta en implementación.** Un Bloqueante ([REVIEW.md](trabajo/HU-05-llavero/REVIEW.md)): guardar una clave dice «Guardada en el llavero» mientras la aplicación sigue usando la vieja del `.env`. Causa raíz: una premisa equivocada del propio PLAN | 03/09 |

| DEUDA-transcripcion-incremental | [HU-18 propuesta](../docs/plan-correccion-calidad-windows.md#hu-18--ver-cómo-la-transcripción-se-actualiza-mientras-ocurre-la-clase) · flujo STT local | **Plan inicial en `trabajo/DEUDA-transcripcion-incremental/PLAN.md`.** Aún no se cambia código: primero se mide Whisper vs Vosk en español y se protege el trabajo sin confirmar que ya toca `transcriptor.rs`, VAD y la pantalla de grabación. La selección depende de latencia/calidad en el equipo objetivo; falta el WAV separado del incidente para atribuir el eco. | 06/10 |

## Por hacer

Ordenado por lo que costaría más caro si se queda como está.

| # | Tarea | HU | Por qué | Agentes |
|---|---|---|---|---|
| T-2 | **Búsqueda en la interfaz** | [HU-04](../docs/06-historias-de-usuario.md#hu-04--buscar-en-todo-lo-que-he-grabado) | El backend existe y el puente expone `buscar`, pero `FraseDto` **no lleva `session_id`**: los criterios 2 y 3 no se pueden cumplir sin regenerar el puente. **Parcialmente bloqueada por B-5** | `contradictor`, `revisor-codigo` |
| T-5 | Importar audio desde la interfaz | [HU-03](../docs/06-historias-de-usuario.md#hu-03--importar-un-audio-que-grabé-con-otra-cosa) | Botón maqueta con backend completo detrás. **Bloqueada por B-5** | Los tres |
| T-6b | **Resto de T-6**: el botón de captura manual durante la grabación | HU-11 §4 | La parte previa a grabar ya está resuelta (ver Terminadas, T-6). Queda «Capturar diapositiva» **durante** la sesión, en `grabacion.dart`. No es una tarea chica: en la disposición compacta ese botón es el elemento principal y quitarlo obliga a repensarla. Necesita su `PLAN.md` | `contradictor` antes, luego `revisor-codigo` |
| T-17 | **El `.exe` no está firmado, y en Windows 11 moderno eso lo bloquea** | HU-10 · empaquetado | Ni `packaging/windows/inno_setup.iss` ni `release.yml` tienen una sola referencia a firma de código (verificado por `grep`). Consecuencias, de menor a mayor: en cualquier Windows, SmartScreen muestra «Windows protegió su PC» y hay que forzarlo a mano; y en un equipo con **Smart App Control** activo —el caso de la máquina del PO, y el de cualquier instalación limpia y reciente de Windows 11— **el instalador se bloquea sin opción de continuar**. Afecta igual a `dictar_api.dll`. El contraste está en Android, que sí lo tiene resuelto con `key.properties` porque un APK sin firmar no se puede distribuir; en Windows falta el equivalente. **No es un detalle de acabado: hoy el PO no podría instalar su propia aplicación.** Requiere un certificado de firma (un OV ronda 200-400 $/año y aún debe ganar reputación en SmartScreen; un EV la da desde el primer día y es lo que satisface a SAC), integrado con `SignTool` en el `.iss` y el certificado como secreto del repositorio. **Decisión de producto y de gasto: la toma el PO** | `contradictor` sobre el alcance, antes de planear |
| T-7 | ⚠️ **Dice la sesión hermana que la cerró — sin revisar.** Metió un job `android` en `ci.yml` que cruza el núcleo con `cargo ndk` y comprueba que `libdictar_api.so` está dentro del APK **en cada PR**, no solo tras un tag. Si es así, resuelve el hallazgo. **No se marca terminada hasta que lo verifique `auditor-plataforma`**: viene de HU-02, que no ha pasado por el mecanismo. El enunciado original, abajo | `auditor-plataforma` |
| T-7bis | **El chequeo de «el núcleo va dentro» no corría en los PR** | HU-10 §3 | Hallazgo de `qa`. El paso que verifica que el `.exe` y el APK llevan la librería nativa solo vive en `release.yml`, que dispara con `tags: v*`. Una regresión que reintroduzca el fallo original del proyecto —paquete sin núcleo, aplicación con datos de demostración— **no se vería en el PR que la introduce**, solo al cortar un release. Misma clase de fallo silencioso, movido más tarde en el pipeline | `auditor-plataforma` |
| T-8 | `pipewire_src` no vacía el remuestreador al cerrar | Deuda de HU-01 | Hallazgo del `contradictor`. Pierde hasta ~21 ms por pista en **cada** cierre de sesión, hoy, en silencio. WASAPI sí lo hará, así que Linux queda inconsistente con Windows | `revisor-codigo` |
| T-9 | El cambio de dispositivo por defecto a mitad de sesión no se soporta | Deuda de HU-01 | Conectar unos auriculares durante la clase termina esa pista. Decidido fuera de alcance en HU-01, sin resolver en ninguna plataforma | `contradictor` sobre el alcance, antes de planear |
| T-11 | **Limpiar `libsecret-1-dev` — y NO hacerlo a ciegas** | Deuda de HU-05 | `libsecret-1-dev` está en el CI, en `release.yml`, en `INSTALL.md`, en el README y en `docs/01-arquitectura.md:591` desde el commit inicial, y el backend de `keyring` que se usa no lo necesita. **Pero el `auditor-plataforma` encontró la trampa:** `xcap` —dependencia de `core/screen-capture`, ajena a esta HU— arrastra `dbus` → `libdbus-sys`, que sí necesita `libdbus-1-dev` en Linux, y **eso no está declarado en ningún sitio**. Si hoy llega por accidente como dependencia transitiva de `libsecret-1-dev` vía `apt`, quitarlo rompería la captura de pantalla en Linux por un motivo que no tiene nada que ver con el llavero, y el error señalaría al sitio equivocado. Declarar `libdbus-1-dev` explícitamente **antes** de tocar nada | `auditor-plataforma` |
| T-16 | **`una_sesion_de_una_sola_pista_termina_si_esa_pista_muere` sigue sin proteger nada** | HU-01 | Hallazgo de T-15. El arreglo del orquestador (derivar `sistema_vivo` de `pistas_al_arrancar` en vez de escribirlo a mano) **no cambió qué mutaciones detecta la prueba**: el `assert_eq!(pistas_al_arrancar, vec![Track::Mic])` ya existía antes y es una igualdad exacta del vector, así que si pasa, `.contains(&Track::System)` solo puede dar `false`. Demostrado con `diff -u` contra la versión previa y simulando cuatro variantes. La prueba sigue siendo redundante al 100 % con `sin_dispositivo_de_salida_se_graba_solo_el_microfono` y `sin_ninguna_pista_viva_el_bucle_termina`. **Decidir: hacerla útil de verdad o borrarla.** Tres pruebas donde dos bastan es ruido, y el comentario afirma una protección que no existe | `implementador`, con `verificador-pruebas` detrás |
| T-15 | **Revisar lo que implementó el orquestador bajo la regla vieja** | HU-01 · HU-11 · HU-13 | Tres cambios sin revisar por nadie, hechos antes de que el PO prohibiera al orquestador implementar. (a) `README.md:93`, la afirmación falsa sobre el `.exe` desactivado. (b) **T-6**: `Ventana.esEscritorio` nuevo en `ventana.dart`, y con él se condicionan la tarjeta de región en `ajustes.dart` y el área más el interruptor de captura en `grabacion.dart`; además `_capturarPantalla` pasó a arrancar según plataforma. (c) **v3-1 y v3-2 de HU-01**: la prueba `con_las_dos_pistas_vivas_la_sesion_sigue` y el encadenamiento real en `una_sesion_de_una_sola_pista_termina_si_esa_pista_muere`, ambos en `sincronia.rs`. Ninguno pasó por `revisor-codigo` ni por `verificador-pruebas` | `revisor-codigo` + `verificador-pruebas` |
| T-14 | El fin del canal no dice por qué terminó | Deuda de HU-01 | Declarada por el implementador en la tercera vuelta. Al cerrarse el canal de audio, el consumidor no puede distinguir «terminó porque no queda ninguna pista viva» de «terminó porque se llamó a `detener()`». Hoy no rompe nada, pero una interfaz que quiera avisar al usuario de que la grabación murió sola necesita más que «el `Receiver` dejó de entregar» | `contradictor` sobre el alcance |
| ~~T-13~~ | ✅ **CERRADA el 04/09, y mucho más barata de lo que este tablero suponía.** Decía que hacía falta un runner propio —`macos-latest`— porque «compilar cruzado no es ejecutar». El principio es cierto y **no aplicaba a este caso**: lo que estaba roto no era el comportamiento, era **la compilación**. Un `cargo check` cruzado lo caza entero, en segundos y sin runner. La sesión hermana añadió al job de Linux de `ci.yml` un paso con `rustup target add x86_64-unknown-freebsd` y `cargo check --target x86_64-unknown-freebsd -p dictar-audio --all-targets`, y verificó que **falla antes del arreglo y pasa después**. La lección: antes de pedir infraestructura, comprobar qué clase de defecto se busca — un fallo de compilación no necesita ejecución | ✅ |
| T-13bis | **Enunciado original: los tests de la rama `NoSoportada` no los ejecuta ningún job** | Deuda de HU-01 · HU-12 | Están gateados a «ni Linux ni Windows», y ningún job de `ci.yml` compila para un `target_os` así, de modo que **el test nunca corre**. Dos precisiones que acotan la urgencia, y conviene no perderlas: el `verificador-pruebas` señaló que **compilar cruzado no es ejecutar**, así que añadir un target no basta —haría falta un runner, probablemente `macos-latest`—; y el `auditor-plataforma` corrigió el alcance del problema: el job `android` de `release.yml` **sí compila el código de producción** de esa rama vía `cargo-ndk`, así que el riesgo de que la rama no compile ya está cubierto. Lo que falta es solo la ejecución del test. Coste de CI recurrente por un invariante verificable leyendo: lo decide el PO | `contradictor` sobre el alcance, antes de planear |
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
