import 'package:dictar_ia/widgets/notas_markdown.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

Widget _envolver(Widget hijo) => MaterialApp(
      home: Scaffold(body: SingleChildScrollView(child: hijo)),
    );

void main() {
  testWidgets('los títulos se renderizan sin las almohadillas', (tester) async {
    await tester.pumpWidget(_envolver(
      const NotasMarkdown(markdown: '# Cálculo II\n\n## Avisos'),
    ));

    expect(find.text('Cálculo II'), findsOneWidget);
    expect(find.textContaining('#'), findsNothing);
  });

  testWidgets('una marca de tiempo es pulsable y devuelve los milisegundos',
      (tester) async {
    int? pulsado;
    await tester.pumpWidget(_envolver(
      NotasMarkdown(
        markdown: '- Parcial del tema 3 [00:22:00]',
        onSaltar: (ms) => pulsado = ms,
      ),
    ));

    await tester.tap(find.text('00:22:00'));
    await tester.pump();

    // 22 minutos = 1_320_000 ms. Es lo que permite verificar en el audio lo
    // que afirma la IA.
    expect(pulsado, 1320000);
  });

  testWidgets('también se detecta la marca envuelta en cursiva', (tester) async {
    int? pulsado;
    await tester.pumpWidget(_envolver(
      NotasMarkdown(
        markdown: '- **Región de convergencia** — «esto cae» _[00:04:15]_',
        onSaltar: (ms) => pulsado = ms,
      ),
    ));

    await tester.tap(find.text('00:04:15'));
    await tester.pump();
    expect(pulsado, 255000);
  });

  testWidgets('acepta marcas en mm:ss además de hh:mm:ss', (tester) async {
    int? pulsado;
    await tester.pumpWidget(_envolver(
      NotasMarkdown(markdown: 'Dijo esto [02:30]', onSaltar: (ms) => pulsado = ms),
    ));

    await tester.tap(find.text('02:30'));
    await tester.pump();
    expect(pulsado, 150000);
  });

  testWidgets('los compromisos se dibujan como casillas', (tester) async {
    await tester.pumpWidget(_envolver(
      const NotasMarkdown(markdown: '- [ ] Enviar propuesta — **2026-08-14**'),
    ));

    expect(find.byIcon(Icons.check_box_outline_blank), findsOneWidget);
    expect(find.textContaining('[ ]'), findsNothing);
  });

  testWidgets('la negrita no deja los asteriscos a la vista', (tester) async {
    await tester.pumpWidget(_envolver(
      const NotasMarkdown(markdown: 'Parcial el **15 de septiembre**'),
    ));
    expect(find.textContaining('**'), findsNothing);
  });

  testWidgets('las citas del cliente se muestran destacadas', (tester) async {
    await tester.pumpWidget(_envolver(
      const NotasMarkdown(markdown: '> se nos va medio equipo tres días al mes'),
    ));
    expect(
      find.textContaining('se nos va medio equipo'),
      findsOneWidget,
    );
  });

  testWidgets('un bloque de fórmula se renderiza aparte', (tester) async {
    await tester.pumpWidget(_envolver(
      const NotasMarkdown(markdown: r'''
Definición:

$$
F(s)=\int_0^\infty f(t)e^{-st}dt
$$
'''),
    ));

    expect(find.textContaining(r'F(s)=\int'), findsOneWidget);
    // Los delimitadores no deben verse.
    expect(find.text(r'$$'), findsNothing);
  });

  testWidgets('sin manejador, las marcas siguen mostrándose', (tester) async {
    await tester.pumpWidget(_envolver(
      const NotasMarkdown(markdown: 'Algo [00:01:00]'),
    ));
    expect(find.text('00:01:00'), findsOneWidget);
  });
}
