//! Reading scanned 2D prints of flat plates for agents.
//!
//! Everything works in plate millimetres with the origin at the lower-left
//! corner of the plan view and y up. The plate outline is located on the sheet
//! by its aspect ratio and line weight (visible outlines are drawn about twice
//! as heavy as dimension lines and table rules), and its four corners are
//! refined separately so a slightly rotated scan maps correctly.
//!
//! Renders go through `pdftoppm` (poppler) for PDF prints and are cached on
//! disk; PNG and PGM prints are read directly. None of this is a modelling
//! path: it only tells an agent what is drawn where, so the agent can read
//! positions off a grid, and it checks a model's holes against the drawing.
use std::cell::RefCell;

thread_local! {
    static HINT: RefCell<Option<(f64, f64, f64, f64)>> = const { RefCell::new(None) };
}

/// Restrict the outline search to a window of the sheet, as fractions of the
/// page (x0, y0, x1, y1 from the top-left corner), for sheets where the
/// automatic search picks another rectangle.
pub fn set_hint(hint: Option<(f64, f64, f64, f64)>) {
    HINT.with(|h| *h.borrow_mut() = hint);
}

/// Parse "x0,y0,x1,y1" into an ordered millimetre window.
pub fn parse_region(text: &str) -> Result<(f64, f64, f64, f64), String> {
    let v: Vec<f64> = text
        .split(',')
        .map(|t| {
            t.trim()
                .parse::<f64>()
                .map_err(|_| format!("region needs four numbers, got {text}"))
        })
        .collect::<Result<_, _>>()?;
    if v.len() != 4 {
        return Err(format!("region needs four numbers x0,y0,x1,y1, got {text}"));
    }
    Ok((
        v[0].min(v[2]),
        v[1].min(v[3]),
        v[0].max(v[2]),
        v[1].max(v[3]),
    ))
}

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct Gray {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Gray {
    #[inline]
    fn at(&self, x: i64, y: i64) -> u8 {
        if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
            255
        } else {
            self.px[y as usize * self.w + x as usize]
        }
    }
}

fn parse_pgm(data: &[u8]) -> Gray {
    let mut idx = 0usize;
    let mut fields: Vec<String> = Vec::new();
    while fields.len() < 4 {
        while idx < data.len() && data[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if data[idx] == b'#' {
            while data[idx] != b'\n' {
                idx += 1;
            }
            continue;
        }
        let start = idx;
        while idx < data.len() && !data[idx].is_ascii_whitespace() {
            idx += 1;
        }
        fields.push(String::from_utf8_lossy(&data[start..idx]).to_string());
    }
    idx += 1;
    assert_eq!(fields[0], "P5", "expected a P5 pgm");
    let w: usize = fields[1].parse().unwrap();
    let h: usize = fields[2].parse().unwrap();
    Gray {
        w,
        h,
        px: data[idx..idx + w * h].to_vec(),
    }
}

fn cache_dir() -> PathBuf {
    let dir = std::env::var("LIMO_CAD_PRINT_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("limo-cad-print-cache"));
    fs::create_dir_all(&dir).ok();
    dir
}

/// Where the print comes from: a PDF page rasterised through `pdftoppm`
/// (poppler) at any resolution, a PNG or PGM image used at its own
/// resolution, or an in-memory image (tests).
#[derive(Clone, Debug)]
pub enum Source {
    Pdf { path: PathBuf, page: u32 },
    Image { path: PathBuf },
    Memory { image: Gray, dpi: u32 },
}

impl Source {
    /// Open a print by file name: `.pdf` renders through pdftoppm, `.png` and
    /// `.pgm` are read as they are.
    pub fn open(path: &Path, page: u32) -> Result<Source, String> {
        if !path.is_file() {
            return Err(format!("print {} is not a file", path.display()));
        }
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        match extension.as_str() {
            "pdf" => Ok(Source::Pdf {
                path: path.to_path_buf(),
                page,
            }),
            "png" | "pgm" => Ok(Source::Image {
                path: path.to_path_buf(),
            }),
            other => Err(format!(
                "unsupported print format .{other}; use PDF, PNG or PGM"
            )),
        }
    }

    /// The resolution a whole-page render has. A PDF renders at any dpi; an
    /// image has one native resolution and every dpi maps to it.
    fn native_dpi(&self) -> Option<u32> {
        match self {
            Source::Pdf { .. } => None,
            Source::Image { .. } => Some(CAL_DPI),
            Source::Memory { dpi, .. } => Some(*dpi),
        }
    }

    /// Effective dpi for a request: what the render will actually be.
    pub fn effective_dpi(&self, wanted: u32) -> u32 {
        self.native_dpi().unwrap_or(wanted)
    }

    /// Render the page (or a pixel crop of it) in grayscale at `dpi`.
    pub fn render(&self, dpi: u32, crop: Option<(i64, i64, i64, i64)>) -> Result<Gray, String> {
        match self {
            Source::Pdf { path, page } => render_pdf(path, dpi, *page, crop),
            Source::Image { path } => {
                let full = load_image(path)?;
                Ok(crop_gray(&full, crop))
            }
            Source::Memory { image, .. } => Ok(crop_gray(image, crop)),
        }
    }
}

fn crop_gray(full: &Gray, crop: Option<(i64, i64, i64, i64)>) -> Gray {
    let Some((x, y, w, h)) = crop else {
        return full.clone();
    };
    let (w, h) = (w.max(1) as usize, h.max(1) as usize);
    let mut px = vec![255u8; w * h];
    for row in 0..h {
        for col in 0..w {
            px[row * w + col] = full.at(x + col as i64, y + row as i64);
        }
    }
    Gray { w, h, px }
}

fn load_image(path: &Path) -> Result<Gray, String> {
    let data = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    if data.starts_with(b"P5") {
        return Ok(parse_pgm(&data));
    }
    let decoder = png::Decoder::new(std::io::Cursor::new(data));
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("decode {}: {e}", path.display()))?;
    let buffer_size = reader.output_buffer_size().ok_or_else(|| {
        format!(
            "decode {}: PNG dimensions exceed addressable memory",
            path.display()
        )
    })?;
    let mut buffer = vec![0; buffer_size];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|e| format!("decode {}: {e}", path.display()))?;
    let (w, h) = (info.width as usize, info.height as usize);
    let channels = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => {
            return Err("indexed PNG is not supported; export as RGB or grayscale".into())
        }
    };
    let bytes_per_sample = if info.bit_depth == png::BitDepth::Sixteen {
        2
    } else {
        1
    };
    let stride = channels * bytes_per_sample;
    let mut px = Vec::with_capacity(w * h);
    for i in 0..w * h {
        let o = i * stride;
        let sample = |c: usize| buffer[o + c * bytes_per_sample];
        let value = if channels >= 3 {
            ((u32::from(sample(0)) * 299 + u32::from(sample(1)) * 587 + u32::from(sample(2)) * 114)
                / 1000) as u8
        } else {
            sample(0)
        };
        px.push(value);
    }
    Ok(Gray { w, h, px })
}

/// Render one PDF page (or a pixel crop of it) to grayscale through pdftoppm, cached on disk.
fn render_pdf(
    pdf: &Path,
    dpi: u32,
    page: u32,
    crop: Option<(i64, i64, i64, i64)>,
) -> Result<Gray, String> {
    let meta = fs::metadata(pdf).map_err(|e| format!("read {}: {e}", pdf.display()))?;
    let stamp = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let key = format!(
        "{}-{}-{}-{}-{}-{:?}",
        pdf.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        meta.len(),
        stamp,
        dpi,
        page,
        crop
    )
    .replace(['(', ')', ' ', ',', '/'], "_");
    let path = cache_dir().join(format!("{key}.pgm"));
    if let Ok(data) = fs::read(&path) {
        if data.len() > 16 {
            return Ok(parse_pgm(&data));
        }
    }
    let prefix = cache_dir().join(format!("{key}.tmp"));
    let mut cmd = Command::new("pdftoppm");
    cmd.args([
        "-gray",
        "-r",
        &dpi.to_string(),
        "-f",
        &page.to_string(),
        "-l",
        &page.to_string(),
    ]);
    if let Some((x, y, w, h)) = crop {
        cmd.args([
            "-x",
            &x.to_string(),
            "-y",
            &y.to_string(),
            "-W",
            &w.to_string(),
            "-H",
            &h.to_string(),
        ]);
    }
    let status = cmd
        .args(["-singlefile"])
        .arg(pdf)
        .arg(&prefix)
        .status()
        .map_err(|e| format!("pdftoppm (poppler) is needed to read PDF prints: {e}"))?;
    if !status.success() {
        return Err(format!("pdftoppm failed on {} page {page}", pdf.display()));
    }
    let produced = cache_dir().join(format!("{key}.tmp.pgm"));
    fs::rename(&produced, &path).map_err(|e| format!("cache render: {e}"))?;
    Ok(parse_pgm(&fs::read(&path).map_err(|e| e.to_string())?))
}

