# REVIEW — <ID> «título»

Consolidado por el orquestador a partir de los reportes de los revisores. **Ninguno de ellos es
quien implementó.**

Reportes de origen, en esta misma carpeta:

- `REVIEW-codigo.md` — `revisor-codigo`
- `REVIEW-pruebas.md` — `verificador-pruebas`
- `REVIEW-plataforma.md` — `auditor-plataforma` (si aplicaba)

El orquestador **no los resume ni los suaviza**. Los referencia y decide.

## 1. Hallazgos consolidados

| # | Hallazgo | Origen | Severidad | Estado |
|---|---|---|---|---|

Severidades en `protocolo.md`. «Estado» es: resuelto / aceptado como deuda / rechazado con razón
/ escalado al PO.

## 2. Desacuerdos

Dónde no coinciden dos revisores, o un revisor y quien implementó. Ambas posturas, sin promediar,
y la decisión del orquestador con su razón.

Si no hubo desacuerdos, decirlo — y sospechar un poco.

## 3. Qué quedó sin verificar

Lo que ningún revisor pudo comprobar y por qué. Es lo que determina si el cierre es pleno o
condicionado. Con B-1 activo, acá va todo lo que dependa de compilar.

## 4. Checklist de cierre

De `protocolo.md`:

- [ ] `cargo fmt --all -- --check` en verde
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` en verde
- [ ] `cargo test --workspace` en verde
- [ ] `flutter analyze` y `flutter test` en verde
- [ ] Cada criterio de aceptación de `docs/06` tiene su prueba, nombrada
- [ ] Cada prueba nueva se puso en rojo al mutar el código que protege
- [ ] Los nombres de prueba describen el fallo que previenen, en español
- [ ] Si toca audio: se respetan los invariantes 1, 2 y 3
- [ ] Si toca plataforma o empaquetado: `auditor-plataforma` lo cotejó, con archivo y línea
- [ ] Documentación afectada actualizada en el mismo cambio

## Veredicto

**TERMINADA** / **APROBADA, cierre condicionado** (a qué) / **vuelve a implementación** (con qué).

Orquestador · fecha.
