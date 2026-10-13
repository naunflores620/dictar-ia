# REVIEW-codigo — AV-1 «Que la grabación siga al dispositivo de audio activo»

Revisado: `refs/revision/av-1` (el árbol `D:\dev_v1\dictar_ia-av1` es idéntico en `core/`; `md5` de `sincronia.rs` coincide con la ref). Líneas de `wasapi_src.rs` y `sincronia.rs` = las de la ref.

## 1. Veredicto

**NO APROBADA. 1 hallazgo Bloqueante, 5 Importantes.** Lo que depende de ejecutar una reapertura real (Paso 0, (a), (b), (c), tiempos de reapertura) es **NO VERIFICABLE**: ninguna reapertura se ha ejecutado nunca, ni por el implementador ni por mí. El Bloqueante sale de leer el código y de reproducir su aritmética con la lógica real de producción (`PistaCapturada`), no de una ejecución con COM.

Reproducido por mí: `cargo test -p dictar-audio` en un target aislado → 80/80, cuatro ejecuciones seguidas; `cargo check --target x86_64-unknown-freebsd -p dictar-audio --all-targets` → verde; `cargo clippy -p dictar-audio --all-targets -- -D warnings` → un solo error, `lib.rs:304` (el preexistente), ningún aviso en los archivos tocados (ver §5 sobre el ejemplo).

## 2. Hallazgos

### BLOQUEANTE

**B-1. Las tramas se fechan al drenar, no al capturar; con el búfer de 3 s un bloqueo del hilo desplaza una pista entera hasta 2,5 s, y el arranque mete un pre-roll sin fechar. Es la causa de (d) par 1.**
`wasapi_src.rs:688` (`timestamp_ms = inicio.elapsed()` leído *después* de `GetBuffer`), `:98` (`DURACION_BUFFER_HNS` = 3 s), `:265-291` (cada pista se abre **y arranca** seguida; `inicio` nace después), `:550-573` (`reabrir` bloquea el único hilo durante `abrir_y_arrancar`).

Qué pasa: `muestras_de_relleno(ts, emitidas, paquete)` supone que `ts` es el instante en que sonó el paquete. Con el búfer de 200 ms eso fallaba como mucho 200 ms (lo demás se perdía y quedaba como silencio en el sitio correcto). Con 3 s el búfer conserva todo lo ocurrido durante el bloqueo; al volver, el primer paquete lleva `ts = ahora` y recibe como relleno todo el bloqueo (S − p de silencio), y los S segundos de audio retenidos se escriben detrás. Los siguientes paquetes de la ráfaga llevan el mismo `ts`, relleno 0 (`saturating_sub`) y se anexan. Resultado: el WAV queda **adelantado** respecto del reloj en ≈ S, y como el audio posterior se anexa a continuación, **el desfase es permanente** (el sistema se cura solo en el próximo silencio ≥ S; el micrófono, que nunca calla, no).

Reproducción (copia mía de `sincronia.rs` con un test añadido que usa `PistaCapturada::procesar_paquete` de producción: paquetes de 10 ms a 48 kHz, 1 s normal, bloqueo de S ms con el audio retenido y drenado con el mismo `ts`, 1 s normal):

```
stall=0 ms    desfase=4 ms
stall=200 ms  desfase=194 ms     <- el máximo posible con el búfer anterior
stall=1000 ms desfase=984 ms
stall=2500 ms desfase=2477 ms    <- la apertura de micrófono de 2472 ms medida en HANDOFF
```

Escenario concreto de AV-1: cambio del micrófono por defecto (c) o `DEVICE_INVALIDATED` del micrófono, con apertura de ~2,5 s (la medida del propio HANDOFF). Durante ella no se lee `system`. Al volver, `system.wav` queda ≈2,4 s adelantado respecto de `mic.wav` y del reloj; la diarización por pista pierde la alineación. Es la ronda 3 la que lo introduce: el comentario de `:90-98` lo vende como «solo da margen».

Mismo mecanismo en el arranque (decisión 7 del HANDOFF, «equivale en efecto»: no equivale). En la base se abrían las dos pistas y *luego* se arrancaban ambas; ahora el micrófono hace `Start()` y espera a que se abra el sistema (62–732 ms) **antes** de que nazca `inicio`. Esos ms quedan en el búfer del micrófono (antes capado a 200 ms, ahora 3 s) y se leen con `ts ≈ 0`: se anexan sin relleno.

**(d) par 1: ¿ruido o causa en el código? Causa en el código.** Evidencia (artefactos del implementador `r3_new*.txt`, `r3_base*.txt`, que releí; **no reproduje yo la ejecución con COM**):

