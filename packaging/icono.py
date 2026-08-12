#!/usr/bin/env python3
"""Genera el icono de dictar_ia en todos los tamaños que hacen falta.

    python3 packaging/icono.py

El icono se dibuja por código en lugar de guardarse como imagen suelta para
que sea reproducible: cambiar un color o una proporción es editar una línea y
volver a ejecutar, no abrir un editor gráfico y exportar ocho veces.

El concepto: una onda de sonido a la izquierda que se convierte en líneas de
texto a la derecha. Es exactamente lo que hace la aplicación —audio dentro,
apuntes fuera— y se distingue de la marea de iconos de micrófono que usan
todas las grabadoras.

Se dibuja a 4x y se reduce con LANCZOS: es lo que da bordes limpios a 48 px,
que es el tamaño al que de verdad se ve en el menú de aplicaciones.
"""

from pathlib import Path

from PIL import Image, ImageDraw

RAIZ = Path(__file__).resolve().parent.parent

# Índigo a violeta, la misma familia que el color base de la interfaz.
COLOR_A = (61, 90, 254)
COLOR_B = (124, 77, 255)
BLANCO = (255, 255, 255, 255)

# Tamaños del tema de iconos de Linux. 48 es el que se ve en el menú.
TAMANOS_LINUX = [16, 24, 32, 48, 64, 128, 256, 512]

# Densidades de Android, con su tamaño en píxeles.
DENSIDADES_ANDROID = {
    "mdpi": 48,
    "hdpi": 72,
    "xhdpi": 96,
    "xxhdpi": 144,
    "xxxhdpi": 192,
}


def degradado(tam: int) -> Image.Image:
    """Fondo en degradado diagonal."""
    img = Image.new("RGB", (tam, tam))
    px = img.load()
    for y in range(tam):
        for x in range(tam):
            # Diagonal: mezcla según la suma de coordenadas.
            t = (x + y) / (2 * tam - 2)
            px[x, y] = tuple(
                int(a + (b - a) * t) for a, b in zip(COLOR_A, COLOR_B)
            )
    return img


def dibujar(tam: int, simple: bool = False) -> Image.Image:
    """Dibuja el icono al tamaño pedido.

    Con ``simple`` se usa una versión de menos elementos y más gruesos. No es
    lo mismo que reducir: a 16 px, tres líneas de texto separadas por dos
    píxeles se funden en una mancha gris. Simplificar el dibujo es lo único
    que mantiene el icono legible ahí.
    """
    fondo = degradado(tam)

    # Esquinas redondeadas al estilo de los iconos modernos: ~22 % del lado.
    mascara = Image.new("L", (tam, tam), 0)
    ImageDraw.Draw(mascara).rounded_rectangle(
        [0, 0, tam - 1, tam - 1], radius=int(tam * 0.22), fill=255
    )

    img = Image.new("RGBA", (tam, tam), (0, 0, 0, 0))
    img.paste(fondo, (0, 0), mascara)
    d = ImageDraw.Draw(img)

    u = tam / 100.0  # unidad relativa, para que todo escale igual

    # --- Onda: barras verticales de altura desigual --------------------------
    # Pocas y gruesas a propósito: a 48 px, cinco barras finas se convierten en
    # una mancha gris.
    alturas = [44, 30] if simple else [30, 52, 38]
    ancho_barra = (11 if simple else 7) * u
    x = (20 if simple else 22) * u
    centro_y = 50 * u

    for h in alturas:
        alto = h * u
        d.rounded_rectangle(
            [x, centro_y - alto / 2, x + ancho_barra, centro_y + alto / 2],
            radius=ancho_barra / 2,
            fill=BLANCO,
        )
        x += ancho_barra + (6 if simple else 4) * u

    # --- Texto: tres líneas horizontales de distinta longitud ----------------
    # Longitudes desiguales para que se lean como párrafo y no como rejilla.
    largos = [28, 20] if simple else [30, 24, 18]
    grosor = (11 if simple else 7) * u
    hueco = (7 if simple else 4) * u
    x0 = (55 if simple else 55) * u
    y = centro_y - (len(largos) * (grosor + hueco) - hueco) / 2

    for largo in largos:
        d.rounded_rectangle(
            [x0, y, x0 + largo * u, y + grosor],
            radius=grosor / 2,
            fill=BLANCO,
        )
        y += grosor + hueco

    return img


def render(tam: int) -> Image.Image:
    """Dibuja a 4x y reduce: es lo que da bordes limpios en los tamaños chicos.

    Por debajo de 32 px se usa la versión simplificada.
    """
    grande = dibujar(tam * 4, simple=tam < 32)
    return grande.resize((tam, tam), Image.LANCZOS)


def main() -> None:
    # --- Linux ---------------------------------------------------------------
    base = RAIZ / "packaging" / "iconos"
    for tam in TAMANOS_LINUX:
        destino = base / "hicolor" / f"{tam}x{tam}" / "apps"
        destino.mkdir(parents=True, exist_ok=True)
        render(tam).save(destino / "dictar-ia.png")
    print(f"✔ Linux: {len(TAMANOS_LINUX)} tamaños en {base}/hicolor")

    # --- Android -------------------------------------------------------------
    android = RAIZ / "app" / "android" / "app" / "src" / "main" / "res"
    if android.is_dir():
        for densidad, tam in DENSIDADES_ANDROID.items():
            destino = android / f"mipmap-{densidad}"
            destino.mkdir(parents=True, exist_ok=True)
            render(tam).save(destino / "ic_launcher.png")
        print(f"✔ Android: {len(DENSIDADES_ANDROID)} densidades")

    # --- Ventana y documentación --------------------------------------------
    render(512).save(RAIZ / "packaging" / "iconos" / "dictar-ia.png")
    print("✔ 512x512 para la ventana y el README")


if __name__ == "__main__":
    main()
