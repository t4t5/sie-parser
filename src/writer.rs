//! Canonical SIE 4 rendering and CP437 encoding.
//!
//! Records are emitted in this order: identification metadata; company and
//! fiscal-year metadata; accounts (with their attributes); dimensions and
//! objects; IB, UB, PSALDO, and RES balances; then vouchers. Map-backed
//! records use key order, fiscal years use descending year index (current
//! first), period balances use `(year, period, account, objects)` order, and
//! vouchers use `(series, number)` order. Voucher rows retain vector order.

use crate::cp437::encode_cp437;
use crate::document::{AccountNo, DimensionNo, ObjectList, SieDocument, VoucherRowKind, YearIdx};
use anyhow::{Context, Result, anyhow, bail, ensure};
use rust_decimal::Decimal;
use std::collections::BTreeSet;
use std::path::Path;

/// Validate that a document can be rendered as canonical SIE 4.
pub fn validate(document: &SieDocument) -> Result<()> {
    ensure!(
        document.header.import_flag <= 1,
        "#FLAGGA must be 0 or 1, got {}",
        document.header.import_flag
    );
    ensure!(
        document.header.format.eq_ignore_ascii_case("PC8"),
        "unsupported #FORMAT {:?}; SIE 4 output requires PC8",
        document.header.format
    );
    ensure!(
        document.header.sie_type == 4,
        "unsupported #SIETYP {}; this writer produces SIE type 4",
        document.header.sie_type
    );
    ensure!(
        !document.header.program_name.is_empty(),
        "#PROGRAM program name must not be empty"
    );
    ensure!(
        !document.header.program_version.is_empty(),
        "#PROGRAM version must not be empty"
    );
    if document.header.signature.is_some() {
        ensure!(
            document.header.generation_date.is_some(),
            "#GEN signature requires a generation date"
        );
    }
    ensure!(
        matches!(
            document.header.chart_type.as_str(),
            "BAS95" | "BAS96" | "EUBAS97" | "NE2007"
        ),
        "unsupported #KPTYP {:?}",
        document.header.chart_type
    );
    ensure!(
        document.header.currency.len() == 3
            && document
                .header
                .currency
                .bytes()
                .all(|byte| byte.is_ascii_uppercase()),
        "#VALUTA must be a three-letter uppercase currency code, got {:?}",
        document.header.currency
    );

    ensure!(
        !document.company.name.is_empty(),
        "#FNAMN must not be empty"
    );
    validate_orgnr(&document.company.orgnr_raw)?;
    validate_all_text(document)?;

    let mut years = BTreeSet::new();
    for year in &document.years {
        ensure!(
            years.insert(year.idx),
            "duplicate #RAR year index {}",
            year.idx
        );
        ensure!(
            year.idx <= 0,
            "#RAR year index must be 0 or negative, got {}",
            year.idx
        );
        ensure!(
            year.start <= year.end,
            "#RAR {} starts after it ends ({} > {})",
            year.idx,
            year.start,
            year.end
        );
    }
    ensure!(
        years.contains(&0),
        "#RAR 0 (current fiscal year) is required"
    );

    for (&key, account) in &document.accounts {
        validate_account(key)?;
        ensure!(
            account.no == key,
            "account map key {key} does not match Account.no {}",
            account.no
        );
        if let Some(sru) = account.sru {
            ensure!(
                (1..=9999).contains(&sru),
                "#SRU for account {key} is out of range: {sru}"
            );
        }
        if let Some(account_type) = &account.account_type {
            ensure!(
                matches!(account_type.as_str(), "T" | "S" | "K" | "I"),
                "#KTYP for account {key} must be T, S, K, or I, got {account_type:?}"
            );
        }
    }

    for (&key, dimension) in &document.dimensions {
        validate_dimension(key)?;
        ensure!(
            dimension.no == key,
            "dimension map key {key} does not match Dimension.no {}",
            dimension.no
        );
        if let Some(parent) = dimension.super_dimension {
            ensure!(
                parent != key,
                "#UNDERDIM {key} cannot be its own super-dimension"
            );
            ensure!(
                document.dimensions.contains_key(&parent),
                "#UNDERDIM {key} references undeclared dimension {parent}"
            );
        }
    }
    for ((dimension, object_no), object) in &document.objects {
        ensure!(
            document.dimensions.contains_key(dimension),
            "#OBJEKT {:?} references undeclared dimension {dimension}",
            object.no
        );
        ensure!(
            object.dimension_no == *dimension && object.no == *object_no,
            "#OBJEKT map key ({dimension}, {object_no:?}) does not match its value"
        );
        ensure!(
            !object.no.is_empty(),
            "#OBJEKT object number must not be empty"
        );
    }

    validate_balance_map(document, "#IB", &document.ib, &years)?;
    validate_balance_map(document, "#UB", &document.ub, &years)?;
    validate_balance_map(document, "#RES", &document.res, &years)?;

    for balance in &document.period_balances {
        validate_reference(document, &years, balance.year, balance.account, "#PSALDO")?;
        validate_object_list(document, &balance.objects, "#PSALDO")?;
        format_decimal(balance.amount, "#PSALDO amount")?;
        format_decimal(balance.quantity, "#PSALDO quantity")?;
    }

    let mut voucher_keys = BTreeSet::new();
    for voucher in &document.vouchers {
        ensure!(
            !voucher.series.is_empty(),
            "#VER series must not be empty (voucher number {})",
            voucher.number
        );
        ensure!(
            voucher.number > 0,
            "#VER number must be greater than zero (series {:?})",
            voucher.series
        );
        ensure!(
            voucher_keys.insert((voucher.series.as_str(), voucher.number)),
            "duplicate #VER series/number ({:?}, {})",
            voucher.series,
            voucher.number
        );
        for (row_index, row) in voucher.rows.iter().enumerate() {
            ensure!(
                document.accounts.contains_key(&row.account),
                "#VER {:?} {} row {} references undeclared account {}",
                voucher.series,
                voucher.number,
                row_index + 1,
                row.account
            );
            validate_object_list(
                document,
                &row.objects,
                &format!(
                    "#VER {:?} {} row {}",
                    voucher.series,
                    voucher.number,
                    row_index + 1
                ),
            )?;
            format_decimal(
                row.amount,
                &format!(
                    "#VER {:?} {} row {} amount",
                    voucher.series,
                    voucher.number,
                    row_index + 1
                ),
            )?;
            format_decimal(
                row.quantity,
                &format!(
                    "#VER {:?} {} row {} quantity",
                    voucher.series,
                    voucher.number,
                    row_index + 1
                ),
            )?;
        }
    }
    Ok(())
}

