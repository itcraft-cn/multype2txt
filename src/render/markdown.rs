//! Markdown 渲染器：把 Office 文档的中间表示（IR）渲染为 Markdown。
//!
//! # 为什么自研而不直接用底层库的 `to_markdown()`
//!
//! 库自带的渲染器面向「文档再生产」，存在三处与本工具目标冲突的行为：
//!
//! 1. 相邻的同样式片段各自包裹标记，输出 `**甲****乙**`，Markdown 解析器
//!    会读成错误的强调边界；这里先合并同样式片段再包裹；
//! 2. 有序列表的编号一律写成 `1.`，人读会数错、模型读会误判；这里按真实
//!    序号（含 `start_number` 起始值）递增；
//! 3. 图片一律渲染成 `![]()`，描述与字节双双丢失；这里保留描述，并按
//!    [`ImagePolicy`] 决定是否把字节另存为文件。
//!
//! # 图片处理
//!
//! 本工具**不解析图片内容**，这是显式的能力边界。占位符只回答两个问题：
//! 「这里有一张图」「它叫什么」。是否要看图、能不能看图，由阅读方决定——
//! 需要时用 `--images-dir` 把字节落盘，占位符 URL 指向真实文件即可。
//!
//! # 表格
//!
//! Markdown 表格无法表达合并单元格，静默摊平会破坏行列语义。因此：
//! 无合并时输出标准 Markdown 表格；一旦出现合并，整表改用 HTML
//! `<table>`（Markdown 允许内嵌 HTML，`colspan`/`rowspan` 得以保留）。

use std::path::Path;

use log::warn;
use office_oxide::ir::{
    CodeBlock, DocumentIR, Element, Heading, Image, InlineContent, List, Note, Paragraph, Section,
    Table, TableCell, TextSpan,
};

use crate::error::ConvertError;
use crate::image::{join_url, write_image_bytes};
use crate::output::{ImagePolicy, OutputFormat};
use crate::render::{DocumentRenderer, Source};

/// 将 Office 文档的 IR 渲染为 Markdown 文本。
///
/// 渲染器自身不持有解析状态，可安全复用；单次渲染的进度（图片序号等）
/// 记录在内部 [`State`] 中，随调用创建。
#[derive(Debug, Clone)]
pub struct MarkdownRenderer {
    /// 图片处理策略。
    images: ImagePolicy,
}

impl MarkdownRenderer {
    /// 用给定的图片策略构造渲染器。
    pub fn new(images: ImagePolicy) -> Self {
        Self { images }
    }
}

impl DocumentRenderer for MarkdownRenderer {
    fn format(&self) -> OutputFormat {
        OutputFormat::Markdown
    }

    fn render(&self, source: &Source<'_>) -> Result<String, ConvertError> {
        match source {
            // 本来就是文本的输入（PDF）原样透传：PDF 没有结构可还原，
            // 任何「结构化」改写都只是凭空编造。
            Source::Plain(text) => Ok((*text).to_string()),
            Source::Office(doc) => {
                let ir = doc.to_ir();
                let mut state = State {
                    policy: &self.images,
                    image_seq: 0,
                };
                render_document(&ir, &mut state)
            }
        }
    }
}

/// 单次渲染的可变状态。
struct State<'a> {
    /// 图片处理策略（借用渲染器配置，避免克隆）。
    policy: &'a ImagePolicy,
    /// 已输出的图片计数，同时用作占位符编号与文件序号。
    image_seq: usize,
}

// ---------------------------------------------------------------------------
// 文档与节
// ---------------------------------------------------------------------------

/// 渲染整个 IR：各节之间以主题分隔线 `---` 连接。
///
/// 空节会被跳过，避免出现连续分隔线。
fn render_document(ir: &DocumentIR, state: &mut State<'_>) -> Result<String, ConvertError> {
    let mut sections = Vec::new();
    for section in &ir.sections {
        let text = render_section(section, state)?;
        if !text.trim().is_empty() {
            // 去掉块尾残留的换行：拼接时会再补两个，留着会多出空行，
            // 让 Markdown 出现「孤立段落」的错觉。
            sections.push(text.trim_end().to_string());
        }
    }
    Ok(sections.join("\n\n---\n\n"))
}

