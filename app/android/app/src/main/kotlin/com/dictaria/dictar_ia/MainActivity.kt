package com.dictaria.dictar_ia

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

/**
 * Lo que la interfaz de Flutter no puede hacer por su cuenta en Android:
 * pedir permisos al usuario y sostener la grabación en segundo plano.
 *
 * Se resuelve con un `MethodChannel` propio y no con un plugin del ecosistema
 * (`permission_handler` y compañía) por dos razones concretas: son cuarenta
 * líneas de Kotlin, y cada plugin nuevo es una dependencia más que puede
 * bloquear un `flutter pub get` — que en este proyecto ya ha sido un bloqueo
 * real. El lado Dart está en `app/lib/plataforma/android.dart`.
 */
class MainActivity : FlutterActivity() {

    companion object {
        private const val CANAL = "dictar/android"

        /** Cualquier número; solo tiene que coincidir con el de la respuesta. */
        private const val CODIGO_PERMISOS = 4711
    }

    /**
     * La llamada de Dart que está esperando la respuesta del usuario.
     *
     * `requestPermissions` es asíncrono y contesta en otro método, así que hay
     * que guardar aquí a quién responderle. Se pone a `null` en cuanto se
     * contesta: un `MethodChannel.Result` respondido dos veces lanza en Flutter.
     */
    private var permisosPendientes: MethodChannel.Result? = null

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)

        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, CANAL).setMethodCallHandler {
            llamada, respuesta ->
            when (llamada.method) {
                "tienePermisos" -> respuesta.success(tieneMicrofono())
                "pedirPermisos" -> pedirPermisos(respuesta)
                "iniciarServicio" -> {
                    servicio(ServicioGrabacion.ACCION_INICIAR)
                    respuesta.success(null)
                }
                "detenerServicio" -> {
                    servicio(ServicioGrabacion.ACCION_DETENER)
                    respuesta.success(null)
                }
                else -> respuesta.notImplemented()
            }
        }
    }

    /**
     * Solo se comprueba `RECORD_AUDIO`.
     *
     * `POST_NOTIFICATIONS` se pide junto a él, pero no se exige: sin
     * notificación la grabación es peor —el sistema puede acabar matando el
     * servicio— y aun así grabar un rato es mejor que no dejar grabar nada.
     * Sin micrófono, en cambio, no hay nada que hacer.
     */
    private fun tieneMicrofono(): Boolean =
        checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED

    private fun pedirPermisos(respuesta: MethodChannel.Result) {
        if (tieneMicrofono()) {
            respuesta.success(true)
            return
        }

        // Si ya hay una petición en vuelo, se contesta a la anterior con lo
        // que se sabe hoy en vez de dejarla colgada: un `Result` sin
        // responder deja el `await` de Dart esperando para siempre, y la
        // pantalla de grabación se queda muerta.
        permisosPendientes?.success(false)
        permisosPendientes = respuesta

        val permisos = mutableListOf(Manifest.permission.RECORD_AUDIO)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            permisos.add(Manifest.permission.POST_NOTIFICATIONS)
        }

        requestPermissions(permisos.toTypedArray(), CODIGO_PERMISOS)
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != CODIGO_PERMISOS) return

        val respuesta = permisosPendientes ?: return
        permisosPendientes = null

        // Se contesta por el permiso de micrófono en concreto, no por «todos
        // concedidos»: denegar solo las notificaciones no puede impedir
        // grabar. Se busca por nombre y no por posición porque el sistema no
        // garantiza el orden del array.
        val i = permissions.indexOf(Manifest.permission.RECORD_AUDIO)
        val concedido = i >= 0 &&
            i < grantResults.size &&
            grantResults[i] == PackageManager.PERMISSION_GRANTED

        respuesta.success(concedido)
    }

    private fun servicio(accion: String) {
        val intent = Intent(this, ServicioGrabacion::class.java).setAction(accion)
        if (accion == ServicioGrabacion.ACCION_INICIAR) {
            // `startForegroundService` y no `startService`: desde Android 8 el
            // segundo lanza si la aplicación no está en primer plano.
            startForegroundService(intent)
        } else {
            startService(intent)
        }
    }

    override fun onDestroy() {
        // Si la actividad muere con una petición sin contestar, se contesta
        // antes de irse. Si no, el `await` de Dart del otro lado no vuelve
        // nunca.
        permisosPendientes?.success(false)
        permisosPendientes = null
        super.onDestroy()
    }
}
