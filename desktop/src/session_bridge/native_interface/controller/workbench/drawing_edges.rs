//! Batched projected edges for drawing paper. OCCT projections remain complete
//! for associative annotations; one retained physical-resolution image carries
//! the visible strokes.
use super::{paper_point, raster_stroke_width, Label};
use crate::state::{DrawingProjectionBasis, ResolvedDrawingProjection};
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use limo_cad_interface::DocumentContext;
use limo_cad_occt::drawing_export::{PaperGraphicsBudget, PaperPrimitive};
use limo_cad_occt::DrawingProjectionDto;
use limo_cad_sketch::{
    DrawingLineStyleDto, DrawingSheetDto, DrawingViewDerivationDto, DrawingViewDto,
};
use resvg::tiny_skia::{
    LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, StrokeDash, Transform,
};
use std::collections::BTreeMap;
use std::sync::Arc;

#[path = "drawing_edges/derived.rs"]
mod derived;
#[path = "drawing_edges/presentation.rs"]
mod presentation;

pub(super) type Projections = BTreeMap<u64, (DrawingViewDto, DrawingProjectionDto)>;
pub(super) type ProjectionBases = BTreeMap<u64, DrawingProjectionBasis>;

#[derive(Clone)]
pub(super) struct SourceKey {
    document_revision: u64,
    geometry_revision: u64,
    layout: Arc<SourceLayout>,
}

/// Immutable source metadata shared by paper, projection cache and drag stamps.
/// Revision stamps stay in SourceKey so selecting a sheet cannot retag a saved
/// drag or an error receipt through shared ownership.
#[derive(Clone, PartialEq)]
struct SourceLayout {
    owner: DocumentContext,
    sheet_id: u64,
    views: Vec<DrawingViewDto>,
    visible: DrawingLineStyleDto,
    hidden: DrawingLineStyleDto,
    hatch: DrawingLineStyleDto,
    cutting_plane: DrawingLineStyleDto,
    phantom: DrawingLineStyleDto,
    break_line: DrawingLineStyleDto,
    hatch_spacing_mm: f64,
    text_height_mm: f64,
}

impl PartialEq for SourceKey {
    fn eq(&self, other: &Self) -> bool {
        self.document_revision == other.document_revision
            && self.geometry_revision == other.geometry_revision
            && (Arc::ptr_eq(&self.layout, &other.layout) || self.layout == other.layout)
    }
}

impl SourceKey {
    pub(super) fn belongs_to_document(&self, owner: &DocumentContext) -> bool {
        self.layout.owner.window_id == owner.window_id
            && self.layout.owner.document_id == owner.document_id
    }

    pub(super) fn new(
        owner: DocumentContext,
        document_revision: u64,
        geometry_revision: u64,
        sheet: &DrawingSheetDto,
    ) -> Self {
        Self {
            document_revision,
            geometry_revision,
            layout: Arc::new(SourceLayout::new(owner, sheet)),
        }
    }

    pub(super) fn refresh(
        &mut self,
        owner: &DocumentContext,
        document_revision: u64,
        geometry_revision: u64,
        sheet: &DrawingSheetDto,
    ) {
        if !self.layout.matches(owner, sheet) {
            self.layout = Arc::new(SourceLayout::new(owner.clone(), sheet));
        }
        self.document_revision = document_revision;
        self.geometry_revision = geometry_revision;
    }
}

impl SourceLayout {
    fn matches(&self, owner: &DocumentContext, sheet: &DrawingSheetDto) -> bool {
        &self.owner == owner
            && self.sheet_id == sheet.id
            && self.views == sheet.views
            && self.visible == sheet.style.visible
            && self.hidden == sheet.style.hidden
            && self.hatch == sheet.style.hatch
            && self.cutting_plane == sheet.style.cutting_plane
            && self.phantom == sheet.style.phantom
            && self.break_line == sheet.style.break_line
            && self.hatch_spacing_mm == sheet.style.hatch_spacing_mm
            && self.text_height_mm == sheet.style.text_height_mm
    }

