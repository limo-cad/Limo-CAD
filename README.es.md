<table align="center"><tr>
<td align="center" width="120"><a href="README.md" hreflang="en">English</a></td>
<td align="center" width="120"><a href="README.zh-CN.md" hreflang="zh-CN">简体中文</a></td>
<td align="center" width="120"><b>Español</b></td>
<td align="center" width="120"><a href="README.de.md" hreflang="de">Deutsch</a></td>
</tr></table>

<p align="center"><img src="docs/assets/branding/limo-proposal-concept-en.png" alt="Limo CAD. Design with understanding." width="720"></p>

# Limo CAD

> **Diseñar con comprensión.**

**CAD paramétrico fácil de usar, gratuito y de código abierto, hoy y siempre.**
Diseña piezas mecánicas, ensamblajes y planos en tu propio equipo, a mano o con
tu agente de IA, y mantén editable cada croquis y cada operación.

[![Vista previa de Bevy](https://img.shields.io/badge/Bevy-0.20.0--rc.2-blue)](https://github.com/limo-cad/Limo-CAD/releases/tag/bevy-preview-0.2.2-20261004.1)
[![Licencia: LGPL 2.1+](https://img.shields.io/badge/license-LGPL%202.1%2B-blue)](LICENSE)
[![Discussions](https://img.shields.io/github/discussions/limo-cad/Limo-CAD?label=discussions)](https://github.com/limo-cad/Limo-CAD/discussions)

**Pre-alfa · Vista previa de Bevy rc.2 · Versión de la aplicación 0.2.2**
· [Notas, revisión de origen y comprobaciones](https://github.com/limo-cad/Limo-CAD/releases/tag/bevy-preview-0.2.2-20261004.1)
· [Ayuda de instalación (en inglés)](docs/INSTALL.md)

| Plataforma | Descarga |
|---|---|
| Windows 11 | [ZIP x64](https://github.com/limo-cad/Limo-CAD/releases/download/bevy-preview-0.2.2-20261004.1/noBS-CAD-0.2.2-windows-x64.zip) |
| Linux | [DEB para Ubuntu 26.04 x64](https://github.com/limo-cad/Limo-CAD/releases/download/bevy-preview-0.2.2-20261004.1/noBS.CAD_0.2.2_amd64.deb) |

Estos paquetes usan la revisión `9b082687`; todavía no incluyen las correcciones
de integración posteriores. Windows ARM64, macOS y AppImage siguen pendientes de
calificación. La interfaz Bevy para el navegador está en desarrollo. Los nombres
publicados conservan el nombre anterior del producto. Consulta el
[estado de la transición (en inglés)](docs/native-transition-status.md).

Los paquetes de Windows no están firmados. SmartScreen puede advertir en el primer
inicio; elige **Más información → Ejecutar de todas formas**. Guarda copias de tus proyectos pre-alfa.

> **Nota sobre el idioma:** esta página está en español. Los documentos, ejemplos y
> recursos de conocimiento enlazados están por ahora solo en inglés y se indican
> con «(en inglés)» o apuntan directamente a páginas en inglés. Se agradece la
> ayuda de colaboradores para revisar y traducir.

## Por qué Limo CAD

- **Gratuito y de código abierto, para siempre.** Sin versión de pago ni funciones
  bloqueadas. El código es [LGPL 2.1 o posterior](LICENSE), así que seguirá abierto.
- **Fácil de usar.** La facilidad de uso es una de las tres prioridades del
  proyecto, junto con la fiabilidad y el rendimiento. La
  [lección de la primera pieza](#crea-tu-primera-pieza) lleva unos minutos.
- **Local y tuyo.** Sin cuenta, suscripción ni servicio en la nube. Un proyecto
  completo (piezas, ensamblajes y planos) vive en un único archivo `.limo`.
- **Historial paramétrico real.** Los croquis con restricciones gobiernan las
  operaciones sólidas; cambia una cota y todo lo posterior se reconstruye.
- **Preparado para agentes.** Un servidor MCP integrado permite que cualquier
  agente compatible con MCP cree y edite modelos, y lo que genera es el mismo
  historial editable que harías a mano.
- **Formatos abiertos.** Exportación a STEP, STL y 3MF; planos a DXF e impresión/PDF.

## Hecho en Limo CAD

Cada diseño se construyó desde un documento en blanco mediante MCP, y sus croquis,
operaciones y relaciones de ensamblaje siguen siendo editables. **Ver** reproduce
la grabación de la construcción; **Bucle de construcción** es un extracto breve acelerado.

<table>
<tr>
<td align="center" valign="top" width="33%">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#garden-bench"><img src="docs/assets/showcase/bench.png" alt="Banco de jardín con listones de respaldo abombados y reposabrazos redondeados"></a><br>
<b>Banco de jardín</b><br>
Cambia la cota de un listón y todo el respaldo se actualiza. El bastidor, los brazos y las uniones siguen siendo editables.
</td>
<td align="center" valign="top" width="33%">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#d-screw-vise"><img src="docs/assets/showcase/vise.png" alt="Tornillo de banco con mordaza deslizante guiada y mango compacto de tornillo en D"></a><br>
<b>Tornillo de banco</b><br>
Gira el tornillo y la mordaza lo sigue. Mordazas de 100 mm, recorrido de 90 mm, seis piezas impresas más tornillería.
</td>
<td align="center" valign="top" width="33%">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#vertical-axis-turbine"><img src="docs/assets/showcase/turbine.png" alt="Turbina de eje vertical de dos etapas con eje sobre rodamientos y transmisión al generador"></a><br>
<b>Turbina de eje vertical</b><br>
Dos etapas Savonius sobre un eje con rodamientos, que mueven un generador mediante una transmisión 4:1.
</td>
</tr>
<tr>
<td align="center">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#garden-bench"><b>Ver</b></a>
· <a href="https://limo-cad.github.io/Limo-CAD/open.html#garden-bench">Abrir receta</a><br>
<a href="examples/scripts/garden-bench.limo.jsonc">Código</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/bench.nbcad">.nbcad</a>
· <a href="docs/assets/showcase/bench-loop.gif">Bucle de construcción</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/bench-build-full.mp4">MP4</a>
</td>
<td align="center">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#d-screw-vise"><b>Ver</b></a>
· <a href="https://limo-cad.github.io/Limo-CAD/open.html#d-screw-vise">Abrir receta</a><br>
<a href="examples/scripts/d-screw-vise.limo.jsonc">Código</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/vise.nbcad">.nbcad</a>
· <a href="docs/assets/showcase/vise-loop.gif">Bucle de construcción</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/vise-build-full.mp4">MP4</a>
</td>
<td align="center">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#vertical-axis-turbine"><b>Ver</b></a>
· <a href="https://limo-cad.github.io/Limo-CAD/open.html#vertical-axis-turbine">Abrir receta</a><br>
<a href="examples/scripts/vertical-axis-turbine.limo.jsonc">Código</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/turbine.nbcad">.nbcad</a>
· <a href="docs/assets/showcase/turbine-loop.gif">Bucle de construcción</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/turbine-build-full.mp4">MP4</a>
</td>
</tr>
</table>

<!-- Print photos: add docs/assets/showcase/<design>-printed.jpg when supplied. -->

Los enlaces de receta cargan el código en **Scripts** para que lo revises antes de
ejecutarlo. Para inspeccionar enseguida un diseño terminado, descarga su `.limo`
y usa **Archivo → Abrir**. Son ejemplos de desarrollo; el ajuste físico y la
capacidad de carga siguen sin cualificarse.
[Diseños, planos y validación (en inglés)](docs/flagship-examples.md) · [Todas las recetas (en inglés)](examples/scripts/README.md)

## Crea tu primera pieza

Tras [instalar el CAD](docs/INSTALL.md), abre **Scripts**, elige
**Sketch, extrude, ease the edges** y selecciona **Run in new design**.
La lección construye un bloque de 60 × 30 × 12 mm con los bordes superiores redondeados.

Cuando termine, haz doble clic en la extrusión del historial de operaciones y cambia
su **Distancia** de **12 a 18 mm**. Guarda el resultado como `first-part.limo` y
vuelve a abrirlo para seguir editando.
[Instrucciones paso a paso (en inglés)](docs/INSTALL.md#make-your-first-part)

## Diseña, ensambla, dibuja

Los croquis con restricciones y la geometría de referencia gobiernan operaciones
sólidas editables. Reutiliza piezas en ensamblajes, define uniones y comprueba su
movimiento e interferencias. Mantén las piezas, los ensamblajes y los planos juntos
en un único proyecto `.limo`.

Asigna materiales y colores por cuerpo y exporta **3MF** para tu programa de laminado.
Las etiquetas de material y los metadatos de color ayudan en el traspaso; el perfil
de impresión real se elige en el programa de laminado. **STEP** conserva la
geometría exacta y **STL** ofrece exportación de malla. Las hojas de plano se
exportan a **DXF** e impresión/PDF.
[Ensamblajes (en inglés)](docs/ASSEMBLIES.md) · [Planos y cobertura de exportación (en inglés)](docs/2D_DRAWINGS.md)

## Trabaja con un agente

El MCP local por stdio está siempre disponible en la aplicación instalada. Trae tu
agente y modelo compatibles con MCP para crear una pieza, editar una operación
existente, inspeccionar un ensamblaje o reproducir una demostración. El CAD mantiene
el mismo proyecto editable tanto si usas las herramientas tú como si se las pides a un agente.

[Conecta tu agente (en inglés)](docs/INSTALL.md#connect-an-mcp-agent) y prueba:

> Use Limo CAD to run the fillet-basics lesson in a new design in the open CAD
> window. Preserve my existing documents. After the final checks pass, change
> the stock extrusion from 12 to 18 mm, inspect the result and keep it open.

(El mensaje está en inglés porque los nombres de las lecciones integradas están en inglés.)

Un agente es opcional. **Scripts** puede construir un ejemplo incluido, explicar sus
capítulos y mostrar la construcción con subtítulos, movimientos de cámara y controles
de reproducción. Puedes inspeccionar y editar la receta antes de ejecutarla.
[Interfaz MCP (en inglés)](mcp-server/README.md) · [Recetas y reproducción (en inglés)](docs/native-scripts.md)
· [Conocimiento de ingeniería (en inglés)](knowledge/index.md)

## Ayuda a construirlo

Las contribuciones son bienvenidas. Nuestras prioridades son la **fiabilidad, el
rendimiento y la facilidad de uso**, en ese orden. Trae una pieza, un error
reproducible o una mejora concreta.
[Contribuir (en inglés)](CONTRIBUTING.md) · [Configuración para desarrollo (en inglés)](docs/DEVELOPMENT.md)
· [Documentación (en inglés)](docs/INDEX.md)

¿Preguntas, ideas o algo que hayas creado? Abre un hilo en
[Discussions](https://github.com/limo-cad/Limo-CAD/discussions). Si Limo CAD te
resulta útil, una estrella ayuda a que otras personas lo encuentren.

Trabajamos hacia lecciones de diseño guiadas y asistentes conversacionales, y
desarrollamos una base inicial de **CAM de 3 ejes** con generación de trayectorias,
simulación de material en bruto y posprocesadores adaptados a la máquina. No es un
CAM seguro para producción; consulta las
[guías de CAM y límites de seguridad (en inglés)](docs/cam/README.md). El análisis
de resistencia sigue siendo una capacidad futura.
[Dirección del proyecto (en inglés)](docs/goals.md)

## Fundamentos de código abierto

- **[Open CASCADE Technology](https://github.com/Open-Cascade-SAS/OCCT)** — geometría e intercambio CAD.
- **[Bevy](https://bevy.org/) y [wgpu](https://wgpu.rs/)** — interfaz y renderizado nativos.
- **[Rust](https://rust-lang.org/)** — modelado, ensamblajes y ejecución de recetas.

Gracias también a [FreeCAD](https://www.freecad.org/) y a la comunidad CAD de código abierto en general.

## Licencia

[GNU LGPL 2.1 o posterior](LICENSE). Libre para usar, inspeccionar y mejorar.

<details>
<summary>Avisos de terceros y compatibilidad con ratón 3D</summary>

Las licencias y atribuciones de las dependencias están en los [Avisos de terceros](THIRD_PARTY_NOTICES.md).
Las fuentes de los iconos constan en la [Procedencia de los iconos](docs/ICON_PROVENANCE.md).
Otros proyectos CAD tienen sus propias licencias; consulta la [guía de contribución](CONTRIBUTING.md#license--borrow).

El escritorio Bevy admite dispositivos 3Dconnexion SpaceMouse mediante entrada HID nativa.
Limo CAD es independiente y no está afiliado, respaldado ni certificado por 3Dconnexion.
3Dconnexion y SpaceMouse son marcas comerciales o registradas de 3Dconnexion.
Las herramientas de desarrollo de dispositivos de entrada 3D y la tecnología relacionada se
proporcionan con licencia de 3Dconnexion. © 3Dconnexion 1992–2020. Todos los derechos reservados.

</details>