/// Render a document as UTF-8 SIE text with canonical CRLF line endings.
///
/// The returned string has no trailing newline.
pub fn render(document: &SieDocument) -> Result<String> {
    validate(document)?;
    let mut lines = Vec::new();
    let header = &document.header;
    let company = &document.company;

    lines.push(format!("#FLAGGA {}", header.import_flag));
    lines.push(format!(
        "#PROGRAM {} {}",
        quote(&header.program_name, "#PROGRAM program name")?,
        quote(&header.program_version, "#PROGRAM version")?
    ));
    lines.push("#FORMAT PC8".to_string());
    if let Some(date) = header.generation_date {
        let mut line = format!("#GEN {date}");
        if let Some(signature) = &header.signature {
            line.push(' ');
            line.push_str(&quote(signature, "#GEN signature")?);
        }
        lines.push(line);
    }
    lines.push("#SIETYP 4".to_string());
    if let Some(prose) = &header.prose {
        lines.push(format!("#PROSA {}", quote(prose, "#PROSA")?));
    }
    if let Some(company_type) = &header.company_type {
        lines.push(format!("#FTYP {}", quote(company_type, "#FTYP")?));
    }
    if let Some(file_number) = &company.file_number {
        lines.push(format!("#FNR {}", quote(file_number, "#FNR")?));
    }

    let mut orgnr = format!("#ORGNR {}", format_orgnr(&company.orgnr_raw)?);
    push_optional_slots(
        &mut orgnr,
        &[
            company.acquisition_number.as_deref(),
            company.activity_number.as_deref(),
        ],
        "#ORGNR suffix",
    )?;
    lines.push(orgnr);

    if let Some(industry_code) = &header.industry_code {
        lines.push(format!("#BKOD {}", quote(industry_code, "#BKOD")?));
    }
    if !company.contact.is_empty()
        || !company.distribution_address.is_empty()
        || !company.postal_address.is_empty()
        || !company.telephone.is_empty()
    {
        lines.push(format!(
            "#ADRESS {} {} {} {}",
            quote(&company.contact, "#ADRESS contact")?,
            quote(
                &company.distribution_address,
                "#ADRESS distribution address"
            )?,
            quote(&company.postal_address, "#ADRESS postal address")?,
            quote(&company.telephone, "#ADRESS telephone")?
        ));
    }
    lines.push(format!("#FNAMN {}", quote(&company.name, "#FNAMN")?));

    let mut years: Vec<_> = document.years.iter().collect();
    years.sort_by_key(|year| std::cmp::Reverse(year.idx));
    for year in years {
        lines.push(format!("#RAR {} {} {}", year.idx, year.start, year.end));
    }
    if let Some(taxation_year) = header.taxation_year {
        lines.push(format!("#TAXAR {taxation_year}"));
    }
    if let Some(extent_date) = header.extent_date {
        lines.push(format!("#OMFATTN {extent_date}"));
    }
    lines.push(format!("#KPTYP {}", header.chart_type));
    lines.push(format!("#VALUTA {}", header.currency));

    for account in document.accounts.values() {
        lines.push(format!(
            "#KONTO {} {}",
            account.no,
            quote(&account.name, &format!("#KONTO {} name", account.no))?
        ));
        if let Some(account_type) = &account.account_type {
            lines.push(format!("#KTYP {} {}", account.no, account_type));
        }
        if let Some(unit) = &account.unit {
            lines.push(format!(
                "#ENHET {} {}",
                account.no,
                quote(unit, &format!("#ENHET {}", account.no))?
            ));
        }
        if let Some(sru) = account.sru {
            lines.push(format!("#SRU {} {}", account.no, sru));
        }
    }

    for dimension in document.dimensions.values() {
        if let Some(parent) = dimension.super_dimension {
            lines.push(format!(
                "#UNDERDIM {} {} {}",
                dimension.no,
                quote(&dimension.name, &format!("#UNDERDIM {} name", dimension.no))?,
                parent
            ));
        } else {
            lines.push(format!(
                "#DIM {} {}",
                dimension.no,
                quote(&dimension.name, &format!("#DIM {} name", dimension.no))?
            ));
        }
        for object in document
            .objects
            .range((dimension.no, String::new())..)
            .take_while(|((dimension_no, _), _)| *dimension_no == dimension.no)
            .map(|(_, object)| object)
        {
            lines.push(format!(
                "#OBJEKT {} {} {}",
                object.dimension_no,
                quote(
                    &object.no,
                    &format!("#OBJEKT {} object number", object.dimension_no)
                )?,
                quote(
                    &object.name,
                    &format!("#OBJEKT {} {:?} name", object.dimension_no, object.no)
                )?
            ));
        }
    }

    render_balance_map(&mut lines, "#IB", &document.ib)?;
    render_balance_map(&mut lines, "#UB", &document.ub)?;

    let mut period_balances: Vec<_> = document.period_balances.iter().collect();
    period_balances.sort_by(|left, right| {
        (left.year, left.period, left.account, &left.objects).cmp(&(
            right.year,
            right.period,
            right.account,
            &right.objects,
        ))
    });
    for balance in period_balances {
        let mut line = format!(
            "#PSALDO {} {} {} {} {}",
            balance.year,
            balance.period,
            balance.account,
            format_object_list(&balance.objects, "#PSALDO object list")?,
            format_decimal(balance.amount, "#PSALDO amount")?
        );
        if !balance.quantity.is_zero() {
            line.push(' ');
            line.push_str(&format_decimal(balance.quantity, "#PSALDO quantity")?);
        }
        lines.push(line);
    }
    render_balance_map(&mut lines, "#RES", &document.res)?;

    let mut vouchers: Vec<_> = document.vouchers.iter().collect();
    vouchers.sort_by(|left, right| (&left.series, left.number).cmp(&(&right.series, right.number)));
    for voucher in vouchers {
        let mut line = format!(
            "#VER {} {} {}",
            quote(
                &voucher.series,
                &format!("#VER {:?} series", voucher.series)
            )?,
            voucher.number,
            voucher.date
        );
        push_voucher_slots(&mut line, voucher)?;
        lines.push(line);
        lines.push("{".to_string());
        for (row_index, row) in voucher.rows.iter().enumerate() {
            let label = match row.kind {
                VoucherRowKind::Transaction => "#TRANS",
                VoucherRowKind::Added => "#RTRANS",
                VoucherRowKind::Removed => "#BTRANS",
            };
            let context = format!(
                "#VER {:?} {} row {}",
                voucher.series,
                voucher.number,
                row_index + 1
            );
            let mut row_line = format!(
                "{label} {} {} {}",
                row.account,
                format_object_list(&row.objects, &context)?,
                format_decimal(row.amount, &format!("{context} amount"))?
            );
            push_row_slots(&mut row_line, row, &context)?;
            lines.push(row_line);
        }
        lines.push("}".to_string());
    }

    Ok(lines.join("\r\n"))
}

