# sie-parser

Rust parser, encoder, and typed document model for the [**SIE 4B**](https://sie.se/format/) file format — the Swedish standard for exchanging bookkeeping data between accounting programs.

Useful if you're building any SIE-consuming tool (importer, converter, analytics, validator).

## Usage

```rust
use sie_parser::{document, read_file};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let src = read_file(Path::new("ledger.se"))?;
    let doc = document::read(&src)?;

    println!("{}", doc.company.name);
    for acc in doc.accounts.values() {
        println!("{} {}", acc.no, acc.name);
    }
    Ok(())
}
```

`read_file` auto-detects encoding — real-world SIE files are usually
**CP437** (IBM PC-8) with CRLF line endings — and decodes to UTF-8.

For lower-level access (raw items, byte-offset spans, diagnostics with
stable error codes), call `parse` directly instead of `document::read`.

## Generating SIE4

Build a typed document and let this crate handle canonical record ordering,
quoting, decimal validation, CRLF endings, and CP437 encoding:

```rust
use sie_parser::{
    Account, Company, FiscalYear, Header, SieDocument, write_document,
};
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let mut document = SieDocument {
        header: Header {
            program_name: "ledger-export".into(),
            program_version: "1.0".into(),
            ..Header::default()
        },
        company: Company {
            name: "Exempel AB".into(),
            orgnr_raw: "5591741383".into(),
            ..Company::default()
        },
        years: vec![FiscalYear {
            idx: 0,
            start: "20260101".parse()?,
            end: "20261231".parse()?,
        }],
        ..SieDocument::default()
    };
    document.accounts.insert(1930, Account::new(1930, "Företagskonto"));

    write_document(&document, Path::new("ledger.se"))?;
    Ok(())
}
```

Use `render_document` for UTF-8 SIE text or `encode_document` for CP437
bytes. Rendering is fallible: invalid references, excessive decimal
precision, control characters, and unsupported output metadata are rejected.
Encoding additionally reports CP437 failures with the affected record.

## Companion projects

- [`sie-lsp`](https://github.com/t4t5/sie-lsp) — language server and CLI
  built on top of this crate.
- [`sie.nvim`](https://github.com/t4t5/sie.nvim) — Neovim plugin for `.se`
  files.
