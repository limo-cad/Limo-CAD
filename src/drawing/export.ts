import { getEngine } from '../engine';
import type {
  DrawingAnnotationDto,
  DrawingPolylineDto,
  DrawingProjectionDto,
  DrawingSheetDto,
  DrawingViewDto,
  ProfileCatalogItemDto,
} from '../engine/types';
import { chooseSaveTarget, writeSaveTarget, type SaveType } from '../files/fileIO';
import { useAppStore } from '../store/appStore';
import type { UnitSystem } from '../types/document';
import {
  drawingFormatShortLabel,
  drawingSheetSize,
  drawingToleranceNoteText,
  drawingViewPaperBounds,
  drawingViewTransform,
} from './sheet';
import {
  angularDimensionGeometry,
  arcLengthDimensionGeometry,
  arrowPolygon,
  automaticSymmetryAxisGeometry,
  boltCircleGeometry,
  centerLineBetweenEdgesGeometry,
  centerLineGeometry,
  centerMarkGeometry,
  drawingChamferText,
  drawingAngularDimensionText,
  drawingDimensionText,
  drawingDimensionTextWidth,
  drawingHoleCalloutText,
  drawingLinearDimensionLayout,
  lineDimensionGeometry,
  linearDimensionGeometry,
  pointLineDimensionGeometry,
  ordinateDimensionGeometry,
  radialDimensionGeometry,
  resolveDrawingAnchor,
  resolveDrawingAttachment,
  resolveDrawingCircle,
  resolveDrawingLine,
  type DrawingDimensionGeometry,
} from './annotations';
import { buildDrawingSheetDxf, buildManufacturingProfileDxf } from './dxf';
import { drawingProjectionRequestForView } from './projection';
import { drawingLineStyle, type DrawingLineRole } from './styles';

const DXF_TYPE: SaveType = {
  description: 'AutoCAD Drawing Interchange',
  extension: '.dxf',
  mime: 'application/dxf',
};

const SVG_TYPE: SaveType = {
  description: 'Scalable Vector Drawing',
  extension: '.svg',
  mime: 'image/svg+xml',
};

/** Primary editable CAD interchange export. Geometry is written 1:1 in mm. */
export async function exportActiveDrawingDxf(): Promise<boolean> {
  const state = useAppStore.getState();
  const sheet = state.drawingDocument.sheets.find(
    (candidate) => candidate.id === state.drawingDocument.active_sheet_id,
  );
  if (!sheet) throw new Error('There is no active drawing sheet to export.');
  const dxf = await drawingSheetDxf(sheet, state.document?.settings.units ?? 'mm');
  const project = safeFilePart(state.document?.name ?? 'Untitled');
  const sheetName = safeFilePart(sheet.name);
  const target = await chooseSaveTarget(`${project}-${sheetName}.dxf`, DXF_TYPE);
  if (!target) return false;
  await writeSaveTarget(target, new TextEncoder().encode(dxf));
  return true;
}

export async function drawingSheetDxf(
  sheet: DrawingSheetDto,
  units: UnitSystem = 'mm',
): Promise<string> {
  return buildDrawingSheetDxf(sheet, await drawingProjections(sheet), units);
}

/** Export one exact sketch profile and its hole wires in local sketch-plane mm. */
export async function exportManufacturingProfileDxf(
  catalog: ProfileCatalogItemDto,
  profileIndex: number,
): Promise<boolean> {
  const state = useAppStore.getState();
  const dxf = buildManufacturingProfileDxf(catalog, profileIndex);
  const project = safeFilePart(state.document?.name ?? 'Untitled');
  const sketch = safeFilePart(catalog.sketch_name);
  const target = await chooseSaveTarget(`${project}-${sketch}-profile-${profileIndex + 1}.dxf`, DXF_TYPE);
  if (!target) return false;
  await writeSaveTarget(target, new TextEncoder().encode(dxf));
  return true;
}

export async function exportActiveDrawingSvg(): Promise<boolean> {
  const state = useAppStore.getState();
  const sheet = state.drawingDocument.sheets.find(
    (candidate) => candidate.id === state.drawingDocument.active_sheet_id,
  );
  if (!sheet) throw new Error('There is no active drawing sheet to export.');
  const svg = await drawingSheetSvg(sheet, state.document?.settings.units ?? 'mm');
  const project = safeFilePart(state.document?.name ?? 'Untitled');
  const sheetName = safeFilePart(sheet.name);
  const target = await chooseSaveTarget(`${project}-${sheetName}.svg`, SVG_TYPE);
  if (!target) return false;
  await writeSaveTarget(target, new TextEncoder().encode(svg));
  return true;
}

export function printActiveDrawing(): void {
  window.print();
}

export async function drawingSheetSvg(
  sheet: DrawingSheetDto,
  units: UnitSystem = 'mm',
): Promise<string> {
  const projections = await drawingProjections(sheet);
  const [width, height] = drawingSheetSize(sheet.format, sheet.orientation);
  const projectionsByView = new Map(
    sheet.views.map((view, index) => [view.id, projections[index]] as const),
  );
  const views = sheet.views
    .map((view, index) => viewSvg(view, projections[index], sheet))
    .join('\n');
  const derivedSources = sheet.views.map((child) => {
    const parentId = child.derivation?.parent_view_id;
    if (parentId == null) return '';
    const parent = sheet.views.find((view) => view.id === parentId);
    const projection = projectionsByView.get(parentId);
    return parent && projection ? derivedSourceSvg(child, parent, projection, sheet) : '';
  }).join('\n');
  const annotations = sheet.annotations
    .map((annotation) => `<g class="nbs-${annotationLineRole(annotation)}">${annotationSvg(annotation, sheet, projectionsByView, units)}</g>`)
    .join('\n');
  return `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="${width}mm" height="${height}mm" viewBox="0 0 ${width} ${height}">
  ${drawingSvgStyleSheet(sheet)}
  <rect width="${width}" height="${height}" fill="white"/>
  <g fill="none" stroke="#17191c" stroke-linecap="round" stroke-linejoin="round">${views}${derivedSources}</g>
  ${annotations}
  ${borderAndTitleBlock(sheet, width, height)}
</svg>`;
}

async function drawingProjections(sheet: DrawingSheetDto): Promise<DrawingProjectionDto[]> {
  const engine = await getEngine();
  const scene = useAppStore.getState().solidScene;
  return Promise.all(
    sheet.views.map((view) =>
      engine.drawingProjection(drawingProjectionRequestForView(view, sheet.views, scene, useAppStore.getState().assemblySolution)),
    ),
  );
}

