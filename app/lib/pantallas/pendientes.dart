import 'package:flutter/material.dart';

import '../datos/repositorio.dart';
import '../modelos/dominio.dart';
import 'sesion.dart';

/// Panel de pendientes: fechas de examen, entregas y compromisos con clientes,
/// cruzando todas las asignaturas y clientes.
///
/// Es lo que responde a «¿qué tengo pendiente?» sin abrir una sola grabación, y
/// la razón de que los avisos vivan en su propia tabla y no dentro del JSON de
/// cada sesión.
class PantallaPendientes extends StatefulWidget {
  const PantallaPendientes({super.key, required this.repo});

  final Repositorio repo;

  @override
  State<PantallaPendientes> createState() => _PantallaPendientesState();
}

class _PantallaPendientesState extends State<PantallaPendientes> {
  late Future<(List<Aviso>, List<Topic>)> _datos;

  @override
  void initState() {
    super.initState();
    _datos = _cargar();
  }

  Future<(List<Aviso>, List<Topic>)> _cargar() async {
    final a = await widget.repo.pendientes();
    final t = await widget.repo.topics();
    return (a, t);
  }

  Future<void> _descartar(Aviso a) async {
    await widget.repo.descartarAviso(a.id);
    setState(() => _datos = _cargar());
  }

  @override
  Widget build(BuildContext context) {
    final ahora = DateTime.now();

    return Scaffold(
      appBar: AppBar(title: const Text('Pendientes')),
      body: FutureBuilder<(List<Aviso>, List<Topic>)>(
        future: _datos,
        builder: (context, snap) {
          if (!snap.hasData) {
            return const Center(child: CircularProgressIndicator());
          }

          final (avisos, topics) = snap.data!;
          if (avisos.isEmpty) {
            return const Center(child: Text('Nada pendiente. 🎉'));
          }

          // Con fecha primero y en orden; lo que no la tiene —una pista de
          // examen, por ejemplo— informa pero no urge, y va al final.
          final conFecha = avisos.where((a) => a.fecha != null).toList()
            ..sort((a, b) => a.fecha!.compareTo(b.fecha!));
          final sinFecha = avisos.where((a) => a.fecha == null).toList();

          final nombres = {for (final t in topics) t.id: t.nombre};

          return ListView(
            padding: const EdgeInsets.only(bottom: 88),
            children: [
              for (final a in conFecha)
                _FilaAviso(
                  aviso: a,
                  topic: nombres[a.topicId],
                  ahora: ahora,
                  repo: widget.repo,
                  onDescartar: () => _descartar(a),
                ),
              if (sinFecha.isNotEmpty) ...[
                const Padding(
                  padding: EdgeInsets.fromLTRB(16, 24, 16, 8),
                  child: Text('Sin fecha'),
                ),
                for (final a in sinFecha)
                  _FilaAviso(
                    aviso: a,
                    topic: nombres[a.topicId],
                    ahora: ahora,
                    repo: widget.repo,
                    onDescartar: () => _descartar(a),
                  ),
              ],
            ],
          );
        },
      ),
    );
  }
}

class _FilaAviso extends StatelessWidget {
  const _FilaAviso({
    required this.aviso,
    required this.ahora,
    required this.repo,
    required this.onDescartar,
    this.topic,
  });

  final Aviso aviso;
  final String? topic;
  final DateTime ahora;
  final Repositorio repo;
  final VoidCallback onDescartar;

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context);
    final dias = aviso.diasRestantes(ahora);
    final urgente = aviso.esUrgente(ahora);
    final vencido = aviso.estaVencido(ahora);

    final colorPlazo = vencido
        ? t.colorScheme.error
        : urgente
            ? t.colorScheme.tertiary
            : t.colorScheme.outline;

    return Dismissible(
      key: ValueKey(aviso.id),
      direction: DismissDirection.endToStart,
      onDismissed: (_) => onDescartar(),
      background: Container(
        alignment: Alignment.centerRight,
        padding: const EdgeInsets.only(right: 24),
        color: t.colorScheme.surfaceContainerHighest,
        child: const Icon(Icons.check),
      ),
      child: ListTile(
        leading: Icon(_icono(aviso.tipo), color: colorPlazo),
        title: Text(aviso.texto),
        subtitle: Row(
          children: [
            if (topic != null) ...[
              Text(topic!),
              const Text(' · '),
            ],
            Text(aviso.tipo.etiqueta),
          ],
        ),
        trailing: dias == null
            ? null
            : Column(
                mainAxisAlignment: MainAxisAlignment.center,
                crossAxisAlignment: CrossAxisAlignment.end,
                children: [
                  Text(
                    _plazo(dias),
                    style: t.textTheme.labelLarge?.copyWith(
                      color: colorPlazo,
                      fontWeight: urgente || vencido ? FontWeight.w700 : null,
                    ),
                  ),
                  Text(aviso.fecha!, style: t.textTheme.labelSmall),
                ],
              ),
        // Cada aviso lleva su ts_ms: se puede saltar al audio y comprobar que
        // el profesor dijo exactamente eso.
        onTap: () => Navigator.of(context).push(
          MaterialPageRoute(
            builder: (_) => PantallaSesion(repo: repo, sesionId: aviso.sesionId),
          ),
        ),
      ),
    );
  }

  static String _plazo(int dias) {
    if (dias < 0) return 'vencido';
    if (dias == 0) return 'hoy';
    if (dias == 1) return 'mañana';
    return 'en $dias d';
  }

  static IconData _icono(TipoAviso t) => switch (t) {
        TipoAviso.fechaExamen => Icons.event_outlined,
        TipoAviso.entrega => Icons.assignment_outlined,
        TipoAviso.pistaExamen => Icons.push_pin_outlined,
        TipoAviso.cambioAula => Icons.meeting_room_outlined,
        TipoAviso.compromisoMio => Icons.assignment_ind_outlined,
        TipoAviso.compromisoSuyo => Icons.person_outline,
      };
}
