# Protocolo de trabajo — dictar_ia

Documento interno (ver `LEEME.md`). Define quién hace qué, en qué orden, y con qué derecho
alguien dice que algo está terminado.

## Por qué existe

Este software graba clases que ocurren una sola vez. El propio núcleo lo dice en
`core/audio-capture/src/wav.rs`: *«perder una clase por un fallo del programa es lo único que
no tiene arreglo después»*. Y tiene un segundo modo de fallo, más insidioso, que ya se
materializó dos veces: **fallar en silencio**. El `.exe` se construía sin la librería nativa,
`main.dart` no la encontraba, caía en su `catch` y la aplicación arrancaba con datos de
demostración. Parecía funcionar durante meses.

Trabajando con agentes, el modo de fallo más peligroso no es que escriban código malo: es que
**se aprueben entre ellos**. Un agente complaciente produce un `HANDOFF.md` que dice «todo
verde» y un `REVIEW.md` que dice «aprobada», y nadie miró nada. Sobre un proyecto que ya falla
en silencio por su cuenta, eso es letal.

Este protocolo está diseñado contra las dos cosas.

## Roles

### Product Owner — el usuario (Naun)

- Define alcance, prioridades y qué historia se trabaja.
- Resuelve lo que el orquestador escala: decisiones de producto, alcance, bloqueos externos.
- Autoriza commits, push y cualquier acción hacia afuera.

### Orquestador — esta sesión

- Elige la tarea siguiendo [`docs/06`](../docs/06-historias-de-usuario.md) y la prioridad del PO.
- Escribe el `PLAN.md` **antes** de delegar. Sin plan no hay implementación.
- Lanza los subagentes y arma con sus salidas el `REVIEW.md`.
- Es el único que cierra una tarea, y solo con la evidencia en la mano.
- Mantiene `tablero.md` y `historial/decisiones.md`.
- **No implementa. Nunca**, ni una línea, por pequeña que parezca la tarea. Toda implementación
  va a un `implementador`, incluida la corrección de un `assert` o de un comentario.

  > **Decisión del PO, 2026-09-03.** Este documento decía antes lo contrario: que el orquestador
  > podía implementar tareas chicas a cambio de perder el derecho a revisarlas. El PO lo revirtió
  > tras ver que el orquestador había implementado tres cosas en un mismo día. La razón es la que
  > sostiene todo lo demás: el orquestador **escribe el `PLAN.md` y dicta el veredicto**, así que
  > si además implementa, no queda nadie con distancia para contradecirlo — se convierte en el
  > único que no puede ser corregido. «Es una tarea chica» es exactamente el argumento con el que
  > esa frontera se erosiona, y por eso no se admite ninguna excepción.

### Subagentes

Seis roles. Catálogo y recetas de lanzamiento en [`agentes.md`](agentes.md). Todos con
**Sonnet**.

| Agente | Qué ataca |
|---|---|
| `implementador` | Hace el trabajo del `PLAN.md` |
| `contradictor` | Ataca el **plan**, antes de que se escriba código |
| `revisor-codigo` | Ataca el **código** entregado |
| `verificador-pruebas` | Ataca las **pruebas**: las muta para ver si de verdad protegen |
| `auditor-plataforma` | Ataca el **contrato multiplataforma** y el empaquetado |
| `qa` | Ataca el **tablero**: contrasta el estado declarado de cada HU contra el repositorio |

Contrato común, para todos:

```
Puede:
- leer todo el repositorio y toda la documentación
- ejecutar comandos de verificación
- discrepar del PLAN, del orquestador y del usuario, con evidencia

No puede:
- cambiar el alcance de la tarea
- aprobar su propio trabajo
- afirmar que verificó algo que no ejecutó
- hacer commit, push, ni nada hacia afuera
- editar archivos fuera de los que su PLAN declara
```

## Flujo de una tarea

```
PO prioriza
   ↓
ORQUESTADOR abre trabajo/<ID>/ y escribe PLAN.md
   ↓
CONTRADICTOR ataca el PLAN            ← se lanza SIEMPRE: es barato,
   ↓                                    y es donde un error cuesta más caro
PLAN corregido, o defendido por escrito
   ↓
IMPLEMENTADOR (siempre un subagente) → HANDOFF.md
   ↓
REVISIÓN, en paralelo:
   revisor-codigo · verificador-pruebas · auditor-plataforma (si toca plataforma o empaquetado)
   ↓
ORQUESTADOR consolida REVIEW.md y decide
   ↓
TERMINADA  /  vuelve a implementación  /  APROBADA con cierre condicionado
```

Nunca: `petición → alguien empieza a programar`.

