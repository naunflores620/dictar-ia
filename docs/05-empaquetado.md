# 05 — Empaquetado y distribución

Cómo pasar de `flutter build` a un **`.exe` instalable en Windows** y un **`.deb` en Linux**.

Es el punto flojo de Flutter: no genera instaladores. Es trabajo de una vez, y una vez montado
en CI no se vuelve a tocar.

---

## 1. Lo que Flutter produce por sí solo

```bash
flutter build windows --release
# → build\windows\x64\runner\Release\
#     dictar_ia.exe          ← NO es distribuible por sí solo
#     flutter_windows.dll
#     msvcp140.dll, vcruntime140.dll, vcruntime140_1.dll
#     data\...

flutter build linux --release
# → build/linux/x64/release/bundle/
#     dictar_ia              ← binario
#     lib/                   ← librerías, con RPATH=$ORIGIN/lib
#     data/                  ← assets y ICU
```

**Punto clave:** el `.exe` de esa carpeta **no es el instalador**. Es el ejecutable, y depende
de las DLL y de `data\` que están a su lado. Lo que distribuyes es un segundo `.exe`: el
instalador, que copia todo eso a `Archivos de programa` y crea los accesos directos.

Detalle a favor: Flutter ya copia `msvcp140.dll` y `vcruntime140*.dll` en la carpeta Release,
así que **no necesitas exigir el Visual C++ Redistributable** al usuario. Es una fuente
habitual de "en mi máquina funciona".

---

## 2. Dónde encaja la librería Rust

`core/api` se compila como **cdylib**. Su ubicación en el paquete no es negociable:

| Plataforma | Artefacto | Ubicación | Por qué |
|---|---|---|---|
| Windows | `dictar_core.dll` | Junto a `dictar_ia.exe` | Windows busca las DLL en el directorio del ejecutable |
| Linux | `libdictar_core.so` | `bundle/lib/` | El binario de Flutter lleva `RPATH=$ORIGIN/lib` |
| Android | `libdictar_core.so` | `jniLibs/<abi>/` | Lo gestiona `flutter_rust_bridge` |

`flutter_rust_bridge` puede encargarse de compilar y colocar el artefacto con `cargokit` en el
build de Flutter. Merece la pena configurarlo desde el principio: hacerlo a mano y copiarlo con
un script es la vía rápida a un instalador que arranca en tu equipo y falla en el de al lado.

**Cómo verificarlo antes de distribuir:**

```bash
# Linux — qué librerías busca y cuáles no encuentra
ldd build/linux/x64/release/bundle/dictar_ia | grep "not found"
readelf -d build/linux/x64/release/bundle/dictar_ia | grep RPATH

# Windows (PowerShell, con Dependencies.exe o dumpbin)
dumpbin /dependents build\windows\x64\runner\Release\dictar_ia.exe
```

---

## 3. Windows — instalador `.exe` con Inno Setup

**Inno Setup** es gratuito, maduro y el estándar de facto. Genera un único `.exe` de instalación.

`packaging/windows/inno_setup.iss`:

```iss
[Setup]
AppName=dictar_ia
AppVersion=0.1.0
AppPublisher=Tu Nombre
DefaultDirName={autopf}\dictar_ia
DefaultGroupName=dictar_ia
OutputDir=..\..\dist
OutputBaseFilename=dictar_ia-0.1.0-windows-setup
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Instalación por usuario: no pide UAC ni permisos de administrador
PrivilegesRequired=lowest
UninstallDisplayIcon={app}\dictar_ia.exe
WizardStyle=modern

[Languages]
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"

[Files]
Source: "..\..\build\windows\x64\runner\Release\*"; \
  DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Icons]
Name: "{group}\dictar_ia";       Filename: "{app}\dictar_ia.exe"
Name: "{autodesktop}\dictar_ia"; Filename: "{app}\dictar_ia.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Crear acceso directo en el escritorio"; \
  GroupDescription: "Accesos directos:"

