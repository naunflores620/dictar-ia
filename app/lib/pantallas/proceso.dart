import 'dart:async';

import 'package:flutter/material.dart';

import '../datos/repositorio.dart';
import 'sesion.dart';

/// Transcribe una sesión y genera sus notas, enseñando el avance.
///
/// Es una pantalla propia y no un diálogo porque tarda minutos: una clase de
/// dos horas son unos veintiséis a la velocidad medida. Un diálogo modal que
/// bloquea media hora sería insoportable; una pantalla deja claro que la
/// aplicación está trabajando y que se puede cerrar sin perder nada.
class PantallaProceso extends StatefulWidget {
  const PantallaProceso({
    super.key,
    required this.repo,
    required this.sesionId,
  });

  final Repositorio repo;
  final String sesionId;

  @override
  State<PantallaProceso> createState() => _PantallaProcesoState();
}

class _PantallaProcesoState extends State<PantallaProceso> {
  Progreso? _progreso;
  StreamSubscription<Progreso>? _sub;
  Object? _fallo;

  @override
  void initState() {
    super.initState();
    _sub = widget.repo.progreso.listen((p) {
      if (mounted) setState(() => _progreso = p);
    });
    _procesar();
  }

  @override
  void dispose() {
    _sub?.cancel();
    super.dispose();
  }

  Future<void> _procesar() async {
    try {
      final r = await widget.repo.procesar(widget.sesionId);
      if (!mounted) return;

      // Se sustituye la pantalla en lugar de apilarla: volver atrás desde los
      // apuntes debe llevar al listado, no a una barra de progreso terminada.
      Navigator.of(context).pushReplacement(
        MaterialPageRoute(
          builder: (_) =>
              PantallaSesion(repo: widget.repo, sesionId: widget.sesionId),
        ),
      );
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text(
            '${r.frases} frases · ${r.avisos} aviso(s) · '
            '\$${r.costeUsd.toStringAsFixed(4)} con ${r.proveedor}',
          ),
        ),
      );
    } catch (e) {
      if (mounted) setState(() => _fallo = e);
    }
  }

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context);

    return Scaffold(
      appBar: AppBar(
        title: const Text('Generando apuntes'),
        // Durante el procesado no hay botón de cerrar: salir a medias dejaría
        // la sesión sin notas y sin decir por qué. Con el fallo en pantalla sí
        // se puede volver.
        automaticallyImplyLeading: _fallo != null,
      ),
      body: Center(
        child: Padding(
          padding: const EdgeInsets.all(32),
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 420),
            child: _fallo != null ? _vistaFallo(t) : _vistaProgreso(t),
          ),
        ),
      ),
    );
  }

  Widget _vistaProgreso(ThemeData t) {
    final p = _progreso;

    return Column(
      mainAxisAlignment: MainAxisAlignment.center,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          p?.fase ?? 'Preparando…',
          style: t.textTheme.titleMedium,
          textAlign: TextAlign.center,
        ),
        const SizedBox(height: 16),
        // Sin progreso conocido se muestra indeterminada en vez de fingir un
        // porcentaje: una barra que miente es peor que una que no sabe.
        LinearProgressIndicator(
          value: (p?.fraccion ?? 0) > 0 ? p!.fraccion : null,
          minHeight: 6,
        ),
        const SizedBox(height: 12),
        Text(
          p?.detalle ?? '',
          style: t.textTheme.bodySmall?.copyWith(color: t.colorScheme.outline),
          textAlign: TextAlign.center,
        ),
        const SizedBox(height: 32),
        Text(
          'La transcripción va a unas 4,6 veces el tiempo real: una clase de '
          'dos horas tarda alrededor de media hora.\n\n'
          'El audio ya está guardado, así que puedes cerrar la aplicación y '
          'retomarlo después desde el listado.',
          style: t.textTheme.bodySmall
              ?.copyWith(color: t.colorScheme.outline, height: 1.5),
          textAlign: TextAlign.center,
        ),
      ],
    );
  }

  Widget _vistaFallo(ThemeData t) {
    return Column(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        Icon(Icons.error_outline, size: 48, color: t.colorScheme.error),
        const SizedBox(height: 16),
        Text('No se pudieron generar las notas', style: t.textTheme.titleMedium),
        const SizedBox(height: 12),
        Text(
          '$_fallo',
          style: t.textTheme.bodySmall?.copyWith(color: t.colorScheme.outline),
          textAlign: TextAlign.center,
        ),
        const SizedBox(height: 24),
        // El audio está en disco pase lo que pase, así que reintentar es
        // siempre una opción y conviene decirlo.
        Text(
          'El audio está guardado. Puedes reintentarlo.',
          style: t.textTheme.bodyMedium,
          textAlign: TextAlign.center,
        ),
        const SizedBox(height: 16),
        FilledButton.icon(
          onPressed: () {
            setState(() {
              _fallo = null;
              _progreso = null;
            });
            _procesar();
          },
          icon: const Icon(Icons.refresh),
          label: const Text('Reintentar'),
        ),
      ],
    );
  }
}
