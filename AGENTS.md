# multype2txt

将 Office（DOC/DOCX/XLS/XLSX/PPT/PPTX）与 PDF 文档转换为结构化文本的命令行工具。

## 项目定位

从 Hydrakiller 项目中抽取出的独立文档转换工具，只做一件事：
把文档解析为目标格式的文本，并可靠地交付给调用方（标准输出或文件）。

- 不涉及网络：无 LLM、无 Web 服务、无前端。
- 不做文档生成：不生成 PDF、不写 Office、不处理 XLSB。
- 输入是单个文件，输出是文本，行为可预测，便于被脚本与管道复用。
- **只还原，不推断**：结构来自文档自身的声明式定义；需要靠启发式猜的地方宁可不猜。
- **只交付，不取舍**：全量抽取，略读与摘要是阅读方（LLM）的职责。

### 三条产品线

| 输入 | 默认输出 | 边界 |
|------|----------|------|
| doc/docx | Markdown | 图片留占位符，有描述则填入占位符 |
| xls/xlsx | CSV，必须全量 | 图片完全忽略、不留占位符 |
| ppt/pptx | 结构 + 文字，仅此而已 | 不碰图 |
| pdf | 纯文本 | 无 Markdown 结构，`-f markdown/csv` 一律报错；表格按坐标几何重建为对齐的纯文本网格（判定链与边界见 `pdf_table` 模块文档）；`--images-dir` 可按页导出图片 |

**图我们不碰，这是本工具的明确局限。** `--images-dir` 可把图片字节另存，
供具备读图能力的调用方自取，但工具本身绝不解析图片内容。

## 技术架构

```
CLI (clap)  ->  converter (按扩展名分派 + 选目标格式)
                 |-- office_oxide  -> Document -> IR / 原生网格
                 `-- pdf_oxide     -> PDF 文本流（页间 \x0C）+ 按页抽图
                         |
                    render (DocumentRenderer trait)
                     |-- markdown.rs   IR -> Markdown（自研）
                     |-- grid.rs       网格 -> CSV（xlsx/xls 适配）
                     `-- PlainText     纯文本
                         |
                    log + log4rs (诊断：默认只进日志文件)
                  stdout / -o FILE (结果)
```

解析与渲染严格分离：`converter` 只负责打开文档与选定格式，`render` 负责产出文本。
新增输出格式只需实现 `DocumentRenderer` 并在工厂加一个分支。

## 核心组件

| 组件 | 技术 | 说明 |
|------|------|------|
| 命令行层 | Rust + clap | 参数解析、输出分发、退出码 |
| 分派层 | converter | 依据扩展名选解析器与默认格式，扩展名大小写不敏感 |
| Office 解析 | office_oxide | 六种 Office 格式，纯 Rust |
| PDF 解析 | pdf_oxide | 文本抽取，页间以换页符分隔；可按页抽图（走 `page_image_handles`） |
| 渲染层 | 自研 | `DocumentRenderer` trait，Markdown / CSV / 纯文本三个实现 |
| 网格层 | `SheetSource` trait | xlsx 与 xls 各一个适配，CSV 序列化只写一次 |
| 错误处理 | thiserror + anyhow | 库内定义 ConvertError，入口统一收口 |
| 日志 | log + log4rs | 默认只写 `/tmp/<输入名>.conv.err.log`（有问题才生成），`-v` 追加 stderr；结果写 stdout |

## 代码结构

