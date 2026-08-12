import 'package:flutter/material.dart';

import 'dart:async';

import '../datos/repositorio.dart';
import '../modelos/dominio.dart';
import '../widgets/notas_markdown.dart';
import 'proceso.dart';

/// Detalle de una sesión: las notas y la transcripción, con el audio detrás.
///
/// Las notas van primero y la transcripción en la segunda pestaña. El orden no
/// es cosmético: el 95 % de las veces lo que se quiere son los apuntes, y la
/// transcripción solo se abre para verificar algo concreto.
class PantallaSesion extends StatefulWidget {
  const PantallaSesion({super.key, required this.repo, required this.sesionId});

  final Repositorio repo;
  final String sesionId;

  @override
  State<PantallaSesion> createState() => _PantallaSesionState();
}

class _PantallaSesionState extends State<PantallaSesion> {
  late Future<(Sesion, Notas?, List<Frase>)> _datos;

  /// Posición del reproductor. Al pulsar una marca de tiempo en las notas se
  /// salta aquí, suena el audio desde ese punto y se resalta la frase.
  int _posicionMs = 0;
  int _totalMs = 0;
  bool _sonando = false;
  bool _pausado = false;
  Timer? _reloj;

  @override
  void initState() {
    super.initState();
    _datos = _cargar();
  }

  @override
  void dispose() {
    _reloj?.cancel();
    // Que la música no siga al salir de la pantalla.
    widget.repo.detenerReproduccion();
    super.dispose();
  }

  Future<(Sesion, Notas?, List<Frase>)> _cargar() async {
    final s = await widget.repo.sesion(widget.sesionId);
    final n = await widget.repo.notas(widget.sesionId);
    final t = await widget.repo.transcripcion(widget.sesionId);
    return (s, n, t);
  }

  Future<void> _playPausa() async {
    if (!_sonando) {
      try {
        final dur = await widget.repo.reproducir(widget.sesionId, _posicionMs);
        if (!mounted) return;
        setState(() {
          _sonando = true;
          _pausado = false;
          _totalMs = dur;
        });
        _arrancarReloj();
      } catch (e) {
        if (!mounted) return;
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('No se pudo reproducir: $e')),
        );
      }
      return;
    }

    setState(() => _pausado = !_pausado);
    await widget.repo.pausarReproduccion(_pausado);
  }

  /// Sigue la posición real del audio mientras suena.
  void _arrancarReloj() {
    _reloj?.cancel();
    _reloj = Timer.periodic(const Duration(milliseconds: 300), (_) async {
      final e = await widget.repo.estadoReproduccion();
      if (!mounted || e == null) return;
      setState(() {
        _posicionMs = e.posicionMs;
        _totalMs = e.duracionMs;
        if (e.terminado) {
          _sonando = false;
          _pausado = false;
          _reloj?.cancel();
        }
      });
    });
  }

  /// Salta al instante pedido; si no sonaba, empieza a sonar desde ahí.
  ///
  /// Es lo que da sentido a las marcas de tiempo de los apuntes: pinchar una
  /// afirmación y OÍR el momento exacto en que se dijo.
  Future<void> _saltarA(int ms) async {
    setState(() => _posicionMs = ms);

    if (_sonando) {
      await widget.repo.saltarReproduccion(ms);
      if (_pausado) {
        setState(() => _pausado = false);
        await widget.repo.pausarReproduccion(false);
      }
    } else {
      await _playPausa();
    }
  }

  @override
  Widget build(BuildContext context) {
    return FutureBuilder<(Sesion, Notas?, List<Frase>)>(
      future: _datos,
      builder: (context, snap) {
        if (!snap.hasData) {
          return const Scaffold(body: Center(child: CircularProgressIndicator()));
        }

        final (sesion, notas, frases) = snap.data!;

        return DefaultTabController(
          length: 2,
          child: Scaffold(
            appBar: AppBar(
              title: Text(sesion.tituloMostrado),
              bottom: const TabBar(
                tabs: [
                  Tab(text: 'Notas'),
                  Tab(text: 'Transcripción'),
                ],
              ),
              actions: [
                IconButton(
                  icon: const Icon(Icons.download_outlined),
                  tooltip: 'Exportar a Markdown',
                  onPressed: notas == null ? null : () {},
                ),
              ],
            ),
            body: TabBarView(
              children: [
                _VistaNotas(
                  notas: notas,
                  onSaltar: _saltarA,
                  sesion: sesion,
                  repoProceso: widget.repo,
                ),
                _VistaTranscripcion(frases: frases, posicionMs: _posicionMs),
              ],
            ),
            bottomNavigationBar: _BarraReproductor(
              posicionMs: _posicionMs,
              totalMs: _totalMs != 0
                  ? _totalMs
                  : (sesion.duracion?.inMilliseconds ?? 0),
              sonando: _sonando && !_pausado,
              onPlayPausa: _playPausa,
              onSaltar: _saltarA,
            ),
          ),
        );
      },
    );
  }
}

