#!/usr/bin/env bash
# Comprueba el .deb en un Debian limpio.
#
#   ./packaging/linux/verify_deb.sh dist/dictar-ia_0.1.0_amd64.deb
#
# Esta prueba de dos minutos evita el fallo más común del empaquetado en Linux:
# el paquete instala en tu equipo —que tiene medio sistema por otras vías— y
# falla en el del usuario por una dependencia no declarada.

set -euo pipefail

DEB="${1:-}"
if [[ -z "$DEB" || ! -f "$DEB" ]]; then
  echo "uso: $0 <archivo.deb>" >&2
  exit 1
fi

if ! command -v docker >/dev/null 2>&1 && ! command -v podman >/dev/null 2>&1; then
  echo "error: hace falta docker o podman para la comprobación" >&2
  exit 1
fi

MOTOR=$(command -v docker || command -v podman)
DIR=$(cd "$(dirname "$DEB")" && pwd)
NOMBRE=$(basename "$DEB")

echo "→ instalando $NOMBRE en debian:12 limpio"
"$MOTOR" run --rm -v "$DIR:/paq:ro" debian:12 bash -euc "
  apt-get update -qq
  # apt resuelve las dependencias declaradas; si falta alguna, falla aquí,
  # que es exactamente lo que queremos comprobar.
  apt-get install -y -qq /paq/$NOMBRE >/dev/null
  echo '  ✔ instalado'

  test -x /usr/bin/dictar-ia            || { echo '  ✘ falta el lanzador'; exit 1; }
  test -f /usr/share/applications/dictar-ia.desktop \
                                        || { echo '  ✘ falta el .desktop'; exit 1; }
  echo '  ✔ archivos en su sitio'

  # Una librería que no se encuentra en tiempo de ejecución es el otro fallo
  # clásico: el rpath del bundle de Flutter debe resolver lib/ correctamente.
  if ldd /usr/lib/dictar-ia/dictar_ia | grep -q 'not found'; then
    echo '  ✘ hay librerías sin resolver:'
    ldd /usr/lib/dictar-ia/dictar_ia | grep 'not found'
    exit 1
  fi
  echo '  ✔ todas las librerías resuelven'

  apt-get remove -y -qq dictar-ia >/dev/null
  echo '  ✔ se desinstala limpiamente'
"

echo "✔ el paquete pasa la comprobación"
