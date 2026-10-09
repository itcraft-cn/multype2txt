# 变更日志

[English](CHANGELOG_en.md)

本项目全部变更记录于此文件。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [0.3.0-dev] - 未发布

### 新增

- **诊断日志与输出彻底隔离**（log4rs 双路）：默认只写日志文件
  `/tmp/<输入文件名>.conv.err.log`，级别 warn——没有警告或错误就不留下文件；
  同一输入重复转换先删旧文件，日志只反映最近一次。存在警告或错误时，全部输出
  结束后在 stderr 打一行提示给出日志路径；`-v` 起日志同时输出到 stderr
  （console + file，info / debug），此时不打提示行。失败原因改走
  `eprintln!`，保证默认模式下终端仍能看到失败原因。不读取 `RUST_LOG` 的
  既有原则保持不变。

### 修复

- **落实外部代码评审的 10 项处置**，核心是「风险点写进注释时必须同时落成
  断言或日志」：
  - EMF 魔数判定增加偏移 40 的 `dSignature` 校验，4 字节 `01 00 00 00`
    不再被误判为 EMF；
  - PDF 页对齐契约（分段数 == 页数）在生产环境校验，不等即返回
    `ImageExport`，占位符绝不错位插入；
  - 列表续行缩进按 marker 宽度对齐（`- ` 2 列、`1. ` 3 列），修有序列表
    续行被严格解析器当新段落的问题；
  - Windows 反斜杠路径在占位符 URL 中归一为 `/`；
  - xlsx 共享字符串索引越界不再静默返回空串，改为错误值 + warn 日志；
  - SVG 识别扫描窗口 1 KiB 放宽到 8 KiB；
  - `format_number` 科学计数法边界、`Source::Plain` 不可达分支、表格转义
    局限均以文档说明。

### 文档

- AGENTS.md 合并 Rust 模板，新增「日志与输出的边界不可倒置」设计约束；
- README 同步日志隔离说明与评审相关描述。

## [0.2.0] - 2026-10-09

### 新增

- **按输入格式分派结构化输出**（三条产品线）：
  - `.docx` / `.pptx` → Markdown：标题、列表、表格、超链接、脚注、
    图片占位符；自研 IR → Markdown 渲染（相邻同样式片段合并、有序列表
    按真实序号、链接与强调顺序 `[**文本**](url)`）；
  - `.xlsx` / `.xls` → CSV：**全量**抽取单元格计算值，绕开 IR 直读原生
    网格以保证列对齐与行完整，多工作表以 `# sheet:` 注释行分隔并附
    合并区域元信息；
  - `.pdf` → 纯文本：页间插入换页符（`U+000C`）保留分页边界；
  - `-f` 可覆盖自动映射，不兼容组合（如 PDF 要 CSV）直接报错，不静默降级。
- **PDF 图片导出**：`--images-dir` 对 PDF 按页扫描内容流抽图落盘，
  占位符追加在对应页末尾，页边界不变；单张图失败以
  `[图片N 导出失败: …]` 如实标注，不中断正文。
- **结构还原探针** `examples/structure_probe.rs`：量化纯文本与
  结构化输出之间的结构信息差距。
- AI skill `skills/multype2txt/SKILL.md`：指导 Agent 零 token 消耗地
  把文档转为结构化文本，含 legacy 提示与 xls 乱码处置指引。

### 文档

- legacy `.doc` / `.ppt` 局限：结构行实测对比、推荐先转 OOXML 再转换；
- `.xls` SST 缺陷（CONTINUE 边界错位导致输出乱码）根因、实测数据与
  「先转 `.xlsx`」绕开方案。

### 变更

- 样例目录更名为 `samples/`，补充 `.doc` / `.ppt` / `.xls` / `.xlsb` 样例。

## [0.1.0] - 2026-09-27

### 新增

- 从 hydrakiller 抽取为独立命令行工具 `multype2txt`：Office 六种格式
  （`.doc` / `.docx` / `.ppt` / `.pptx` / `.xls` / `.xlsx`）与 `.pdf`
  转纯文本，提供 CLI 与库两种形态；纯 Rust 实现，无 C/C++ 动态库依赖。
- 结果写标准输出或 `-o` 文件，BrokenPipe（`| head`）视为正常结束。
- AI skill 初版。

### 修复

- 日志固定写 stderr 并默认关闭（仅 error），避免污染数据流；刻意不读取
  `RUST_LOG`。

[0.3.0-dev]: https://github.com/itcraft-cn/multype2txt/compare/v0.2.0_20261009...HEAD
[0.2.0]: https://github.com/itcraft-cn/multype2txt/compare/v0.1.0_20260927...v0.2.0_20261009
[0.1.0]: https://github.com/itcraft-cn/multype2txt/releases/tag/v0.1.0_20260927
