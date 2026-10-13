<table align="center"><tr>
<td align="center" width="120"><a href="README.md" hreflang="en">English</a></td>
<td align="center" width="120"><a href="README.zh-CN.md" hreflang="zh-CN">简体中文</a></td>
<td align="center" width="120"><a href="README.es.md" hreflang="es">Español</a></td>
<td align="center" width="120"><b>Deutsch</b></td>
</tr></table>

<p align="center"><img src="docs/assets/branding/limo-proposal-concept-en.png" alt="Limo CAD. Design with understanding." width="720"></p>

# Limo CAD

> **Konstruieren mit Verständnis.**

**Einfach zu bedienendes parametrisches CAD, kostenlos und quelloffen, jetzt und für immer.**
Konstruiere mechanische Bauteile, Baugruppen und Zeichnungen auf deinem eigenen Rechner, von
Hand oder mit deinem KI-Agenten, und behalte jede Skizze und jedes Feature editierbar.

[![Bevy-Vorschau](https://img.shields.io/badge/Bevy-0.20.0--rc.2-blue)](https://github.com/limo-cad/Limo-CAD/releases/tag/bevy-preview-0.2.2-20261004.1)
[![Lizenz: LGPL 2.1+](https://img.shields.io/badge/license-LGPL%202.1%2B-blue)](LICENSE)
[![Discussions](https://img.shields.io/github/discussions/limo-cad/Limo-CAD?label=discussions)](https://github.com/limo-cad/Limo-CAD/discussions)

**Pre-Alpha · Bevy-rc.2-Vorschau · Anwendungsversion 0.2.2**
· [Vorschauhinweise, Quellstand und Prüfungen](https://github.com/limo-cad/Limo-CAD/releases/tag/bevy-preview-0.2.2-20261004.1)
· [Installationshilfe (englisch)](docs/INSTALL.md)

| Plattform | Download |
|---|---|
| Windows 11 | [x64-ZIP](https://github.com/limo-cad/Limo-CAD/releases/download/bevy-preview-0.2.2-20261004.1/noBS-CAD-0.2.2-windows-x64.zip) |
| Linux | [DEB für Ubuntu 26.04 x64](https://github.com/limo-cad/Limo-CAD/releases/download/bevy-preview-0.2.2-20261004.1/noBS.CAD_0.2.2_amd64.deb) |

Diese Pakete stammen vom Quellstand `9b082687`; spätere Integrationskorrekturen
sind noch nicht enthalten. Windows ARM64, macOS und AppImage sind bis zur
Qualifikation zurückgehalten. Die Bevy-Browseroberfläche ist noch in Entwicklung.
Veröffentlichte Dateinamen behalten den früheren Produktnamen. Siehe
[Umstellungsstatus (englisch)](docs/native-transition-status.md).

Die Windows-Pakete sind nicht signiert. SmartScreen kann beim ersten Start warnen;
wähle **Weitere Informationen → Trotzdem ausführen**. Sichere wichtige Pre-Alpha-Projekte.

> **Hinweis zur Sprache:** Diese Seite ist auf Deutsch. Die verlinkten Dokumente, Beispiele und
> Wissensressourcen gibt es derzeit nur auf Englisch; sie sind mit „(englisch)“ gekennzeichnet
> oder verweisen direkt auf englische Seiten. Hilfe beim Prüfen und Übersetzen ist willkommen.

## Warum Limo CAD

- **Kostenlos und quelloffen, auf Dauer.** Kein Bezahltarif, keine gesperrten Funktionen.
  Der Code steht unter [LGPL 2.1 oder neuer](LICENSE) und bleibt dadurch offen.
- **Einfach zu bedienen.** Bedienbarkeit ist neben Zuverlässigkeit und Leistung eine der
  drei Prioritäten des Projekts. Die [Lektion zum ersten Bauteil](#erstelle-dein-erstes-bauteil)
  dauert wenige Minuten.
- **Lokal und in deiner Hand.** Kein Konto, kein Abonnement, kein Cloud-Dienst. Ein ganzes
  Projekt (Bauteile, Baugruppen und Zeichnungen) liegt in einer einzigen `.limo`-Datei.
- **Echte parametrische Historie.** Bemaßte Skizzen steuern Volumen-Features; ändere ein
  Maß, und alles Nachfolgende wird neu aufgebaut.
- **Agentenbereit.** Ein eingebauter MCP-Server lässt jeden MCP-kompatiblen Agenten Modelle
  erstellen und bearbeiten; das Ergebnis ist dieselbe editierbare Historie, die du von Hand erzeugen würdest.
- **Offene Formate.** Export als STEP, STL und 3MF; Zeichnungen als DXF und Druck/PDF.

## Entstanden in Limo CAD

Jedes Design wurde über MCP aus einem leeren Dokument aufgebaut, und seine Skizzen,
Features und Baugruppenbeziehungen bleiben editierbar. **Ansehen** spielt die
Aufzeichnung des Aufbaus ab; **Aufbau-Schleife** ist ein kurzer, beschleunigter Ausschnitt.

<table>
<tr>
<td align="center" valign="top" width="33%">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#garden-bench"><img src="docs/assets/showcase/bench.png" alt="Gartenbank mit gewölbten Rückenlatten und abgerundeten Armlehnen"></a><br>
<b>Gartenbank</b><br>
Ändere das Maß einer Latte, und die ganze Rückenlehne aktualisiert sich. Rahmen, Armlehnen und Verbindungen bleiben editierbar.
</td>
<td align="center" valign="top" width="33%">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#d-screw-vise"><img src="docs/assets/showcase/vise.png" alt="Schraubstock mit geführter Gleitbacke und kompaktem D-Spindelgriff"></a><br>
<b>Schraubstock</b><br>
Drehe die Spindel, und die Backe folgt. 100-mm-Backen, 90 mm Hub, sechs gedruckte Teile plus Normteile.
</td>
<td align="center" valign="top" width="33%">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#vertical-axis-turbine"><img src="docs/assets/showcase/turbine.png" alt="Zweistufige Vertikalachsen-Turbine mit gelagerter Welle und Generatorantrieb"></a><br>
<b>Vertikalachsen-Turbine</b><br>
Zwei Savonius-Stufen auf einer gelagerten Welle, die über eine 4:1-Übersetzung einen Generator antreiben.
</td>
</tr>
<tr>
<td align="center">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#garden-bench"><b>Ansehen</b></a>
· <a href="https://limo-cad.github.io/Limo-CAD/open.html#garden-bench">Rezept öffnen</a><br>
<a href="examples/scripts/garden-bench.limo.jsonc">Quelltext</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/bench.nbcad">.nbcad</a>
· <a href="docs/assets/showcase/bench-loop.gif">Aufbau-Schleife</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/bench-build-full.mp4">MP4</a>
</td>
<td align="center">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#d-screw-vise"><b>Ansehen</b></a>
· <a href="https://limo-cad.github.io/Limo-CAD/open.html#d-screw-vise">Rezept öffnen</a><br>
<a href="examples/scripts/d-screw-vise.limo.jsonc">Quelltext</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/vise.nbcad">.nbcad</a>
· <a href="docs/assets/showcase/vise-loop.gif">Aufbau-Schleife</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/vise-build-full.mp4">MP4</a>
</td>
<td align="center">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#vertical-axis-turbine"><b>Ansehen</b></a>
· <a href="https://limo-cad.github.io/Limo-CAD/open.html#vertical-axis-turbine">Rezept öffnen</a><br>
<a href="examples/scripts/vertical-axis-turbine.limo.jsonc">Quelltext</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/turbine.nbcad">.nbcad</a>
· <a href="docs/assets/showcase/turbine-loop.gif">Aufbau-Schleife</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/turbine-build-full.mp4">MP4</a>
</td>
</tr>
</table>

<!-- Print photos: add docs/assets/showcase/<design>-printed.jpg when supplied. -->

Rezept-Links laden den Quelltext in **Scripts**, damit du ihn vor dem Ausführen prüfen
kannst. Um ein fertiges Design sofort anzusehen, lade seine `.limo`-Datei herunter und
nutze **Datei → Öffnen**. Dies sind Entwicklungsbeispiele; die Passung in der Praxis und
die Belastbarkeit sind noch nicht qualifiziert.
[Designs, Zeichnungen und Validierung (englisch)](docs/flagship-examples.md) · [Alle Rezepte (englisch)](examples/scripts/README.md)

## Erstelle dein erstes Bauteil

Öffne nach der [Installation von CAD](docs/INSTALL.md) **Scripts**, wähle
**Sketch, extrude, ease the edges** und klicke auf **Run in new design**.
Die Lektion erstellt einen 60 × 30 × 12 mm großen Block mit abgerundeten oberen Kanten.

Doppelklicke danach in der Feature-Historie auf die Extrusion und ändere ihre
**Distanz** von **12 auf 18 mm**. Speichere das Ergebnis als `first-part.limo` und
öffne es erneut, um weiterzuarbeiten.
[Schritt-für-Schritt-Anleitung (englisch)](docs/INSTALL.md#make-your-first-part)

## Konstruieren, zusammenbauen, zeichnen

Bemaßte Skizzen und Referenzgeometrie steuern editierbare Volumen-Features.
Verwende Bauteile in Baugruppen wieder, definiere Gelenke und prüfe Bewegung und
Kollisionen. Bauteile, Baugruppen und Zeichnungen bleiben zusammen in einem
`.limo`-Projekt.

Weise jedem Körper Material und Farbe zu und exportiere **3MF** für deinen Slicer.
Materialbezeichnungen und Farbmetadaten unterstützen die Übergabe; das eigentliche
Druckprofil wählst du im Slicer. **STEP** enthält die exakte Geometrie, **STL**
liefert den Netzexport. Zeichnungsblätter lassen sich als **DXF** und Druck/PDF exportieren.
[Baugruppen (englisch)](docs/ASSEMBLIES.md) · [Zeichnungen und Exportumfang (englisch)](docs/2D_DRAWINGS.md)

## Mit einem Agenten arbeiten

Lokales stdio-MCP steht in der installierten Anwendung immer zur Verfügung. Bring deinen
bevorzugten MCP-kompatiblen Agenten samt Modell mit, um ein Bauteil zu erstellen, ein
vorhandenes Feature zu bearbeiten, eine Baugruppe zu prüfen oder eine Demonstration
abzuspielen. CAD behält dasselbe editierbare Projekt, egal ob du die Werkzeuge selbst
benutzt oder einen Agenten damit beauftragst.

[Verbinde deinen Agenten (englisch)](docs/INSTALL.md#connect-an-mcp-agent) und probiere:

> Use Limo CAD to run the fillet-basics lesson in a new design in the open CAD
> window. Preserve my existing documents. After the final checks pass, change
> the stock extrusion from 12 to 18 mm, inspect the result and keep it open.

(Der Prompt ist englisch, weil die Namen der eingebauten Lektionen englisch sind.)

Ein Agent ist optional. **Scripts** kann ein mitgeliefertes Beispiel aufbauen, seine
Kapitel erklären und den Aufbau mit Untertiteln, Kamerabewegungen und Wiedergabesteuerung
zeigen. Du kannst das Rezept vor dem Ausführen prüfen und bearbeiten.
[MCP-Schnittstelle (englisch)](mcp-server/README.md) · [Rezepte und Wiedergabe (englisch)](docs/native-scripts.md)
· [Ingenieurwissen (englisch)](knowledge/index.md)

## Mitbauen

Beiträge sind willkommen. Unsere Prioritäten sind **Zuverlässigkeit, Leistung und
Bedienbarkeit**, in dieser Reihenfolge. Bring ein Bauteil, einen reproduzierbaren
Fehler oder eine gezielte Verbesserung mit.
[Mitwirken (englisch)](CONTRIBUTING.md) · [Entwickler-Setup (englisch)](docs/DEVELOPMENT.md)
· [Dokumentation (englisch)](docs/INDEX.md)

Fragen, Ideen oder etwas Selbstgebautes? Eröffne einen Thread in den
[Discussions](https://github.com/limo-cad/Limo-CAD/discussions). Wenn dir Limo CAD
nützt, hilft ein Stern anderen, das Projekt zu finden.

Wir arbeiten an geführten Konstruktionslektionen und Dialog-Assistenten und entwickeln
eine frühe **3-Achs-CAM**-Grundlage mit Werkzeugweg-Erzeugung, Rohteilsimulation und
maschinenspezifischen Postprozessoren. Es ist kein produktionssicheres CAM; lies die
[CAM-Anleitungen und Sicherheitsgrenzen (englisch)](docs/cam/README.md).
Festigkeitsanalyse bleibt eine zukünftige Fähigkeit.
[Projektrichtung (englisch)](docs/goals.md)

## Open-Source-Grundlagen

- **[Open CASCADE Technology](https://github.com/Open-Cascade-SAS/OCCT)** — Geometrie und CAD-Austausch.
- **[Bevy](https://bevy.org/) und [wgpu](https://wgpu.rs/)** — native Oberfläche und Rendering.
- **[Rust](https://rust-lang.org/)** — Modellierung, Baugruppen und Rezeptausführung.

Dank auch an [FreeCAD](https://www.freecad.org/) und die gesamte Open-Source-CAD-Gemeinschaft.

## Lizenz

[GNU LGPL 2.1 oder neuer](LICENSE). Frei nutzbar, einsehbar und verbesserbar.

<details>
<summary>Hinweise zu Drittanbietern und 3D-Maus-Unterstützung</summary>

Lizenzen und Quellenangaben der Abhängigkeiten stehen in den [Hinweisen zu Drittanbietern](THIRD_PARTY_NOTICES.md).
Die Quellen der Symbole sind in der [Herkunft der Symbole](docs/ICON_PROVENANCE.md) dokumentiert.
Andere CAD-Projekte haben eigene Lizenzen; siehe die [Hinweise zum Mitwirken](CONTRIBUTING.md#license--borrow).

Der Bevy-Desktop unterstützt 3Dconnexion-SpaceMouse-Geräte über native HID-Eingabe.
Limo CAD ist unabhängig und steht in keiner Verbindung zu 3Dconnexion; es wird von
3Dconnexion weder empfohlen noch zertifiziert.
3Dconnexion und SpaceMouse sind Marken oder eingetragene Marken von 3Dconnexion.
Entwicklungswerkzeuge für 3D-Eingabegeräte und verwandte Technologie werden unter Lizenz
von 3Dconnexion bereitgestellt. © 3Dconnexion 1992–2020. Alle Rechte vorbehalten.

</details>
