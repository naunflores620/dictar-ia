# _orquestacion — mecanismo de trabajo

Esta carpeta **no es parte del producto**. No se compila, no se empaqueta, no se distribuye.
Es el mecanismo con el que esta sesión de Claude Code reparte, ejecuta y —sobre todo—
**contradice** el trabajo sobre `dictar_ia`.

Se deriva del protocolo de `lector_json`, que a su vez viene del proyecto de la UES. La regla
que le da todo el valor no cambia:

> Quien implementa nunca es quien aprueba, y quien revisa tiene que reproducir, no creer.

## Contenido

| Archivo | Qué es |
|---|---|
| `protocolo.md` | Roles, flujo de una tarea, reglas de contradicción y checklist de cierre |
| `agentes.md` | Catálogo de subagentes, cuándo se lanza cada uno y con qué recetas |
| `tablero.md` | Estado real: qué está en curso, qué está en revisión, qué está bloqueado |
| `historial/decisiones.md` | Bitácora: qué se delegó, qué encontró la revisión, qué se decidió |
| `plantillas/` | `PLAN.md`, `HANDOFF.md`, `REVIEW.md` |
| `trabajo/` | Una carpeta por tarea **en curso**. No se pre-crean las 13 HU del backlog |

Las definiciones ejecutables de los subagentes viven en [`.claude/agents/`](../.claude/agents/),
no acá: son configuración de la herramienta, no documentación.

## Fuentes de verdad — esta carpeta las referencia, jamás las copia

| Qué | Dónde vive |
|---|---|
| Historias de usuario y criterios de aceptación | [`docs/06-historias-de-usuario.md`](../docs/06-historias-de-usuario.md) |
| Arquitectura, pipeline y modelo de datos | [`docs/01-arquitectura.md`](../docs/01-arquitectura.md) |
| Tecnologías elegidas y por qué | [`docs/02-stack-tecnologico.md`](../docs/02-stack-tecnologico.md) |
| Capa de proveedores de IA y plantillas de notas | [`docs/03-proveedores-ia.md`](../docs/03-proveedores-ia.md) |
| Fases, orden y riesgos | [`docs/04-roadmap.md`](../docs/04-roadmap.md) |
| Empaquetado de `.exe`, `.deb` y APK | [`docs/05-empaquetado.md`](../docs/05-empaquetado.md) |
| Compuertas ejecutables | [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) |

Dos fuentes de verdad son cero fuentes de verdad. Si algo de acá contradice a `docs/`,
manda `docs/` y esta carpeta está mal.

> **Trampa real, y le pasó al primer agente que corrió.** La sesión tiene
> `d:\lector_json\_orquestacion` entre sus directorios de trabajo: es el proyecto **del que
> este mecanismo se derivó**, en Go y sobre facturación electrónica, y tiene archivos con los
> mismos nombres que estos. Un `protocolo.md` o un `tablero.md` sin ruta absoluta puede
> resolver allá. Todo prompt de agente debe nombrar las rutas empezando por `d:\dictar_ia`.

## En qué se aparta del protocolo del que viene

- **El eje de dominio es otro.** Allá el riesgo era el mapeo contra la norma del Ministerio de
  Hacienda, y existía un `auditor-fiscal`. Acá los dos riesgos que este proyecto ya ha
  materializado son el **contrato multiplataforma** (código que solo compila en Linux, paquetes
  que salen sin el núcleo dentro) y **perder una grabación**. De ahí `auditor-plataforma`, y
  la lista de trampas de audio en la plantilla de `PLAN.md`.

- **Se admite trabajo en paralelo, con una condición.** Allá el límite es una tarea en curso.
  Acá pueden correr varias a la vez **solo si sus conjuntos de archivos no se solapan**, y el
  `PLAN.md` de cada una declara qué archivos toca. Es una regla operativa, no una preferencia:
  dos agentes editando el mismo archivo se pisan, y ya pasó una vez con `core/audio-capture`.

- **Existe un agente `qa`** que no está en el original. Audita el **tablero**, no una tarea:
  contrasta el estado declarado de cada HU contra el repositorio. Nace de un problema real de
  este proyecto —hay mucho código escrito y nunca compilado— que el original no tenía.

## Qué NO se afloja

- Sin `PLAN.md` no se implementa.
- El revisor no es el implementador. Nunca.
- Un veredicto sin reproducción no es un veredicto: es una opinión.
- Nada se cierra con una prueba que no fallaría si el código estuviera mal.
- Nada sale hacia afuera —commit, push, PR— sin que lo pida el PO en ese momento.
