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

## Companion projects

- [`sie-lsp`](https://github.com/t4t5/sie-lsp) — language server and CLI
  built on top of this crate.
- [`sie.nvim`](https://github.com/t4t5/sie.nvim) — Neovim plugin for `.se`
  files.