[Run]
Filename: "{app}\dictar_ia.exe"; Description: "Ejecutar dictar_ia"; \
  Flags: nowait postinstall skipifsilent
```

Compilar: `iscc packaging\windows\inno_setup.iss`

**`PrivilegesRequired=lowest` es deliberado:** instala en el perfil del usuario, sin diálogo de
UAC. Para una app personal es lo cómodo. Cámbialo a `admin` solo si algún día necesitas
instalar un driver o un servicio.

### Firma de código

Sin firmar, **SmartScreen mostrará una advertencia** a quien descargue el instalador. Tú no la
verás porque tu propio equipo confía en lo que compilas.

- **Uso personal o unos pocos compañeros:** no firmes. Que sepan pulsar "Más información →
  Ejecutar de todas formas".
- **Distribución más amplia:** certificado OV (~70–200 $/año). Desde 2023 exigen almacenar la
  clave en un HSM o token, lo que complica el CI.

No lo abordes ahora. Es coste y fricción que no compra nada mientras el usuario seas tú.

### Alternativa: MSIX

El paquete `msix` de Dart genera un `.msix` con actualizaciones automáticas y desinstalación
limpia. Requiere firma (basta un certificado autofirmado para uso propio, que el usuario debe
instalar antes). Más moderno, pero más fricción para el caso "descarga y ejecuta". Inno Setup
es la elección pragmática.

---

## 4. Linux — paquete `.deb`

### 4.1 Estructura

Un `.deb` es un árbol de directorios más metadatos. Nombre en minúsculas y con guiones:
`dictar-ia`, no `dictar_ia`.

```
dictar-ia_0.1.0_amd64/
├── DEBIAN/
│   ├── control
│   └── postinst                              (opcional)
└── usr/
    ├── bin/
    │   └── dictar-ia                         → wrapper que lanza el binario real
    ├── lib/dictar-ia/                        ← contenido de bundle/ tal cual
    │   ├── dictar_ia
    │   ├── lib/ (incluye libdictar_core.so)
    │   └── data/
    └── share/
        ├── applications/dictar-ia.desktop
        └── icons/hicolor/256x256/apps/dictar-ia.png
```

`DEBIAN/control`:

```
Package: dictar-ia
Version: 0.1.0
Section: education
Priority: optional
Architecture: amd64
Depends: libgtk-3-0 (>= 3.24), libglib2.0-0, libstdc++6,
         libpipewire-0.3-0, libsecret-1-0, libsqlcipher0, libvulkan1
Maintainer: Tu Nombre <tu@email>
Description: Grabación de clases y reuniones con apuntes generados por IA
 Captura sesiones online, transcribe con Whisper en local y genera
 apuntes estructurados con marcas de tiempo verificables.
```

`usr/share/applications/dictar-ia.desktop`:

```ini
[Desktop Entry]
Type=Application
Name=dictar_ia
Comment=Grabación de clases con apuntes por IA
Exec=/usr/bin/dictar-ia
Icon=dictar-ia
Categories=Education;AudioVideo;
Terminal=false
StartupWMClass=dictar_ia
```

Construir:

```bash
dpkg-deb --build --root-owner-group dictar-ia_0.1.0_amd64
```

### 4.2 Las dependencias son el fallo número uno

Un `.deb` que arranca en tu equipo y falla en otro casi siempre es una dependencia no declarada.
Tu máquina de desarrollo tiene instalado medio sistema por otras vías.

**Genera la lista real en lugar de escribirla a mano:**

```bash
# Lista los paquetes que proveen cada .so que el binario necesita
ldd build/linux/x64/release/bundle/dictar_ia \
  | awk '/=>/ {print $3}' | grep -v '^$' \
  | xargs -r dpkg -S 2>/dev/null | cut -d: -f1 | sort -u
```

**Y pruébalo siempre en un contenedor limpio, nunca solo en tu equipo:**

```bash
docker run --rm -it -v "$PWD/dist:/dist" debian:12 bash
  apt update && apt install -y /dist/dictar-ia_0.1.0_amd64.deb
  dictar-ia --version