/// Render and encode a document using the SIE-mandated CP437 code page.
pub fn encode(document: &SieDocument) -> Result<Vec<u8>> {
    let rendered = render(document)?;
    let mut output = Vec::with_capacity(rendered.len());
    for (line_index, line) in rendered.split("\r\n").enumerate() {
        if line_index != 0 {
            output.extend_from_slice(b"\r\n");
        }
        let record = line.split_ascii_whitespace().next().unwrap_or("record");
        match encode_cp437(line) {
            Ok(bytes) => output.extend(bytes),
            Err(error) => {
                return Err(anyhow!(
                    "cannot encode {record} record on line {}: {error}",
                    line_index + 1
                ));
            }
        }
    }
    Ok(output)
}

/// Render, CP437-encode, and write a document to a file.
pub fn write_file(document: &SieDocument, path: &Path) -> Result<()> {
    let bytes = encode(document)?;
    std::fs::write(path, bytes)
        .with_context(|| format!("failed to write SIE document to {}", path.display()))
}

fn validate_orgnr(raw: &str) -> Result<()> {
    ensure!(
        (raw.len() == 10 || raw.len() == 12) && raw.bytes().all(|byte| byte.is_ascii_digit()),
        "#ORGNR must contain 10 or 12 digits (an optional dash is accepted when reading), got {raw:?}"
    );
    Ok(())
}

