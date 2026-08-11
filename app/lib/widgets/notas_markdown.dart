import 'package:flutter/material.dart';

/// Renderizador de Markdown a medida, con marcas de tiempo clicables.
///
/// Se escribe a mano en vez de usar `flutter_markdown` por una razón concreta:
/// el valor de estas notas está en poder pinchar `[00:04:15]` y saltar al
/// segundo exacto del audio para verificar lo que afirma la IA. Un renderizador
/// genérico trata esa marca como texto plano, y verificar dejaría de ser un
/// gesto para convertirse en una búsqueda manual.
class NotasMarkdown extends StatelessWidget {
  const NotasMarkdown({
    super.key,
    required this.markdown,
    this.onSaltar,
  });

  final String markdown;

  /// Se invoca con los milisegundos al pulsar una marca de tiempo.
  final void Function(int ms)? onSaltar;

  @override
  Widget build(BuildContext context) {
    final bloques = <Widget>[];
    final lineas = markdown.split('\n');
    var enFormula = false;
    final formula = StringBuffer();

    for (final linea in lineas) {
      final t = linea.trimRight();

      // Bloques $$ ... $$ de LaTeX.
      if (t.trim() == r'$$') {
        if (enFormula) {
          bloques.add(_Formula(formula.toString().trim()));
          formula.clear();
        }
        enFormula = !enFormula;
        continue;
      }
      if (enFormula) {
        formula.writeln(t);
        continue;
      }

      if (t.trim().isEmpty) {
        bloques.add(const SizedBox(height: 10));
      } else if (t.startsWith('### ')) {
        bloques.add(_Titulo(t.substring(4), 3, onSaltar));
      } else if (t.startsWith('## ')) {
        bloques.add(_Titulo(t.substring(3), 2, onSaltar));
      } else if (t.startsWith('# ')) {
        bloques.add(_Titulo(t.substring(2), 1, onSaltar));
      } else if (t.trimLeft().startsWith('> ')) {
        bloques.add(_Cita(t.trimLeft().substring(2), onSaltar));
      } else if (_esVinieta(t)) {
        bloques.add(_Vineta(t, onSaltar));
      } else {
        bloques.add(_Parrafo(t, onSaltar));
      }
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: bloques,
    );
  }

  static bool _esVinieta(String t) {
    final s = t.trimLeft();
    return s.startsWith('- ') || s.startsWith('* ');
  }
}

/// Fragmenta el texto en trozos normales, en negrita y en marca de tiempo.
///
/// `[00:04:15]` y `_[00:04:15]_` se convierten en enlaces; `**x**` en negrita;
/// `_x_` en cursiva.
List<InlineSpan> _fragmentar(
  BuildContext context,
  String texto,
  void Function(int ms)? onSaltar,
) {
  final base = DefaultTextStyle.of(context).style;
  final color = Theme.of(context).colorScheme.primary;
  final spans = <InlineSpan>[];

  final patron = RegExp(
    r'\[(\d{1,2}:\d{2}(?::\d{2})?)\]'   // marca de tiempo
    r'|\*\*(.+?)\*\*'                     // negrita
    r'|_([^_\n]+?)_',                     // cursiva
  );

  var pos = 0;
  for (final m in patron.allMatches(texto)) {
    if (m.start > pos) {
      spans.add(TextSpan(text: texto.substring(pos, m.start)));
    }

    if (m.group(1) != null) {
      final etiqueta = m.group(1)!;
      spans.add(_marca(etiqueta, base, color, onSaltar));
    } else if (m.group(2) != null) {
      spans.add(TextSpan(
        text: m.group(2),
        style: const TextStyle(fontWeight: FontWeight.w700),
      ));
    } else {
      // La cursiva puede contener a su vez una marca de tiempo:
      // `_[00:04:15]_`. Se procesa recursivamente para no perderla.
      final dentro = m.group(3)!;
      final anidada = RegExp(r'^\[(\d{1,2}:\d{2}(?::\d{2})?)\]$').firstMatch(dentro);
      if (anidada != null) {
        spans.add(_marca(anidada.group(1)!, base, color, onSaltar));
      } else {
        spans.add(TextSpan(
          text: dentro,
          style: TextStyle(
            fontStyle: FontStyle.italic,
            color: base.color?.withValues(alpha: 0.7),
          ),
        ));
      }
    }
    pos = m.end;
  }

  if (pos < texto.length) {
    spans.add(TextSpan(text: texto.substring(pos)));
  }
  return spans;
}

InlineSpan _marca(
  String etiqueta,
  TextStyle base,
  Color color,
  void Function(int ms)? onSaltar,
) {
  return WidgetSpan(
    alignment: PlaceholderAlignment.middle,
    child: _MarcaTiempo(etiqueta: etiqueta, color: color, onSaltar: onSaltar),
  );
}

class _MarcaTiempo extends StatelessWidget {
  const _MarcaTiempo({
    required this.etiqueta,
    required this.color,
    this.onSaltar,
  });

