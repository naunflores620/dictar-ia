import 'dart:async';

import 'package:dictar_ia/datos/repositorio.dart';
import 'package:dictar_ia/modelos/dominio.dart';
import 'package:dictar_ia/pantallas/grabacion.dart';
import 'package:dictar_ia/ventana.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

/// Repositorio de demostración al que el test le dicta los avisos de audio.
///
/// El núcleo real no está en una prueba de widget; lo que importa aquí es qué
/// hace la pantalla con lo que le llega por [avisosAudio], no quién lo emite.
class _RepoConAvisos extends RepositorioDemo {
  final _avisos = StreamController<AvisoAudio>.broadcast();

  /// Detener no termina nunca: así el test mira la pantalla **mientras** el
  /// núcleo tarda en parar, que es cuando la banda tiene que haberse vaciado.
  final _detenido = Completer<void>();

  void emitir(AvisoAudio a) => _avisos.add(a);

  @override
  Stream<AvisoAudio> get avisosAudio => _avisos.stream;

  @override
  Future<void> detenerGrabacion() => _detenido.future;

  @override
  void dispose() {
    _avisos.close();
    super.dispose();
  }
}

const _eco = AvisoAudio(
  pista: Pista.micro,
  codigo: 'eco',
  activo: true,
  mensaje: 'El micrófono recoge lo que suena por los altavoces.',
  valor: 0.7,
);

AvisoAudio _apagado(AvisoAudio a) => AvisoAudio(
  pista: a.pista,
  codigo: a.codigo,
  activo: false,
  mensaje: a.mensaje,
);