/// 渲染单个节。
///
/// 节标题（`Section::title`）只在没有被首个标题元素重复表达时才输出：
/// 幻灯片的标题既是 `title` 又是首元素的 Heading，两者都写会重复一遍。
fn render_section(section: &Section, state: &mut State<'_>) -> Result<String, ConvertError> {
    let mut parts = Vec::new();

    if let Some(title) = section.title.as_deref() {
        let title = title.trim();
        if !title.is_empty() && !has_lead_heading(&section.elements, title) {
            parts.push(format!("## {title}"));
        }
    }

    for element in &section.elements {
        let text = render_element(element, state)?;
        if !text.trim().is_empty() {
            // 同上：块间以空行分隔，块自身不带尾换行。
            parts.push(text.trim_end().to_string());
        }
    }

    Ok(parts.join("\n\n"))
}

/// 判断首个有效元素是否为标题，且其文字正是给定的节标题。
///
/// 跳过空段落（底层库在标题前可能插入空段），遇到第一个有内容的元素即可
/// 判定：若它不是标题，则节标题没有被表达过，需要单独输出。
fn has_lead_heading(elements: &[Element], title: &str) -> bool {
    for element in elements {
        match element {
            Element::Heading(heading) => {
                return inline_plain(&heading.content).trim() == title;
            }
            Element::Paragraph(paragraph) if inline_is_empty(&paragraph.content) => continue,
            _ => return false,
        }
    }
    false
}

// ---------------------------------------------------------------------------
// 块级元素
// ---------------------------------------------------------------------------

/// 渲染一个块级元素。
///
/// 返回的文本不带首尾空行，由调用方负责块间分隔。
#[allow(clippy::too_many_lines)]
fn render_element(element: &Element, state: &mut State<'_>) -> Result<String, ConvertError> {
    match element {
        Element::Heading(heading) => Ok(render_heading(heading, state)),
        Element::Paragraph(paragraph) => Ok(render_paragraph(paragraph, state)),
        Element::Table(table) => render_table(table, state),
        Element::List(list) => render_list(list, 0, state),
        Element::Image(image) => Ok(render_image(image, state)?),
        Element::ThematicBreak => Ok("---".to_string()),
        Element::CodeBlock(block) => Ok(render_code_block(block)),
        // 文本框与脚注/尾注的**正文属于文字**，必须保留。
        Element::TextBox(boxed) => render_blocks(&boxed.content, state),
        Element::Footnote(note) => render_note(note, state),
        Element::Endnote(note) => render_note(note, state),
        // 矢量装饰（分隔线、边框）不含文字，纯文本场景无信息量。
        Element::Shape(_) => Ok(String::new()),
        Element::PageBreak | Element::ColumnBreak => Ok(String::new()),
        // `Element` 标记为 non_exhaustive：新增变体时此处不致编译失败。
        other => {
            warn!("跳过未处理的文档元素: {:?}", std::mem::discriminant(other));
            Ok(String::new())
        }
    }
}

/// 渲染一组块级元素，块间以空行分隔。
fn render_blocks(elements: &[Element], state: &mut State<'_>) -> Result<String, ConvertError> {
    let mut parts = Vec::new();
    for element in elements {
        let text = render_element(element, state)?;
        if !text.trim().is_empty() {
            parts.push(text.trim_end().to_string());
        }
    }
    Ok(parts.join("\n\n"))
}

/// 渲染标题，层级收敛到 1–6。
fn render_heading(heading: &Heading, _state: &mut State<'_>) -> String {
    let level = usize::from(heading.level.clamp(1, 6));
    let text = collapse_inline(&render_inline(&heading.content));
    if text.is_empty() {
        return String::new();
    }
    format!("{} {text}", "#".repeat(level))
}

