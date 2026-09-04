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
VCROOT="/c/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC/14.44.35207"
SDK="/c/Program Files (x86)/Windows Kits/10"
SDKV=10.0.26100.0

export PATH="$VCROOT/bin/Hostx64/x64:$HOME/.cargo/bin:$PATH"
export LIB="$(cygpath -w "$VCROOT/lib/x64");$(cygpath -w "$SDK/Lib/$SDKV/ucrt/x64");$(cygpath -w "$SDK/Lib/$SDKV/um/x64")"
export INCLUDE="$(cygpath -w "$VCROOT/include");$(cygpath -w "$SDK/Include/$SDKV/ucrt");$(cygpath -w "$SDK/Include/$SDKV/um");$(cygpath -w "$SDK/Include/$SDKV/shared")"
