import 'dart:async';

import 'package:dictar_ia/datos/participantes.dart';
import 'package:dictar_ia/datos/repositorio.dart';
import 'package:dictar_ia/modelos/dominio.dart';
import 'package:dictar_ia/pantallas/sesion.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

/// DIA-1: ver quién habló en una sesión, ponerle nombre y regenerar los
/// apuntes. Todo sobre `PantallaSesion` y `participantesDe` reales; lo único que
/// se sustituye es el repositorio, que es la frontera con el núcleo.

Frase _frase(Pista pista, String? quien, int ini, int fin) => Frase(
  inicioMs: ini,
  finMs: fin,
  texto: 'texto $ini',
  pista: pista,
  hablante: quien,
);

/// Repositorio que apunta lo que le piden y devuelve notas siempre, para que la
/// pestaña de notas enseñe el botón «Regenerar».
class _RepoEspia extends RepositorioDemo {
  final renombrados = <(Pista, String, String)>[];
  int regeneraciones = 0;
  int procesados = 0;
  Completer<void>? bloqueo;

  @override
  Future<Notas?> notas(String sesionId) async => const Notas(
    markdown: '# Reunión\n\nResumen de la reunión de prueba.\n',
    tldr: 'Resumen de la reunión de prueba.',
    plantilla: 'team',
    proveedor: 'prueba',
    modelo: 'prueba',
  );

  @override
  Future<int> renombrarParticipante({
    required String sesionId,
    required Pista pista,
    required String de,
    required String a,
  }) {
    renombrados.add((pista, de, a));
    return super.renombrarParticipante(
      sesionId: sesionId,
      pista: pista,
      de: de,
      a: a,
    );
  }

  @override
  Future<ResultadoProceso> regenerarNotas(String sesionId) async {
    regeneraciones++;
    await bloqueo?.future;
    return super.regenerarNotas(sesionId);
  }

  @override
  Future<ResultadoProceso> procesar(String sesionId) {
    procesados++;
    return super.procesar(sesionId);
  }
}

void main() {
  group('participantesDe', () {
    test('agrupa por canal y nombre, suma el tiempo y ordena', () {
      final p = participantesDe([
        _frase(Pista.sistema, 'Participante 2', 0, 4000),
        _frase(Pista.micro, 'Yo', 4000, 6000),
        _frase(Pista.sistema, 'Participante 1', 6000, 16000),
        _frase(Pista.sistema, 'Participante 2', 16000, 18000),
        _frase(Pista.micro, null, 20000, 21000),
      ]);

      // Primero el sistema, con quien más habla arriba; luego el micrófono.
      expect(p.map((x) => x.nombre), [
        'Participante 1',
        'Participante 2',
        'Yo',
      ]);
      expect(p[0].tiempoHablado, const Duration(seconds: 10));
      expect(p[1].tiempoHablado, const Duration(seconds: 6));
      expect(p[1].frases, 2);
      // La frase sin hablante cae en la etiqueta por defecto de su canal, que
      // aquí coincide con «Yo»: es la misma voz y debe contarse una sola vez.
      expect(p[2].frases, 2);
      expect(p[2].tiempoHablado, const Duration(seconds: 3));
    });

    test('el mismo nombre en canales distintos son participantes distintos', () {
      // Antes de incluir el canal en la clave, un «Yo» del sistema se habría
      // fundido con el del micrófono y renombrarlo habría tocado las dos pistas.
      final p = participantesDe([
        _frase(Pista.sistema, 'Ana', 0, 1000),
        _frase(Pista.micro, 'Ana', 1000, 2000),
      ]);
      expect(p.length, 2);
      expect(p[0].pista, Pista.sistema);
      expect(p[1].pista, Pista.micro);
    });

    test('una frase con el final antes que el inicio no resta tiempo', () {
      final p = participantesDe([_frase(Pista.sistema, 'Ana', 5000, 4000)]);
      expect(p.single.tiempoHablado, Duration.zero);
    });
  });

  group('PantallaSesion', () {
    Future<_RepoEspia> abrir(WidgetTester tester, [_RepoEspia? dado]) async {
      final repo = dado ?? _RepoEspia();
      final id = (await repo.sesiones()).first.id;
      await tester.pumpWidget(
        MaterialApp(
          home: PantallaSesion(repo: repo, sesionId: id),
        ),
      );
      await tester.pumpAndSettle();
      return repo;
    }

    testWidgets(
      'la transcripción enseña los participantes con canal y tiempo',
      (tester) async {
        await abrir(tester);
        await tester.tap(find.text('Transcripción'));
        await tester.pumpAndSettle();

        expect(find.text('Participantes'), findsOneWidget);
        // La demostración tiene a «Profesor» (sistema) y a una voz propia.
        expect(
          find.textContaining('sistema', findRichText: true),
          findsOneWidget,
        );
        expect(
          find.textContaining('micrófono', findRichText: true),
          findsOneWidget,
        );
        expect(find.byIcon(Icons.edit_outlined), findsNWidgets(2));
      },
    );

    testWidgets('renombrar a un participante lo cambia en el panel', (
      tester,
    ) async {
      final repo = await abrir(tester);
      await tester.tap(find.text('Transcripción'));
      await tester.pumpAndSettle();

      await tester.tap(find.byTooltip('Renombrar a Profesor'));
      await tester.pumpAndSettle();
      await tester.enterText(find.byType(TextField), '  Dra. Pérez ');
      await tester.tap(find.text('Guardar'));
      // Sin `pumpAndSettle`: esperaría a que el aviso se retire solo.
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 300));

      expect(repo.renombrados, [(Pista.sistema, 'Profesor', 'Dra. Pérez')]);
      expect(find.byTooltip('Renombrar a Dra. Pérez'), findsOneWidget);
      expect(find.byTooltip('Renombrar a Profesor'), findsNothing);
      // Y se avisa de que los apuntes aún no lo saben.
      expect(find.textContaining('hasta que los regeneres'), findsOneWidget);
    });

    testWidgets('cancelar o dejar el mismo nombre no llama al núcleo', (
      tester,
    ) async {
      final repo = await abrir(tester);
      await tester.tap(find.text('Transcripción'));
      await tester.pumpAndSettle();

      await tester.tap(find.byTooltip('Renombrar a Profesor'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Cancelar'));
      await tester.pumpAndSettle();

      await tester.tap(find.byTooltip('Renombrar a Profesor'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Guardar'));
      await tester.pumpAndSettle();

      expect(repo.renombrados, isEmpty);
    });

    testWidgets('Regenerar vuelve a generar las notas sin volver a procesar', (
      tester,
    ) async {
      final repo = await abrir(tester);

      await tester.tap(find.text('Regenerar'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 400));
      await tester.pump(const Duration(milliseconds: 300));

      expect(repo.regeneraciones, 1);
      // `procesar` transcribe: regenerar no debe llegar a él nunca.
      expect(repo.procesados, 0);
      expect(find.textContaining('Apuntes regenerados'), findsOneWidget);
    });

    testWidgets('mientras regenera no se puede lanzar otra generación', (
      tester,
    ) async {
      final repo = await abrir(tester, _RepoEspia()..bloqueo = Completer());

      await tester.tap(find.text('Regenerar'));
      await tester.pump();

      // El botón se sustituye por un indicador: cada generación cuesta dinero.
      expect(find.text('Regenerar'), findsNothing);
      expect(find.byType(CircularProgressIndicator), findsOneWidget);
      expect(repo.regeneraciones, 1);

      repo.bloqueo!.complete();
      await tester.pumpAndSettle();
      expect(find.text('Regenerar'), findsOneWidget);
    });
  });
}