/// 渲染正文段落（内联样式展开后的单块文本）。
fn render_paragraph(paragraph: &Paragraph, _state: &mut State<'_>) -> String {
    render_inline(&paragraph.content)
}

/// 渲染预格式化代码块。
fn render_code_block(block: &CodeBlock) -> String {
    let language = block.language.as_deref().unwrap_or("").trim();
    let body = block.content.trim_end_matches('\n');
    if body.is_empty() {
        return String::new();
    }
    format!("```{language}\n{body}\n```")
}

/// 渲染脚注/尾注正文，以脚注定义语法 `[^标记]: 正文` 输出。
fn render_note(note: &Note, state: &mut State<'_>) -> Result<String, ConvertError> {
    let body = render_blocks(&note.content, state)?;
    let body = collapse_inline(&body);
    if body.is_empty() {
        return Ok(String::new());
    }
    let marker = note
        .marker
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map_or_else(|| note.id.to_string(), str::to_string);
    Ok(format!("[^{marker}]: {body}"))
}

// ---------------------------------------------------------------------------
// 内联样式
// ---------------------------------------------------------------------------

/// 决定一段文字在 Markdown 中强调方式的样式集合。
///
/// 只保留会改变 Markdown 语法的属性；字体、字号、颜色等排版信息在纯文本
/// 目标下无意义，刻意忽略。
#[derive(Debug, Clone, PartialEq, Eq)]
struct SpanStyle {
    /// 粗体。
    bold: bool,
    /// 斜体。
    italic: bool,
    /// 删除线。
    strikethrough: bool,
    /// 超链接目标。
    hyperlink: Option<String>,
}

impl SpanStyle {
    /// 取出参与 Markdown 样式的字段。
    fn of(span: &TextSpan) -> Self {
        Self {
            bold: span.bold,
            italic: span.italic,
            strikethrough: span.strikethrough,
            hyperlink: span
                .hyperlink
                .as_deref()
                .map(str::trim)
                .filter(|u| !u.is_empty())
                .map(str::to_string),
        }
    }
}

/// 渲染内联内容。
///
/// 关键步骤是**先合并相邻同样式片段再包裹标记**：底层库把一句话拆成多个
/// run，逐个包裹会得到 `**甲****乙**` 这类破坏强调边界的输出。
fn render_inline(content: &[InlineContent]) -> String {
    let mut out = String::new();
    let mut pending: Option<(SpanStyle, String)> = None;

    for item in content {
        match item {
            InlineContent::LineBreak => {
                if let Some((style, text)) = pending.take() {
                    out.push_str(&wrap_style(&style, &text));
                }
                out.push('\n');
            }
            InlineContent::Text(span) => {
                if span.text.is_empty() {
                    continue;
                }
                let style = SpanStyle::of(span);
                match pending.as_mut() {
                    // 相邻且样式一致 → 拼接，稍后一次性包裹。
                    Some((last_style, last_text)) if *last_style == style => {
                        last_text.push_str(&span.text);
                    }
                    _ => {
                        if let Some((last_style, last_text)) = pending.take() {
                            out.push_str(&wrap_style(&last_style, &last_text));
                        }
                        pending = Some((style, span.text.clone()));
                    }
                }
            }
            InlineContent::FootnoteRef(reference) => {
                if let Some((style, text)) = pending.take() {
                    out.push_str(&wrap_style(&style, &text));
                }
                out.push_str(&note_ref_mark(reference));
            }
            InlineContent::EndnoteRef(reference) => {
                if let Some((style, text)) = pending.take() {
                    out.push_str(&wrap_style(&style, &text));
                }
                out.push_str(&note_ref_mark(reference));
            }
            // `InlineContent` 标记为 non_exhaustive：新增变体时先收尾当前
            // 待处理片段，保证强调边界不被破坏，再记日志提示有内容被略过。
            other => {
                if let Some((style, text)) = pending.take() {
                    out.push_str(&wrap_style(&style, &text));
                }
                warn!("跳过未处理的内联元素: {:?}", std::mem::discriminant(other));
            }
        }
    }

    if let Some((style, text)) = pending.take() {
        out.push_str(&wrap_style(&style, &text));
    }

    out
}

