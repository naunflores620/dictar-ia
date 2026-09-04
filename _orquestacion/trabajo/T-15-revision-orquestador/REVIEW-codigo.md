# REVIEW-codigo — T-15

> **Nota del orquestador.** Este informe lo escribió el `revisor-codigo` pero no pudo guardarlo:
> su definición en `.claude/agents/` no incluía la herramienta `Write`, un defecto de
> configuración del mecanismo, no suyo. Lo entregó como texto y el orquestador lo volcó aquí
> **íntegro, sin resumir ni suavizar**, como manda `protocolo.md`. El defecto quedó corregido:
> `revisor-codigo` y `auditor-plataforma` ya tienen `Write`.

## 1. Veredicto

**NO VERIFICABLE por B-1 y B-2** en todo lo que dependa de compilar Rust o analizar/testear
Flutter. En revisión estática: **2 hallazgos Importantes, 0 Bloqueantes**, más 3 notas. (c) sin
hallazgos.

Confirmé los dos bloqueos de primera mano, hoy:
- `cargo`/`rustc`: ausentes del PATH. B-1 sigue activo.
- `flutter pub get` (en `app/`): falla — *"The current Dart SDK version is 3.11.4. Because
  dictar_ia requires SDK version ^3.12.2, version solving failed."* B-2 sigue activo. No corrí
  `flutter analyze` (la instrucción era condicional a que `pub get` funcionara, y no funciona;
  correrlo igual solo produciría el ruido que la regla 8 excluye).

## 2. Hallazgos

### Importante — H1: la frase nueva del README ya no es del todo cierta (README.md:4-6, 93, 191)

**Qué pasa.** El texto dice: *"Windows y Android son el objetivo de diseño, pero ninguno de los
dos graba todavía — falta el backend de captura de audio en cada uno (WASAPI *loopback* en
Windows, AAudio en Android)"* (líneas 4-6), y la tabla de empaquetado dice *"el `.exe` y el APK
todavía no graban"* (línea 93). A fecha de esta revisión, `core/audio-capture/src/lib.rs:172-175`
ya enruta `target_os = "windows"` a `wasapi_src::iniciar(cfg)`:

```rust
#[cfg(target_os = "windows")]
{
    wasapi_src::iniciar(cfg)
}
```

Esto no es un stub: `wasapi_src.rs` es un módulo real de ~480 líneas con COM/WASAPI *loopback*
completo. Confirmé que esta rama **no existía en HEAD** (`git show HEAD:core/audio-capture/src/lib.rs`
solo tiene la rama Linux y un catch-all `NoSoportada`), así que el cambio es posterior al commit y
comparte el mismo árbol sin commits que T-15. Encontré el origen:
`_orquestacion/trabajo/HU-01-wasapi/` (PLAN.md, HANDOFF.md, REVIEW*.md) es una HU real, activa, en
su "3.ª vuelta" (`tablero.md:71,99`) — una tarea paralela distinta de T-15, sobre el mismo árbol.

La misma afirmación, casi palabra por palabra, está en `.github/workflows/release.yml:14-16`
("`iniciar()` devuelve `NoSoportada` fuera de Linux"), archivo que la propia tarea me pidió leer
para verificar el README — y ahí también es falsa para Windows.

**Qué debería pasar.** No necesariamente reescribirla en la otra dirección ahora mismo:
`wasapi_src.rs` viene de una HU que en su última revisión formal (`HU-01-wasapi/REVIEW.md`) tuvo
hallazgos **Bloqueantes** (p. ej. un `?` compartido entre las dos pistas que pierde el resto de la
sesión ante un error COM transitorio) y sigue sin cerrar. Afirmar "Windows graba" sería tan falso
como afirmar que no graba. El propio proyecto ya tiene el patrón correcto para esto:
`tablero.md:100` (T-3/HU-05) trata la frase del README sobre el llavero exactamente así — *"las dos
cosas serán falsas al cerrar esta HU, pero no se toca antes: si la HU volviera a implementación, el
README mentiría en la otra dirección"*. La frase sobre Windows/WASAPI necesita el mismo
tratamiento: quedar registrada como condición de cierre atada al cierre real de HU-01-wasapi, no
una corrección improvisada ahora.

