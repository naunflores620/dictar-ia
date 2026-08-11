//! Exportación de notas a Markdown.
//!
//! Vive en el núcleo y no en la interfaz porque lo usan tres sitios: la
//! exportación de la aplicación, el envío de un acta a un cliente y la CLI.

use dictar_domain::notes::NotesPayload;
use dictar_domain::{ClassNotes, ClientMinutes, TeamNotes, TsMs};

/// Convierte una marca de tiempo en `hh:mm:ss`, para que sea legible y
/// clicable en la interfaz.
pub fn ts(ms: TsMs) -> String {
    let s = ms.max(0) / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

pub fn render(payload: &NotesPayload, titulo: &str) -> String {
    match payload {
        NotesPayload::Class(n) => clase(n, titulo),
        NotesPayload::Client(n) => cliente(n, titulo),
        NotesPayload::Team(n) => equipo(n, titulo),
    }
}

fn cabecera(titulo: &str, tldr: &str) -> String {
    format!("# {titulo}\n\n{tldr}\n")
}

fn clase(n: &ClassNotes, titulo: &str) -> String {
    let mut o = cabecera(titulo, &n.tldr);

    // Los avisos van arriba del todo, antes que el contenido académico: una
    // fecha de examen que el profesor dijo una sola vez es lo que más se pierde
    // y lo que más cuesta perder.
    if !n.avisos.is_empty() {
        o.push_str("\n## ⚠️ Avisos\n\n");
        for a in &n.avisos {
            let fecha = a
                .fecha
                .as_deref()
                .map(|f| format!(" — **{f}**"))
                .unwrap_or_default();
            o.push_str(&format!("- [{}] {}{}\n", ts(a.ts_ms), a.texto, fecha));
        }
    }

    if !n.senales_examen.is_empty() {
        o.push_str("\n## 📌 Marcado como importante\n\n");
        for s in &n.senales_examen {
            o.push_str(&format!(
                "- **{}** — «{}» _[{}]_\n",
                s.tema,
                s.cita,
                ts(s.ts_ms)
            ));
        }
    }

    if !n.temas.is_empty() {
        o.push_str("\n## Contenido\n");
        for t in &n.temas {
            o.push_str(&format!(
                "\n### {} _[{} – {}]_\n\n",
                t.titulo,
                ts(t.inicio_ms),
                ts(t.fin_ms)
            ));
            for p in &t.puntos {
                o.push_str(&format!("- {p}\n"));
            }
        }
    }

    if !n.conceptos.is_empty() {
        o.push_str("\n## Conceptos\n\n");
        for c in &n.conceptos {
            let duda = if c.incierto {
                " ⚠️_(transcripción dudosa)_"
            } else {
                ""
            };
            o.push_str(&format!(
                "- **{}**{}: {} _[{}]_\n",
                c.termino,
                duda,
                c.definicion,
                ts(c.ts_ms)
            ));
            if let Some(e) = &c.ejemplo {
                o.push_str(&format!("  - _Ejemplo:_ {e}\n"));
            }
        }
    }

    if !n.formulas.is_empty() {
        o.push_str("\n## Fórmulas\n\n");
        for f in &n.formulas {
            o.push_str(&format!("$$\n{}\n$$\n", f.expresion));
            if let Some(s) = &f.significado {
                o.push_str(&format!("{s}\n"));
            }
            if let Some(id) = f.slide_id {
                o.push_str(&format!("_Diapositiva #{id}_ · "));
            }
            o.push_str(&format!("_[{}]_\n\n", ts(f.ts_ms)));
        }
    }

    if !n.dudas.is_empty() {
        o.push_str("\n## Preguntas en clase\n\n");
        for d in &n.dudas {
            o.push_str(&format!("- **P:** {} _[{}]_\n", d.pregunta, ts(d.ts_ms)));
            if let Some(r) = &d.respuesta {
                o.push_str(&format!("  **R:** {r}\n"));
            }
        }
    }

    if !n.bibliografia.is_empty() {
        o.push_str("\n## Bibliografía\n\n");
        for b in &n.bibliografia {
            o.push_str(&format!("- {} _[{}]_\n", b.referencia, ts(b.ts_ms)));
        }
    }

    o
}

fn cliente(n: &ClientMinutes, titulo: &str) -> String {
    let mut o = cabecera(titulo, &n.tldr);

    let c = &n.contexto_cliente;
    if c.empresa.is_some() || c.sector.is_some() || !c.herramientas.is_empty() {
        o.push_str("\n## Cliente\n\n");
        for (etiqueta, valor) in [
            ("Empresa", c.empresa.as_deref()),
            ("Sector", c.sector.as_deref()),
            ("Tamaño", c.tamano.as_deref()),
        ] {
            if let Some(v) = valor {
                o.push_str(&format!("- **{etiqueta}:** {v}\n"));
            }
        }
        if !c.herramientas.is_empty() {
            o.push_str(&format!(
                "- **Herramientas:** {}\n",
                c.herramientas.join(", ")
            ));
        }
    }

    if !n.necesidades.is_empty() {
        o.push_str("\n## Necesidades detectadas\n\n");
        for x in &n.necesidades {
            o.push_str(&format!(
                "- {} _({:?})_ _[{}]_\n",
                x.texto,
                x.prioridad,
                ts(x.ts_ms)
            ));
            if let Some(cita) = &x.cita {
                o.push_str(&format!("  > {cita}\n"));
            }
        }
    }

    if !n.objeciones.is_empty() {
        o.push_str("\n## Objeciones\n\n");
        for x in &n.objeciones {
            // Las no resueltas se marcan: son las que hay que preparar para la
            // próxima reunión, y perderlas cuesta el trato.
            let marca = if x.resuelta { "✅" } else { "❗" };
            o.push_str(&format!("- {} {} _[{}]_\n", marca, x.objecion, ts(x.ts_ms)));
            if let Some(r) = &x.respuesta {
                o.push_str(&format!("  - _Respuesta:_ {r}\n"));
            }
        }
    }

    if let Some(p) = &n.presupuesto {
        o.push_str(&format!(
            "\n## Presupuesto\n\n{} _[{}]_\n",
            p.texto,
            ts(p.ts_ms)
        ));
    }

    if !n.acuerdos.is_empty() {
        o.push_str("\n## Acuerdos\n\n");
        for a in &n.acuerdos {
            o.push_str(&format!("- {} _[{}]_\n", a.texto, ts(a.ts_ms)));
        }
    }

    o.push_str("\n## Compromisos\n\n");
    o.push_str("### Míos\n\n");
    if n.compromisos.mios.is_empty() {
        o.push_str("_Ninguno registrado._\n");
    }
    for c in &n.compromisos.mios {
        let f = c
            .fecha
            .as_deref()
            .map(|f| format!(" — **{f}**"))
            .unwrap_or_default();
        o.push_str(&format!("- [ ] {}{} _[{}]_\n", c.texto, f, ts(c.ts_ms)));
    }

    o.push_str("\n### Del cliente\n\n");
    if n.compromisos.del_cliente.is_empty() {
        o.push_str("_Ninguno registrado._\n");
    }
    for c in &n.compromisos.del_cliente {
        let f = c
            .fecha
            .as_deref()
            .map(|f| format!(" — **{f}**"))
            .unwrap_or_default();
        o.push_str(&format!("- [ ] {}{} _[{}]_\n", c.texto, f, ts(c.ts_ms)));
    }

    if let Some(p) = &n.proximo_paso {
        let f = p
            .fecha
            .as_deref()
            .map(|f| format!(" ({f})"))
            .unwrap_or_default();
        o.push_str(&format!("\n## Próximo paso\n\n**{}**{}\n", p.texto, f));
    }

    if !n.senales.is_empty() {
        o.push_str("\n## Señales\n\n");
        for s in &n.senales {
            o.push_str(&format!(
                "- _{:?}_: «{}» _[{}]_\n",
                s.tipo,
                s.cita,
                ts(s.ts_ms)
            ));
        }
    }

    o
}

fn equipo(n: &TeamNotes, titulo: &str) -> String {
    let mut o = cabecera(titulo, &n.tldr);

    if !n.temas.is_empty() {
        o.push_str("\n## Temas\n");
        for t in &n.temas {
            o.push_str(&format!(
                "\n### {} _[{} – {}]_\n\n",
                t.titulo,
                ts(t.inicio_ms),
                ts(t.fin_ms)
            ));
            for p in &t.puntos {
                o.push_str(&format!("- {p}\n"));
            }
        }
    }

    if !n.decisiones.is_empty() {
        o.push_str("\n## Decisiones\n\n");
        for d in &n.decisiones {
            o.push_str(&format!("- {} _[{}]_\n", d.texto, ts(d.ts_ms)));
        }
    }

    if !n.tareas.is_empty() {
        o.push_str("\n## Tareas\n\n");
        for t in &n.tareas {
            let quien = t
                .responsable
                .as_deref()
                .map(|r| format!("**{r}** — "))
                .unwrap_or_default();
            let cuando = t
                .fecha
                .as_deref()
                .map(|f| format!(" _({f})_"))
                .unwrap_or_default();
            o.push_str(&format!(
                "- [ ] {}{}{} _[{}]_\n",
                quien,
                t.texto,
                cuando,
                ts(t.ts_ms)
            ));
        }
    }

    if !n.abierto.is_empty() {
        o.push_str("\n## Sin cerrar\n\n");
        for a in &n.abierto {
            o.push_str(&format!("- {} _[{}]_\n", a.texto, ts(a.ts_ms)));
        }
    }

    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use dictar_domain::notes::*;

    #[test]
    fn el_tiempo_se_formatea_como_reloj() {
        assert_eq!(ts(0), "00:00:00");
        assert_eq!(ts(3_671_000), "01:01:11");
        // Un valor negativo por un error de reloj no debe romper el documento.
        assert_eq!(ts(-5), "00:00:00");
    }

    fn apuntes() -> ClassNotes {
        ClassNotes {
            tldr: "Transformadas de Laplace.".into(),
            temas: vec![Tema {
                titulo: "Definición".into(),
                puntos: vec!["Integral impropia".into()],
                inicio_ms: 0,
                fin_ms: 600_000,
            }],
            conceptos: vec![Concepto {
                termino: "Región de convergencia".into(),
                definicion: "Donde la integral converge".into(),
                ejemplo: None,
                incierto: true,
                ts_ms: 120_000,
            }],
            formulas: vec![Formula {
                expresion: r"F(s)=\int_0^\infty f(t)e^{-st}dt".into(),
                significado: Some("Definición".into()),
                slide_id: Some(7),
                ts_ms: 90_000,
            }],
            avisos: vec![Aviso {
                tipo: AvisoKind::Examen,
                texto: "Parcial del tema 3".into(),
                fecha: Some("2026-09-15".into()),
                ts_ms: 2_400_000,
            }],
            senales_examen: vec![SenalExamen {
                tema: "Convergencia".into(),
                cita: "esto cae seguro".into(),
                ts_ms: 1_800_000,
            }],
            dudas: vec![],
            bibliografia: vec![],
            glosario_nuevo: vec![],
        }
    }

    #[test]
    fn los_avisos_van_antes_que_el_contenido_academico() {
        let md = render(&NotesPayload::Class(apuntes()), "Cálculo II");
        let pos_avisos = md.find("Avisos").unwrap();
        let pos_contenido = md.find("## Contenido").unwrap();
        assert!(
            pos_avisos < pos_contenido,
            "la fecha de examen es lo que más cuesta perder: va arriba"
        );
    }

    #[test]
    fn la_fecha_del_aviso_queda_destacada() {
        let md = render(&NotesPayload::Class(apuntes()), "Cálculo II");
        assert!(md.contains("**2026-09-15**"));
    }

    #[test]
    fn un_concepto_dudoso_se_marca_para_que_el_usuario_lo_verifique() {
        let md = render(&NotesPayload::Class(apuntes()), "x");
        assert!(md.contains("transcripción dudosa"));
    }

    #[test]
    fn las_formulas_salen_en_bloque_latex() {
        let md = render(&NotesPayload::Class(apuntes()), "x");
        assert!(md.contains("$$"));
        assert!(md.contains("Diapositiva #7"));
    }

    #[test]
    fn el_acta_de_cliente_separa_los_compromisos_y_marca_lo_no_resuelto() {
        let acta = ClientMinutes {
            tldr: "Enviaremos propuesta.".into(),
            contexto_cliente: ContextoCliente {
                empresa: Some("Acme".into()),
                ..Default::default()
            },
            necesidades: vec![],
            objeciones: vec![Objecion {
                objecion: "El precio parece alto".into(),
                respuesta: None,
                resuelta: false,
                ts_ms: 60_000,
            }],
            presupuesto: None,
            acuerdos: vec![],
            compromisos: Compromisos {
                mios: vec![Compromiso {
                    texto: "Enviar propuesta".into(),
                    fecha: Some("2026-08-20".into()),
                    ts_ms: 100,
                }],
                del_cliente: vec![],
            },
            proximo_paso: None,
            senales: vec![],
        };

        let md = render(&NotesPayload::Client(acta), "Acme S.A.");
        assert!(md.contains("### Míos"));
        assert!(md.contains("### Del cliente"));
        assert!(md.contains("_Ninguno registrado._"));
        // Una objeción sin resolver debe verse a simple vista.
        assert!(md.contains("❗ El precio parece alto"));
        // Los compromisos son casillas: se pueden ir marcando.
        assert!(md.contains("- [ ] Enviar propuesta"));
    }

    #[test]
    fn un_documento_vacio_no_genera_secciones_huecas() {
        let vacio = TeamNotes {
            tldr: "Reunión breve.".into(),
            temas: vec![],
            decisiones: vec![],
            tareas: vec![],
            abierto: vec![],
        };
        let md = render(&NotesPayload::Team(vacio), "Semanal");
        assert!(!md.contains("## Tareas"));
        assert!(!md.contains("## Decisiones"));
        assert!(md.contains("Reunión breve."));
    }
}
