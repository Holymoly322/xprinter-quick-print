static PDFIUM_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[test]
#[ignore = "Requires packaged pdfium.dll next to test executable; no printer access"]
fn pdfium_runtime() {
    let _guard = PDFIUM_TEST_LOCK.lock().unwrap();
    let renderer = xprinter_quick_print::pdf::Renderer::new().unwrap();
    let page = renderer
        .page(std::path::Path::new("tests/fixtures/label.pdf"), 0, 1200)
        .unwrap();
    assert_eq!(page.count, 1);
    assert!((page.size[0] - 100.).abs() < 0.01);
    assert!((page.size[1] - 150.).abs() < 0.01);
    let label = xprinter_quick_print::pdf::compose(&page, &Default::default(), 203.).unwrap();
    assert!(label.pixels().any(|p| p[0] < 128));
    label.save("runtime-smoke-preview.png").unwrap();
}

#[test]
#[ignore = "Explicit virtual Microsoft Print to PDF test only; no physical printer"]
fn virtual_pdf_backend() {
    let _guard = PDFIUM_TEST_LOCK.lock().unwrap();
    let input = std::path::Path::new("tests/fixtures/label.pdf");
    let output = std::env::current_dir()
        .unwrap()
        .join("virtual-print-result.pdf");
    assert!(
        !output.exists(),
        "Remove previous diagnostic output deliberately before rerunning"
    );
    let preset = xprinter_quick_print::layout::Preset {
        width: 210.,
        height: 297.,
        margins: [5.; 4],
        ..Default::default()
    };
    let id = xprinter_quick_print::printing::print_to_pdf(input, &preset, 2, &output).unwrap();
    assert!(id > 0);
    for _ in 0..100 {
        if output.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let renderer = xprinter_quick_print::pdf::Renderer::new().unwrap();
    let page = renderer.page(&output, 0, 800).unwrap();
    assert_eq!(page.count, 2, "Exactly two copies must be generated");
    assert!(page.pixels.pixels().any(|p| p[0] < 128));
}