fn format_orgnr(raw: &str) -> Result<String> {
    validate_orgnr(raw)?;
    let split = raw.len() - 4;
    Ok(format!("{}-{}", &raw[..split], &raw[split..]))
}

fn validate_account(account: AccountNo) -> Result<()> {
    ensure!(
        (1000..=9999).contains(&account),
        "account number must be in 1000..=9999, got {account}"
    );
    Ok(())
}

fn validate_dimension(dimension: DimensionNo) -> Result<()> {
    ensure!(
        (1..=999).contains(&dimension),
        "dimension number must be in 1..=999, got {dimension}"
    );
    Ok(())
}

fn validate_balance_map(
    document: &SieDocument,
    label: &str,
    balances: &std::collections::BTreeMap<(YearIdx, AccountNo), Decimal>,
    years: &BTreeSet<YearIdx>,
) -> Result<()> {
    for (&(year, account), &amount) in balances {
        validate_reference(document, years, year, account, label)?;
        format_decimal(amount, &format!("{label} {year} {account} amount"))?;
    }
    Ok(())
}

fn validate_reference(
    document: &SieDocument,
    years: &BTreeSet<YearIdx>,
    year: YearIdx,
    account: AccountNo,
    label: &str,
) -> Result<()> {
    ensure!(
        years.contains(&year),
        "{label} references undeclared fiscal-year index {year}"
    );
    ensure!(
        document.accounts.contains_key(&account),
        "{label} references undeclared account {account}"
    );
    Ok(())
}