/// 为脚注引用生成可见标记。
fn note_ref_mark(reference: &office_oxide::ir::FootnoteRef) -> String {
    match reference.marker.as_deref().map(str::trim) {
        Some(marker) if !marker.is_empty() => format!("[{marker}]"),
        _ => format!("[{}]", reference.note_id),
    }
}

/// 按样式为文本加上 Markdown 强调标记。
///
/// 由内向外依次是删除线、斜体、粗体，链接包在最外面，得到
/// `**~~文本~~**`、`[**文字**](地址)` 这类标准写法——链接文字里的强调
/// 放在方括号内，链接本身才落在圆括号外，两者都不会破坏语法边界。
fn wrap_style(style: &SpanStyle, text: &str) -> String {
    let mut out = text.to_string();
    if style.strikethrough {
        out = format!("~~{out}~~");
    }
    if style.italic {
        out = format!("*{out}*");
    }
    if style.bold {
        out = format!("**{out}**");
    }
    if let Some(url) = &style.hyperlink {
        out = format!("[{out}]({url})");
    }
    out
}

/// 去掉全部样式，取内联内容的纯文本（用于比对与单行折叠）。
fn inline_plain(content: &[InlineContent]) -> String {
    let mut out = String::new();
    for item in content {
        match item {
            InlineContent::Text(span) => out.push_str(&span.text),
            InlineContent::LineBreak => out.push('\n'),
            InlineContent::FootnoteRef(reference) => out.push_str(&note_ref_mark(reference)),
            InlineContent::EndnoteRef(reference) => out.push_str(&note_ref_mark(reference)),
            // non_exhaustive 兜底：未知元素在纯文本比对场景下没有可取文字。
            _ => {}
        }
    }
    out
}

/// 内联内容是否没有任何文字。
fn inline_is_empty(content: &[InlineContent]) -> bool {
    !content.iter().any(|item| match item {
        InlineContent::Text(span) => !span.text.is_empty(),
        _ => false,
    })
}

/// 把多行文本折叠成单行（表格单元格、标题、列表项都必须单行）。
///
/// 连续空白压缩为一个空格，Markdown 表格内的换行改用 `<br>` 表达。
fn collapse_inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for ch in text.chars() {
        if ch == '\n' || ch == '\r' || ch == '\t' || ch == ' ' {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(ch);
    }
    out
}

// ---------------------------------------------------------------------------
// 列表
// ---------------------------------------------------------------------------