**Caso concreto.** Alguien lee el README hoy, decide no evaluar el `.exe` en Windows "porque no
graba", cuando el código ya lo intenta — con fiabilidad todavía no cerrada por su propia cadena de
revisión.

**Atribución.** No es descuido de quien escribió esta línea (README.md tiene mtime 08:39; `lib.rs`
con la rama Windows apareció después, hacia el mediodía). Era cierta cuando se escribió.

*(No lo marco como sospecha: tengo la línea exacta de `lib.rs` que contradice la afirmación, y la
comparación contra HEAD que fecha el cambio.)*

### Importante — H2: HU-11 criterios 1 y 4 no tienen ninguna prueba (app/lib/ventana.dart, ajustes.dart, grabacion.dart)

**Qué pasa.** `docs/06-historias-de-usuario.md`, HU-11, exige: (1) un móvil en vertical usa la
disposición amplia, no la compacta de escritorio; (4) las funciones sin sentido en móvil no se
ofrecen. `app/test/` solo tiene `dominio_test.dart` y `notas_markdown_test.dart` (verificado por
`Glob`); ninguno de los dos, ni ningún otro archivo de test, menciona `Ventana`, `esEscritorio`,
`soportado`, `debugDefaultTargetPlatformOverride`, `PantallaAjustes` ni `PantallaGrabacion`
(verificado por `Grep` sobre todo `app/test`, cero resultados).

**Qué debería pasar.** Al menos un `testWidgets` por pantalla que fije
`debugDefaultTargetPlatformOverride = TargetPlatform.android` y compruebe que "Área de la
diapositiva" / "Capturar diapositivas" no se renderizan; y un test unitario de
`Ventana.esEscritorio` para cada plataforma.

**Caso concreto.** Alguien invierte por error la negación en `esEscritorio` (o agrega
`TargetPlatform.android` a la lista de escritorio) y ningún test lo detecta — tampoco el día en que
B-2 se resuelva y `flutter test` pueda correr, porque no existe ningún test que ejercite esa rama.

### Nota — esEscritorio incluye macOS, sin producto que lo use ahí

`ventana.dart:52-54` incluye `TargetPlatform.macOS` en `esEscritorio`. Es una condición heredada (ya
estaba, igual, dentro de `preparar()` antes de este diff — verificado contra `git show HEAD`), no
algo nuevo de T-6. Se registra porque ahora es un getter público reutilizado en más sitios, así que
su alcance efectivo creció, pero no es defecto de este cambio.

### Nota — docs/06 (HU-11 criterio 4) quedó desactualizado por el propio T-6

`docs/06-historias-de-usuario.md` sigue marcando el criterio 4 de HU-11 como incumplido, citando
`ajustes.dart:645-654` sin protección de plataforma. Verifiqué que **hoy no es así**: esa navegación
vive dentro de `_AreaCaptura`, y `_AreaCaptura` solo se inserta en el árbol si
`Ventana.esEscritorio` (`ajustes.dart:67-70`) — no lo tomo de esa auditoría, lo comprobé yo mismo
leyendo el archivo. Los `mtimes` cuadran con la secuencia: la auditoría de QA es de las 07:26;
`ventana.dart`/`ajustes.dart` cambiaron pasado el mediodía, después. Es decir, la auditoría disparó
el arreglo y nadie volvió a correr QA para reflejarlo. No es un hallazgo de código; hace falta una
pasada de `qa` para refrescar `docs/06`.

### Nota — el resumen de T-6 no menciona el arreglo de `_esCompacto`

`grabacion.dart` trae, en el mismo diff, un cambio no descrito ni en el `LEEME.md` de T-15 ni en
`tablero.md:132`: `_esCompacto` pasó de `ancho < 460 || alto < 560` a decidir solo por `alto < 560`.
Lo revisé igual, por venir en el archivo que se me pidió mirar (ver §4). Es correcto y necesario
para que HU-11 criterio 1 se sostenga en un móvil real, pero su ausencia en ambos resúmenes es un
hueco de trazabilidad de la propia tarea, no del código.

