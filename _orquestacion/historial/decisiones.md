# Bitácora de orquestación

Qué se delegó, qué encontró la revisión y qué se decidió. Es el registro operativo, no
documentación del producto: lo que cambia el producto va a [`docs/`](../../docs/).

Orden inverso: lo más reciente arriba.

---

## 2026-09-03 — el PO prohíbe que el orquestador implemente

- **Decisión del PO, y revierte lo que decía `protocolo.md`.** El documento permitía al
  orquestador implementar tareas chicas a cambio de perder el derecho a revisarlas. Queda
  derogado: **no implementa nunca, ni una línea**. Toda implementación va a un `implementador`.
- **La razón, y es más fuerte que la que tenía la regla original.** El orquestador escribe el
  `PLAN.md` y dicta el veredicto. Si además implementa, se convierte en el único participante al
  que nadie puede contradecir — justo lo contrario de para qué existe este mecanismo. Y el
  argumento «es una tarea chica» es precisamente el que erosiona esa frontera, así que no se
  admite ninguna excepción.
- **Lo que motivó la decisión:** el orquestador implementó tres cosas el mismo día. La corrección
  de `README.md:93`, la tarea T-6 entera (`Ventana.esEscritorio` y las condiciones de plataforma
  en `ajustes.dart` y `grabacion.dart`) y los dos arreglos de prueba de la tercera vuelta de
  HU-01. Cada una parecía trivial por separado; juntas son un patrón.
- **Deuda que deja:** esos tres cambios **no los ha revisado nadie**. Abierta T-15 para que pasen
  por `revisor-codigo` y `verificador-pruebas` como cualquier otro trabajo.
- **T-15 le dio la razón al PO el mismo día, con evidencia.** El `verificador-pruebas` revisó los
  dos arreglos de prueba que había hecho el orquestador:
  - **v3-1 correcto**, y demostrado más fuerte de lo que se pidió: de las 16 funciones booleanas
    de dos entradas, solo `||` coincide con las cuatro pruebas en las cuatro filas. Prueba de
    suficiencia, no ausencia de contraejemplos.
  - **v3-2 inútil.** Rescató del scratchpad la versión previa al arreglo, hizo `diff -u` real y
    encontró que el `assert_eq!(pistas_al_arrancar, vec![Track::Mic])` **ya existía antes**: es
    una igualdad exacta del vector, así que si pasa, `.contains(&Track::System)` solo puede dar
    `false`. Simuló cuatro variantes y el veredicto rojo/verde es idéntico con la derivación
    nueva y con el literal viejo. La prueba sigue siendo redundante al 100 % con las otras dos.
  - **Lo relevante no es el error, sino su forma:** el orquestador cometió exactamente el fallo
    que llevaba el día entero señalando en los implementadores —desplazar el problema en vez de
    resolverlo— y además escribió un comentario afirmando una protección que la prueba no da, que
    por la regla del propio protocolo es un hallazgo Importante. Nadie lo habría visto si T-15 no
    existiera. Es el argumento empírico de por qué el orquestador no debe implementar: no porque
    escriba peor, sino porque **nadie estaba mirando**.
- **Segundo corte de cuota** el mismo día, ahora hasta las 19:40. Cayeron los tres revisores de la
  tercera vuelta de HU-05. Esta vez la comprobación de si habían alcanzado a escribir se hizo
  bien —`grep "Tercera vuelta"` sobre los tres informes, cero en los tres—, aplicando la lección
  del primer corte: comparar contenido, no solo integridad. En el primero se dio por bueno que
  ningún informe se había escrito porque todos «terminaban en frases completas», y era falso.

## 2026-09-03 — primera vuelta: T-1 auditada, dos planes contradichos, dos implementaciones en marcha

- **T-1 cerrada (`qa`).** Las cuatro HU en 🟡 siguen en 🟡, y eso es el resultado correcto: sin
  toolchain no se puede subir ninguna a 🟢 aunque sus criterios individuales estén limpios. El
  agente reverificó B-1 y B-2 en Windows **y** en WSL `Ubuntu-26.04`, y confirmó que nadie
  instaló nada. Actualizó los cuatro estados en `docs/06` con evidencia archivo+línea por
  criterio.