/// 渲染列表，`indent` 为嵌套层级（每层 4 空格）。
///
/// 有序列表按真实序号递增（支持 `start_number` 起始），无序列表统一用
/// `-`：Markdown 的 `*`/`+`/`•` 语义相同，统一符号让输出稳定、可比对。
fn render_list(list: &List, indent: usize, state: &mut State<'_>) -> Result<String, ConvertError> {
    let padding = " ".repeat(indent * 4);
    let start = list.start_number.unwrap_or(1);
    let mut out = String::new();

    for (index, item) in list.items.iter().enumerate() {
        let marker = if list.ordered {
            format!("{}. ", start + index as u32)
        } else {
            "- ".to_string()
        };

        let blocks = render_blocks(&item.content, state)?;
        let nested = match &item.nested {
            Some(sub) => render_list(sub, indent + 1, state)?,
            None => String::new(),
        };

        if blocks.trim().is_empty() {
            // 只有子列表的条目：父项自身无文字，但仍需占位以保持层级。
            if nested.is_empty() {
                continue;
            }
            out.push_str(&padding);
            out.push_str(&marker);
            out.push('\n');
        } else {
            let mut lines = blocks.split('\n');
            let first = lines.next().unwrap_or("");
            out.push_str(&format!("{padding}{marker}{}\n", collapse_inline(first)));
            // 同一条目的其余块缩进对齐，避免被解析成新条目。
            for line in lines {
                if line.trim().is_empty() {
                    continue;
                }
                out.push_str(&format!("{padding}  {}\n", collapse_inline(line)));
            }
        }

        out.push_str(&nested);
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// 表格
// ---------------------------------------------------------------------------

/// 渲染表格：有合并单元格时走 HTML，否则走 Markdown 表格。
fn render_table(table: &Table, state: &mut State<'_>) -> Result<String, ConvertError> {
    if table.rows.is_empty() {
        return Ok(String::new());
    }
    if table_has_merge(table) {
        render_table_html(table, state)
    } else {
        render_table_markdown(table, state)
    }
}

/// 表格是否存在合并单元格。
///
/// `col_span`/`row_span` 为 0 或 1 都表示未合并（底层库对无合并单元格
/// 可能写 0），因此以「大于 1」判定。
fn table_has_merge(table: &Table) -> bool {
    table.rows.iter().any(|row| {
        row.cells
            .iter()
            .any(|cell| cell.col_span > 1 || cell.row_span > 1)
    })
}

/// 渲染标准 Markdown 表格。
///
/// 首行作为表头（Markdown 表格必须有表头）；行内单元格数量不足时右侧
/// 补空，保证每行列数一致。
fn render_table_markdown(table: &Table, state: &mut State<'_>) -> Result<String, ConvertError> {
    let column_count = table
        .rows
        .iter()
        .map(|row| row.cells.len())
        .max()
        .unwrap_or(0);
    if column_count == 0 {
        return Ok(String::new());
    }

    let mut rows: Vec<Vec<String>> = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        let mut cells = Vec::with_capacity(column_count);
        for cell in &row.cells {
            cells.push(join_cell_lines_markdown(&render_cell_lines(cell, state)?));
        }
        while cells.len() < column_count {
            cells.push(String::new());
        }
        rows.push(cells);
    }

    let mut out = String::new();
    if let Some(caption) = table
        .caption
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        out.push_str(&format!("*{caption}*\n\n"));
    }

    out.push_str(&format!("| {} |\n", rows[0].join(" | ")));
    out.push_str(&format!("| {} |\n", vec!["---"; column_count].join(" | ")));
    for row in rows.iter().skip(1) {
        out.push_str(&format!("| {} |\n", row.join(" | ")));
    }

    Ok(out.trim_end().to_string())
}

/// 渲染 HTML 表格（仅在需要保留合并单元格时使用）。
fn render_table_html(table: &Table, state: &mut State<'_>) -> Result<String, ConvertError> {
    let mut out = String::from("<table>\n");
    if let Some(caption) = table
        .caption
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        out.push_str(&format!("  <caption>{}</caption>\n", escape_html(caption)));
    }

    for row in &table.rows {
        out.push_str("  <tr>\n");
        for cell in &row.cells {
            let tag = if row.is_header { "th" } else { "td" };
            let mut attributes = String::new();
            if cell.col_span > 1 {
                attributes.push_str(&format!(" colspan=\"{}\"", cell.col_span));
            }
            if cell.row_span > 1 {
                attributes.push_str(&format!(" rowspan=\"{}\"", cell.row_span));
            }
            let content = join_cell_lines_html(&render_cell_lines(cell, state)?);
            out.push_str(&format!("    <{tag}{attributes}>{content}</{tag}>\n"));
        }
        out.push_str("  </tr>\n");
    }

    out.push_str("</table>");
    Ok(out)
}

