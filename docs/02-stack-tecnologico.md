# 02 — Stack tecnológico

**Plataformas objetivo:** Windows, Linux y Android.
iOS queda fuera de alcance; el núcleo Rust y Flutter lo soportan si algún día hace falta.

---

## 1. La decisión estructural: núcleo en Rust

El pipeline —captura → VAD → segmentación → STT → notas → almacenamiento— se escribe una vez,
en un lenguaje nativo, y las tres plataformas lo consumen. Es lo que evita que el proyecto se
convierta en tres proyectos.

**Núcleo: Rust.** Razones concretas:

- Acceso directo a las APIs de audio de cada SO (WASAPI, PipeWire, AAudio) con bindings maduros
  y sin capa intermedia.
- Sin recolector de basura → sin pausas en el hilo de audio, que es *tiempo real blando*. Un GC
  pausando 50 ms en el hilo de captura produce cortes audibles en una grabación de dos horas.
- Todo lo necesario tiene bindings de primera calidad, y varios son proyectos Rust nativos:
  `whisper-rs`, `ort` (ONNX), `df` (DeepFilterNet), `rusqlite`, `xcap`.
- `tokio` para orquestar transcripción, captura de pantalla y llamadas a proveedores sin
  bloquear la UI.

Descartados para el núcleo: **C++** (mismo poder, mucho más coste de build multiplataforma),
**Go** (GC en el hilo de audio, `gomobile` mediocre), **Python** (empaquetar un runtime completo
para el usuario final es un problema recurrente).

---

## 2. Capa de UI: **Flutter** + `flutter_rust_bridge`

**Decisión tomada, y la razón es el móvil.** Si Android se va a construir con Flutter, usar
Flutter también en escritorio significa **una sola interfaz para Windows, Linux y Android**.
La alternativa —React para escritorio, Flutter para móvil— serían dos UIs de la misma
aplicación: dos veces cada pantalla, cada corrección y cada rediseño. Es el error que hace que
un proyecto de una persona se atasque a los seis meses.

Esto contradice la recomendación anterior de este documento (Tauri v2), y conviene decir por
qué cambió: sin móvil, la UI documental favorecía a la tecnología web. Con Flutter ya decidido
en móvil, un único código de UI pesa más que cualquier otra ventaja.

| Criterio | Valoración |
|---|---|
| Plataformas desde un código | Windows, Linux, Android ✅ |
| Puente a Rust | `flutter_rust_bridge` v2: genera el FFI, soporta `async` y `Stream` |
| Renderizado | Motor propio (Impeller/Skia): idéntico en Windows y Linux, sin depender del WebView del sistema |
| Listas largas | `ListView.builder` virtualiza de serie — importa: una clase de 2 h son miles de líneas |
| Audio y segundo plano en Android | Ecosistema maduro (`record`, `audio_session`, foreground services) |
| Empaquetado | Su punto flojo. Ver [05-empaquetado.md](05-empaquetado.md) |

El puente encaja bien: la transcripción en vivo es un `Stream<Utterance>` de Rust consumido
directamente por un `StreamBuilder`, sin polling ni IPC manual.

**Dos puntos flojos conocidos y su solución:**

1. **Fórmulas matemáticas.** `flutter_math_fork` cubre la mayor parte de LaTeX, pero no todo. Si
   una fórmula de los apuntes no renderiza, la salida alternativa es incrustar un `WebView`
   mínimo con KaTeX **solo para ese bloque**, no para la app entera.
2. **Empaquetado.** Flutter no genera instaladores; hay que montarlo. Es trabajo de una vez, y
   está resuelto en [05-empaquetado.md](05-empaquetado.md).

**Descartados:** Tauri v2 (empaqueta mejor de fábrica, pero obligaría a una segunda UI para
Android) y Electron (~150 MB por instalador, alto consumo de memoria en grabaciones largas).

---

## 3. Transcripción (STT)

### Local — es el modo por defecto, no una opción

Con 150 h por semestre, el cloud costaría ~$40 y el local $0. **`whisper.cpp`** vía `whisper-rs`:

| Uso | Modelo | Aceleración |
|---|---|---|
| Pasada en vivo (escritorio) | `small` Q5 | CPU — debe dejar el equipo usable durante la clase |
| Pasada final, GPU NVIDIA | `large-v3-turbo` Q5_K | CUDA |
| **Pasada final, este equipo** | `large-v3-turbo` Q5 | **CPU — 4,6× tiempo real** |
| Pasada final, equipo modesto | `medium` | AVX2/AVX512 |
| Android | No transcribe: graba y transfiere al PC | — |

