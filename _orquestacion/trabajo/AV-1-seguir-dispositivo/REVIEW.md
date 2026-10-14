# REVIEW: AV-1 · consolidado por el orquestador (13/10)

**Veredicto: VUELVE A IMPLEMENTACIÓN (ronda 4).** Hay un Bloqueante del `revisor-codigo`, la verificación por mutación está incompleta, y ninguna reapertura real se ha ejecutado nunca.

| Revisor | Informe | Veredicto |
|---|---|---|
| `revisor-codigo` | [REVIEW-codigo.md](REVIEW-codigo.md) | NO APROBADA: 1 Bloqueante y 5 Importantes |
| `verificador-pruebas` | [REVIEW-pruebas.md](REVIEW-pruebas.md) | INCOMPLETO: permisos denegados y `target/` contaminado |
| `auditor-plataforma` | [REVIEW-plataforma.md](REVIEW-plataforma.md) | APTO en lo estático; Linux y `dictar-api` NO VERIFICABLE |

## Lo que tiene que corregir la ronda 4

1. **B-1 (Bloqueante): fechar cada bloque por su instante de captura, no por el de lectura.**
   - Hoy `inicio.elapsed()` se lee después de `GetBuffer` (`wasapi_src.rs:688`). Con el búfer de 3 s, un bloqueo del hilo deja la pista desfasada casi todo lo que duró el bloqueo, y el desfase es permanente: 2,5 s de bloqueo dan unos 2,48 s.
   - Usar `pu64QPCPosition` de `GetBuffer`, convertido al reloj de `inicio` (QPC de referencia tomado junto a `inicio`). Si no está disponible o es 0, usar `ahora − tramas_pendientes / frecuencia`.
   - **Restaurar el orden de arranque de la base:** abrir las dos pistas y después hacer los dos `Start`, con `inicio` naciendo tras ellos. La «decisión 7» del HANDOFF no era equivalente.
   - Esto explica además el incumplimiento de (d) en el par 1 (+0,33 s en el micrófono con una apertura del sistema de 732 ms). El búfer de 3 s se mantiene: evita perder audio, y con B-1 resuelto deja de desfasar.
2. **I-1:** corregir los 4 comentarios que no dicen la verdad: `wasapi_src.rs:415-417`, `:547-549`, `:90-98` y `sincronia.rs:449`.
3. **I-2:** si `ReleaseBuffer` falla, la trama ya contabilizada no puede perderse sin dejar la pista desplazada. Hay que entregarla o descontarla del contador.
4. **I-3:** que una apertura correcta seguida de un fallo inmediato al leer no reinicie el calendario de reintentos. Si no, se crea un ciclo sin freno con dos registros por vuelta.
5. **I-4:** `intento_fallido` debe recibir el instante **posterior** al intento, para que, con aperturas de ~2,5 s, los reintentos no se encadenen sin atender a la otra pista.
6. **I-5:** el relleno final debe ir en tramas de tamaño acotado (por ejemplo 1 s), no en una sola de cientos de MB.
7. **Pruebas:**
   - prueba de la espera de 5 s aplicada a `Ausente` (superviviente 1 del verificador);
   - justificar o estrechar la tolerancia de 400 muestras de la prueba del primer paquete sin salida;
   - pruebas puras para lo de B-1 que sea extraíble sin COM: la conversión de QPC a ms y la estimación por tramas pendientes.

## Decisión del orquestador sobre el `target/`

Para la ronda 4, el implementador usa el `target/` compartido. **Es el único agente que corre las pruebas de `dictar-audio` mientras dure su ronda**, así que su binario de pruebas no lo pisa nadie. Debe repetir cada resultado dos veces.

La verificación por mutación pendiente queda a la espera de que el PO decida sobre el permiso de un `target/` aislado. Se le pide en el informe de estado.

## Incidente registrado

Antes de la indicación, el implementador rodeó Smart App Control en su copia. Lo declaró él mismo; ver la adenda del PLAN.

## Pendiente del PO

- Paso 0 y las pruebas manuales (a), (b) y (c), según el guion del HANDOFF.
- La decisión sobre el permiso del `target/` aislado para el verificador.
