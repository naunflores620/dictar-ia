# dictar_ia

Grabación de clases online y reuniones, con transcripción y **apuntes generados por IA** al
estilo de Notion AI. La única plataforma donde la grabación está **comprobada contra clases
reales** es **Linux**. Windows (WASAPI *loopback*) y Android (AAudio, solo micrófono) ya tienen
su backend de captura escrito, pero **ninguno de los dos se ha ejecutado todavía sobre un equipo
de verdad**. La lógica que no depende del hardware —formatos, relojes, qué pistas se
graban— sí tiene pruebas, y el CI las ejecuta; el código que habla con el sistema de audio, no.
Hasta que alguien grabe una clase con ellos, dan por buenos supuestos que no lo son.

El motor de IA es **intercambiable**: Gemini, DeepSeek, OpenAI o modelos locales (Ollama /
llama.cpp), sin tocar código — solo configuración.

---

## Para qué es

| Uso | Frecuencia | Cómo se captura | Salida |
|---|---|---|---|
| **Clases universitarias online** | ~10 h/semana | Loopback de Meet / Zoom / Teams | Apuntes |
| **Reuniones con clientes** | Puntual | Loopback de Meet / Zoom | Acta comercial |
| Reuniones presenciales | Ocasional | Grabas con el móvil e importas el archivo (por CLI: el botón de la interfaz aún no está cableado) | Acta simple |

Llamadas telefónicas, no: es imposible en Android e iOS a nivel de sistema operativo, y no hace
falta para este caso de uso.

---

## Documentación de diseño

| Documento | Contenido |
|---|---|
| [docs/01-arquitectura.md](docs/01-arquitectura.md) | Captura, pipeline, captura de diapositivas, modelo de datos |
| [docs/02-stack-tecnologico.md](docs/02-stack-tecnologico.md) | Tecnologías elegidas, alternativas y por qué |
| [docs/03-proveedores-ia.md](docs/03-proveedores-ia.md) | Capa multi-proveedor, plantillas de notas, prompts, costes |
| [docs/04-roadmap.md](docs/04-roadmap.md) | Fases, riesgos y criterios de aceptación |
| [docs/05-empaquetado.md](docs/05-empaquetado.md) | Cómo generar el `.exe` de Windows y el `.deb` de Linux |

---

## Las decisiones que definen el proyecto

**1. Dos pistas de audio, nunca una mezcla.**
Se graba el micrófono y el loopback del sistema por separado. De ahí salen gratis tres cosas:
diarización exacta (tú vs. profesor/cliente) sin ningún modelo, mejor transcripción al no haber
voces solapadas, y control independiente de cada pista.

**2. Captura automática de diapositivas.**
Muestreo de pantalla + hash perceptual: cuando cambia la lámina, se guarda una captura anclada
al segundo exacto y se le pasa OCR. Los apuntes salen con las diapositivas intercaladas en su
sitio. Es la diferencia entre una transcripción y unos apuntes de verdad.

**3. Whisper local por defecto, no cloud.**
150 h por semestre cuestan ~$40 en transcripción cloud y **$0** en local. Al proveedor de IA
solo va texto ya transcrito, que cuesta ~$2,60 el semestre entero — menos que un café. Y el
audio nunca sale del equipo.

**4. Doble pasada de transcripción.**
Modelo pequeño en vivo durante la clase (desechable, para poder releer lo que se dijo hace diez
minutos) y `large-v3-turbo` al terminar, con glosario y contexto completo (el bueno). Es lo que
hace que la experiencia de Notion se sienta buena.

**5. Glosario que aprende por asignatura.**
El OCR de las diapositivas y los términos recurrentes de clases anteriores alimentan el
`initial_prompt` de Whisper. Cada clase mejora la transcripción de la siguiente; a mitad de
semestre el sistema ya domina el vocabulario del curso.

**6. Dos plantillas de notas, no una.**
Unos apuntes de clase (conceptos, fórmulas, avisos, **"esto entra en el examen"**) y un acta
comercial (necesidades, objeciones, compromisos, próximo paso) no se parecen en nada.