    fn new(owner: DocumentContext, sheet: &DrawingSheetDto) -> Self {
        Self {
            owner,
            sheet_id: sheet.id,
            views: sheet.views.clone(),
            visible: sheet.style.visible.clone(),
            hidden: sheet.style.hidden.clone(),
            hatch: sheet.style.hatch.clone(),
            cutting_plane: sheet.style.cutting_plane.clone(),
            phantom: sheet.style.phantom.clone(),
            break_line: sheet.style.break_line.clone(),
            hatch_spacing_mm: sheet.style.hatch_spacing_mm,
            text_height_mm: sheet.style.text_height_mm,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RasterKey {
    pub sheet_mm: [f32; 2],
    pub paper_scale: f32,
    pub render_scale: f32,
    /// Visible paper x, y, width, height in millimetres, before stroke guard.
    /// Derived from the same inverse paper transform used for picking.
    pub visible_mm: [f64; 4],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RasterRegion {
    pub origin_mm: [f64; 2],
    pub size_mm: [f64; 2],
    dimensions: [u32; 2],
}

#[derive(Clone, Copy)]
struct Limits {
    points: usize,
    stroke_steps: f64,
    pixels: u64,
    dimension: u32,
    metadata_bytes: usize,
    retained_bytes: usize,
    mask_pixels: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            points: 2_000_000,
            stroke_steps: 4_000_000.,
            pixels: 16_777_216,
            dimension: 8192,
            metadata_bytes: 32 * 1024 * 1024,
            retained_bytes: 128 * 1024 * 1024,
            mask_pixels: 64 * 1024 * 1024,
        }
    }
}

struct Source {
    key: SourceKey,
    projections: Projections,
    bases: ProjectionBases,
    hatches: Vec<PaperPrimitive>,
    view_art: Vec<derived::ViewArtwork>,
    source_marks: Vec<PaperPrimitive>,
    source_labels: Vec<Label>,
    retained_bytes: usize,
}

#[derive(Resource, Default)]
pub(super) struct EdgeCache {
    source: Option<Source>,
    previous_source: Option<Source>,
    raster: Option<(RasterKey, RasterRegion, Handle<Image>)>,
    previous_raster: Option<(RasterKey, RasterRegion, Image)>,
    failure: Option<(SourceKey, RasterKey, String)>,
}

pub(super) struct Prepared<'a> {
    pub image: Handle<Image>,
    /// Place this image inside the paper parent at this exact paper rectangle.
    /// It is already at native physical resolution; never stretch to a sheet.
    pub region: RasterRegion,
    pub projections: &'a Projections,
    pub source_labels: &'a [Label],
    pub source_changed: bool,
}

impl EdgeCache {
    pub(super) fn evict_document(&mut self, owner: &DocumentContext) {
        if self
            .source
            .as_ref()
            .is_some_and(|source| source.key.belongs_to_document(owner))
        {
            self.source = None;
            self.raster = None;
        }
        if self
            .previous_source
            .as_ref()
            .is_some_and(|source| source.key.belongs_to_document(owner))
        {
            self.previous_source = None;
            self.previous_raster = None;
        }
        if self
            .failure
            .as_ref()
            .is_some_and(|(key, _, _)| key.belongs_to_document(owner))
        {
            self.failure = None;
        }
    }

