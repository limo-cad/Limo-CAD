//! Presentation of shared sheet metadata without changing the saved document.
use super::{
    annotations::{Art, CheckedArt},
    Fill, Ink, Label, LabelAlign,
};
use limo_cad_sketch::*;
use std::borrow::Cow;

const MIN_TEXT: f64 = 1.8;

/// Frame rendering consumes only its visible metadata. Borrow it during cache
/// lookup and retain an owned snapshot only when the frame changes.
#[derive(PartialEq)]
pub(super) struct Source<'a> {
    paper_size: [f64; 2],
    name: Cow<'a, str>,
    format: DrawingSheetFormat,
    projection_method: DrawingProjectionMethod,
    tolerance_note: Cow<'a, DrawingToleranceNoteDto>,
    title_block: Cow<'a, DrawingTitleBlockDto>,
    style: Style<'a>,
    revisions: Cow<'a, [DrawingRevisionDto]>,
    bom: Cow<'a, [DrawingBomItemDto]>,
    revision_table_position: Option<[f64; 2]>,
    bom_table_position: Option<[f64; 2]>,
}

#[derive(PartialEq)]
struct Style<'a> {
    visible: Cow<'a, DrawingLineStyleDto>,
    dimension: Cow<'a, DrawingLineStyleDto>,
    text_height_mm: f64,
    small_text_height_mm: f64,
}

impl<'a> Source<'a> {
    pub(super) fn new(sheet: &'a DrawingSheetDto, paper_size: [f64; 2]) -> Self {
        Self {
            paper_size,
            name: Cow::Borrowed(&sheet.name),
            format: sheet.format,
            projection_method: sheet.projection_method,
            tolerance_note: Cow::Borrowed(&sheet.tolerance_note),
            title_block: Cow::Borrowed(&sheet.title_block),
            style: Style {
                visible: Cow::Borrowed(&sheet.style.visible),
                dimension: Cow::Borrowed(&sheet.style.dimension),
                text_height_mm: sheet.style.text_height_mm,
                small_text_height_mm: sheet.style.small_text_height_mm,
            },
            revisions: Cow::Borrowed(if sheet.revision_table_position.is_some() {
                &sheet.revisions
            } else {
                &[]
            }),
            bom: Cow::Borrowed(if sheet.bom_table_position.is_some() {
                &sheet.bom
            } else {
                &[]
            }),
            revision_table_position: sheet.revision_table_position,
            bom_table_position: sheet.bom_table_position,
        }
    }

    pub(super) fn into_owned(self) -> Source<'static> {
        Source {
            paper_size: self.paper_size,
            name: Cow::Owned(self.name.into_owned()),
            format: self.format,
            projection_method: self.projection_method,
            tolerance_note: Cow::Owned(self.tolerance_note.into_owned()),
            title_block: Cow::Owned(self.title_block.into_owned()),
            style: Style {
                visible: Cow::Owned(self.style.visible.into_owned()),
                dimension: Cow::Owned(self.style.dimension.into_owned()),
                text_height_mm: self.style.text_height_mm,
                small_text_height_mm: self.style.small_text_height_mm,
            },
            revisions: Cow::Owned(self.revisions.into_owned()),
            bom: Cow::Owned(self.bom.into_owned()),
            revision_table_position: self.revision_table_position,
            bom_table_position: self.bom_table_position,
        }
    }
}

fn preflight(art: &mut CheckedArt, sheet: &Source<'_>) {
    let rows = sheet
        .revision_table_position
        .map_or(0, |_| sheet.revisions.len())
        .saturating_add(sheet.bom_table_position.map_or(0, |_| sheet.bom.len()));
    if !art.frame_input(0, rows) {
        return;
    }
    let title = &sheet.title_block;
    for text in [
        sheet.name.as_ref(),
        &title.title,
        &title.drawing_number,
        &title.company,
        &title.revision,
        &title.material,
        &title.finish,
        &title.author,
        &title.checked_by,
        &title.approved_by,
        &sheet.tolerance_note.custom,
    ] {
        if !art.frame_input(text.len(), 0) {
            return;
        }
    }
    if sheet.revision_table_position.is_some() {
        for row in sheet.revisions.iter() {
            for text in [
                &row.revision,
                &row.date,
                &row.description,
                &row.change_order,
                &row.approved_by,
            ] {
                if !art.frame_input(text.len(), 0) {
                    return;
                }
            }
        }
    }
    if sheet.bom_table_position.is_some() {
        for row in sheet.bom.iter() {
            for text in [
                &row.item_number,
                &row.part_number,
                &row.description,
                &row.material,
            ] {
                if !art.frame_input(text.len(), 0) {
                    return;
                }
            }
        }
    }
}

