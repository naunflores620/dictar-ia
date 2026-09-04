# PLAN — T-16 «HU-02 · Grabar en Android»

HU de origen: [`docs/06-historias-de-usuario.md#hu-02--grabar-en-android`](../../../docs/06-historias-de-usuario.md)

## Análisis

Hoy `dictar_audio::iniciar` en Android cae en la rama `#[cfg(not(any(linux, windows)))]` y
devuelve `NoSoportada`. Falta el backend entero, el servicio en primer plano y la petición de
permisos.

**No se persigue el loopback.** No existe forma de capturar el audio de una videollamada ajena
en Android, y el caso de uso de esta HU es la reunión presencial. `capturar_sistema` no falla
si llega en `true`: se registra y se ignora, porque fallar dejaría sin grabar a quien reutiliza
la misma `CaptureConfig` del escritorio.

**AAudio por FFI directo, no `oboe`.** AAudio es C plano en `libaaudio.so` desde API 26, y la
superficie que hace falta son doce funciones. El crate `oboe` traería un build de C++ al cruce
con el NDK para envolver lo mismo. Es el mismo criterio con el que `wasapi_src` usa constantes
crudas en vez de arrastrar dependencias por comodidad.

**Lo que NO se reimplementa.** `sincronia.rs` nació como «lógica pura de la captura WASAPI»,
pero `normalizar_a_f32`, `muestras_de_relleno` y `pistas_a_grabar` no tienen nada de WASAPI:
son el formato nativo, el reloj y la política. Android las reutiliza tal cual. Ese es el cobro
de la separación que hizo HU-01, y la única edición que recibe ese archivo es la de su
comentario de cabecera, que hoy miente por omisión.

Queda deliberadamente fuera: la reproducción en Android (es HU-07 y solo nombra Windows), la
enumeración real de dispositivos (AAudio no la tiene; es `AudioManager`, en Java) y el cambio
de dispositivo a mitad de sesión (T-9, fuera de alcance en todas las plataformas).

## Archivos y paquetes afectados

Lista cerrada.

| Ruta | Cambio esperado |
|---|---|
| `core/audio-capture/src/aaudio_src.rs` | **Nuevo.** FFI a AAudio, hilo de lectura y sesión |
| `core/audio-capture/src/lib.rs` | Declarar el módulo y añadir la rama Android a `iniciar` y `dispositivos` |
| `core/audio-capture/src/sincronia.rs` | **Solo el comentario de cabecera**: deja de ser «de WASAPI». Ni una línea de lógica |
| `core/audio-capture/Cargo.toml` | Sin dependencias nuevas: el `extern "C"` no las necesita |
| `app/android/app/src/main/kotlin/.../ServicioGrabacion.kt` | **Nuevo.** Servicio en primer plano con notificación |
| `app/android/app/src/main/kotlin/.../MainActivity.kt` | Los dos `MethodChannel`: permisos y servicio |
| `app/android/app/src/main/AndroidManifest.xml` | Declarar el `<service>` con `foregroundServiceType` |
| `app/lib/plataforma/android.dart` | **Nuevo.** Envoltorio Dart de los dos canales |
| `app/lib/pantallas/grabacion.dart` | Pedir permiso antes de grabar; arrancar y parar el servicio |

## Dependencias

- HU-10 (que el APK lleve el núcleo dentro) — la parte de Gradle ya está hecha y sin verificar.
- Herramientas: NDK **presente** (28.2.13676358). Falta `cargo-ndk` y el objetivo
  `aarch64-linux-android`, que dependen de que B-1 se resuelva en esta misma sesión.

## Casos del dominio que hay que cubrir

- [x] **Audio que no viene a 16 kHz** — AAudio entrega lo que el dispositivo quiera (44,1 o 48
      kHz es lo normal). Se consulta el formato **real** tras abrir el flujo, no el pedido, y se
      remuestrea en el origen con `mezcla::Remuestreador`
- [x] **Cola del remuestreador** — se vacía al cerrar. Es T-8 en `pipewire_src`, deuda abierta;
      aquí no se introduce
- [x] **Pista de sistema ausente** — es el caso **normal** en Android, no el excepcional
- [ ] Pistas de distinta longitud — solo hay una pista
- [x] **Sesión interrumpida** — si Android mata el proceso, el WAV ya escrito sirve
- [x] **Plataforma sin backend** — la rama `NoSoportada` sigue existiendo para lo que no sea
      Linux, Windows ni Android
- [x] **Clase de dos horas** — criterio 5. El servicio en primer plano es justo lo que lo
      sostiene; sin él Android corta a los minutos
- [ ] Base de datos de versión anterior — no se toca el esquema
- [ ] Sin clave de API y sin conexión — no se toca la capa de proveedores

## Riesgos

| Riesgo | Mitigación |
|---|---|
| AAudio no concede 16 kHz mono `f32` y devuelve otra cosa en silencio | Se leen `getSampleRate`, `getChannelCount` y `getFormat` **después** de abrir y se adapta. Nunca se asume lo pedido |
| `AAudioStream_read` bloqueante deja el hilo colgado al detener | `timeoutNanos` acotado, y el bucle comprueba la señal de parada en cada vuelta |
| El servicio arranca sin permiso de notificación (Android 13+) y el sistema lo mata | `POST_NOTIFICATIONS` se pide junto a `RECORD_AUDIO`, antes de grabar |
| Nada de esto se puede ejecutar sin un móvil | Declarado. La lógica probable va a `sincronia.rs`, que sí corre en el CI |

## Estrategia

1. `aaudio_src.rs`: FFI, apertura, hilo de lectura, `CaptureSession`. Reutiliza `sincronia`.
2. `lib.rs`: módulo y ramas de despacho. `sincronia.rs`: solo la cabecera.
3. Kotlin: servicio en primer plano, canales y manifiesto.
4. Dart: envoltorio de los canales y el enganche en `_iniciar` / `_detener`.
5. Pruebas de la lógica pura, y `cargo ndk build` como compuerta real.

## Pruebas necesarias

| Prueba | Cubre | Mutación esperada que la pone en rojo |
|---|---|---|
| `una_sesion_de_android_graba_solo_el_microfono` | AC 1 | Que `pistas_para_android` incluya `Track::System` |
| `pedir_capturar_el_sistema_en_android_no_impide_grabar` | AC 1 | Que devuelva `Err` en vez de seguir con el micrófono |
| `el_formato_real_manda_sobre_el_pedido` | Riesgo 1 | Fijar 16 kHz en vez de leer el del flujo |
| `un_flujo_a_44100_se_remuestrea_a_16000` | Dominio | Quitar el remuestreo |

## Agentes a lanzar

`contradictor` antes de implementar; `revisor-codigo`, `verificador-pruebas` y
`auditor-plataforma` después. **No se han lanzado**: el PO delegó esta HU directamente en esta
sesión. Queda declarado como deuda de revisión, igual que T-15.