`large-v3-turbo` es el punto dulce: precisión cercana a `large-v3` a ~6× su velocidad y buen
rendimiento en español. Se descargan bajo demanda, **no se empaquetan en el instalador** — el
modelo pesa más que la aplicación entera.

Dos requisitos duros del motor STT, por los que `whisper.cpp` encaja:

1. **`initial_prompt`** — es por donde entra el glosario de la asignatura. Sin esto, el diseño de
   vocabulario acumulado no funciona.
2. **Ejecución por segmentos con contexto** — necesaria para la pasada en vivo con
   LocalAgreement-2.

Alternativa: **`faster-whisper`** (CTranslate2) como proceso auxiliar. Más rápido en CPU y con
mejores `word_timestamps`, a cambio de arrastrar un runtime de Python al instalador — lo que
complica bastante el empaquetado. Solo si las marcas de tiempo por palabra resultan críticas.

### Cloud — puntual

Para cuando una grabación concreta salió mal y quieres una segunda opinión. No es el modo
habitual.

| Proveedor | Fuerte en | Precio orientativo |
|---|---|---|
| Deepgram Nova | Streaming de baja latencia, diarización incluida | ~$0.26/h |
| AssemblyAI | Diarización y post-proceso | ~$0.37/h |
| Gemini | Acepta audio directo; transcribe y resume en una llamada | Competitivo |

Los precios cambian: viven en `config/providers.toml`, no en el código.

---

## 4. Componentes de apoyo

| Necesidad | Elección | Por qué |
|---|---|---|
| Captura de audio | `cpal` + `wasapi` / `pipewire-rs` | `cpal` para lo común; API nativa para el loopback fino |
| Captura de pantalla | `xcap` | Multiplataforma, captura por ventana concreta |
| Hash perceptual | `image` + `img_hash` (dHash) | Detección de cambio de diapositiva |
| OCR de diapositivas | `tesseract` (local) o Gemini Vision | Tesseract basta para texto; las fórmulas necesitan modelo visual |
| VAD | Detector propio: energía + mínimo deslizante | Ver nota abajo |
| Denoise (solo presencial) | DeepFilterNet 3 (`df`) | Nativo Rust, ruido + dereverberación. Apagado por defecto |
| Filtro paso-alto / AGC | `biquad` + normalización propia | Retumbe de climatización, distancia al micro |
| Base de datos | SQLite + `rusqlite` | Sin servidor, un archivo, transaccional |
| Cifrado en reposo | SQLCipher | Integración directa con SQLite |
| Búsqueda vectorial | `sqlite-vec` | Evita añadir una BD vectorial completa |
| Embeddings | `multilingual-e5-small` (ONNX, 384 dim) | Ligero y bueno en español |
| Audio comprimido | Opus vía `libopus` | 11 MB/hora en 2 pistas |
| Resample | `rubato` | Alta calidad, sin dependencias externas |
| Fórmulas en la UI | `flutter_math_fork` | LaTeX en Flutter |
| HTTP | `reqwest` + `tokio` | SSE para streaming de proveedores |
| Reintentos | `backoff` | Retroceso exponencial con *jitter* |
| Llavero | `keyring-rs` (escritorio) + Keystore vía JNI (Android) | Almacenamiento seguro por SO |
| Logs | `tracing` | Trazado estructurado, spans por sesión |

### Sobre el detector de voz: no se usa Silero

El diseño original preveía Silero VAD por ONNX. Al implementarlo se vio que no compensa para
este caso: el audio principal es el loopback digital de Meet o Zoom, con una relación
señal-ruido altísima, y ahí un detector por energía con estimación del fondo por **mínimo sobre
ventana deslizante** discrimina perfectamente. Arrastrar ONNX Runtime al instalador —decenas de
megas y una dependencia nativa más— para eso no se justifica.

El detalle que hace que funcione es usar el mínimo y no una media móvil: una media sube mientras
alguien habla, así que con un profesor que encadena treinta segundos sin pausa acabaría tomando
su voz por ruido de fondo. El mínimo no, porque el habla siempre baja de nivel entre sílabas.

Para audio de sala ruidoso —reuniones presenciales, que son minoría— Silero sí compensaría, y
encaja detrás del mismo interfaz sin tocar nada más.

---

## 5. Empaquetado

Punto débil de Flutter y el que más trabajo manual exige: `.exe` para Windows y `.deb` para
Linux. Recetas concretas, dependencias y el manejo de la librería Rust en
**[05-empaquetado.md](05-empaquetado.md)**.

