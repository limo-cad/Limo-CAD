import type {
  DrawingAnnotationDto,
  DrawingDocumentDto,
  DrawingLineStyleDto,
  DrawingProjectionDto,
  DrawingSheetDto,
  DrawingSheetFormat,
  DrawingSheetOrientation,
  DrawingSheetStyleDto,
  DrawingStandard,
  DrawingTitleBlockDto,
  DrawingToleranceNoteDto,
  DrawingViewDto,
} from '../engine/types';

const PAPER: Record<DrawingSheetFormat, [number, number]> = {
  a0: [841, 1189],
  a1: [594, 841],
  a2: [420, 594],
  a3: [297, 420],
  a4: [210, 297],
  letter: [215.9, 279.4],
  ansi_b: [279.4, 431.8],
  ansi_c: [431.8, 558.8],
  ansi_d: [558.8, 863.6],
  ansi_e: [863.6, 1117.6],
};

export const ISO_DRAWING_FORMATS: DrawingSheetFormat[] = ['a4', 'a3', 'a2', 'a1', 'a0'];
export const ANSI_DRAWING_FORMATS: DrawingSheetFormat[] = ['letter', 'ansi_b', 'ansi_c', 'ansi_d', 'ansi_e'];

export function drawingFormatsForStandard(standard: DrawingStandard): DrawingSheetFormat[] {
  return standard === 'ansi' ? ANSI_DRAWING_FORMATS : ISO_DRAWING_FORMATS;
}

export function defaultDrawingFormat(standard: DrawingStandard): DrawingSheetFormat {
  return standard === 'ansi' ? 'letter' : 'a4';
}

export function defaultDrawingSheetStyle(): DrawingSheetStyleDto {
  const continuous = (width_mm: number) => ({ width_mm, dash_mm: [] });
  const dashed = (width_mm: number, dash_mm: number[]) => ({ width_mm, dash_mm });
  return {
    name: 'noBS CAD Default',
    font_family: 'Arial, Helvetica, sans-serif',
    text_height_mm: 3.5,
    small_text_height_mm: 2.5,
    arrow_size_mm: 2.5,
    visible: continuous(0.5),
    hidden: dashed(0.25, [4, 2]),
    center: dashed(0.25, [8, 1.5, 1.2, 1.5]),
    cutting_plane: dashed(0.7, [10, 2, 2, 2]),
    phantom: dashed(0.25, [10, 1.5, 1.2, 1.5, 1.2, 1.5]),
    break_line: continuous(0.35),
    dimension: continuous(0.25),
    extension: continuous(0.25),
    leader: continuous(0.25),
    hatch: continuous(0.18),
    hatch_angle_deg: 45,
    hatch_spacing_mm: 2.5,
  };
}

function normalizedLineStyle(
  candidate: DrawingLineStyleDto | undefined,
  fallback: DrawingLineStyleDto,
): DrawingLineStyleDto {
  return {
    width_mm: Number.isFinite(candidate?.width_mm) ? candidate!.width_mm : fallback.width_mm,
    dash_mm: Array.isArray(candidate?.dash_mm) ? candidate!.dash_mm : fallback.dash_mm,
  };
}

function normalizedSheetStyle(style: DrawingSheetStyleDto | undefined): DrawingSheetStyleDto {
  const fallback = defaultDrawingSheetStyle();
  return {
    ...fallback,
    ...style,
    visible: normalizedLineStyle(style?.visible, fallback.visible),
    hidden: normalizedLineStyle(style?.hidden, fallback.hidden),
    center: normalizedLineStyle(style?.center, fallback.center),
    cutting_plane: normalizedLineStyle(style?.cutting_plane, fallback.cutting_plane),
    phantom: normalizedLineStyle(style?.phantom, fallback.phantom),
    break_line: normalizedLineStyle(style?.break_line, fallback.break_line),
    dimension: normalizedLineStyle(style?.dimension, fallback.dimension),
    extension: normalizedLineStyle(style?.extension, fallback.extension),
    leader: normalizedLineStyle(style?.leader, fallback.leader),
    hatch: normalizedLineStyle(style?.hatch, fallback.hatch),
  };
}