class _VistaNotas extends StatelessWidget {
  const _VistaNotas({
    required this.notas,
    required this.onSaltar,
    required this.sesion,
    required this.repoProceso,
  });

  final Notas? notas;
  final Sesion sesion;
  final void Function(int ms) onSaltar;
  final Repositorio repoProceso;

  @override
  Widget build(BuildContext context) {
    if (notas == null || notas!.markdown.trim().isEmpty) {
      // Un vacío sin salida deja al usuario mirando una pantalla en blanco:
      // la acción que falta se ofrece aquí mismo.
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(32),
          child: Column(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Text(
                'Esta sesión aún no tiene apuntes.',
                style: Theme.of(context).textTheme.titleMedium,
                textAlign: TextAlign.center,
              ),
              const SizedBox(height: 8),
              Text(
                'El audio está guardado. Puedes escucharlo abajo, o generar '
                'los apuntes ahora.',
                style: Theme.of(context).textTheme.bodyMedium,
                textAlign: TextAlign.center,
              ),
              const SizedBox(height: 20),
              FilledButton.icon(
                onPressed: () => Navigator.of(context).pushReplacement(
                  MaterialPageRoute(
                    builder: (_) => PantallaProceso(
                      repo: repoProceso,
                      sesionId: sesion.id,
                    ),
                  ),
                ),
                icon: const Icon(Icons.auto_awesome),
                label: const Text('Generar apuntes'),
              ),
            ],
          ),
        ),
      );
    }

    return ListView(
      padding: const EdgeInsets.fromLTRB(20, 16, 20, 32),
      children: [
        // Constrained para que el texto no se estire a lo ancho de un monitor:
        // una línea de 200 caracteres es ilegible.
        Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 760),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                NotasMarkdown(markdown: notas!.markdown, onSaltar: onSaltar),
                const SizedBox(height: 28),
                _PieProcedencia(notas: notas!),
              ],
            ),
          ),
        ),
      ],
    );
  }
}

/// Procedencia de las notas.
///
/// Se muestra siempre: saber qué modelo generó un documento y cuánto costó es
/// lo que permite confiar en él, detectar una configuración cara por accidente
/// y comparar calidad cuando se cambia de proveedor.
class _PieProcedencia extends StatelessWidget {
  const _PieProcedencia({required this.notas});

  final Notas notas;

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
      decoration: BoxDecoration(
        color: t.colorScheme.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(8),
      ),
      child: Row(
        children: [
          Icon(Icons.auto_awesome, size: 15, color: t.colorScheme.outline),
          const SizedBox(width: 8),
          Expanded(
            child: Text(
              'Generado con ${notas.modelo} · \$${notas.costeUsd.toStringAsFixed(3)}',
              style: t.textTheme.labelMedium?.copyWith(color: t.colorScheme.outline),
            ),
          ),
          TextButton(onPressed: () {}, child: const Text('Regenerar')),
        ],
      ),
    );
  }
}

