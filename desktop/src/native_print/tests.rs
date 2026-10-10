use super::Page;

#[test]
fn prepared_print_preserves_physical_paper_size_and_safe_job_title() {
    let page = Page::prepare(
        "Drawing\0\n revision 2".into(),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="297mm" height="210mm" viewBox="0 0 297 210"><path d="M10 10 L100 10" stroke="black"/></svg>"#,
    )
    .unwrap();
    assert!((page.size_mm[0] - 297.).abs() < 0.001);
    assert!((page.size_mm[1] - 210.).abs() < 0.001);
    assert_eq!(page.title, "Drawing revision 2");
    assert!(page.pdf.starts_with(b"%PDF-"));
}

#[test]
fn prepared_print_writes_a_pdf_without_opening_a_dialog() {
    let page = Page::prepare(
        "Sheet".into(),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="210mm" height="297mm" viewBox="0 0 210 297"><path d="M0 0 L10 0" stroke="black"/></svg>"#,
    )
    .unwrap();
    let title = page.title.clone();
    let size = page.size_mm;
    let path = super::write_retained_pdf(&page).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(path.ends_with("sheet.pdf"));
    assert!(path
        .parent()
        .and_then(|parent| parent.file_name())
        .is_some_and(|name| name.to_string_lossy().starts_with("Limo-CAD-print-")));
    assert!(bytes.starts_with(b"%PDF-"));
    assert_eq!(bytes, page.pdf);
    assert_eq!(page.title, title);
    assert_eq!(page.size_mm, size);
    let directory = path.parent().unwrap().to_path_buf();
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(&directory).unwrap();
}

#[test]
fn prepared_print_rejects_invalid_and_out_of_range_paper_without_a_dialog() {
    assert!(Page::prepare("Test".into(), "not SVG").is_err());
    for size in ["0.1mm", "2100mm"] {
        let svg =
            format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}"/>"#);
        assert!(Page::prepare("Test".into(), &svg).is_err());
    }
}
