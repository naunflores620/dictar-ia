# HANDOFF — <ID> «título»

Escrito por quien implementó, al entregar.

> **Para los revisores:** esto es una **declaración**, no evidencia. Todo lo que dice acá está
> por verificarse. Ver `protocolo.md`, regla 1.

## Estado

Terminada / parcial / bloqueada — y qué falta, con su porqué.

## Archivos tocados

Si hay alguno que no estaba en el `PLAN.md`, va marcado y con su razón.

| Ruta | Qué se hizo | ¿Estaba en el PLAN? |
|---|---|---|

## Comandos para reproducir

Los comandos exactos, para que el revisor los corra él mismo:

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd app && flutter analyze && flutter test
```

**Resultado obtenido** (pegar la salida real, no un resumen). Si un comando no se pudo ejecutar,
decirlo así, con el error tal cual. Nunca escribir «pasan» sin haberlos corrido.

Con el bloqueo B-1 activo lo esperable es que ninguno se haya podido ejecutar. Decirlo es lo
correcto; inventar un resultado es la única falta grave de este documento.

## Criterios de aceptación

| AC de `docs/06` | Prueba que lo cubre | Estado |
|---|---|---|

## Invariantes del producto

Solo los que la tarea toca (`protocolo.md`, «Invariantes del producto»). Para cada uno, cómo se
respeta:

| Invariante | Cómo se respeta acá |
|---|---|

## Decisiones que se apartan del PLAN

Solo las desviaciones, con su razón. Si no hubo, decirlo.

## Lo que NO pude verificar

Sección obligatoria. Qué quedó sin comprobar y por qué: falta de herramienta, de dato, de
hardware, de acceso. Vacía solo si de verdad se verificó todo.

## Deuda que dejo

Lo que quedó fuera a propósito y debería volver al tablero.