function linearDimensionSvg(
  geometry: DrawingDimensionGeometry,
  text: string,
  sheet: DrawingSheetDto,
): string {
  const layout = drawingLinearDimensionLayout(
    geometry,
    text,
    sheet.style.text_height_mm,
    sheet.style.arrow_size_mm,
    sheet.standard,
  );
  const path = [
    `M${point(geometry.firstExtension[0])}L${point(geometry.firstExtension[1])}`,
    `M${point(geometry.secondExtension[0])}L${point(geometry.secondExtension[1])}`,
    `M${point(layout.lineStart)}L${point(layout.lineEnd)}`,
  ].join(' ');
  const baselineOffset = layout.maskDimensionLine
    ? 0.8
    : Math.max(1.2, sheet.style.text_height_mm * 0.22 + sheet.style.dimension.width_mm * 2);
  const textPosition: [number, number] = [layout.textPosition[0], layout.textPosition[1] - baselineOffset];
  const transform = `rotate(${round(geometry.textAngle)} ${round(layout.textPosition[0])} ${round(layout.textPosition[1])})`;
  return `<g data-dimension-arrows="${layout.arrowsOutside ? 'outside' : 'inside'}" data-dimension-text="${layout.textOutside ? 'outside' : 'inside'}" data-dimension-line="${layout.maskDimensionLine ? 'interrupted' : 'continuous'}"><path d="${path}" fill="none"/><polygon points="${arrowPolygon(geometry.dimensionStart, layout.firstArrowToward, sheet.style.arrow_size_mm)}"/><polygon points="${arrowPolygon(geometry.dimensionEnd, layout.secondArrowToward, sheet.style.arrow_size_mm)}"/>${dimensionValueText(textPosition, text, sheet.style.text_height_mm, 'middle', transform, layout.maskDimensionLine)}</g>`;
}