```
src/main.rs            命令行入口（-i/-o/-f/--images-dir/-v）
src/lib.rs             库入口
src/error.rs           ConvertError
src/output.rs          OutputFormat / ConvertOptions / ImagePolicy
src/converter/mod.rs   格式分派与默认输出映射
src/converter/office.rs
src/converter/pdf.rs   PDF 文本抽取（基线阅读顺序 + 表格页坐标重建）+ 按页图片导出
src/converter/pdf_table/   PDF 表格重建（坐标几何，判定链见模块文档）
    mod.rs      编排：归一化 -> 行 -> 区块 -> 列 -> 网格 -> 自检
    word.rs     词几何归一化（字体度量修正）、显示宽度、词间空格判定
    lines.rs    行聚类、表格行/正文行切分、目录行识别
    columns.rs  列检出：一条没有任何词跨越的垂直空白带即列界
    grid.rs     网格装配、复制源对齐方式、渲染
    verify.rs   自检：字符守恒 + 行宽有界
    profile.rs  全部可调量（一律无量纲，逐项注明来源）
    pages.rs    真实页面测试数据与整篇回归门槛
src/image.rs           图片落盘公共约定（命名/扩展名魔数/URL 拼接），Office 与 PDF 共用
src/render/mod.rs      DocumentRenderer trait + 工厂 + 纯文本实现
src/render/markdown.rs 自研 IR -> Markdown 渲染器
src/render/grid.rs     SheetSource trait + xlsx/xls 适配 + CSV 序列化
tests/convert.rs       端到端测试（链路、分派、报错、图片导出）
examples/structure_probe.rs  结构还原探针（量化 plain_text vs markdown/html 差距）
examples/pdf_words_probe.rs  词坐标探针 / 测试数据生成器（TSV，见 tests/data/pdf/）
tests/data/pdf/              真实页面词坐标（由上述 example 生成，供单元测试与回归门槛）
samples/               样例文档
```

### 关键设计约束（改动时必须知道）

- **CSV 绕开 IR**：底层库把工作表转 IR 时会按「散文还是表格」做启发式取舍并跳过
  空行，会破坏列对齐与行完整；CSV 必须读原生网格（`as_xlsx()` / `as_xls()`）。
- **行回调的两个生命周期都不能省**：`RowVisitor` 别名必须写成
  `dyn for<'b> FnMut(...) + 'a`，少任一处都会退化成 `'static` 而编译失败。
- **不静默降级**：输入输出格式不兼容时返回 `IncompatibleOutput`，
  图片导出失败时返回 `ImageExport`，都不得悄悄降级。
- **日志与输出的边界不可倒置**：诊断日志默认只进
  `/tmp/<输入名>.conv.err.log`（log4rs file appender，`-v` 才追加
  stderr console），stdout 永远只有转换结果；失败原因走 `eprintln!`
  而非日志（默认模式日志不上终端，走日志会让调用方看不到失败原因）；
  提示行只在默认模式且日志文件非空时打——文件为空必须删除，
  否则「文件存在」本身就是假信号。
- **xls 的 SST 错位缺陷不修、只文档化**：`office_oxide` 0.1.3 把 SST 与
  `CONTINUE` 记录简单拼接后顺序读，忽略 [MS-XLS] §2.5.293 规定的边界
  grbit 编码标志，字符串错位导致输出混入 NUL/控制字节（实测 65562 行中
  30 行，`-f text` 同样中招）。0.1.12+ 已修但引入 `defined_names` 字段，
  pdf_oxide 0.3.73 ~ 0.3.78 均未适配而编译失败，0.1.4 ~ 0.1.11 无修复，
  升级必须 fork 其一——故锁定版本并在 README/SKILL 建议使用者先转
  `.xlsx`，**不得**为了「消掉乱码」在输出侧剔除控制字符（那会掩盖数据
  错位，把被污染的单元格伪装成正常内容）。
- **有序列表按真实序号递增**，相邻同样式片段先合并再包裹强调标记，
  这是自研渲染器而非直接调用库 `to_markdown()` 的根本原因。
- **PDF 表格重建只依赖无量纲事实，不引入 point 常量**：
  行 = 垂直投影重叠；表格行 = 行内存在 ≥1 em 的列间空白；区块 = 连续的
  表格行；列界 = 一条没有任何词跨越、宽度 ≥1 em 的垂直空白带；对齐方式
  复制源几何（右边缘比左边缘稳定即右对齐），不看内容类型。全部参数集中在
  `pdf_table/profile.rs`，每项注明来源（字体度量/排版常识/结构定义）。
  **禁止**再写「间隙 ≥12pt」「连续 ≥3 行」这类为某份样本试出来的阈值。