fn validate_object_list(document: &SieDocument, objects: &ObjectList, context: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    for (dimension, object) in objects {
        ensure!(
            document.dimensions.contains_key(dimension),
            "{context} references undeclared dimension {dimension}"
        );
        ensure!(
            seen.insert(*dimension),
            "{context} contains dimension {dimension} more than once"
        );
        ensure!(
            !object.is_empty(),
            "{context} has an empty object number for dimension {dimension}"
        );
    }
    Ok(())
}

fn validate_all_text(document: &SieDocument) -> Result<()> {
    let mut check = |value: &str, context: &str| validate_text(value, context);
    check(&document.header.program_name, "#PROGRAM program name")?;
    check(&document.header.program_version, "#PROGRAM version")?;
    optional_text(
        &mut check,
        document.header.signature.as_deref(),
        "#GEN signature",
    )?;
    optional_text(&mut check, document.header.prose.as_deref(), "#PROSA")?;
    optional_text(&mut check, document.header.company_type.as_deref(), "#FTYP")?;
    optional_text(
        &mut check,
        document.header.industry_code.as_deref(),
        "#BKOD",
    )?;
    check(&document.company.name, "#FNAMN")?;
    optional_text(&mut check, document.company.file_number.as_deref(), "#FNR")?;
    check(&document.company.contact, "#ADRESS contact")?;
    check(
        &document.company.distribution_address,
        "#ADRESS distribution address",
    )?;
    check(&document.company.postal_address, "#ADRESS postal address")?;
    check(&document.company.telephone, "#ADRESS telephone")?;
    optional_text(
        &mut check,
        document.company.acquisition_number.as_deref(),
        "#ORGNR acquisition number",
    )?;
    optional_text(
        &mut check,
        document.company.activity_number.as_deref(),
        "#ORGNR activity number",
    )?;
    for account in document.accounts.values() {
        check(&account.name, &format!("#KONTO {} name", account.no))?;
        optional_text(
            &mut check,
            account.unit.as_deref(),
            &format!("#ENHET {}", account.no),
        )?;
    }
    for dimension in document.dimensions.values() {
        check(&dimension.name, &format!("#DIM {} name", dimension.no))?;
    }
    for object in document.objects.values() {
        check(
            &object.no,
            &format!("#OBJEKT {} object number", object.dimension_no),
        )?;
        check(
            &object.name,
            &format!("#OBJEKT {} {:?} name", object.dimension_no, object.no),
        )?;
    }
    for voucher in &document.vouchers {
        let context = format!("#VER {:?} {}", voucher.series, voucher.number);
        check(&voucher.series, &format!("{context} series"))?;
        check(&voucher.text, &format!("{context} text"))?;
        for (index, row) in voucher.rows.iter().enumerate() {
            let row_context = format!("{context} row {}", index + 1);
            check(&row.text, &format!("{row_context} text"))?;
            for (dimension, object) in &row.objects {
                check(
                    object,
                    &format!("{row_context} dimension {dimension} object"),
                )?;
            }
        }
    }
    for balance in &document.period_balances {
        for (dimension, object) in &balance.objects {
            check(object, &format!("#PSALDO dimension {dimension} object"))?;
        }
    }
    Ok(())
}