const CAL_DPI: u32 = 300;

#[derive(Clone, Debug)]
pub struct Cal {
    dpi: u32,
    tl: (f64, f64),
    tr: (f64, f64),
    bl: (f64, f64),
    br: (f64, f64),
    length: f64,
    width: f64,
}

impl Cal {
    fn px_per_mm(&self) -> f64 {
        (self.br.0 - self.bl.0) / self.length
    }
    fn skew_deg(&self) -> f64 {
        (self.br.1 - self.bl.1)
            .atan2(self.br.0 - self.bl.0)
            .to_degrees()
    }
    /// plate mm -> pixels at `dpi`, minus an optional crop offset
    fn to_px(&self, dpi: u32, off: (f64, f64)) -> impl Fn(f64, f64) -> (f64, f64) + '_ {
        let k = dpi as f64 / self.dpi as f64;
        let (tl, tr, bl, br) = (
            sc(self.tl, k),
            sc(self.tr, k),
            sc(self.bl, k),
            sc(self.br, k),
        );
        let (l, w) = (self.length, self.width);
        move |x: f64, y: f64| {
            let (u, v) = (x / l, y / w);
            let px =
                bl.0 + u * (br.0 - bl.0) + v * (tl.0 - bl.0) + u * v * (tr.0 - tl.0 - br.0 + bl.0);
            let py =
                bl.1 + u * (br.1 - bl.1) + v * (tl.1 - bl.1) + u * v * (tr.1 - tl.1 - br.1 + bl.1);
            (px - off.0, py - off.1)
        }
    }
    /// pixels at `dpi` (plus crop offset) -> plate mm, affine from bl, br, tl
    fn to_mm(&self, dpi: u32, off: (f64, f64)) -> impl Fn(f64, f64) -> (f64, f64) + '_ {
        let k = dpi as f64 / self.dpi as f64;
        let (tl, bl, br) = (sc(self.tl, k), sc(self.bl, k), sc(self.br, k));
        let (ax, ay) = ((br.0 - bl.0) / self.length, (br.1 - bl.1) / self.length);
        let (bx, by) = ((tl.0 - bl.0) / self.width, (tl.1 - bl.1) / self.width);
        let det = ax * by - ay * bx;
        move |px: f64, py: f64| {
            let (dx, dy) = (px + off.0 - bl.0, py + off.1 - bl.1);
            ((dx * by - dy * bx) / det, (ax * dy - ay * dx) / det)
        }
    }
    pub fn json(&self) -> String {
        format!(
            "{{\"dpi\":{},\"tl\":[{},{}],\"tr\":[{},{}],\"bl\":[{},{}],\"br\":[{},{}],\"px_per_mm\":{:.4},\"skew_deg\":{:.3}}}",
            self.dpi, self.tl.0, self.tl.1, self.tr.0, self.tr.1, self.bl.0, self.bl.1, self.br.0, self.br.1, self.px_per_mm(), self.skew_deg()
        )
    }
}

fn sc(p: (f64, f64), k: f64) -> (f64, f64) {
    (p.0 * k, p.1 * k)
}

fn dark_counts(
    g: &Gray,
    x0: usize,
    x1: usize,
    y0: usize,
    y1: usize,
    thr: u8,
) -> (Vec<usize>, Vec<usize>) {
    let mut rows = vec![0usize; g.h];
    let mut cols = vec![0usize; g.w];
    for (offset, row_count) in rows[y0..y1].iter_mut().enumerate() {
        let y = y0 + offset;
        let row = &g.px[y * g.w..(y + 1) * g.w];
        for x in x0..x1 {
            if row[x] < thr {
                *row_count += 1;
                cols[x] += 1;
            }
        }
    }
    (rows, cols)
}

fn peaks(counts: &[usize], lo: usize, hi: usize, n: usize, gap: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (lo..hi).collect();
    order.sort_by(|a, b| counts[*b].cmp(&counts[*a]));
    let mut chosen: Vec<usize> = Vec::new();
    for i in order {
        if chosen
            .iter()
            .all(|c| (*c as i64 - i as i64).abs() > gap as i64)
        {
            chosen.push(i);
        }
        if chosen.len() == n {
            break;
        }
    }
    chosen.sort();
    chosen
}

/// Fraction of the pixels along a horizontal (or vertical) segment that are dark within +-2 px.
/// Support of a side measured with a small band, so only a candidate row/column that really
/// sits on the line scores; for long sides the band grows a little with skew.
fn tight_support(g: &Gray, fixed: usize, lo: usize, hi: usize, horizontal: bool) -> f64 {
    side_support_band(
        g,
        fixed,
        lo.min(hi),
        lo.max(hi),
        horizontal,
        3 + ((hi.max(lo) - lo.min(hi)) as f64 * 0.003) as i64,
    )
}

/// Same, with an explicit +-band in px (a scan can be rotated by half a degree, so a short
/// segment of a long side needs the long side's band).
fn side_support_band(
    g: &Gray,
    fixed: usize,
    lo: usize,
    hi: usize,
    horizontal: bool,
    band: i64,
) -> f64 {
    if hi <= lo {
        return 0.0;
    }
    let mut dark = 0;
    for t in lo..=hi {
        let mut hit = false;
        for d in -band..=band {
            let (x, y) = if horizontal {
                (t as i64, fixed as i64 + d)
            } else {
                (fixed as i64 + d, t as i64)
            };
            if g.at(x, y) < 110 {
                hit = true;
                break;
            }
        }
        if hit {
            dark += 1;
        }
    }
    dark as f64 / (hi - lo + 1) as f64
}

/// Median stroke width (px) of a horizontal or vertical line at `fixed`, sampled along lo..hi
/// every 8 px: the perpendicular dark run through the nearest dark pixel within +-band.
fn stroke_width(g: &Gray, fixed: usize, lo: usize, hi: usize, horizontal: bool, band: i64) -> f64 {
    let mut widths: Vec<i64> = Vec::new();
    let mut t = lo;
    while t <= hi {
        let mut found: Option<i64> = None;
        for d in -band..=band {
            let (x, y) = if horizontal {
                (t as i64, fixed as i64 + d)
            } else {
                (fixed as i64 + d, t as i64)
            };
            if g.at(x, y) < 110 {
                found = Some(fixed as i64 + d);
                break;
            }
        }
        if let Some(c) = found {
            let (mut a, mut b) = (c, c);
            let px = |p: i64| {
                if horizontal {
                    g.at(t as i64, p)
                } else {
                    g.at(p, t as i64)
                }
            };
            while px(a - 1) < 110 && c - a < 40 {
                a -= 1;
            }
            while px(b + 1) < 110 && b - c < 40 {
                b += 1;
            }
            widths.push(b - a + 1);
        }
        t += 8;
    }
    if widths.is_empty() {
        return 0.0;
    }
    widths.sort();
    widths[widths.len() / 2] as f64
}

type CalibrationCandidate = (f64, f64, usize, usize, usize, usize, f64, f64);