function annotationSvg(
  annotation: DrawingAnnotationDto,
  sheet: DrawingSheetDto,
  projections: Map<number, DrawingProjectionDto>,
  units: UnitSystem,
): string {
  const arrowSize = sheet.style.arrow_size_mm;
  if (annotation.kind === 'note') {
    return noteSvg(annotation);
  }
  if (annotation.kind === 'revision_cloud') {
    const points = annotation.points.map(point).join(' ');
    return `<g fill="none" stroke="#b54432"><polygon points="${points}" stroke-width="0.5" stroke-dasharray="2 1"/><text x="${round(annotation.points[0]?.[0] ?? 0)}" y="${round((annotation.points[0]?.[1] ?? 0) - 2)}" fill="#b54432" font-size="3.2">REV ${escapeXml(annotation.revision)}</text></g>`;
  }
  const view = sheet.views.find((candidate) => candidate.id === annotation.view_id);
  const projection = projections.get(annotation.view_id);
  if (!view || !projection) return '';
  switch (annotation.kind) {
    case 'linear_dimension': {
      const first = resolveDrawingAnchor(annotation.first, view, projection);
      const second = resolveDrawingAnchor(annotation.second, view, projection);
      const geometry = first && second
        ? linearDimensionGeometry(first, second, annotation.mode, annotation.offset, view.scale)
        : null;
      if (!geometry) return brokenAnnotationSvg(view);
      const text = drawingDimensionText(geometry.value, annotation.precision, annotation.prefix, annotation.suffix, units, annotation.presentation, sheet.standard);
      return `<g fill="#23272d" stroke="#23272d" stroke-width="0.34">${linearDimensionSvg(geometry, text, sheet)}</g>`;
    }
    case 'line_dimension': {
      const first = resolveDrawingLine(annotation.first, view, projection);
      const second = annotation.second ? resolveDrawingLine(annotation.second, view, projection) : null;
      const result = first && (!annotation.second || second)
        ? lineDimensionGeometry(first, second, annotation.mode, annotation.position, view.scale)
        : null;
      if (!result) return brokenAnnotationSvg(view);
      if (result.kind === 'angular') {
        const geometry = result.geometry;
        const text = drawingAngularDimensionText(geometry.value, annotation.precision, annotation.prefix, annotation.suffix, annotation.presentation, sheet.standard);
        return `<g fill="#23272d" stroke="#23272d"><path d="M${point(geometry.vertex)}L${point(geometry.firstRay)} M${point(geometry.vertex)}L${point(geometry.secondRay)} ${geometry.arcPath}" fill="none" stroke-width="0.34"/>${dimensionValueText(geometry.textPosition, text, sheet.style.text_height_mm)}</g>`;
      }
      const geometry = result.geometry;
      const text = drawingDimensionText(geometry.value, annotation.precision, annotation.prefix, annotation.suffix, units, annotation.presentation, sheet.standard);
      return `<g fill="#23272d" stroke="#23272d" stroke-width="0.34">${linearDimensionSvg(geometry, text, sheet)}</g>`;
    }
    case 'point_line_dimension': {
      const resolvedPoint = resolveDrawingAnchor(annotation.point, view, projection);
      const line = resolveDrawingLine(annotation.line, view, projection);
      const geometry = resolvedPoint && line
        ? pointLineDimensionGeometry(resolvedPoint, line, annotation.position, view.scale)
        : null;
      if (!geometry) return brokenAnnotationSvg(view);
      const text = drawingDimensionText(geometry.value, annotation.precision, annotation.prefix, annotation.suffix, units, annotation.presentation, sheet.standard);
      return `<g fill="#23272d" stroke="#23272d" stroke-width="0.34">${linearDimensionSvg(geometry, text, sheet)}</g>`;
    }
    case 'radial_dimension': {
      const resolved = resolveDrawingCircle(annotation.feature, view, projection);
      if (!resolved) return brokenAnnotationSvg(view);
      const geometry = radialDimensionGeometry(resolved, annotation.mode, annotation.leader_angle_deg, annotation.offset);
      const symbol = annotation.mode === 'diameter' ? '⌀' : 'R';
      const text = drawingDimensionText(geometry.value, annotation.precision, `${annotation.prefix}${symbol}`, annotation.suffix, units, annotation.presentation, sheet.standard);
      return `<g fill="#23272d" stroke="#23272d"><path d="M${point(geometry.center)}L${point(geometry.featurePoint)}L${point(geometry.shoulder)}" fill="none" stroke-width="0.34"/><polygon points="${arrowPolygon(geometry.featurePoint, geometry.center, arrowSize)}"/>${dimensionValueText(geometry.textPosition, text, sheet.style.text_height_mm, geometry.textPosition[0] >= geometry.center[0] ? 'start' : 'end')}</g>`;
    }
    case 'angular_dimension': {
      const vertex = resolveDrawingAnchor(annotation.vertex, view, projection);
      const first = resolveDrawingAnchor(annotation.first, view, projection);
      const second = resolveDrawingAnchor(annotation.second, view, projection);
      const geometry = vertex && first && second ? angularDimensionGeometry(vertex, first, second, annotation.radius) : null;
      if (!geometry) return brokenAnnotationSvg(view);
      const text = drawingAngularDimensionText(geometry.value, annotation.precision, annotation.prefix, annotation.suffix, annotation.presentation, sheet.standard);
      return `<g fill="#23272d" stroke="#23272d"><path d="M${point(geometry.vertex)}L${point(geometry.firstRay)} M${point(geometry.vertex)}L${point(geometry.secondRay)} ${geometry.arcPath}" fill="none" stroke-width="0.34"/>${dimensionValueText(geometry.textPosition, text, sheet.style.text_height_mm)}</g>`;
    }
    case 'hole_note': {
      const resolved = resolveDrawingCircle(annotation.feature, view, projection);
      if (!resolved) return brokenAnnotationSvg(view);
      const direction = normalize2([annotation.position[0] - resolved.center[0], annotation.position[1] - resolved.center[1]]);
      const featurePoint: [number, number] = [resolved.center[0] + direction[0] * resolved.paperRadius, resolved.center[1] + direction[1] * resolved.paperRadius];
      const text = drawingHoleCalloutText(annotation, sheet.standard, units);
      return `<g fill="#23272d" stroke="#23272d"><path d="M${point(featurePoint)}L${point(annotation.position)}" fill="none" stroke-width="0.34"/><polygon points="${arrowPolygon(featurePoint, annotation.position, arrowSize)}"/>${multilineText([annotation.position[0] + 1.2, annotation.position[1] - 0.8], text)}</g>`;
    }
    case 'center_mark': {
      const resolved = resolveDrawingCircle(annotation.feature, view, projection);
      if (!resolved) return brokenAnnotationSvg(view);
      const geometry = centerMarkGeometry(resolved, annotation.extension);
      const path = `M${point(geometry.horizontal[0])}L${point(geometry.horizontal[1])} M${point(geometry.vertical[0])}L${point(geometry.vertical[1])}`;
      return `<g fill="none" stroke="#356170"><path d="${path}" stroke-width="0.38" stroke-dasharray="6 1.5 1.2 1.5"/><circle cx="${round(geometry.center[0])}" cy="${round(geometry.center[1])}" r="0.48" fill="white" stroke-width="0.36"/></g>`;
    }
    case 'center_line': {
      const first = resolveDrawingCircle(annotation.first, view, projection);
      const second = resolveDrawingCircle(annotation.second, view, projection);
      const geometry = first && second
        ? centerLineGeometry(first, second, annotation.extension)
        : null;
      if (!geometry) return brokenAnnotationSvg(view);
      return `<g fill="white" stroke="#356170"><path d="M${point(geometry.start)}L${point(geometry.end)}" fill="none" stroke-width="0.38" stroke-dasharray="8 1.5 1.2 1.5"/><circle cx="${round(geometry.firstCenter[0])}" cy="${round(geometry.firstCenter[1])}" r="0.48" stroke-width="0.36"/><circle cx="${round(geometry.secondCenter[0])}" cy="${round(geometry.secondCenter[1])}" r="0.48" stroke-width="0.36"/></g>`;
    }
    case 'center_line_between_edges': {
      const first = resolveDrawingLine(annotation.first, view, projection);
      const second = resolveDrawingLine(annotation.second, view, projection);
      const geometry = first && second
        ? centerLineBetweenEdgesGeometry(first, second, annotation.extension)
        : null;
      if (!geometry) return brokenAnnotationSvg(view);
      return `<path d="M${point(geometry.start)}L${point(geometry.end)}" fill="none" stroke="#356170" stroke-width="0.38" stroke-dasharray="8 1.5 1.2 1.5"/>`;
    }
    case 'automatic_symmetry_axis': {
      const segments = automaticSymmetryAxisGeometry(view, projection, annotation.axis, annotation.extension);
      return `<path d="${segments.map(([start, end]) => `M${point(start)}L${point(end)}`).join(' ')}" fill="none" stroke="#356170" stroke-width="0.38" stroke-dasharray="8 1.5 1.2 1.5"/>`;
    }
    case 'bolt_circle_center_line': {
      const circles = annotation.features.map((feature) => resolveDrawingCircle(feature, view, projection));
      if (circles.some((circle) => !circle)) return brokenAnnotationSvg(view);
      const geometry = boltCircleGeometry(circles.filter((circle): circle is NonNullable<typeof circle> => Boolean(circle)), annotation.extension);
      if (!geometry) return brokenAnnotationSvg(view);
      const marks = geometry.marks.map((mark) => `M${point(mark.horizontal[0])}L${point(mark.horizontal[1])} M${point(mark.vertical[0])}L${point(mark.vertical[1])}`).join(' ');
      return `<g fill="none" stroke="#356170"><circle cx="${round(geometry.center[0])}" cy="${round(geometry.center[1])}" r="${round(geometry.radius)}" stroke-width="0.38" stroke-dasharray="8 1.5 1.2 1.5"/><path d="${marks}" stroke-width="0.32" stroke-dasharray="6 1.5 1.2 1.5"/></g>`;
    }
    case 'chain_dimension': {
      const anchors = annotation.anchors.map((anchor) => resolveDrawingAnchor(anchor, view, projection));
      if (anchors.some((anchor) => !anchor)) return brokenAnnotationSvg(view);
      const resolved = anchors.filter((anchor): anchor is NonNullable<typeof anchor> => Boolean(anchor));
      const pairs = annotation.layout === 'baseline'
        ? resolved.slice(1).map((target, index) => [resolved[0], target, annotation.offset + index * annotation.spacing] as const)
        : resolved.slice(1).map((target, index) => [resolved[index], target, annotation.offset + (annotation.layout === 'continued' ? index * annotation.spacing : 0)] as const);
      const rendered = pairs.map(([first, second, offset]) => {
        const geometry = linearDimensionGeometry(first, second, annotation.mode, offset, view.scale);
        if (!geometry) return '';
        const text = drawingDimensionText(geometry.value, annotation.precision, annotation.prefix, annotation.suffix, units, annotation.presentation, sheet.standard);
        return linearDimensionSvg(geometry, text, sheet);
      }).join('');
      return `<g fill="#23272d" stroke="#23272d" stroke-width="0.34">${rendered}</g>`;
    }
    case 'ordinate_dimension': {
      const origin = resolveDrawingAnchor(annotation.origin, view, projection);
      const target = resolveDrawingAnchor(annotation.target, view, projection);
      const geometry = origin && target ? ordinateDimensionGeometry(origin, target, annotation.offset, view.scale) : null;
      if (!geometry) return brokenAnnotationSvg(view);
      const values = annotation.axis === 'x'
        ? `X ${drawingDimensionText(geometry.xValue, annotation.precision, '', '', units, annotation.presentation, sheet.standard)}`
        : annotation.axis === 'y'
          ? `Y ${drawingDimensionText(geometry.yValue, annotation.precision, '', '', units, annotation.presentation, sheet.standard)}`
          : `X ${drawingDimensionText(geometry.xValue, annotation.precision, '', '', units, annotation.presentation, sheet.standard)}  Y ${drawingDimensionText(geometry.yValue, annotation.precision, '', '', units, annotation.presentation, sheet.standard)}`;
      return `<g fill="#23272d" stroke="#23272d"><circle cx="${round(geometry.origin[0])}" cy="${round(geometry.origin[1])}" r="1.1" fill="none" stroke-width="0.3"/><path d="M${point(geometry.target)}L${point(geometry.elbow)}" fill="none" stroke-width="0.34"/>${dimensionValueText(geometry.textPosition, values, sheet.style.text_height_mm)}</g>`;
    }
    case 'arc_length_dimension': {
      const circle = resolveDrawingCircle(annotation.feature, view, projection);
      const first = resolveDrawingAnchor(annotation.first, view, projection);
      const second = resolveDrawingAnchor(annotation.second, view, projection);
      const geometry = circle && first && second ? arcLengthDimensionGeometry(circle, first, second, annotation.offset) : null;
      if (!geometry) return brokenAnnotationSvg(view);
      const text = `⌒ ${drawingDimensionText(geometry.value, annotation.precision, '', '', units, annotation.presentation, sheet.standard)}`;
      return `<g fill="#23272d" stroke="#23272d"><path d="${geometry.path}" fill="none" stroke-width="0.34"/>${dimensionValueText(geometry.textPosition, text, sheet.style.text_height_mm)}</g>`;
    }
    case 'jogged_radius_dimension': {
      const circle = resolveDrawingCircle(annotation.feature, view, projection);
      if (!circle) return brokenAnnotationSvg(view);
      const direction = normalize2([annotation.jog[0] - circle.center[0], annotation.jog[1] - circle.center[1]]);
      const featurePoint: [number, number] = [circle.center[0] + direction[0] * circle.paperRadius, circle.center[1] + direction[1] * circle.paperRadius];
      const text = drawingDimensionText(circle.circle.radius, annotation.precision, 'R', '', units, annotation.presentation, sheet.standard);
      return `<g fill="#23272d" stroke="#23272d"><path d="M${point(featurePoint)}L${point(annotation.jog)} l2,-1 l2,2 L${point(annotation.position)}" fill="none" stroke-width="0.34"/><polygon points="${arrowPolygon(featurePoint, annotation.jog, arrowSize)}"/>${dimensionValueText([annotation.position[0] + 1.2, annotation.position[1] - 0.8], text, sheet.style.text_height_mm, 'start')}</g>`;
    }
    case 'chamfer_note': {
      const first = resolveDrawingAnchor(annotation.first, view, projection);
      const second = resolveDrawingAnchor(annotation.second, view, projection);
      if (!first || !second) return brokenAnnotationSvg(view);
      const attachment: [number, number] = [(first.paper[0] + second.paper[0]) / 2, (first.paper[1] + second.paper[1]) / 2];
      const text = drawingChamferText(
        annotation.length,
        annotation.angle_deg,
        annotation.prefix,
        sheet.standard,
        units,
      );
      return `<g fill="#23272d" stroke="#23272d"><path d="M${point(attachment)}L${point(annotation.position)}" fill="none" stroke-width="0.34"/><polygon points="${arrowPolygon(attachment, annotation.position, arrowSize)}"/>${outlinedText([annotation.position[0] + 1.2, annotation.position[1] - 0.8], text, 'start')}</g>`;
    }
    case 'datum_feature':
    case 'gdt_frame':
    case 'surface_texture':
    case 'item_balloon': {
      const attachment = resolveDrawingAttachment(annotation.attachment, view, projection);
      if (!attachment) return brokenAnnotationSvg(view);
      let text = '';
      if (annotation.kind === 'datum_feature') text = annotation.target_index ? `${annotation.label}${annotation.target_index}` : annotation.label;
      if (annotation.kind === 'gdt_frame') text = gdtFrameText(annotation);
      if (annotation.kind === 'surface_texture') text = `⌯ Ra ${trimNumber(annotation.roughness_ra)}${annotation.process ? ` ${annotation.process}` : ''}`;
      if (annotation.kind === 'item_balloon') text = sheet.bom.find((item) => item.id === annotation.bom_item_id)?.item_number ?? '?';
      if (annotation.kind === 'item_balloon') {
        return `<g fill="#fff" stroke="#23272d"><path d="M${point(attachment.point)}L${point(annotation.position)}" fill="none" stroke-width="0.34"/><circle cx="${round(annotation.position[0])}" cy="${round(annotation.position[1])}" r="4"/><text x="${round(annotation.position[0])}" y="${round(annotation.position[1] + 1.15)}" fill="#23272d" stroke="none" font-size="3.2" text-anchor="middle">${escapeXml(text)}</text></g>`;
      }
      return leaderTextSvg(attachment.point, annotation.position, text, arrowSize);
    }
    case 'edge_requirement': {
      const line = resolveDrawingLine(annotation.attachment, view, projection);
      if (!line) return brokenAnnotationSvg(view);
      const attachment = [(line.start[0] + line.end[0]) / 2, (line.start[1] + line.end[1]) / 2] as [number, number];
      const text = `${annotation.upper_deviation >= 0 ? '+' : ''}${trimNumber(annotation.upper_deviation)}/${trimNumber(annotation.lower_deviation)}${annotation.note ? ` ${annotation.note}` : ''}`;
      return leaderTextSvg(attachment, annotation.position, text, arrowSize);
    }
    case 'weld_symbol': {
      const line = resolveDrawingLine(annotation.attachment, view, projection);
      if (!line) return brokenAnnotationSvg(view);
      const attachment = [(line.start[0] + line.end[0]) / 2, (line.start[1] + line.end[1]) / 2] as [number, number];
      const referenceEnd: [number, number] = [annotation.position[0] + 28, annotation.position[1]];
      const weld = weldSymbolPath(annotation.weld_type, annotation.position);
      const details = `${trimNumber(annotation.size)}${annotation.length ? `-${trimNumber(annotation.length)}` : ''}${annotation.pitch ? `-${trimNumber(annotation.pitch)}` : ''}`;
      return `<g fill="none" stroke="#23272d" stroke-width="0.34"><path d="M${point(attachment)}L${point(annotation.position)}L${point(referenceEnd)} ${weld}"/><polygon points="${arrowPolygon(attachment, annotation.position, arrowSize)}"/>${outlinedText([annotation.position[0] + 4, annotation.position[1] - 2], details, 'start')}${annotation.tail ? outlinedText([referenceEnd[0] + 2, referenceEnd[1] - 1], annotation.tail, 'start') : ''}</g>`;
    }
  }
}

