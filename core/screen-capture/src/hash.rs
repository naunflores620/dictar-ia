//! Hash perceptual para detectar cambios de diapositiva.
//!
//! Se usa **dHash** (hash de diferencias) y no un hash criptográfico: aquí no
//! interesa si dos capturas son idénticas byte a byte —nunca lo son, porque el
//! cursor se mueve y el vídeo del profesor cambia— sino si *se parecen*.
//!
//! dHash reduce la imagen a 9×8 en gris y compara cada píxel con su vecino de
//! la derecha: 64 comparaciones, 64 bits. Es robusto frente a cambios de brillo
//! y a compresión, y sensible a que cambie la estructura de lo que se ve, que
//! es exactamente lo que distingue una diapositiva de la siguiente.

use image::{imageops::FilterType, DynamicImage, GenericImageView};

/// Ancho del muestreo: una columna más que el alto, para las 8 comparaciones.
const ANCHO: u32 = 9;
const ALTO: u32 = 8;

/// Calcula el dHash de una imagen.
pub fn dhash(img: &DynamicImage) -> u64 {
    let pequena = img
        .resize_exact(ANCHO, ALTO, FilterType::Triangle)
        .to_luma8();

    let mut bits = 0u64;
    let mut i = 0;

    for y in 0..ALTO {
        for x in 0..(ANCHO - 1) {
            let izq = pequena.get_pixel(x, y).0[0];
            let der = pequena.get_pixel(x + 1, y).0[0];
            if izq > der {
                bits |= 1 << i;
            }
            i += 1;
        }
    }

    bits
}

/// Número de bits distintos entre dos hashes.
pub fn distancia(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

pub fn a_hex(h: u64) -> String {
    format!("{h:016x}")
}

pub fn desde_hex(s: &str) -> Option<u64> {
    u64::from_str_radix(s, 16).ok()
}

/// Recorta la región central de la imagen.
///
/// Es lo que evita que la miniatura de la webcam del profesor, que está en una
/// esquina y se mueve constantemente, cuente como diapositiva nueva. También
/// deja fuera las barras de herramientas de Meet o Zoom.
///
/// `margen` es la fracción que se recorta por cada lado, de 0 a 0,4.
pub fn region_central(img: &DynamicImage, margen: f32) -> DynamicImage {
    let m = margen.clamp(0.0, 0.4);
    let (w, h) = img.dimensions();

    let dx = (w as f32 * m) as u32;
    let dy = (h as f32 * m) as u32;
    let nw = w.saturating_sub(dx * 2).max(1);
    let nh = h.saturating_sub(dy * 2).max(1);

    img.crop_imm(dx, dy, nw, nh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn lienzo(w: u32, h: u32, color: [u8; 3]) -> DynamicImage {
        let mut img = RgbaImage::new(w, h);
        for p in img.pixels_mut() {
            *p = Rgba([color[0], color[1], color[2], 255]);
        }
        DynamicImage::ImageRgba8(img)
    }

    /// Imagen con una banda vertical, para simular contenido con estructura.
    fn con_banda(w: u32, h: u32, x0: u32, ancho: u32) -> DynamicImage {
        let mut img = RgbaImage::new(w, h);
        for (x, _y, p) in img.enumerate_pixels_mut() {
            let claro = x >= x0 && x < x0 + ancho;
            let v = if claro { 240 } else { 20 };
            *p = Rgba([v, v, v, 255]);
        }
        DynamicImage::ImageRgba8(img)
    }

    #[test]
    fn la_misma_imagen_da_el_mismo_hash() {
        let a = con_banda(400, 300, 100, 80);
        let b = con_banda(400, 300, 100, 80);
        assert_eq!(dhash(&a), dhash(&b));
        assert_eq!(distancia(dhash(&a), dhash(&b)), 0);
    }

    #[test]
    fn dos_diapositivas_distintas_dan_hashes_lejanos() {
        let a = con_banda(400, 300, 40, 60);
        let b = con_banda(400, 300, 260, 60);
        let d = distancia(dhash(&a), dhash(&b));
        assert!(d > 8, "distancia {d}: deberían parecerse poco");
    }

    #[test]
    fn un_cambio_de_escala_no_altera_el_hash() {
        // La misma diapositiva capturada a otra resolución sigue siendo la
        // misma: si el hash cambiara, un redimensionado de ventana generaría
        // una lámina falsa.
        let grande = con_banda(1600, 900, 400, 200);
        let pequena = con_banda(800, 450, 200, 100);
        let d = distancia(dhash(&grande), dhash(&pequena));
        assert!(d <= 4, "distancia {d} entre la misma imagen a dos tamaños");
    }

    #[test]
    fn el_hexadecimal_va_y_vuelve() {
        let h = 0xdead_beef_1234_5678u64;
        assert_eq!(desde_hex(&a_hex(h)), Some(h));
        assert_eq!(a_hex(h).len(), 16);
    }

    #[test]
    fn un_hexadecimal_invalido_no_se_confunde_con_cero() {
        // Devolver 0 haría que una lámina corrupta pareciera idéntica a otra
        // igual de corrupta, y se descartarían las dos.
        assert_eq!(desde_hex("no-es-hex"), None);
    }

    #[test]
    fn la_region_central_recorta_por_los_cuatro_lados() {
        let img = lienzo(1000, 800, [128, 128, 128]);
        let c = region_central(&img, 0.1);
        assert_eq!(c.dimensions(), (800, 640));
    }

    #[test]
    fn un_margen_excesivo_se_limita_en_vez_de_vaciar_la_imagen() {
        let img = lienzo(1000, 800, [128, 128, 128]);
        let c = region_central(&img, 0.9);
        let (w, h) = c.dimensions();
        assert!(w > 0 && h > 0, "quedó {w}x{h}");
    }

    #[test]
    fn un_cambio_solo_en_la_esquina_no_altera_la_region_central() {
        // Es el caso de la webcam del profesor: cambia constantemente y no
        // debería contar como diapositiva nueva.
        let mut a = con_banda(1000, 800, 300, 200).to_rgba8();
        let base = DynamicImage::ImageRgba8(a.clone());

        // Se altera solo una esquina, fuera del 80 % central.
        for y in 0..60 {
            for x in 0..60 {
                a.put_pixel(x, y, Rgba([255, 0, 0, 255]));
            }
        }
        let con_webcam = DynamicImage::ImageRgba8(a);

        let d = distancia(
            dhash(&region_central(&base, 0.12)),
            dhash(&region_central(&con_webcam, 0.12)),
        );
        assert_eq!(d, 0, "la esquina no debe influir en la región central");
    }
}
