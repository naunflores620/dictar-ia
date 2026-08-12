import 'dart:io';

import 'package:flutter/material.dart';
import 'package:window_manager/window_manager.dart';

import '../datos/repositorio.dart';
import '../datos/repositorio_rust.dart';

/// Selector del área de la pantalla que se guarda como diapositiva.
///
/// Existe por un motivo de privacidad, no de estética. Sin recorte, una
/// captura de una clase por videollamada incluye la barra de tareas, las
/// pestañas del navegador, la URL de la reunión y **las caras y los nombres de
/// todos los participantes**. Son datos de terceros que nadie ha consentido
/// que se guarden en tu disco ni que se manden a un modelo de IA.
///
/// Se elige una vez y vale para todas las sesiones: el profesor comparte
/// pantalla siempre en el mismo sitio.
class PantallaRegion extends StatefulWidget {
  const PantallaRegion({super.key, required this.repo});

  final Repositorio repo;

  @override
  State<PantallaRegion> createState() => _PantallaRegionState();
}

class _PantallaRegionState extends State<PantallaRegion> {
  String? _imagen;
  int _anchoPantalla = 0;
  int _altoPantalla = 0;
  String? _error;

  Offset? _inicio;
  Offset? _fin;

  @override
  void initState() {
    super.initState();
    _capturar();
  }

  Future<void> _capturar() async {
    final r = widget.repo;
    if (r is! RepositorioRust) {
      setState(() => _error = 'El núcleo no está disponible');
      return;
    }

    try {
      // La propia ventana saldría en la captura y taparía justo la zona que
      // hay que elegir. Se esconde un instante.
      await windowManager.hide();
      await Future<void>.delayed(const Duration(milliseconds: 350));

      final (ruta, w, h) = await r.capturaParaSeleccion();

      await windowManager.show();
      await windowManager.focus();

      if (!mounted) return;
      setState(() {
        _imagen = ruta;
        _anchoPantalla = w;
        _altoPantalla = h;
      });
    } catch (e) {
      await windowManager.show();
      if (mounted) setState(() => _error = '$e');
    }
  }

  Future<void> _guardar(Rect enPantalla) async {
    final r = widget.repo;
    if (r is! RepositorioRust) return;

    await r.guardarRegion(
      x: enPantalla.left.round(),
      y: enPantalla.top.round(),
      ancho: enPantalla.width.round(),
      alto: enPantalla.height.round(),
    );

    if (!mounted) return;
    Navigator.of(context).pop(true);
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text(
          'Área guardada: ${enPantalla.width.round()}×'
          '${enPantalla.height.round()} px',
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context);

    return Scaffold(
      appBar: AppBar(
        title: const Text('Elegir el área de la diapositiva'),
        actions: [
          TextButton(
            onPressed: () async {
              final r = widget.repo;
              if (r is RepositorioRust) await r.guardarRegion();
              if (context.mounted) Navigator.of(context).pop(true);
            },
            child: const Text('Usar la pantalla entera'),
          ),
        ],
      ),
      body: _error != null
          ? Center(child: Text('No se pudo capturar la pantalla:\n$_error'))
          : _imagen == null
              ? const Center(child: CircularProgressIndicator())
              : _selector(t),
    );
  }

  Widget _selector(ThemeData t) {
    return Column(
      children: [
        Container(
          width: double.infinity,
          padding: const EdgeInsets.all(12),
          color: t.colorScheme.surfaceContainerHighest,
          child: Text(
            'Arrastra sobre la zona donde el profesor comparte la diapositiva. '
            'Todo lo de fuera —las cámaras de tus compañeros, tus pestañas, la '
            'barra de tareas— dejará de guardarse.',
            style: t.textTheme.bodySmall,
          ),
        ),
        Expanded(
          child: LayoutBuilder(
            builder: (context, limites) {
              // La imagen se muestra encajada; hay que convertir las
              // coordenadas del widget a las de la pantalla real.
              final escala = (limites.maxWidth / _anchoPantalla)
                  .clamp(0.0, limites.maxHeight / _altoPantalla);
              final anchoVista = _anchoPantalla * escala;
              final altoVista = _altoPantalla * escala;

              return Center(
                child: SizedBox(
                  width: anchoVista,
                  height: altoVista,
                  child: GestureDetector(
                    onPanStart: (d) => setState(() {
                      _inicio = d.localPosition;
                      _fin = d.localPosition;
                    }),
                    onPanUpdate: (d) => setState(() => _fin = d.localPosition),
                    onPanEnd: (_) => setState(() {}),
                    child: Stack(
                      fit: StackFit.expand,
                      children: [
                        Image.file(File(_imagen!), fit: BoxFit.fill),
                        if (_rect != null)
                          CustomPaint(painter: _PintorSeleccion(_rect!, t)),
                      ],
                    ),
                  ),
                ),
              );
            },
          ),
        ),
        SafeArea(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Row(
              children: [
                Expanded(
                  child: Text(
                    _rect == null
                        ? 'Sin selección'
                        : '${_enPantalla!.width.round()}×'
                            '${_enPantalla!.height.round()} px',
                    style: t.textTheme.bodyMedium,
                  ),
                ),
                if (_rect != null)
                  TextButton(
                    onPressed: () => setState(() {
                      _inicio = null;
                      _fin = null;
                    }),
                    child: const Text('Borrar'),
                  ),
                const SizedBox(width: 8),
                FilledButton.icon(
                  // Una selección diminuta suele ser un clic sin querer: mejor
                  // no dejar guardarla que acabar con capturas de un cuadro de
                  // diez píxeles.
                  onPressed: _rect == null || _rect!.width < 40 || _rect!.height < 40
                      ? null
                      : () => _guardar(_enPantalla!),
                  icon: const Icon(Icons.check),
                  label: const Text('Usar esta área'),
                ),
              ],
            ),
          ),
        ),
      ],
    );
  }

  Rect? get _rect {
    if (_inicio == null || _fin == null) return null;
    return Rect.fromPoints(_inicio!, _fin!);
  }

  /// La selección convertida a coordenadas de la pantalla real.
  Rect? get _enPantalla {
    final r = _rect;
    if (r == null) return null;

    final ctx = context.findRenderObject();
    if (ctx is! RenderBox) return null;

    // La escala se recalcula igual que en el LayoutBuilder para no depender de
    // estado compartido entre construcciones.
    final ancho = ctx.size.width;
    final alto = ctx.size.height - 140;
    final escala =
        (ancho / _anchoPantalla).clamp(0.0, alto / _altoPantalla);
    if (escala <= 0) return null;

    return Rect.fromLTWH(
      r.left / escala,
      r.top / escala,
      r.width / escala,
      r.height / escala,
    );
  }
}

/// Dibuja el recuadro elegido y oscurece lo que queda fuera.
class _PintorSeleccion extends CustomPainter {
  _PintorSeleccion(this.rect, this.tema);

  final Rect rect;
  final ThemeData tema;

  @override
  void paint(Canvas canvas, Size size) {
    // Oscurecer fuera de la selección enseña de un vistazo qué se va a
    // descartar, que es justo lo que hay que decidir aquí.
    final fuera = Path.combine(
      PathOperation.difference,
      Path()..addRect(Offset.zero & size),
      Path()..addRect(rect),
    );
    canvas.drawPath(fuera, Paint()..color = Colors.black54);

    canvas.drawRect(
      rect,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2
        ..color = tema.colorScheme.primary,
    );
  }

  @override
  bool shouldRepaint(_PintorSeleccion viejo) => viejo.rect != rect;
}
