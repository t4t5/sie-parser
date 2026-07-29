//! Typed, read/write-friendly domain model over a parsed SIE file.
//!
//! The low-level [`crate::parse`] API remains lossless with respect to spans
//! and diagnostics. This module provides the accounting records needed to
//! produce a canonical SIE 4 file.

use crate::parser;
use crate::types::{Field, FieldValue, Item, ParseOutput};
use anyhow::{Context, Result, anyhow, bail};
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

/// Year index as used in SIE `#RAR`/`#IB`/`#UB`/`#RES`: 0 = current fiscal
/// year, -1 = previous, etc.
pub type YearIdx = i32;

/// SIE account number (1000..9999 range in BAS charts).
pub type AccountNo = u32;

/// Skatteverket SRU reporting code (e.g. 7302).
pub type SruCode = u32;

/// SIE dimension number.
pub type DimensionNo = u32;

/// An ordered list of `(dimension number, object number)` pairs.
pub type ObjectList = Vec<(DimensionNo, String)>;

/// A validated SIE calendar date, represented as `YYYYMMDD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SieDate {
    year: u16,
    month: u8,
    day: u8,
}

impl SieDate {
    pub fn new(year: u16, month: u8, day: u8) -> Result<Self> {
        if year == 0 || year > 9999 {
            bail!("SIE date year must be in 0001..=9999, got {year}");
        }
        if !(1..=12).contains(&month) {
            bail!("SIE date month must be in 01..=12, got {month}");
        }
        let max_day = days_in_month(year, month);
        if day == 0 || day > max_day {
            bail!("invalid day {day} for {year:04}-{month:02}");
        }
        Ok(Self { year, month, day })
    }

    pub fn parse(value: &str) -> Result<Self> {
        value.parse()
    }

    pub const fn year(self) -> u16 {
        self.year
    }

    pub const fn month(self) -> u8 {
        self.month
    }

    pub const fn day(self) -> u8 {
        self.day
    }
}

impl fmt::Display for SieDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}{:02}{:02}", self.year, self.month, self.day)
    }
}

impl FromStr for SieDate {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        if value.len() != 8 || !value.bytes().all(|b| b.is_ascii_digit()) {
            bail!("invalid SIE date {value:?}: expected YYYYMMDD");
        }
        let year = value[0..4].parse()?;
        let month = value[4..6].parse()?;
        let day = value[6..8].parse()?;
        Self::new(year, month, day).with_context(|| format!("invalid SIE date {value:?}"))
    }
}

/// A validated accounting period, represented as `YYYYMM`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct YearMonth {
    year: u16,
    month: u8,
}

impl YearMonth {
    pub fn new(year: u16, month: u8) -> Result<Self> {
        if year == 0 || year > 9999 {
            bail!("period year must be in 0001..=9999, got {year}");
        }
        if !(1..=12).contains(&month) {
            bail!("period month must be in 01..=12, got {month}");
        }
        Ok(Self { year, month })
    }

    pub fn parse(value: &str) -> Result<Self> {
        value.parse()
    }

    pub const fn year(self) -> u16 {
        self.year
    }

    pub const fn month(self) -> u8 {
        self.month
    }
}

impl fmt::Display for YearMonth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}{:02}", self.year, self.month)
    }
}

