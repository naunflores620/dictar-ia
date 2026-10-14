import 'package:dictar_ia/datos/repositorio.dart';
import 'package:dictar_ia/modelos/dominio.dart';
import 'package:dictar_ia/pantallas/sesion.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

/// La pantalla de detalle de una sesión, con datos buenos detrás.
///
/// Nace de un caso concreto y no de una idea de cobertura: el 2026-09-10 esta
/// pantalla apareció **en blanco** sobre una sesión que sí tenía contenido
/// —comprobado en la base: 23 frases definitivas con texto y 418 caracteres de
/// Markdown—, y ni el núcleo ni el puente ni el registro de Flutter daban
/// ningún error. Con todo verde y la pantalla vacía, el único sitio donde
/// quedaba mirar era el propio árbol de widgets, y no había forma de mirarlo
/// sin abrir la aplicación a mano.
///
/// Estos tests son esa forma.

/// Repositorio con notas y transcripción garantizadas.
///
/// No vale usar `RepositorioDemo` tal cual: sus notas dependen del `id` de la
/// sesión y devuelven `null` para la mayoría, así que un test montado sobre él
/// falla por el motivo equivocado — lo comprobé pagándolo.
class _RepoConTodo extends RepositorioDemo {
  @override
  Future<Notas?> notas(String sesionId) async => const Notas(
        markdown: '# Cálculo II\n\nResumen de la clase de prueba.\n',
        tldr: 'Resumen de la clase de prueba.',
        plantilla: 'class',
        proveedor: 'prueba',
        modelo: 'prueba',
      );
}

void main() {
  Future<String> abrir(WidgetTester tester, Repositorio repo) async {
    final id = (await repo.sesiones()).first.id;
    await tester.pumpWidget(
      MaterialApp(home: PantallaSesion(repo: repo, sesionId: id)),
    );
    await tester.pumpAndSettle();
    return id;
  }

  testWidgets('la pestaña de notas enseña el Markdown', (tester) async {
    final repo = _RepoConTodo();
    await abrir(tester, repo);

    expect(find.text('Notas'), findsOneWidget);
    expect(find.text('Transcripción'), findsOneWidget);

    expect(
      find.textContaining('Resumen de la clase'),
      findsAtLeast(1),
      reason: 'la pestaña de notas se pintó vacía',
    );
  });

  testWidgets('se puede cambiar a la pestaña de transcripción',
      (tester) async {
    final repo = _RepoConTodo();
    await abrir(tester, repo);

    // `warnIfMissed` queda en su valor por defecto a propósito: si el toque no
    // impacta en nada —que es justo lo que pasaba— se quiere ver el aviso.
    await tester.tap(find.text('Transcripción'));
    await tester.pumpAndSettle();

    final frases = await repo.transcripcion('x');
    final trozo = frases.first.texto.split(' ').take(4).join(' ');

    expect(
      find.textContaining(trozo),
      findsAtLeast(1),
      reason: 'la transcripción no se pintó al cambiar de pestaña',
    );
  });
}