function brokenAnnotationSvg(view: DrawingViewDto): string {
  return `<g><circle cx="${view.position[0]}" cy="${view.position[1] - 8}" r="3.1" fill="#fff3f0" stroke="#b54432" stroke-width="0.45"/><text x="${view.position[0]}" y="${view.position[1] - 6.8}" fill="#b54432" font-size="3.5" font-weight="700" text-anchor="middle">!</text></g>`;
}

function leaderTextSvg(
  attachment: [number, number],
  position: [number, number],
  text: string,
  arrowSize: number,
): string {
  return `<g fill="#23272d" stroke="#23272d"><path d="M${point(attachment)}L${point(position)}" fill="none" stroke-width="0.34"/><polygon points="${arrowPolygon(attachment, position, arrowSize)}"/>${outlinedText([position[0] + 1.2, position[1] - 0.8], text, 'start')}</g>`;
}

function gdtFrameText(annotation: Extract<DrawingAnnotationDto, { kind: 'gdt_frame' }>): string {
  const symbols: Record<typeof annotation.characteristic, string> = {
    straightness: '—', flatness: '▱', circularity: '○', cylindricity: '⌭',
    profile_line: '⌒', profile_surface: '⌓', angularity: '∠', perpendicularity: '⊥',
    parallelism: '∥', position: '⌖', concentricity: '◎', symmetry: '≡',
    circular_runout: '↗', total_runout: '↗↗',
  };
  const modifier = annotation.material_condition === 'maximum' ? ' Ⓜ'
    : annotation.material_condition === 'least' ? ' Ⓛ'
      : annotation.material_condition === 'regardless' ? ' Ⓢ' : '';
  const datums = annotation.datums.map((datum) => {
    const datumModifier = datum.material_condition === 'maximum' ? 'Ⓜ'
      : datum.material_condition === 'least' ? 'Ⓛ'
        : datum.material_condition === 'regardless' ? 'Ⓢ' : '';
    return `${datum.label}${datumModifier}`;
  }).join(' | ');
  return `${symbols[annotation.characteristic]} | ${annotation.diameter_zone ? '⌀' : ''}${trimNumber(annotation.tolerance)}${modifier}${datums ? ` | ${datums}` : ''}${annotation.projected_zone ? ` Ⓟ${trimNumber(annotation.projected_zone)}` : ''}${annotation.free_state ? ' Ⓕ' : ''}`;
}