void main() {
  group('HU-11 criterio 4 — funciones que no tienen sentido en un móvil', () {
    // Antes de este archivo, ningún test de `app/test` mencionaba
    // `PantallaGrabacion` ni `debugDefaultTargetPlatformOverride` (comprobado
    // por `grep` en la revisión de T-15): invertir la negación de
    // `Ventana.esEscritorio`, o sumar `TargetPlatform.android` a su lista de
    // plataformas de escritorio, no lo habría detectado nadie.
    testWidgets(
      'un móvil en Android no ofrece elegir el área de captura manual',
      (tester) async {
        // El reset va en un `finally`, síncrono al final del propio cuerpo, y
        // no en `addTearDown`: `TestWidgetsFlutterBinding` comprueba que las
        // variables de depuración de Flutter volvieron a su valor por
        // defecto **dentro** de `runTest`, antes de que el `Future` del test
        // se resuelva -- y los `tearDown`/`addTearDown` de `package:test`
        // solo corren después de eso. Un `addTearDown` aquí deja el test en
        // rojo por una razón ajena a lo que se está probando.
        debugDefaultTargetPlatformOverride = TargetPlatform.android;
        try {
          final repo = RepositorioDemo();
          addTearDown(repo.dispose);

          await tester.pumpWidget(
            MaterialApp(home: PantallaGrabacion(repo: repo)),
          );
          await tester.pumpAndSettle();

          expect(find.text('Área para captura manual'), findsNothing);
          expect(find.text('Capturar diapositivas'), findsNothing);
        } finally {
          debugDefaultTargetPlatformOverride = null;
        }
      },
    );

    testWidgets(
      'en escritorio sí se ofrece elegir el área de captura manual',
      (tester) async {
        // El mismo montaje que el test anterior, con la única variable que
        // importa cambiada. Sin este segundo test, la ausencia en Android
        // podría deberse a cualquier otra cosa —un `FutureBuilder` que nunca
        // resuelve, un fallo silencioso al construir la fila— y no a la
        // comprobación de plataforma que este par existe para proteger.
        //
        // Antes buscaba «Área de la diapositiva» y «Capturar diapositivas»:
        // la pantalla de grabación dejó de ofrecer la captura automática y
        // ese texto solo sobrevive en Ajustes, así que el test fallaba en
        // escritorio por un texto que ya no existe y no por la plataforma.
        debugDefaultTargetPlatformOverride = TargetPlatform.windows;
        try {
          final repo = RepositorioDemo();
          addTearDown(repo.dispose);

          await tester.pumpWidget(
            MaterialApp(home: PantallaGrabacion(repo: repo)),
          );
          await tester.pumpAndSettle();

          expect(find.text('Área para captura manual'), findsOneWidget);
          expect(find.text('Elegir'), findsOneWidget);
          // Lo quitado no se reintroduce: la captura automática ya no existe.
          expect(find.text('Capturar diapositivas'), findsNothing);
        } finally {
          debugDefaultTargetPlatformOverride = null;
        }
      },
    );
  });

  testWidgets(
    'un teléfono en vertical usa la disposición amplia, no la compacta '
    'del panel de escritorio',
    (tester) async {
      // HU-11 criterio 1. `_esCompacto` decidía por ancho o por alto
      // (`ancho < 460 || alto < 560`): un teléfono mide unos 400 de ancho, así
      // que cumplía esa condición siempre y caía en la disposición pensada
      // para el panel de 380×330, aunque tuviera alto de sobra para la
      // transcripción en vivo. Ahora decide solo por alto.
      //
      // La plataforma es iOS y no Android a propósito: `_iniciar()` llama a
      // `Android.tienePermisos()` antes de arrancar, y en Android eso toca un
      // `MethodChannel` real que este test no tiene motivo para simular —lo
      // que se comprueba aquí es `_esCompacto` (el alto de `MediaQuery`), no
      // el flujo de permisos de HU-02—. `Android.esAndroid` da `false` en
      // cualquier plataforma que no sea, literalmente, Android, así que en
      // iOS ese paso se salta solo. Y iOS tampoco es escritorio, así que el
      // montaje sigue siendo fiel a "un teléfono".
      debugDefaultTargetPlatformOverride = TargetPlatform.iOS;
      final repo = RepositorioDemo();
      try {
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);

        // El tamaño de la ventana se deja en el por defecto del test mientras
        // se está en `_configuracion()` (antes de grabar): a un ancho de
        // móvil (~400) el desplegable de asignatura desborda por su
        // `helperText` largo, un defecto real y preexistente de esa pantalla
        // —detectado corriendo este mismo test— y ajeno a lo que aquí se
        // verifica (`_esCompacto`, que solo se evalúa **durante** la
        // grabación). Encogerse a un tamaño de teléfono se hace después de
        // arrancar, cuando ese desplegable ya no está en el árbol.
        await tester.pumpWidget(
          MaterialApp(home: PantallaGrabacion(repo: repo)),
        );
        await tester.tap(find.text('Empezar a grabar'));
        await tester.pump();
        // `RepositorioDemo.iniciarGrabacion` no marca `grabando: true` hasta
        // el primer tick de su `Timer.periodic` de un segundo: antes de este
        // pump la pantalla todavía muestra la configuración, no la
        // grabación.
        await tester.pump(const Duration(seconds: 1));

        tester.view.physicalSize = const Size(400, 800); // móvil en vertical
        tester.view.devicePixelRatio = 1.0;
        await tester.pump();

        // Alto de sobra (800) para la transcripción en vivo: toca la
        // disposición amplia, no la compacta.
        expect(find.text('Escuchando…'), findsOneWidget);
        expect(find.text('Detener y generar notas'), findsNothing);

        // El mismo ancho estrecho, pero con el alto real del panel encogido
        // de escritorio (`Ventana.compacto`, 330 de alto): aquí sí toca la
        // compacta. Sin esta segunda mitad, la primera no distinguiría
        // "decide por alto" de "nunca es compacta".
        tester.view.physicalSize = Size(
          Ventana.compacto.width,
          Ventana.compacto.height,
        );
        await tester.pump();

        expect(find.text('Detener y generar notas'), findsOneWidget);
        expect(find.text('Escuchando…'), findsNothing);
      } finally {
        // `dispose()` y no `addTearDown`: cancela el `Timer.periodic` de
        // `RepositorioDemo.iniciarGrabacion` de forma síncrona, antes de que
        // el cuerpo del test termine. `AutomatedTestWidgetsFlutterBinding`
        // exige que no quede ningún timer pendiente **dentro** de
        // `runTest`, antes de que corran los `addTearDown` de
        // `package:test` -- el mismo motivo por el que el override de
        // plataforma se resetea aquí y no con `addTearDown`.
        repo.dispose();
        debugDefaultTargetPlatformOverride = null;
      }
    },
  );
  group('AV-2 — avisos de salud del audio durante la grabación', () {
    // iOS por la misma razón que el test de arriba: no es Android (no toca el
    // `MethodChannel` de permisos) ni escritorio (no encoge la ventana).
    Future<void> grabar(WidgetTester tester, Repositorio repo) async {
      await tester.pumpWidget(MaterialApp(home: PantallaGrabacion(repo: repo)));
      await tester.tap(find.text('Empezar a grabar'));
      await tester.pump();
      await tester.pump(const Duration(seconds: 1));
    }

    // El evento llega por un `Stream` y se entrega en un microtask que
    // `pump` vacía **después** de dibujar el cuadro: hace falta un segundo
    // `pump` para ver el resultado.
    Future<void> emitir(
      WidgetTester tester,
      _RepoConAvisos repo,
      AvisoAudio a,
    ) async {
      repo.emitir(a);
      await tester.pump();
      await tester.pump();
    }

    testWidgets(
      'la banda enseña el aviso, se puede cerrar, vuelve al reactivarse y se '
      'vacía al detener',
      (tester) async {
        debugDefaultTargetPlatformOverride = TargetPlatform.iOS;
        final repo = _RepoConAvisos();
        try {
          await grabar(tester, repo);
          expect(find.text(_eco.mensaje), findsNothing);

          await emitir(tester, repo, _eco);
          expect(find.text(_eco.mensaje), findsOneWidget);

          await tester.tap(find.byTooltip('Ocultar este aviso'));
          await tester.pump();
          expect(find.text(_eco.mensaje), findsNothing);

          // Cerrado no es resuelto: mientras siga activo no vuelve a pintarse
          // cada vez que el núcleo lo repite...
          await emitir(tester, repo, _eco);
          expect(find.text(_eco.mensaje), findsNothing);

          // ...pero si se apaga y se reactiva, es otro aviso y vuelve.
          await emitir(tester, repo, _apagado(_eco));
          await emitir(tester, repo, _eco);
          expect(find.text(_eco.mensaje), findsOneWidget);

          // Al pulsar Detener la banda se vacía de inmediato, sin esperar a
          // que el núcleo termine de parar.
          await tester.tap(find.text('Detener'));
          await tester.pump();
          await tester.pump();
          expect(find.text(_eco.mensaje), findsNothing);
        } finally {
          repo.dispose();
          debugDefaultTargetPlatformOverride = null;
        }
      },
    );

    testWidgets(
      'en la disposición compacta el aviso es un punto con el texto en el '
      'tooltip',
      (tester) async {
        debugDefaultTargetPlatformOverride = TargetPlatform.iOS;
        final repo = _RepoConAvisos();
        try {
          addTearDown(tester.view.resetPhysicalSize);
          addTearDown(tester.view.resetDevicePixelRatio);
          await grabar(tester, repo);

          tester.view.physicalSize = Size(
            Ventana.compacto.width,
            Ventana.compacto.height,
          );
          tester.view.devicePixelRatio = 1.0;
          await tester.pump();
          expect(find.byKey(const ValueKey('punto-aviso-audio')), findsNothing);

          await emitir(tester, repo, _eco);
          expect(
            find.byKey(const ValueKey('punto-aviso-audio')),
            findsOneWidget,
          );
          // No hay banda: no cabe en 330 px de alto.
          expect(find.byTooltip('Ocultar este aviso'), findsNothing);

          await tester.tap(find.byKey(const ValueKey('punto-aviso-audio')));
          await tester.pump();
          expect(find.text(_eco.mensaje), findsOneWidget);
          // Deja correr el tiempo del tooltip: un temporizador vivo al final
          // del test lo rompe.
          await tester.pump(const Duration(seconds: 5));

          await emitir(tester, repo, _apagado(_eco));
          expect(find.byKey(const ValueKey('punto-aviso-audio')), findsNothing);
        } finally {
          repo.dispose();
          debugDefaultTargetPlatformOverride = null;
        }
      },
    );
  });
}
