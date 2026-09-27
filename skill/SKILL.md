---
name: multype2txt
description: 用本地二进制将 Office/PDF 文档零 token 消耗地转换为纯文本。触发场景：(1) 需要读取或分析 doc/docx/ppt/pptx/xls/xlsx/pdf 文件内容，(2) 用户提到 "转换文件为 txt"、"文档转文本"、"docx to txt"、"pdf to text"、"office to text"、"解析文档"，(3) 需要把二进制文档交给模型处理但不想消耗 token 或直接读会乱码，(4) 批量把文档目录转换为 txt。优先使用本技能而不是直接 cat/读取二进制文档。
---

# multype2txt 文档转文本技能

## 作用

调用本地命令 `multype2txt`，把 Office 与 PDF 文档转换为纯文本。

- 转换在本地完成，**不消耗任何 token**：模型不需要读取二进制内容，也不需要请求
  外部服务。
- 转换结果写入 stdout 或文件；模型按需只读取其中需要的那一段，进一步节省上下文。
- 覆盖格式：`doc` `docx` `ppt` `pptx` `xls` `xlsx` `pdf`。

## 为什么不直接读取文档

`.docx/.xlsx/.pptx/.pdf` 等是二进制或压缩包格式，直接读取会得到乱码或极长噪声，
既浪费 token 也无法理解。正确做法是先用本技能转换为 `.txt`，再读取文本。

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
multype2txt -i <输入文件> [-o <输出文件>] [-v|-vv]
```

| 参数 | 说明 |
|------|------|
| `-i, --input <FILE>` | 必填。输入文档路径。 |
| `-o, --output <FILE>` | 可选。输出文本文件路径；缺省写入标准输出。 |
| `-v, --verbose` | 可选。打开诊断日志（`-v`=info，`-vv`=debug），日志只写 stderr。 |
| `-h, --help` / `-V, --version` | 帮助 / 版本。 |

### 基本示例

```bash
# 输出到标准输出（适合管道，只读取需要的一段）
multype2txt -i report.pdf | head -n 100

# 落盘为 txt，后续反复读取不再重复转换
multype2txt -i report.docx -o report.txt

# 批量转换整个目录为同名 txt
for f in docs/*.docx docs/*.pdf; do
    multype2txt -i "$f" -o "${f%.*}.txt"
done
```

## 推荐工作流

1. 判断输入是否为二进制文档格式；是则使用本技能，不要直接读取原文。
2. 小文件可直接输出到 stdout 并读取；大文件建议先 `-o` 落盘为 `.txt`。
3. 对落盘的 `.txt` 再用检索/分段读取，只把需要的片段带入上下文。
4. 批量处理时用循环逐一转换，避免单个文档占用过多上下文。

## 输出约定

- 转换结果只出现在 stdout（或 `-o` 文件）中。
- 诊断日志只写 stderr，默认关闭（仅 error）；需要排查时加 `-v`。
- 该命令不读取 `RUST_LOG`，外部环境变量不会导致日志混入数据。
- PDF 按页面阅读顺序抽取正文，页与页之间以换页符 `U+000C`（`\f`）分隔，
  用于保留分页边界，不产生可见的标记文本。

## 退出码

| 退出码 | 含义 | 处理建议 |
|--------|------|----------|
| `0` | 转换成功 | 继续后续处理 |
| `1` | 转换失败 | 检查文件是否存在、格式是否受支持、是否损坏；加 `-v` 查看原因 |
| `2` | 命令行参数错误 | 检查 `-i/-o` 参数 |

## 注意事项

- 不支持的格式：`xlsb`、`csv`、`txt`、`rtf`、`odt` 等会返回退出码 1。
  txt 本身已是文本，无需转换。
- 加密（受密码保护）的 OOXML 文档不支持解密，会明确报错。
- 扩展名大小写不敏感；工具会结合文件头签名识别真实格式，扩展名写错多可容忍。
- 不要把 `-o` 的文件名与输入放在同一路径覆盖源文件；如需保留原件，输出到不同目录。

## 排障

```bash
# 查看帮助
multype2txt --help

# 打开诊断日志定位失败原因
multype2txt -v -i broken.pdf -o out.txt

# 确认版本
multype2txt --version
```
