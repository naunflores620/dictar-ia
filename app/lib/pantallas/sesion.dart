import 'package:flutter/material.dart';

import 'dart:async';

import '../datos/participantes.dart';
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

  /// Mientras se regeneran los apuntes: una generación cuesta dinero y no debe
  /// poder lanzarse dos veces con un doble toque.
  bool _regenerando = false;

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

  /// Pide un nombre nuevo para un participante y lo aplica en el núcleo.
  ///
  /// Solo cambia la transcripción: los apuntes ya generados siguen llevando el
  /// nombre anterior hasta que el usuario pulse «Regenerar», y se le dice.
  Future<void> _renombrar(Participante p) async {
    final nuevo = await showDialog<String>(
      context: context,
      builder: (_) => _DialogoRenombrar(participante: p),
    );
    final limpio = nuevo?.trim();
    if (limpio == null || limpio.isEmpty || limpio == p.nombre) return;

    try {
      await widget.repo.renombrarParticipante(
        sesionId: widget.sesionId,
        pista: p.pista,
        de: p.nombre,
        a: limpio,
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('No se pudo renombrar: $e')));
      return;
    }
    if (!mounted) return;
    setState(() {
      _datos = _cargar();
    });
    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(
        content: Text(
          'Renombrado. Los apuntes siguen con el nombre anterior hasta que '
          'los regeneres.',
        ),
      ),
    );
  }

  /// Genera de nuevo los apuntes con la transcripción tal como está ahora.
  Future<void> _regenerar() async {
    if (_regenerando) return;
    setState(() => _regenerando = true);
    try {
      final r = await widget.repo.regenerarNotas(widget.sesionId);
      if (!mounted) return;
      setState(() {
        _datos = _cargar();
      });
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text(
            'Apuntes regenerados · \$${r.costeUsd.toStringAsFixed(4)} '
            'con ${r.proveedor}',
          ),
        ),
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('No se pudieron regenerar los apuntes: $e')),
      );
    } finally {
      if (mounted) setState(() => _regenerando = false);
    }
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
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(SnackBar(content: Text('No se pudo reproducir: $e')));
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
          return const Scaffold(
            body: Center(child: CircularProgressIndicator()),
          );
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
                  onRegenerar: _regenerar,
                  regenerando: _regenerando,
                ),
                _VistaTranscripcion(
                  frases: frases,
                  posicionMs: _posicionMs,
                  onRenombrar: _renombrar,
                ),
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
    required this.onRegenerar,
    required this.regenerando,
  });

  final Notas? notas;
  final Sesion sesion;
  final void Function(int ms) onSaltar;
  final Repositorio repoProceso;
  final VoidCallback onRegenerar;
  final bool regenerando;

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
                    builder: (_) =>
                        PantallaProceso(repo: repoProceso, sesionId: sesion.id),
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
                _PieProcedencia(
                  notas: notas!,
                  onRegenerar: onRegenerar,
                  regenerando: regenerando,
                ),
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
  const _PieProcedencia({
    required this.notas,
    required this.onRegenerar,
    required this.regenerando,
  });

  final Notas notas;
  final VoidCallback onRegenerar;
  final bool regenerando;

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
              style: t.textTheme.labelMedium?.copyWith(
                color: t.colorScheme.outline,
              ),
            ),
          ),
          if (regenerando)
            const SizedBox(
              width: 18,
              height: 18,
              child: CircularProgressIndicator(strokeWidth: 2),
            )
          else
            Tooltip(
              message:
                  'Vuelve a generar los apuntes con la transcripción actual. '
                  'No transcribe de nuevo; cuesta una generación.',
              child: TextButton(
                onPressed: onRegenerar,
                child: const Text('Regenerar'),
              ),
            ),
        ],
      ),
    );
  }
}

class _VistaTranscripcion extends StatelessWidget {
  const _VistaTranscripcion({
    required this.frases,
    required this.posicionMs,
    required this.onRenombrar,
  });

  final List<Frase> frases;
  final int posicionMs;
  final void Function(Participante) onRenombrar;

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
      // El primer elemento es el panel de participantes.
      itemCount: frases.length + 1,
      itemBuilder: (context, i) {
        if (i == 0) {
          return _PanelParticipantes(
            participantes: participantesDe(frases),
            onRenombrar: onRenombrar,
          );
        }
        final f = frases[i - 1];
        final activa = posicionMs >= f.inicioMs && posicionMs < f.finMs;
        return _FilaFrase(frase: f, resaltada: activa);
      },
    );
  }
}