function weldSymbolPath(
  weldType: Extract<DrawingAnnotationDto, { kind: 'weld_symbol' }>['weld_type'],
  position: [number, number],
): string {
  const [x, y] = position;
  switch (weldType) {
    case 'fillet': return `M${round(x + 10)},${round(y)} l4,-4 l0,4`;
    case 'square_groove': return `M${round(x + 10)},${round(y - 4)} v8 M${round(x + 14)},${round(y - 4)} v8`;
    case 'v_groove': return `M${round(x + 10)},${round(y - 4)} l2,4 l2,-4`;
    case 'bevel_groove': return `M${round(x + 10)},${round(y - 4)} v4 l4,-4`;
    case 'u_groove': return `M${round(x + 10)},${round(y - 4)} q0,4 2,4 q2,0 2,-4`;
    case 'j_groove': return `M${round(x + 10)},${round(y - 4)} q0,4 2,4 l2,-4`;
    case 'plug_slot': return `M${round(x + 10)},${round(y - 3)} h5 v6 h-5 z`;
    case 'spot': return `M${round(x + 12)},${round(y)} m-2,0 a2,2 0 1,0 4,0 a2,2 0 1,0 -4,0`;
    case 'seam': return `M${round(x + 10)},${round(y)} h6 M${round(x + 12)},${round(y - 2)} v4 M${round(x + 14)},${round(y - 2)} v4`;
    case 'surfacing': return `M${round(x + 10)},${round(y)} q2,-4 4,0`;
  }
}

function outlinedText(
  position: [number, number],
  text: string,
  anchor: 'start' | 'middle' | 'end' = 'middle',
  transform = '',
): string {
  return `<text x="${round(position[0])}" y="${round(position[1])}"${transform ? ` transform="${transform}"` : ''} fill="#23272d" stroke="white" stroke-width="1.6" paint-order="stroke" text-anchor="${anchor}">${escapeXml(text)}</text>`;
}

function dimensionValueText(
  position: [number, number],
  text: string,
  textHeight: number,
  anchor: 'start' | 'middle' | 'end' = 'middle',
  transform = '',
  maskDimensionLine = true,
): string {
  const width = drawingDimensionTextWidth(text, textHeight);
  const paddingY = 0.75;
  const height = textHeight * 1.18 + paddingY * 2;
  const x = anchor === 'middle'
    ? position[0] - width / 2
    : anchor === 'start'
      ? position[0] - 0.8
      : position[0] - width + 0.8;
  const y = position[1] - textHeight * 0.94 - paddingY;
  const transformAttribute = transform ? ` transform="${transform}"` : '';
  if (!maskDimensionLine) {
    return `<text x="${round(position[0])}" y="${round(position[1])}"${transformAttribute} fill="#23272d" stroke="none" text-anchor="${anchor}">${escapeXml(text)}</text>`;
  }
  return `<rect class="nbs-dimension-text-mask" x="${round(x)}" y="${round(y)}" width="${round(width)}" height="${round(height)}" fill="white" stroke="none"${transformAttribute}/>${outlinedText(position, text, anchor, transform)}`;
}

function multilineText(position: [number, number], text: string): string {
  const spans = text.split('\n').map((line, index) => `<tspan x="${round(position[0])}" dy="${index === 0 ? 0 : '1.25em'}">${escapeXml(line)}</tspan>`).join('');
  return `<text x="${round(position[0])}" y="${round(position[1])}" fill="#23272d" stroke="white" stroke-width="1.6" paint-order="stroke" text-anchor="start">${spans}</text>`;
}

function noteSvg(note: Extract<DrawingAnnotationDto, { kind: 'note' }>): string {
  const lines = note.text.split('\n');
  const spans = lines.map((line, index) =>
    `<tspan x="${round(note.position[0])}" dy="${index === 0 ? 0 : '1.25em'}">${escapeXml(line)}</tspan>`,
  ).join('');
  return `<text x="${round(note.position[0])}" y="${round(note.position[1])}" fill="#23272d">${spans}</text>`;
}