/// Find the plate outline on the sheet.
/// Candidate lines are the strongest vertical and horizontal ink lines away from the page
/// border (the drawing frame). A candidate rectangle pairs two of each with the span ratio
/// width/length within 3 %; its vertical sides must be drawn over their length and its
/// horizontal sides at least at the ends (a notched outline still has those), and its strokes
/// must have visible-line weight (about 4 px at 300 dpi; dimension lines and table rules are
/// about 2.5 px). Rectangles in the title-block corner (bottom right) are skipped. Among the
/// survivors the strongest lines win, which is the main view.
pub fn calibrate(source: &Source, length: f64, width: f64) -> Result<Cal, String> {
    let cal_dpi = source.effective_dpi(CAL_DPI);
    let g = source.render(cal_dpi, None)?;
    let (rows, cols) = dark_counts(&g, 0, g.w, 0, g.h, 110);
    let (mx, my) = ((g.w as f64 * 0.06) as usize, (g.h as f64 * 0.06) as usize);
    let xc = peaks(&cols, mx, g.w - mx, 48, 6);
    let yc = peaks(&rows, my, g.h - my, 48, 6);
    let target = width / length;
    let debug = false;
    let expect: Option<Vec<usize>> = None;
    let hint: Option<(f64, f64, f64, f64)> = HINT.with(|h| *h.borrow());

    let mut cands: Vec<CalibrationCandidate> = Vec::new();
    for i in 0..xc.len() {
        for j in i + 1..xc.len() {
            let (x0, x1) = (xc[i], xc[j]);
            let span_x = (x1 - x0) as f64;
            if span_x < g.w as f64 * 0.06 {
                continue;
            }
            let want = span_x * target;
            for k in 0..yc.len() {
                let y0 = yc[k];
                for &y1 in &yc[k + 1..] {
                    let span_y = (y1 - y0) as f64;
                    if span_y < want * 0.97 {
                        continue;
                    }
                    if span_y > want * 1.03 {
                        break;
                    }
                    let dbg = expect.as_ref().is_some_and(|v| {
                        (v[0] as i64 - x0 as i64).abs() <= 6
                            && (v[1] as i64 - x1 as i64).abs() <= 6
                            && (v[2] as i64 - y0 as i64).abs() <= 6
                            && (v[3] as i64 - y1 as i64).abs() <= 6
                    });
                    if let Some((hx0, hy0, hx1, hy1)) = hint {
                        let (fx0, fx1, fy0, fy1) = (
                            x0 as f64 / g.w as f64,
                            x1 as f64 / g.w as f64,
                            y0 as f64 / g.h as f64,
                            y1 as f64 / g.h as f64,
                        );
                        if fx0 < hx0 - 0.02
                            || fx1 > hx1 + 0.02
                            || fy0 < hy0 - 0.02
                            || fy1 > hy1 + 0.02
                        {
                            if dbg {
                                eprintln!("  expected: outside hint");
                            }
                            continue;
                        }
                    } else {
                        let (fy1, fh) = (y1 as f64 / g.h as f64, (y1 - y0) as f64 / g.h as f64);
                        if fy1 > 0.80 && fh < 0.15 {
                            if dbg {
                                eprintln!("  expected: in the title-block band");
                            }
                            continue;
                        }
                    }
                    let end = ((span_x * 0.06) as usize).max(8);

                    let skew_band = 3 + (span_x * 0.005) as i64;
                    let left = tight_support(&g, x0, y0, y1, false);
                    let right = tight_support(&g, x1, y0, y1, false);
                    let ends = [
                        side_support_band(&g, y0, x0, x0 + end, true, skew_band),
                        side_support_band(&g, y0, x1 - end, x1, true, skew_band),
                        side_support_band(&g, y1, x0, x0 + end, true, skew_band),
                        side_support_band(&g, y1, x1 - end, x1, true, skew_band),
                    ];

                    let mid = (y0 + y1) / 2;
                    let third = ((y1 - y0) / 3).max(4);
                    let widths = [
                        stroke_width(&g, x0, mid - third, mid + third, false, 4),
                        stroke_width(&g, x1, mid - third, mid + third, false, 4),
                    ];
                    let mut sorted = [widths[0], widths[1], widths[0], widths[1]];
                    sorted.sort_by(|p, q| p.partial_cmp(q).unwrap());
                    let median = (widths[0] + widths[1]) / 2.0;
                    let strength = (cols[x0] + cols[x1] + rows[y0] + rows[y1]) as f64;
                    if dbg {
                        eprintln!("  expected: left {left:.2} right {right:.2} ends {:?} widths {widths:?} median {median} strength {strength}", ends.iter().map(|v| (v * 100.0).round() / 100.0).collect::<Vec<_>>());
                    }
                    if left < 0.6 || right < 0.6 || ends.iter().any(|s| *s < 0.5) {
                        continue;
                    }
                    if median < 2.0 || median > 8.0 {
                        continue;
                    }
                    cands.push((strength, span_x, x0, x1, y0, y1, sorted[0], median));
                }
            }
        }
    }
    if debug {
        let mut show = cands.clone();
        show.sort_by(|p, q| q.0.partial_cmp(&p.0).unwrap());
        eprintln!(
            "{} candidates; by strength (strength, span, x0, x1, y0, y1, thinnest, median width):",
            cands.len()
        );
        for c in show.iter().take(10) {
            eprintln!(
                "  {:.0} {:.0} {} {} {} {} {:.1} {:.1}",
                c.0, c.1, c.2, c.3, c.4, c.5, c.6, c.7
            );
        }
    }

    let max_width = cands.iter().map(|c| c.7).fold(0.0, f64::max);
    let best = cands
        .iter()
        .cloned()
        .filter(|c| c.7 >= 0.9 * max_width && c.6 >= 0.75 * max_width)
        .fold(None, |acc: Option<CalibrationCandidate>, c| match acc {
            None => Some(c),
            Some(b) => {
                if c.0 > b.0 {
                    Some(c)
                } else {
                    Some(b)
                }
            }
        });
    let Some((_, span_x, x0, x1, yt, yb, _, _)) = best else {
        return Err("plate outline not found on the page: check length_mm and width_mm, or pass a hint window around the plan view".into());
    };
    let scale = span_x / length;
    let band = ((length.min(width) * 0.25 * scale) as usize).max(20);
    let inset = (band / 8).max(4);
    let refine_row = |yg: usize, xlo: usize, xhi: usize| -> usize {
        let mut best = (0.0, yg);
        for y in yg.saturating_sub(12)..(yg + 13).min(g.h) {
            let s = side_support_band(&g, y, xlo, xhi.min(g.w), true, 1);
            let wdt = stroke_width(&g, y, xlo, xhi.min(g.w), true, 1);
            let score = s * wdt - 0.01 * (y as f64 - yg as f64).abs();
            if score > best.0 {
                best = (score, y);
            }
        }
        best.1
    };
    let refine_col = |xg: usize, ylo: usize, yhi: usize| -> usize {
        let mut best = (0.0, xg);
        for x in xg.saturating_sub(12)..(xg + 13).min(g.w) {
            let s = side_support_band(&g, x, ylo, yhi.min(g.h), false, 1);
            let wdt = stroke_width(&g, x, ylo, yhi.min(g.h), false, 1);
            let score = s * wdt - 0.01 * (x as f64 - xg as f64).abs();
            if score > best.0 {
                best = (score, x);
            }
        }
        best.1
    };
    let tl = (
        refine_col(x0, yt + inset, yt + inset + band),
        refine_row(yt, x0 + inset, x0 + inset + band),
    );
    let tr = (
        refine_col(x1, yt + inset, yt + inset + band),
        refine_row(yt, x1 - inset - band, x1 - inset),
    );
    let bl = (
        refine_col(x0, yb - inset - band, yb - inset),
        refine_row(yb, x0 + inset, x0 + inset + band),
    );
    let br = (
        refine_col(x1, yb - inset - band, yb - inset),
        refine_row(yb, x1 - inset - band, x1 - inset),
    );
    Ok(Cal {
        dpi: cal_dpi,
        tl: (tl.0 as f64, tl.1 as f64),
        tr: (tr.0 as f64, tr.1 as f64),
        bl: (bl.0 as f64, bl.1 as f64),
        br: (br.0 as f64, br.1 as f64),
        length,
        width,
    })
}