function normalizedAnnotation(annotation: DrawingAnnotationDto): DrawingAnnotationDto {
  if (
    annotation.kind === 'linear_dimension'
    || annotation.kind === 'line_dimension'
    || annotation.kind === 'point_line_dimension'
    || annotation.kind === 'radial_dimension'
    || annotation.kind === 'angular_dimension'
    || annotation.kind === 'chain_dimension'
    || annotation.kind === 'ordinate_dimension'
    || annotation.kind === 'arc_length_dimension'
    || annotation.kind === 'jogged_radius_dimension'
  ) {
    return {
      ...annotation,
      presentation: annotation.presentation ?? {
        tolerance: { mode: 'none', upper: 0, lower: 0 },
        basic: false,
        reference: false,
        dual_units: null,
        fit_class: '',
      },
    } as DrawingAnnotationDto;
  }
  return annotation;
}

function normalizedTitleBlock(
  title: DrawingTitleBlockDto | undefined,
): DrawingTitleBlockDto {
  return {
    title: title?.title ?? '',
    drawing_number: title?.drawing_number ?? '',
    revision: title?.revision ?? 'A',
    author: title?.author ?? '',
    checked_by: title?.checked_by ?? '',
    approved_by: title?.approved_by ?? '',
    company: title?.company ?? '',
    material: title?.material ?? '',
    finish: title?.finish ?? '',
  };
}

/**
 * Upgrade additive drawing-schema fields at the single frontend boundary.
 *
 * Saved projects are already migrated by Rust, but this also protects the UI
 * while a development browser is briefly running an older cached WASM engine.
 * Keeping the compatibility logic here prevents every drawing panel and
 * exporter from growing its own partial-document checks.
 */
export function normalizeDrawingDocument(drawing: DrawingDocumentDto): DrawingDocumentDto {
  const fallbackStyle = defaultDrawingSheetStyle();
  const sheets = (drawing.sheets ?? []).map((sheet) => {
    const standard = sheet.standard ?? 'iso';
    return {
      ...sheet,
      format: sheet.format ?? defaultDrawingFormat(standard),
      orientation: sheet.orientation ?? 'landscape',
      standard,
      projection_method: sheet.projection_method
        ?? (standard === 'ansi' ? 'third_angle' : 'first_angle'),
      tolerance_note: sheet.tolerance_note ?? {
        preset: standard === 'ansi' ? 'ansi_decimal' : 'iso2768_medium',
        custom: '',
      },
      title_block: normalizedTitleBlock(sheet.title_block),
      views: (sheet.views ?? []).map((view) => ({
        ...view,
        scope: view.scope ?? 'definition',
        occurrence_ids: view.occurrence_ids ?? [],
        body_ids: view.body_ids ?? [],
        show_hidden_lines: view.show_hidden_lines ?? false,
        show_tangent_edges: view.show_tangent_edges ?? true,
        parent_view_id: view.parent_view_id ?? null,
        alignment: view.alignment ?? 'free',
        derivation: view.derivation ?? null,
      })),
      annotations: (sheet.annotations ?? []).map(normalizedAnnotation),
      style: normalizedSheetStyle(sheet.style),
      template_name: sheet.template_name ?? fallbackStyle.name,
      revisions: sheet.revisions ?? [],
      bom: sheet.bom ?? [],
      release: sheet.release ?? {
        status: 'draft',
        released_revision: '',
        released_at: '',
      },
      revision_table_position: sheet.revision_table_position ?? null,
      bom_table_position: sheet.bom_table_position ?? null,
    } satisfies DrawingSheetDto;
  });

  const templates = (drawing.templates ?? []).map((template) => ({
    ...template,
    projection_method: template.projection_method
      ?? (template.standard === 'ansi' ? 'third_angle' : 'first_angle'),
    tolerance_note: template.tolerance_note ?? {
      preset: template.standard === 'ansi' ? 'ansi_decimal' : 'iso2768_medium',
      custom: '',
    },
    title_defaults: normalizedTitleBlock(template.title_defaults),
    style: normalizedSheetStyle(template.style),
  }));

  const highestSheetId = Math.max(0, ...sheets.map((sheet) => sheet.id));
  const highestViewId = Math.max(0, ...sheets.flatMap((sheet) => sheet.views.map((view) => view.id)));
  const highestAnnotationId = Math.max(
    0,
    ...sheets.flatMap((sheet) => sheet.annotations.map((annotation) => annotation.id)),
  );
  const highestRevisionId = Math.max(
    0,
    ...sheets.flatMap((sheet) => sheet.revisions.map((revision) => revision.id)),
  );
  const highestBomItemId = Math.max(
    0,
    ...sheets.flatMap((sheet) => sheet.bom.map((item) => item.id)),
  );
  const highestTemplateId = Math.max(0, ...templates.map((template) => template.id));
  const activeSheetId = drawing.active_sheet_id !== null
    && sheets.some((sheet) => sheet.id === drawing.active_sheet_id)
    ? drawing.active_sheet_id
    : (sheets[0]?.id ?? null);

  return {
    sheets,
    active_sheet_id: activeSheetId,
    next_sheet_id: Math.max(drawing.next_sheet_id ?? 1, highestSheetId + 1),
    next_view_id: Math.max(drawing.next_view_id ?? 1, highestViewId + 1),
    next_annotation_id: Math.max(drawing.next_annotation_id ?? 1, highestAnnotationId + 1),
    next_revision_id: Math.max(drawing.next_revision_id ?? 1, highestRevisionId + 1),
    next_bom_item_id: Math.max(drawing.next_bom_item_id ?? 1, highestBomItemId + 1),
    templates,
    next_template_id: Math.max(drawing.next_template_id ?? 1, highestTemplateId + 1),
  };
}