**Paralelismo entre tareas.** Pueden correr varias a la vez si sus `PLAN.md` declaran conjuntos
de archivos disjuntos. El orquestador lo comprueba antes de lanzar; si dos planes se solapan,
una espera. No es burocracia: dos agentes editando `core/audio-capture/src/lib.rs` a la vez se
pisan, y ya ocurrió.

## Las reglas de contradicción

Esto es lo que separa una revisión real de un sello de goma. Son obligatorias para los agentes
revisores y están escritas dentro de sus definiciones en `.claude/agents/`.

**1. El `HANDOFF.md` es la declaración del acusado, no evidencia.**
El revisor recibe el plan y el código. Lo que el implementador dice que hizo se verifica; no se
cita como prueba de nada.

**2. Reproducir o callar.**
«Las pruebas pasan» solo se puede escribir después de haberlas corrido. Si no se pudieron
correr, el veredicto es `NO VERIFICABLE` y se explica por qué. Un `NO VERIFICABLE` honesto vale
más que un `APROBADA` inventado, y no cuenta como falla del implementador.

**3. Prohibido aprobar por ausencia de evidencia.**
«No encontré problemas» es un veredicto válido **solo** acompañado de qué se buscó y cómo. La
sección «Qué verifiqué y no marqué» es obligatoria y es la parte que más pesa.

**4. Nada de cuotas de hallazgos.**
No se le pide a nadie «encontrá tres problemas»: eso fabrica hallazgos falsos, que son peores
que ninguno. Se le pide que demuestre el trabajo de búsqueda.

**5. Sección obligatoria: «Premisas que cuestiono».**
Todo revisor tiene que atacar al menos una premisa del plan o del código y decir a qué
conclusión llegó. Puede concluir que la premisa se sostiene — pero tiene que haberla atacado.

**6. Sin preámbulos ni elogios.**
Nada de «excelente trabajo». Hallazgos, evidencia y veredicto. El tono del proyecto es sobrio;
las revisiones también.

**7. El desacuerdo se registra, no se lima.**
Si revisor e implementador no coinciden, ambas posturas quedan escritas en el `REVIEW.md` y
decide el orquestador. Si orquestador y PO no coinciden, decide el PO y queda en la bitácora.

**8. El analizador roto no es un hallazgo.**
En esta máquina `flutter pub get` falla por versión del SDK, y entonces el analizador marca como
indefinidos hasta `Size` y `debugPrint`. Reportar eso como defecto del código es ruido. Regla
específica de este proyecto, y existe porque ya despistó una vez.

## La regla de la réplica

Salió de la primera vuelta del mecanismo, y no como una idea: como el mismo fallo cometido por
**dos implementadores independientes, el mismo día, en dos crates distintos**.

> Una prueba tiene que ejercitar la función de producción. Si construye su propia versión de lo
> que dice probar, no prueba nada.

Los dos casos reales:

- **HU-01.** El test que blindaba el invariante 4 llamaba a un `enum Plataforma` escrito a mano
  en el propio módulo, desconectado de los `#[cfg(target_os = ...)]` reales de `lib.rs`. Habría
  seguido en verde con el `cfg` roto — incluido el caso exacto que este proyecto ya sufrió,
  confundir Android con «no-Linux». El enum no se usaba en ningún otro sitio: existía solo para
  que el test tuviera algo que llamar.
- **HU-05.** El test que comprobaba el orden de la cadena de resolutores construía su propia
  `CadenaResolvers`, ya en el orden correcto, en vez de llamar a `resolver_por_defecto()`, que es
  la función que decide ese orden. Reordenar la cadena real no lo habría puesto en rojo.

Es un fallo especialmente traicionero porque la prueba **parece** cubrir el criterio, y el
criterio queda marcado como cumplido. Cuesta más caro que no tener prueba: da falsa seguridad
justo sobre lo que más importa.

Cómo se detecta, y es la pregunta que hace `verificador-pruebas`: *¿qué línea del código de
producción ejecuta esta prueba?* Si la respuesta es «ninguna, construye la suya», es un hallazgo
**Importante** como mínimo.

## Severidad de un hallazgo

| Nivel | Significa | Efecto |
|---|---|---|
| **Bloqueante** | Pierde audio, corrompe una sesión, rompe la compilación en alguna plataforma, produce un paquete que arranca con datos de demostración, **expone una credencial**, o **le dice al usuario algo falso sobre el estado de sus datos o sus claves** | No se cierra |
| **Importante** | Deuda real que va a doler: prueba que no protege, error que se traga, invariante sin cubrir, firma que cambia según la plataforma | Se corrige, o se escala al PO con su justificación |
| **Menor** | Legibilidad, nombres, ruido | A criterio del orquestador |
| **Nota** | Observación sin acción | Solo se registra |

Un comentario que describe mal lo que hace el código es **Importante**, no Menor. En este
repositorio los comentarios llevan el porqué de cada decisión —es su rasgo más característico—
y uno equivocado sobrevive al código que describía.