function viewSvg(view: DrawingViewDto, projection: DrawingProjectionDto, sheet: DrawingSheetDto): string {
  const visible = projection.visible.map((polyline) => pathSvg(polyline, 'visible')).join('');
  const hidden = projection.hidden.map((polyline) => pathSvg(polyline, 'hidden')).join('');
  const section = projection.section.map((polyline) => pathSvg(polyline, 'visible')).join('');
  const sectionLike = view.derivation?.type === 'section' || view.derivation?.type === 'removed_section';
  const removed = view.derivation?.type === 'removed_section';
  const detail = view.derivation?.type === 'detail' ? view.derivation : null;
  const detailCenter = detail ? resolveDrawingAnchor(detail.center, view, projection)?.paper ?? null : null;
  const detailRadius = detail ? detail.radius * view.scale : 0;
  const sectionRegion = sectionLike ? sectionRegionSvgPath(view, projection) : '';
  const sectionClipId = `nbs-section-${view.id}`;
  const detailClipId = `nbs-detail-${view.id}`;
  const hatchId = `nbs-hatch-${view.id}`;
  const [x, y, width, height] = drawingViewPaperBounds(view, projection);
  const angle = view.derivation && 'hatch_angle_deg' in view.derivation
    ? view.derivation.hatch_angle_deg
    : sheet.style.hatch_angle_deg;
  const spacing = view.derivation && 'hatch_spacing_mm' in view.derivation
    ? view.derivation.hatch_spacing_mm
    : sheet.style.hatch_spacing_mm;
  const definitions = `${detailCenter ? `<clipPath id="${detailClipId}"><circle cx="${round(detailCenter[0])}" cy="${round(detailCenter[1])}" r="${round(detailRadius)}"/></clipPath>` : ''}${sectionRegion ? `<clipPath id="${sectionClipId}"><path d="${sectionRegion}" fill-rule="evenodd"/></clipPath><pattern id="${hatchId}" patternUnits="userSpaceOnUse" width="${round(spacing)}" height="${round(spacing)}" patternTransform="rotate(${round(angle)})"><line class="nbs-hatch" x1="0" y1="0" x2="0" y2="${round(spacing)}" stroke="#65717c"/></pattern>` : ''}`;
  const projectionGraphics = `<g${detailCenter ? ` clip-path="url(#${detailClipId})"` : ''}>${sectionRegion ? `<rect x="${round(x)}" y="${round(y)}" width="${round(Math.max(width, 1))}" height="${round(Math.max(height, 1))}" fill="url(#${hatchId})" clip-path="url(#${sectionClipId})"/>` : ''}${!removed ? `<g transform="${drawingViewTransform(view, projection)}">${visible}${hidden}</g>` : ''}${sectionLike ? `<g transform="${drawingViewTransform(view, projection)}">${section}</g>` : ''}${view.derivation?.type === 'broken' ? brokenViewSvg(view, projection) : ''}</g>`;
  return `
    <defs>${definitions}</defs>
    ${projectionGraphics}
    ${detailCenter ? `<circle class="nbs-phantom" cx="${round(detailCenter[0])}" cy="${round(detailCenter[1])}" r="${round(detailRadius)}" fill="none"/>` : ''}
    <text x="${view.position[0]}" y="${view.position[1] + Math.max(8, (projection.bounds[3] - projection.bounds[1]) * view.scale / 2 + 5)}" fill="#30343a" stroke="none" font-family="${escapeXml(sheet.style.font_family)}" font-size="${round(sheet.style.small_text_height_mm)}" text-anchor="middle">${escapeXml(view.name)} · ${scaleLabel(view.scale)}</text>`;
}

function pathSvg(polyline: DrawingPolylineDto, role: DrawingLineRole): string {
  if (polyline.points.length < 2) return '';
  const data = polyline.points
    .map(([x, y], index) => `${index === 0 ? 'M' : 'L'}${round(x)} ${round(y)}`)
    .join(' ');
  return `<path class="nbs-${role}" d="${data}" vector-effect="non-scaling-stroke"${role === 'hidden' ? ' opacity="0.72"' : ''}/>`;
}

function borderAndTitleBlock(
  sheet: DrawingSheetDto,
  width: number,
  height: number,
): string {
  const blockWidth = Math.min(180, width - 10);
  const blockHeight = 44;
  const x = width - 5 - blockWidth;
  const y = height - 5 - blockHeight;
  const title = sheet.title_block;
  const tolerance = drawingToleranceNoteText(sheet.tolerance_note);
  return `<g fill="none" stroke="#30343a">
    <rect class="nbs-visible" x="5" y="5" width="${width - 10}" height="${height - 10}"/>
    <rect class="nbs-dimension" x="${x}" y="${y}" width="${blockWidth}" height="${blockHeight}"/>
    <path class="nbs-dimension" d="M${x} ${y + 15}H${x + blockWidth} M${x} ${y + 23}H${x + blockWidth} M${x} ${y + 31}H${x + blockWidth} M${x} ${y + 38}H${x + blockWidth} M${x + blockWidth * 0.62} ${y}V${y + 23} M${x + blockWidth * 0.78} ${y + 23}V${y + blockHeight} M${x + blockWidth * 0.9} ${y + 31}V${y + blockHeight}"/>
  </g>
  <g fill="#30343a" font-family="${escapeXml(sheet.style.font_family)}" font-size="${round(sheet.style.small_text_height_mm)}">
    <text x="${x + 3}" y="${y + 6.2}" font-size="${round(sheet.style.text_height_mm + 0.8)}" font-weight="650">${escapeXml(title.title || sheet.name)}</text>
    <text x="${x + 3}" y="${y + 12.2}">DRAWING: ${escapeXml(title.drawing_number || '—')}</text>
    <text x="${x + blockWidth * 0.64}" y="${y + 6.2}">SHEET: ${escapeXml(sheet.name)}</text>
    <text x="${x + blockWidth * 0.64}" y="${y + 12.2}">${drawingFormatShortLabel(sheet.format)} · ${sheet.projection_method === 'first_angle' ? '1ST ANGLE' : '3RD ANGLE'}</text>
    <text x="${x + 3}" y="${y + 20.4}">${escapeXml(tolerance || 'TOLERANCES: AS SPECIFIED')}</text>
    <text x="${x + 3}" y="${y + 28.2}">COMPANY: ${escapeXml(title.company || '—')}</text><text x="${x + blockWidth * 0.8}" y="${y + 28.2}">REV ${escapeXml(title.revision || '—')}</text>
    <text x="${x + 3}" y="${y + 35.6}">MATERIAL: ${escapeXml(title.material || '—')}</text><text x="${x + blockWidth * 0.8}" y="${y + 35.6}">FINISH: ${escapeXml(title.finish || '—')}</text>
    <text x="${x + 3}" y="${y + 42.2}">DRAWN: ${escapeXml(title.author || '—')}</text><text x="${x + blockWidth * 0.4}" y="${y + 42.2}">CHECKED: ${escapeXml(title.checked_by || '—')}</text><text x="${x + blockWidth * 0.79}" y="${y + 42.2}">APPROVED: ${escapeXml(title.approved_by || '—')}</text>
  </g>${revisionTableSvg(sheet)}${bomTableSvg(sheet)}`;
}