/// A model hole in plate millimetres (plan-view frame, lower-left origin).
#[derive(Clone, Debug)]
pub struct Hole {
    pub x: f64,
    pub y: f64,
    pub d: f64,
    pub cb: Option<f64>,
}

/// Ring completeness at radius r around (cx, cy): the fraction of the 52 directions that are
/// more than 12 degrees away from the four axes where ink (< 150) lies within +-tol px of r.
/// Leaving the axes out means the crosshair, the centrelines and any line through the centre
/// do not count, so a bare line crossing scores ~0 and a circle scores ~1.
fn ring_completeness_tol(g: &Gray, cx: f64, cy: f64, r: f64, tol: i64) -> f64 {
    let mut hit = 0;
    let mut total = 0;
    for k in 0..72 {
        let deg = k * 5;
        let off_axis = (deg % 90 > 12) && (deg % 90 < 78);
        if !off_axis {
            continue;
        }
        total += 1;
        let a = (deg as f64).to_radians();
        let (ca, sa) = (a.cos(), a.sin());
        for d in -tol..=tol {
            let rr = r + d as f64;
            if g.at((cx + rr * ca).round() as i64, (cy + rr * sa).round() as i64) < 110 {
                hit += 1;
                break;
            }
        }
    }
    hit as f64 / total as f64
}

/// Interior lightness: 8 directions at 30 and 60 degrees in each quadrant (clear of the
/// crosshair and of an X), sampled in the annulus 0.3 r .. 0.55 r (clear of the ring's inner
/// blur); the fraction whose samples are all paper (> 125). A hole symbol scores >= 0.5, a
/// solid arrowhead, a filled dot or a text glyph scores low.
fn interior_light(g: &Gray, cx: f64, cy: f64, r: f64) -> f64 {
    let mut light = 0;
    for k in 0..8 {
        let deg = 30.0 + 30.0 * (k % 2) as f64 + 90.0 * (k / 2) as f64;
        let a = deg.to_radians();
        let (ca, sa) = (a.cos(), a.sin());
        let mut ok = true;
        for i in 0..4 {
            let rr = r * (0.30 + 0.25 * i as f64 / 3.0);
            if g.at((cx + rr * ca).round() as i64, (cy + rr * sa).round() as i64) <= 125 {
                ok = false;
                break;
            }
        }
        if ok {
            light += 1;
        }
    }
    light as f64 / 8.0
}

/// Fraction of the off-axis directions in which the ink run through radius r is thicker than
/// `max_mm`: a drawn circle is a thin uniform line (~0.15), a solid arrowhead wedge or a
/// line crossed at a shallow angle gives long radial runs.
fn thick_fraction(g: &Gray, cx: f64, cy: f64, r: f64, ppm: f64, max_mm: f64) -> f64 {
    let mut thick = 0;
    let mut total = 0;
    let limit = (max_mm * ppm) as i64;
    for k in 0..72 {
        let deg = k * 5;
        if !((deg % 90 > 12) && (deg % 90 < 78)) {
            continue;
        }
        total += 1;
        let a = (deg as f64).to_radians();
        let (ca, sa) = (a.cos(), a.sin());

        let mut found = None;
        for d in -2..=2 {
            let rr = r + d as f64;
            if g.at((cx + rr * ca).round() as i64, (cy + rr * sa).round() as i64) < 110 {
                found = Some(rr);
                break;
            }
        }
        if let Some(rr) = found {
            let mut inner = rr;
            while inner > 0.0
                && g.at(
                    (cx + (inner - 1.0) * ca).round() as i64,
                    (cy + (inner - 1.0) * sa).round() as i64,
                ) < 110
            {
                inner -= 1.0;
            }
            let mut outer = rr;
            while outer - rr < 40.0
                && g.at(
                    (cx + (outer + 1.0) * ca).round() as i64,
                    (cy + (outer + 1.0) * sa).round() as i64,
                ) < 110
            {
                outer += 1.0;
            }
            if (outer - inner) as i64 > limit {
                thick += 1;
            }
        }
    }
    if total == 0 {
        0.0
    } else {
        thick as f64 / total as f64
    }
}

/// Distance (px) between (cx, cy) and the centroid of the dark pixels within radius `r`.
fn dark_centroid_offset(g: &Gray, cx: f64, cy: f64, r: f64) -> f64 {
    let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
    let ri = r.ceil() as i64;
    for dy in -ri..=ri {
        for dx in -ri..=ri {
            if (dx * dx + dy * dy) as f64 <= r * r
                && g.at(cx.round() as i64 + dx, cy.round() as i64 + dy) < 110
            {
                sx += dx as f64;
                sy += dy as f64;
                n += 1.0;
            }
        }
    }
    if n == 0.0 {
        f64::MAX
    } else {
        ((sx / n).powi(2) + (sy / n).powi(2)).sqrt()
    }
}

/// Fraction of 24 directions (every 15 degrees) that are dark at radius r: for solid discs.
fn disc_dark(g: &Gray, cx: f64, cy: f64, r: f64) -> f64 {
    let mut dark = 0;
    for k in 0..24 {
        let a = (k as f64 + 0.5) * std::f64::consts::TAU / 24.0;
        if g.at(
            (cx + r * a.cos()).round() as i64,
            (cy + r * a.sin()).round() as i64,
        ) < 110
        {
            dark += 1;
        }
    }
    dark as f64 / 24.0
}

#[derive(Clone)]
struct Probe {
    kind: &'static str,
    ring: f64,
    interior: f64,
    r_px: f64,
    cb_px: Option<f64>,
    cx: f64,
    cy: f64,
}

fn none_probe(cx: f64, cy: f64) -> Probe {
    Probe {
        kind: "none",
        ring: 0.0,
        interior: 0.0,
        r_px: 0.0,
        cb_px: None,
        cx,
        cy,
    }
}

fn rank(p: &Probe) -> f64 {
    match p.kind {
        "symbol" => 3.0 + p.ring,
        "dot" => 2.0 + p.ring,
        "dashed" => 1.0 + p.ring,
        _ => 0.0,
    }
}

/// What is drawn exactly at (cx, cy). A solid dot: a full ring at 0.4..1.2 mm with a dark
/// interior and light paper outside. Otherwise the smallest radius from 1.0 mm to r_max with
/// an off-axis ring >= 0.9 and a light interior = "symbol" (a second complete ring within
/// 3.5 r with a not-empty interior is its counterbore); an off-axis ring >= 0.5 with light
/// interior and light outside = "dashed" (hidden-line circle); otherwise "none".
fn probe_centre(g: &Gray, cx: f64, cy: f64, ppm: f64, r_max_mm: f64) -> Probe {
    let tol = (0.2 * ppm).round().max(1.0) as i64;
    let mut best_dashed: Option<Probe> = None;
    let mut r = 1.0 * ppm;
    let r_max = r_max_mm * ppm;
    while r <= r_max {
        let s = ring_completeness_tol(g, cx, cy, r, tol);
        if s >= 0.5 {
            let li = interior_light(g, cx, cy, r);
            if s >= 0.9
                && li >= 0.5
                && thick_fraction(g, cx, cy, r, ppm, (1.0f64).max(0.4 * r / ppm)) <= 0.25
            {
                let mut cb = None;
                let mut r2 = r * 1.3 + 2.0;
                let r2_max = (r * 3.5).min(40.0 * ppm);
                while r2 <= r2_max {
                    if ring_completeness_tol(g, cx, cy, r2, tol) >= 0.9 {
                        cb = Some(r2);
                        break;
                    }
                    r2 += 0.1 * ppm;
                }
                return Probe {
                    kind: "symbol",
                    ring: s,
                    interior: li,
                    r_px: r,
                    cb_px: cb,
                    cx,
                    cy,
                };
            }
            if li >= 0.5
                && interior_light(g, cx, cy, r * 1.9) >= 0.5
                && thick_fraction(g, cx, cy, r, ppm, (1.0f64).max(0.4 * r / ppm)) <= 0.25
                && best_dashed.as_ref().is_none_or(|p| s > p.ring)
            {
                best_dashed = Some(Probe {
                    kind: "dashed",
                    ring: s,
                    interior: li,
                    r_px: r,
                    cb_px: None,
                    cx,
                    cy,
                });
            }
        }
        r += 0.05 * ppm;
    }

    let mut r = 0.6 * ppm;
    while r <= 1.6 * ppm {
        if disc_dark(g, cx, cy, r) >= 0.9
            && disc_dark(g, cx, cy, r * 0.7) >= 0.9
            && disc_dark(g, cx, cy, r + 0.5 * ppm) <= 0.25
            && dark_centroid_offset(g, cx, cy, r + 0.5 * ppm) <= 0.2 * ppm
        {
            let mut edge = r;
            while edge <= 1.8 * ppm && disc_dark(g, cx, cy, edge + 0.05 * ppm) >= 0.9 {
                edge += 0.05 * ppm;
            }
            return Probe {
                kind: "dot",
                ring: 1.0,
                interior: 0.0,
                r_px: edge,
                cb_px: None,
                cx,
                cy,
            };
        }
        r += 0.1 * ppm;
    }
    best_dashed.unwrap_or(none_probe(cx, cy))
}