fn optional_text(
    check: &mut impl FnMut(&str, &str) -> Result<()>,
    value: Option<&str>,
    context: &str,
) -> Result<()> {
    if let Some(value) = value {
        check(value, context)?;
    }
    Ok(())
}

fn validate_text(value: &str, context: &str) -> Result<()> {
    if let Some(character) = value
        .chars()
        .find(|character| character.is_control() || matches!(character, '\n' | '\r'))
    {
        bail!(
            "{context} contains disallowed control character U+{:04X}",
            character as u32
        );
    }
    Ok(())
}

fn quote(value: &str, context: &str) -> Result<String> {
    validate_text(value, context)?;
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    Ok(format!("\"{escaped}\""))
}

fn format_object_list(objects: &ObjectList, context: &str) -> Result<String> {
    if objects.is_empty() {
        return Ok("{}".to_string());
    }
    let mut fields = Vec::with_capacity(objects.len() * 2);
    for (dimension, object) in objects {
        fields.push(dimension.to_string());
        fields.push(quote(object, context)?);
    }
    Ok(format!("{{{}}}", fields.join(" ")))
}

fn format_decimal(value: Decimal, context: &str) -> Result<String> {
    if value.is_zero() {
        return Ok("0".to_string());
    }
    let normalized = value.normalize();
    if normalized.scale() > 2 {
        bail!(
            "{context} has more than two fractional digits: {}",
            normalized
        );
    }
    Ok(normalized.to_string())
}

fn render_balance_map(
    lines: &mut Vec<String>,
    label: &str,
    balances: &std::collections::BTreeMap<(YearIdx, AccountNo), Decimal>,
) -> Result<()> {
    for (&(year, account), &amount) in balances {
        lines.push(format!(
            "{label} {year} {account} {}",
            format_decimal(amount, &format!("{label} {year} {account} amount"))?
        ));
    }
    Ok(())
}

/// Append optional positional fields through the last present field, using
/// `""` for any interior gap.
fn push_optional_slots(line: &mut String, fields: &[Option<&str>], context: &str) -> Result<()> {
    let Some(last) = fields.iter().rposition(Option::is_some) else {
        return Ok(());
    };
    for field in &fields[..=last] {
        line.push(' ');
        line.push_str(&quote(field.unwrap_or(""), context)?);
    }
    Ok(())
}

fn push_voucher_slots(line: &mut String, voucher: &crate::document::Voucher) -> Result<()> {
    let has_text = !voucher.text.is_empty();
    let has_registration = voucher.registration_date.is_some();
    if !(has_text || has_registration) {
        return Ok(());
    }
    let context = format!("#VER {:?} {}", voucher.series, voucher.number);
    line.push(' ');
    line.push_str(&quote(&voucher.text, &format!("{context} text"))?);
    if has_registration {
        line.push(' ');
        if let Some(date) = voucher.registration_date {
            line.push_str(&date.to_string());
        } else {
            line.push_str("\"\"");
        }
    }
    Ok(())
}