/// 渲染单元格内容，返回若干**单行**片段。
///
/// 单元格里的段落分界在单行场景下用 `<br>` 表达，因此这里先按空行切分、
/// 再折叠每段；转义交给调用方，因为 Markdown 表格与 HTML 表格的转义规则
/// 不同（前者只需转义管道符，后者需要转义 HTML 元字符，而 `<br>` 恰恰
/// 必须保留为字面标签）。
fn render_cell_lines(cell: &TableCell, state: &mut State<'_>) -> Result<Vec<String>, ConvertError> {
    let body = render_blocks(&cell.content, state)?;
    Ok(body
        .split("\n\n")
        .map(collapse_inline)
        .filter(|part| !part.is_empty())
        .collect())
}

/// 拼接单元格片段为单行（Markdown 表格用，纯文本 + 管道符转义）。
fn join_cell_lines_markdown(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| escape_table_cell(line))
        .collect::<Vec<_>>()
        .join("<br>")
}

/// 拼接单元格片段为单行（HTML 表格用，先转义文本再保留 `<br>` 标签）。
fn join_cell_lines_html(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| escape_html(line))
        .collect::<Vec<_>>()
        .join("<br>")
}

/// 转义 Markdown 表格单元格内的管道符，否则会撑破表格结构。
fn escape_table_cell(text: &str) -> String {
    text.replace('|', "\\|")
}

/// 转义 HTML 文本。
fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 图片
// ---------------------------------------------------------------------------

/// 渲染图片占位符。
///
/// 按 [`ImagePolicy`] 决定 URL 形态；`Export` 模式下把字节落盘，失败会
/// 直接返回错误而不是悄悄退回虚拟 URL——静默降级会让调用方以为图已导出。
fn render_image(image: &Image, state: &mut State<'_>) -> Result<String, ConvertError> {
    if matches!(state.policy, ImagePolicy::Suppress) {
        return Ok(String::new());
    }

    state.image_seq += 1;
    let sequence = state.image_seq;
    let description = image
        .alt_text
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty());
    let alt = match description {
        Some(text) => format!("图片{sequence}: {text}"),
        None => format!("图片{sequence}"),
    };

    let url = match state.policy {
        ImagePolicy::Virtual | ImagePolicy::Suppress => format!("image://{sequence}"),
        ImagePolicy::Export { dir, url_prefix } => {
            let file_name = export_image(dir, sequence, image)?;
            join_url(url_prefix, &file_name)
        }
    };

    Ok(format!("![{alt}]({url})"))
}

