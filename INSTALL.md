# Preparación del entorno

Detectado en tu equipo: **Ubuntu 26.04**, PipeWire 1.6.2, Intel Arc 140V (Lunar Lake),
8 núcleos, 30 GB RAM. Rust y Flutter se instalan en `$HOME` sin sudo.

## 1. Paquetes del sistema — el único paso que necesita sudo

```bash
sudo apt update && sudo apt install -y \
  cmake ninja-build clang pkg-config \
  libgtk-3-dev liblzma-dev libstdc++-15-dev \
  libpipewire-0.3-dev libasound2-dev \
  libsecret-1-dev libsqlite3-dev \
  libvulkan-dev glslc spirv-tools
```

Para qué es cada bloque:

| Paquetes | Para qué |
|---|---|
| `cmake ninja-build clang pkg-config` | Compilar `whisper.cpp` y el escritorio de Flutter |
| `libgtk-3-dev liblzma-dev` | Flutter en Linux |
| `libpipewire-0.3-dev libasound2-dev` | Captura de audio: loopback del sistema y micrófono |
| `libsecret-1-dev` | Llavero del SO para las claves de API |
| `libsqlite3-dev` | Base de datos local |
| `libvulkan-dev glslc spirv-tools` | Aceleración de Whisper en tu Intel Arc 140V |

> No hace falta `libssl-dev`: el proyecto usa **rustls**, así que no depende de OpenSSL.

## 2. Rust

Se instala solo, sin sudo (ya lanzado):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
```

## 3. Flutter

```bash
cd ~
curl -LO https://storage.googleapis.com/flutter_infra_release/releases/stable/linux/flutter_linux_stable.tar.xz
tar xf flutter_linux_stable.tar.xz
echo 'export PATH="$HOME/flutter/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc
flutter config --enable-linux-desktop --enable-android
flutter doctor
```

## 4. Android (solo si vas a compilar el APK)

Dos trampas que cuestan una tarde si no se saben:

**Ubuntu trae un JRE, no un JDK.** `java -version` responde, pero no hay `javac`, y Gradle
falla con un críptico `Toolchain ... does not provide the required capabilities:
[JAVA_COMPILER]`. Hace falta un JDK de verdad, y **17**, no el 25 del sistema.

```bash
mkdir -p ~/jdks && cd ~/jdks
curl -fLO --output-dir . "https://api.adoptium.net/v3/binary/latest/17/ga/linux/x64/jdk/hotspot/normal/eclipse"
tar xzf eclipse && rm eclipse
export JAVA_HOME=~/jdks/jdk-17.0.20+8      # ajusta al directorio que salga
```

**El SDK no necesita Android Studio.** Bastan las herramientas de línea de órdenes:

```bash
mkdir -p ~/Android/Sdk/cmdline-tools && cd ~/Android/Sdk/cmdline-tools
curl -fLO https://dl.google.com/android/repository/commandlinetools-linux-13114758_latest.zip
unzip -q commandlinetools-linux-*.zip && rm commandlinetools-linux-*.zip
mv cmdline-tools latest                     # sdkmanager exige esta estructura

export ANDROID_HOME=~/Android/Sdk
export PATH="$ANDROID_HOME/cmdline-tools/latest/bin:$PATH"
yes | sdkmanager --licenses
sdkmanager "platform-tools" "platforms;android-36" "build-tools;36.0.0"

flutter config --android-sdk "$ANDROID_HOME"
```

Compilar. `--target-platform android-arm64` cubre cualquier móvil actual y evita empaquetar
tres arquitecturas:

```bash
cd app && flutter build apk --release --target-platform android-arm64
```

Para dejarlo permanente, añade a `~/.bashrc`:

```bash
export JAVA_HOME=$HOME/jdks/jdk-17.0.20+8
export ANDROID_HOME=$HOME/Android/Sdk
export PATH="$JAVA_HOME/bin:$HOME/flutter/bin:$ANDROID_HOME/platform-tools:$PATH"
```

## 5. Verificar

```bash
rustc --version && flutter --version && pkg-config --modversion libpipewire-0.3
javac -version && sdkmanager --version     # si vas a compilar para Android
```

---

## Sobre tu hardware

**Intel Arc 140V con Vulkan** es una buena noticia para este proyecto: `whisper.cpp` tiene
backend Vulkan y funciona bien con las Arc integradas de Lunar Lake. Es notablemente más rápido
que CPU y no requiere instalar ningún runtime propietario, a diferencia de CUDA.

La fase 0 medirá la velocidad real de `large-v3-turbo` en tu equipo. Con 8 núcleos y esa GPU la
expectativa razonable es procesar 10 h de clase en bastante menos de una hora.
</content>
</invoke>
