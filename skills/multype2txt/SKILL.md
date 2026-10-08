---
name: multype2txt
description: 用本地二进制将 Office/PDF 文档零 token 消耗地转换为结构化文本（docx 转 Markdown、xlsx 转 CSV、pptx 转结构文字、pdf 转纯文本）。触发场景：(1) 需要读取或分析 doc/docx/ppt/pptx/xls/xlsx/pdf 文件内容，(2) 用户提到 "转换文件为 txt"、"文档转文本"、"docx to markdown"、"xlsx to csv"、"pdf to text"、"office to text"、"解析文档"，(3) 需要把二进制文档交给模型处理但不想消耗 token 或直接读会乱码，(4) 批量把文档目录转换为文本。优先使用本技能而不是直接 cat/读取二进制文档。
---

# multype2txt 文档转文本技能

## 作用

调用本地命令 `multype2txt`，把 Office 与 PDF 文档转换为对应格式的文本。

- 转换在本地完成，**不消耗任何 token**：模型不需要读取二进制内容，也不需要请求
  外部服务。
- 转换结果写入 stdout 或文件；模型按需只读取其中需要的那一段，进一步节省上下文。
- 覆盖格式：`doc` `docx` `ppt` `pptx` `xls` `xlsx` `pdf`。

## 默认输出格式

按输入类型自动选择，可用 `-f` 覆盖：

| 输入 | 默认输出 | 说明 |
|------|----------|------|
| `doc` `docx` | Markdown | 标题、列表、表格、超链接保留为 Markdown 语法 |
| `xls` `xlsx` | CSV | **全量**单元格数据，多工作表以 `# sheet:` 注释行分隔 |
| `ppt` `pptx` | Markdown | 幻灯片页、标题、列表层级、演讲者备注，页间以 `---` 分隔 |
| `pdf` | 纯文本 | 无结构可还原，`-f markdown/csv` 会直接报错 |

## 为什么不直接读取文档

`.docx/.xlsx/.pptx/.pdf` 等是二进制或压缩包格式，直接读取会得到乱码或极长噪声，
既浪费 token 也无法理解。正确做法是先用本技能转换，再读取文本。

对 xlsx 尤其重要：原文件动辄数百万字符的二进制噪声，转换成 CSV 后才能被直接理解。

## 定位可执行文件

按以下顺序选择其一：

1. 已在 PATH 中：直接使用 `multype2txt`。
2. 在本仓库内：先构建，再使用产物路径。
   ```bash
   cargo build --release
   ./target/release/multype2txt --help
   ```
3. 长期使用可安装到用户目录：
   ```bash
   cargo install --path .
   # 或
   cp target/release/multype2txt ~/.local/bin/
   ```

下文统一以 `multype2txt` 表示所选路径。

## 用法

```text
multype2txt -i <输入文件> [-o <输出文件>] [-f <格式>] [--images-dir <目录>] [-v|-vv]
```

| 参数 | 说明 |
|------|------|
| `-i, --input <FILE>` | 必填。输入文档路径。 |
| `-o, --output <FILE>` | 可选。输出文件路径；缺省写入标准输出。 |
| `-f, --format <FMT>` | 可选。`auto`（默认）/ `markdown` / `csv` / `text`。 |
| `--images-dir <DIR>` | 可选。把图片字节导出到该目录，占位符指向实际文件。仅 Markdown 输出有效。 |
| `-v, --verbose` | 可选。打开诊断日志（`-v`=info，`-vv`=debug），日志只写 stderr。 |
| `-h, --help` / `-V, --version` | 帮助 / 版本。 |

### 基本示例

