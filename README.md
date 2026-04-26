# sie-parser

Rust parser, encoder, and typed document model for the
[**SIE 4B**](https://sie.se/format/) file format — the Swedish standard for
exchanging bookkeeping data between accounting programs.

No LSP, async, or tokio dependencies. Use this if you're building any
SIE-consuming tool (importer, converter, analytics, validator).

## Usage

```rust
use sie_parser::{parse, SieDocument, Severity};

let src = std::fs::read_to_string("ledger.se")?;
let out = parse(&src);

// Surface diagnostics:
for d in &out.diagnostics {
    println!("{:?} [{}] {}", d.severity, d.code, d.message);
}

// Walk the typed document model:
let doc = SieDocument::from_items(&out.items);
println!("{}", doc.company.name.as_deref().unwrap_or("(unnamed)"));
for acc in &doc.accounts {
    println!("{} {}", acc.no, acc.name);
}
```

`parse` never returns `Err` — every problem becomes a `Diagnostic` with a
stable `&'static str` code (see `sie_parser::diagnostics`). This means a
malformed file still produces a partial parse you can inspect.

## Encoding

Real-world SIE files are encoded in **CP437** (IBM PC-8) and typically use
CRLF line endings. The convenience function `read_file` auto-detects the
encoding (via the `#FORMAT PC8` marker or a UTF-8 validity check) and decodes
to UTF-8. `parse` itself takes a `&str` and assumes the caller has already
decoded.

```rust
let src = sie_parser::read_file("ledger.se")?;
let out = sie_parser::parse(&src);
```

## Companion projects

- [`sie-lsp`](https://github.com/t4t5/sie-lsp) — language server and CLI
  built on top of this crate.
- [`sie.nvim`](https://github.com/t4t5/sie.nvim) — Neovim plugin for `.se`
  files.
