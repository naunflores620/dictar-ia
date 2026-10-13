# REVIEW-codigo — AV-2 «Avisos de salud del audio durante la grabación»

Revisado sobre `git diff f45b274 refs/revision/av-2` y el árbol de trabajo (mismo contenido). El `HANDOFF.md` se trató como declaración; lo contrastado está en cada sección.

## 1. Veredicto

**RECHAZADA hasta corregir 2 hallazgos Importantes (H-1, H-2). Nada Bloqueante.** Compilación, pruebas Rust (68), clippy y Flutter reproducidos; D7 reproducido; D4 medido en Release (supera 20 ms una vez). Lo único `NO VERIFICABLE` es el criterio 6 (pruebas manuales con altavoces y persona) y el comportamiento del dispositivo real (WASAPI).

H-1 es un fallo real de la alineación por `timestamp_ms` (D2) en el caso que D2 existe para cubrir: reproducido con una prueba fuera del repositorio. H-2: la prueba que dice proteger esa alineación no la protege (mutación hecha).

## 2. Hallazgos

### Importante

**H-1. `Pista::escribir` interpreta `timestamp_ms` como el inicio de la trama; en Windows es el final. Tras un hueco largo del loopback, el anillo del sistema queda ciego.**
`core/api/src/salud_audio.rs:164-188` (`let ini = timestamp_ms.max(0) * MUESTRAS_POR_MS`).

- Qué pasa. En WASAPI el `timestamp_ms` se lee **después** de `GetBuffer` (`wasapi_src.rs:392`) y es el instante de recepción; el paquete que reanuda trae `relleno + audio` y acaba en ese instante (`sincronia.rs:85-105`, comentario: «se lee *después* de que `GetBuffer` ya entregó ese audio»). En PipeWire sí es el inicio (`pipewire_src.rs:56`, conteo de muestras previo). `salud_audio` asume el inicio, sin `cfg` (que el invariante 4 prohíbe).
- Caso concreto. Loopback mudo 45 s (el escenario de `sin_senal_inicio`, o una pausa de la clase). Llega el paquete con `ts = 45 000`, `pcm` = 45 s de ceros + audio. `escribir` lo coloca en `[45 s, 90 s]`: `self.fin` salta a 90 s. Las tramas siguientes (`ini ≈ 45 s`) cumplen `fin <= self.fin - CAPACIDAD_ANILLO` y se **descartan** («llegó tan tarde», línea 168) hasta que el reloj alcanza ~74 s. Para un hueco de 3 min (`sin_senal_pausa`) son ~164 s sin sistema en el anillo.
- Efecto. Durante ese tiempo `eco`, `recorte` y `nivel_bajo` del sistema ven silencio. Un `eco` activo se apaga a los 30 s por «falta de datos» (línea 483) aunque el eco siga. `sin_senal_*` no se ven afectados (usan `ultima_activa`, que no pasa por el anillo).
- Reproducción. Copia de trabajo en el scratchpad (`scratchpad/mut`), prueba `wasapi_relleno_largo_deja_ciego_el_anillo`: micrófono con eco latente continuo, sistema mudo 0-45 s, paquete de relleno con `ts` = fin del paquete, voz después. Con el código entregado el `eco` se activa a los **85 000 ms**; con `ini = ts*16 - pcm.len()` a los **60 000 ms** (lo esperable: 5 s de actividad + dos evaluaciones). Un modelo aislado de `Pista` dio el mismo resultado: 701 de 1000 tramas posteriores invisibles (14 s).
- Qué debería pasar. Ubicar la trama por su **final** en Windows. Una forma que sirve a las dos plataformas sin `cfg`: `ini = max(self.fin_de_la_pista, ts*16 - pcm.len())` (en PipeWire nunca hay hueco, así que resulta contiguo; en WASAPI el relleno cae en su sitio). Hay que revisar `un_hueco_sin_tramas_queda_a_cero...` y las otras dos pruebas del anillo, que codifican la semántica de «inicio».
- Por qué no se vio. D7 alimenta WAV ya rellenados y con `ts` de inicio; las pruebas sintéticas usan `ts` de inicio. El propio HANDOFF lo admite: «Que `timestamp_ms` marque el inicio... lo asumí».
- Nota de severidad: no pierde audio ni toca disco; falla en lo que D2 prometía. Lo subo a Importante, no Menor, porque es justo el caso que motivó la decisión de diseño.

**H-2. `el_eco_se_alinea_por_timestamp_y_no_por_orden_de_llegada` no protege la alineación, y su comentario dice lo contrario.**
`salud_audio.rs:1026-1061`.