fn push_row_slots(
    line: &mut String,
    row: &crate::document::VoucherRow,
    context: &str,
) -> Result<()> {
    let has_date = row.transaction_date.is_some();
    let has_text = !row.text.is_empty();
    let has_quantity = !row.quantity.is_zero();
    if !(has_date || has_text || has_quantity) {
        return Ok(());
    }
    line.push(' ');
    if let Some(date) = row.transaction_date {
        line.push_str(&date.to_string());
    } else {
        line.push_str("\"\"");
    }
    if has_text || has_quantity {
        line.push(' ');
        line.push_str(&quote(&row.text, &format!("{context} text"))?);
    }
    if has_quantity {
        line.push(' ');
        line.push_str(&format_decimal(
            row.quantity,
            &format!("{context} quantity"),
        )?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Account, Company, FiscalYear, Header, SieDate, Voucher, VoucherRow};
    use std::collections::BTreeMap;
    use std::str::FromStr;

    fn minimal_document() -> SieDocument {
        let mut accounts = BTreeMap::new();
        accounts.insert(
            1930,
            Account {
                no: 1930,
                name: "Bank".to_string(),
                sru: None,
                account_type: None,
                unit: None,
            },
        );
        SieDocument {
            header: Header {
                program_name: "Test".to_string(),
                program_version: "1.0".to_string(),
                ..Header::default()
            },
            company: Company {
                name: "Exempel AB".to_string(),
                orgnr_raw: "5591741383".to_string(),
                ..Company::default()
            },
            years: vec![FiscalYear {
                idx: 0,
                start: "20240101".parse().unwrap(),
                end: "20241231".parse().unwrap(),
            }],
            accounts,
            ..SieDocument::default()
        }
    }

    #[test]
    fn exact_minimal_render_and_crlf() {
        let output = render(&minimal_document()).unwrap();
        assert_eq!(
            output,
            concat!(
                "#FLAGGA 0\r\n",
                "#PROGRAM \"Test\" \"1.0\"\r\n",
                "#FORMAT PC8\r\n",
                "#SIETYP 4\r\n",
                "#ORGNR 559174-1383\r\n",
                "#FNAMN \"Exempel AB\"\r\n",
                "#RAR 0 20240101 20241231\r\n",
                "#KPTYP EUBAS97\r\n",
                "#VALUTA SEK\r\n",
                "#KONTO 1930 \"Bank\""
            )
        );
        assert!(!output.ends_with("\r\n"));
        assert!(!output.contains('\n') || output.contains("\r\n"));
    }

    #[test]
    fn escapes_strings_and_rejects_precision() {
        let mut document = minimal_document();
        document.company.name = "A \"quote\" \\\\ path".to_string();
        assert!(
            render(&document)
                .unwrap()
                .contains("#FNAMN \"A \\\"quote\\\" \\\\\\\\ path\"")
        );
        document
            .ib
            .insert((0, 1930), Decimal::from_str("1.001").unwrap());
        assert!(
            render(&document)
                .unwrap_err()
                .to_string()
                .contains("fractional")
        );
    }

    #[test]
    fn vouchers_keep_row_order_and_all_kinds() {
        let mut document = minimal_document();
        document.vouchers.push(Voucher {
            series: "A".to_string(),
            number: 1,
            date: "20240201".parse().unwrap(),
            text: String::new(),
            registration_date: None,
            rows: vec![
                VoucherRow {
                    kind: VoucherRowKind::Removed,
                    account: 1930,
                    objects: vec![],
                    amount: Decimal::ONE,
                    transaction_date: None,
                    text: String::new(),
                    quantity: Decimal::ZERO,
                },
                VoucherRow {
                    kind: VoucherRowKind::Added,
                    account: 1930,
                    objects: vec![],
                    amount: -Decimal::ONE,
                    transaction_date: Some(SieDate::new(2024, 2, 2).unwrap()),
                    text: "rättad".to_string(),
                    quantity: Decimal::new(2, 0),
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
        let rendered = render(&document).unwrap();
        let removed = rendered.find("#BTRANS").unwrap();
        let added = rendered.find("#RTRANS").unwrap();
        let normal = rendered.find("#TRANS").unwrap();
        assert!(removed < added && added < normal);
        assert!(rendered.contains("#VER \"A\" 1 20240201\r\n{\r\n"));
        assert!(rendered.contains("#RTRANS 1930 {} -1 20240202 \"rättad\" 2"));
    }

    #[test]
    fn cp437_error_has_record_context() {
        let mut document = minimal_document();
        document.company.name = "Emoji 😀 AB".to_string();
        let error = encode(&document).unwrap_err().to_string();
        assert!(error.contains("#FNAMN"), "{error}");
        assert!(
            error.contains("'😀'") || error.contains("\"😀\""),
            "{error}"
        );
        assert!(error.contains("U+1F600"), "{error}");
    }
}
