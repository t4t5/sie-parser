use rust_decimal::Decimal;
use sie_parser::{
    Account, Company, FiscalYear, Header, PeriodBalance, Severity, SieDocument, Voucher,
    VoucherRow, VoucherRowKind, YearMonth, document, encode_document, parse, render_document,
};
use std::str::FromStr;

fn document() -> SieDocument {
    let mut document = SieDocument {
        header: Header {
            program_name: "Testgenerator".into(),
            program_version: "2".into(),
            generation_date: Some("20260729".parse().unwrap()),
            ..Header::default()
        },
        company: Company {
            name: "Å Ä Ö å ä ö AB".into(),
            orgnr_raw: "5591741383".into(),
            contact: "Eva \"E\"".into(),
            distribution_address: r"C:\Kontor".into(),
            postal_address: "123 45 STAD".into(),
            telephone: String::new(),
            ..Company::default()
        },
        years: vec![FiscalYear {
            idx: 0,
            start: "20260101".parse().unwrap(),
            end: "20261231".parse().unwrap(),
        }],
        ..SieDocument::default()
    };
    // Deliberately insert in reverse order; BTreeMap-backed records render
    // canonically by account number.
    document
        .accounts
        .insert(3000, Account::new(3000, "Försäljning"));
    document
        .accounts
        .insert(1930, Account::new(1930, "Företagskonto"));
    document
        .dimensions
        .insert(1, sie_parser::Dimension::new(1, "Resultatenhet"));
    document.period_balances.push(PeriodBalance {
        year: 0,
        period: "202601".parse().unwrap(),
        account: 3000,
        objects: vec![(1, "Norr".into())],
        amount: Decimal::from_str("-123.40").unwrap(),
        quantity: Decimal::ZERO,
    });
    document.vouchers.push(Voucher {
        series: "B".into(),
        number: 2,
        date: "20260131".parse().unwrap(),
        text: String::new(),
        registration_date: None,
        rows: vec![],
    });
    document.vouchers.push(Voucher {
        series: "A".into(),
        number: 1,
        date: "20260130".parse().unwrap(),
        text: "Rättelse".into(),
        registration_date: Some("20260201".parse().unwrap()),
        rows: vec![
            VoucherRow {
                kind: VoucherRowKind::Removed,
                account: 1930,
                objects: vec![],
                amount: Decimal::from_str("-10").unwrap(),
                transaction_date: None,
                text: String::new(),
                quantity: Decimal::ZERO,
            },
            VoucherRow {
                kind: VoucherRowKind::Added,
                account: 3000,
                objects: vec![(1, "Norr".into())],
                amount: Decimal::from_str("10").unwrap(),
                transaction_date: Some("20260129".parse().unwrap()),
                text: "Ny rad".into(),
                quantity: Decimal::from_str("2.5").unwrap(),
            },
            VoucherRow {
                kind: VoucherRowKind::Transaction,
                account: 1930,
                objects: vec![],
                amount: Decimal::ZERO,
                transaction_date: None,
                text: String::new(),
                quantity: Decimal::ZERO,
            },
        ],
    });
    document
}

#[test]
fn public_writer_covers_ordering_objects_periods_and_vouchers() {
    let document = document();
    let rendered = render_document(&document).unwrap();

    assert!(rendered.contains("#ADRESS \"Eva \\\"E\\\"\" \"C:\\\\Kontor\" \"123 45 STAD\" \"\""));
    assert!(rendered.contains("#PSALDO 0 202601 3000 {1 \"Norr\"} -123.4"));
    assert!(rendered.contains("#VER \"B\" 2 20260131\r\n{\r\n}"));
    assert!(rendered.contains("#RTRANS 3000 {1 \"Norr\"} 10 20260129 \"Ny rad\" 2.5"));
    assert!(rendered.find("#KONTO 1930").unwrap() < rendered.find("#KONTO 3000").unwrap());
    assert!(rendered.find("#VER \"A\"").unwrap() < rendered.find("#VER \"B\"").unwrap());

    let output = parse(&rendered);
    assert!(
        output
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != Severity::Error)
    );
    let canonical = document::read(&rendered).unwrap();
    let rendered_again = render_document(&canonical).unwrap();
    assert_eq!(document::read(&rendered_again).unwrap(), canonical);
}

#[test]
fn public_encoder_uses_cp437_for_swedish_letters() {
    let encoded = encode_document(&document()).unwrap();
    assert!(
        encoded
            .windows(6)
            .any(|window| { window == [0x8f, b' ', 0x8e, b' ', 0x99, b' '] })
    );
}

#[test]
fn public_validation_rejects_bad_orgnr_and_excess_precision() {
    let mut document = document();
    document.company.orgnr_raw = "not-an-orgnr".into();
    assert!(
        render_document(&document)
            .unwrap_err()
            .to_string()
            .contains("#ORGNR")
    );

    document.company.orgnr_raw = "5591741383".into();
    document.period_balances[0].amount = Decimal::from_str("1.001").unwrap();
    assert!(
        render_document(&document)
            .unwrap_err()
            .to_string()
            .contains("fractional")
    );
}

#[test]
fn compact_source_populates_new_typed_records() {
    let source = concat!(
        "#FLAGGA 0\r\n",
        "#PROGRAM \"Test\" \"1\"\r\n",
        "#FORMAT PC8\r\n",
        "#GEN 20260729 \"AB\"\r\n",
        "#SIETYP 4\r\n",
        "#ORGNR 559174-1383 1 2\r\n",
        "#FNAMN \"Bolag\"\r\n",
        "#RAR 0 20260101 20261231\r\n",
        "#KPTYP EUBAS97\r\n",
        "#VALUTA SEK\r\n",
        "#KONTO 1930 \"Bank\"\r\n",
        "#KTYP 1930 T\r\n",
        "#ENHET 1930 \"st\"\r\n",
        "#DIM 1 \"Resultatenhet\"\r\n",
        "#OBJEKT 1 \"N\" \"Norr\"\r\n",
        "#PSALDO 0 202601 1930 {1 \"N\"} 12.5 2\r\n",
        "#VER A 1 20260131 \"\" 20260201\r\n",
        "{\r\n",
        "#BTRANS 1930 {} -12.5 \"\" \"Borttagen\" 2\r\n",
        "}\r\n",
    );
    let document = document::read(source).unwrap();

    assert_eq!(document.header.signature.as_deref(), Some("AB"));
    assert_eq!(document.company.acquisition_number.as_deref(), Some("1"));
    assert_eq!(document.accounts[&1930].account_type.as_deref(), Some("T"));
    assert_eq!(
        document.period_balances[0].period,
        YearMonth::new(2026, 1).unwrap()
    );
    assert_eq!(document.period_balances[0].objects, vec![(1, "N".into())]);
    assert_eq!(document.vouchers[0].registration_date.unwrap().day(), 1);
    assert_eq!(document.vouchers[0].rows[0].kind, VoucherRowKind::Removed);
    assert_eq!(document.vouchers[0].rows[0].text, "Borttagen");
}