| ejecución | apertura sistema | tramas mic en t=1 s (nuevo) | mic.wav nuevo − base |
|---|---|---|---|
| par 1 | 732 ms | **81** (lo normal es ~50) | **+0,33 s** |
| par 2 | 50 ms | 50 | +0,02 s |
| par 3 | 34 ms | 51 | +0,04 s |

El exceso de ~31 tramas en el primer segundo ≈ 0,3 s ≈ la diferencia de duración; solo aparece en el micrófono (que arranca antes que el sistema) y solo cuando la apertura del sistema es larga. El HANDOFF lo atribuye a la carga de la máquina «sin poder afirmar» nada; la carga es el disparador (aperturas lentas), la causa es el código. Además: los «huecos de 2–3,4 s entre tramas» que el HANDOFF no explica son del lado del consumidor (si hubieran sido bloqueos del bucle, con este mecanismo `system.wav` habría salido ~2–3 s más largo, y salió ±0,02 s).

Qué debería pasar: la trama se fecha con el instante de captura (`pu64QPCPosition` de `GetBuffer`, o `ahora − tramas_aún_pendientes/frecuencia`), y los dos `Start()` van después de abrir ambas pistas y antes de `inicio` (como en la base). Hace falta una prueba que alimente una ráfaga con búfer acumulado.

### IMPORTANTE

**I-1. Comentarios que mienten (4).**
- `wasapi_src.rs:415-417`: «10 ms es una fracción pequeña de la ventana de **200 ms** del búfer». El búfer es de 3 s desde la ronda 3.
- `wasapi_src.rs:547-549` (doc de `reabrir`): «abrir un cliente WASAPI puede tardar **decenas de milisegundos** y su búfer es de **200 ms**». Medido: hasta 2472 ms; búfer 3000 ms. Dos datos falsos en una frase.
- `wasapi_src.rs:90-98`: «Un búfer más grande no añade latencia … solo da margen». Cierto para la entrega de paquetes; falso para la línea de tiempo (B-1). Es el comentario que más daño hará, porque cierra la discusión.
- `sincronia.rs:449-450` (doc de `relleno_final`): «Una pista viva no lo necesita: su cola ya la emite `vaciar`». Falso para un loopback que termina en silencio (el propio HANDOFF lo lista como deuda). Un lector que confíe en él no buscará por qué `system.wav` sale corto.

**I-2. Un fallo de `ReleaseBuffer` descarta una trama ya contabilizada y desplaza la pista.** `wasapi_src.rs:702-710`. `procesar_paquete` ya incrementó `muestras_emitidas` (y puede llevar un relleno de segundos tras un silencio de loopback); si `ReleaseBuffer(..)?` falla (p. ej. `DEVICE_INVALIDATED` justo entre `GetBuffer` y `ReleaseBuffer`), la función retorna antes del `match resultado` y la trama no se envía. Antes la sesión moría; ahora la pista se reabre y sigue, con el contador por delante de lo escrito: todo lo posterior queda adelantado en (relleno + paquete) y no se corrige. Arreglo: enviar `resultado` antes de propagar el error de `ReleaseBuffer`.

**I-3. (Sospecha) Bucle de reapertura sin freno si el cliente abre bien y falla al leer.** `sincronia.rs:360-375` + `wasapi_src.rs:342,395-406`. `empezar_reapertura` reinicia `desde`, `intentos` y `ultimo_intento = None`, así que cada vuelta de «abre → falla al leer» reintenta de inmediato y el calendario 2 s / 5 s nunca se alcanza; el enfriamiento de 5 s solo vale para el camino D1. Resultado posible: abrir/cerrar el motor de audio sin pausa más un `error!` + `info!` por ciclo. No lo disparé (necesita un dispositivo que abra y devuelva `DEVICE_INVALIDATED` en la primera lectura). Prueba que lo mostraría: `empezar_reapertura` → `al_abrir` → `empezar_reapertura` a menos de 1 s, esperando que `toca_reintentar` sea falso.

**I-4. (Sospecha) `intento_fallido` se marca con el instante de *antes* del intento.** `wasapi_src.rs:570` toma `ahora` antes de `abrir_y_arrancar` y `:600` lo pasa a `intento_fallido`. Si el intento fallido tarda ≥ 2 s (las aperturas de micrófono medidas llegan a 2,5 s), el siguiente ya toca de inmediato: el hilo queda casi permanentemente dentro de `abrir`, la otra pista solo se drena al inicio de cada intento y `detener()` espera un intento entero. Con B-1 sin arreglar, además desplaza la otra pista en cada ciclo. Arreglo: `inicio.elapsed()` tras el intento.