/// Probe at (cx, cy) and at the eight neighbours 1 px away; keep the best.
fn probe_refined(g: &Gray, cx: f64, cy: f64, ppm: f64, r_max_mm: f64) -> Probe {
    let mut best = probe_centre(g, cx, cy, ppm, r_max_mm);
    for dx in [-1.0, 0.0, 1.0] {
        for dy in [-1.0, 0.0, 1.0] {
            if dx == 0.0 && dy == 0.0 {
                continue;
            }
            let p = probe_centre(g, cx + dx, cy + dy, ppm, r_max_mm);
            if rank(&p) > rank(&best) {
                best = p;
            }
        }
    }
    best
}

/// Ink crossings: pixels with a horizontal and a vertical ink run of at least 1.2 mm through
/// them (the centre mark of every hole symbol, and every line crossing of the drawing),
/// clustered by 4-connectivity; returns cluster centroids and sizes.
fn find_crossings(g: &Gray, ppm: f64) -> Vec<(f64, f64, usize)> {
    let (w, h) = (g.w, g.h);
    let dark = |v: u8| v < 150;
    let run = (1.2 * ppm).round().max(4.0) as usize;
    let mut hrun = vec![0u16; w * h];
    for y in 0..h {
        let row = &g.px[y * w..(y + 1) * w];
        let mut x = 0;
        while x < w {
            if dark(row[x]) {
                let start = x;
                while x < w && dark(row[x]) {
                    x += 1;
                }
                let len = (x - start).min(65535) as u16;
                for i in start..x {
                    hrun[y * w + i] = len;
                }
            } else {
                x += 1;
            }
        }
    }
    let mut vrun = vec![0u16; w * h];
    for x in 0..w {
        let mut y = 0;
        while y < h {
            if dark(g.px[y * w + x]) {
                let start = y;
                while y < h && dark(g.px[y * w + x]) {
                    y += 1;
                }
                let len = (y - start).min(65535) as u16;
                for i in start..y {
                    vrun[i * w + x] = len;
                }
            } else {
                y += 1;
            }
        }
    }
    let mut mark = vec![false; w * h];
    for i in 0..w * h {
        mark[i] = hrun[i] as usize >= run && vrun[i] as usize >= run;
    }
    let mut seen = vec![false; w * h];
    let mut stack: Vec<usize> = Vec::new();
    let mut cands: Vec<(f64, f64, usize)> = Vec::new();
    for s in 0..w * h {
        if !mark[s] || seen[s] {
            continue;
        }
        seen[s] = true;
        stack.clear();
        stack.push(s);
        let (mut sx, mut sy, mut n) = (0usize, 0usize, 0usize);
        while let Some(p) = stack.pop() {
            let (x, y) = (p % w, p / w);
            sx += x;
            sy += y;
            n += 1;
            let mut push = |q: usize| {
                if mark[q] && !seen[q] {
                    seen[q] = true;
                    stack.push(q);
                }
            };
            if x > 0 {
                push(p - 1);
            }
            if x + 1 < w {
                push(p + 1);
            }
            if y > 0 {
                push(p - w);
            }
            if y + 1 < h {
                push(p + w);
            }
        }
        cands.push((sx as f64 / n as f64, sy as f64 / n as f64, n));
    }
    cands
}

/// Probe at the point, then at every ink crossing within `search_mm` (the crosshair of a
/// symbol nearby), then on a 0.25 mm grid; returns the best probe and where it was found.
fn probe_near(
    g: &Gray,
    crossings: &[(f64, f64, usize)],
    cx: f64,
    cy: f64,
    ppm: f64,
    r_max_mm: f64,
    search_mm: f64,
) -> Probe {
    let mut best = probe_refined(g, cx, cy, ppm, r_max_mm);
    if best.kind == "symbol" || best.kind == "dot" {
        return best;
    }
    let s_px = search_mm * ppm;
    for (x, y, _) in crossings {
        if (x - cx).abs() <= s_px && (y - cy).abs() <= s_px {
            let p = probe_refined(g, *x, *y, ppm, r_max_mm);
            if rank(&p) > rank(&best) {
                best = p;
            }
        }
    }
    if best.kind == "symbol" || best.kind == "dot" {
        return best;
    }
    let steps = (search_mm / 0.25).round() as i64;
    for dy in -steps..=steps {
        for dx in -steps..=steps {
            let p = probe_centre(
                g,
                cx + dx as f64 * 0.25 * ppm,
                cy + dy as f64 * 0.25 * ppm,
                ppm,
                r_max_mm,
            );
            if rank(&p) > rank(&best) {
                best = p;
            }
        }
    }
    best
}

pub fn ring_score(
    source: &Source,
    cal: &Cal,
    holes: &[Hole],
    dpi: u32,
    search_mm: f64,
) -> Result<String, String> {
    let dpi = source.effective_dpi(dpi);
    let g = source.render(dpi, None)?;
    let ppm = cal.px_per_mm() * dpi as f64 / cal.dpi as f64;
    let to_px = cal.to_px(dpi, (0.0, 0.0));
    let to_mm = cal.to_mm(dpi, (0.0, 0.0));
    let crossings = find_crossings(&g, ppm);
    let mut items = Vec::new();
    let mut off = 0;
    for h in holes {
        let (px, py) = to_px(h.x, h.y);
        let p = probe_near(
            &g,
            &crossings,
            px,
            py,
            ppm,
            if h.d.is_nan() {
                3.0
            } else {
                (h.d * 0.9).clamp(3.0, 32.0)
            },
            search_mm,
        );
        let on = p.kind != "none";
        if !on {
            off += 1;
        }
        let (fx, fy) = to_mm(p.cx, p.cy);
        let offset = ((fx - h.x).powi(2) + (fy - h.y).powi(2)).sqrt();
        items.push(format!(
            "{{\"x\":{},\"y\":{},\"diameter\":{},\"drawn\":\"{}\",\"drawn_at\":[{:.1},{:.1}],\"offset_mm\":{:.1},\"ring\":{:.2},\"interior_light\":{:.2},\"drawn_diameter_mm\":{:.1},\"drawn_counterbore_mm\":{}}}",
            h.x, h.y, h.d, p.kind, fx, fy, offset, p.ring, p.interior, 2.0 * p.r_px / ppm, p.cb_px.map_or("null".to_string(), |v| format!("{:.1}", 2.0 * v / ppm))
        ));
    }
    Ok(format!("{{\"ok\":true,\"dpi\":{},\"holes\":{},\"nothing_drawn_within_search\":{},\"search_mm\":{},\"legend\":\"drawn: symbol = circle with a light interior (drawn_at is its centre, offset_mm from the model hole), dot = solid dot, dashed = partial ring such as a hidden-line circle, none = nothing round within search_mm\",\"items\":[{}]}}", dpi, holes.len(), off, search_mm, items.join(",")))
}

