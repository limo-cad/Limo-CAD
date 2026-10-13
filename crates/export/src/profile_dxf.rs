//! Exact manufacturing-region DXF in local sketch-plane millimetres.
//! Consumes the shared profile catalog; sheet scale, placement and units never
//! transform machining coordinates. Native UI and MCP exports share this contract.
use limo_cad_solid::{Point2Dto as P, ProfileCatalogItemDto, ProfileCurveDto, ProfileLoopDto};
use std::fmt::Write;

const MAX_POINTS: usize = 1_000_000;
const MODEL_SPACE: u32 = 0x27;

pub fn write_profile_dxf(catalog: &ProfileCatalogItemDto, index: u32) -> Result<String, String> {
    if catalog.profiles.len() > 65_536 {
        return Err("Too many profile loops to export".into());
    }
    let outer = catalog
        .profiles
        .iter()
        .find(|p| p.index == index)
        .ok_or("The selected sketch profile no longer exists")?;
    let mut indices = std::collections::BTreeSet::new();
    if catalog.profiles.iter().any(|p| !indices.insert(p.index)) {
        return Err("Profile indices are ambiguous".into());
    }
    if outer.nesting_depth % 2 != 0 {
        return Err("Choose a material region, not a hole wire".into());
    }
    let mut writer = Writer::default();
    writer.add_loop(outer, "PROFILE_OUTER")?;
    for hole in catalog
        .profiles
        .iter()
        .filter(|p| p.parent_index == Some(index))
    {
        if hole.nesting_depth != outer.nesting_depth + 1 {
            return Err("The profile nesting is inconsistent".into());
        }
        writer.add_loop(hole, "PROFILE_HOLES")?;
    }
    writer.finish()
}

