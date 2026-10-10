//! Presentation-only drawing navigation, following DrawingWorkspace.tsx.
//! It never edits the sheet,
//! changes a drawing view's scale, or acquires the document/OCCT locks.
use limo_cad_interface::{DocumentContext, Rect};

const PX_PER_MM: f64 = 3.;
const MIN_ZOOM: f64 = 0.01;
const MAX_ZOOM: f64 = 5.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Pane {
    pub bounds: Rect,
    /// Left, top, right, bottom logical pixels. The native painter supplies
    /// its existing chrome spacing; navigation does not invent another pane.
    pub padding: [f64; 4],
}
impl Pane {
    fn available(self) -> [f64; 2] {
        [
            self.bounds.width - self.padding[0] - self.padding[2],
            self.bounds.height - self.padding[1] - self.padding[3],
        ]
    }
    fn validate(self) -> Result<(), String> {
        if [
            self.bounds.x,
            self.bounds.y,
            self.bounds.width,
            self.bounds.height,
        ]
        .into_iter()
        .chain(self.padding)
        .any(|n| !n.is_finite())
            || self.padding.iter().any(|n| *n < 0.)
            || self.available().iter().any(|n| *n <= 0.)
        {
            return Err("Drawing pane has invalid bounds or padding".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PaperTransform {
    pub origin: [f64; 2],
    pub scale: f64,
    pub sheet_mm: [f64; 2],
    pub clip: Rect,
}
impl PaperTransform {
    pub(super) fn to_screen(self, paper: [f64; 2]) -> [f64; 2] {
        [
            self.origin[0] + paper[0] * self.scale,
            self.origin[1] + paper[1] * self.scale,
        ]
    }
    /// The same inverse is used for view/annotation drags and anchor picking.
    /// A captured drag may use it outside the pane; picking uses `pick` below.
    pub(super) fn to_paper(self, screen: [f64; 2]) -> [f64; 2] {
        [
            (screen[0] - self.origin[0]) / self.scale,
            (screen[1] - self.origin[1]) / self.scale,
        ]
    }
    pub(super) fn pick(self, screen: [f64; 2]) -> Option<[f64; 2]> {
        let paper = self.to_paper(screen);
        (inside(self.clip, screen)
            && (0..2).all(|i| paper[i] >= 0. && paper[i] <= self.sheet_mm[i]))
        .then_some(paper)
    }
    /// Visible paper rectangle in millimetres: x, y, width, height. The edge
    /// cache adds a physical stroke guard to this region before rasterizing.
    pub(super) fn visible_paper(self) -> Option<[f64; 4]> {
        let a = self.to_paper([self.clip.x, self.clip.y]);
        let b = self.to_paper([
            self.clip.x + self.clip.width,
            self.clip.y + self.clip.height,
        ]);
        let min = [a[0].max(0.), a[1].max(0.)];
        let max = [b[0].min(self.sheet_mm[0]), b[1].min(self.sheet_mm[1])];
        (max[0] > min[0] && max[1] > min[1]).then_some([
            min[0],
            min[1],
            max[0] - min[0],
            max[1] - min[1],
        ])
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum WheelUnit {
    Line,
    Pixel,
}
pub(super) struct Wheel {
    /// Original Winit deltas: pixels are physical, signs oppose DOM deltas.
    pub delta: [f64; 2],
    pub unit: WheelUnit,
    pub window_scale: f64,
    pub ctrl: bool,
    pub alt: bool,
    pub macos: bool,
    pub now_ms: f64,
}
#[derive(Clone, Copy, PartialEq)]
enum WheelKind {
    Mouse,
    Trackpad,
}
#[derive(Default)]
struct WheelGesture {
    kind: Option<WheelKind>,
    last_ms: f64,
    count: u32,
}
impl WheelGesture {
    fn pans(&mut self, unit: WheelUnit, delta: [f64; 2], now: f64) -> bool {
        let gap = now - self.last_ms;
        self.last_ms = now;
        if gap > 350. || gap < 0. {
            self.kind = None;
            self.count = 0;
        }
        self.count = self.count.saturating_add(1);
        if unit != WheelUnit::Pixel {
            self.kind = Some(WheelKind::Mouse);
            return false;
        }
        if delta[0] != 0. || delta[1].fract() != 0. || delta[1].abs() < 50. {
            self.kind = Some(WheelKind::Trackpad);
            return true;
        }
        if self.kind == Some(WheelKind::Trackpad) {
            return true;
        }
        if self.count >= 3 && gap < 120. {
            self.kind = Some(WheelKind::Trackpad);
            return true;
        }
        if self.kind == Some(WheelKind::Mouse) {
            return false;
        }
        if delta[1].abs() >= 100. && gap > 250. {
            self.kind = Some(WheelKind::Mouse);
            return false;
        }
        true
    }
}
struct Pan {
    cursor: [f64; 2],
    scroll: [f64; 2],
}

pub(super) struct Navigation {
    owner: DocumentContext,
    sheet_id: u64,
    sheet_mm: [f64; 2],
    pane: Pane,
    pub zoom: f64,
    pub fitted: bool,
    scroll: [f64; 2],
    pan: Option<Pan>,
    wheel: WheelGesture,
}
impl Navigation {
    pub(super) fn new(
        owner: DocumentContext,
        sheet_id: u64,
        sheet_mm: [f64; 2],
        pane: Pane,
    ) -> Result<Self, String> {
        validate_sheet(sheet_mm)?;
        pane.validate()?;
        let mut state = Self {
            owner,
            sheet_id,
            sheet_mm,
            pane,
            zoom: 1.,
            fitted: true,
            scroll: [0.; 2],
            pan: None,
            wheel: WheelGesture::default(),
        };
        state.fit();
        Ok(state)
    }
    pub(super) fn matches(&self, owner: &DocumentContext, sheet_id: u64) -> bool {
        &self.owner == owner && self.sheet_id == sheet_id
    }
    pub(super) fn is_panning(&self) -> bool {
        self.pan.is_some()
    }
    /// Observe active owner/sheet and pane changes from the completed frame.
    /// Owner/sheet/format changes refit; an ordinary resize only refits while
    /// fitted. A resize cancels capture.
    pub(super) fn observe(
        &mut self,
        owner: DocumentContext,
        sheet_id: u64,
        sheet_mm: [f64; 2],
        pane: Pane,
    ) -> Result<(), String> {
        validate_sheet(sheet_mm)?;
        pane.validate()?;
        let reset = !self.matches(&owner, sheet_id) || self.sheet_mm != sheet_mm;
        let resize = self.pane != pane;
        self.owner = owner;
        self.sheet_id = sheet_id;
        self.sheet_mm = sheet_mm;
        self.pane = pane;
        if reset || resize {
            self.cancel();
        }
        if reset || (resize && self.fitted) {
            self.fit();
        } else {
            self.clamp_scroll();
        }
        Ok(())
    }
    pub(super) fn transform(&self) -> PaperTransform {
        let scale = self.zoom * PX_PER_MM;
        let available = self.pane.available();
        PaperTransform {
            origin: [
                self.pane.bounds.x
                    + self.pane.padding[0]
                    + ((available[0] - self.sheet_mm[0] * scale) * 0.5).max(0.)
                    - self.scroll[0],
                self.pane.bounds.y + self.pane.padding[1] - self.scroll[1],
            ],
            scale,
            sheet_mm: self.sheet_mm,
            clip: self.pane.bounds,
        }
    }
    fn clamp_scroll(&mut self) {
        let available = self.pane.available();
        for (i, extent) in available.iter().enumerate() {
            self.scroll[i] = self.scroll[i].clamp(
                0.,
                (self.sheet_mm[i] * self.zoom * PX_PER_MM - extent).max(0.),
            );
        }
    }
    pub(super) fn fit(&mut self) {
        let available = self.pane.available();
        self.zoom = MAX_ZOOM
            .min(available[0] / (self.sheet_mm[0] * PX_PER_MM))
            .min(available[1] / (self.sheet_mm[1] * PX_PER_MM));
        self.fitted = true;
        self.scroll = [0.; 2];
        self.cancel();
    }
    pub(super) fn zoom_at(&mut self, requested: f64, point: Option<[f64; 2]>) -> bool {
        if !requested.is_finite() || point.is_some_and(|p| p.iter().any(|n| !n.is_finite())) {
            return false;
        }
        self.fitted = false;
        let zoom = requested.clamp(MIN_ZOOM, MAX_ZOOM);
        if (zoom - self.zoom).abs() < 1e-4 {
            return false;
        }
        let point = point.unwrap_or([
            self.pane.bounds.x + self.pane.bounds.width * 0.5,
            self.pane.bounds.y + self.pane.bounds.height * 0.5,
        ]);
        let paper = self.transform().to_paper(point);
        let anchor = [
            paper[0].clamp(0., self.sheet_mm[0]),
            paper[1].clamp(0., self.sheet_mm[1]),
        ];
        self.zoom = zoom;
        self.scroll = [0.; 2];
        let projected = self.transform().to_screen(anchor);
        self.scroll = [projected[0] - point[0], projected[1] - point[1]];
        self.clamp_scroll();
        true
    }
    pub(super) fn wheel(
        &mut self,
        owner: &DocumentContext,
        sheet_id: u64,
        cursor: [f64; 2],
        wheel: Wheel,
    ) -> bool {
        if !self.matches(owner, sheet_id)
            || !inside(self.pane.bounds, cursor)
            || self.pan.is_some()
            || wheel.delta.iter().any(|n| !n.is_finite())
            || !wheel.now_ms.is_finite()
            || !wheel.window_scale.is_finite()
            || wheel.window_scale <= 0.
        {
            return false;
        }
        let factor = if wheel.unit == WheelUnit::Line {
            16.
        } else {
            wheel.window_scale.recip()
        };
        let logical = wheel.delta.map(|v| -v * factor);
        let delta = logical.map(|v| v.clamp(-240., 240.));
        if delta == [0.; 2] {
            return true;
        }
        let pan = !wheel.ctrl
            && !wheel.alt
            && if wheel.macos {
                wheel.unit == WheelUnit::Pixel
            } else {
                self.wheel.pans(wheel.unit, logical, wheel.now_ms)
            };
        if pan {
            self.fitted = false;
            self.scroll[0] += delta[0];
            self.scroll[1] += delta[1];
            self.clamp_scroll();
        } else {
            self.zoom_at(
                self.zoom * (-delta[1] * if wheel.ctrl { 0.007 } else { 0.002 }).exp(),
                Some(cursor),
            );
        }
        true
    }
    /// Native PinchGesture is incremental (positive means magnify), unlike
    /// WebKit's total gesture scale. This retains the same anchored zoom.
    pub(super) fn pinch(
        &mut self,
        owner: &DocumentContext,
        sheet_id: u64,
        cursor: [f64; 2],
        delta: f64,
    ) -> bool {
        if !self.matches(owner, sheet_id)
            || !inside(self.pane.bounds, cursor)
            || self.pan.is_some()
            || !delta.is_finite()
        {
            return false;
        }
        self.zoom_at(self.zoom * delta.clamp(-1., 1.).exp(), Some(cursor));
        true
    }
    /// The event adapter calls this only for the middle button, after its
    /// modal/control check. Capture continues beyond the pane until release.
    pub(super) fn begin_pan(
        &mut self,
        owner: &DocumentContext,
        sheet_id: u64,
        cursor: [f64; 2],
    ) -> bool {
        if !self.matches(owner, sheet_id) || !inside(self.pane.bounds, cursor) || self.pan.is_some()
        {
            return false;
        }
        self.fitted = false;
        self.pan = Some(Pan {
            cursor,
            scroll: self.scroll,
        });
        true
    }
    pub(super) fn pan_to(
        &mut self,
        owner: &DocumentContext,
        sheet_id: u64,
        cursor: [f64; 2],
    ) -> bool {
        if !self.matches(owner, sheet_id) || cursor.iter().any(|n| !n.is_finite()) {
            self.cancel();
            return false;
        }
        let Some(pan) = &self.pan else {
            return false;
        };
        self.scroll = [
            pan.scroll[0] - cursor[0] + pan.cursor[0],
            pan.scroll[1] - cursor[1] + pan.cursor[1],
        ];
        self.clamp_scroll();
        true
    }
    /// Call on button release, focus/cursor/owner loss, Escape, and DPI change.
    pub(super) fn cancel(&mut self) -> bool {
        self.wheel = WheelGesture::default();
        self.pan.take().is_some()
    }
}
fn validate_sheet(size: [f64; 2]) -> Result<(), String> {
    if size.iter().any(|n| !n.is_finite() || *n <= 0.) {
        return Err("Drawing sheet has invalid paper dimensions".into());
    }
    Ok(())
}
fn inside(rect: Rect, p: [f64; 2]) -> bool {
    p.iter().all(|n| n.is_finite())
        && p[0] >= rect.x
        && p[1] >= rect.y
        && p[0] < rect.x + rect.width
        && p[1] < rect.y + rect.height
}

#[cfg(test)]
#[path = "drawing_navigation/tests.rs"]
mod tests;