- Mutación: en la copia, `escribir` con `let ini = self.fin;` (orden de llegada, `timestamp_ms` ignorado). Resultado: fallan solo las 3 pruebas del anillo (`un_hueco_sin_tramas...`, `el_anillo_no_devuelve_audio_viejo...`, `una_trama_demasiado_antigua...`). **La prueba de eco pasa**, con 25 de 28 en verde.
- Causa: la prueba entrega el relleno de ceros (`relleno`, líneas 1052-1058), de modo que el orden de llegada y el de `timestamp` coinciden. Para distinguirlos habría que **no** entregar el relleno y evaluar durante el hueco.
- El comentario («Antes del arreglo... un falso eco perfecto. Por llegada coinciden») afirma que la prueba falla con el defecto; no falla. Comentario falso = Importante según el criterio del repositorio. El HANDOFF reconocía no haber hecho mutación.

### Menor

- **M-1. Comentario del anillo exagera.** `salud_audio.rs:52-54`: «caería siempre fuera». Con ritmo estable, el desvío es una trama (decenas de ms); el margen de 1 s solo importa por H-1. Reescribir o quitar «siempre».
- **M-2. Comentario de prueba con cifra equivocada.** `salud_audio.rs:718`: «100 s de silencio»; el tramo es de 10 s a 150 s, 140 s.
- **M-3. Doc de `trama` incompleta.** `salud_audio.rs:287-289`: dice que solo resuelve «señal recuperada, micrófono de vuelta», pero `reglas_de_presencia` también **activa** (p. ej. `mic_sin_tramas` desde una trama del sistema si no hubo `tick`).
- **M-4. Texto fijo que deja de ser cierto.** `salud_audio.rs:122`: «Hace 3 minutos que no suena nada» se queda en pantalla mientras el silencio sigue 20 o 60 minutos. Falso por defecto, no por exceso; considerar «Hace más de 3 minutos».
- **M-5. `AvisoAudio.esGrave`** (`dominio.dart`): «Cuando no llega nada, la grabación no sirve» incluye `sin_senal_pausa`, que puede ser una pausa legítima de la clase. El color de error exagera.
- **M-6. `_detener` (`grabacion.dart:343-349`) cancela y vacía antes de `await detenerGrabacion()`.** Si el núcleo lanza, la pantalla se queda grabando sin banda y sin suscripción hasta el final de la sesión, sin ninguna señal. Escenario poco probable; el HANDOFF lo presenta como decisión (nº 3) sin mencionar este coste. Mitigar volviendo a suscribir en un `catch`, o documentarlo.
- **M-7. `recorte` se desactiva más tarde que lo documentado.** El plan dice «10 s sin recorte»; `ultimo_recorte` se actualiza mientras la ventana de 5 s todavía contenga saturación, así que el apagado llega 10-15 s después de la última muestra saturada. La prueba (línea 914) lo comenta bien, pero `CALMA_RECORTE` no.
- **M-8. Ejemplo `salud_audio_eval.rs`.** Termina con código 0 aunque todas las sesiones fallen (solo `eprintln!`), y salta en silencio las carpetas con solo `mic.wav`. Sin consecuencias para producto.
- **M-9. Pruebas ausentes.**
  - Ninguna prueba fija que `bucle_escritura` llame a `tick` en el brazo `Timeout`, ni que `salud.trama` vaya tras `escritor.escribir`. Lo verifiqué leyendo; una refactorización puede romperlo sin que nada se ponga en rojo. `bucle_escritura` se puede probar con un `mpsc` y un directorio temporal.
  - Dart: `case 'salud'` de `RepositorioRust` sin prueba (declarado). La limpieza **al empezar** no tiene prueba (en la práctica cada sesión monta una pantalla nueva, así que es defensiva). Falta comprobar que un evento llegado **después** de Detener no repinta la banda.
  - Criterio 1 del plan: no hay prueba de bloqueo de 15 s para `nivel_bajo`, `recorte` ni `eco`; solo para `mic_sin_tramas`.

### Nota

- **N-1. D4 (informar, no decidir).** Release reproducido (`target/release/examples/salud_audio_eval.exe`; no hubo bloqueo 4551): máx por tick **6,9 ms** (Diseño), **2,9 ms** (Comunicaciones), **35,8 ms** (clase larga, 3464 evaluaciones > 100 µs); medias 0,8-1,2 ms. El máximo de 35,8 ms supera el umbral de 20 ms del plan, una vez en la clase larga; no está caracterizado si fue la evaluación de eco o un parón del sistema. No hay pérdida de audio posible por esto (el canal de tramas es `mpsc::channel` sin límite), pero retrasa la alimentación de Vosk lo que dure. La decisión de cambiar de hilo es del coordinador.
- **N-2.** La CLI (`cli/src/sesion.rs:109-116`) ignora `Evento::Salud` por `Ok(_)`: quien grabe por CLI no recibe avisos. Fuera de alcance, pero conviene anotarlo.
- **N-3.** En la rama Android de la prueba D8, `Capturar diapositivas` `findsNothing` ya es vacuo (la cadena no existe en ninguna pantalla); lo que protege es el contraste de «Área para captura manual» con la rama de escritorio.
- **N-4.** El mínimo de actividad del micrófono en el eco (3 s) se cumple con ruido de sala > −50 dBFS, porque «activa» mide actividad, no voz. No produce falso eco (correlación con una envolvente plana ≈ 0 en la cuenta que hice: ≥ 250 pares, sd ≈ 0,06), pero el mínimo del micrófono es casi decorativo.