fn stroke(art: &mut CheckedArt, a: [f64; 2], b: [f64; 2], style: &DrawingLineStyleDto) {
    art.styled_path(&[a, b], style, Ink::Frame);
}
fn rectangle(art: &mut CheckedArt, x: f64, y: f64, w: f64, h: f64, style: &DrawingLineStyleDto) {
    art.styled_path(
        &[[x, y], [x + w, y], [x + w, y + h], [x, y + h], [x, y]],
        style,
        Ink::Frame,
    );
}
fn label(art: &mut CheckedArt, x: f64, baseline: f64, text: String, size: f64, overflow: bool) {
    if !art.ready() {
        return;
    }
    let width = (advance(&text) * size).max(size);
    art.push_label(Label {
        x: (x + width * 0.5) as f32,
        y: (baseline - size * 0.4) as f32,
        text,
        width_mm: width as f32,
        height_mm: (size * 1.18) as f32,
        text_height_mm: size as f32,
        align: LabelAlign::Start,
        ink: if overflow {
            Ink::Overflow
        } else {
            Ink::FrameText
        },
        ..Default::default()
    });
}

/// Conservative font-independent advances for the title layout.
fn advance(text: &str) -> f64 {
    text.chars()
        .map(|c| {
            if c.is_whitespace() {
                0.33
            } else if "ilI.,:;!|'`".contains(c) {
                0.32
            } else if "MW@%".contains(c) {
                0.95
            } else if c.is_ascii_uppercase() {
                0.75
            } else if c.is_ascii() {
                0.65
            } else {
                1.
            }
        })
        .sum()
}
fn wrap(text: &str, max_advance: f64) -> Vec<String> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = Vec::new();
    for paragraph in normalized.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if !line.is_empty() && advance(&format!("{line} {word}")) <= max_advance {
                line.push(' ');
                line.push_str(word);
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            for c in word.chars() {
                if !line.is_empty() && advance(&line) + advance(&c.to_string()) > max_advance {
                    lines.push(std::mem::take(&mut line));
                }
                line.push(c);
            }
        }
        lines.push(line);
    }
    lines
}
fn cell(art: &mut CheckedArt, text: String, bounds: [f64; 4], requested_size: f64) {
    if !art.ready() {
        return;
    }
    let [x, y, w, h] = bounds;
    let maximum = if requested_size.is_finite() {
        requested_size.clamp(MIN_TEXT, 5.)
    } else {
        MIN_TEXT
    };
    let mut size = maximum;
    loop {
        let lines = wrap(&text, (w - 3.) / size);
        if size + (lines.len().saturating_sub(1) as f64) * size * 1.2 <= h - 2. + 1e-8
            && lines
                .iter()
                .all(|line| advance(line) * size <= w - 3. + 1e-8)
        {
            for (index, text) in lines.into_iter().enumerate() {
                label(
                    art,
                    x + 1.5,
                    y + 1. + size * 0.85 + index as f64 * size * 1.2,
                    text,
                    size,
                    false,
                );
            }
            return;
        }
        if size <= MIN_TEXT {
            break;
        }
        size = (size - 0.1).max(MIN_TEXT);
    }
    label(
        art,
        x + 1.5,
        y + 1. + MIN_TEXT,
        "! TEXT TOO LONG".into(),
        MIN_TEXT,
        true,
    );
}
fn or_dash(text: &str) -> &str {
    if text.is_empty() {
        "—"
    } else {
        text
    }
}
fn tolerance(note: &DrawingToleranceNoteDto) -> &str {
    match note.preset {
        DrawingTolerancePreset::None => "TOLERANCES: AS SPECIFIED",
        DrawingTolerancePreset::Iso2768Fine => "GENERAL TOLERANCES ISO 2768-f",
        DrawingTolerancePreset::Iso2768Medium => "GENERAL TOLERANCES ISO 2768-m",
        DrawingTolerancePreset::Iso2768Coarse => "GENERAL TOLERANCES ISO 2768-c",
        DrawingTolerancePreset::Iso2768VeryCoarse => "GENERAL TOLERANCES ISO 2768-v",
        DrawingTolerancePreset::AnsiDecimal => {
            "UNLESS OTHERWISE SPECIFIED: .X ±.1  .XX ±.01  .XXX ±.005"
        }
        DrawingTolerancePreset::Custom => {
            if note.custom.trim().is_empty() {
                "TOLERANCES: AS SPECIFIED"
            } else {
                note.custom.trim()
            }
        }
    }
}
fn format_label(format: DrawingSheetFormat) -> &'static str {
    match format {
        DrawingSheetFormat::A0 => "ISO A0",
        DrawingSheetFormat::A1 => "ISO A1",
        DrawingSheetFormat::A2 => "ISO A2",
        DrawingSheetFormat::A3 => "ISO A3",
        DrawingSheetFormat::A4 => "ISO A4",
        DrawingSheetFormat::Letter => "ANSI A",
        DrawingSheetFormat::AnsiB => "ANSI B",
        DrawingSheetFormat::AnsiC => "ANSI C",
        DrawingSheetFormat::AnsiD => "ANSI D",
        DrawingSheetFormat::AnsiE => "ANSI E",
    }
}
fn grid(
    art: &mut CheckedArt,
    position: [f64; 2],
    width: f64,
    rows: usize,
    columns: &[f64],
    line: &DrawingLineStyleDto,
) {
    let [x, y] = position;
    let height = rows as f64 * 6.;
    art.fill(Fill {
        x: x as f32,
        y: y as f32,
        width: width as f32,
        height: height as f32,
        round: false,
    });
    rectangle(art, x, y, width, height, line);
    for row in 1..rows {
        if !art.ready() {
            return;
        }
        stroke(
            art,
            [x, y + row as f64 * 6.],
            [x + width, y + row as f64 * 6.],
            line,
        );
    }
    for column in columns {
        stroke(art, [x + column, y], [x + column, y + height], line);
    }
}