**7. Todo con `ts_ms`.**
Cada afirmación generada por la IA es clicable y lleva al segundo exacto del audio y a la
diapositiva que estaba en pantalla. Sin esto no puedes confiar en unos apuntes automáticos.

**8. Sin backend, sin cuentas, sin coste mensual.**
Todo vive en SQLite local, sin servidor y sin login: la simplificación más valiosa del diseño.
Lo que hoy está pendiente de terminar, sin rodeos: el cifrado en reposo (`SQLCipher`) existe
como feature `cifrado`, pero apagada por defecto — vendoriza OpenSSL y multiplica el tiempo de
compilación, lo que estorba mientras se itera sobre el esquema, así que la base de datos hoy no
está cifrada. Las claves de API se resuelven en cadena: variable de entorno, luego archivo
`.env`; el llavero del SO es el tercer eslabón previsto y todavía no está implementado (pendiente
de `libsecret`), así que hoy las claves quedan en texto plano en el `.env`.

---

## Stack

| Capa | Elección |
|---|---|
| Núcleo | **Rust** — audio (hoy solo PipeWire/Linux), VAD, STT, captura de pantalla, proveedores, almacenamiento |
| UI | **Flutter** + `flutter_rust_bridge` — una sola interfaz para Windows, Linux y Android |
| Transcripción | `whisper.cpp` / `large-v3-turbo`, con Vulkan si hay GPU |
| Datos | SQLite + FTS5. `SQLCipher` existe como feature `cifrado`, apagada por defecto; `sqlite-vec` no se usa todavía |
| IA | Adaptador compatible-OpenAI (cubre DeepSeek, OpenAI, Ollama…) + adaptador Gemini nativo |
| Empaquetado | `dpkg-deb` (`.deb`), Inno Setup (`.exe`) y `flutter build apk`, vía GitHub Actions. Los tres llevan el núcleo dentro y el CI falla si no; el `.exe` y el APK todavía no graban |

---

## Estado

**El flujo completo funciona**: graba las dos pistas, transcribe con Whisper en local y genera
los apuntes. Verificado de extremo a extremo con audio real.

| Componente | Estado | Tests |
|---|---|---|
| [core/domain](core/domain/) — tipos y esquemas JSON derivados | ✅ | 14 |
| [core/storage](core/storage/) — SQLite, FTS5, glosario acumulativo | ✅ | 28 |
| [core/providers](core/providers/) — Gemini, OpenAI-compat, `.env`, enrutado | ✅ | 82 |
| [core/notes](core/notes/) — map-reduce, prompts, Markdown | ✅ | 33 |
| [core/audio-capture](core/audio-capture/) — PipeWire, WASAPI y AAudio | ✅ | 55 |
| [core/screen-capture](core/screen-capture/) — muestreo de pantalla, hash perceptual | ✅ | 24 |
| [core/vad](core/vad/) — detección de voz y troceado | ✅ | 21 |
| [core/stt](core/stt/) — whisper.cpp, filtros de alucinación | ✅ | 40 |
| [core/api](core/api/) — fachada del núcleo | ✅ | 36 |
| [cli/](cli/) — `dictar` | ✅ | 16 |
| [app/](app/) — interfaz Flutter | ✅ grabar, procesar y leer apuntes; importar y buscar sin cablear | 24 |
| Puente `flutter_rust_bridge` | ✅ 33 funciones | — |

**373 tests** — 349 en Rust y 24 en Dart. Los de Flutter **están en verde**
(`flutter analyze` limpio, `flutter test` en verde, comprobado el 04/09). Los de Rust se cuentan,
no se han ejecutado en esta máquina: `cargo` no estuvo instalado durante toda la fase en la que
se escribieron WASAPI y el llavero, así que **la compuerta que vale es el CI**, no una
afirmación de este archivo.

### Medido en un Core Ultra (Lunar Lake), 8 núcleos