## 3. Premisas que cuestiono

1. **«`timestamp_ms` es el inicio de la trama» (todo `Pista`).** Falsa en WASAPI. Conclusión: H-1.
2. **«El orden de llegada no sirve como reloj, por tanto alinear por `timestamp_ms` es lo que protege.»** Cierta a medias. Como `sincronia.rs` siempre entrega el relleno al volver, el orden de llegada y el de `timestamp` coinciden **tras** el hueco; lo que de verdad aporta la alineación es el **reloj de pared** (`posicion(ahora)`) para evaluar *durante* el hueco, y eso sí está bien hecho (`extraer` devuelve 0 más allá de `fin`). La prueba D2 no ejercita eso (H-2).
3. **«Un micrófono abierto entrega tramas siempre»** (`MIC_SIN_TRAMAS`, 10 s). Por código: WASAPI compartido entrega paquetes con la bandera SILENT incluso en silencio y `procesar_paquete` emite por bloques de 1024 muestras, así que la premisa es razonable. No lo pude ejercer con hardware: un Bluetooth en renegociación o un dispositivo que no entregue paquetes en mute daría falsos positivos de «dejó de enviar». `NO VERIFICABLE`.
4. **«`Instant::now()` del hilo de escritura equivale al instante de captura.»** Lo puse a prueba pensando en un parón del hilo de escritura (flush lento, evaluación de eco larga): la cola se acumula, pero `trama()` fija `ultima_trama = ahora` **antes** de `reglas_de_presencia`, así que al reanudar no se dispara `mic_sin_tramas` en falso. Conclusión: se sostiene.
5. **«El coste del eco es asumible en el hilo de escritura.»** Medido: media ~1 ms, pico 6,9 ms en dos sesiones, 35,8 ms en la tercera (N-1). Se sostiene salvo ese pico; no he podido atribuirlo.
6. **«El vaciado final no necesita `SaludAudio`: no hay interfaz.»** Se sostiene: la pantalla cancela la suscripción antes de `detenerGrabacion`, y el `rx_ev` se sustituye en cada sesión (`puente.rs:649, 719`), así que no quedan eventos viejos.

## 4. Qué verifiqué y no marqué