    /// Only a committed SelectSheet may carry projections across a document
    /// revision. Edits, Undo, replay and document replacement still miss the
    /// exact source key, including its owner epoch and geometry revision.
    pub(super) fn advance_sheet_selection(&mut self, owner: &DocumentContext, from: u64, to: u64) {
        if from.checked_add(1) != Some(to) {
            return;
        }
        for source in [&mut self.source, &mut self.previous_source]
            .into_iter()
            .flatten()
        {
            if &source.key.layout.owner == owner && source.key.document_revision == from {
                source.key.document_revision = to;
            }
        }
    }
    pub(super) fn source_labels(&self, key: &SourceKey) -> Option<&[Label]> {
        self.source
            .as_ref()
            .filter(|source| &source.key == key)
            .map(|source| source.source_labels.as_slice())
    }
    pub(super) fn projections(&self, key: &SourceKey) -> Option<&Projections> {
        self.source
            .as_ref()
            .filter(|source| &source.key == key)
            .map(|source| &source.projections)
    }
    pub(super) fn bases(&self, key: &SourceKey) -> Option<&ProjectionBases> {
        self.source
            .as_ref()
            .filter(|source| &source.key == key)
            .map(|source| &source.bases)
    }
    /// All source and raster work succeeds before replacing retained assets.
    /// Callers must hide the edge image/annotations on Err and show its message;
    /// an old document or partial sheet is never returned as a fallback.
    #[cfg(test)]
    pub(super) fn prepare<'a>(
        &'a mut self,
        images: &mut Assets<Image>,
        key: SourceKey,
        raster: RasterKey,
        project: impl FnMut(&DrawingViewDto) -> Result<DrawingProjectionDto, String>,
    ) -> Result<Prepared<'a>, String> {
        self.prepare_with_limits(images, key, raster, project, Limits::default())
    }
    pub(super) fn prepare_sheet<'a>(
        &'a mut self,
        images: &mut Assets<Image>,
        key: SourceKey,
        raster: RasterKey,
        project: impl FnMut(&DrawingViewDto) -> Result<ResolvedDrawingProjection, String>,
        sources: impl FnMut(
            &Projections,
            &mut PaperGraphicsBudget,
        ) -> Result<Vec<PaperPrimitive>, String>,
    ) -> Result<Prepared<'a>, String> {
        self.prepare_sheet_with_limits(images, key, raster, project, sources, Limits::default())
    }
    #[cfg(test)]
    fn prepare_with_limits<'a>(
        &'a mut self,
        images: &mut Assets<Image>,
        key: SourceKey,
        raster: RasterKey,
        mut project: impl FnMut(&DrawingViewDto) -> Result<DrawingProjectionDto, String>,
        limits: Limits,
    ) -> Result<Prepared<'a>, String> {
        self.prepare_sheet_with_limits(
            images,
            key,
            raster,
            |view| {
                Ok(ResolvedDrawingProjection {
                    projection: project(view)?,
                    basis: limo_cad_occt::drawing_projection_basis(view.direction, view.up)
                        .map_err(|error| error.to_string())?,
                })
            },
            |_, _| Ok(vec![]),
            limits,
        )
    }
    fn prepare_sheet_with_limits<'a>(
        &'a mut self,
        images: &mut Assets<Image>,
        key: SourceKey,
        raster: RasterKey,
        project: impl FnMut(&DrawingViewDto) -> Result<ResolvedDrawingProjection, String>,
        sources: impl FnMut(
            &Projections,
            &mut PaperGraphicsBudget,
        ) -> Result<Vec<PaperPrimitive>, String>,
        limits: Limits,
    ) -> Result<Prepared<'a>, String> {
        if let Some((failed_key, failed_raster, error)) = &self.failure {
            if failed_key == &key && *failed_raster == raster {
                return Err(error.clone());
            }
        }
        let source_changed = self.source.as_ref().is_none_or(|source| source.key != key);
        let raster_changed = source_changed
            || self
                .raster
                .as_ref()
                .is_none_or(|(saved, _, handle)| *saved != raster || !images.contains(handle.id()));
        if raster_changed {
            let warm_hit = source_changed
                && self.previous_source.as_ref().is_some_and(|source| {
                    source.key == key && source.retained_bytes <= limits.retained_bytes
                });
            let result = (|| {
                let region = raster.region(&key, limits)?;
                let next = if source_changed && !warm_hit {
                    Some(Source::project_resolved(
                        key.clone(),
                        project,
                        sources,
                        limits,
                    )?)
                } else {
                    None
                };
                let source = next
                    .as_ref()
                    .or(if warm_hit {
                        self.previous_source.as_ref()
                    } else {
                        self.source.as_ref()
                    })
                    .ok_or("Drawing edge source is missing")?;
                let reuse_pixels = warm_hit
                    && self
                        .previous_raster
                        .as_ref()
                        .is_some_and(|(saved, crop, image)| {
                            *saved == raster
                                && *crop == region
                                && raster_bytes(image) <= limits.pixels.saturating_mul(4)
                        });
                let image = if reuse_pixels {
                    None
                } else {
                    Some(source.rasterize_with_limits(raster, region, limits)?)
                };
                Ok::<_, String>((next, image, region))
            })();
            let (next, image, region) = match result {
                Ok(value) => value,
                Err(error) => {
                    self.failure = Some((key, raster, error.clone()));
                    return Err(error);
                }
            };
            let image = image.unwrap_or_else(|| self.previous_raster.take().unwrap().2);
            let mut retired_pixels = None;
            let handle = if let Some((_, _, handle)) = &self.raster {
                if images.contains(handle.id()) {
                    retired_pixels = Some(std::mem::replace(
                        &mut *images.get_mut(handle).unwrap(),
                        image,
                    ));
                    handle.clone()
                } else {
                    images.add(image)
                }
            } else {
                images.add(image)
            };
            if source_changed {
                let source = next
                    .or_else(|| self.previous_source.take())
                    .expect("A changed source was projected or found in the warm cache");
                let retained_bytes = source.retained_bytes;
                self.previous_source = self.source.replace(source).filter(|previous| {
                    previous.retained_bytes.saturating_add(retained_bytes) <= limits.retained_bytes
                });
                self.previous_raster = if self.previous_source.is_some() {
                    self.raster
                        .as_ref()
                        .zip(retired_pixels)
                        .map(|((saved, crop, _), image)| (*saved, *crop, image))
                } else {
                    None
                };
            }
            let current_bytes = raster_bytes(images.get(&handle).unwrap());
            if self.previous_raster.as_ref().is_some_and(|(_, _, image)| {
                current_bytes.saturating_add(raster_bytes(image)) > limits.pixels.saturating_mul(4)
            }) {
                self.previous_raster = None;
            }
            self.raster = Some((raster, region, handle));
            self.failure = None;
        }
        Ok(Prepared {
            image: self.raster.as_ref().unwrap().2.clone(),
            region: self.raster.as_ref().unwrap().1,
            projections: &self.source.as_ref().unwrap().projections,
            source_labels: &self.source.as_ref().unwrap().source_labels,
            source_changed,
        })
    }
}