> **Las dos causales del final de la fila «Bloqueante» se añadieron el 2026-09-03**, después de
> que la tabla original se quedara corta dos veces en el mismo día, las dos en HU-05: una
> funcionalidad que decía «Guardada en el llavero» mientras la aplicación seguía usando la clave
> vieja del `.env`, y una escritura «atómica» que dejaba el archivo de claves en claro con
> permisos abiertos durante una ventana, en cada guardado. Ninguna de las dos encajaba en la
> letra de la tabla y las dos eran obviamente bloqueantes. Regla que se deduce de ahí: **si un
> hallazgo obliga a discutir si la tabla lo cubre, la tabla está incompleta, no el hallazgo.**

## Invariantes del producto

No son estilo. Romper cualquiera de estas es **Bloqueante**, y todo revisor las comprueba:

1. **Se escribe a disco antes de procesar nada.** Si el programa muere en el minuto 90, el
   audio tiene que estar entero en el archivo.
2. **Dos pistas, nunca una mezcla.** Micrófono y sistema se graban por separado. De ahí sale la
   diarización sin modelo.
3. **Todo el pipeline a 16 kHz mono `f32`.** Cualquier entrada con otra frecuencia se remuestrea
   en el origen, una sola vez.
4. **Ninguna firma pública cambia según la plataforma.** Quien llama no escribe `#[cfg]`. Si una
   plataforma no soporta algo, devuelve `NoSoportada`; no desaparece la función.
5. **Un fallo no se traga.** Especialmente el de cargar el núcleo: caer en datos de
   demostración sin decirlo es la forma en que este proyecto ha fallado antes.
6. **El paquete lleva el núcleo dentro.** `.deb`, `.exe` y APK se verifican en CI.

## Checklist de cierre

Las compuertas, en el orden en que se corren:

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd app && flutter analyze && flutter test
```

Y además, para dar por terminada una tarea:

- [ ] Cada criterio de aceptación de la HU en [`docs/06`](../docs/06-historias-de-usuario.md) tiene su prueba, nombrada.
- [ ] Cada prueba nueva pasó por `verificador-pruebas`: se mutó el código que protege y la prueba se puso en rojo.
- [ ] Los tests se llaman como frases en español que describen el fallo que previenen.
- [ ] Si toca audio: se respetan los invariantes 1, 2 y 3.
- [ ] Si toca `#[cfg]`, `Cargo.toml` de plataforma, CMake o Gradle: `auditor-plataforma` lo cotejó, con archivo y línea.
- [ ] La documentación afectada quedó actualizada en el mismo cambio.
- [ ] `REVIEW.md` completo, con autores distintos del implementador.

Si una casilla no se puede marcar por una limitación del entorno y no por el trabajo, la tarea
queda **APROBADA, cierre condicionado**, con el bloqueo anotado en `tablero.md`.

## Reglas no negociables

- **Git.** Rama por tarea desde `main`. No se hace merge a `main` sin decisión del PO.
- **Nada hacia afuera sin autorización.** Ni commit, ni push, ni PR, ni tocar servicios
  externos, salvo que el PO lo pida en ese momento.
- **Español, tono sobrio.** Nombres, comentarios y nombres de prueba. Los comentarios explican
  el **porqué**, nunca el qué; el estándar es el que ya tiene `core/audio-capture`.
- **El puente no se edita a mano.** `core/api/src/frb_generated.rs` y `app/lib/src/rust/` los
  genera `flutter_rust_bridge_codegen`. Si hace falta una función nueva en el puente, se escribe
  en `puente.rs` y se regenera; tocar el generado rompe la trazabilidad y se pierde en la
  siguiente regeneración.
- **Los iconos no se dibujan a mano.** Se edita `packaging/icono.py` y se regenera.

## Estado del entorno — bloqueo activo

**No hay toolchain de Rust en esta máquina.** Verificado el 2026-09-03: no existen `cargo`,
`rustc` ni `rustfmt`, ni en el PATH de Windows ni dentro de la única distribución de WSL con
shell (`Ubuntu-26.04`). Tampoco hay NDK de Android. Y el Flutter instalado (Dart 3.11.4) es
más viejo que el `sdk: ^3.12.2` que exige `app/pubspec.yaml`, así que `flutter pub get` falla y
con él `flutter analyze` y `flutter test`.

Consecuencia directa: **ninguna compuerta se puede ejecutar**, y ninguna tarea que toque código
puede pasar de **APROBADA, cierre condicionado**.

Mientras dure, los revisores trabajan en modo estático y **están obligados a declararlo**: el
veredicto correcto es `NO VERIFICABLE` en todo lo que dependa de ejecución, nunca `APROBADA`.
Ver `tablero.md`.