- **旋转页靠字体度量纠正，不猜方向**：`pdf_oxide 0.3.73` 在 `/Rotate` 页把
  词宽写进包围盒高度（5 字词变成 48×53 方块），`word::normalize` 以
  「行盒高度 ≤ 1.3 em」为界判定高度不可信，保留下沿按字号重算。旋转因此
  不再需要方向识别，也不会退化成一行巨串。
- **自检是硬门槛**：`verify` 要求重建后字符守恒、行宽不超过物理上界；
  `pdf.rs` 另做跨路径检查（重建不得比基线少字）。失败一律回退基线并记
  warn，绝不输出半成品。
- **PDF 图片走 `page_image_handles` 而非 `extract_images`**：后者对内联图
  （`BI...ID...EI`）未补 `/Subtype` 会报错并被底层吞掉，整页表现为无图。
  页对齐依赖 `extract_all_text` 的分段数恒等于 `page_count`（Rust 字面量
  写 `'\x0C'`，没有 `\f` 转义）；单张图失败只标注不中断，目录不可写等
  整体性失败仍返回 `ImageExport`。

## AI guide

### 角色定位

1. 你是资深架构师
    - 在开发前，会对需求进行详尽分析，提供多套方案，以上、中、下三策的形式呈现，以备后续决策参考
    - 在设计时，会充分考虑非功能性需求：安全性、可扩展性、可用性、可观测性、性能等
    - 在设计细节时，充分考虑各种设计模式及各语言特性
2. 你是资深开发者，对Rust非常了解
    - 对Rust的官方库及周边库均了解
    - 对Rust的RAII机制理解深刻
    - 对Rust的内存布局非常清楚
    - 开发上偏好过程式+trait多态
    - 对CPU指令也熟悉
3. 你是百科全书，熟知大量常识；输出格式偏好 `markdown`

### 环境变量

${AI_SPEC_ROOT} 定义在 bash/zsh 环境变量中，可被读取: `echo ${AI_SPEC_ROOT}`

### 交互规则

必须遵循 interaction.rules.md 中描述的规则

授权读取：${AI_SPEC_ROOT}/rules/interaction.rules.md

本项目篇幅允许内联，以下为要点（以授权文件为准）：

1. 所有交互使用简体中文，输出不得带 Emoji
2. 每次交互先检索 memrec-mcp，并持续记录核心观点、关键节点、重要内容
3. 产出文件后执行 git 提交；仅以当前 `user.name` 提交，不推送到远端
4. 提交遵循约定式提交规范（Conventional Commits）
5. MEMORY.md 写入 .gitignore，不提交到 git
6. 编码时合理生成注释：文件头/类头/函数头/方法头应有描述与注意事项
7. 修改时不删除原有注释，语义变化时变更或重新补充
8. 禁止在代码中使用 stdout/stderr 做调试输出（CLI 的正式结果输出与日志除外）
9. 本机为 Linux，倾向使用 rg/fd/sd/eza/dust/plocate/f2/rrn 等高效工具
10. 检索优选 codegraph-mcp，次选 ripgrep，兜底 grep；不存在 `.codegraph/` 时主动创建
11. 文档中画图必须使用 mermaid，禁止 ASCII Art（文件夹罗列的 `├──` 制图字符豁免）

### 编码规范

授权读取：${AI_SPEC_ROOT}/lang-spec/spec.rust.md
授权读取：${AI_SPEC_ROOT}/lang-spec/review.rust.md

要点：

- 命名：模块/函数/变量小写蛇形，类型帕斯卡，常量全大写蛇形
- 生产代码禁止 `unwrap()` / `panic()` / `expect()`
- 使用 `Result` 与 `?` 传播错误，使用 `thiserror` / `anyhow`
- 优先借用 `&T` / `&[T]`，能延迟克隆就用 `Cow`
- 遵循单一职责，优先组合而非继承
- 单元测试放同文件 `#[cfg(test)]`，集成测试放 `tests/`

### 代码审查

Rust 代码审查主要依赖编译器和 Clippy 静态分析的结果。

## 开发命令

```bash
cargo fmt
cargo clippy --all-targets
cargo test
cargo build --release
```