fn raster_bytes(image: &Image) -> u64 {
    image
        .data
        .as_ref()
        .map_or(0, |pixels| pixels.capacity() as u64)
}

impl RasterKey {
    fn region(self, source: &SourceKey, limits: Limits) -> Result<RasterRegion, String> {
        if self
            .sheet_mm
            .into_iter()
            .chain([self.paper_scale, self.render_scale])
            .any(|v| !v.is_finite() || v <= 0.)
        {
            return Err("Drawing paper size or DPI is invalid".into());
        }
        if self.visible_mm.iter().any(|n| !n.is_finite())
            || self.visible_mm[2..].iter().any(|n| *n <= 0.)
        {
            return Err("Drawing visible paper region is invalid".into());
        }
        let scale = f64::from(self.paper_scale) * f64::from(self.render_scale);
        let mut stroke = 0f64;
        let section = source.layout.views.iter().any(|view| {
            matches!(
                view.derivation,
                Some(
                    DrawingViewDerivationDto::Section { .. }
                        | DrawingViewDerivationDto::RemovedSection { .. }
                )
            )
        });
        let detail = source.layout.views.iter().any(|view| {
            matches!(
                view.derivation,
                Some(DrawingViewDerivationDto::Detail { .. })
            )
        });
        let auxiliary = source.layout.views.iter().any(|view| {
            matches!(
                view.derivation,
                Some(DrawingViewDerivationDto::Auxiliary { .. })
            )
        });
        let broken = source.layout.views.iter().any(|view| {
            matches!(
                view.derivation,
                Some(DrawingViewDerivationDto::Broken { .. })
            )
        });
        for style in [&source.layout.visible, &source.layout.hidden]
            .into_iter()
            .chain(section.then_some(&source.layout.hatch))
            .chain(section.then_some(&source.layout.cutting_plane))
            .chain((detail || auxiliary).then_some(&source.layout.phantom))
            .chain(broken.then_some(&source.layout.break_line))
        {
            let width =
                raster_stroke_width(style.width_mm as f32, self.paper_scale, self.render_scale)
                    * self.render_scale;
            if !style.width_mm.is_finite()
                || style.width_mm <= 0.
                || !width.is_finite()
                || width <= 0.
            {
                return Err("Drawing edge width cannot be rendered at this scale".into());
            }
            stroke = stroke.max(f64::from(width));
        }
        if detail || auxiliary {
            stroke = stroke.max(f64::from(
                raster_stroke_width(0.48, self.paper_scale, self.render_scale) * self.render_scale,
            ));
        }
        let guard = stroke * 2. + 2.;
        let mut start = [0.; 2];
        let mut end = [0.; 2];
        for i in 0..2 {
            let min = self.visible_mm[i].max(0.);
            let max =
                (self.visible_mm[i] + self.visible_mm[i + 2]).min(f64::from(self.sheet_mm[i]));
            if !max.is_finite() || max <= min {
                return Err("Drawing visible region does not intersect its paper".into());
            }
            start[i] = (min * scale - guard).floor().max(0.);
            end[i] = (max * scale + guard)
                .ceil()
                .min((f64::from(self.sheet_mm[i]) * scale).ceil());
        }
        let physical = [end[0] - start[0], end[1] - start[1]];
        if physical
            .iter()
            .any(|v| !v.is_finite() || *v > limits.dimension as f64)
        {
            return Err(format!(
                "Drawing visible region and stroke guard exceed {} physical pixels on one axis; resize the window or reduce its display scale",
                limits.dimension
            ));
        }
        let [width, height] = physical.map(|v| v.ceil().max(1.) as u32);
        if u64::from(width) * u64::from(height) > limits.pixels {
            return Err(format!(
                "Drawing visible region and stroke guard exceed the {} pixel rendering budget; resize the window or reduce its display scale",
                limits.pixels
            ));
        }
        Ok(RasterRegion {
            origin_mm: start.map(|n| n / scale),
            size_mm: [f64::from(width) / scale, f64::from(height) / scale],
            dimensions: [width, height],
        })
    }
}

