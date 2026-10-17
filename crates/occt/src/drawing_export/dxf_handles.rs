//! One deterministic handle namespace for graphical entities and their blocks.
//! R2007 entity ownership follows Autodesk's Common Group Codes for Entities:
//! https://help.autodesk.com/cloudhelp/2023/ENU/AutoCAD-DXF/files/GUID-3610039E-27D1-4E23-B6D3-7E60B22BB5BD.htm
use std::fmt::Write;

pub(super) struct Handles {
    next: usize,
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn graphical_entities_and_unicode_outlines_have_resolvable_unique_owners() {
        let mut paper = Paper {
            dimensions: Vec::new(),
            size: [297., 210.],
            items: vec![],
        };
        paper.line(
            vec![[1., 2.], [3., 4.]],
            "VISIBLE",
            &DrawingLineStyleDto {
                width_mm: 0.25,
                dash_mm: vec![],
            },
        );
        paper.line(
            vec![[1., 2.], [3., 4.], [5., 6.]],
            "HIDDEN",
            &DrawingLineStyleDto {
                width_mm: 0.25,
                dash_mm: vec![2., 1.],
            },
        );
        paper.text([10., 20.], "Plain", 3.);
        paper.text([10., 30.], "O\u{2300}", 3.);
        paper.items.push(Primitive::Triangle {
            points: [[1., 1.], [2., 1.], [2., 2.]],
            layer: "DIMENSION",
        });
        let output = dxf(&paper, "Arial").unwrap();
        let lines: Vec<_> = output.lines().collect();
        assert_eq!(lines.len() % 2, 0);
        let mut records: Vec<Vec<(&str, &str)>> = vec![];
        for pair in lines.chunks_exact(2) {
            if pair[0] == "0" {
                records.push(vec![]);
            }
            records.last_mut().unwrap().push((pair[0], pair[1]));
        }
        let field = |record: &Vec<(&str, &str)>, code| {
            record.iter().find(|p| p.0 == code).map(|p| p.1.to_owned())
        };
        let mut handles = BTreeSet::new();
        let mut blocks = BTreeMap::new();
        for record in &records {
            if matches!(record[0].1, "SECTION" | "ENDSEC" | "ENDTAB" | "EOF") {
                continue;
            }
            let handle = field(record, "5").expect("Every object has a handle");
            assert!(handles.insert(handle.clone()), "Duplicate handle {handle}");
            if record[0].1 == "BLOCK_RECORD" {
                blocks.insert(field(record, "2").unwrap(), handle);
            }
        }
        assert_eq!(blocks.len(), 2);
        let model = &blocks["*Model_Space"];
        let mut kinds = BTreeSet::new();
        for record in &records {
            let kind = record[0].1;
            if matches!(kind, "LINE" | "LWPOLYLINE" | "TEXT" | "SOLID" | "HATCH") {
                kinds.insert(kind);
                assert_eq!(field(record, "330").as_ref(), Some(model));
                assert!(record.contains(&("100", "AcDbEntity")));
                if kind == "TEXT" {
                    assert_eq!(
                        record.iter().filter(|p| **p == ("100", "AcDbText")).count(),
                        2
                    );
                    if record.contains(&("60", "1")) {
                        let visibility = record.iter().position(|p| *p == ("60", "1")).unwrap();
                        let text = record
                            .iter()
                            .position(|p| *p == ("100", "AcDbText"))
                            .unwrap();
                        assert!(visibility < text, "Visibility belongs to AcDbEntity");
                    }
                }
            }
            if let Some(owner) = field(record, "330") {
                assert!(
                    owner == "0" || handles.contains(&owner),
                    "Unresolved owner {owner}"
                );
            }
        }
        assert_eq!(
            kinds,
            BTreeSet::from(["LINE", "LWPOLYLINE", "TEXT", "SOLID", "HATCH"])
        );
        for (name, handle) in blocks {
            let begins: Vec<_> = records
                .iter()
                .filter(|r| r[0].1 == "BLOCK" && field(r, "2").as_ref() == Some(&name))
                .collect();
            assert_eq!(begins.len(), 1);
            assert_eq!(field(begins[0], "330"), Some(handle.clone()));
            assert_eq!(
                records
                    .iter()
                    .filter(|r| r[0].1 == "ENDBLK" && field(r, "330").as_ref() == Some(&handle))
                    .count(),
                1
            );
        }
        let seed = output
            .split("$HANDSEED\n5\n")
            .nth(1)
            .unwrap()
            .lines()
            .next()
            .unwrap();
        let seed = usize::from_str_radix(seed, 16).unwrap();
        assert!(handles
            .iter()
            .all(|h| usize::from_str_radix(h, 16).unwrap() < seed));
        assert_eq!(output, dxf(&paper, "Arial").unwrap());
    }
}

impl Handles {
    pub(super) fn new(next: usize) -> Self {
        Self { next }
    }

    pub(super) fn next(&mut self) -> usize {
        let handle = self.next;
        self.next += 1;
        handle
    }

    pub(super) fn seed(&self) -> usize {
        self.next
    }

    pub(super) fn entity(&mut self, out: &mut String, kind: &str, owner: usize, layer: &str) {
        let handle = self.next();
        writeln!(
            out,
            "0\n{kind}\n5\n{handle:X}\n330\n{owner:X}\n100\nAcDbEntity\n8\n{layer}"
        )
        .unwrap();
    }

    pub(super) fn empty_space(&mut self, out: &mut String, name: &str, owner: usize) {
        self.entity(out, "BLOCK", owner, "0");
        writeln!(
            out,
            "100\nAcDbBlockBegin\n2\n{name}\n70\n0\n10\n0\n20\n0\n30\n0\n3\n{name}\n1\n"
        )
        .unwrap();
        self.entity(out, "ENDBLK", owner, "0");
        out.push_str("100\nAcDbBlockEnd\n");
    }
}