/// 把图片字节写入目标目录，返回生成的文件名。
///
/// 命名、扩展名识别与落盘细节统一在 [`crate::image`]，与 PDF 输出共用——
/// 同一个 `--images-dir` 不该有两套文件命名规则。
fn export_image(dir: &Path, sequence: usize, image: &Image) -> Result<String, ConvertError> {
    let bytes = image
        .data
        .as_deref()
        .filter(|bytes| !bytes.is_empty())
        .ok_or_else(|| ConvertError::ImageExport {
            path: dir.display().to_string(),
            message: "文档未提供图片字节".to_string(),
        })?;

    write_image_bytes(dir, sequence, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use office_oxide::ir::{InlineContent, ListItem, TableCell, TableRow};

    fn text(value: &str) -> InlineContent {
        InlineContent::Text(TextSpan::plain(value))
    }

    fn bold(value: &str) -> InlineContent {
        InlineContent::Text(TextSpan {
            text: value.to_string(),
            bold: true,
            ..Default::default()
        })
    }

    /// 用一个只含段落的条目构造列表项。
    fn list_item(value: &str) -> ListItem {
        ListItem {
            content: vec![Element::Paragraph(Paragraph {
                content: vec![text(value)],
                ..Default::default()
            })],
            nested: None,
        }
    }

    /// 用指定图片策略构造渲染状态。
    fn state_with(policy: &ImagePolicy) -> State<'_> {
        State {
            policy,
            image_seq: 0,
        }
    }

    #[test]
    fn merges_adjacent_spans_of_same_style() {
        // 底层库把一句话拆成两个 run，逐个包裹会得到 `**甲****乙**`。
        let rendered = render_inline(&[bold("XNet"), bold(" 组成")]);
        assert_eq!(rendered, "**XNet 组成**");
    }

    #[test]
    fn different_styles_stay_separate() {
        let rendered = render_inline(&[bold("粗"), text(" 常")]);
        assert_eq!(rendered, "**粗** 常");
    }

    #[test]
    fn link_wraps_outside_emphasis() {
        let span = InlineContent::Text(TextSpan {
            text: "官网".to_string(),
            bold: true,
            hyperlink: Some("https://example.com".to_string()),
            ..Default::default()
        });
        assert_eq!(render_inline(&[span]), "[**官网**](https://example.com)");
    }

    #[test]
    fn ordered_list_numbers_increment() {
        let list = List {
            ordered: true,
            items: vec![list_item("第一项"), list_item("第二项")],
            ..Default::default()
        };
        let policy = ImagePolicy::Virtual;
        let mut state = state_with(&policy);
        let out = render_list(&list, 0, &mut state).expect("列表渲染不应失败");
        assert!(out.contains("1. 第一项"), "实际输出: {out}");
        assert!(out.contains("2. 第二项"), "实际输出: {out}");
    }

    #[test]
    fn table_without_merge_is_markdown() {
        let table = Table {
            rows: vec![row(&["标题", "值"]), row(&["a", "b"])],
            ..Default::default()
        };
        let policy = ImagePolicy::Virtual;
        let mut state = state_with(&policy);
        let out = render_table(&table, &mut state).expect("表格渲染不应失败");
        assert!(out.contains("| 标题 | 值 |"), "实际输出: {out}");
        assert!(out.contains("| --- | --- |"), "实际输出: {out}");
    }

    #[test]
    fn merged_table_falls_back_to_html() {
        let mut table = Table {
            rows: vec![row(&["跨列"]), row(&["a", "b"])],
            ..Default::default()
        };
        table.rows[0].cells[0].col_span = 2;
        let policy = ImagePolicy::Virtual;
        let mut state = state_with(&policy);
        let out = render_table(&table, &mut state).expect("表格渲染不应失败");
        assert!(out.contains("<table>"), "实际输出: {out}");
        assert!(out.contains("colspan=\"2\""), "实际输出: {out}");
    }

    #[test]
    fn pipes_are_escaped_in_cells() {
        let table = Table {
            rows: vec![row(&["a|b", "c"])],
            ..Default::default()
        };
        let policy = ImagePolicy::Virtual;
        let mut state = state_with(&policy);
        let out = render_table(&table, &mut state).expect("表格渲染不应失败");
        assert!(out.contains("a\\|b"), "实际输出: {out}");
    }

    #[test]
    fn image_placeholder_carries_description() {
        let image = Image {
            alt_text: Some("系统架构图".to_string()),
            data: None,
            ..Default::default()
        };
        let policy = ImagePolicy::Virtual;
        let mut state = state_with(&policy);
        let out = render_image(&image, &mut state).expect("占位符渲染不应失败");
        assert_eq!(out, "![图片1: 系统架构图](image://1)");
    }

    #[test]
    fn suppressed_image_renders_nothing() {
        let image = Image {
            alt_text: Some("隐藏".to_string()),
            ..Default::default()
        };
        let policy = ImagePolicy::Suppress;
        let mut state = state_with(&policy);
        let out = render_image(&image, &mut state).expect("抑制渲染不应失败");
        assert!(out.is_empty());
    }

    #[test]
    fn heading_level_is_clamped() {
        let heading = Heading {
            level: 9,
            content: vec![text("标题")],
            ..Default::default()
        };
        let policy = ImagePolicy::Virtual;
        let mut state = state_with(&policy);
        assert_eq!(render_heading(&heading, &mut state), "###### 标题");
    }

    fn row(values: &[&str]) -> TableRow {
        TableRow {
            cells: values
                .iter()
                .map(|value| TableCell {
                    content: vec![Element::Paragraph(Paragraph {
                        content: vec![text(value)],
                        ..Default::default()
                    })],
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }
}