class _VistaTranscripcion extends StatelessWidget {
  const _VistaTranscripcion({required this.frases, required this.posicionMs});

  final List<Frase> frases;
  final int posicionMs;

  @override
  Widget build(BuildContext context) {
    if (frases.isEmpty) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(32),
          child: Text(
            'Sin transcripción todavía.\n\n'
            'Se genera junto con los apuntes: usa «Generar apuntes» en la '
            'pestaña de notas. El audio se puede escuchar abajo igualmente.',
            textAlign: TextAlign.center,
            style: Theme.of(context).textTheme.bodyMedium,
          ),
        ),
      );
    }

    return ListView.builder(
      padding: const EdgeInsets.fromLTRB(16, 12, 16, 32),
      itemCount: frases.length,
      itemBuilder: (context, i) {
        final f = frases[i];
        final activa = posicionMs >= f.inicioMs && posicionMs < f.finMs;
        return _FilaFrase(frase: f, resaltada: activa);
      },
    );
  }
}

class _FilaFrase extends StatelessWidget {
  const _FilaFrase({required this.frase, this.resaltada = false});

  final Frase frase;
  final bool resaltada;

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context);
    final esMio = frase.pista == Pista.micro;

    return Container(
      margin: const EdgeInsets.symmetric(vertical: 3),
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
      decoration: BoxDecoration(
        color: resaltada ? t.colorScheme.primaryContainer : null,
        borderRadius: BorderRadius.circular(8),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 62,
            child: Text(
              formatearTs(frase.inicioMs),
              style: t.textTheme.labelSmall?.copyWith(
                color: t.colorScheme.outline,
                fontFeatures: const [FontFeature.tabularFigures()],
              ),
            ),
          ),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  frase.quien,
                  style: t.textTheme.labelSmall?.copyWith(
                    fontWeight: FontWeight.w700,
                    // La pista del micrófono es siempre uno mismo: darle otro
                    // color hace que la conversación se lea de un vistazo.
                    color: esMio ? t.colorScheme.tertiary : t.colorScheme.primary,
                  ),
                ),
                Text(
                  frase.texto,
                  style: t.textTheme.bodyMedium?.copyWith(
                    height: 1.4,
                    // La pasada en vivo es provisional: se atenúa para que se
                    // note que todavía puede cambiar.
                    color: frase.definitiva ? null : t.colorScheme.outline,
                    fontStyle: frase.definitiva ? null : FontStyle.italic,
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _BarraReproductor extends StatelessWidget {
  const _BarraReproductor({
    required this.posicionMs,
    required this.totalMs,
    required this.sonando,
    required this.onPlayPausa,
    required this.onSaltar,
  });

  final int posicionMs;
  final int totalMs;
  final bool sonando;
  final VoidCallback onPlayPausa;
  final void Function(int ms) onSaltar;

  @override
  Widget build(BuildContext context) {
    final progreso =
        totalMs == 0 ? 0.0 : (posicionMs / totalMs).clamp(0.0, 1.0);

    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(12, 4, 16, 8),
        child: Row(
          children: [
            IconButton.filled(
              onPressed: onPlayPausa,
              icon: Icon(sonando ? Icons.pause : Icons.play_arrow),
            ),
            const SizedBox(width: 8),
            Text(formatearTs(posicionMs),
                style: Theme.of(context).textTheme.labelMedium),
            Expanded(
              child: Slider(
                value: progreso,
                // Arrastrar la bolita salta de verdad, no solo dibuja.
                onChanged: totalMs == 0
                    ? null
                    : (v) => onSaltar((v * totalMs).round()),
              ),
            ),
            Text(
              totalMs == 0 ? '--:--' : formatearTs(totalMs),
              style: Theme.of(context).textTheme.labelMedium,
            ),
          ],
        ),
      ),
    );
  }
}