impl Source {
    #[cfg(test)]
    fn project(
        key: SourceKey,
        mut project: impl FnMut(&DrawingViewDto) -> Result<DrawingProjectionDto, String>,
        sources: impl FnMut(
            &Projections,
            &mut PaperGraphicsBudget,
        ) -> Result<Vec<PaperPrimitive>, String>,
        limits: Limits,
    ) -> Result<Self, String> {
        Self::project_resolved(
            key,
            |view| {
                Ok(ResolvedDrawingProjection {
                    projection: project(view)?,
                    basis: limo_cad_occt::drawing_projection_basis(view.direction, view.up)
                        .map_err(|error| error.to_string())?,
                })
            },
            sources,
            limits,
        )
    }
    fn project_resolved(
        key: SourceKey,
        mut project: impl FnMut(&DrawingViewDto) -> Result<ResolvedDrawingProjection, String>,
        sources: impl FnMut(
            &Projections,
            &mut PaperGraphicsBudget,
        ) -> Result<Vec<PaperPrimitive>, String>,
        limits: Limits,
    ) -> Result<Self, String> {
        let mut projections = Projections::new();
        let mut bases = ProjectionBases::new();
        let mut points = 0usize;
        let mut metadata = 0usize;
        let mut retained = 0usize;
        let mut steps = 0.;
        for view in &key.layout.views {
            if !view.scale.is_finite()
                || view.scale <= 0.
                || view.position.iter().any(|v| !v.is_finite())
            {
                return Err(format!(
                    "Drawing view '{}' has invalid paper placement or scale",
                    view.name
                ));
            }
            let resolved = project(view)
                .map_err(|error| format!("Cannot project drawing view '{}': {error}", view.name))?;
            let projection = resolved.projection;
            let basis = resolved.basis;
            if basis
                .direction
                .iter()
                .chain(&basis.up)
                .chain(&basis.right)
                .any(|n| !n.is_finite())
            {
                return Err(format!(
                    "Drawing view '{}' has a non-finite projection basis",
                    view.name
                ));
            }
            bases.insert(view.id, basis);
            retained = retained.saturating_add(128 + std::mem::size_of::<DrawingProjectionBasis>());
            if projection.bounds.iter().any(|v| !v.is_finite()) {
                return Err(format!(
                    "Drawing view '{}' has non-finite projection bounds",
                    view.name
                ));
            }
            let count = projection
                .visible
                .iter()
                .chain(&projection.hidden)
                .chain(&projection.section)
                .map(|line| line.points.len())
                .sum::<usize>();
            points = points
                .saturating_add(count)
                .saturating_add(projection.anchors.len())
                .saturating_add(projection.circles.len());
            metadata = metadata.saturating_add(
                projection
                    .topology_signatures
                    .iter()
                    .map(|(a, b)| a.len().saturating_add(b.len()))
                    .sum::<usize>(),
            );
            metadata = metadata.saturating_add(
                projection
                    .anchors
                    .iter()
                    .map(|a| a.edge_key.len())
                    .sum::<usize>(),
            );
            metadata = metadata.saturating_add(
                projection
                    .circles
                    .iter()
                    .map(|a| a.edge_key.len())
                    .sum::<usize>(),
            );
            for lines in [&projection.visible, &projection.hidden, &projection.section] {
                retained = retained.saturating_add(
                    lines.capacity() * std::mem::size_of::<limo_cad_occt::DrawingPolylineDto>(),
                );
                for line in lines {
                    retained = retained
                        .saturating_add(line.points.capacity() * std::mem::size_of::<[f64; 2]>());
                }
            }
            retained = retained.saturating_add(
                projection.anchors.capacity()
                    * std::mem::size_of::<limo_cad_occt::DrawingProjectionAnchorDto>(),
            );
            retained = retained.saturating_add(
                projection.circles.capacity()
                    * std::mem::size_of::<limo_cad_occt::DrawingProjectedCircleDto>(),
            );
            retained = retained.saturating_add(
                projection
                    .topology_signatures
                    .iter()
                    .map(|(a, b)| {
                        128usize
                            .saturating_add(a.capacity())
                            .saturating_add(b.capacity())
                    })
                    .sum::<usize>(),
            );
            retained = retained.saturating_add(
                projection
                    .anchors
                    .iter()
                    .map(|a| a.edge_key.capacity())
                    .sum::<usize>(),
            );
            retained = retained.saturating_add(
                projection
                    .circles
                    .iter()
                    .map(|a| a.edge_key.capacity())
                    .sum::<usize>(),
            );
            if points > limits.points
                || metadata > limits.metadata_bytes
                || retained > limits.retained_bytes
            {
                return Err(format!(
                    "Drawing projection exceeds the retained geometry budget ({} points, {} metadata bytes, {} retained bytes); simplify the sheet's views",
                    limits.points, limits.metadata_bytes, limits.retained_bytes
                ));
            }
            for (lines, style, _) in paths(view, &projection, &key) {
                let interval = style.dash_mm.iter().copied().reduce(f64::min);
                if !style.width_mm.is_finite()
                    || style.width_mm <= 0.
                    || style.dash_mm.iter().any(|n| !n.is_finite() || *n <= 0.)
                {
                    return Err("Drawing edge style has invalid stroke or dash lengths".into());
                }
                for line in lines {
                    if line.points.iter().flatten().any(|v| !v.is_finite()) {
                        return Err(format!(
                            "Drawing view '{}' has non-finite projected edges",
                            view.name
                        ));
                    }
                    for pair in line.points.windows(2) {
                        let distance =
                            (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]) * view.scale;
                        steps += 1. + interval.map_or(0., |dash| (distance / dash).ceil());
                        if !steps.is_finite() || steps > limits.stroke_steps {
                            return Err(format!(
                                "Drawing edge detail exceeds the {} stroke-step rendering budget; simplify the sheet's views or dash pattern",
                                limits.stroke_steps
                            ));
                        }
                    }
                }
            }
            if projections
                .insert(view.id, (view.clone(), projection))
                .is_some()
            {
                return Err("Drawing sheet contains duplicate view identities".into());
            }
        }
        let (hatches, view_art, source_marks, source_labels, retained_bytes) =
            presentation::build(&key, &projections, sources, limits, points, retained, steps)?;
        Ok(Self {
            key,
            projections,
            bases,
            hatches,
            view_art,
            source_marks,
            source_labels,
            retained_bytes,
        })
    }
    #[cfg(test)]
    fn rasterize(&self, key: RasterKey, region: RasterRegion) -> Result<Image, String> {
        self.rasterize_with_limits(key, region, Limits::default())
    }
    fn rasterize_with_limits(
        &self,
        key: RasterKey,
        region: RasterRegion,
        limits: Limits,
    ) -> Result<Image, String> {
        let mut mask_pixels = 0u64;
        for art in &self.view_art {
            mask_pixels = mask_pixels.checked_add(art.decoration.mask_pixels(key,region))
                .filter(|pixels| *pixels<=limits.mask_pixels)
                .ok_or("Drawing detail clips exceed the raster work limit; zoom into fewer detail boundaries")?;
        }
        for label in &self.source_labels {
            if [
                label.x,
                label.y,
                label.width_mm,
                label.height_mm,
                label.text_height_mm,
            ]
            .iter()
            .any(|value| !(value * key.paper_scale * key.render_scale).is_finite())
            {
                return Err(
                    "Drawing section label cannot render at this paper scale or DPI".into(),
                );
            }
        }
        let [width, height] = region.dimensions;
        let mut pixmap =
            Pixmap::new(width, height).ok_or("Unable to allocate drawing paper image")?;
        let factor = f64::from(key.paper_scale) * f64::from(key.render_scale);
        for (view_key, art) in self.key.layout.views.iter().zip(&self.view_art) {
            if !art.decoration.visible(key, region) {
                continue;
            }
            let (view, projection) = &self.projections[&view_key.id];
            presentation::draw(&mut pixmap, &self.hatches[art.hatches.clone()], key, region)?;
            let mask = art.decoration.mask(key, region)?;
            for (lines, style, hidden) in paths(view, projection, &self.key) {
                let mut builder = PathBuilder::new();
                let mut paths = 0;
                for line in lines {
                    if line.points.len() < 2 {
                        continue;
                    }
                    for (index, point) in line.points.iter().enumerate() {
                        let paper = paper_point(view, *point, projection);
                        let [x, y] = [
                            (paper[0] - region.origin_mm[0]) * factor,
                            (paper[1] - region.origin_mm[1]) * factor,
                        ]
                        .map(|v| v as f32);
                        if !x.is_finite() || !y.is_finite() {
                            return Err(format!(
                                "Drawing view '{}' lies outside finite render coordinates",
                                view.name
                            ));
                        }
                        if index == 0 {
                            builder.move_to(x, y);
                        } else {
                            builder.line_to(x, y);
                        }
                    }
                    paths += 1;
                }
                if paths == 0 {
                    continue;
                }
                let path = builder
                    .finish()
                    .ok_or("Unable to build drawing edge paths")?;
                let mut paint = Paint::default();
                if hidden {
                    paint.set_color_rgba8(132, 138, 146, 255);
                } else {
                    paint.set_color_rgba8(36, 40, 45, 255);
                }
                paint.anti_alias = true;
                let mut stroke = Stroke {
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    width: raster_stroke_width(
                        style.width_mm as f32,
                        key.paper_scale,
                        key.render_scale,
                    ) * key.render_scale,
                    ..Default::default()
                };
                if !style.dash_mm.is_empty() {
                    let mut pattern = style
                        .dash_mm
                        .iter()
                        .map(|v| (*v * factor) as f32)
                        .collect::<Vec<_>>();
                    if pattern.len() % 2 != 0 {
                        pattern.extend_from_within(..);
                    }
                    stroke.dash = Some(
                        StrokeDash::new(pattern, 0.)
                            .ok_or("Drawing dash pattern cannot be rendered at this scale")?,
                    );
                }
                pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), mask.as_ref());
            }
            art.decoration.draw(&mut pixmap, &self.key, key, region)?;
        }
        presentation::draw(&mut pixmap, &self.source_marks, key, region)?;
        for pixel in pixmap.data_mut().as_chunks_mut::<4>().0 {
            let alpha = u32::from(pixel[3]);
            if alpha != 0 && alpha != 255 {
                for channel in &mut pixel[..3] {
                    *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
                }
            }
        }
        Ok(Image::new(
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            pixmap.take(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        ))
    }
}

