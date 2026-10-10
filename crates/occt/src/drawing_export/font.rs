//! Record the sheet's primary font family without inventing a platform font
//! filename. Characters that family cannot draw are embedded separately as
//! outlines; the STYLE table still carries the family name.
use std::fmt::Write;

pub(super) fn family(list: &str) -> Result<String, String> {
    let first = list.split(',').next().unwrap_or_default().trim();
    let first = first
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| first.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(first)
        .trim();
    if first.is_empty() || first.len() > 255 || first.chars().any(char::is_control) {
        return Err(
            "Drawing DXF requires a primary font family of 1–255 bytes without control characters"
                .into(),
        );
    }
    Ok(first.into())
}

pub(super) fn tables(out: &mut String, handle: usize, family: &str) {
    let style = handle + 1;
    let app_table = handle + 2;
    let app = handle + 3;
    writeln!(out, "0\nTABLE\n2\nSTYLE\n5\n{handle:X}\n330\n0\n100\nAcDbSymbolTable\n70\n1\n0\nSTYLE\n5\n{style:X}\n330\n{handle:X}\n100\nAcDbSymbolTableRecord\n100\nAcDbTextStyleTableRecord\n2\nSTANDARD\n70\n0\n40\n0\n41\n1\n50\n0\n71\n0\n42\n2.5\n3\n\n4\n\n1001\nACAD\n1000\n{family}\n1071\n34\n0\nENDTAB\n0\nTABLE\n2\nAPPID\n5\n{app_table:X}\n330\n0\n100\nAcDbSymbolTable\n70\n1\n0\nAPPID\n5\n{app:X}\n330\n{app_table:X}\n100\nAcDbSymbolTableRecord\n100\nAcDbRegAppTableRecord\n2\nACAD\n70\n0\n0\nENDTAB").unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn primary_family_retains_unicode_and_rejects_record_injection() {
        assert_eq!(
            family(" 'Microsoft YaHei', Arial, sans-serif").unwrap(),
            "Microsoft YaHei"
        );
        assert_eq!(family("\"思源黑体\", sans-serif").unwrap(), "思源黑体");
        for value in ["", " ,Arial", "Arial\n0\nEOF", "Arial\0"] {
            assert!(family(value).is_err());
        }
        assert!(family(&"x".repeat(256)).is_err());
        assert!(family(&"x".repeat(255)).is_ok());
    }

    #[test]
    fn exported_text_references_the_saved_family_without_rewriting_the_document() {
        let (mut doc, scene, _) = super::super::tests::fixture(12.8);
        doc.sheets[0].views.clear();
        doc.sheets[0]
            .annotations
            .retain(|a| matches!(a, limo_cad_sketch::DrawingAnnotationDto::Note { .. }));
        doc.sheets[0].style.font_family = "Microsoft YaHei, Arial, sans-serif".into();
        let before = doc.clone();
        let output = super::super::export_sheet(
            &doc,
            &scene,
            &Default::default(),
            &super::super::DrawingExportRequest {
                sheet_id: 1,
                format: super::super::DrawingExportFormat::Dxf,
            },
            |_| Err("No views expected".into()),
        )
        .unwrap();
        assert!(output.contains("1001\nACAD\n1000\nMicrosoft YaHei\n1071\n34\n"));
        assert!(output.contains("7\nSTANDARD\n"));
        assert!(!output.contains("YaHei.ttf"));
        assert_eq!(doc, before);
    }
}