/// Encode an RGB buffer as PNG bytes.
pub fn encode_png(w: usize, h: usize, rgb: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(rgb).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

struct Canvas {
    w: usize,
    h: usize,
    rgb: Vec<u8>,
}

impl Canvas {
    fn from_gray(g: &Gray) -> Canvas {
        let mut rgb = Vec::with_capacity(g.w * g.h * 3);
        for v in &g.px {
            rgb.extend_from_slice(&[*v, *v, *v]);
        }
        Canvas {
            w: g.w,
            h: g.h,
            rgb,
        }
    }
    fn put(&mut self, x: i64, y: i64, c: [u8; 3]) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            let o = (y as usize * self.w + x as usize) * 3;
            self.rgb[o..o + 3].copy_from_slice(&c);
        }
    }
    fn circle(&mut self, cx: f64, cy: f64, r: f64, c: [u8; 3], thick: usize) {
        let steps = ((std::f64::consts::TAU * r) as usize).max(24);
        for k in 0..steps {
            let a = k as f64 * std::f64::consts::TAU / steps as f64;
            for t in 0..thick {
                let rr = r + t as f64;
                self.put(
                    (cx + rr * a.cos()).round() as i64,
                    (cy + rr * a.sin()).round() as i64,
                    c,
                );
            }
        }
    }
    fn cross(&mut self, cx: f64, cy: f64, s: i64, c: [u8; 3]) {
        for d in -s..=s {
            self.put(cx.round() as i64 + d, cy.round() as i64, c);
            self.put(cx.round() as i64, cy.round() as i64 + d, c);
        }
    }
    fn line(&mut self, a: (f64, f64), b: (f64, f64), c: [u8; 3]) {
        let n = ((b.0 - a.0).abs().max((b.1 - a.1).abs()) as usize).max(1);
        for i in 0..=n {
            let t = i as f64 / n as f64;
            self.put(
                (a.0 + (b.0 - a.0) * t).round() as i64,
                (a.1 + (b.1 - a.1) * t).round() as i64,
                c,
            );
        }
    }
}

const RED: [u8; 3] = [230, 0, 0];
const BLUE: [u8; 3] = [0, 0, 230];
const GREEN: [u8; 3] = [0, 160, 0];
const MAGENTA: [u8; 3] = [200, 0, 200];
const ORANGE: [u8; 3] = [230, 120, 0];

/// Pixel bounds of a millimetre region at `dpi`.
fn region_px(cal: &Cal, dpi: u32, region: (f64, f64, f64, f64)) -> (i64, i64, i64, i64) {
    let m = cal.to_px(dpi, (0.0, 0.0));
    let pts = [
        m(region.0, region.1),
        m(region.2, region.1),
        m(region.0, region.3),
        m(region.2, region.3),
    ];
    let x0 = pts.iter().map(|p| p.0).fold(f64::MAX, f64::min).floor() as i64;
    let x1 = pts.iter().map(|p| p.0).fold(f64::MIN, f64::max).ceil() as i64;
    let y0 = pts.iter().map(|p| p.1).fold(f64::MAX, f64::min).floor() as i64;
    let y1 = pts.iter().map(|p| p.1).fold(f64::MIN, f64::max).ceil() as i64;
    (x0, y0, x1 - x0, y1 - y0)
}

fn draw_holes(
    cv: &mut Canvas,
    to_px: &dyn Fn(f64, f64) -> (f64, f64),
    holes: &[Hole],
    ppm: f64,
    color: [u8; 3],
) {
    let thick = (ppm as usize).max(2);
    for h in holes {
        let (px, py) = to_px(h.x, h.y);
        cv.circle(px, py, (h.d / 2.0 * ppm).max(3.0), color, thick);
        cv.cross(px, py, (6.0 * (ppm / 2.0).max(1.0)) as i64, color);
        if let Some(cb) = h.cb {
            cv.circle(px, py, cb / 2.0 * ppm, BLUE, thick);
        }
    }
}

fn draw_grid(
    cv: &mut Canvas,
    to_px: &dyn Fn(f64, f64) -> (f64, f64),
    region: (f64, f64, f64, f64),
    step: f64,
    ppm: f64,
) {
    let (x0, y0, x1, y1) = region;
    let mut x = (x0 / step).ceil() * step;
    while x <= x1 {
        let long = ((x / step).round() as i64) % 5 == 0;
        let len = if long { 4.0 * ppm } else { 2.0 * ppm };
        let (ax, ay) = to_px(x, y0);
        let (bx, by) = to_px(x, y1);
        if long {
            cv.line((ax, ay), (bx, by), [200, 220, 255]);
        }
        cv.line((ax, ay), (ax, ay - len), GREEN);
        cv.line((bx, by), (bx, by + len), GREEN);
        x += step;
    }
    let mut y = (y0 / step).ceil() * step;
    while y <= y1 {
        let long = ((y / step).round() as i64) % 5 == 0;
        let len = if long { 4.0 * ppm } else { 2.0 * ppm };
        let (ax, ay) = to_px(x0, y);
        let (bx, by) = to_px(x1, y);
        if long {
            cv.line((ax, ay), (bx, by), [200, 220, 255]);
        }
        cv.line((ax, ay), (ax + len, ay), GREEN);
        cv.line((bx, by), (bx - len, by), GREEN);
        y += step;
    }
}

/// A millimetre window of the print with tick marks and the model holes drawn on it.
pub struct CropImage {
    pub png: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub dpi: u32,
    pub px_per_mm: f64,
}

pub fn crop(
    source: &Source,
    cal: &Cal,
    region: (f64, f64, f64, f64),
    dpi: u32,
    holes: &[Hole],
    grid: f64,
) -> Result<CropImage, String> {
    let dpi = source.effective_dpi(dpi);
    let (cx, cy, cw, ch) = region_px(cal, dpi, region);
    let g = source.render(dpi, Some((cx, cy, cw, ch)))?;
    let mut cv = Canvas::from_gray(&g);
    let to_px = cal.to_px(dpi, (cx as f64, cy as f64));
    let ppm = cal.px_per_mm() * dpi as f64 / cal.dpi as f64;
    if grid > 0.0 {
        draw_grid(&mut cv, &to_px, region, grid, ppm);
    }
    draw_holes(&mut cv, &to_px, holes, ppm, RED);
    Ok(CropImage {
        png: encode_png(cv.w, cv.h, &cv.rgb)?,
        width: cv.w,
        height: cv.h,
        dpi,
        px_per_mm: ppm,
    })
}

#[derive(Clone, Debug)]
struct Symbol {
    x: f64,
    y: f64,
    d: f64,
    cb: Option<f64>,
    px: (f64, f64),
    kind: &'static str,
    ring_score: f64,
}

/// Every ink crossing whose surroundings are light off-axis is probed for a drawn circle.
fn detect_symbols(g: &Gray, ppm: f64, to_mm: &dyn Fn(f64, f64) -> (f64, f64)) -> Vec<Symbol> {
    let cands = find_crossings(g, ppm);
    let mut out: Vec<Symbol> = Vec::new();
    for (cx, cy, _n) in cands {
        let near_light = interior_light(g, cx, cy, 3.0 * ppm);
        if near_light < 0.4 && interior_light(g, cx, cy, 0.6 * ppm) > 0.3 {
            continue;
        }
        let p = probe_refined(g, cx, cy, ppm, 32.0);
        if p.kind != "symbol" && p.kind != "dot" && !(p.kind == "dashed" && p.ring >= 0.6) {
            continue;
        }
        let (mx, my) = to_mm(p.cx, p.cy);
        if let Some(o) = out
            .iter_mut()
            .find(|o| ((o.x - mx).powi(2) + (o.y - my).powi(2)).sqrt() < 1.0)
        {
            if rank_kind(p.kind, p.ring) > rank_kind(o.kind, o.ring_score) {
                o.x = mx;
                o.y = my;
                o.d = 2.0 * p.r_px / ppm;
                o.cb = p.cb_px.map(|v| 2.0 * v / ppm);
                o.px = (p.cx, p.cy);
                o.kind = p.kind;
                o.ring_score = p.ring;
            }
            continue;
        }
        out.push(Symbol {
            x: mx,
            y: my,
            d: 2.0 * p.r_px / ppm,
            cb: p.cb_px.map(|v| 2.0 * v / ppm),
            px: (p.cx, p.cy),
            kind: p.kind,
            ring_score: p.ring,
        });
    }
    out.sort_by(|a, b| (a.y, a.x).partial_cmp(&(b.y, b.x)).unwrap());
    out
}