function drawingSvgStyleSheet(sheet: DrawingSheetDto): string {
  const roles: DrawingLineRole[] = ['visible', 'hidden', 'center', 'cutting_plane', 'phantom', 'break_line', 'dimension', 'extension', 'leader', 'hatch'];
  const rules = roles.map((role) => {
    const line = drawingLineStyle(sheet.style, role);
    const selector = [`.nbs-${role}`, `.nbs-${role} path`, `.nbs-${role} line`, `.nbs-${role} polyline`, `.nbs-${role} polygon`, `.nbs-${role} circle`, `.nbs-${role} rect`].join(',');
    return `${selector}{stroke-width:${round(line.width_mm)}!important;${line.dash_mm.length > 0 ? `stroke-dasharray:${line.dash_mm.map(round).join(' ')}!important;` : 'stroke-dasharray:none;' }}`;
  }).join('');
  return `<style>text{font-family:${escapeCss(sheet.style.font_family)};font-size:${round(sheet.style.text_height_mm)}}${rules}</style>`;
}

function annotationLineRole(annotation: DrawingAnnotationDto): DrawingLineRole {
  if (['center_mark', 'center_line', 'center_line_between_edges', 'automatic_symmetry_axis', 'bolt_circle_center_line'].includes(annotation.kind)) return 'center';
  if (annotation.kind === 'revision_cloud') return 'break_line';
  if (['hole_note', 'chamfer_note', 'datum_feature', 'gdt_frame', 'surface_texture', 'edge_requirement', 'weld_symbol', 'item_balloon', 'note'].includes(annotation.kind)) return 'leader';
  return 'dimension';
}

function derivedSourceSvg(child: DrawingViewDto, parent: DrawingViewDto, projection: DrawingProjectionDto, sheet: DrawingSheetDto): string {
  const derivation = child.derivation;
  if (!derivation) return '';
  const color = '#5d50c8';
  if (derivation.type === 'section' || derivation.type === 'removed_section') {
    const first = resolveDrawingAnchor(derivation.first, parent, projection);
    const second = resolveDrawingAnchor(derivation.second, parent, projection);
    if (!first || !second) return brokenAnnotationSvg(parent);
    const direction = normalize2([second.paper[0] - first.paper[0], second.paper[1] - first.paper[1]]);
    const normal: [number, number] = [-direction[1], direction[0]];
    const firstArrow: [number, number] = [first.paper[0] + normal[0] * 5, first.paper[1] + normal[1] * 5];
    const secondArrow: [number, number] = [second.paper[0] + normal[0] * 5, second.paper[1] + normal[1] * 5];
    const labelParts = derivation.label.trim().split(/\s+/);
    const label = labelParts[labelParts.length - 1] || derivation.label;
    return `<g class="nbs-cutting_plane" fill="${color}" stroke="${color}"><path d="M${point(first.paper)}L${point(second.paper)}" fill="none"/><polygon points="${arrowPolygon(first.paper, firstArrow, sheet.style.arrow_size_mm)}"/><polygon points="${arrowPolygon(second.paper, secondArrow, sheet.style.arrow_size_mm)}"/>${outlinedText([first.paper[0] - direction[0] * 4, first.paper[1] - direction[1] * 4], label)}${outlinedText([second.paper[0] + direction[0] * 4, second.paper[1] + direction[1] * 4], label)}</g>`;
  }
  if (derivation.type === 'detail') {
    const center = resolveDrawingAnchor(derivation.center, parent, projection)?.paper;
    if (!center) return brokenAnnotationSvg(parent);
    const radius = derivation.radius * parent.scale;
    return `<g class="nbs-phantom" fill="none" stroke="${color}"><circle cx="${round(center[0])}" cy="${round(center[1])}" r="${round(radius)}"/>${outlinedText([center[0] + radius + 3, center[1] - radius - 1], derivation.label, 'start')}</g>`;
  }
  if (derivation.type === 'auxiliary') {
    const line = resolveDrawingLine(derivation.reference, parent, projection);
    if (!line) return brokenAnnotationSvg(parent);
    const center: [number, number] = [(line.start[0] + line.end[0]) / 2, (line.start[1] + line.end[1]) / 2];
    const direction = normalize2([line.end[0] - line.start[0], line.end[1] - line.start[1]]);
    const normal: [number, number] = [-direction[1], direction[0]];
    const sign = derivation.flipped ? -1 : 1;
    const end: [number, number] = [center[0] + normal[0] * sign * 8, center[1] + normal[1] * sign * 8];
    return `<g class="nbs-phantom" fill="${color}" stroke="${color}"><path d="M${point(line.start)}L${point(line.end)} M${point(center)}L${point(end)}" fill="none"/><polygon points="${arrowPolygon(end, center, sheet.style.arrow_size_mm)}"/>${outlinedText([end[0] + normal[0] * sign * 3, end[1] + normal[1] * sign * 3], derivation.label)}</g>`;
  }
  if (derivation.type === 'broken') {
    const [x, y, width, height] = drawingViewPaperBounds(parent, projection);
    const center = derivation.axis === 'horizontal' ? x + width / 2 : y + height / 2;
    const path = derivation.axis === 'horizontal'
      ? breakZigzagSvgPath(center, y, y + height, 'vertical')
      : breakZigzagSvgPath(center, x, x + width, 'horizontal');
    return `<path class="nbs-break_line" d="${path}" fill="none" stroke="${color}"/>`;
  }
  return '';
}

function brokenViewSvg(view: DrawingViewDto, projection: DrawingProjectionDto): string {
  if (view.derivation?.type !== 'broken') return '';
  const [x, y, width, height] = drawingViewPaperBounds(view, projection);
  const gap = Math.max(3, view.derivation.gap_mm);
  if (view.derivation.axis === 'horizontal') {
    const center = x + width / 2;
    return `<g><rect x="${round(center - gap / 2)}" y="${round(y - 1)}" width="${round(gap)}" height="${round(height + 2)}" fill="white" stroke="none"/><path class="nbs-break_line" d="${breakZigzagSvgPath(center - gap / 2, y, y + height, 'vertical')} ${breakZigzagSvgPath(center + gap / 2, y, y + height, 'vertical')}" fill="none"/></g>`;
  }
  const center = y + height / 2;
  return `<g><rect x="${round(x - 1)}" y="${round(center - gap / 2)}" width="${round(width + 2)}" height="${round(gap)}" fill="white" stroke="none"/><path class="nbs-break_line" d="${breakZigzagSvgPath(center - gap / 2, x, x + width, 'horizontal')} ${breakZigzagSvgPath(center + gap / 2, x, x + width, 'horizontal')}" fill="none"/></g>`;
}