**I-5. Relleno en una sola trama: asignación sin cota.** `sincronia.rs:451-465` (`vec![0.0; faltan]`) y `PistaCapturada::procesar_paquete`. Pista pedida que nunca abrió, clase de 2 h: 115 200 000 muestras = **~460 MB** en una sola `AudioFrame` en el hilo de captura al detener (y ~230 MB si el dispositivo aparece a la hora). Antes los huecos eran de segundos. `EscritorPistas::escribir` escribe en streaming, así que el pico es solo la trama, pero en el momento de detener. Debería trocearse (p. ej. en tramas de 1 s).

**I-6. Pruebas: el criterio protegido no es el que falla.** Las 80 pruebas pasan y cubren bien `ControlPista` (revisé la tabla D2, márgenes y fronteras), pero nada cubre: ráfaga con ts de drenaje (B-1), `ReleaseBuffer` fallido (I-2), reapertura repetida (I-3), ni nada de `wasapi_src.rs` (el HANDOFF lo admite). Criterios de `docs/06`/PLAN sin ejecución real: Paso 0, 3(a), 3(b), 3(c), 4 (tiempos de reapertura).

### MENOR / NOTA

- **Nota.** `detener()` y `Drop` hacen `join()` sin plazo (`wasapi_src.rs:160-162,174-176`): una `Activate` colgada en una reapertura bloquea quien detiene. Antes solo ocurría en el arranque.
- **Nota.** `Initialize` con 3 s sin retroceso (`:765-778`): si un controlador virtual (p. ej. VoiceMeeter, Bluetooth HFP) lo rechazara, se clasifica Transitorio y la pista reintenta para siempre con la misma petición. No probado; probablemente sin efecto en dispositivos normales.
- **Nota.** Criterio (a) «≤ 3 s hasta señal»: el diseño (2 sondeos de 1 s + apertura de hasta 0,7 s + ventana de 1 s para medirlo) deja el peor caso en ~3 s; pasa o no según la fase. No es un defecto, es un margen que el PO debe conocer.
- **Nota.** Una pista Viva que termina en silencio no se rellena hasta `fin_ms` (deuda declarada, preexistente; la medida de línea base lo muestra).
- **Nota.** AV-1 sola cierra el caso «la sesión termina» y abre «la sesión sigue grabando nada sin aviso»: depende de AV-2 (PLAN lo dice). No debe integrarse sin ella.
- **Menor.** `sincronia.rs:531`: línea de 121 caracteres (contados, no bytes); preexistente (`b7f93eb`), una cadena con espacios internos que además imprime un hueco de 14 espacios en el mensaje.

## 3. Premisas que cuestiono

1. **«Un búfer mayor no añade latencia, solo margen»** (`:90-98`, ronda 3). Conclusión: cierta para la entrega, **falsa para el relleno**. El relleno depende de que el instante de lectura sea el de captura; un búfer que acumula lo rompe (B-1). Es la pregunta que me hiciste: el búfer de 3 s no añade latencia de entrega, pero **sí** altera el relleno.
2. **«Abrir y arrancar seguidas equivale a abrir ambas y luego arrancar»** (decisión 7). Conclusión: no equivale; mueve el `Start` del micrófono antes del reloj (B-1, (d) par 1).
3. **«El drenaje previo de D4 hace inocuo el bloqueo»**. Conclusión: drena lo anterior al bloqueo, no lo que se acumula durante él; con 3 s de búfer lo acumulado se retiene y se fecha mal.
4. **«El incumplimiento de (d) es carga, no el búfer»** (HANDOFF dice que no puede afirmar). Conclusión: la carga lo dispara pero la causa es de código (tabla de B-1). Con una sola repetición cargada el implementador no podía verlo; la distribución de tramas del primer segundo sí.
5. **«Un error del sondeo nunca toca una pista sana»**. Conclusión: se sostiene (ver §4).
6. **«Idempotente ante cambio de default + `DEVICE_INVALIDATED`»**. Conclusión: se sostiene en la máquina de estados y en el cableado; lo que queda abierto es la *repetición* de la reapertura (I-3).

## 4. Qué verifiqué y no marqué