```bash
# docx 输出 Markdown（适合直接交给模型阅读，保留标题与层级）
multype2txt -i report.docx

# xlsx 输出 CSV 到文件，后续反复读取不再重复转换
multype2txt -i sheet.xlsx -o sheet.csv

# 退回纯文本（只要正文、不要结构时）
multype2txt -f text -i report.docx -o report.txt

# 输出到 stdout 并只取前一段，节省上下文
multype2txt -i report.pdf | head -n 100

# 需要看图时把图片字节另存，占位符会指向真实文件
multype2txt --images-dir report-imgs -i report.docx -o report.md

# 批量转换整个目录
for f in docs/*.docx docs/*.pdf; do
    multype2txt -i "$f" -o "${f%.*}.md"
done
```

## 推荐工作流

1. 判断输入是否为二进制文档格式；是则使用本技能，不要直接读取原文。
2. 小文件可直接输出到 stdout 并读取；大文件建议先 `-o` 落盘。
3. 对落盘的文本再用检索/分段读取，只把需要的片段带入上下文。
4. xlsx 走 CSV 后**不要在转换环节做裁剪**：略读、采样、摘要是你的职责，
   转换器只负责把数据完整交出来。
5. 批量处理时用循环逐一转换，避免单个文档占用过多上下文。

## 输出约定

- 转换结果只出现在 stdout（或 `-o` 文件）中。
- 诊断日志只写 stderr，默认关闭（仅 error）；需要排查时加 `-v`。
- 该命令不读取 `RUST_LOG`，外部环境变量不会导致日志混入数据。
- PDF 按页面阅读顺序抽取正文，页与页之间以换行符 `U+000C`（`\f`）分隔，
  用于保留分页边界，不产生可见的标记文本。
- CSV 中以 `#` 开头的行是元信息（工作表名、行列数、合并区域），**不是数据**；
  严格的 CSV 解析器需跳过这些行。

## 图片处理

**本工具不解析图片内容，这是明确的能力边界。**

- Markdown 输出中，图片表现为占位符 `![图片1: 系统架构图](image://1)`：
  有描述时填入描述，无描述时只有编号。
- 需要真实图片时加 `--images-dir`，字节会落盘，占位符 URL 指向实际文件，
  由你自行决定是否读取。
- Excel 输出中图片完全忽略、不留占位符；PPT 图表数据同样不导出（属于「图」）。

## 退出码

| 退出码 | 含义 | 处理建议 |
|--------|------|----------|
| `0` | 转换成功，或下游提前关闭管道（`\| head`） | 继续后续处理 |
| `1` | 转换失败 | 检查文件是否存在、格式是否受支持、输入输出格式是否兼容、是否损坏；加 `-v` 查看原因 |
| `2` | 命令行参数错误 | 检查 `-i/-o/-f` 参数 |

## 注意事项

- 不支持的格式：`xlsb`、`csv`、`txt`、`rtf`、`odt` 等会返回退出码 1。
  txt 本身已是文本，无需转换。
  - `xlsb` 暂不支持是刻意的：依赖的 `rxlsb` 存在列错位与公式值丢失缺陷，
    在修复前宁可拒绝也不产出错位的数据。
- 请求了输入无法提供的格式会直接失败（例如对 PDF 要 CSV、对文档要 CSV），
  报错信息会说明原因——这是刻意设计，避免拿到「看起来像、其实不是」的数据。
- `--images-dir` 只对 Markdown 输出有效；对 CSV/PDF 使用会报错而不是静默忽略。
- 加密（受密码保护）的 OOXML 文档不支持解密，会明确报错。
- 扩展名大小写不敏感；工具会结合文件头签名识别真实格式，扩展名写错多可容忍。
- 不要把 `-o` 的文件名与输入放在同一路径覆盖源文件；如需保留原件，输出到不同目录。
- 图片文件按 `img-0001.png` 顺序编号，**建议每个文档使用独立的 `--images-dir`**，
  复用同一目录会覆盖同名文件。

## 排障

```bash
# 查看帮助
multype2txt --help

# 打开诊断日志定位失败原因
multype2txt -v -i broken.pdf -o out.txt

# 确认版本
multype2txt --version

# 想确认输出格式映射是否符合预期
multype2txt -f markdown -i sheet.xlsx | head -n 20
```