- **T-10 cerrada, y es un error propio que el mecanismo cazó.** `README.md:93` decía que el
  `.exe` de Inno Setup «está escrito pero desactivado hasta que Windows pueda grabar». Es falso:
  `release.yml` lo compila y empaqueta en cada tag sin ninguna condición que lo desactive. La
  frase se escribió **antes** de restaurar la pata de Windows en el mismo día y nadie la volvió
  a mirar. Justo la clase de desalineación que HU-13 existe para perseguir, y la encontró un
  agente cuyo único trabajo era desconfiar del tablero.
- **T-7 abierta, hallazgo de `qa` fuera de los criterios.** El paso que verifica que el paquete
  lleva el núcleo dentro solo vive en `release.yml`, que dispara con `tags: v*`. Una regresión
  que reintroduzca el fallo histórico del proyecto no se vería en el PR que la introduce, solo
  al cortar un release. El criterio 3 de HU-10 está cumplido a la letra y el riesgo sigue ahí:
  es el mismo fallo silencioso, movido más tarde en el pipeline.
- **Trampa del directorio ajeno.** El `qa` empezó leyendo el `protocolo.md` y el `tablero.md` de
  `d:\lector_json`, que está entre los directorios de trabajo de la sesión y tiene archivos con
  los mismos nombres. Lo detectó y lo corrigió solo, pero lo reportó — bien reportado. Anotado
  en `LEEME.md`: los prompts nombran rutas absolutas desde `d:\dictar_ia`.

### Lo que devolvieron los contradictores

Los dos planes salieron en **revisión 2**. El mecanismo se pagó solo en esta vuelta.

- **HU-01, objeción de fondo:** el plan mandaba replicar el reloj de `pipewire_src`, que deriva
  el `timestamp_ms` del conteo de muestras emitidas. Eso es seguro **solo** porque las dos
  pistas viven en el mismo grafo de PipeWire con un reloj único; WASAPI son dos `IAudioClient`
  con osciladores independientes, y el loopback no entrega paquetes durante el silencio. El plan
  además se contradecía: proponía una prueba cuya mutación reprobaría el propio mecanismo que
  mandaba imitar. **Decisión del orquestador, corrigiendo también al contradictor:** no es
  «rellenar con silencio **o** usar el reloj monótono» como él planteaba, son **las dos cosas**.
  El reloj arregla el timestamp del frame; sin el relleno, el WAV en disco queda corto y todo lo
  posterior suena adelantado. La disyuntiva era falsa.
- **HU-01, objeción de verificabilidad:** las cuatro pruebas propuestas **no se habrían ejecutado
  nunca**. El módulo es Windows-only, el job de Linux corre `cargo test` pero no lo compila, y
  el de Windows solo hace `cargo check --all-targets`, que tipa-chequea sin ejecutar. Se
  extrae la lógica pura a `sincronia.rs` sin `#[cfg]` de plataforma, y se añade `cargo test` al
  job de Windows.
- **HU-01, decidido al revés de lo sugerido:** el contradictor observó que `pipewire_src` no
  llama a `Remuestreador::vaciar()` al cerrar, así que «hacer lo mismo» significaba aceptar la
  misma pérdida de ~21 ms por pista. WASAPI **sí** lo llamará: replicar un defecto conocido por
  simetría es la peor de las dos opciones. La inconsistencia va al tablero como T-8.
- **HU-05, objeción que rompía la lista de archivos:** el retorno de `guardar_clave` es un
  `PathBuf` porque siempre escribe en un archivo, y con el llavero no hay ruta que devolver. El
  plan lo prohibía tocar `core/api` sin decir cómo evitarlo. Resuelto con el tipo `Origen`, que
  **ya existía** en `secretos.rs` y ya distingue llavero de archivo. Se descarta por escrito la
  alternativa que el contradictor describía —fabricar un `PathBuf` con el texto «llavero del
  sistema»—: es la clase de mentira de tipos que este repositorio evita.