```

Esta prueba de dos minutos evita el fallo más común de todo el empaquetado en Linux.

### 4.3 Complementa con AppImage

Ubuntu y derivadas son solo una parte del mundo. Un **AppImage** funciona en cualquier distro
sin instalar nada y sin permisos de root, y te sirve además para probar rápido en Fedora o Arch.
Cuesta poco más y `flutter_distributor` genera ambos.

Prioridad: `.deb` primero (tu caso), AppImage después, **Flatpak no** por ahora — su sandbox
obliga a pasar audio y captura de pantalla por portales, y eso es trabajo real que no toca
hacer hasta que el producto esté maduro.

---

## 5. `flutter_distributor` — orquestar los dos

En lugar de scripts sueltos, una herramienta que invoca Inno Setup en Windows y construye
`.deb`/`.AppImage` en Linux desde una configuración común.

```bash
dart pub global activate flutter_distributor
```

`packaging/distribute_options.yaml`:

```yaml
output: dist/
releases:
  - name: prod
    jobs:
      - name: linux-deb
        package:
          platform: linux
          target: deb
      - name: linux-appimage
        package:
          platform: linux
          target: appimage
      - name: windows-exe
        package:
          platform: windows
          target: exe        # usa Inno Setup por debajo
```

```bash
flutter_distributor package --platform linux   --targets deb,appimage
flutter_distributor package --platform windows --targets exe
```

Cada target lleva además su `packaging/<target>/make_config.yaml` con el nombre, iconos y las
dependencias del `control`.

**Alternativa si `flutter_distributor` estorba:** `fpm` convierte un directorio en `.deb` o
`.rpm` en una línea. Menos integrado, más directo de depurar:

```bash
fpm -s dir -t deb -n dictar-ia -v 0.1.0 -a amd64 \
    -d libgtk-3-0 -d libpipewire-0.3-0 -d libsecret-1-0 \
    --deb-no-default-config-files \
    build/linux/x64/release/bundle/=/usr/lib/dictar-ia/
```

---

## 6. Compilación cruzada: no existe

**No puedes construir el `.exe` de Windows desde Linux.** Flutter necesita MSVC y el SDK de
Windows. No hay atajo fiable.

Dos opciones:

1. **GitHub Actions** — gratis en repositorios públicos, y la vía correcta. Compilas cada
   plataforma en su propio runner.
2. Una máquina virtual Windows, si prefieres no depender de CI.

`.github/workflows/release.yml`:

```yaml
name: release
on:
  push:
    tags: ['v*']

jobs:
  build:
    strategy:
      matrix:
        include:
          - os: ubuntu-22.04      # la más antigua razonable: fija el glibc mínimo
            platform: linux
            targets: deb,appimage
          - os: windows-latest
            platform: windows
            targets: exe
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: subosito/flutter-action@v2
        with: { channel: stable }

      - name: Dependencias de compilación (Linux)
        if: matrix.platform == 'linux'
        run: |
          sudo apt update
          sudo apt install -y ninja-build libgtk-3-dev clang cmake \
                              pkg-config libpipewire-0.3-dev libsecret-1-dev

      - run: flutter build ${{ matrix.platform }} --release
      - run: dart pub global activate flutter_distributor
      - run: flutter_distributor package --platform ${{ matrix.platform }} --targets ${{ matrix.targets }}

      - uses: actions/upload-artifact@v4
        with:
          name: dictar_ia-${{ matrix.platform }}
          path: dist/