/// Section and removed-section derivations render cut loops; removed sections
/// omit the ordinary projection. Complete projections remain available to anchors.
fn paths<'a>(
    view: &DrawingViewDto,
    projection: &'a DrawingProjectionDto,
    key: &'a SourceKey,
) -> impl Iterator<
    Item = (
        &'a Vec<limo_cad_occt::DrawingPolylineDto>,
        &'a DrawingLineStyleDto,
        bool,
    ),
> {
    let removed = matches!(
        view.derivation,
        Some(DrawingViewDerivationDto::RemovedSection { .. })
    );
    let section = removed
        || matches!(
            view.derivation,
            Some(DrawingViewDerivationDto::Section { .. })
        );
    [
        (&projection.visible, &key.layout.visible, false, !removed),
        (
            &projection.hidden,
            &key.layout.hidden,
            true,
            !removed && view.show_hidden_lines,
        ),
        (&projection.section, &key.layout.visible, false, section),
    ]
    .into_iter()
    .filter(|(_, _, _, enabled)| *enabled)
    .map(|(lines, style, hidden, _)| (lines, style, hidden))
}

#[cfg(test)]
#[path = "drawing_edges/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "drawing_edges/section_tests.rs"]
mod section_tests;

#[cfg(test)]
#[path = "drawing_edges/derived_tests.rs"]
mod derived_tests;