  final String etiqueta;
  final Color color;
  final void Function(int ms)? onSaltar;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: 'Escuchar este momento',
      child: InkWell(
        onTap: onSaltar == null ? null : () => onSaltar!(parsear(etiqueta)),
        borderRadius: BorderRadius.circular(4),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 1),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(Icons.play_circle_outline, size: 13, color: color),
              const SizedBox(width: 3),
              Text(
                etiqueta,
                style: TextStyle(
                  color: color,
                  fontSize: 12,
                  fontFeatures: const [FontFeature.tabularFigures()],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  /// `01:02:03` o `02:30` → milisegundos.
  static int parsear(String etiqueta) {
    final partes = etiqueta.split(':').map(int.parse).toList();
    final segundos = partes.fold<int>(0, (acc, n) => acc * 60 + n);
    return segundos * 1000;
  }
}

class _Titulo extends StatelessWidget {
  const _Titulo(this.texto, this.nivel, this.onSaltar);
  final String texto;
  final int nivel;
  final void Function(int ms)? onSaltar;

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context).textTheme;
    final estilo = switch (nivel) {
      1 => t.headlineSmall,
      2 => t.titleLarge,
      _ => t.titleMedium,
    };
    return Padding(
      padding: EdgeInsets.only(top: nivel == 1 ? 4 : 18, bottom: 6),
      child: DefaultTextStyle.merge(
        style: estilo!.copyWith(fontWeight: FontWeight.w700),
        child: Text.rich(TextSpan(children: _fragmentar(context, texto, onSaltar))),
      ),
    );
  }
}

class _Parrafo extends StatelessWidget {
  const _Parrafo(this.texto, this.onSaltar);
  final String texto;
  final void Function(int ms)? onSaltar;

  @override
  Widget build(BuildContext context) => Padding(
        padding: const EdgeInsets.symmetric(vertical: 2),
        child: Text.rich(
          TextSpan(children: _fragmentar(context, texto, onSaltar)),
          style: Theme.of(context).textTheme.bodyMedium?.copyWith(height: 1.45),
        ),
      );
}

class _Vineta extends StatelessWidget {
  const _Vineta(this.linea, this.onSaltar);
  final String linea;
  final void Function(int ms)? onSaltar;

  @override
  Widget build(BuildContext context) {
    final sangria = (linea.length - linea.trimLeft().length) ~/ 2;
    var contenido = linea.trimLeft().substring(2);

    // `- [ ] tarea` se dibuja como casilla: los compromisos se van marcando.
    bool? casilla;
    if (contenido.startsWith('[ ] ')) {
      casilla = false;
      contenido = contenido.substring(4);
    } else if (contenido.startsWith('[x] ')) {
      casilla = true;
      contenido = contenido.substring(4);
    }

    return Padding(
      padding: EdgeInsets.only(left: 4.0 + sangria * 16, top: 2, bottom: 2),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (casilla != null)
            Padding(
              padding: const EdgeInsets.only(right: 6, top: 1),
              child: Icon(
                casilla ? Icons.check_box : Icons.check_box_outline_blank,
                size: 17,
              ),
            )
          else
            const Padding(
              padding: EdgeInsets.only(right: 8, top: 1),
              child: Text('•'),
            ),
          Expanded(
            child: Text.rich(
              TextSpan(children: _fragmentar(context, contenido, onSaltar)),
              style: Theme.of(context).textTheme.bodyMedium?.copyWith(height: 1.4),
            ),
          ),
        ],
      ),
    );
  }
}

class _Cita extends StatelessWidget {
  const _Cita(this.texto, this.onSaltar);
  final String texto;
  final void Function(int ms)? onSaltar;

  @override
  Widget build(BuildContext context) {
    final c = Theme.of(context).colorScheme;
    return Container(
      margin: const EdgeInsets.only(left: 20, top: 4, bottom: 6),
      padding: const EdgeInsets.only(left: 12, top: 4, bottom: 4),
      decoration: BoxDecoration(
        border: Border(left: BorderSide(color: c.primary.withValues(alpha: 0.4), width: 3)),
      ),
      child: Text.rich(
        TextSpan(children: _fragmentar(context, texto, onSaltar)),
        style: Theme.of(context)
            .textTheme
            .bodyMedium
            ?.copyWith(fontStyle: FontStyle.italic, height: 1.4),
      ),
    );
  }
}

/// Fórmula en LaTeX.
///
/// De momento se muestra en monoespaciada. Cuando entre `flutter_math_fork`
/// aquí se sustituye por el renderizado real; el resto del árbol no cambia.
class _Formula extends StatelessWidget {
  const _Formula(this.latex);
  final String latex;

  @override
  Widget build(BuildContext context) {
    final c = Theme.of(context).colorScheme;
    return Container(
      width: double.infinity,
      margin: const EdgeInsets.symmetric(vertical: 8),
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      decoration: BoxDecoration(
        color: c.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(8),
      ),
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: Text(
          latex,
          style: const TextStyle(fontFamily: 'monospace', fontSize: 14, height: 1.4),
        ),
      ),
    );
  }
}
