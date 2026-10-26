# Changelog

[中文](CHANGELOG.md)

All notable changes to this project are documented in this file.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
versioning follows [Semantic Versioning](https://semver.org/).

## [0.3.0] - 2026-10-26

### Added

- **PDF tables rebuilt from coordinate geometry**: a PDF has no table
  semantics, only per-word coordinates. By default (`--pdf-tables=auto`)
  pages that contain a detected table are rebuilt from word bounding boxes
  and font sizes into a column-aligned plain-text grid (numeric columns
  right-aligned, row-spanning cells left empty); pages without tables keep
  the reading-order baseline, so there is no regression.
  The decision chain uses dimensionless facts only: rows = vertical
  projection overlap, plus "same column, different baseline" (which stops
  sparse numeric tables from bleeding between rows); table rows = at least
  one ≥1 em inter-column gap in the row; blocks = consecutive table rows;
  column boundaries = vertical whitespace bands left empty by a majority of
  rows (≥60%), at least 1 em wide, crossed by no word; alignment copies the
  source geometry (a stabler right edge means right-aligned); self-checks
  require character conservation and line widths within the physical bound —
  any failure reverts the whole page to the baseline.
- **`--pdf-tables=auto|off`**: critical numeric documents can fall back to
  linear reading order (`off`) in one flag, producing output identical to
  before table rebuilding existed.
- Word-coordinate probe `examples/pdf_words_probe.rs`, plus the real-page
  word coordinates under `tests/data/pdf/` used by unit tests and the
  whole-document regression gate for table rebuilding.
- **Diagnostics fully isolated from output** (log4rs, dual target): by
  default logs go only to `/tmp/<input-filename>.conv.err.log` at level
  warn — no warnings or errors means no file is left behind; re-converting
  the same input deletes the old file first, so the log always reflects
  only the most recent run. When warnings or errors occurred, a single
  hint line is printed to stderr after all output, pointing at the log
  path; with `-v` logs are additionally mirrored to stderr
  (console + file, info / debug) and the hint line is suppressed — the
  log is already visible in real time. Failure reasons now go through
  `eprintln!`, so they remain visible on the terminal in default mode.
  The existing rule of never reading `RUST_LOG` is unchanged.

### Fixed

- **PDF table rebuilding converged against real documents, item by item**:
  - row clustering gained a "same column, different baseline" hard
    constraint: in sparse numeric tables adjacent rows sit close on x and
    vertical overlap alone merged them into the row above (page 194 read
    81% / 33% on the wrong row in testing);
  - numeric tokens are re-joined: split values (`117.` + `95%`,
    `1` + `31.30%`) merge at gaps <0.1 em, while gaps ≥0.25 em between
    independent numbers never get glued;
  - pseudo-bold deduplication: the same glyph drawn twice (`fairfair`)
    collapses to one entry when the text matches and the boxes nearly
    coincide;
  - column boundaries switched to majority voting (support ≥60%) and are
    shaped from multi-word rows only, plus a leading two-cluster split that
    catches long text columns being merged;
  - the cross-path gate now also caps extra characters: page-number footers
    are allowed, but more than 20% extra is treated as duplicated drawing
    and reverted (a 1% gate rejected every table in the tested document);
  - the inter-word space threshold went from 0.25 em to 0.1 em: adjacent
    words in the stream have only two origins — real spaces (0.18~2.0 em
    measured) and split words (≤0.05 em) — and 0.25 em glued tightly set
    documents into `Documentationrequirements`;
  - rotated pages are corrected through font metrics (a box height >1.3 em
    is untrustworthy) instead of guessing orientation, and no longer
    degrade into one giant line.
- **Addressed 10 findings from an external code review**, around the
  principle that a risk documented in a comment must also become an
  assertion or a log call:
  - EMF sniffing now validates the `dSignature` at offset 40, so the
    4-byte prefix `01 00 00 00` is no longer misidentified as EMF;
  - the PDF page-alignment contract (segment count == page count) is
    verified in production and returns `ImageExport` on mismatch,
    so placeholders are never inserted on the wrong page;
  - list continuation lines are indented by marker width (`- ` = 2,
    `1. ` = 3 columns), fixing ordered-list continuations being parsed
    as new paragraphs by strict parsers;
  - Windows backslash paths are normalized to `/` in placeholder URLs;
  - out-of-range xlsx shared-string indexes no longer collapse to an
    empty string silently — they become an error value plus a warn log;
  - the SVG detection scan window was widened from 1 KiB to 8 KiB;
  - the `format_number` scientific-notation boundary, the unreachable
    `Source::Plain` branch, and table-escaping limits are documented.

### Documentation

- AGENTS.md merged with the Rust template and gained the design
  constraint "the boundary between logs and output must not be inverted",
  plus the dimensionless criteria and prohibitions of PDF table rebuilding;
- README updated with log isolation, PDF table rebuilding (including
  `--pdf-tables=off`) and its known boundaries;
- the AI skill was rewritten from the LLM usage perspective: three opening
  rules, a per-input-format decision table, a fixed dump → spot-check →
  slice workflow, and the diff double-run rules for PDF.

## [0.2.0] - 2026-10-09

### Added

- **Structured output dispatched by input type** (three product lines):
  - `.docx` / `.pptx` to Markdown: headings, lists, tables, links,
    footnotes and image placeholders; a self-written IR-to-Markdown
    renderer (merging adjacent same-style runs, real ordered-list
    numbering, link/emphasis order `[**text**](url)`);
  - `.xlsx` / `.xls` to CSV: **full** extraction of computed cell values,
    reading the native grid instead of the IR to guarantee column
    alignment and row completeness, multiple sheets separated by
    `# sheet:` comment lines with merge metadata;
  - `.pdf` to plain text: form-feed characters (`U+000C`) between pages
    preserve page boundaries;
  - `-f` overrides the automatic mapping; incompatible combinations
    (e.g. CSV for PDF) fail fast instead of degrading silently.
- **PDF image export**: `--images-dir` scans each page's content stream,
  writes image bytes to disk and appends a placeholder at the end of that
  page without changing page boundaries; a failing image is honestly
  annotated as `[image N export failed: ...]` instead of aborting.
- **Structure probe** `examples/structure_probe.rs`: quantifies the
  structural information gap between plain text and structured output.
- AI skill `skills/multype2txt/SKILL.md`: teaches agents to convert
  documents to structured text with zero token cost, including legacy
  format warnings and the xls garbled-output workaround.

### Documentation

- Legacy `.doc` / `.ppt` limitations: measured structure-line comparison
  and the recommendation to convert to OOXML first;
- Root cause, measurements and the "convert to `.xlsx` first" workaround
  for the `.xls` SST defect (CONTINUE boundary misalignment causing
  garbled output).

### Changed

- Sample directory renamed to `samples/`, with added `.doc` / `.ppt` /
  `.xls` / `.xlsb` samples.

## [0.1.0] - 2026-09-27

### Added

- Extracted from hydrakiller as a standalone command-line tool
  `multype2txt`: converts six Office formats (`.doc` / `.docx` / `.ppt` /
  `.pptx` / `.xls` / `.xlsx`) and `.pdf` to plain text, available both as
  a CLI and as a library; pure Rust, no C/C++ dynamic libraries.
- Results go to standard output or an `-o` file; BrokenPipe (`| head`)
  is treated as a normal exit.
- First version of the AI skill.

### Fixed

- Logs pinned to stderr and disabled by default (errors only) so they
  cannot pollute the data stream; `RUST_LOG` is deliberately ignored.

[0.3.0]: https://github.com/itcraft-cn/multype2txt/compare/v0.2.0_20261009...v0.3.0_20261026
[0.2.0]: https://github.com/itcraft-cn/multype2txt/compare/v0.1.0_20260927...v0.2.0_20261009
[0.1.0]: https://github.com/itcraft-cn/multype2txt/releases/tag/v0.1.0_20260927