pub(super) fn try_render(sheet: &Source<'_>) -> Result<Art, String> {
    render_checked(sheet).map_err(|error| format!("Drawing frame: {error}"))
}
fn render_checked(sheet: &Source<'_>) -> Result<Art, String> {
    let [paper_width, paper_height] = sheet.paper_size;
    let mut art = CheckedArt::default();
    preflight(&mut art, sheet);
    if !art.ready() {
        return art.finish();
    }
    rectangle(
        &mut art,
        5.,
        5.,
        paper_width - 10.,
        paper_height - 10.,
        &sheet.style.visible,
    );
    let width = 180_f64.min(paper_width - 10.);
    let x = paper_width - width - 5.;
    let y = paper_height - 49.;
    rectangle(&mut art, x, y, width, 44., &sheet.style.dimension);
    for [x1, y1, x2, y2] in [
        [0., 14., 1., 14.],
        [0., 22., 1., 22.],
        [0., 28., 1., 28.],
        [0., 38., 1., 38.],
        [0.64, 0., 0.64, 14.],
        [0.7, 22., 0.7, 28.],
        [0.45, 28., 0.45, 38.],
        [1. / 3., 38., 1. / 3., 44.],
        [2. / 3., 38., 2. / 3., 44.],
    ] {
        stroke(
            &mut art,
            [x + x1 * width, y + y1],
            [x + x2 * width, y + y2],
            &sheet.style.dimension,
        );
    }
    let title = &sheet.title_block;
    let small = sheet.style.small_text_height_mm;
    let mut add = |text: String, left: f64, top: f64, w: f64, h: f64, size: f64| {
        cell(
            &mut art,
            text,
            [x + left * width, y + top, w * width, h],
            size,
        );
    };
    add(
        if title.title.is_empty() {
            sheet.name.to_string()
        } else {
            title.title.clone()
        },
        0.,
        0.,
        0.64,
        9.,
        sheet.style.text_height_mm.min(3.5),
    );
    add(
        format!("DRAWING: {}", or_dash(&title.drawing_number)),
        0.,
        9.,
        0.64,
        5.,
        small,
    );
    add(format!("SHEET: {}", sheet.name), 0.64, 0., 0.36, 9., small);
    add(
        format!(
            "{} · {}",
            format_label(sheet.format),
            if sheet.projection_method == DrawingProjectionMethod::FirstAngle {
                "1ST ANGLE"
            } else {
                "3RD ANGLE"
            }
        ),
        0.64,
        9.,
        0.36,
        5.,
        small,
    );
    add(
        tolerance(&sheet.tolerance_note).into(),
        0.,
        14.,
        1.,
        8.,
        small,
    );
    add(
        format!("COMPANY: {}", or_dash(&title.company)),
        0.,
        22.,
        0.7,
        6.,
        small,
    );
    add(
        format!("REV {}", or_dash(&title.revision)),
        0.7,
        22.,
        0.3,
        6.,
        small,
    );
    add(
        format!("MATERIAL: {}", or_dash(&title.material)),
        0.,
        28.,
        0.45,
        10.,
        small,
    );
    add(
        format!("FINISH: {}", or_dash(&title.finish)),
        0.45,
        28.,
        0.55,
        10.,
        small,
    );
    add(
        format!("DRAWN: {}", or_dash(&title.author)),
        0.,
        38.,
        1. / 3.,
        6.,
        small,
    );
    add(
        format!("CHECKED: {}", or_dash(&title.checked_by)),
        1. / 3.,
        38.,
        1. / 3.,
        6.,
        small,
    );
    add(
        format!("APPROVED: {}", or_dash(&title.approved_by)),
        2. / 3.,
        38.,
        1. / 3.,
        6.,
        small,
    );

    if let Some([x, y]) = sheet.revision_table_position {
        grid(
            &mut art,
            [x, y],
            112.,
            sheet.revisions.len() + 1,
            &[12., 28., 82.],
            &sheet.style.dimension,
        );
        for (dx, text) in [(2., "REV"), (14., "DATE"), (30., "DESCRIPTION / APPROVAL")] {
            label(&mut art, x + dx, y + 4.2, text.into(), small, false);
        }
        for (i, revision) in sheet.revisions.iter().enumerate() {
            if !art.ready() {
                break;
            }
            let baseline = y + (i + 1) as f64 * 6. + 4.2;
            let description = if !revision.description.is_empty() {
                &revision.description
            } else {
                or_dash(&revision.change_order)
            };
            let description = if revision.approved_by.is_empty() {
                description.into()
            } else {
                format!("{description} · {}", revision.approved_by)
            };
            for (dx, text) in [
                (2., revision.revision.clone()),
                (14., revision.date.clone()),
                (30., description),
            ] {
                label(&mut art, x + dx, baseline, text, small, false);
            }
        }
    }
    if let Some([x, y]) = sheet.bom_table_position {
        grid(
            &mut art,
            [x, y],
            132.,
            sheet.bom.len() + 1,
            &[12., 40., 100., 112.],
            &sheet.style.dimension,
        );
        for (dx, text) in [
            (2., "ITEM"),
            (14., "PART"),
            (42., "DESCRIPTION"),
            (102., "QTY"),
            (114., "MATERIAL"),
        ] {
            label(&mut art, x + dx, y + 4.2, text.into(), small, false);
        }
        for (i, item) in sheet.bom.iter().enumerate() {
            if !art.ready() {
                break;
            }
            let quantity = format!("{:.3}", item.quantity)
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_owned();
            for (dx, text) in [
                (2., item.item_number.clone()),
                (14., or_dash(&item.part_number).into()),
                (42., item.description.clone()),
                (102., quantity),
                (114., or_dash(&item.material).into()),
            ] {
                label(
                    &mut art,
                    x + dx,
                    y + (i + 1) as f64 * 6. + 4.2,
                    text,
                    small,
                    false,
                );
            }
        }
    }
    art.finish()
}

#[cfg(test)]
fn render(sheet: &DrawingSheetDto, width: f64, height: f64) -> Art {
    try_render(&Source::new(sheet, [width, height])).expect("frame within presentation limits")
}

#[cfg(test)]
#[path = "drawing_frame/tests.rs"]
mod tests;