- **HU-05, suposición de nueve meses desmontada:** `libsecret-1-dev` está en el CI y en
  `INSTALL.md` **desde el commit inicial**, antes de que existiera `secretos.rs`. El
  contradictor lo fechó con `git log` y comprobó en crates.io que el backend Linux por defecto
  de `keyring` 4.2.0 es `zbus-secret-service-keyring-store`, cliente D-Bus en Rust puro que **no
  enlaza `libsecret`**. Era una suposición de apertura de proyecto que nadie verificó nunca.
- **HU-05, prueba que no protegía:** las dos pruebas de AC 4 usaban un doble de `KeyResolver`,
  que no ejecuta ni una línea del resolutor real. **Corrijo también su propuesta:** sugería
  apoyarse en que el CI de Linux no tiene sesión gráfica, pero eso hace la prueba frágil en la
  máquina de un desarrollador con GNOME, donde sí hay llavero. Se replantean para valer en los
  dos entornos.
- **Regla suspendida:** rama por tarea. El árbol tiene 24 archivos sin commitear y crear ramas
  los arrastraría. Vuelve en cuanto el PO autorice el primer commit. Arrastra una consecuencia
  que el proyecto del que esto se derivó ya documentó: `verificador-pruebas` no puede usar
  `isolation: "worktree"`, porque un worktree se crea desde un commit y no llevaría nada que
  mutar.

## 2026-09-03 — se crea el mecanismo

- Se crea `_orquestacion/`, adaptada del protocolo de `lector_json` (`d:\lector_json\_orquestacion`),
  que a su vez viene del proyecto de la UES.
- **El eje de dominio cambia.** Allá el riesgo era el mapeo contra la norma del Ministerio de
  Hacienda, y había un `auditor-fiscal`. Acá los dos riesgos que este proyecto **ya ha
  materializado** son el contrato multiplataforma y el fallo silencioso:
  - `core/api` referenciaba `dictar_audio::reproductor`, que estaba tras
    `#[cfg(target_os = "linux")]`. El núcleo llevaba tiempo sin compilar en Windows y el CI daba
    verde porque solo corría en `ubuntu-24.04`.
  - `core/screen-capture` depende de `xcap`, que no tiene implementación para Android: habría
    roto el cruce en cuanto se intentara.
  - El `.exe` y el APK se construían **sin la librería nativa dentro**, y la aplicación
    arrancaba con datos de demostración en vez de decir que no encontró el núcleo.
  De ahí `auditor-plataforma` en lugar de `auditor-fiscal`, y los seis invariantes del producto
  en `protocolo.md`.
- **Se añade un agente que el original no tiene: `qa`.** Audita el tablero, no una tarea. Nace
  de un problema concreto de este proyecto: hay bastante código escrito y nunca compilado, y sin
  alguien que lo persiga el 🟡 se convierte en 🟢 por optimismo. Es el único autorizado a
  cambiar el estado de una HU.
- **Se afloja el límite WIP de una tarea**, y se sustituye por uno operativo: varias tareas en
  paralelo solo si sus `PLAN.md` declaran archivos disjuntos. No es preferencia: en la sesión
  del mismo día, tres agentes trabajaron a la vez sobre `core/audio-capture` y hubo que
  repartirles los archivos a mano para que no se pisaran.
- **Se añade la regla de contradicción 8**, que no está en el original: el analizador de Dart
  está roto por B-2 y marca como indefinidos hasta `Size` y `debugPrint`. Reportarlo como
  defecto es ruido, y ya despistó una vez.
- **Bloqueos B-1 a B-4 anotados.** No hay `cargo`, ni NDK, y Flutter es más viejo que el
  `pubspec`. Mismo caso que allá con Go. El protocolo lo absorbe con `NO VERIFICABLE` en lugar
  de dejar que alguien apruebe a ciegas.
- Se escribe [`docs/06-historias-de-usuario.md`](../../docs/06-historias-de-usuario.md) con las
  13 historias y su tablero de estado. Cuatro nacen ya en 🟡 —implementadas y sin compilar— en
  vez de en 🟢, que es lo que habría dicho un informe complaciente.
- Pendiente de decisión del PO: si `_orquestacion/` se versiona en el repositorio del producto o
  se ignora. Por ahora queda versionada, sin commit.
