import java.io.FileInputStream
import java.util.Properties

plugins {
    id("com.android.application")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

// ---------------------------------------------------------------------------
// Núcleo Rust
//
// El equivalente de lo que linux/CMakeLists.txt y windows/CMakeLists.txt hacen
// en el escritorio. Sin esto el APK se genera igual, pero sin `libdictar_api.so`
// dentro: al arrancar, `main.dart` no encuentra la librería, cae en su `catch`
// y la aplicación se abre con datos de demostración. Parece funcionar, y no
// funciona.
//
// Se usa `cargo ndk` en vez de invocar a cargo a pelo porque hace falta algo
// más que el target: el enlazador del NDK, el `CMAKE_TOOLCHAIN_FILE` para que
// `whisper-rs-sys` pueda compilar whisper.cpp cruzado, y los argumentos de
// clang para bindgen. `cargo ndk` prepara ese entorno; a mano son quince
// variables fáciles de equivocar.
//
// Aquí hubo un `--bindgen` que **no existe en cargo-ndk 4.x** y hacía fallar
// la orden entera con «unexpected argument». Venía de la 3.x, donde había que
// pedir esas variables de clang a mano; ahora se ponen solas. No se detectó
// porque este archivo nunca se había ejecutado.
//
//   cargo install cargo-ndk && rustup target add aarch64-linux-android
//
// Necesita ANDROID_NDK_HOME apuntando al NDK.
// ---------------------------------------------------------------------------

// La raíz del workspace de Rust: app/android -> app -> repo.
val raizRust = rootProject.projectDir.parentFile.parentFile

// arm64 cubre prácticamente cualquier móvil en uso. Se puede pedir más desde
// la línea de órdenes sin tocar este archivo:
//   flutter build apk -Pabis=arm64-v8a,armeabi-v7a
val abisRust = (project.findProperty("abis") as String? ?: "arm64-v8a")
    .split(",")
    .map { it.trim() }
    .filter { it.isNotEmpty() }

val compilarNucleo by tasks.registering(Exec::class) {
    group = "build"
    description = "Compila core/api para Android y deja los .so en jniLibs"

    workingDir = raizRust

    // jniLibs es donde Gradle recoge las librerías nativas sin más
    // configuración; `cargo ndk -o` escribe ahí la jerarquía por ABI que
    // Gradle espera.
    val destino = file("src/main/jniLibs")

    commandLine(
        buildList {
            add("cargo")
            add("ndk")
            abisRust.forEach { abi -> add("-t"); add(abi) }
            // El mismo 26 que `minSdk`, y por el mismo motivo: `cargo ndk`
            // usa el nivel 21 por defecto y `libaaudio.so` no existe en el
            // sysroot del NDK por debajo del 26, así que `aaudio_src.rs` no
            // enlaza. Si estos dos números se separan, el APK se genera para
            // un nivel en el que el núcleo no puede grabar.
            add("--platform")
            add("26")
            add("-o")
            add(destino.absolutePath)
            add("build")
            add("--release")
            add("-p")
            add("dictar-api")
        }
    )
}

// ---------------------------------------------------------------------------
// Firma de release
//
// La plantilla de `flutter create` firma el release con la clave de depuración,
// que sirve para probar en tu propio móvil y para nada más: Play la rechaza, y
// dos APK firmados así no se pueden actualizar entre sí. Si existe un
// `key.properties` con la clave de verdad, se usa; si no, se sigue con la de
// depuración para no romper `flutter run --release` en local.
//
// key.properties (NO se versiona, está en .gitignore):
//   storeFile=/ruta/a/dictar_ia.keystore
//   storePassword=...
//   keyAlias=dictar_ia
//   keyPassword=...
// ---------------------------------------------------------------------------
val archivoFirma = rootProject.file("key.properties")
val propiedadesFirma = Properties().apply {
    if (archivoFirma.exists()) {
        FileInputStream(archivoFirma).use { load(it) }
    }
}
val hayFirmaPropia = archivoFirma.exists()

android {
    namespace = "com.dictaria.dictar_ia"
    compileSdk = flutter.compileSdkVersion
    ndkVersion = flutter.ndkVersion

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        applicationId = "com.dictaria.dictar_ia"
        // 26 y no `flutter.minSdkVersion`: AAudio, con el que
        // `core/audio-capture` graba en Android, existe desde API 26. Con el
        // mínimo de la plantilla de Flutter el APK se instalaría en teléfonos
        // donde `libaaudio.so` no existe, y la aplicación caería al abrir la
        // grabación en vez de al instalarse — el peor momento posible.
        //
        // API 26 es Android 8, de 2017: por debajo de eso no queda
        // prácticamente ningún teléfono en uso.
        minSdk = 26
        targetSdk = flutter.targetSdkVersion
        versionCode = flutter.versionCode
        versionName = flutter.versionName

        // Que el APK no cargue con ABIs para las que no se ha compilado el
        // núcleo: sin este filtro, un móvil x86 instalaría la aplicación y se
        // encontraría sin librería nativa.
        ndk {
            abiFilters.addAll(abisRust)
        }
    }

    signingConfigs {
        if (hayFirmaPropia) {
            create("release") {
                storeFile = propiedadesFirma.getProperty("storeFile")?.let { file(it) }
                storePassword = propiedadesFirma.getProperty("storePassword")
                keyAlias = propiedadesFirma.getProperty("keyAlias")
                keyPassword = propiedadesFirma.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            signingConfig = if (hayFirmaPropia) {
                signingConfigs.getByName("release")
            } else {
                signingConfigs.getByName("debug")
            }
        }
    }
}

// El núcleo tiene que estar compilado antes de que Gradle empaquete las
// librerías nativas. `preBuild` es el punto anterior a todo lo demás, así que
// vale igual para debug, para release y para el bundle.
tasks.named("preBuild") {
    dependsOn(compilarNucleo)
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

flutter {
    source = "../.."
}
