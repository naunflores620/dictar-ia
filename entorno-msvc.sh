# Entorno de compilación de Rust en Windows sin `vcvars64.bat`.
#
# Dos trampas concretas, las dos costaron tiempo:
#
# 1. Git Bash trae un `link` de coreutils en /usr/bin que ensombrece al
#    `link.exe` de MSVC. El síntoma no dice nada: «extra operand» y
#    «Try 'link --help'» al enlazar cualquier build script. Por eso el
#    directorio de MSVC va DELANTE en el PATH, no detrás.
#
# 2. `vcvars64.bat` no funciona con estas Build Tools porque la instalación
#    no quedó registrada (`vswhere` no devuelve nada), así que LIB e INCLUDE
#    se arman a mano.
#
# Uso:  source entorno-msvc.sh && cargo test
#
# En el CI no hace falta nada de esto: los runners de GitHub traen MSVC
# registrado y `vcvars` funciona. Es solo para compilar en esta máquina.
VSROOT="/c/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools"
VCROOT="$VSROOT/VC/Tools/MSVC/14.44.35207"
SDK="/c/Program Files (x86)/Windows Kits/10"
SDKV=10.0.26100.0

# El `cmake.exe` que trae las propias Build Tools. No está en el PATH por su
# cuenta y `whisper-rs-sys` lo necesita para compilar whisper.cpp.
export PATH="$VCROOT/bin/Hostx64/x64:$VSROOT/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin:$HOME/.cargo/bin:$PATH"

# `whisper-rs-sys` compila whisper.cpp con CMake y genera sus bindings con
# bindgen, y las dos cosas necesitan que alguien les diga dónde mirar.
#
# 1. libclang, para bindgen. OJO con la versión: LLVM 22 hace que bindgen 0.71
#    —la que trae whisper-rs-sys 0.13— genere `whisper_full_params` como
#    struct opaco, y el crate no compila con un error que no señala a la causa
#    («attempt to compute 1_usize - 264_usize»). Con LLVM 18 va bien.
# 2. El toolchain de MSVC, sin el cual whisper se compila sin AVX2 y sin `/O2`,
#    once veces más lento. Ver packaging/windows/msvc-toolchain.cmake.
export LIBCLANG_PATH="$(cygpath -w "$HOME/LLVM18/bin")"
export CMAKE_TOOLCHAIN_FILE="$(cygpath -w "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/packaging/windows/msvc-toolchain.cmake")"
export LIB="$(cygpath -w "$VCROOT/lib/x64");$(cygpath -w "$SDK/Lib/$SDKV/ucrt/x64");$(cygpath -w "$SDK/Lib/$SDKV/um/x64")"
export INCLUDE="$(cygpath -w "$VCROOT/include");$(cygpath -w "$SDK/Include/$SDKV/ucrt");$(cygpath -w "$SDK/Include/$SDKV/um");$(cygpath -w "$SDK/Include/$SDKV/shared")"