- **Invariante 1 (vaciar antes de soltar), todos los caminos.** `soltar_para_reabrir` (`:528-545`) llama a `cerrar_pista` (vacía, luego `Stop`) y **solo después** lee `muestras_emitidas()`; `empezar_reapertura` recibe ese valor. Camino de detención (`:428-443`): `cerrar_pista` para las vivas, `relleno_final` para las demás. Si `vaciar` falla, se registra y el contador no incluye la cola (queda como silencio por relleno al reabrir): coherente. `Remuestreador::vaciar` recorta el relleno de ceros al ratio, así que no infla el contador más de ~1 muestra.
- **Contador nunca vuelve a 0 en reintentos.** `intento_fallido` solo toca `ultimo_intento`/`intentos`; `fallo_permanente` copia el contador a `Ausente`; `al_abrir` no lo toca y `abrir_cliente` recibe `p.control.contador()` como `muestras_previas`. Único `0` con sentido: `sin_abrir_al_arrancar` y `abrir_si_se_pidio` (pista que no existía). Las pruebas `los_reintentos_fallidos…` y `un_fallo_permanente…` lo fijan y las leí sin réplicas.
- **Idempotencia.** Orden del bucle: lectura (1) → sondeo (2) → reintentos (3). Si en (1) la pista pasa a Reabriendo, `sondear` devuelve `Nada` (solo actúa en Viva/Ausente), así que no hay segundo cierre. `empezar_reapertura` devuelve `false` en Reabriendo/NoPedida; `soltar_para_reabrir` con `abierta == None` no hace COM.
- **Error del sondeo.** `sondear` con `None` limpia `candidato` y retorna `Nada` sin tocar el estado; solo se llama a COM cuando toca (≥ 1 s). El `warn!` solo en el primero de la racha.
- **Margen.** 2 sondeos seguidos del mismo id, enfriamiento de 5 s solo para Viva (una Ausente no tiene qué proteger). Probé a mano la secuencia 10 s:b, 11 s:c, 12 s:c → reabre en el segundo `c`; y vaivén a/b/a: no reabre. Durante el enfriamiento el contador de racha sigue subiendo y reabre en el primer sondeo posterior a los 5 s.
- **Relleno final hasta `fin_ms`.** `muestras_de_relleno(fin_ms, contador, 0)`; sin resta negativa (`saturating_sub`); `timestamp_ms = fin_ms`. Cubre Reabriendo y Ausente, incluida la que nunca abrió.
- **Sin salida por estado de dispositivos; hilos/COM al detener.** El `loop` solo sale por `rx_parar` (mensaje o desconexión); no hay `?` ni `return` dentro del cuerpo (`reabrir`, `soltar…` devuelven `()`). Orden de destrucción al salir: `pistas` → `enumerador` → `_com` (declarados en ese orden) → `CoUninitialize` al final y con los `IAudioClient` ya detenidos y soltados. `Drop` de `SesionWasapi` cubre el caso de soltar sin `detener`. Pendiente solo la nota del `join` sin plazo.
- **Lógica pura de `PistaCapturada::continuar`.** `canales.max(1)`, `Remuestreador::nuevo` falla → `Permanente`; primer paquete sin salida del remuestreador a 48 kHz: lo cubre una prueba que reproduce el caso real (leí el cuerpo).
- **Anchos de línea.** Contados en caracteres con script: una sola línea >100 en los tres archivos (la preexistente de `sincronia.rs:531`).
- **`aaudio_src.rs`**: solo un comentario, y es verdadero.
- **`id_de_dispositivo`**: libera el puntero también en el camino de error (`CoTaskMemFree` tras `to_string`), igual que antes.
- **Entorno (para quien reproduzca).** En el target compartido `D:\dev_v1\dictar_ia\target` obtuve **8 fallos y luego 1 fallo** de pruebas puras de `sincronia` con el mismo código; en un target aislado (`D:\dev_v1\target-av1-rev`, copia de la ref con `git archive`) pasan 80/80 cuatro veces; repetido tras la indicación del orquestador en ese target: 81/81 con mi prueba añadida y los mismos desfases. Es contaminación del target compartido por otra ejecución concurrente (se ven carpetas `mut*` en el scratchpad), no un defecto de AV-1. **No lo trato como hallazgo**, pero quien use ese target para verificar puede ver rojos falsos.

## 5. Qué no pude verificar y qué haría falta

- **Reaperturas reales (Paso 0, (a), (b), (c), D4 de reapertura).** Requieren un segundo dispositivo de salida y de entrada y deshabilitar uno como administrador. Nada de `reabrir`/`soltar_para_reabrir` se ha ejecutado. Con B-1 corregido, repetir (c) y mirar la diferencia de duración y de posición entre `mic.wav` y `system.wav` (no solo la duración total).
- **(d) del código final.** No ejecuté yo el ejemplo con COM (me apoyé en los logs del implementador y en la reproducción aritmética). Para cerrarlo: tres pares nuevos tras corregir B-1, y contar tramas del micrófono en el primer segundo.
- **Clippy de `dictar-api --examples`** (el ejemplo `wasapi_loopback_eval.rs`): no lo corrí; solo `dictar-audio`. Leí el ejemplo: el único `Subscriber` hecho a mano es código de ejemplo y no tiene errores evidentes, pero sin clippy no lo afirmo.
- **Windows con Bluetooth y `eCommunications`**: sin dispositivo, sin cobertura.
- Residuos míos: `D:\dev_v1\target-av1-rev` (target aislado) y la copia `…\scratchpad\rev1` (con el test `revision_rafaga` añadido a *mi copia*, no al repositorio); se pueden borrar. No toqué ningún archivo del repositorio salvo este informe.
