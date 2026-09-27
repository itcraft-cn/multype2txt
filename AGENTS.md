# multype2txt

将 Office（DOC/DOCX/XLS/XLSX/PPT/PPTX）与 PDF 文档转换为纯文本的命令行工具。

## 项目定位

从 Hydrakiller 项目中抽取出的独立文档转换工具，只做一件事：
把文档解析为纯文本，并可靠地交付给调用方（标准输出或文件）。

- 不涉及网络：无 LLM、无 Web 服务、无前端。
- 不做文档生成：不生成 PDF、不写 Office、不处理 XLSB。
- 输入是单个文件，输出是纯文本，行为可预测，便于被脚本与管道复用。

## 技术架构

```
CLI (clap)  ->  converter (按扩展名分派)
                 |-- office_oxide  -> DOC/DOCX/XLS/XLSX/PPT/PPTX
                 `-- pdf_oxide     -> PDF
                         |
                    log + env_logger (诊断)
                  stdout / -o FILE (结果)
```

## 核心组件

| 组件 | 技术 | 说明 |
|------|------|------|
| 命令行层 | Rust + clap | 参数解析、输出分发、退出码 |
| 分派层 | converter | 依据扩展名选择解析器，扩展名大小写不敏感 |
| Office 解析 | office_oxide | 六种 Office 格式，纯 Rust |
| PDF 解析 | pdf_oxide | 文本抽取，页间以换页符分隔 |
| 错误处理 | thiserror + anyhow | 库内定义 ConvertError，入口统一收口 |
| 日志 | log + env_logger | 诊断仅写 stderr，默认关闭（error），`-v` 开启；结果写 stdout |

## 代码结构

```
src/main.rs            命令行入口
src/lib.rs             库入口
src/error.rs           ConvertError
src/converter/mod.rs   格式分派与支持列表
src/converter/office.rs
src/converter/pdf.rs
tests/convert.rs       端到端转换测试
samples/               样例文档
```

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
