//! Conversión de marcas de tiempo a fecha civil.
//!
//! Existe por un fallo concreto y caro: sin decirle al modelo en qué fecha
//! ocurrió la sesión, «el parcial es el 15 de septiembre» se convierte en un
//! año inventado. Y una fecha de examen equivocada es peor que ninguna fecha,
//! porque el usuario se fía de ella.
//!
//! Se implementa a mano en vez de añadir una dependencia de calendario: son
//! doce líneas del algoritmo de días civiles de Howard Hinnant, y el núcleo no
//! necesita zonas horarias ni aritmética de fechas más allá de esto.

use dictar_domain::EpochMs;

/// Fecha UTC en ISO-8601 (`2026-08-11`) a partir de milisegundos de época.
pub fn iso(epoch_ms: EpochMs) -> String {
    let dias = epoch_ms.div_euclid(86_400_000);
    let (a, m, d) = civil_desde_dias(dias);
    format!("{a:04}-{m:02}-{d:02}")
}

/// Algoritmo de Howard Hinnant: días desde 1970-01-01 → (año, mes, día).
fn civil_desde_dias(dias: i64) -> (i64, u32, u32) {
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Fecha de hoy en ISO-8601. Para la CLI y para valores por defecto.
pub fn hoy() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    iso(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_epoca_es_el_primero_de_enero_de_1970() {
        assert_eq!(iso(0), "1970-01-01");
    }

    #[test]
    fn convierte_fechas_conocidas() {
        // 2026-08-11T00:00:00Z
        assert_eq!(iso(1_786_406_400_000), "2026-08-11");
        // 2000-03-01, justo después de un 29 de febrero bisiesto.
        assert_eq!(iso(951_868_800_000), "2000-03-01");
    }

    #[test]
    fn maneja_el_ano_bisiesto() {
        // 2024-02-29 existe; 2100-02-29 no, porque 2100 no es bisiesto.
        assert_eq!(iso(1_709_164_800_000), "2024-02-29");
    }

    #[test]
    fn una_fecha_anterior_a_1970_no_rompe_la_conversion() {
        // div_euclid en vez de división normal: con `/` los negativos
        // redondearían hacia cero y daría un día de más.
        assert_eq!(iso(-86_400_000), "1969-12-31");
    }

    #[test]
    fn hoy_devuelve_una_fecha_con_forma_valida() {
        let h = hoy();
        assert_eq!(h.len(), 10);
        assert_eq!(h.matches('-').count(), 2);
        let anio: i64 = h[..4].parse().unwrap();
        assert!(anio >= 2024, "el año {anio} no tiene sentido");
    }
}
