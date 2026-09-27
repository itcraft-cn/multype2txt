# multype2txt

将 Office 与 PDF 文档转换为纯文本的命令行工具。

`multype2txt` 从 [Hydrakiller](https://example.invalid) 项目中抽取文档解析能力而来，
去除了 LLM、Web 服务、前端等无关依赖，只保留“文档 -> 纯文本”这一条最核心的链路，
便于在脚本、管道与批处理场景中独立使用。

## 特性

- 单命令、零配置：`multype2txt -i <文件>` 即得到文本。
- 覆盖常见办公与电子文档格式，纯 Rust 实现，无 C/C++ 动态库依赖。
- 结果默认写入标准输出，天然适配管道；`-o` 可落盘为 `.txt`。
- 诊断日志固定写 stderr，默认关闭（仅 error），需要时用 `-v` 开启，绝不会混入标准输出。
- 扩展名大小写不敏感，并借助文件头签名识别真实格式（例如把 `.xls` 误存为 `.doc` 也能解析）。

## 支持格式

| 格式 | 扩展名 | 解析库 |
|------|--------|--------|
| Word（OOXML） | `.docx` | office_oxide |
| Word（二进制） | `.doc` | office_oxide |
| PowerPoint（OOXML） | `.pptx` | office_oxide |
| PowerPoint（二进制） | `.ppt` | office_oxide |
| Excel（OOXML） | `.xlsx` | office_oxide |
| Excel（二进制） | `.xls` | office_oxide |
| PDF | `.pdf` | pdf_oxide |

说明：

- Office 六种格式由 `office_oxide` 统一处理，会自动识别文件真实类型。
- 加密的 OOXML 文档（受密码保护）不支持解密，会给出明确错误。
- PDF 按页面阅读顺序抽取正文，页与页之间插入换页符（`U+000C` / `\f`），
  以保留分页边界；在普通文本查看器中表现为分页，不产生可见的标记文本。

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
multype2txt -i <输入文件> [-o <输出文件>]
```

### 参数

| 参数 | 说明 |
|------|------|
| `-i, --input <FILE>` | 必填。输入文档路径。 |
| `-o, --output <FILE>` | 可选。输出文本文件路径；缺省时写入标准输出。 |
| `-v, --verbose` | 可选。打开诊断日志：`-v` 为 info，`-vv` 为 debug。 |
| `-h, --help` | 打印帮助。 |
| `-V, --version` | 打印版本。 |

### 日志与数据隔离

- 转换结果只出现在标准输出（或 `-o` 指定的文件）中。
- 诊断日志始终写 stderr，且默认只输出 error；`-v` 逐级放宽到 info/debug。
- 为避免外部环境误开日志，程序不读取 `RUST_LOG`；日志级别完全由 `-v` 控制。

### 示例

```bash
# 输出到标准输出
multype2txt -i report.pdf

# 输出到文件
multype2txt -i report.docx -o report.txt

# 接入管道（结果与诊断分离）
multype2txt -i annual.pptx | head -n 50

# 批量转换
for f in docs/*.docx; do
    multype2txt -i "$f" -o "${f%.docx}.txt"
done

# 需要排查问题时打开诊断日志（写 stderr，不影响结果）
multype2txt -v -i report.pdf -o report.txt
```

### 退出码

| 退出码 | 含义 |
|--------|------|
| `0` | 转换成功 |
| `1` | 转换失败（格式不支持、文件损坏、写入失败等） |
| `2` | 命令行参数错误 |

## 项目结构

```text
src/
  main.rs              # 命令行入口：参数解析、输出分发、退出码
  lib.rs               # 库入口，对外导出 convert_file / is_supported
  error.rs             # ConvertError：统一的错误类型
  converter/
    mod.rs             # 按扩展名分派到具体解析器
    office.rs          # Office 六种格式 -> 文本
    pdf.rs             # PDF -> 文本
tests/
  convert.rs           # 针对 samples/ 的端到端转换测试
samples/               # 随附的小体积样例文档
```

设计上刻意保持扁平：`converter` 只依赖底层解析库，错误集中到 `ConvertError`，
命令行层只负责 I/O 与退出码，便于作为库被其他程序复用。

## 开发

```bash
cargo fmt
cargo clippy --all-targets
cargo test
```

## 许可

Apache License 2.0，详见 [LICENSE](LICENSE)。
