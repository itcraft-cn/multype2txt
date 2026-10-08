# multype2txt

将 Office 与 PDF 文档转换为文本的命令行工具。

按输入格式自动选择输出形态：**Word 转 Markdown、Excel 转 CSV、PPT 转结构+文字、PDF 转纯文本**。
同一条链路既能在脚本与管道中独立使用，也能作为库被其他程序复用。

核心目的是便于 AI 读取这些文档：结构用来替代猜测，全量抽取用来替代猜测性的省略。

## 三条产品线

| 输入 | 默认输出 | 说明 |
|------|----------|------|
| `.docx` | Markdown | 标题、列表、表格、超链接、脚注保留为 Markdown 语法；图片只留占位符 |
| `.doc` | Markdown | **尽力支持**：只有文字与大致标题，列表/表格/样式/图片均无，见 [legacy 二进制格式的局限](#legacy-二进制格式的局限) |
| `.xlsx` / `.xls` | CSV | **全量**抽取单元格计算值，多工作表以注释行分隔；图片完全忽略、不留占位 |
| `.pptx` | Markdown | 幻灯片页、标题、列表层级、演讲者备注；图片只留占位符 |
| `.ppt` | Markdown | **尽力支持**：页与标题保留，列表层级全部丢失，见同上 |
| `.pdf` | 纯文本 | 按阅读顺序抽取正文，页间插入换页符（`U+000C`）；`--images-dir` 可按页导出图片 |

设计取舍：

- **全量抽取，不做略读。** 是否采样、截断、摘要属于阅读方（LLM）的职责，转换器只负责把内容完整交出去——一旦在这里省略，后续任何环节都无法找回。
- **图我们不碰，这是本工具的明确局限。** 占位符只回答「这里有一张图」「它叫什么」，不解析图片内容；需要时用 `--images-dir` 把字节另存为文件，由阅读方自行决定是否读图。
- **结构是还原出来的，不是推断出来的。** docx/xlsx/pptx 本身是声明式结构，解析即可还原；凡是需要靠启发式猜的地方，宁可不猜。
- **不兼容就报错，不静默降级。** 对 PDF 要 CSV、对文档要 CSV 都会直接失败退出，避免拿到一份「看起来像、其实不是」的数据。

## 特性

- 单命令、零配置：`multype2txt -i <文件>` 即得到对应格式的文本。
- 覆盖常见办公与电子文档格式，纯 Rust 实现，无 C/C++ 动态库依赖。
- 结果默认写入标准输出，天然适配管道；`-o` 可落盘。
- `-f` 可覆盖自动映射（`-f text` 退回纯文本）。
- 诊断日志固定写 stderr，默认关闭（仅 error），需要时用 `-v` 开启，绝不会混入标准输出。
- 扩展名大小写不敏感，并借助文件头签名识别真实格式（例如把 `.xls` 误存为 `.doc` 也能解析）。
- 下游提前关闭管道（`| head`）视为正常结束，不报错、退出码为 `0`。
- 对 legacy 二进制格式（`.doc` / `.ppt`）做**最大努力支持**：能转出文字与大致标题，
  但结构弱于 OOXML，详见 [legacy 二进制格式的局限](#legacy-二进制格式的局限)。

## 支持格式

| 格式 | 扩展名 | 默认输出 | 解析库 |
|------|--------|----------|--------|
| Word（OOXML） | `.docx` | Markdown | office_oxide |
| Word（二进制） | `.doc` | Markdown | office_oxide |
| PowerPoint（OOXML） | `.pptx` | Markdown | office_oxide |
| PowerPoint（二进制） | `.ppt` | Markdown | office_oxide |
| Excel（OOXML） | `.xlsx` | CSV | office_oxide |
| Excel（二进制） | `.xls` | CSV | office_oxide |
| PDF | `.pdf` | 纯文本 | pdf_oxide |

说明：

- Office 六种格式由 `office_oxide` 统一处理，会自动识别文件真实类型。
- 加密的 OOXML 文档（受密码保护）不支持解密，会给出明确错误。
- PDF 按页面阅读顺序抽取正文，页与页之间插入换页符（`U+000C` / `\f`），
  以保留分页边界；在普通文本查看器中表现为分页，不产生可见的标记文本。

### legacy 二进制格式的局限

`.doc` / `.ppt` 是二进制流式格式，与 OOXML 的声明式结构不同：底层库对它们只抽取
**文字流**，不解析格式记录（粗体、编号、列表层级、表格、图片锚点），
结构由逐行启发式重建。因此同一份内容两种格式的输出完整度差距明显：

| 输入 | 标题行 | 列表行 | 结构合计 |
|------|--------|--------|----------|
| `samples/demo.doc` | 1 | 0 | 1 |
| `samples/demo.docx` | 7 | 34 | 41 |
| `samples/demo.ppt` | 26 | 0 | 26 |
| `samples/demo.pptx` | 26 | 12 | 38 |

具体表现：

- **`.doc`**：正文只剩段落，「目录」「卷一·自然概貌」这类层级不会被识别为标题或列表；
  只有首行短句按启发式判为一级标题。
- **`.ppt`**：页标题保留（底层读取了占位符类型），但**列表层级全部丢失**，
  项目符号退化为独立的 `*` 字符行或斜体文本。
- **`.xls`**：不受影响——CSV 走原生网格读取，绕开 IR 的启发式取舍，数据仍全量对齐。

**推荐先转成 `.docx` / `.pptx` 再交给本工具**，转换后列表、表格、粗体与图片占位符
都会完整还原：

```bash
# Word / PowerPoint 的「另存为」即可；批量可用 LibreOffice
libreoffice --headless --convert-to docx --outdir out/ legacy.doc
libreoffice --headless --convert-to pptx --outdir out/ legacy.ppt

multype2txt -i out/legacy.docx -o legacy.md
```

`--images-dir` 同样只对 OOXML 生效：`.doc` / `.ppt` 的图片字节虽能被底层抽出，
但 IR 不携带位置与描述，无法在正文定位，因此**既不导出也不报错**——
这是当前「最大努力支持」范围内的既定行为，而非可依赖的图片导出路径。

### xls 的 SST 缺陷（输出乱码）

`.xls` 的共享字符串表（SST）可能被 `CONTINUE` 记录切开。[MS-XLS] §2.5.293 规定：
字符串被边界切断时，`CONTINUE` 的首字节是**新的编码标志**（可把后续字符从
Latin-1 切换到 UTF-16LE），并非字符数据。底层库将这些记录**简单拼接后顺序读**，
该标志字节被当作字符读入，其后所有偏移全部错位。

实测 `samples/demo.xls`（65562 行）：

| 转换路径 | 坏行 | 控制字符 |
|----------|------|----------|
| 直接转 `.xls` | 30 行（第 51~80 行） | 82134（含 76215 个 NUL） |
| 先转 `.xlsx` 再转 | 0 | 0 |

坏行内容是错位后的原始记录字节（半截 UTF-16LE 的 URL、记录头、NUL），
在终端与编辑器中表现为乱码，`file` 也会把输出判为二进制。
**这是解析层缺陷，不是 CSV 序列化问题**——`-f text` 走 IR 路径同样中招。

底层 `office_oxide` 0.1.12+ 已修复该缺陷，但同批新增的 `DocumentIR.defined_names`
字段 pdf_oxide 至今未适配（0.3.73 ~ 0.3.78 构造 IR 时都会因缺字段而编译失败），
而 0.1.4 ~ 0.1.11 不含该修复，升级必须 fork 其一，评估后放弃。

**遇到乱码请先转 `.xlsx`**——OOXML 是 XML，共享字符串走另一条解析路径，不受影响：

```bash
libreoffice --headless --convert-to xlsx --outdir out/ dirty.xls
multype2txt -i out/dirty.xlsx -o dirty.csv
```

### PDF 的图片导出

PDF 默认输出纯文本，图片没有落点。`--images-dir` 对 PDF 有效：按页扫描内容流
抽图落盘，并在**该页正文末尾**追加与 Markdown 输出同款的占位符——PDF 没有标题、
列表结构可挂占位符，页末是唯一既不打断正文、又能保留页归属的位置：

```text
……该页正文……
![图片1](report-imgs/img-0001.png)
```

页边界不会因此改变：占位符插在换页符之前，页数与不加该参数时完全一致。

**逐张尽力，失败如实标注。** PDF 里常见内联图像（`BI...ID...EI`），底层库对
这类图的解码存在缺陷（缺 `/ColorSpace` 条目时拒绝解码，而规范允许省略）。
遇到这种图时不落盘、不写占位符，改为在该页末尾写一行说明并记 error 日志：

```text
[图片3 导出失败: Image error: Image missing /ColorSpace]
```

这样既不会让一张图毁掉整篇正文，也不会变成与「文档里本来没有图」无法区分的
静默失败。底层库修复后图片会自动落盘，本工具无需改动。

### CSV 的约定

多工作表无法用一个 CSV 表达，因此以 `#` 注释行分隔并附元信息：

```text
# sheet: 销售明细  rows=1200 cols=8  merged=2
# merged: A1:C1
日期,产品,数量,金额
2024-05-01,甲,3,120
```

- `#` 开头的行是**元信息，不是数据**；严格的 CSV 解析器需自行跳过。
- 数据区遵循 RFC 4180 转义；列按整表最大列数对齐，中间空行按真实行号补齐。
- 单元格只导出**计算结果**：日期为 ISO 8601，错误值为 `#REF!` 等原文，布尔为 `TRUE`/`FALSE`。
- `.xls`（BIFF8）的解析未保留合并单元格，该格式不输出 `# merged` 行。

### 已知局限

- **不解析图片内容**，仅占位（`--images-dir` 可把字节落盘）。
- `--images-dir` 的生效范围是 **Markdown 输出（`.docx` / `.pptx`）与 PDF 输入**：
  对 `.doc` / `.ppt` 不生效（不导出也不报错，见
  [legacy 二进制格式的局限](#legacy-二进制格式的局限)），对 CSV 输出直接报错。
- **`.xls` 可能输出乱码**：共享字符串跨 `CONTINUE` 边界后错位，混入 NUL 与
  控制字节（实测 65562 行中 30 行），且 `-f text` 同样中招；建议先转 `.xlsx`
  再转换，原因见 [xls 的 SST 缺陷](#xls-的-sst-缺陷输出乱码)。
- **PDF 的内联图可能导出失败**：底层库对缺 `/ColorSpace` 的内联图像拒绝解码，
  该图会以 `[图片N 导出失败: …]` 标注而非落盘，见 [PDF 的图片导出](#pdf-的图片导出)。
- **`.xlsb` 不支持**：依赖的 `rxlsb` 存在列号丢弃、公式值丢失等数据正确性缺陷，
  在修复前宁可拒绝也不产出错位的数据。
- PDF 只有文字流，无结构可还原，因此 `-f markdown/csv` 对 PDF 一律报错。
- 图表（chart）数据不导出——它属于「图」，不在本工具的处理范围内。

## 安装与构建

要求 Rust 1.88 及以上。

```bash
# 调试构建
cargo build

# 发布构建（产物位于 target/release/multype2txt）
cargo build --release
```

也可以直接运行：

```bash
cargo run -- -i samples/demo.docx
```

## 用法

```text
multype2txt -i <输入文件> [-o <输出文件>] [-f <格式>] [--images-dir <目录>]
```

### 参数

| 参数 | 说明 |
|------|------|
| `-i, --input <FILE>` | 必填。输入文档路径。 |
| `-o, --output <FILE>` | 可选。输出文件路径；缺省时写入标准输出。 |
| `-f, --format <FMT>` | 可选。`auto`（默认，按输入类型决定）/ `markdown` / `csv` / `text`。 |
| `--images-dir <DIR>` | 可选。把图片字节导出到该目录，占位符 URL 指向实际文件。对 Markdown 输出（`.docx` / `.pptx`）与 PDF 有效；对 `.doc` / `.ppt` 不生效，对 CSV 输出报错。 |
| `-v, --verbose` | 可选。打开诊断日志：`-v` 为 info，`-vv` 为 debug。 |
| `-h, --help` | 打印帮助。 |
| `-V, --version` | 打印版本。 |

### 日志与数据隔离

- 转换结果只出现在标准输出（或 `-o` 指定的文件）中。
- 诊断日志始终写 stderr，且默认只输出 error；`-v` 逐级放宽到 info/debug。
- 为避免外部环境误开日志，程序不读取 `RUST_LOG`；日志级别完全由 `-v` 控制。

### 示例

```bash
# docx 输出 Markdown 到标准输出
multype2txt -i report.docx

# xlsx 输出 CSV 到标准输出
multype2txt -i sheet.xlsx

# 输出到文件
multype2txt -i report.docx -o report.md

# 退回纯文本（丢弃全部结构）
multype2txt -f text -i report.docx -o report.txt

# 图片字节另存，占位符指向真实文件
multype2txt --images-dir report-imgs -i report.docx -o report.md

# PDF 同样可导出图片，占位符插在对应页末尾
multype2txt --images-dir report-imgs -i report.pdf -o report.txt

# legacy .doc 结构弱，先转 .docx 再转换（推荐）
libreoffice --headless --convert-to docx --outdir out/ legacy.doc
multype2txt -i out/legacy.docx -o legacy.md

# 接入管道（结果与诊断分离）
multype2txt -i annual.pptx | head -n 50

# 批量转换
for f in docs/*.docx; do
    multype2txt -i "$f" -o "${f%.docx}.md"
done

# 需要排查问题时打开诊断日志（写 stderr，不影响结果）
multype2txt -v -i report.pdf -o report.txt
```

### 退出码

| 退出码 | 含义 |
|--------|------|
| `0` | 转换成功，或下游提前关闭管道（`| head`） |
| `1` | 转换失败（格式不支持、输入输出格式不兼容、文件损坏、写入失败等） |
| `2` | 命令行参数错误 |

## 项目结构

```text
src/
  main.rs              # 命令行入口：参数解析、输出分发、退出码
  lib.rs               # 库入口，对外导出 convert_file / convert_with 等
  error.rs             # ConvertError：统一的错误类型
  output.rs            # OutputFormat / ConvertOptions / ImagePolicy
  converter/
    mod.rs             # 按扩展名分派，选择目标输出格式
    office.rs          # Office 六种格式 -> 打开文档后交给渲染层
    pdf.rs             # PDF -> 纯文本
  render/
    mod.rs             # DocumentRenderer trait + 渲染器工厂 + 纯文本实现
    markdown.rs        # 自研 IR -> Markdown 渲染器
    grid.rs            # SheetSource trait + xlsx/xls 网格适配 + CSV 序列化
tests/
  convert.rs           # 针对 samples/ 的端到端测试（链路 + 分派 + 报错 + 图片导出）
examples/
  structure_probe.rs   # 结构还原探针：量化纯文本与结构化输出的差距
samples/               # 随附的小体积样例文档
```

解析与渲染分离：`converter` 只负责打开文档并选定目标格式，`render` 负责产出文本。
新增一种输出格式只需实现 `DocumentRenderer` 并在工厂里加一个分支，分派逻辑完全不用改。

## 开发

```bash
cargo fmt
cargo clippy --all-targets
cargo test
```

### 结构还原探针

量化 `plain_text()` 与 `to_markdown()` / `to_html()` 之间的结构信息差距，
用于判断「把输出从纯文本升级为结构化」能拿到多少增量：

```bash
cargo run --example structure_probe
```

输出走日志（stderr），包含各格式的字符量、markdown 结构标记统计、
图片占位降级数量，以及 docx 的 markdown 预览。

## 许可

Apache License 2.0，详见 [LICENSE](LICENSE)。