/// Quién habló en la sesión, con el canal y el tiempo, y un lápiz para ponerle
/// nombre. Se deriva de las frases: no hay una lista aparte que mantener.
class _PanelParticipantes extends StatelessWidget {
  const _PanelParticipantes({
    required this.participantes,
    required this.onRenombrar,
  });

  final List<Participante> participantes;
  final void Function(Participante) onRenombrar;

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context);
    return Container(
      margin: const EdgeInsets.only(bottom: 12),
      padding: const EdgeInsets.fromLTRB(14, 10, 8, 6),
      decoration: BoxDecoration(
        color: t.colorScheme.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(8),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('Participantes', style: t.textTheme.titleSmall),
          for (final p in participantes)
            Row(
              children: [
                Icon(
                  p.pista == Pista.micro
                      ? Icons.mic_none
                      : Icons.speaker_phone_outlined,
                  size: 16,
                  color: t.colorScheme.outline,
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Text.rich(
                    TextSpan(
                      children: [
                        TextSpan(
                          text: p.nombre,
                          style: const TextStyle(fontWeight: FontWeight.w700),
                        ),
                        TextSpan(
                          text:
                              '  ·  ${p.pista == Pista.micro ? 'micrófono' : 'sistema'}'
                              '  ·  ${formatearTs(p.tiempoHablado.inMilliseconds)} '
                              'hablados',
                          style: t.textTheme.bodySmall?.copyWith(
                            color: t.colorScheme.outline,
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
                IconButton(
                  icon: const Icon(Icons.edit_outlined, size: 18),
                  tooltip: 'Renombrar a ${p.nombre}',
                  onPressed: () => onRenombrar(p),
                ),
              ],
            ),
        ],
      ),
    );
  }
}

/// Pide el nombre nuevo de un participante.
class _DialogoRenombrar extends StatefulWidget {
  const _DialogoRenombrar({required this.participante});

  final Participante participante;

  @override
  State<_DialogoRenombrar> createState() => _DialogoRenombrarState();
}

class _DialogoRenombrarState extends State<_DialogoRenombrar> {
  late final TextEditingController _campo = TextEditingController(
    text: widget.participante.nombre,
  );

  @override
  void dispose() {
    _campo.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text('Renombrar a ${widget.participante.nombre}'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          TextField(
            controller: _campo,
            autofocus: true,
            decoration: const InputDecoration(labelText: 'Nombre'),
            onSubmitted: (v) => Navigator.of(context).pop(v),
          ),
          const SizedBox(height: 8),
          Text(
            'Si le pones el nombre de otro participante de este canal, '
            'quedan fusionados: sirve para juntar una voz que se partió en dos.',
            style: Theme.of(context).textTheme.bodySmall,
          ),
        ],
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: const Text('Cancelar'),
        ),
        FilledButton(
          onPressed: () => Navigator.of(context).pop(_campo.text),
          child: const Text('Guardar'),
        ),
      ],
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
                    color: esMio
                        ? t.colorScheme.tertiary
                        : t.colorScheme.primary,
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
    final progreso = totalMs == 0
        ? 0.0
        : (posicionMs / totalMs).clamp(0.0, 1.0);

    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(12, 4, 16, 8),
        // La altura va acotada, y no es cosmética: sin esto la pantalla entera
        // se queda en blanco.
        //
        // `Scaffold` da a `bottomNavigationBar` restricciones **holgadas** en
        // vertical —puede ocupar lo que quiera hasta el alto de la ventana— y
        // `Slider` responde a eso estirándose hasta llenarlas. Medido en un
        // test de 800×600: el Slider salía de 588 px de alto, la barra pasaba a
        // ocupar de y=4 a y=600 y al cuerpo del `Scaffold` le quedaba altura
        // **cero**. Las dos pestañas se pintaban vacías aunque hubiera notas y
        // transcripción detrás, y encima la barra quedaba por delante de la
        // `TabBar`, así que tampoco se podía cambiar de pestaña.
        //
        // Nada avisaba: ni una excepción de Flutter, ni un desbordamiento, ni
        // un error del núcleo. Solo una pantalla en blanco.
        child: SizedBox(
          height: 48,
          child: Row(
            children: [
              IconButton.filled(
                onPressed: onPlayPausa,
                icon: Icon(sonando ? Icons.pause : Icons.play_arrow),
              ),
              const SizedBox(width: 8),
              Text(
                formatearTs(posicionMs),
                style: Theme.of(context).textTheme.labelMedium,
              ),
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
      ),
    );
  }
}