struct Writer {
    entities: String,
    next: u32,
    points: usize,
    bounds: [f64; 4],
}
impl Default for Writer {
    fn default() -> Self {
        Self {
            entities: String::new(),
            next: 0x100,
            points: 0,
            bounds: [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ],
        }
    }
}
fn distance(a: P, b: P) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}
impl Writer {
    fn point(&mut self, p: P) -> Result<(), String> {
        self.points += 1;
        if self.points > MAX_POINTS || !p.x.is_finite() || !p.y.is_finite() {
            return Err("Profile geometry is non-finite or exceeds the export limit".into());
        }
        self.bounds[0] = self.bounds[0].min(p.x);
        self.bounds[1] = self.bounds[1].min(p.y);
        self.bounds[2] = self.bounds[2].max(p.x);
        self.bounds[3] = self.bounds[3].max(p.y);
        Ok(())
    }
    fn begin(&mut self, kind: &str, layer: &str, subclass: &str) {
        writeln!(self.entities, "0\n{kind}\n5\n{:X}\n330\n{MODEL_SPACE:X}\n100\nAcDbEntity\n8\n{layer}\n100\n{subclass}", self.next).unwrap();
        self.next += 1;
    }
    fn coordinates(&mut self, code: u32, p: P) {
        writeln!(
            self.entities,
            "{code}\n{}\n{}\n{}\n{}\n0",
            p.x,
            code + 10,
            p.y,
            code + 20
        )
        .unwrap();
    }
    fn polyline(&mut self, points: &[P], closed: bool, layer: &str) -> Result<(), String> {
        let points = if closed
            && points.len() > 1
            && distance(points[0], points[points.len() - 1]) <= 1e-6
        {
            &points[..points.len() - 1]
        } else {
            points
        };
        if points.len() < if closed { 3 } else { 2 } {
            return Err("Profile contains a degenerate wire".into());
        }
        for p in points {
            self.point(*p)?;
        }
        self.begin("LWPOLYLINE", layer, "AcDbPolyline");
        writeln!(
            self.entities,
            "90\n{}\n70\n{}\n43\n0",
            points.len(),
            u8::from(closed)
        )
        .unwrap();
        for p in points {
            writeln!(self.entities, "10\n{}\n20\n{}", p.x, p.y).unwrap();
        }
        Ok(())
    }
    fn circle(
        &mut self,
        center: P,
        radius: f64,
        layer: &str,
        arc: Option<(f64, f64)>,
    ) -> Result<(), String> {
        if !radius.is_finite() || radius <= 0. {
            return Err("Profile circle has an invalid radius".into());
        }
        self.point(center)?;
        self.point(P::new(center.x - radius, center.y - radius))?;
        self.point(P::new(center.x + radius, center.y + radius))?;
        self.begin(
            if arc.is_some() { "ARC" } else { "CIRCLE" },
            layer,
            "AcDbCircle",
        );
        self.coordinates(10, center);
        writeln!(self.entities, "40\n{radius}").unwrap();
        if let Some((start, end)) = arc {
            writeln!(self.entities, "100\nAcDbArc\n50\n{start}\n51\n{end}").unwrap();
        }
        Ok(())
    }
    fn add_loop(&mut self, profile: &ProfileLoopDto, layer: &str) -> Result<(), String> {
        if !profile.area.is_finite() || profile.area.abs() <= 1e-12 {
            return Err("Profile has no finite material area".into());
        }
        if profile.curves.is_empty() {
            return self.polyline(&profile.points, true, layer);
        }
        if profile.curves.len() > MAX_POINTS {
            return Err("Too many profile curves to export".into());
        }
        let mut ends = Vec::new();
        for curve in &profile.curves {
            match curve {
                ProfileCurveDto::Line { start, end, .. }
                | ProfileCurveDto::Arc { start, end, .. } => ends.push((*start, *end)),
                ProfileCurveDto::Polyline { points, .. } if points.len() >= 2 => {
                    ends.push((points[0], points[points.len() - 1]))
                }
                ProfileCurveDto::Circle { .. } if profile.curves.len() == 1 => {}
                _ => return Err("Profile contains an incomplete boundary curve".into()),
            }
        }
        for index in 0..ends.len() {
            if distance(ends[index].1, ends[(index + 1) % ends.len()].0) > 1e-6 {
                return Err("Profile boundary curves do not form a closed wire".into());
            }
        }
        for curve in &profile.curves {
            match curve {
                ProfileCurveDto::Line { start, end, .. } => {
                    self.point(*start)?;
                    self.point(*end)?;
                    if distance(*start, *end) <= 1e-12 {
                        return Err("Profile contains a zero-length edge".into());
                    }
                    self.begin("LINE", layer, "AcDbLine");
                    self.coordinates(10, *start);
                    self.coordinates(11, *end);
                }
                ProfileCurveDto::Circle { center, radius, .. } => {
                    self.circle(*center, *radius, layer, None)?
                }
                ProfileCurveDto::Polyline { points, .. } => self.polyline(points, false, layer)?,
                ProfileCurveDto::Arc {
                    start, mid, end, ..
                } => {
                    self.point(*start)?;
                    self.point(*mid)?;
                    self.point(*end)?;
                    if let Some((center, radius, a, b)) = arc(*start, *mid, *end)? {
                        self.circle(center, radius, layer, Some((a, b)))?;
                    } else {
                        self.polyline(&[*start, *mid, *end], false, layer)?;
                    }
                }
            }
        }
        Ok(())
    }
    fn finish(self) -> Result<String, String> {
        if self.entities.is_empty() {
            return Err("Profile has no boundary geometry".into());
        }
        let [minx, miny, maxx, maxy] = self.bounds;
        let mut text = format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1027\n9\n$INSUNITS\n70\n4\n9\n$MEASUREMENT\n70\n1\n9\n$HANDSEED\n5\n{:X}\n9\n$EXTMIN\n10\n{minx}\n20\n{miny}\n30\n0\n9\n$EXTMAX\n10\n{maxx}\n20\n{maxy}\n30\n0\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n",self.next);
        text.push_str("0\nTABLE\n2\nLTYPE\n5\n2\n330\n0\n100\nAcDbSymbolTable\n70\n1\n0\nLTYPE\n5\n3\n330\n2\n100\nAcDbSymbolTableRecord\n100\nAcDbLinetypeTableRecord\n2\nCONTINUOUS\n70\n0\n3\nSolid line\n72\n65\n73\n0\n40\n0\n0\nENDTAB\n0\nTABLE\n2\nLAYER\n5\n4\n330\n0\n100\nAcDbSymbolTable\n70\n3\n");
        for (i, layer) in ["0", "PROFILE_OUTER", "PROFILE_HOLES"]
            .into_iter()
            .enumerate()
        {
            writeln!(text,"0\nLAYER\n5\n{:X}\n330\n4\n100\nAcDbSymbolTableRecord\n100\nAcDbLayerTableRecord\n2\n{layer}\n70\n0\n62\n{}\n6\nCONTINUOUS",i+5,if layer=="PROFILE_HOLES" {4} else {7}).unwrap();
        }
        text.push_str("0\nENDTAB\n0\nTABLE\n2\nBLOCK_RECORD\n5\n26\n330\n0\n100\nAcDbSymbolTable\n70\n1\n0\nBLOCK_RECORD\n5\n27\n330\n26\n100\nAcDbSymbolTableRecord\n100\nAcDbBlockTableRecord\n2\n*Model_Space\n70\n0\n0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n5\n29\n330\n27\n100\nAcDbEntity\n8\n0\n100\nAcDbBlockBegin\n2\n*Model_Space\n70\n0\n10\n0\n20\n0\n30\n0\n3\n*Model_Space\n1\n\n0\nENDBLK\n5\n2A\n330\n27\n100\nAcDbEntity\n8\n0\n100\nAcDbBlockEnd\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n");
        text.push_str(&self.entities);
        text.push_str("0\nENDSEC\n0\nEOF\n");
        Ok(text.replace('\n', "\r\n"))
    }
}

fn arc(start: P, mid: P, end: P) -> Result<Option<(P, f64, f64, f64)>, String> {
    let (mx, my, ex, ey) = (
        mid.x - start.x,
        mid.y - start.y,
        end.x - start.x,
        end.y - start.y,
    );
    let d = 2. * (mx * ey - my * ex);
    if !d.is_finite() {
        return Err("Profile arc exceeds finite numeric range".into());
    }
    if d.abs() < 1e-10 {
        return Ok(None);
    }
    let (m, e) = (mx * mx + my * my, ex * ex + ey * ey);
    let center = P::new(
        start.x + (m * ey - e * my) / d,
        start.y + (mx * e - ex * m) / d,
    );
    let angle = |p: P| {
        (p.y - center.y)
            .atan2(p.x - center.x)
            .to_degrees()
            .rem_euclid(360.)
    };
    let radius = distance(center, start);
    let (start, mid, end) = (angle(start), angle(mid), angle(end));
    let (a, b) = if (mid - start).rem_euclid(360.) <= (end - start).rem_euclid(360.) + 1e-7 {
        (start, end)
    } else {
        (end, start)
    };
    Ok(Some((center, radius, a, b)))
}

#[cfg(test)]
mod tests;