Resumen: `flutter_distributor` orquesta **Inno Setup** en Windows y genera `.deb`, `.rpm` y
`.AppImage` en Linux desde un único `distribute_options.yaml`.

---

## 6. Lo que NO se construye

No construir es tan importante como construir. Recortes explícitos:

- **iOS** — requiere Mac para compilar y 99 $/año. Fuera hasta que haya un motivo real.
- **Backend** — no hay servidor, no hay coste mensual, no hay que mantener nada. Los datos son
  archivos locales; el móvil transfiere por LAN. Es la simplificación más valiosa del diseño.
- **Cuentas de usuario** — sin servidor no hay usuarios.
- **Transcripción en vivo en móvil** — 30–50 % de batería por hora. El móvil graba, el PC
  transcribe.
- **Diarización avanzada** — las dos pistas separadas la hacen innecesaria en sesiones online.
- **Grabación de llamadas telefónicas** — imposible sin root, e irrelevante aquí.

La app Android **sí** se construye, pero en la fase 4: cubre el 5 % de las sesiones y no debe
adelantar al escritorio. Ver [04-roadmap.md](04-roadmap.md).

---

## 7. Estructura de repositorio propuesta

```
dictar_ia/
├── core/                        # Workspace Rust — el corazón del proyecto
│   ├── audio-capture/           # trait AudioSource + WASAPI / PipeWire / File / Android
│   ├── screen-capture/          # xcap + dHash + detección de diapositiva
│   ├── preprocess/              # HPF, DeepFilterNet, AGC (solo presencial)
│   ├── vad/                     # Silero
│   ├── segmenter/               # troceado con solape
│   ├── stt/                     # trait SttEngine + whisper-rs + LocalAgreement-2
│   ├── ocr/                     # tesseract o proveedor visual
│   ├── providers/               # trait LlmProvider: openai-compat, gemini, local
│   ├── notes/                   # plantillas + map-reduce + validación de esquema
│   ├── glossary/                # términos recurrentes por topic
│   ├── sync/                    # mDNS + transferencia HTTP en LAN (fase 4)
│   ├── storage/                 # SQLite, migraciones, sqlite-vec
│   ├── crypto/                  # SQLCipher, llavero
│   └── api/                     # fachada única que consume la UI  ← cdylib
├── app/                         # Flutter — una sola UI para las 3 plataformas
│   ├── lib/
│   ├── windows/  linux/  android/
│   └── rust_bridge/             # generado por flutter_rust_bridge
├── packaging/
│   ├── windows/inno_setup.iss
│   ├── linux/debian/control
│   └── distribute_options.yaml
├── models/                      # descarga bajo demanda, no versionado
├── config/
│   ├── providers.toml
│   └── prompts/
└── docs/
```

`core/api` es la pieza que protege la inversión: la UI no conoce los submódulos, solo una
fachada (`start_session`, `stop_session`, `subscribe_transcript`, `import_audio`, `get_notes`,
`ask_topic`). Se compila como **cdylib** (`.dll` en Windows, `.so` en Linux y Android) y es lo
que consume `flutter_rust_bridge`.

---

## 8. Riesgos técnicos y mitigación

| Riesgo | Impacto | Mitigación |
|---|---|---|
| **Loopback en Linux con Wayland / Flatpak** | Alto | PipeWire nativo + portal; probar en tu distro concreta en fase 0 |
| **El `.deb` no arranca en otra máquina** por dependencias no declaradas | Alto | Probar en un contenedor Debian limpio, no solo en tu equipo. Ver [05](05-empaquetado.md) |
| La `.so` de Rust no se encuentra en tiempo de ejecución | Medio | `rpath` correcto en el bundle Linux; `.dll` junto al `.exe` en Windows |
| Detección de diapositiva con falsos positivos (webcam, cursor) | Medio | Región central estable + umbral de área + confirmación en 2 capturas |
| STT en vivo deja el equipo inutilizable durante la clase | Medio | Interruptor para desactivarlo; los apuntes finales no dependen de él |
| Whisper lento sin GPU | Medio | Procesado nocturno por lotes; detección de capacidad al instalar |
| Alucinaciones de Whisper en silencios (profesor leyendo) | Medio | El VAD elimina los silencios antes de transcribir — es la causa principal |
| `flutter_math_fork` no renderiza alguna fórmula | Bajo | WebView con KaTeX solo para ese bloque |
| Audio de reunión presencial inservible | Bajo (caso minoritario) | DeepFilterNet + recomendación de colocación del micro |
</content>
</invoke>