fn rank_kind(kind: &str, ring: f64) -> f64 {
    match kind {
        "symbol" => 3.0 + ring,
        "dot" => 2.0 + ring,
        "dashed" => 1.0 + ring,
        _ => 0.0,
    }
}

/// Detected symbols matched against model holes, with an optional PNG of the match.
pub struct SymbolReport {
    pub json: String,
    pub png: Option<Vec<u8>>,
}

pub fn symbols(
    source: &Source,
    cal: &Cal,
    region: Option<(f64, f64, f64, f64)>,
    dpi: u32,
    holes: &[Hole],
    draw: bool,
) -> Result<SymbolReport, String> {
    let dpi = source.effective_dpi(dpi);
    let region = region.unwrap_or((0.0, 0.0, cal.length, cal.width));
    let (cx, cy, cw, ch) = region_px(cal, dpi, region);
    let g = source.render(dpi, Some((cx, cy, cw, ch)))?;
    let ppm = cal.px_per_mm() * dpi as f64 / cal.dpi as f64;
    let to_mm = cal.to_mm(dpi, (cx as f64, cy as f64));
    let to_px = cal.to_px(dpi, (cx as f64, cy as f64));
    let t = Instant::now();
    let all: Vec<Symbol> = detect_symbols(&g, ppm, &to_mm)
        .into_iter()
        .filter(|s| s.x > -2.0 && s.x < cal.length + 2.0 && s.y > -2.0 && s.y < cal.width + 2.0)
        .collect();
    let dashed: Vec<&Symbol> = all.iter().filter(|s| s.kind == "dashed").collect();
    let dashed_json: Vec<String> = dashed
        .iter()
        .map(|s| {
            format!(
                "{{\"x\":{:.1},\"y\":{:.1},\"diameter\":{:.1}}}",
                s.x, s.y, s.d
            )
        })
        .collect();
    let syms: Vec<Symbol> = all.iter().filter(|s| s.kind != "dashed").cloned().collect();
    let detect_ms = t.elapsed().as_millis();

    let mut matched: Vec<(usize, usize, f64)> = Vec::new();
    let mut used = vec![false; syms.len()];
    let mut model_only: Vec<usize> = Vec::new();
    for (hi, h) in holes.iter().enumerate() {
        if h.x < region.0 || h.x > region.2 || h.y < region.1 || h.y > region.3 {
            continue;
        }
        let mut best: Option<(f64, usize)> = None;
        for (si, s) in syms.iter().enumerate() {
            if used[si] {
                continue;
            }
            let dist = ((h.x - s.x).powi(2) + (h.y - s.y).powi(2)).sqrt();
            if dist <= 2.5 && best.is_none_or(|b| dist < b.0) {
                best = Some((dist, si));
            }
        }
        match best {
            Some((d, si)) => {
                used[si] = true;
                matched.push((hi, si, d));
            }
            None => model_only.push(hi),
        }
    }
    let print_only: Vec<usize> = (0..syms.len()).filter(|i| !used[*i]).collect();
    let mut png = None;
    if draw {
        let mut cv = Canvas::from_gray(&g);
        for (hi, _, _) in &matched {
            let h = &holes[*hi];
            let (px, py) = to_px(h.x, h.y);
            cv.circle(
                px,
                py,
                (h.d / 2.0 * ppm).max(3.0),
                GREEN,
                (ppm as usize).max(2),
            );
        }
        for hi in &model_only {
            let h = &holes[*hi];
            let (px, py) = to_px(h.x, h.y);
            cv.circle(
                px,
                py,
                (h.d / 2.0 * ppm).max(4.0) + 2.0,
                RED,
                (ppm as usize).max(3),
            );
            cv.cross(px, py, (8.0 * ppm / 2.0) as i64, RED);
        }
        for si in &print_only {
            let s = &syms[*si];
            cv.circle(
                s.px.0,
                s.px.1,
                s.d / 2.0 * ppm + 4.0,
                MAGENTA,
                (ppm as usize).max(3),
            );
        }
        if holes.is_empty() {
            for s in &syms {
                cv.circle(
                    s.px.0,
                    s.px.1,
                    s.d / 2.0 * ppm + 3.0,
                    ORANGE,
                    (ppm as usize).max(2),
                );
            }
        }
        png = Some(encode_png(cv.w, cv.h, &cv.rgb)?);
    }
    let sym_json: Vec<String> = syms
        .iter()
        .map(|s| format!("{{\"x\":{:.1},\"y\":{:.1},\"diameter\":{:.1},\"counterbore\":{},\"drawn\":\"{}\"}}", s.x, s.y, s.d, s.cb.map_or("null".to_string(), |v| format!("{v:.1}")), s.kind))
        .collect();
    let mo: Vec<String> = model_only
        .iter()
        .map(|hi| {
            format!(
                "{{\"x\":{},\"y\":{},\"diameter\":{}}}",
                holes[*hi].x, holes[*hi].y, holes[*hi].d
            )
        })
        .collect();
    let po: Vec<String> = print_only
        .iter()
        .map(|si| {
            format!(
                "{{\"x\":{:.1},\"y\":{:.1},\"diameter\":{:.1}}}",
                syms[*si].x, syms[*si].y, syms[*si].d
            )
        })
        .collect();
    let far: Vec<String> = matched
        .iter()
        .filter(|m| m.2 > 1.0)
        .map(|m| {
            format!(
                "{{\"model\":[{},{}],\"drawn\":[{:.1},{:.1}],\"offset_mm\":{:.1}}}",
                holes[m.0].x, holes[m.0].y, syms[m.1].x, syms[m.1].y, m.2
            )
        })
        .collect();
    let json = format!(
        "{{\"ok\":true,\"dpi\":{},\"region_mm\":[{},{},{},{}],\"detect_ms\":{},\"note\":\"symbols are circles or solid dots found at ink crossings; text, arrowheads and concentric rings can still slip through, so treat print_only entries as places to look at, never as positions to model from\",\"symbols_found\":{},\"symbols\":[{}],\"partial_rings\":[{}],\"model_holes_in_region\":{},\"matched\":{},\"model_only\":[{}],\"print_only\":[{}],\"matched_offset_over_1mm\":[{}]{}}}",
        dpi, region.0, region.1, region.2, region.3, detect_ms, syms.len(), sym_json.join(","), dashed_json.join(","),
        matched.len() + model_only.len(), matched.len(), mo.join(","), po.join(","), far.join(","),
        if draw { ",\"legend\":\"green = model hole on a drawn symbol, red = model hole with no symbol, magenta = drawn symbol with no model hole, orange = symbol (no model given)\"" } else { "" }
    );
    Ok(SymbolReport { json, png })
}

/// Synthetic prints for tests and examples: a sheet with a heavy plate
/// outline, light dimension lines, a title-block grid and a frame, plus hole
/// symbols to draw on it. Nothing here reads a real drawing.
pub mod synthetic {
    use super::*;

