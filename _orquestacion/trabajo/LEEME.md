# trabajo/

Una carpeta por tarea **en curso o cerrada**, nombrada `<ID>-<slug>`:

```
trabajo/
├─ HU-04-busqueda/
│  ├─ PLAN.md                 orquestador, antes de implementar
│  ├─ HANDOFF.md              quien implementó
│  ├─ REVIEW-codigo.md        revisor-codigo
│  ├─ REVIEW-pruebas.md       verificador-pruebas
│  ├─ REVIEW-plataforma.md    auditor-plataforma (si aplicaba)
│  └─ REVIEW.md               orquestador: consolidado y veredicto
└─ HU-01-wasapi/
```

El `<ID>` es el de [`docs/06-historias-de-usuario.md`](../../docs/06-historias-de-usuario.md)
cuando la tarea viene de una HU. Para trabajo que no es una HU (correcciones, andamiaje, deuda),
un prefijo propio: `FIX-`, `BOOT-`, `DEUDA-`.

**Las carpetas se crean cuando la tarea arranca, no antes.** No se pre-arma un paquete para cada
historia del backlog: eso sirve para repartir trabajo entre personas, y acá no hay a quién
repartirlo.

La carpeta no se borra al cerrar: el `REVIEW.md` de una tarea terminada es lo que explica por
qué el código quedó como quedó.