function breakZigzagSvgPath(position: number, start: number, end: number, orientation: 'horizontal' | 'vertical'): string {
  const middle = (start + end) / 2;
  return orientation === 'vertical'
    ? `M${round(position)} ${round(start)}L${round(position)} ${round(middle - 4)}l-2 2 4 2-4 2 2 2L${round(position)} ${round(end)}`
    : `M${round(start)} ${round(position)}L${round(middle - 4)} ${round(position)}l2 -2 2 4 2-4 2 2L${round(end)} ${round(position)}`;
}

function sectionRegionSvgPath(view: DrawingViewDto, projection: DrawingProjectionDto): string {
  const segments = projection.section.flatMap((polyline) => polyline.points.slice(1).map((end, index) => [
    projectedPaperPoint(view, projection, polyline.points[index]),
    projectedPaperPoint(view, projection, end),
  ] as [[number, number], [number, number]])).filter(([first, second]) => Math.hypot(first[0] - second[0], first[1] - second[1]) > 1e-5);
  const key = (value: [number, number]) => `${Math.round(value[0] * 1_000)},${Math.round(value[1] * 1_000)}`;
  const adjacency = new Map<string, number[]>();
  segments.forEach((segment, index) => segment.forEach((value) => adjacency.set(key(value), [...(adjacency.get(key(value)) ?? []), index])));
  const used = new Set<number>();
  const paths: string[] = [];
  for (let seed = 0; seed < segments.length; seed += 1) {
    if (used.has(seed)) continue;
    used.add(seed);
    const chain = [segments[seed][0], segments[seed][1]];
    for (let guard = 0; guard < segments.length; guard += 1) {
      const end = chain[chain.length - 1];
      if (chain.length > 2 && key(end) === key(chain[0])) break;
      const nextIndex = (adjacency.get(key(end)) ?? []).find((index) => !used.has(index));
      if (nextIndex === undefined) break;
      used.add(nextIndex);
      const next = segments[nextIndex];
      chain.push(key(next[0]) === key(end) ? next[1] : next[0]);
    }
    if (chain.length >= 4 && key(chain[0]) === key(chain[chain.length - 1])) paths.push(`M${chain.map(point).join('L')}Z`);
  }
  return paths.join(' ');
}

function projectedPaperPoint(view: DrawingViewDto, projection: DrawingProjectionDto, value: [number, number]): [number, number] {
  const centerX = (projection.bounds[0] + projection.bounds[2]) / 2;
  const centerY = (projection.bounds[1] + projection.bounds[3]) / 2;
  return [view.position[0] + (value[0] - centerX) * view.scale, view.position[1] - (value[1] - centerY) * view.scale];
}

function revisionTableSvg(sheet: DrawingSheetDto): string {
  if (!sheet.revision_table_position) return '';
  const [x, y] = sheet.revision_table_position;
  const rowHeight = 6;
  const width = 112;
  const rows = sheet.revisions;
  const height = rowHeight * (rows.length + 1);
  const horizontal = Array.from({ length: rows.length }, (_, index) => `M0 ${(index + 1) * rowHeight}H${width}`).join(' ');
  const text = rows.map((revision, index) => `<text x="2" y="${(index + 1) * rowHeight + 4.2}">${escapeXml(revision.revision)}</text><text x="14" y="${(index + 1) * rowHeight + 4.2}">${escapeXml(revision.date)}</text><text x="30" y="${(index + 1) * rowHeight + 4.2}">${escapeXml(revision.description || revision.change_order || '—')}</text>`).join('');
  return `<g transform="translate(${round(x)} ${round(y)})" font-size="${round(sheet.style.small_text_height_mm)}"><g class="nbs-dimension" fill="white" stroke="#30343a"><rect width="${width}" height="${height}"/><path d="${horizontal} M12 0V${height} M28 0V${height}"/></g><text x="2" y="4.2" font-weight="700">REV</text><text x="14" y="4.2" font-weight="700">DATE</text><text x="30" y="4.2" font-weight="700">DESCRIPTION / APPROVAL</text>${text}</g>`;
}

function bomTableSvg(sheet: DrawingSheetDto): string {
  if (!sheet.bom_table_position) return '';
  const [x, y] = sheet.bom_table_position;
  const rowHeight = 6;
  const width = 132;
  const height = rowHeight * (sheet.bom.length + 1);
  const horizontal = Array.from({ length: sheet.bom.length }, (_, index) => `M0 ${(index + 1) * rowHeight}H${width}`).join(' ');
  const text = sheet.bom.map((item, index) => `<text x="2" y="${(index + 1) * rowHeight + 4.2}">${escapeXml(item.item_number)}</text><text x="14" y="${(index + 1) * rowHeight + 4.2}">${escapeXml(item.part_number || '—')}</text><text x="42" y="${(index + 1) * rowHeight + 4.2}">${escapeXml(item.description)}</text><text x="102" y="${(index + 1) * rowHeight + 4.2}">${trimNumber(item.quantity)}</text><text x="114" y="${(index + 1) * rowHeight + 4.2}">${escapeXml(item.material || '—')}</text>`).join('');
  return `<g transform="translate(${round(x)} ${round(y)})" font-size="${round(sheet.style.small_text_height_mm)}"><g class="nbs-dimension" fill="white" stroke="#30343a"><rect width="${width}" height="${height}"/><path d="${horizontal} M12 0V${height} M40 0V${height} M100 0V${height} M112 0V${height}"/></g><text x="2" y="4.2" font-weight="700">ITEM</text><text x="14" y="4.2" font-weight="700">PART</text><text x="42" y="4.2" font-weight="700">DESCRIPTION</text><text x="102" y="4.2" font-weight="700">QTY</text><text x="114" y="4.2" font-weight="700">MATERIAL</text>${text}</g>`;
}

function normalize2(vector: [number, number]): [number, number] {
  const length = Math.hypot(vector[0], vector[1]);
  return length < 1e-8 ? [1, 0] : [vector[0] / length, vector[1] / length];
}

function trimNumber(value: number): string {
  return Number(value.toFixed(3)).toString();
}

function scaleLabel(scale: number): string {
  if (scale >= 1) return `${round(scale)}:1`;
  return `1:${round(1 / scale)}`;
}

function round(value: number): string {
  return Number(value.toFixed(5)).toString();
}

function point(value: [number, number]): string {
  return `${round(value[0])} ${round(value[1])}`;
}

function escapeXml(value: string): string {
  return value.replace(/[&<>"']/g, (character) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&apos;',
  })[character] ?? character);
}

function escapeCss(value: string): string {
  return value.replace(/[{}<>]/g, '').replace(/;/g, ',');
}

function safeFilePart(value: string): string {
  return value.trim().replace(/[^a-z0-9._-]+/gi, '-') || 'Drawing';
}