```

**Detalle importante:** compila en Linux sobre `ubuntu-22.04`, no sobre `ubuntu-latest`. El
binario exige la versión de glibc de la máquina donde se compiló, así que compilar en la más
antigua razonable amplía las distros donde funciona. Al revés no se puede.

---

## 7. Whisper y la aceleración por GPU

El backend de `whisper.cpp` es la decisión de empaquetado más delicada, porque afecta al tamaño
y a la portabilidad:

| Backend | Tamaño extra | Portabilidad | Velocidad |
|---|---|---|---|
| CPU (AVX2) | ~0 | Funciona en todo | Base |
| **Vulkan** | ~2 MB (`libvulkan1`) | NVIDIA, AMD e Intel con drivers normales | 5–15× |
| CUDA | Cientos de MB | Solo NVIDIA + runtime instalado | 10–30× |

**Estrategia recomendada, corregida tras medirlo:**

- **Lo que distribuyes: build de CPU.** En el equipo de referencia —Core Ultra Lunar Lake con
  Arc 140V— Vulkan da **exactamente la misma velocidad** que la CPU: 4,6× tiempo real en ambos
  casos, medido dos veces. Lo único que añade es la compilación de shaders al arrancar. En
  `large-v3-turbo` el grueso del trabajo está en el decodificador, que es secuencial y no
  aprovecha una GPU integrada.
- **Vulkan sigue disponible** como característica opcional, para máquinas con GPU dedicada AMD o
  Intel, donde sí hay diferencia.
- **CUDA no se empaqueta.** Multiplicaría el instalador por diez para beneficiar a una parte de
  los usuarios. Quien tenga una NVIDIA y procese muchas horas puede compilarlo él.

> Esta corrección es un buen ejemplo de por qué la fase 0 mide en lugar de suponer: el diseño
> daba por hecho que Vulkan aportaría entre 5× y 15×, y la medición dijo que no aporta nada.

---

## 8. Modelos: descarga bajo demanda, nunca en el instalador

| Artefacto | Tamaño |
|---|---|
| `ggml-large-v3-turbo-q5_0.bin` | ~550 MB |
| `ggml-small-q5_1.bin` (pasada en vivo) | ~180 MB |
| Silero VAD (ONNX) | ~2 MB |
| `multilingual-e5-small` (ONNX) | ~120 MB |

El modelo grande pesa **quince veces más que la aplicación**. Se descargan en el primer arranque
con barra de progreso, verificación de hash y posibilidad de reanudar, a la carpeta de datos del
usuario:

- Windows: `%LOCALAPPDATA%\dictar_ia\models\`
- Linux: `~/.local/share/dictar_ia/models/`

Así el instalador se queda en un tamaño razonable y puedes cambiar de modelo sin publicar una
versión nueva.

---

## 9. Tamaños esperados

| Artefacto | Tamaño |
|---|---|
| Instalador Windows (`.exe`) | 35–55 MB |
| Paquete Linux (`.deb`) | 30–50 MB |
| AppImage | 45–65 MB |
| APK Android (fase 4) | 25–40 MB |
| Modelos (primera ejecución) | ~850 MB |

---

## 10. Android (fase 4)

Cuando llegue:

```bash
flutter build apk --release --split-per-abi   # APK por arquitectura, más ligero
flutter build appbundle --release             # AAB, solo si publicas en Play Store
```

`--split-per-abi` genera un APK por ABI en lugar de uno universal; distribuyendo directamente
solo necesitas el `arm64-v8a`, que cubre prácticamente cualquier móvil actual.

Sin Play Store no hay revisión ni política de tienda que cumplir. Requisitos que sí siguen
siendo obligatorios: `foregroundServiceType="microphone"` declarado en el manifiesto, y la
`libdictar_core.so` compilada para `arm64-v8a` (y `armeabi-v7a` si quieres cubrir equipos
antiguos).

---

## 11. Checklist antes de publicar una versión

- [ ] El `.deb` instala y arranca en un contenedor `debian:12` limpio
- [ ] El instalador de Windows funciona en una VM sin herramientas de desarrollo
- [ ] `ldd` no reporta ninguna librería `not found`
- [ ] La app arranca sin los modelos y los descarga correctamente
- [ ] Desinstalar deja el sistema limpio y **conserva** los datos del usuario
- [ ] Grabar → transcribir → generar apuntes funciona en la máquina limpia, no solo en la tuya
- [ ] El número de versión coincide en `pubspec.yaml`, el `control` y el `.iss`
</content>
</invoke>
