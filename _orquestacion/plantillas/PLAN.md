# PLAN — <ID> «título»

Escrito por el orquestador **antes** de delegar. Sin este documento no se implementa.

> Las rutas relativas de esta plantilla están escritas para su destino,
> `_orquestacion/trabajo/<ID>/`. Desde acá no resuelven.

HU de origen: [`docs/06-historias-de-usuario.md#hu-xx`](../../../docs/06-historias-de-usuario.md)

## Análisis

Qué pide la HU en términos técnicos. Supuestos explícitos y qué queda deliberadamente fuera.

No se copian acá los criterios de aceptación: se referencian. Si el plan y `docs/06` no
coinciden, manda `docs/06`.

## Archivos y paquetes afectados

Lista cerrada. El implementador no toca nada que no esté acá, y el orquestador la compara con
los planes de las otras tareas en curso para que no se solapen.

| Ruta | Cambio esperado |
|---|---|

## Dependencias

- HU o tareas que deben estar terminadas antes:
- Herramientas que hacen falta y hoy no están (ver bloqueos en [`tablero.md`](../../tablero.md)):

## Casos del dominio que hay que cubrir

Los que este proyecto olvida siempre. Marcar los que aplican y decir qué se hace con cada uno:

- [ ] **Audio que no viene a 16 kHz** — un WAV del móvil a 48 kHz, o un dispositivo que entrega
      44,1. Se remuestrea en el origen, una sola vez
- [ ] **Cola del remuestreador** — el último bloque incompleto no se pierde
- [ ] **Pista de sistema ausente** — una reunión presencial solo tiene micrófono
- [ ] **Pistas de distinta longitud** — el monitor de salida arranca unos ms tarde; recortar a
      la corta come el final de la clase
- [ ] **Sesión interrumpida** — cierre inesperado a mitad de grabación: el audio ya está en disco
- [ ] **Plataforma sin backend** — devuelve `NoSoportada`, no desaparece la función
- [ ] **Núcleo que no carga** — se dice, no se cae en datos de demostración en silencio
- [ ] **Clase de dos horas** — nada que funcione con 30 s y reviente con 7200
- [ ] **Base de datos de una versión anterior** — la migración repuebla lo que los triggers no
      alcanzan
- [ ] **Sin clave de API y sin conexión** — el flujo local tiene que seguir sirviendo

## Riesgos

| Riesgo | Mitigación |
|---|---|

## Estrategia

Orden de implementación en 3–6 pasos.

## Pruebas necesarias

Qué se prueba, con qué tipo de prueba, y qué criterio de aceptación cubre cada una. Para cada
una, la mutación que debería ponerla en rojo — es lo que va a ejecutar `verificador-pruebas`.

Los nombres van como frases en español que describen el fallo que previenen, al estilo de
`fn la_similitud_nunca_supera_el_uno()`.

| Prueba | Cubre | Mutación esperada que la pone en rojo |
|---|---|---|

## Agentes a lanzar

Según la tabla de [`agentes.md`](../../agentes.md).

---

## Respuesta al contradictor

Se completa **después** de que corre `contradictor`, antes de implementar.

| # | Objeción | Veredicto del orquestador |
|---|---|---|

Cada objeción se acepta (y el plan de arriba se corrige) o se rechaza por escrito con su razón.
Ninguna se ignora.