export function drawingFormatLabel(format: DrawingSheetFormat): string {
  const labels: Record<DrawingSheetFormat, string> = {
    a0: 'ISO A0 · 841 × 1189 mm',
    a1: 'ISO A1 · 594 × 841 mm',
    a2: 'ISO A2 · 420 × 594 mm',
    a3: 'ISO A3 · 297 × 420 mm',
    a4: 'ISO A4 · 210 × 297 mm',
    letter: 'ANSI A · 8.5 × 11 in',
    ansi_b: 'ANSI B · 11 × 17 in',
    ansi_c: 'ANSI C · 17 × 22 in',
    ansi_d: 'ANSI D · 22 × 34 in',
    ansi_e: 'ANSI E · 34 × 44 in',
  };
  return labels[format];
}

export function drawingFormatShortLabel(format: DrawingSheetFormat): string {
  if (format === 'letter') return 'ANSI A';
  if (format.startsWith('ansi_')) return `ANSI ${format.slice(-1).toUpperCase()}`;
  return `ISO ${format.toUpperCase()}`;
}

export function drawingToleranceNoteText(note: DrawingToleranceNoteDto): string {
  switch (note.preset) {
    case 'iso2768_fine': return 'GENERAL TOLERANCES ISO 2768-f';
    case 'iso2768_medium': return 'GENERAL TOLERANCES ISO 2768-m';
    case 'iso2768_coarse': return 'GENERAL TOLERANCES ISO 2768-c';
    case 'iso2768_very_coarse': return 'GENERAL TOLERANCES ISO 2768-v';
    case 'ansi_decimal': return 'UNLESS OTHERWISE SPECIFIED: .X ±.1  .XX ±.01  .XXX ±.005';
    case 'custom': return note.custom.trim();
    case 'none': return '';
  }
}

export function drawingSheetSize(
  format: DrawingSheetFormat,
  orientation: DrawingSheetOrientation,
): [number, number] {
  const [shortEdge, longEdge] = PAPER[format];
  return orientation === 'landscape'
    ? [longEdge, shortEdge]
    : [shortEdge, longEdge];
}

export function drawingViewTransform(
  view: DrawingViewDto,
  projection: DrawingProjectionDto,
): string {
  const centerX = (projection.bounds[0] + projection.bounds[2]) / 2;
  const centerY = (projection.bounds[1] + projection.bounds[3]) / 2;
  return `translate(${view.position[0]} ${view.position[1]}) scale(${view.scale} ${-view.scale}) translate(${-centerX} ${-centerY})`;
}

export function drawingViewPaperBounds(
  view: DrawingViewDto,
  projection: DrawingProjectionDto,
): [number, number, number, number] {
  const width = (projection.bounds[2] - projection.bounds[0]) * view.scale;
  const height = (projection.bounds[3] - projection.bounds[1]) * view.scale;
  return [
    view.position[0] - width / 2,
    view.position[1] - height / 2,
    width,
    height,
  ];
}

export function activeSheetOf(
  sheets: DrawingSheetDto[],
  activeId: number | null,
): DrawingSheetDto | null {
  return sheets.find((sheet) => sheet.id === activeId) ?? null;
}
