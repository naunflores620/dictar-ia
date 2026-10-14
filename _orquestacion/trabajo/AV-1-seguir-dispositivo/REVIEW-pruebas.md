# REVIEW-pruebas: AV-1 · **INCOMPLETO**

> Lo transcribe el orquestador desde el informe que devolvió el `verificador-pruebas`. El agente no tenía herramienta para escribir archivos, y el clasificador de permisos le denegó primero el `target/` aislado («Security Weaken») y después incluso un `git status`. No rodeó nada. **Todos los resultados son con el `target/` compartido**, que estaba contaminado (ver el tablero, incidente del 13/10).

## Veredicto

**INCOMPLETO.** Falta repetir la línea base, todas las mutaciones y el verde final con un `target/` sin contaminar. Eso necesita una decisión del PO sobre permisos.

## Fiabilidad de lo obtenido (valoración del orquestador)

- **Los rojos se consideran fiables.** La contaminación observada iba de los binarios mutantes de este verificador hacia otros agentes (los 8 fallos falsos del revisor). Un rojo implica que se ejecutó un binario mutante, y en esa ventana nadie más mutaba `dictar-audio`.
- **Los supervivientes no son fiables:** pudo ejecutarse un binario sin mutar de otra copia. Hay que repetirlos.

## Mutaciones en rojo (con el `target/` compartido)

Todas sobre `core/audio-capture/src/sincronia.rs`:

- **Sondeo:**
  - L304 (`<` → `<=`, y `< ZERO`);
  - L336 (`veces < 2` → 1 y → 3);
  - L312 (quitar `candidato = None`);
  - L321 (quitar la comparación con `en_uso`);
  - L327 (`*visto == id` → `true`);
  - L300 (todas las variantes del `matches!`);
  - las constantes L133 y L139.
- **Espera de 5 s:** L345 (`<=`, `< ZERO`, `< 4 s`), L342 (`es_viva()` → `false`), L144 (5 → 4 s), L429.
- **Transiciones D2:** L366 (idempotencia), L362, L365, L371, L405, L406, L417, L418, L427.
- **Calendario 2 s/5 s y frontera de 60 s:** L393 (`>`, rápido siempre, lento siempre, sin restar `desde`), L391, L473, las constantes L151, L152 y L153.
- **Aviso cada 30 s:** L441, L442, L375, L156, L438.
- **Relleno final:** L458, L459, L464, L465, L454 (añadir `Viva`/`NoPedida`, quitar `Reabriendo`).

## Supervivientes (hallazgos tentativos, a repetir)

1. **L342, `es_viva()` → `true`.** Ninguna prueba exige que una pista `Ausente` pueda reabrir dentro de los 5 s siguientes a su última apertura. La prueba de «Ausente vuelve» sondea a los 13 y 14 s con `ultima_apertura = 0`. **Falta una prueba de la espera de 5 s aplicada a `Ausente`.**
2. L430, quitar `ultimo_sondeo = ahora` en `al_abrir`: probablemente efecto menor.
3. L426, guarda de `al_abrir` → `true`: estado probablemente inalcanzable o equivalente.
4. L454, quitar `Ausente` del relleno final: salida vacía, probablemente error de compilación o del script. Reintentar.
5. `sin_dispositivo_desde()` y `es_viva()`: solo las usa `wasapi_src.rs`, sin pruebas.

## Sin mutar todavía (por el corte)

- `PistaCapturada::continuar` y `procesar_paquete`: contador a 0, cálculo del relleno, `reales.len()`, `&&`/`||`, `+=` y `vaciar`. **La prueba del primer paquete sin salida usa una tolerancia de 400 muestras, que puede dejar pasar mutaciones pequeñas.**
- `sin_abrir_al_arrancar`, `contador()` y las ramas de la prueba de las 16 combinaciones.
- L104 de `muestras_de_relleno` (preexistente).

## Código sin ninguna prueba (riesgo declarado)

**Todo `wasapi_src.rs`:**

- el bucle de lectura;
- `abrir_y_arrancar` y la medición de `apertura_ms`;
- el drenaje de D4 y `soltar_para_reabrir`;
- `id_de_endpoint_por_defecto`;
- el búfer de 3 s;
- y el **cableado** con `ControlPista`: que el contador se lea después de `vaciar`, que se llame a `relleno_final` al detener y que un mismo veredicto no provoque dos cierres.

Las pruebas de `ControlPista` solo ejercitan la máquina de estados.

## Estado de la copia `../dictar_ia-av1-mut`

Según el agente, la última mutación (M30d) quedó revertida por su script. No pudo comprobarlo con `git status`, así que no está confirmado.
