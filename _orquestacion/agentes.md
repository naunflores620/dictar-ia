# Catálogo de subagentes

Las definiciones ejecutables están en [`.claude/agents/`](../.claude/agents/). Este documento
explica **cuándo** se lanza cada una y **cómo**. Los seis corren con **Sonnet** (`model: sonnet`
en su frontmatter); no hace falta pasar `model` en la llamada.

## Los seis

| Agente | Ataca | Escribe | Se lanza |
|---|---|---|---|
| `implementador` | El problema | Código + `HANDOFF.md` | Cuando el PLAN ya sobrevivió al contradictor |
| `contradictor` | El `PLAN.md` | Reporte al orquestador | **Siempre**, antes de implementar |
| `revisor-codigo` | El código entregado | `REVIEW-codigo.md` | Después de cada implementación |
| `verificador-pruebas` | Las pruebas, por mutación | `REVIEW-pruebas.md` | Siempre que haya pruebas nuevas o tocadas |
| `auditor-plataforma` | El contrato multiplataforma y el empaquetado | `REVIEW-plataforma.md` | Cuando se toca `#[cfg]`, CMake, Gradle, workflows o `Cargo.toml` de plataforma |
| `qa` | El tablero completo | Actualiza `docs/06` y reporta | Al cerrar una tanda, o cuando hay trabajo en 🟡 |

Los revisores se lanzan **en paralelo, en un solo mensaje**, y en segundo plano. Son
independientes entre sí a propósito: si uno viera el reporte del otro, convergerían.

## Recetas de lanzamiento

### Contradictor — antes de implementar

```
Agent(
  subagent_type: "contradictor",
  description: "Atacar PLAN de HU-04",
  prompt: "Leé _orquestacion/trabajo/HU-04-busqueda/PLAN.md y atacalo. La HU está en
           docs/06-historias-de-usuario.md. Reportá al orquestador; no escribas archivos."
)
```

### Implementador

```
Agent(
  subagent_type: "implementador",
  description: "Implementar HU-04",
  prompt: "Implementá lo que dice _orquestacion/trabajo/HU-04-busqueda/PLAN.md, incluidas las
           correcciones de «Respuesta al contradictor». Tocá SOLO los archivos que el plan
           declara. Al terminar escribí HANDOFF.md en esa misma carpeta. No hagas commit."
)
```

### Los revisores, en paralelo

Un solo mensaje con las llamadas que apliquen. `verificador-pruebas` va **con
`isolation: "worktree"`**: necesita mutar el código para comprobar que las pruebas se ponen en
rojo, y el worktree garantiza que esas mutaciones no toquen el árbol de trabajo real.

```
Agent(subagent_type: "revisor-codigo",      description: "Revisar código HU-04",   prompt: "...")
Agent(subagent_type: "verificador-pruebas", description: "Mutar pruebas HU-04",    prompt: "...",
      isolation: "worktree")
Agent(subagent_type: "auditor-plataforma",  description: "Auditar plataforma HU-04", prompt: "...")
```

El prompt de cada uno debe incluir, como mínimo:

- La carpeta de la tarea (`_orquestacion/trabajo/<ID>/`) y el `PLAN.md`.
- Cómo ver el cambio: `git diff` o los archivos concretos.
- El recordatorio de que el `HANDOFF.md` es declaración a verificar, no evidencia.
- Que en esta máquina no hay `cargo` (bloqueo B-1) y qué se espera que haga con eso.

## Qué hace bien y qué hace mal cada uno

### `contradictor`

Le sirve al orquestador para no enamorarse de su propio plan. Busca: el paso que asume algo no
verificado, la dependencia invisible, la parte de la HU que el plan dejó afuera en silencio, y
el caso del dominio que el plan no nombra (audio a 48 kHz, pista de sistema ausente, sesión
cortada a la mitad, plataforma sin backend). Es barato y es el que más devuelve — más todavía
acá, donde no se puede compilar y un error de plan no lo caza ningún compilador.