## 3. Premisas que cuestiono

**"La frase nueva del README es cierta."** Atacada en H1. Conclusión: **no del todo** — cierta para
Android, ya no para Windows, por una razón concreta y verificable (`lib.rs:172-175`), no por
sospecha.

**"`esEscritorio` (y no `soportado`) es el proxy correcto para decidir si ofrecer captura de
pantalla."** La contrasté contra la frontera real del backend, no contra la explicación en el propio
comentario: `core/screen-capture/src/lib.rs:406` gatea sus stubs `NoSoportada` con
`#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]` — exactamente el
mismo conjunto de plataformas que `esEscritorio`. Conclusión: **se sostiene**, y no por casualidad:
es la misma frontera, verificada en el crate que de verdad implementa la función.

**"Revisar T-15 como si sus tres cambios existieran en un árbol estable."** No se sostiene: sin
ramas por tarea (`tablero.md`, "Regla suspendida") ni commits, el árbol es compartido en tiempo real
con HU-01-wasapi y HU-05-llavero. Encontré dos piezas de documentación que se desactualizaron
mientras estaban siendo escritas o poco después (H1, y la nota de `docs/06`/HU-11). La regla de
"archivos disjuntos" del protocolo protege colisiones de edición, no coherencia semántica entre un
archivo de prosa y el código que describe en otra carpeta.

**"La decisión de dejar sin condicionar el botón «Capturar diapositiva» durante la grabación (T-6b)
no abre una vía insegura."** La ataqué trazando la ruta completa: `_capturar()` en
`grabacion.dart:181-202` llama a `widget.repo.capturarDiapositiva()` → `puente.rs:634` →
`core/screen-capture`, que en Android cae en el mismo `NoSoportada` de arriba; el `try/catch` de
`_capturar()` muestra el error en un `SnackBar`, no lo traga ni finge éxito. Conclusión: **se
sostiene**. Es una aspereza de UX (un botón que en Android siempre falla), no un fallo silencioso, y
está trackeada aparte con su propio `PLAN.md` pendiente (`tablero.md:110`).

## 4. Qué verifiqué y no marqué

**(a) README.md / release.yml.** Leí el diff completo de ambos archivos. Confirmé que el job `build`
de `release.yml` corre para `linux` y `windows` sin condición de nivel de job, y que el paso
`instalador .exe` solo tiene `if: matrix.plataforma == 'windows'` — nunca una bandera de
"desactivado"; eso valida que T-10 corrigió bien la mentira original. Para la frase nueva, crucé
contra código: `core/audio-capture/src/lib.rs` líneas 1-60 y 160-200 (rama Windows real) y
`core/screen-capture/src/lib.rs` líneas 395-427 (Android sigue en `NoSoportada`, confirmado). Hice
spot-check de otras dos afirmaciones nuevas verificables sin ejecutar nada:
`core/storage/Cargo.toml:16,20` confirma `default = []` y `cifrado` apagada por defecto. No
re-verifiqué los recuentos de tests («331», «63», «33 funciones del puente»): requieren compilar o
contar; ya los contó `qa` por `grep` en `docs/06` (HU-13), y contar de nuevo sin poder ejecutar
`cargo test`/`flutter test` no aporta nada sobre si *pasan*. No toqué nada de `core/providers`
(instrucción explícita, HU-05 en revisión ahí).

