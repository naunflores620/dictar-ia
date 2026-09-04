package com.dictaria.dictar_ia

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder

/**
 * Servicio en primer plano que sostiene la grabación.
 *
 * Sin esto, Android corta la captura en cuanto la pantalla se apaga o el
 * usuario cambia de aplicación, y una reunión de una hora se queda en unos
 * minutos. No es una optimización: es el único mecanismo que el sistema
 * ofrece para seguir usando el micrófono en segundo plano.
 *
 * El servicio **no graba**. La captura vive en Rust (`core/audio-capture`,
 * `aaudio_src.rs`) y sigue corriendo en su propio hilo nativo, ajeno al ciclo
 * de vida de la actividad. Lo que hace este servicio es declararle al sistema
 * que este proceso está haciendo algo que el usuario ve y quiere, para que no
 * lo mate ni le retire el micrófono. Por eso no hay ningún canal de audio
 * entre Kotlin y Rust: sería una tubería que no transporta nada.
 *
 * La notificación permanente es el precio, y es obligatoria.
 */
class ServicioGrabacion : Service() {

    companion object {
        const val ACCION_INICIAR = "com.dictaria.dictar_ia.INICIAR"
        const val ACCION_DETENER = "com.dictaria.dictar_ia.DETENER"

        private const val CANAL = "grabacion"
        private const val ID_NOTIFICACION = 1
    }

    // El servicio no mantiene estado propio: no hay nada que enlazar.
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACCION_DETENER -> {
                detener()
                return START_NOT_STICKY
            }
            else -> arrancarEnPrimerPlano()
        }

        // START_NOT_STICKY y no START_STICKY: si el sistema mata el proceso a
        // mitad de una reunión, resucitar el servicio solo dejaría la
        // notificación encendida sin nadie grabando detrás — la captura de
        // Rust habría muerto con el proceso. El audio ya escrito está en
        // disco y la sesión se puede procesar; fingir que se sigue grabando
        // sería peor que reconocer que se cortó.
        return START_NOT_STICKY
    }

    private fun arrancarEnPrimerPlano() {
        crearCanal()

        // Tocar la notificación devuelve a la aplicación en vez de abrir otra
        // copia: `singleTop` en el manifiesto más este intent reutilizan la
        // actividad que ya está viva.
        val abrir = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }
        val pendiente = PendingIntent.getActivity(
            this,
            0,
            abrir,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

        val notificacion: Notification = Notification.Builder(this, CANAL)
            .setContentTitle("Grabando")
            .setContentText("dictar_ia está grabando el micrófono")
            .setSmallIcon(android.R.drawable.ic_btn_speak_now)
            .setContentIntent(pendiente)
            // Que no se pueda descartar de un deslizamiento: si el usuario la
            // quita creyendo que cierra la grabación, el sistema mata el
            // servicio y la reunión se pierde a medias.
            .setOngoing(true)
            .build()

        // Desde Android 10 hay que declarar *para qué* es el primer plano, y
        // desde Android 14 el sistema rechaza el servicio si el tipo no
        // coincide con el permiso declarado en el manifiesto.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(
                ID_NOTIFICACION,
                notificacion,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE,
            )
        } else {
            startForeground(ID_NOTIFICACION, notificacion)
        }
    }

    private fun crearCanal() {
        val gestor = getSystemService(NotificationManager::class.java)

        // IMPORTANCE_LOW: la notificación tiene que estar visible, pero no
        // tiene que sonar ni vibrar cada vez que empieza una clase.
        val canal = NotificationChannel(
            CANAL,
            "Grabación en curso",
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            description = "Se muestra mientras dictar_ia está grabando"
            setShowBadge(false)
        }
        gestor.createNotificationChannel(canal)
    }

    private fun detener() {
        // `removeNotification` explícito: sin él, en algunas versiones la
        // notificación sobrevive unos segundos al servicio y el usuario cree
        // que se le sigue grabando.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
            stopForeground(STOP_FOREGROUND_REMOVE)
        } else {
            @Suppress("DEPRECATION")
            stopForeground(true)
        }
        stopSelf()
    }
}