| | |
|---|---|
| Transcripción `large-v3-turbo` Q5 | **4,6× tiempo real** |
| Diez horas de clase | **2,2 h** de procesado nocturno |
| Coste de los apuntes por sesión | **$0.0016 – $0.0024** con DeepSeek |
| Vulkan sobre la Arc 140V | **sin diferencia** — ver [01 §8](docs/01-arquitectura.md) |

## Uso

```bash
# Requisitos del sistema (único paso con sudo) — ver INSTALL.md
sudo apt install -y cmake ninja-build clang pkg-config libgtk-3-dev \
  liblzma-dev libpipewire-0.3-dev libasound2-dev libsecret-1-dev libdbus-1-dev

# Claves de API
cp .env.example .env      # y rellena GEMINI_API_KEY o DEEPSEEK_API_KEY
cargo run --release -p dictar-cli -- proveedores   # dice de dónde coge cada una

# Modelo de Whisper (~550 MB, una sola vez)
mkdir -p ~/.local/share/dictar_ia/models && cd $_
curl -fLO https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin
```

**El flujo completo, en un comando.** Graba, transcribe y resume:

```bash
dictar sesion --tipo clase --contexto "Cálculo II — Dra. Pérez"
# Graba hasta que pulses ENTER, y luego procesa.
```

Otras órdenes:

| Orden | Para qué |
|---|---|
| `dictar sesion` | El producto entero: grabar → transcribir → apuntes |
| `dictar grabar` | Solo capturar, con medidor de nivel por pista |
| `dictar transcribir --in a.wav` | Solo Whisper, con medición de velocidad |
| `dictar notas --in clase.txt` | Solo la IA, desde una transcripción en texto |
| `dictar proveedores` | Qué claves hay y de qué archivo salen |

Sin ninguna clave usa Ollama en local, así que funciona sin conexión y sin coste.

## Interfaz gráfica

```bash
cd app && flutter run -d linux
```

CMake compila el núcleo Rust y copia `libdictar_api.so` al bundle en cada build, así que la
interfaz nunca se queda mirando una versión vieja de la librería.

Para probar contra otro almacén sin tocar los datos de uso diario:

```bash
DICTAR_DATOS=/tmp/pruebas flutter run -d linux
```

Si el núcleo no carga por lo que sea, la aplicación arranca igualmente con datos de
demostración en lugar de quedarse en blanco.

## Siguiente paso

La captura de diapositivas por cambio de pantalla y la transcripción en vivo durante la clase ya
están hechas, no son "siguiente paso". Lo que sí queda:

- **Conectar dos botones de la interfaz al núcleo que ya existe.** En
  `app/lib/pantallas/inicio.dart`, "Importar audio de una reunión presencial" y el buscador son
  maquetas: los dos llaman a `_avisar()`, que solo muestra un aviso de "pendiente de conectar con
  el núcleo". El backend de ambos ya está — `leer_wav` para importar, `db.buscar()` con FTS5 para
  buscar —; falta cablear el botón a la función.
- **Grabar una vez en Windows y una vez en un teléfono.** Los dos backends están escritos
  (`wasapi_src.rs` y `aaudio_src.rs`) y ninguno se ha ejecutado nunca contra un dispositivo. No
  es trabajo de programación: es la comprobación que convierte «debería funcionar» en «funciona»,
  y la que va a encontrar lo que las pruebas sin hardware no pueden encontrar.
- **Revisar HU-02.** El backend de Android se implementó sin pasar por el `contradictor` ni por
  los tres revisores que exige `_orquestacion/protocolo.md`. Está declarado en su `PLAN.md`, no
  escondido, pero es deuda de revisión abierta.

**Y lo más importante: empieza a grabar tus clases de verdad.** El riesgo que queda no es
técnico sino de calidad —que los apuntes sean realmente buenos y no genéricos—, y eso solo se
afina iterando contra clases reales. La orden `dictar sesion` ya sirve para eso hoy.