**(b) Ventana.esEscritorio / soportado.** Leí `ventana.dart` completo y los diffs completos de
`ajustes.dart`, `grabacion.dart`, `region.dart`. Probé por De Morgan que `preparar()` se comporta
igual: `kIsWeb || !(linux||windows||macOS)` (versión vieja) es exactamente `!esEscritorio` (versión
nueva). Recorrí con `Grep` sobre todo `app/lib` las cadenas `windowManager`, `PantallaRegion(`,
`capturarDiapositiva`, `.region(`, `esEscritorio` para confirmar que no queda ninguna vía de acceso
sin condicionar: `windowManager` solo aparece en `ventana.dart` y `region.dart` (ambos protegidos);
`PantallaRegion(` solo se navega desde `ajustes.dart` y `grabacion.dart`, ambos dentro de
`if (Ventana.esEscritorio)`. Confirmé que el `catch` de `region.dart` ya no reinvoca `_capturar()`
(arreglado antes de este diff, verificado contra `git show HEAD`) y que `_error` sí se pinta en
`build()` (líneas 127-128) — no se traga. Revisé estructuralmente ambas listas (`ajustes.dart` y
`grabacion.dart`) buscando `Divider`/`SizedBox` huérfanos: en los dos casos el separador viaja
dentro del mismo bloque `if (...) [...]` que el widget condicionado; sin huérfanos ni duplicados.
Verifiqué el arreglo de `_esCompacto` con números concretos (móvil ~400×800: el chequeo viejo por
ancho lo mandaba siempre a compacto; el nuevo, solo por alto, no).

**(c) sincronia.rs.** El archivo está sin trackear (nunca comiteado), así que no hay `git diff` real
contra el que comparar; leí el contenido completo íntegro. Verifiqué algebraicamente que `||` y `!=`
solo difieren en `(true, true)`, confirmando el comentario de
`con_las_dos_pistas_vivas_la_sesion_sigue`. Tracé
`una_sesion_de_una_sola_pista_termina_si_esa_pista_muere` paso a paso: `pistas_a_grabar(true, false)`
→ `[Mic]` → `.contains(&Track::System)` → `false`, igual que el literal reemplazado, y confirmé que
sí detectaría una regresión donde `pistas_a_grabar` incluyera `System` por error. Medí longitud de
línea en caracteres Unicode (no bytes) con un script propio: máximo 99, ninguna de las dos pruebas
cerca del límite. Encontré por `tablero.md` (sección "Corte por límite de cuota") que estas mismas
cuatro combinaciones ya se le habían encargado antes a un `verificador-pruebas`/`revisor-codigo` de
la 3.ª vuelta de HU-01, que murieron por `rate_limit` sin entregar nada — no hay revisión duplicada
en curso, esta es la primera que llega a entregarse. No revisé el resto de `sincronia.rs`
(`muestras_de_relleno`, `normalizar_a_f32`) ni `wasapi_src.rs`: no es mi encargo y ya tuvieron su
propia cadena de revisión formal con hallazgos Bloqueantes/Importantes propios
(`HU-01-wasapi/REVIEW.md`).

## 5. Qué no pude verificar y qué haría falta

- Que `sincronia.rs` compile, que sus pruebas pasen, `cargo fmt`/`clippy -D warnings`: requieren
  `cargo` (B-1).
- Que `ventana.dart`/`ajustes.dart`/`grabacion.dart`/`region.dart` analicen limpio en
  `flutter analyze`, y cualquier `flutter test`: requieren `flutter pub get`, que falla por versión
  de Dart (B-2).
- Comportamiento real en un dispositivo Android u otra sesión gráfica Linux sin gestor de ventanas
  (el caso que justifica `soportado` en `region.dart`): no tengo ese entorno.
- El resto de `wasapi_src.rs` (manejo de errores COM, el hallazgo Bloqueante de la ronda anterior de
  HU-01-wasapi sobre pérdida de sesión) — fuera de mi encargo, tiene su propia revisión en curso.
- Contenido de `core/providers` para las líneas del README sobre claves de API — explícitamente
  fuera de alcance.

Haría falta: instalar Rust 1.75+ para B-1, y Flutter con Dart ≥3.12.2 (o bajar el `sdk:` de
`pubspec.yaml`) para B-2; correr entonces el checklist completo de cierre. Para H1 en particular,
haría falta coordinar con el cierre de HU-01-wasapi antes de tocar la frase de nuevo, en vez de
corregirla ahora sin esa información.