    /// A synthetic sheet at 300 dpi: a plate outline drawn heavy, dimension
    /// lines drawn light, a title-block-like grid at the bottom, and hole symbols.
    /// A 2400 x 1600 px sheet at 300 dpi with a 200 x 80 mm plate at 5 px/mm; returns
    /// the image, the pixels per millimetre and the image position of the plate's lower-left corner.
    pub fn sheet() -> (Gray, f64, (usize, usize)) {
        let (w, h) = (2400usize, 1600usize);
        let mut px = vec![235u8; w * h];
        let mut line = |x0: i64, y0: i64, x1: i64, y1: i64, width: i64, value: u8| {
            let n = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
            for i in 0..=n {
                let x = x0 + (x1 - x0) * i / n;
                let y = y0 + (y1 - y0) * i / n;
                for dx in -width / 2..=width / 2 {
                    for dy in -width / 2..=width / 2 {
                        let (px_x, px_y) = (x + dx, y + dy);
                        if px_x >= 0 && px_y >= 0 && (px_x as usize) < w && (px_y as usize) < h {
                            px[px_y as usize * w + px_x as usize] = value;
                        }
                    }
                }
            }
        };

        let (x0, x1, y0, y1) = (400i64, 1400i64, 500i64, 900i64);
        for (a, b, c, d) in [
            (x0, y0, x1, y0),
            (x1, y0, x1, y1),
            (x1, y1, x0, y1),
            (x0, y1, x0, y0),
        ] {
            line(a, b, c, d, 5, 20);
        }

        line(x0, y0 - 120, x1, y0 - 120, 2, 40);
        line(x0, y0, x0, y0 - 140, 2, 40);
        line(x1, y0, x1, y0 - 140, 2, 40);
        line(x1 + 150, y0, x1 + 150, y1, 2, 40);
        line(x1, y0, x1 + 170, y0, 2, 40);
        line(x1, y1, x1 + 170, y1, 2, 40);

        for i in 0..6 {
            line(1500, 1200 + i * 60, 2300, 1200 + i * 60, 2, 40);
        }
        for i in 0..5 {
            line(1500 + i * 200, 1200, 1500 + i * 200, 1500, 2, 40);
        }

        for (a, b, c, d) in [
            (60, 60, 2340, 60),
            (2340, 60, 2340, 1540),
            (2340, 1540, 60, 1540),
            (60, 1540, 60, 60),
        ] {
            line(a, b, c, d, 5, 20);
        }
        let g = Gray { w, h, px };
        (g, 5.0, (x0 as usize, y1 as usize))
    }

    /// Draw a hole symbol: a circle of radius r px with a crosshair, at image (cx, cy).
    pub fn symbol(g: &mut Gray, cx: f64, cy: f64, r: f64) {
        let steps = (r * 8.0) as usize;
        for k in 0..steps {
            let a = k as f64 * std::f64::consts::TAU / steps as f64;
            for t in 0..2 {
                let (x, y) = (
                    (cx + (r + t as f64) * a.cos()).round() as i64,
                    (cy + (r + t as f64) * a.sin()).round() as i64,
                );
                if x >= 0 && y >= 0 && (x as usize) < g.w && (y as usize) < g.h {
                    g.px[y as usize * g.w + x as usize] = 20;
                }
            }
        }
        let arm = (r * 1.5) as i64;
        for d in -arm..=arm {
            for (x, y) in [(cx as i64 + d, cy as i64), (cx as i64, cy as i64 + d)] {
                if x >= 0 && y >= 0 && (x as usize) < g.w && (y as usize) < g.h {
                    g.px[y as usize * g.w + x as usize] = 20;
                }
            }
        }
    }

    /// PNG bytes of a grayscale image, for writing a synthetic print to disk.
    pub fn png(image: &Gray) -> Vec<u8> {
        let mut rgb = Vec::with_capacity(image.w * image.h * 3);
        for v in &image.px {
            rgb.extend_from_slice(&[*v, *v, *v]);
        }
        encode_png(image.w, image.h, &rgb).expect("png")
    }
}

#[cfg(test)]
mod tests {
    use super::synthetic::{sheet, symbol};
    use super::*;

    #[test]
    fn calibration_finds_the_heavy_outline_not_the_frame_dimensions_or_title_block() {
        let (g, _, _) = sheet();
        let source = Source::Memory { image: g, dpi: 300 };
        let cal = calibrate(&source, 200.0, 80.0).unwrap();
        assert!(
            (cal.tl.0 - 400.0).abs() <= 2.0 && (cal.tl.1 - 500.0).abs() <= 2.0,
            "{:?}",
            cal.tl
        );
        assert!(
            (cal.br.0 - 1400.0).abs() <= 2.0 && (cal.br.1 - 900.0).abs() <= 2.0,
            "{:?}",
            cal.br
        );
        assert!((cal.px_per_mm() - 5.0).abs() < 0.05);
        assert!(cal.skew_deg().abs() < 0.2);
    }

    #[test]
    fn ring_score_sees_a_symbol_at_a_hole_and_nothing_on_blank_paper() {
        let (mut g, ppm, (ox, oy)) = sheet();

        symbol(&mut g, ox as f64 + 50.0 * ppm, oy as f64 - 30.0 * ppm, 12.0);
        let source = Source::Memory { image: g, dpi: 300 };
        let cal = calibrate(&source, 200.0, 80.0).unwrap();
        let holes = [
            Hole {
                x: 50.0,
                y: 30.0,
                d: 5.0,
                cb: None,
            },
            Hole {
                x: 150.0,
                y: 30.0,
                d: 5.0,
                cb: None,
            },
        ];
        let report: serde_json::Value =
            serde_json::from_str(&ring_score(&source, &cal, &holes, 300, 2.5).unwrap()).unwrap();
        let items = report["items"].as_array().unwrap();
        assert_eq!(items[0]["drawn"], "symbol", "{}", items[0]);
        assert!(
            items[0]["offset_mm"].as_f64().unwrap() < 0.6,
            "{}",
            items[0]
        );
        assert!(
            (items[0]["drawn_diameter_mm"].as_f64().unwrap() - 4.8).abs() < 0.8,
            "{}",
            items[0]
        );
        assert_eq!(items[1]["drawn"], "none", "{}", items[1]);
        assert_eq!(report["nothing_drawn_within_search"], 1);
    }

    #[test]
    fn symbols_are_found_and_matched_against_model_holes() {
        let (mut g, ppm, (ox, oy)) = sheet();
        for (x, y) in [(50.0, 30.0), (100.0, 30.0), (150.0, 50.0)] {
            symbol(&mut g, ox as f64 + x * ppm, oy as f64 - y * ppm, 12.0);
        }
        let source = Source::Memory { image: g, dpi: 300 };
        let cal = calibrate(&source, 200.0, 80.0).unwrap();
        let holes = [
            Hole {
                x: 50.0,
                y: 30.0,
                d: 5.0,
                cb: None,
            },
            Hole {
                x: 100.0,
                y: 30.0,
                d: 5.0,
                cb: None,
            },
            Hole {
                x: 20.0,
                y: 60.0,
                d: 5.0,
                cb: None,
            },
        ];
        let report = symbols(&source, &cal, None, 300, &holes, true).unwrap();
        let value: serde_json::Value = serde_json::from_str(&report.json).unwrap();
        assert_eq!(value["symbols_found"], 3, "{}", report.json);
        assert_eq!(value["matched"], 2, "{}", report.json);
        assert_eq!(value["model_only"].as_array().unwrap().len(), 1);
        assert_eq!(value["print_only"].as_array().unwrap().len(), 1);
        let extra = &value["print_only"][0];
        assert!(
            (extra["x"].as_f64().unwrap() - 150.0).abs() < 1.0
                && (extra["y"].as_f64().unwrap() - 50.0).abs() < 1.0,
            "{extra}"
        );
        assert!(report.png.unwrap().starts_with(b"\x89PNG"));
    }

    #[test]
    fn crop_writes_a_png_of_the_requested_window() {
        let (g, _, _) = sheet();
        let source = Source::Memory { image: g, dpi: 300 };
        let cal = calibrate(&source, 200.0, 80.0).unwrap();
        let image = crop(
            &source,
            &cal,
            (0.0, 0.0, 100.0, 40.0),
            300,
            &[Hole {
                x: 10.0,
                y: 10.0,
                d: 6.0,
                cb: Some(10.0),
            }],
            10.0,
        )
        .unwrap();
        assert!(image.png.starts_with(b"\x89PNG"));
        assert!(
            (image.width as f64 - 500.0).abs() < 6.0 && (image.height as f64 - 200.0).abs() < 6.0,
            "{}x{}",
            image.width,
            image.height
        );
        assert!((image.px_per_mm - 5.0).abs() < 0.05);
    }
}