No sirve para revisar código: no lo lee en profundidad.

### `revisor-codigo`

Lee el diff con ojos hostiles. Su valor está en el error que se traga, el invariante que se
rompe en el caso límite y el comentario que dice una cosa mientras el código hace otra —
importante acá, donde los comentarios llevan el porqué de cada decisión.

No sustituye a `verificador-pruebas`: leer una prueba y creerle es exactamente el error que este
protocolo intenta evitar.

### `verificador-pruebas`

El único que puede afirmar que una prueba sirve. Su criterio es literal:

> si el código estuviera mal, ¿esta prueba fallaría?

Su método es responder esa pregunta ejecutándola: rompe el código a propósito, corre la prueba,
confirma el rojo, revierte, confirma el verde. Una prueba que sigue en verde con el código roto
es un hallazgo **Importante** como mínimo.

**Con el bloqueo B-1 activo no puede hacer su trabajo.** Mientras no haya `cargo`, su veredicto
es `NO VERIFICABLE` y lo que aporta es la mutación *propuesta*: qué línea habría que romper y
qué prueba debería ponerse en rojo. Eso deja el trabajo listo para el día que haya toolchain, y
es honesto sobre lo que no se comprobó.

### `auditor-plataforma`

El que contradice el dominio de este proyecto. Cada afirmación suya se sostiene con archivo y
línea. Comprueba, como mínimo:

- Que cada `#[cfg]` tenga su rama contraria con la **misma firma exacta**, y que quien llama no
  necesite `#[cfg]` propio.
- Que las dependencias específicas de una plataforma estén bajo `[target.'cfg(...)'.dependencies]`
  y no en `[dependencies]`.
- Que lo que el CMake de Linux hace, el de Windows lo haga también — y al revés.
- Que el paquete lleve el núcleo dentro, y que el CI falle si no.
- Que un `Cargo.lock` con dependencias de tres plataformas no signifique que las tres compilan:
  el lock las lista todas sin condicionar.

Es el rol que habría encontrado los dos fallos que este proyecto ya tuvo: `core/api`
referenciando un módulo que solo existía en Linux, y `xcap` sin implementación para Android.

No opina de estilo ni de arquitectura.

### `qa`

El único que mira el conjunto en vez de una tarea. Contrasta el estado declarado de cada HU en
[`docs/06`](../docs/06-historias-de-usuario.md) contra lo que hay de verdad en el repositorio, y
actualiza el tablero de esa misma página. Existe por un problema concreto: hay bastante código
escrito y nunca compilado, y sin alguien que lo persiga, el 🟡 se convierte en 🟢 por
optimismo.

Es el único agente autorizado a cambiar el estado de una HU.

## Cuándo NO lanzar agentes

Lanzar cinco agentes para cambiar una línea de documentación es teatro, y el teatro desprestigia
el mecanismo. Criterio:

| Tipo de cambio | Qué se lanza |
|---|---|
| Redacción, README, comentario suelto | Nada. Lo hace el orquestador |
| Refactor sin cambio de comportamiento | `revisor-codigo` |
| Corrección de error con su prueba de regresión | `revisor-codigo` + `verificador-pruebas` |
| Backend de plataforma, empaquetado, CMake, Gradle, workflow | Los tres, y `contradictor` antes |
| Pantalla nueva de Flutter | `contradictor` + `revisor-codigo` |
| Cambio en el pipeline de audio | Los tres, y `contradictor` antes. Sin excepción |

## Cómo el orquestador arma el `REVIEW.md`

Cada revisor escribe su propio archivo en la carpeta de la tarea. El orquestador **no los resume
ni los suaviza**: los referencia y consolida el veredicto en `REVIEW.md`, dejando los
desacuerdos visibles. Si dos revisores se contradicen entre sí, eso es información valiosa y va
al `REVIEW.md` tal cual, no se promedia.
