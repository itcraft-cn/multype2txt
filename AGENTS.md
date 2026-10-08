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
| pdf | 纯文本 | 无结构可还原，`-f markdown/csv` 一律报错；`--images-dir` 可按页导出图片 |

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
                    log + env_logger (诊断)
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
| 日志 | log + env_logger | 诊断仅写 stderr，默认关闭（error），`-v` 开启；结果写 stdout |

## 代码结构

```
src/main.rs            命令行入口（-i/-o/-f/--images-dir/-v）
src/lib.rs             库入口
src/error.rs           ConvertError
src/output.rs          OutputFormat / ConvertOptions / ImagePolicy
src/converter/mod.rs   格式分派与默认输出映射
src/converter/office.rs
src/converter/pdf.rs   PDF 文本抽取 + 按页图片导出（页末追加占位符）
src/image.rs           图片落盘公共约定（命名/扩展名魔数/URL 拼接），Office 与 PDF 共用
src/render/mod.rs      DocumentRenderer trait + 工厂 + 纯文本实现
src/render/markdown.rs 自研 IR -> Markdown 渲染器
src/render/grid.rs     SheetSource trait + xlsx/xls 适配 + CSV 序列化
tests/convert.rs       端到端测试（链路、分派、报错、图片导出）
examples/structure_probe.rs  结构还原探针（量化 plain_text vs markdown/html 差距）
samples/               样例文档
```

### 关键设计约束（改动时必须知道）

- **CSV 绕开 IR**：底层库把工作表转 IR 时会按「散文还是表格」做启发式取舍并跳过
  空行，会破坏列对齐与行完整；CSV 必须读原生网格（`as_xlsx()` / `as_xls()`）。
- **行回调的两个生命周期都不能省**：`RowVisitor` 别名必须写成
  `dyn for<'b> FnMut(...) + 'a`，少任一处都会退化成 `'static` 而编译失败。
- **不静默降级**：输入输出格式不兼容时返回 `IncompatibleOutput`，
  图片导出失败时返回 `ImageExport`，都不得悄悄降级。
- **有序列表按真实序号递增**，相邻同样式片段先合并再包裹强调标记，
  这是自研渲染器而非直接调用库 `to_markdown()` 的根本原因。
- **PDF 图片走 `page_image_handles` 而非 `extract_images`**：后者对内联图
  （`BI...ID...EI`）未补 `/Subtype` 会报错并被底层吞掉，整页表现为无图。
  页对齐依赖 `extract_all_text` 的分段数恒等于 `page_count`（Rust 字面量
  写 `'\x0C'`，没有 `\f` 转义）；单张图失败只标注不中断，目录不可写等
  整体性失败仍返回 `ImageExport`。

## AI guide

### 角色定位

1. 你是资深架构师
    - 开发前对需求进行详尽分析，必要时提供多套方案，以上、中、下三策呈现
    - 设计时充分考虑非功能性需求：安全性、可扩展性、可用性、可观测性、性能
    - 细节设计上充分考虑设计模式与语言特性
2. 你是资深开发者，对 Rust 非常了解
    - 熟悉官方库与周边库
    - 理解 RAII 与内存布局
    - 偏好过程式 + trait 多态
3. 你是百科全书，熟知大量常识；输出格式偏好 `markdown`

### 环境变量

`${AI_SPEC_ROOT}` 定义在 bash/zsh 环境变量中，可被读取：`echo ${AI_SPEC_ROOT}`

### 交互规则

必须遵循 `${AI_SPEC_ROOT}/agent-template/interaction.rules.md`：

1. 所有交互使用简体中文，输出不得带 Emoji
2. 每次交互先检索 memrec-mcp，并持续记录核心观点、关键节点、重要内容
3. 产出文件后执行 git 提交；仅以当前 `user.name` 提交，不推送到远端
4. 提交遵循约定式提交规范（Conventional Commits）
5. MEMORY.md 写入 .gitignore，不提交到 git
6. 编码时合理生成注释：文件头/类头/函数头/方法头应有描述与注意事项
7. 修改时不删除原有注释，语义变化时变更或重新补充
8. 禁止在代码中使用 stdout/stderr 做调试输出（CLI 的正式结果输出与日志除外）
9. 本机为 Linux，倾向使用 rg/fd/sd/eza/dust/plocate/f2/rrn 等高效工具

### 编码规范

授权读取：`${AI_SPEC_ROOT}/lang-spec/spec.rust.md`

要点：

- 命名：模块/函数/变量小写蛇形，类型帕斯卡，常量全大写蛇形
- 生产代码禁止 `unwrap()` / `panic()` / `expect()`
- 使用 `Result` 与 `?` 传播错误，使用 `thiserror` / `anyhow`
- 优先借用 `&T` / `&[T]`，能延迟克隆就用 `Cow`
- 遵循单一职责，优先组合而非继承
- 单元测试放同文件 `#[cfg(test)]`，集成测试放 `tests/`

### 代码审查

授权读取：`${AI_SPEC_ROOT}/lang-spec/review.rust.md`

Rust 代码审查主要依赖编译器和 Clippy 静态分析的结果。

## 开发命令

```bash
cargo fmt
cargo clippy --all-targets
cargo test
cargo build --release
```
