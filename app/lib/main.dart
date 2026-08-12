import 'package:flutter/material.dart';

import 'datos/repositorio.dart';
import 'datos/repositorio_rust.dart';
import 'pantallas/grabacion.dart';
import 'pantallas/ajustes.dart';
import 'pantallas/inicio.dart';
import 'pantallas/pendientes.dart';
import 'ventana.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await Ventana.preparar();

  // Si el núcleo no carga —falta la librería nativa, o el almacén no se puede
  // abrir— la aplicación arranca igualmente con datos de demostración en vez
  // de quedarse en una pantalla en blanco. Un fallo de infraestructura no
  // debería impedir ver la interfaz.
  Repositorio repo;
  try {
    repo = await RepositorioRust.abrir();
  } catch (e) {
    debugPrint('no se pudo abrir el núcleo ($e); se usan datos de demostración');
    repo = RepositorioDemo();
  }

  runApp(DictarApp(repo: repo));
}

class DictarApp extends StatelessWidget {
  const DictarApp({super.key, required this.repo});

  final Repositorio repo;

  @override
  Widget build(BuildContext context) {
    const semilla = Color(0xFF3D5AFE);

    return MaterialApp(
      title: 'dictar_ia',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: semilla),
        useMaterial3: true,
      ),
      darkTheme: ThemeData(
        colorScheme: ColorScheme.fromSeed(
          seedColor: semilla,
          brightness: Brightness.dark,
        ),
        useMaterial3: true,
      ),
      home: Shell(repo: repo),
    );
  }
}

/// Contenedor con la navegación principal.
///
/// Cambia de riel lateral a barra inferior según el ancho: la misma interfaz
/// sirve para el escritorio, que es donde ocurre casi todo, y para el móvil,
/// que se usa para reuniones presenciales y para consultar.
class Shell extends StatefulWidget {
  const Shell({super.key, required this.repo});

  final Repositorio repo;

  @override
  State<Shell> createState() => _ShellState();
}

class _ShellState extends State<Shell> {
  int _indice = 0;

  static const _destinos = [
    (icono: Icons.mic_none, activo: Icons.mic, etiqueta: 'Sesiones'),
    (icono: Icons.event_note_outlined, activo: Icons.event_note, etiqueta: 'Pendientes'),
    (icono: Icons.settings_outlined, activo: Icons.settings, etiqueta: 'Ajustes'),
  ];

  @override
  void initState() {
    super.initState();
    // Al arrancar, comprobar si alguna sesión quedó a medias por un cierre
    // inesperado. El audio está en disco: nunca se descarta en silencio.
    WidgetsBinding.instance.addPostFrameCallback((_) => _avisarInterrumpidas());
  }

  Future<void> _avisarInterrumpidas() async {
    final pendientes = await widget.repo.interrumpidas();
    if (pendientes.isEmpty || !mounted) return;

    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text(
          '${pendientes.length} grabación(es) quedaron sin cerrar. '
          'El audio está a salvo.',
        ),
        action: SnackBarAction(label: 'Procesar', onPressed: () {}),
        duration: const Duration(seconds: 10),
      ),
    );
  }

  Future<void> _grabar() async {
    await Navigator.of(context).push(
      MaterialPageRoute(builder: (_) => PantallaGrabacion(repo: widget.repo)),
    );
    if (mounted) setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    final ancho = MediaQuery.sizeOf(context).width;
    final esAncho = ancho >= 720;

    final cuerpo = switch (_indice) {
      0 => PantallaInicio(repo: widget.repo),
      1 => PantallaPendientes(repo: widget.repo),
      _ => PantallaAjustes(repo: widget.repo),
    };

    if (esAncho) {
      return Scaffold(
        body: Row(
          children: [
            NavigationRail(
              selectedIndex: _indice,
              onDestinationSelected: (i) => setState(() => _indice = i),
              labelType: NavigationRailLabelType.all,
              leading: Padding(
                padding: const EdgeInsets.symmetric(vertical: 12),
                child: FloatingActionButton(
                  onPressed: _grabar,
                  tooltip: 'Grabar sesión',
                  child: const Icon(Icons.fiber_manual_record),
                ),
              ),
              destinations: [
                for (final d in _destinos)
                  NavigationRailDestination(
                    icon: Icon(d.icono),
                    selectedIcon: Icon(d.activo),
                    label: Text(d.etiqueta),
                  ),
              ],
            ),
            const VerticalDivider(width: 1),
            Expanded(child: cuerpo),
          ],
        ),
      );
    }

    return Scaffold(
      body: cuerpo,
      floatingActionButton: FloatingActionButton(
        onPressed: _grabar,
        tooltip: 'Grabar sesión',
        child: const Icon(Icons.fiber_manual_record),
      ),
      bottomNavigationBar: NavigationBar(
        selectedIndex: _indice,
        onDestinationSelected: (i) => setState(() => _indice = i),
        destinations: [
          for (final d in _destinos)
            NavigationDestination(
              icon: Icon(d.icono),
              selectedIcon: Icon(d.activo),
              label: d.etiqueta,
            ),
        ],
      ),
    );
  }
}
