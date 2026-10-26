# multype2txt

[中文](README.md) · [Changelog](CHANGELOG_en.md)

A command-line tool that converts Office and PDF documents into text, built for AI/LLM consumption.

It picks the output shape automatically from the input format: **Word to Markdown, Excel to CSV, PPT to structure plus text, PDF to plain text**.
The same pipeline works standalone in scripts and pipes, and also as a reusable library.

The core purpose is to make these documents easy for AI to read: structure replaces guesswork, and full extraction replaces speculative omission.

## Three product lines

| Input | Default output | Notes |
|-------|----------------|-------|
| `.docx` | Markdown | Headings, lists, tables, links and footnotes kept as Markdown syntax; images get placeholders only |
| `.doc` | Markdown | **Best effort**: text and rough headings only — no lists/tables/styles/images, see [Limitations of legacy binary formats](#limitations-of-legacy-binary-formats) |
| `.xlsx` / `.xls` | CSV | **Full** extraction of computed cell values; multiple sheets separated by comment lines; images ignored entirely, no placeholders |
| `.pptx` | Markdown | Slide pages, titles, list hierarchy, speaker notes; images get placeholders only |
| `.ppt` | Markdown | **Best effort**: pages and titles kept, list hierarchy fully lost, see above |
| `.pdf` | Plain text | Body text in reading order; pages with a detected table are rebuilt from word coordinates into an aligned plain-text grid (column-aligned, numeric columns right-aligned); form-feed characters (`U+000C`) between pages; `--images-dir` exports images per page |

Design trade-offs:

- **Full extraction, never summarizing.** Sampling, truncation and summarizing are the reader's (LLM's) job; the converter only hands content over completely — once something is omitted here, no later stage can recover it.
- **We do not touch images; this is an explicit limitation.** A placeholder only answers "there is an image here" and "what it is called"; the image content is never parsed. Use `--images-dir` to write the bytes to files and let the reader decide whether to look.
- **Structure is restored, not inferred.** docx/xlsx/pptx are declarative structures by nature — parsing restores them; wherever heuristic guessing would be needed, we prefer not to guess.
- **Incompatible requests fail loudly; no silent degradation.** Asking a PDF for CSV, or a document for CSV, exits with an error rather than handing back something that "looks right but is not".

## Features

