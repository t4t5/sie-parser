//! Integration test: parse the real SIE4 export from Visma Administration 2000
//! and assert it produces no Error-severity diagnostics.

use sie_parser::{Severity, decode_cp437, document, parse, render_document};

#[test]
fn sample_parses_without_errors() {
    let bytes =
        std::fs::read("tests/fixtures/sample.se").expect("tests/fixtures/sample.se must exist");
    let text = decode_cp437(&bytes);
    let out = parse(&text);
    let errors: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error-severity diagnostics, got: {errors:#?}"
    );
    assert!(
        out.items.len() > 2000,
        "expected a large number of items from the 4k-line sample, got {}",
        out.items.len()
    );
    assert_eq!(
        out.items[0].label, "#FLAGGA",
        "first item should be #FLAGGA"
    );
}

#[test]
fn sample_company_name_decoded_from_cp437() {
    let bytes = std::fs::read("tests/fixtures/sample.se").unwrap();
    let text = decode_cp437(&bytes);
    let out = parse(&text);
    let fnamn = out
        .items
        .iter()
        .find(|it| it.label == "#FNAMN")
        .expect("sample has #FNAMN");
    // Expect the Swedish "Övningsbolaget AB" round-tripped from CP437.
    let first_field = &fnamn.fields[0];
    let text = match &first_field.value {
        sie_parser::FieldValue::Quoted { text } => text.as_str(),
        sie_parser::FieldValue::Bare { text } => text.as_str(),
        _ => panic!("unexpected field value"),
    };
    assert_eq!(text, "Övningsbolaget AB");
}

#[test]
fn sample_populates_typed_records_and_round_trips_semantically() {
    let bytes = std::fs::read("tests/fixtures/sample.se").unwrap();
    let text = decode_cp437(&bytes);
    let document = document::read(&text).expect("read typed sample document");

    assert_eq!(document.header.program_version, "2022.2");
    assert_eq!(
        document.company.file_number.as_deref(),
        Some(r"C:\ProgramData\SPCS\SPCS Administration\F÷retag\Ovnbol2000")
    );
    assert_eq!(document.company.postal_address, "123 45 STORSTAD");
    assert_eq!(document.company.postnr.as_deref(), Some("12345"));
    assert_eq!(document.dimensions.len(), 2);
    assert!(!document.objects.is_empty());
    assert!(!document.vouchers.is_empty());
    assert!(!document.vouchers[0].rows.is_empty());

    let rendered = render_document(&document).expect("render typed sample document");
    let parsed = parse(&rendered);
    let errors: Vec<_> = parsed
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "generated output should parse without errors: {errors:#?}"
    );

    let reparsed = document::read(&rendered).expect("read rendered sample document");
    assert_eq!(reparsed, document);
}