impl FromStr for YearMonth {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        if value.len() != 6 || !value.bytes().all(|b| b.is_ascii_digit()) {
            bail!("invalid SIE period {value:?}: expected YYYYMM");
        }
        let year = value[0..4].parse()?;
        let month = value[4..6].parse()?;
        Self::new(year, month).with_context(|| format!("invalid SIE period {value:?}"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub import_flag: u8,
    pub program_name: String,
    pub program_version: String,
    pub generation_date: Option<SieDate>,
    pub signature: Option<String>,
    pub format: String,
    pub sie_type: u8,
    pub prose: Option<String>,
    pub company_type: Option<String>,
    pub industry_code: Option<String>,
    pub taxation_year: Option<i32>,
    pub extent_date: Option<SieDate>,
    pub chart_type: String,
    pub currency: String,
}

impl Default for Header {
    fn default() -> Self {
        Self {
            import_flag: 0,
            program_name: env!("CARGO_PKG_NAME").to_string(),
            program_version: env!("CARGO_PKG_VERSION").to_string(),
            generation_date: None,
            signature: None,
            format: "PC8".to_string(),
            sie_type: 4,
            prose: None,
            company_type: None,
            industry_code: None,
            taxation_year: None,
            extent_date: None,
            chart_type: "EUBAS97".to_string(),
            currency: "SEK".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Company {
    pub name: String,
    /// Stored without a dash: `"559174-1383"` becomes `"5591741383"`.
    pub orgnr_raw: String,
    pub file_number: Option<String>,
    pub contact: String,
    pub distribution_address: String,
    /// The original `#ADRESS` postal-address token.
    pub postal_address: String,
    pub telephone: String,
    /// Convenience values parsed from [`Company::postal_address`].
    pub postnr: Option<String>,
    pub postort: Option<String>,
    pub acquisition_number: Option<String>,
    pub activity_number: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiscalYear {
    pub idx: YearIdx,
    pub start: SieDate,
    pub end: SieDate,
}

impl FiscalYear {
    pub const fn new(idx: YearIdx, start: SieDate, end: SieDate) -> Self {
        Self { idx, start, end }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Account {
    pub no: AccountNo,
    pub name: String,
    pub sru: Option<SruCode>,
    /// `T`, `S`, `K`, or `I`, as used by `#KTYP`.
    pub account_type: Option<String>,
    /// Quantity-reporting unit from `#ENHET`.
    pub unit: Option<String>,
}

impl Account {
    pub fn new(no: AccountNo, name: impl Into<String>) -> Self {
        Self {
            no,
            name: name.into(),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dimension {
    pub no: DimensionNo,
    pub name: String,
    /// Parent dimension for an `#UNDERDIM`; `None` means `#DIM`.
    pub super_dimension: Option<DimensionNo>,
}

impl Dimension {
    pub fn new(no: DimensionNo, name: impl Into<String>) -> Self {
        Self {
            no,
            name: name.into(),
            super_dimension: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SieObject {
    pub dimension_no: DimensionNo,
    pub no: String,
    pub name: String,
}

impl SieObject {
    pub fn new(dimension_no: DimensionNo, no: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            dimension_no,
            no: no.into(),
            name: name.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeriodBalance {
    pub year: YearIdx,
    pub period: YearMonth,
    pub account: AccountNo,
    pub objects: ObjectList,
    pub amount: Decimal,
    pub quantity: Decimal,
}

impl PeriodBalance {
    pub fn new(year: YearIdx, period: YearMonth, account: AccountNo, amount: Decimal) -> Self {
        Self {
            year,
            period,
            account,
            objects: Vec::new(),
            amount,
            quantity: Decimal::ZERO,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoucherRowKind {
    /// A normal `#TRANS` row.
    Transaction,
    /// A supplementary `#RTRANS` row.
    Added,
    /// A removed/cancelled `#BTRANS` row.
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoucherRow {
    pub kind: VoucherRowKind,
    pub account: AccountNo,
    pub objects: ObjectList,
    pub amount: Decimal,
    pub transaction_date: Option<SieDate>,
    pub text: String,
    pub quantity: Decimal,
}

impl VoucherRow {
    pub fn new(kind: VoucherRowKind, account: AccountNo, amount: Decimal) -> Self {
        Self {
            kind,
            account,
            objects: Vec::new(),
            amount,
            transaction_date: None,
            text: String::new(),
            quantity: Decimal::ZERO,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voucher {
    pub series: String,
    pub number: u32,
    pub date: SieDate,
    pub text: String,
    pub registration_date: Option<SieDate>,
    pub rows: Vec<VoucherRow>,
}

impl Voucher {
    pub fn new(series: impl Into<String>, number: u32, date: SieDate) -> Self {
        Self {
            series: series.into(),
            number,
            date,
            text: String::new(),
            registration_date: None,
            rows: Vec::new(),
        }
    }
}

/// Alias using the name used by the SIE specification.
pub type Verification = Voucher;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SieDocument {
    pub header: Header,
    pub company: Company,
    pub years: Vec<FiscalYear>,
    pub accounts: BTreeMap<AccountNo, Account>,
    pub dimensions: BTreeMap<DimensionNo, Dimension>,
    pub objects: BTreeMap<(DimensionNo, String), SieObject>,
    /// `(year_idx, account_no) -> closing balance (#UB)`.
    pub ub: BTreeMap<(YearIdx, AccountNo), Decimal>,
    /// `(year_idx, account_no) -> opening balance (#IB)`.
    pub ib: BTreeMap<(YearIdx, AccountNo), Decimal>,
    /// `(year_idx, account_no) -> profit-and-loss result (#RES)`.
    pub res: BTreeMap<(YearIdx, AccountNo), Decimal>,
    pub period_balances: Vec<PeriodBalance>,
    pub vouchers: Vec<Voucher>,
}

impl SieDocument {
    pub fn current_year(&self) -> Option<&FiscalYear> {
        self.years.iter().find(|y| y.idx == 0)
    }
}

/// Parse SIE source and walk the resulting AST into a typed [`SieDocument`].
///
/// Known records not currently represented are object-level opening/closing
/// balances (`#OIB`, `#OUB`), budgets (`#PBUDGET` and related records), and
/// checksum records such as `#KSUMMA`. Optional `sign` fields on vouchers and
/// transaction rows are also not retained. They remain available through
/// [`crate::parse`].
pub fn read(src: &str) -> Result<SieDocument> {
    let ParseOutput {
        items,
        diagnostics: _,
    } = parser::parse(src);

    let mut doc = SieDocument::default();
    let mut pending_sru = Vec::new();
    let mut pending_types = Vec::new();
    let mut pending_units = Vec::new();

    for item in &items {
        match item.label.to_ascii_uppercase().as_str() {
            "#FLAGGA" => doc.header.import_flag = int_field(item, 0)?,
            "#PROGRAM" => {
                doc.header.program_name = string_field(item, 0)?.to_string();
                doc.header.program_version = string_field(item, 1)?.to_string();
            }
            "#FORMAT" => doc.header.format = string_field(item, 0)?.to_ascii_uppercase(),
            "#GEN" => {
                doc.header.generation_date = Some(date_field(item, 0)?);
                doc.header.signature = optional_string_field(item, 1)?.map(str::to_string);
            }
            "#SIETYP" => doc.header.sie_type = int_field(item, 0)?,
            "#PROSA" => doc.header.prose = Some(string_field(item, 0)?.to_string()),
            "#FTYP" => doc.header.company_type = Some(string_field(item, 0)?.to_string()),
            "#BKOD" => doc.header.industry_code = Some(string_field(item, 0)?.to_string()),
            "#TAXAR" => doc.header.taxation_year = Some(int_field(item, 0)?),
            "#OMFATTN" => doc.header.extent_date = Some(date_field(item, 0)?),
            "#KPTYP" => doc.header.chart_type = string_field(item, 0)?.to_ascii_uppercase(),
            "#VALUTA" => doc.header.currency = string_field(item, 0)?.to_ascii_uppercase(),
            "#FNAMN" => doc.company.name = string_field(item, 0)?.to_string(),
            "#FNR" => doc.company.file_number = Some(string_field(item, 0)?.to_string()),
            "#ORGNR" => {
                doc.company.orgnr_raw = string_field(item, 0)?.replace('-', "");
                doc.company.acquisition_number =
                    optional_string_field(item, 1)?.map(str::to_string);
                doc.company.activity_number = optional_string_field(item, 2)?.map(str::to_string);
            }
            "#ADRESS" => {
                doc.company.contact = string_field(item, 0)?.to_string();
                doc.company.distribution_address = string_field(item, 1)?.to_string();
                doc.company.postal_address = string_field(item, 2)?.to_string();
                doc.company.telephone = string_field(item, 3)?.to_string();
                let (postnr, postort) = parse_postal_address(&doc.company.postal_address);
                doc.company.postnr = postnr;
                doc.company.postort = postort;
            }
            "#RAR" => {
                doc.years.push(FiscalYear {
                    idx: int_field(item, 0)?,
                    start: date_field(item, 1)?,
                    end: date_field(item, 2)?,
                });
            }
            "#KONTO" => {
                let no = int_field(item, 0)?;
                let name = string_field(item, 1)?.to_string();
                doc.accounts.insert(
                    no,
                    Account {
                        no,
                        name,
                        sru: None,
                        account_type: None,
                        unit: None,
                    },
                );
            }
            "#SRU" => pending_sru.push((int_field(item, 0)?, int_field(item, 1)?)),
            "#KTYP" => {
                pending_types.push((int_field(item, 0)?, string_field(item, 1)?.to_string()))
            }
            "#ENHET" => {
                pending_units.push((int_field(item, 0)?, string_field(item, 1)?.to_string()))
            }
            "#DIM" | "#UNDERDIM" => {
                let no = int_field(item, 0)?;
                doc.dimensions.insert(
                    no,
                    Dimension {
                        no,
                        name: string_field(item, 1)?.to_string(),
                        super_dimension: if item.label.eq_ignore_ascii_case("#UNDERDIM") {
                            Some(int_field(item, 2)?)
                        } else {
                            None
                        },
                    },
                );
            }
            "#OBJEKT" => {
                let dimension_no = int_field(item, 0)?;
                let no = string_field(item, 1)?.to_string();
                doc.objects.insert(
                    (dimension_no, no.clone()),
                    SieObject {
                        dimension_no,
                        no,
                        name: string_field(item, 2)?.to_string(),
                    },
                );
            }
            "#IB" => {
                let (year, account, amount) = balance_fields(item)?;
                doc.ib.insert((year, account), amount);
            }
            "#UB" => {
                let (year, account, amount) = balance_fields(item)?;
                doc.ub.insert((year, account), amount);
            }
            "#RES" => {
                let (year, account, amount) = balance_fields(item)?;
                doc.res.insert((year, account), amount);
            }
            "#PSALDO" => doc.period_balances.push(PeriodBalance {
                year: int_field(item, 0)?,
                period: string_field(item, 1)?
                    .parse()
                    .with_context(|| format!("{} field #1 is not a valid period", item.label))?,
                account: int_field(item, 2)?,
                objects: object_list_field(item, 3)?,
                amount: decimal_field(item, 4)?,
                quantity: optional_decimal_field(item, 5)?.unwrap_or(Decimal::ZERO),
            }),
            "#VER" => doc.vouchers.push(voucher_from_item(item)?),
            _ => {}
        }
    }

    for (account, sru) in pending_sru {
        account_mut(&mut doc, account, "#SRU")?.sru = Some(sru);
    }
    for (account, account_type) in pending_types {
        account_mut(&mut doc, account, "#KTYP")?.account_type = Some(account_type);
    }
    for (account, unit) in pending_units {
        account_mut(&mut doc, account, "#ENHET")?.unit = Some(unit);
    }

    if doc.company.orgnr_raw.is_empty() {
        bail!("#ORGNR missing from SIE file");
    }
    if doc.company.name.is_empty() {
        bail!("#FNAMN missing from SIE file");
    }
    if doc.current_year().is_none() {
        bail!("#RAR 0 (current fiscal year) missing from SIE file");
    }

    Ok(doc)
}

fn voucher_from_item(item: &Item) -> Result<Voucher> {
    let mut rows = Vec::new();
    for child in &item.children {
        let kind = match child.label.to_ascii_uppercase().as_str() {
            "#TRANS" => VoucherRowKind::Transaction,
            "#RTRANS" => VoucherRowKind::Added,
            "#BTRANS" => VoucherRowKind::Removed,
            _ => continue,
        };
        rows.push(VoucherRow {
            kind,
            account: int_field(child, 0)?,
            objects: object_list_field(child, 1)?,
            amount: decimal_field(child, 2)?,
            transaction_date: optional_date_field(child, 3)?,
            text: optional_string_field(child, 4)?.unwrap_or("").to_string(),
            quantity: optional_decimal_field(child, 5)?.unwrap_or(Decimal::ZERO),
        });
    }
    Ok(Voucher {
        series: string_field(item, 0)?.to_string(),
        number: int_field(item, 1)?,
        date: date_field(item, 2)?,
        text: optional_string_field(item, 3)?.unwrap_or("").to_string(),
        registration_date: optional_date_field(item, 4)?,
        rows,
    })
}

fn account_mut<'a>(
    doc: &'a mut SieDocument,
    account: AccountNo,
    label: &str,
) -> Result<&'a mut Account> {
    doc.accounts
        .get_mut(&account)
        .ok_or_else(|| anyhow!("{label} references undeclared account {account}"))
}

fn field_str(field: &Field) -> Option<&str> {
    match &field.value {
        FieldValue::Bare { text } | FieldValue::Quoted { text } => Some(text.as_str()),
        FieldValue::ObjectList { .. } => None,
    }
}

fn string_field(item: &Item, idx: usize) -> Result<&str> {
    item.fields
        .get(idx)
        .and_then(field_str)
        .ok_or_else(|| anyhow!("{} field #{} missing or non-string", item.label, idx))
}

fn optional_string_field(item: &Item, idx: usize) -> Result<Option<&str>> {
    let Some(field) = item.fields.get(idx) else {
        return Ok(None);
    };
    let value =
        field_str(field).ok_or_else(|| anyhow!("{} field #{} is not a string", item.label, idx))?;
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

fn int_field<T: FromStr>(item: &Item, idx: usize) -> Result<T>
where
    T::Err: fmt::Display,
{
    let value = string_field(item, idx)?;
    value
        .parse::<T>()
        .map_err(|e| anyhow!("{} field #{} {:?}: {e}", item.label, idx, value))
}

fn date_field(item: &Item, idx: usize) -> Result<SieDate> {
    string_field(item, idx)?
        .parse()
        .with_context(|| format!("{} field #{} is not a valid date", item.label, idx))
}

fn optional_date_field(item: &Item, idx: usize) -> Result<Option<SieDate>> {
    optional_string_field(item, idx)?
        .map(|value| {
            value
                .parse()
                .with_context(|| format!("{} field #{} is not a valid date", item.label, idx))
        })
        .transpose()
}

fn decimal_field(item: &Item, idx: usize) -> Result<Decimal> {
    let value = string_field(item, idx)?;
    Decimal::from_str(value).map_err(|e| anyhow!("{} field #{} {:?}: {e}", item.label, idx, value))
}

fn optional_decimal_field(item: &Item, idx: usize) -> Result<Option<Decimal>> {
    optional_string_field(item, idx)?
        .map(|value| {
            Decimal::from_str(value)
                .map_err(|e| anyhow!("{} field #{} {:?}: {e}", item.label, idx, value))
        })
        .transpose()
}

fn object_list_field(item: &Item, idx: usize) -> Result<ObjectList> {
    let tokens = match item.fields.get(idx).map(|field| &field.value) {
        Some(FieldValue::ObjectList { tokens }) => tokens,
        _ => bail!(
            "{} field #{} missing or not an object list",
            item.label,
            idx
        ),
    };
    if tokens.len() % 2 != 0 {
        bail!(
            "{} field #{} has an odd number of object-list tokens",
            item.label,
            idx
        );
    }
    tokens
        .chunks_exact(2)
        .map(|pair| {
            let dimension = pair[0].parse().with_context(|| {
                format!(
                    "{} field #{} has invalid dimension {:?}",
                    item.label, idx, pair[0]
                )
            })?;
            Ok((dimension, pair[1].clone()))
        })
        .collect()
}

fn balance_fields(item: &Item) -> Result<(YearIdx, AccountNo, Decimal)> {
    Ok((
        int_field(item, 0)?,
        int_field(item, 1)?,
        decimal_field(item, 2)?,
    ))
}

/// Split `"106 31 STOCKHOLM"` into (`"10631"`, `"STOCKHOLM"`).
fn parse_postal_address(address: &str) -> (Option<String>, Option<String>) {
    let trimmed = address.trim();
    let mut chars = trimmed.chars().peekable();
    let mut digits = String::new();
    while let Some(&character) = chars.peek() {
        if character.is_ascii_digit() {
            digits.push(character);
            chars.next();
        } else if character == ' ' && digits.len() < 5 {
            chars.next();
        } else {
            break;
        }
    }
    if digits.len() != 5 {
        return (None, None);
    }
    let city = chars.collect::<String>().trim().to_string();
    if city.is_empty() {
        (Some(digits), None)
    } else {
        (Some(digits), Some(city))
    }
}

const fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

const fn is_leap_year(year: u16) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_validation_and_accessors() {
        let leap: SieDate = "20240229".parse().unwrap();
        assert_eq!((leap.year(), leap.month(), leap.day()), (2024, 2, 29));
        assert_eq!(leap.to_string(), "20240229");
        assert!("20230229".parse::<SieDate>().is_err());
        assert!("20241301".parse::<SieDate>().is_err());
        assert!("2024011".parse::<SieDate>().is_err());
    }

    #[test]
    fn period_validation() {
        let period: YearMonth = "202412".parse().unwrap();
        assert_eq!((period.year(), period.month()), (2024, 12));
        assert_eq!(period.to_string(), "202412");
        assert!("202400".parse::<YearMonth>().is_err());
    }

    #[test]
    fn postal_address_preserves_and_parses_values() {
        assert_eq!(
            parse_postal_address("106 31 STOCKHOLM"),
            (Some("10631".into()), Some("STOCKHOLM".into()))
        );
        assert_eq!(
            parse_postal_address("12345 GÖTEBORG"),
            (Some("12345".into()), Some("GÖTEBORG".into()))
        );
        assert_eq!(parse_postal_address("no postnr here"), (None, None));
    }
}
