import 'package:dictar_ia/modelos/dominio.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('formatearTs', () {
    test('omite las horas cuando no llega a una', () {
      expect(formatearTs(0), '0:00');
      expect(formatearTs(65000), '1:05');
    });

    test('incluye las horas en una clase larga', () {
      expect(formatearTs(3671000), '1:01:11');
    });

    test('un valor negativo por un error de reloj no rompe la vista', () {
      expect(formatearTs(-500), '0:00');
    });
  });

  group('consentimiento', () {
    test('las reuniones con cliente lo exigen', () {
      expect(TipoSesion.cliente.requiereConsentimiento, isTrue);
    });

    test('una clase no lo pide en cada sesión', () {
      // El permiso del docente se registra una vez por asignatura: repetirlo
      // cada clase acaba en que el usuario pulsa «sí» sin leer.
      expect(TipoSesion.clase.requiereConsentimiento, isFalse);
    });
  });

  group('plazos de los avisos', () {
    final ahora = DateTime(2026, 8, 11);

    Aviso conFecha(String? f) => Aviso(
          id: 1,
          sesionId: 's',
          tipo: TipoAviso.entrega,
          texto: 'x',
          fecha: f,
          tsMs: 0,
        );

    test('calcula los días que faltan', () {
      expect(conFecha('2026-08-14').diasRestantes(ahora), 3);
      expect(conFecha('2026-08-11').diasRestantes(ahora), 0);
    });

    test('marca como urgente lo que vence esta semana', () {
      expect(conFecha('2026-08-14').esUrgente(ahora), isTrue);
      expect(conFecha('2026-09-15').esUrgente(ahora), isFalse);
    });

    test('detecta lo vencido', () {
      expect(conFecha('2026-08-01').estaVencido(ahora), isTrue);
      expect(conFecha('2026-08-14').estaVencido(ahora), isFalse);
    });

    test('sin fecha no hay plazo, y eso no es un error', () {
      // Una pista de examen informa pero no urge.
      final a = conFecha(null);
      expect(a.diasRestantes(ahora), isNull);
      expect(a.esUrgente(ahora), isFalse);
      expect(a.estaVencido(ahora), isFalse);
    });

    test('una fecha ilegible no rompe el panel', () {
      expect(conFecha('la semana que viene').diasRestantes(ahora), isNull);
    });

    test('una pista de examen no es accionable', () {
      expect(TipoAviso.pistaExamen.esAccionable, isFalse);
      expect(TipoAviso.fechaExamen.esAccionable, isTrue);
    });
  });

  group('estados', () {
    test('los estados intermedios se reconocen como en proceso', () {
      expect(EstadoSesion.transcribiendo.enProceso, isTrue);
      expect(EstadoSesion.resumiendo.enProceso, isTrue);
      expect(EstadoSesion.lista.enProceso, isFalse);
    });

    test('una clave desconocida no lanza excepción', () {
      // La base puede venir de una versión más nueva: degradar es mejor que
      // reventar al abrir la lista de sesiones.
      expect(EstadoSesion.desdeClave('inventado'), EstadoSesion.fallida);
      expect(TipoSesion.desdeClave('inventado'), TipoSesion.equipo);
    });
  });

  group('sesión', () {
    test('usa el tipo como título cuando no hay uno propio', () {
      final s = Sesion(
        id: 's',
        tipo: TipoSesion.clase,
        estado: EstadoSesion.lista,
        inicio: DateTime(2026, 8, 10),
      );
      expect(s.tituloMostrado, 'Clase');
    });

    test('calcula la duración cuando ha terminado', () {
      final s = Sesion(
        id: 's',
        tipo: TipoSesion.clase,
        estado: EstadoSesion.lista,
        inicio: DateTime(2026, 8, 10, 10),
        fin: DateTime(2026, 8, 10, 11, 30),
      );
      expect(s.duracion, const Duration(hours: 1, minutes: 30));
    });
  });
}