- **Reproducido:** `cargo test -p dictar-api --lib --tests` (con `CARGO_INCREMENTAL=0`; el `target/` compartido da ICE de rustc con incremental activo): **68 passed, 0 failed**, incluidas las 28 de `salud_audio` (el HANDOFF dice 27 + 1) y la de mapeo en `puente.rs`.
- **Reproducido:** `cargo clippy -p dictar-api --all-targets --no-deps -- -D warnings`: exactamente los 3 errores preexistentes de `transcriptor.rs` (58, 848, 966). Sin ninguno en `salud_audio.rs`, `grabacion.rs`, `puente.rs`, `eventos.rs` ni en el ejemplo (revisado también sin `-D warnings`, 3 avisos, los mismos). El error de `audio-capture/src/lib.rs:304` no apareció por `--no-deps`; no es de esta tarea.
- **Reproducido:** `rustfmt --check` sin diferencias en los archivos nuevos y `eventos.rs`; anchos de línea contando caracteres: solo dos líneas > 100 (`grabacion.rs:246`, `:618`), ambas preexistentes (`git blame`) y dentro de macros `tracing`.
- **Reproducido:** `flutter test` (43 pruebas, todas en verde, incluida D8) y `flutter analyze` (1 info preexistente, `ajustes.dart:184`).
- **Reproducido:** D7 en Release. Mismas activaciones que declara el HANDOFF (Comunicaciones: eco a 25 s; clase larga: 50 s, 2355 apaga, 2370, 3400 apaga con valor 0,000, 3415; Diseño: nada; sin `nivel_bajo` ni `sin_senal_*`). No investigué qué pasa en el audio a los 3370-3400 s (silencio real o fallo); el HANDOFF tampoco.
- **`tick` en el brazo `Timeout`:** `grabacion.rs:419-425` queda fuera del `match`, tras `Ok`/`Timeout`; ningún `continue` dentro del brazo `Ok` lo salta. `Disconnected` rompe antes. Correcto.
- **El audio va primero:** `escritor.escribir(&frame)?` (línea 333) precede a `salud.trama`; los envíos son `let _ = tx.send` sobre `mpsc::channel` sin límite (no bloquea). Busqué rutas de pánico en `Pista::escribir/extraer`, `Actividad`, `evaluar_*` (índices negativos, división por cero, `max - 1` con cero): `p` nunca es negativo (`ini ≥ 0`, `desde.max(0)`), `ventana.len().max(1)`, `saturating_*` en tiempos y contadores, NaN saneado en el anillo. No encontré una ruta. No hay `catch_unwind`, así que un pánico futuro mataría el hilo de escritura sin aviso; hoy no veo cómo.
- **Errores tragados:** los `let _ = tx.send(...)` siguen el patrón del resto del bucle (el receptor desaparece al parar). No hay `unwrap_or_default` ni `catch` mudo en el código nuevo. No hay llamada a la función que falló dentro de un manejador de errores.
- **Invariantes de `protocolo.md`:** pistas por separado (un anillo por pista, solo se comparan); 16 kHz (el ejemplo rechaza otra frecuencia, `salud_audio` usa `SAMPLE_RATE`); sin `cfg` en `salud_audio`/`Evento::Salud`; ninguna firma pública del puente cambió (`EventoDto` intacto, mapeo con prueba).
- **Mapeo D5:** `puente.rs:225-239` coincide con la tabla del plan y con `case 'salud'` (`track=='mic'`, `fase`, `definitiva`, `fraccion`, `mensaje`); `Track::as_str` devuelve `"mic"`/`"system"`. Un `track` desconocido cae a `sistema` en Dart; aceptable.
- **Ciclo de vida de la banda (Dart):** vaciado al empezar (`setState` + `_vaciarAvisos`), `_subAvisos?.cancel()` antes de volver a suscribir, cancelado y vaciado síncrono al detener, cancelado en `dispose`, `StreamController` cerrado en `RepositorioRust.dispose`, `RepositorioDemo` con `Stream.empty()`. Doble pulsación de Detener: segunda llamada con `_subAvisos` nulo, inofensiva. Cierre «por clave»: aviso cerrado no reaparece mientras siga activo y vuelve al reactivarse (cubierto por prueba). Excepción: M-6.
- **Anillo de 16 s:** el margen de un segundo cubre el adelanto de una trama; no es el problema de H-1. La capacidad no desborda (`Vec` de 256 000 `f32` por pista, 1 MB).
- **Mínimos de actividad del eco:** 5 s sistema / 3 s micrófono sobre la **misma** ventana alineada `[ahora-15 s, ahora-0,5 s]`; `Actividad::medir` ignora el resto de menos de 30 ms; dos evaluaciones seguidas altas para activar y bajas para apagar, racha rota por zona intermedia o falta de datos. Los casos «mínimo no alcanzado» y «racha rota» tienen prueba.
- **Aritmética:** sin restas sobre `usize` (los índices son `i64`); `saturating_duration_since` en todos los `Instant`; ratios acotados por construcción.
- **Pruebas de eco:** el negativo usa modulación silábica independiente (no ruido blanco) y el positivo retrasa 200 ms y atenúa, como pide el plan.

## 5. Qué no pude verificar y qué haría falta

- **Criterio 6 (a-e), prueba manual en Windows:** requiere persona, altavoces y micrófono. `NO VERIFICABLE`. En particular (d), 3 min sin falsos positivos, es el único control de falsos positivos con dispositivo real.
- **Semántica real de `timestamp_ms` en el dispositivo** (H-1): la deduje del código de `wasapi_src.rs`/`sincronia.rs`; falta una captura real con pausa de loopback para registrar `(timestamp_ms, pcm.len())` de los paquetes.
- **Atribución del pico de 35,8 ms (N-1):** habría que instrumentar por separado la evaluación de eco y repetir varias veces en una máquina sin otros agentes compilando (había varios `cargo` activos durante mis mediciones).
- **Que el evento llegue por el puente real a Dart:** sin núcleo en las pruebas de widget; haría falta una prueba de integración con `flutter_rust_bridge`.
- **Disposición compacta con tooltip en un monitor real.**
- **Doctests:** no ejecutados (ICE conocido de rustdoc).

Efectos colaterales de mi revisión (para que nadie se sorprenda): ejecuté `touch core/api/src/lib.rs` (solo cambió la fecha, no el contenido) para forzar a clippy; produje artefactos de build en `target/` (debug y release); mis copias y pruebas de mutación están en `scratchpad/mut` y `scratchpad/repro`, fuera del repositorio. No edité ningún archivo del repositorio salvo este informe.