- Single command, zero config: `multype2txt -i <file>` yields text in the right format.
- Covers common office and spreadsheet formats, pure Rust with no C/C++ dynamic libraries.
- Results go to standard output by default, pipe-friendly; `-o` writes to a file.
- `-f` overrides the automatic mapping (`-f text` falls back to plain text).
- Diagnostic logs go only to `/tmp/<input-filename>.conv.err.log` by default, with a one-line hint on stderr when something went wrong; `-v` additionally logs to stderr (info / debug), which never mixes into standard output.
- Extensions are case-insensitive and the real format is detected from the file header (e.g. a `.xls` saved as `.doc` still parses).
- A downstream closing the pipe early (`| head`) is treated as a normal exit: no error, exit code `0`.
- Best-effort support for legacy binary formats (`.doc` / `.ppt`): text and rough headings come out, but the structure is weaker than OOXML — see [Limitations of legacy binary formats](#limitations-of-legacy-binary-formats).

## Supported formats

| Format | Extension | Default output | Parser |
|--------|-----------|----------------|--------|
| Word (OOXML) | `.docx` | Markdown | office_oxide |
| Word (binary) | `.doc` | Markdown | office_oxide |
| PowerPoint (OOXML) | `.pptx` | Markdown | office_oxide |
| PowerPoint (binary) | `.ppt` | Markdown | office_oxide |
| Excel (OOXML) | `.xlsx` | CSV | office_oxide |
| Excel (binary) | `.xls` | CSV | office_oxide |
| PDF | `.pdf` | Plain text | pdf_oxide |

Notes:

- All six Office formats are handled by `office_oxide`, which auto-detects the real file type.
- Encrypted (password-protected) OOXML documents are not decrypted; a clear error is reported.
- PDF body text is extracted in page reading order, with a form-feed character (`U+000C` / `\f`) inserted between pages to preserve page boundaries; in a plain text viewer this shows as pagination without any visible marker text.

### Limitations of legacy binary formats

`.doc` / `.ppt` are binary streaming formats, unlike the declarative structure of OOXML: the underlying library only extracts a **text stream** from them and does not parse formatting records (bold, numbering, list hierarchy, tables, image anchors); structure is rebuilt line by line with heuristics. The completeness gap is therefore obvious on the same content:

| Input | Heading lines | List lines | Structure total |
|-------|---------------|------------|-----------------|
| `samples/demo.doc` | 1 | 0 | 1 |
| `samples/demo.docx` | 7 | 34 | 41 |
| `samples/demo.ppt` | 26 | 0 | 26 |
| `samples/demo.pptx` | 26 | 12 | 38 |

Specific behavior:

- **`.doc`**: body text degrades to plain paragraphs; levels like "Table of Contents" or "Part I · Natural Overview" are not recognized as headings or lists — only the first short line is heuristically promoted to an H1.
- **`.ppt`**: page titles survive (the underlying library reads placeholder types), but **list hierarchy is completely lost**; bullets degrade into standalone `*` character lines or italic text.
- **`.xls`**: unaffected — CSV reads the native grid, bypassing the IR heuristics, so data stays fully aligned.

**Convert to `.docx` / `.pptx` first**, after which lists, tables, bold text and image placeholders are all fully restored:

```bash
# Word / PowerPoint's "Save As" is enough; for batches use LibreOffice
libreoffice --headless --convert-to docx --outdir out/ legacy.doc
libreoffice --headless --convert-to pptx --outdir out/ legacy.ppt

multype2txt -i out/legacy.docx -o legacy.md
```

`--images-dir` likewise only works for OOXML: although the image bytes of `.doc` / `.ppt` can be extracted by the underlying library, the IR carries no position or description to anchor them to, so they are **neither exported nor reported** — this is the current intended behavior within "best effort" support, not a dependable image-export path.

### The `.xls` SST defect (garbled output)

The shared string table (SST) of `.xls` may be split across `CONTINUE` records. Per [MS-XLS] §2.5.293, when a string is cut at a boundary, the first byte of the `CONTINUE` record is a **new encoding flag** (which may switch the remaining characters from Latin-1 to UTF-16LE), not character data. The underlying library simply **concatenates** these records and reads sequentially, so that flag byte is consumed as a character and every offset after it is wrong.

Measured on `samples/demo.xls` (65562 rows):

| Conversion path | Corrupted rows | Control characters |
|-----------------|----------------|--------------------|
| Direct `.xls` conversion | 30 rows (rows 51-80) | 82134 (including 76215 NULs) |
| Convert to `.xlsx` first | 0 | 0 |

The corrupted rows contain misaligned raw record bytes (half UTF-16LE URLs, record headers, NULs); they appear as garbled text in terminals and editors, and `file` classifies the output as binary.
**This is a parser-level defect, not a CSV serialization issue** — `-f text` through the IR path is affected equally.

`office_oxide` 0.1.12+ fixes the defect, but the `DocumentIR.defined_names` field introduced in the same batch is still not adapted by pdf_oxide (0.3.73 - 0.3.78 all fail to compile with a missing field when constructing the IR), while 0.1.4 - 0.1.11 lack the fix; upgrading would require forking one of them, which was evaluated and rejected.

**If you see garbled output, convert to `.xlsx` first** — OOXML is XML and shared strings go through a different parsing path:

```bash
libreoffice --headless --convert-to xlsx --outdir out/ dirty.xls
multype2txt -i out/dirty.xlsx -o dirty.csv
```

### PDF table rebuilding

A PDF carries no table semantics, only per-word coordinates. By default
(`--pdf-tables=auto`) pages with a **detected table** are rebuilt from word
bounding boxes and font sizes into a column-aligned plain-text grid: numeric
columns right-aligned, row-spanning cells left empty, column widths taken
from the source geometry alone. Pages without a detected table keep the
reading-order baseline word for word, so enabling this never affects body-text
pages.

The decision chain relies on dimensionless facts only — no point thresholds
tuned for one particular sample:

| Stage | Criterion |
|-------|-----------|
| Rows | vertical projection overlap, plus a "same column, different baseline" constraint that stops sparse numeric tables from bleeding between rows |
| Table rows | at least one ≥1 em inter-column gap in the row |
| Blocks | consecutive table rows |
| Column boundaries | vertical whitespace bands that a majority of rows (≥60%) leave empty, ≥1 em wide, crossed by no word |
| Alignment | copies the source geometry: a stabler right edge means right-aligned, content type is irrelevant |
| Self-check | character conservation plus line widths within the physical bound; any failure reverts the whole page to the baseline |

- Rotated pages are corrected through font metrics instead of guessed
  orientation; pseudo-bold text (the same glyph drawn twice) is deduplicated
  during normalization; inter-word spacing is split at `0.1 em` — below that
  it is a split word, and the measured lower bound for a real space is
  0.18 em.
- **Never a half-finished result**: if rebuilding or the self-check fails, the
  whole page reverts to the baseline and a warn log is written.

**Conservative exit**: for critical numeric documents that must not be
re-flowed, `--pdf-tables=off` disables coordinate rebuilding, producing PDF
output identical to before the feature existed.

### PDF image export

PDF defaults to plain text, so images have nowhere to go. `--images-dir` works for PDF: it scans each page's content stream, writes images to disk, and appends the same placeholder style as Markdown output at the **end of that page's text** — a PDF has no heading or list structure to attach placeholders to, and the page end is the only position that neither interrupts the text nor loses page ownership:

```text
...end of that page's text...
![image1](report-imgs/img-0001.png)
```

Page boundaries do not change: the placeholder is inserted before the form-feed character, so the page count is identical with and without the option.

**Best effort per image, failures annotated honestly.** PDFs commonly contain inline images (`BI...ID...EI`); the underlying library refuses to decode them when the `/ColorSpace` entry is missing (which the specification allows). Such images are not written to disk and get no placeholder — instead, a line is written at the end of that page and an error is logged:

```text
[image 3 export failed: Image error: Image missing /ColorSpace]
```

This neither lets one image destroy the whole document body, nor becomes a silent failure indistinguishable from "the document had no images". Once the underlying library is fixed, images will land on disk automatically with no change needed here.

### CSV conventions

Multiple worksheets cannot be expressed in a single CSV, so they are separated by `#` comment lines carrying metadata:

```text
# sheet: Sales Detail  rows=1200 cols=8  merged=2
# merged: A1:C1
Date,Product,Qty,Amount
2024-05-01,Item A,3,120
```

- Lines starting with `#` are **metadata, not data**; strict CSV parsers must skip them themselves.
- The data area follows RFC 4180 escaping; columns are aligned to the widest row in the sheet, and empty rows are padded to their real row numbers.
- Cells export only **computed values**: dates as ISO 8601, error values as `#REF!` etc., booleans as `TRUE`/`FALSE`.
- `.xls` (BIFF8) parsing does not retain merged cells, so that format emits no `# merged` line.

### Known limitations

- **Image content is never parsed**, only placed (`--images-dir` writes the bytes to disk).
- `--images-dir` applies to **Markdown output (`.docx` / `.pptx`) and PDF input**: it does nothing for `.doc` / `.ppt` (no export and no error, see [Limitations of legacy binary formats](#limitations-of-legacy-binary-formats)), and errors out for CSV output.
- **`.xls` may produce garbled output**: shared strings misalign across `CONTINUE` boundaries, mixing in NULs and control bytes (30 of 65562 rows measured), and `-f text` is affected too; convert to `.xlsx` first — see [The `.xls` SST defect](#the-xls-sst-defect-garbled-output).
- **PDF inline images may fail to export**: the underlying library refuses to decode inline images missing `/ColorSpace`, which are annotated `[image N export failed: ...]` instead of written to disk — see [PDF image export](#pdf-image-export).
- **`.xlsb` is not supported**: the underlying `rxlsb` has data-correctness defects (dropped column numbers, lost formula values); refusing is better than emitting misaligned data.
- PDF **tables** are restored as an aligned plain-text grid (borderless
  tables and dense numeric matrices included), but this is still "plain
  text": no Markdown syntax, and row-spanning cells show up as empty cells on
  the following rows. So `-f markdown/csv` still errors for PDF.
- Every PDF table criterion is dimensionless (em multiples of the font size,
  row ratios, row counts) — no point threshold tuned for one document;
  rotated pages are corrected through font metrics rather than guessed
  orientation; inter-word spacing is split at `0.1 em` (below that it is a
  split word; the measured lower bound for a real space is 0.18 em);
  pseudo-bold duplicate drawing is deduplicated during normalization.
  **Known boundaries**: with two header levels of different column counts,
  alignment only holds at the sub-column level; tables whose cells wrap long
  text across columns (e.g. an appendix rating table) may still merge
  columns; two-column body text is restored as a two-column grid; tables
  spanning pages are not stitched; labels inside diagrams are re-flowed as
  multi-column structures; on a `/Rotate` page, if the library draws a whole
  numeric column as one text run, its bounding box is the union of the run
  and an individual value may land on a neighbouring row (content is never
  lost, position may shift).
- Use `--pdf-tables=off` where zero re-flow matters (critical numeric
  documents): coordinate rebuilding is disabled and PDF output matches
  exactly what it was before the feature.
- PDF **formulas are not restored**: superscripts and subscripts are scattered
  small words in coordinate space, so the rebuilt result may be denser than
  the reading-order stream yet still unreadable (e.g. a drifting `KIRB`
  subscript). This is the granularity limit of the underlying word split —
  no characters are lost, but positions can be locally wrong.
- Chart data is not exported — charts are "images" and out of scope for this tool.

## Install & build

Requires Rust 1.88+.

```bash
# Debug build
cargo build

# Release build (artifact at target/release/multype2txt)
cargo build --release
```

Or run directly:

```bash
cargo run -- -i samples/demo.docx
```

## Usage

```text
multype2txt -i <input> [-o <output>] [-f <format>] [--pdf-tables <MODE>] [--images-dir <dir>]
```

### Options

| Option | Description |
|--------|-------------|
| `-i, --input <FILE>` | Required. Input document path. |
| `-o, --output <FILE>` | Optional. Output file path; defaults to standard output. |
| `-f, --format <FMT>` | Optional. `auto` (default, decided by input type) / `markdown` / `csv` / `text`. |
| `--pdf-tables <MODE>` | Optional. PDF table rebuilding: `auto` (default) restores table pages from coordinate geometry, `off` always emits linear reading order (identical to before the feature). Has no effect on non-PDF input. |
| `--images-dir <DIR>` | Optional. Export image bytes to this directory; placeholder URLs point at the actual files. Effective for Markdown output (`.docx` / `.pptx`) and PDF input; has no effect on `.doc` / `.ppt`; errors for CSV output. |
| `-v, --verbose` | Optional. Also log diagnostics to stderr: `-v` for info, `-vv` for debug. By default (without `-v`) logs go only to the log file at level warn. |
| `-h, --help` | Print help. |
| `-V, --version` | Print version. |

### Log and data isolation

- The conversion result appears only on standard output (or in the file given by `-o`).
- Diagnostic logs go **only to the log file** `/tmp/<input-filename>.conv.err.log`
  (e.g. `/tmp/demo.xlsx.conv.err.log`); neither the terminal nor stdout ever
  sees them. The file is named after the input, and re-converting the same
  input **overwrites** the old file, keeping only the most recent run.
- Default level is warn: **no warnings or errors means no log file** — after a
  clean conversion the temp directory is left untouched.
- When the conversion produced warnings or errors, one line is appended to
  stderr after all output:

  ```text
  warn: this conversion had some issues, see the log at /tmp/report.pdf.conv.err.log
  ```

- With `-v` and above, logs are **also** streamed to stderr (console + file)
  for live observation; the hint line is then suppressed — the log is already
  visible, no need to point at it.
- To avoid logs being enabled by external environments, the program never
  reads `RUST_LOG`; the level is controlled entirely by `-v`.

### Examples

```bash
# docx to Markdown on standard output
multype2txt -i report.docx

# xlsx to CSV on standard output
multype2txt -i sheet.xlsx

# Write to a file
multype2txt -i report.docx -o report.md

# Fall back to plain text (drop all structure)
multype2txt -f text -i report.docx -o report.txt

# Export image bytes; placeholders point at the real files
multype2txt --images-dir report-imgs -i report.docx -o report.md

# PDFs export images too, placeholders at the end of each page
multype2txt --images-dir report-imgs -i report.pdf -o report.txt

# Critical numeric documents that must not be re-flowed: disable PDF table rebuilding
multype2txt --pdf-tables=off -i report.pdf -o report.txt

# Legacy .doc has weak structure: convert to .docx first (recommended)
libreoffice --headless --convert-to docx --outdir out/ legacy.doc
multype2txt -i out/legacy.docx -o legacy.md

# In a pipe (result and diagnostics stay separate)
multype2txt -i annual.pptx | head -n 50

# Batch conversion
for f in docs/*.docx; do
    multype2txt -i "$f" -o "${f%.docx}.md"
done

# Turn on diagnostic logging when debugging (extra stderr, does not affect results)
multype2txt -v -i report.pdf -o report.txt
```

### Exit codes

| Code | Meaning |
|------|---------|
| `0` | Conversion succeeded, or the downstream closed the pipe early (`\| head`) |
| `1` | Conversion failed (unsupported format, incompatible input/output pair, corrupted file, write failure, etc.) |
| `2` | Command-line argument error |

## Project structure

```text
src/
  main.rs              # CLI entry: argument parsing, output dispatch, exit codes
  lib.rs               # Library entry, exports convert_file / convert_with etc.
  error.rs             # ConvertError: unified error type
  output.rs            # OutputFormat / ConvertOptions / ImagePolicy
  converter/
    mod.rs             # Dispatch by extension, choose target output format
    office.rs          # Six Office formats -> open document, hand to render layer
    pdf.rs             # PDF -> plain text (baseline reading order + coordinate table rebuilding)
    pdf_table/         # PDF table rebuilding: the coordinate-geometry decision chain
      mod.rs           #   orchestration: normalize -> rows -> blocks -> columns -> grid -> verify
      word.rs          #   word geometry normalization (font-metric correction), spacing decision
      lines.rs         #   row clustering, table/body row split, table-of-contents row detection
      columns.rs       #   column detection: a vertical whitespace band no word crosses is a column boundary
      grid.rs          #   grid assembly, alignment copied from source, rendering
      verify.rs        #   self-check: character conservation + bounded line width
      profile.rs       #   every tunable (dimensionless, each with its source)
  render/
    mod.rs             # DocumentRenderer trait + renderer factory + plain text impl
    markdown.rs        # Self-written IR -> Markdown renderer
    grid.rs            # SheetSource trait + xlsx/xls grid adapters + CSV serialization
tests/
  convert.rs           # End-to-end tests against samples/ (pipeline + dispatch + errors + image export)
  data/pdf/            # Word coordinates of real pages (TSV) for table-rebuild unit tests and regression gates
examples/
  structure_probe.rs   # Structure probe: quantifies plain-text vs structured output
  pdf_words_probe.rs   # Word-coordinate probe / test-data generator (TSV, see tests/data/pdf/)
samples/               # Small sample documents
```

Parsing and rendering are separated: `converter` only opens the document and picks the target format; `render` produces the text.
Adding a new output format only requires implementing `DocumentRenderer` and adding a branch to the factory — dispatch logic stays untouched.

## Development

```bash
cargo fmt
cargo clippy --all-targets
cargo test
```

### Structure probe

Quantifies the structural information gap between `plain_text()` and
`to_markdown()` / `to_html()`, to judge how much upgrading the output from
plain text to structured text actually buys:

```bash
cargo run --example structure_probe
```

Output goes through the log (stderr) and contains per-format character counts,
markdown structure marker statistics, image-placeholder fallback counts, and a
markdown preview of the docx.

## License

Apache License 2.0, see [LICENSE](LICENSE).
