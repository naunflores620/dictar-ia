# Cómo hay que compilar whisper.cpp con MSVC — y por qué hace falta decirlo.
#
# `whisper-rs-sys` invoca a CMake a través del crate `cmake`, y ese crate no
# expone ninguna forma de pasarle opciones. La única puerta que deja abierta es
# `CMAKE_TOOLCHAIN_FILE`, que sí lee del entorno. Este archivo es esa puerta.
#
# Sin él, en Windows salen dos defectos a la vez, ninguno de los cuales avisa:
# el build pasa en verde y el binario funciona, solo que once veces más lento.
#
# Medido el 2026-09-10 en un Core Ultra 9 288V, transcribiendo 106 s de audio
# con el modelo `small` y las opciones de la pasada definitiva:
#
#     como estaba          no terminó en 600 s
#     solo AVX2                      368 s   0,3x tiempo real
#     AVX2 + /O2                      32 s   3,8x tiempo real
#
# Es decir: diez horas de clase pasan de 33 h de procesado a 2,7 h, que es lo
# que el README promete y que en Windows era falso.
#
# Uso, tanto en local como en el CI:
#
#     CMAKE_TOOLCHAIN_FILE=<ruta a este archivo> cargo build --release
#
# En Linux no hace falta nada de esto: ggml con GCC usa `-march=native` y CMake
# no pierde sus flags de release, así que allí ya se compilaba bien. Este
# archivo es específico de MSVC; aplicarlo en Linux rompería los flags.

# --- 1. Instrucciones vectoriales ---
#
# `GGML_NATIVE=ON` (el defecto) sirve con GCC y Clang, que entienden
# `-march=native`. Con MSVC no hay equivalente: hay que pedir cada juego de
# instrucciones por su nombre. Sin esto, ggml se compila para SSE2 en un chip
# que tiene AVX2, FMA y F16C.
#
# Y hay que pedirlo por estas opciones y NO metiendo `/arch:AVX2` en CFLAGS:
# el flag a pelo activa el camino AVX2 de `ggml.c` sin que se incluya
# `immintrin.h` —ese include lo añade el propio CMake de ggml cuando la opción
# está activa—, y la compilación falla con los `_mm256_*` sin declarar.
set(GGML_NATIVE OFF CACHE BOOL "" FORCE)
set(GGML_AVX    ON  CACHE BOOL "" FORCE)
set(GGML_AVX2   ON  CACHE BOOL "" FORCE)
set(GGML_FMA    ON  CACHE BOOL "" FORCE)
set(GGML_F16C   ON  CACHE BOOL "" FORCE)

# --- 2. Optimización ---
#
# Este es el que de verdad costaba las once veces, y el más difícil de ver.
#
# El crate `cmake` construye `CMAKE_C_FLAGS_RELEASE` a partir de lo que le da
# el crate `cc` y se lo pasa a CMake con `-D`. Al hacerlo **pisa el valor por
# defecto de CMake para MSVC**, que es el que lleva `/O2 /Ob2 /DNDEBUG`. El
# resultado es un `CMAKE_C_FLAGS_RELEASE` que dice ` -nologo -MD -Brepro -W0`
# y nada más: ggml compilado con `/Od`, sin optimizar, dentro de un perfil que
# se llama «Release».
#
# Eso explica también por qué AVX2 por sí solo no arreglaba casi nada: código
# sin optimizar no se vectoriza, así que las instrucciones estaban disponibles
# y sin usar.
#
# Un toolchain file se procesa después de que los `-D` de la línea de órdenes
# entren en la caché, así que `FORCE` los sustituye. Ponerlo en CFLAGS no vale
# por otro motivo añadido: Git Bash convierte `/O2` en una ruta de Windows
# (`C:/Program Files/Git/O2`) antes de que llegue al compilador.
set(CMAKE_C_FLAGS_RELEASE   "/MD /O2 /Ob2 /DNDEBUG" CACHE STRING "" FORCE)
set(CMAKE_CXX_FLAGS_RELEASE "/MD /O2 /Ob2 /DNDEBUG" CACHE STRING "" FORCE)

# --- Lo que esto cuesta, dicho claro ---
#
# El binario resultante **exige AVX2**, así que no arranca en CPUs anteriores a
# 2013 (Intel anterior a Haswell, AMD anterior a Excavator). Para el caso de
# uso de este proyecto —grabar clases en un portátil actual— es un intercambio
# razonable, pero es una decisión de producto y queda escrita aquí para que no
# se tome por accidente. La alternativa, si algún día importa, es
# `GGML_CPU_ALL_VARIANTS`, que compila varias variantes y elige en tiempo de
# ejecución, a cambio de un binario más grande y una compilación más larga.
