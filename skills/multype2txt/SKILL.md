---
name: multype2txt
description: 用本地二进制将 Office/PDF 文档零 token 消耗地转换为结构化文本（docx 转 Markdown、xlsx 转 CSV、pptx 转结构文字、pdf 转纯文本）。触发场景：(1) 需要读取或分析 doc/docx/ppt/pptx/xls/xlsx/pdf 文件内容，(2) 用户提到 "转换文件为 txt"、"文档转文本"、"docx to markdown"、"xlsx to csv"、"pdf to text"、"office to text"、"解析文档"，(3) 需要把二进制文档交给模型处理但不想消耗 token 或直接读会乱码，(4) 批量把文档目录转换为文本。优先使用本技能而不是直接 cat/读取二进制文档。
---

# multype2txt

把 Office / PDF 转成本地文本，供你后续阅读与检索。转换在本地完成，**不消耗 token**；
消耗 token 的只有你把输出读进上下文这一动作。

## 三条铁律

1. **二进制文档一律先转换**：`doc/docx/ppt/pptx/xls/xlsx/pdf` 直接 `cat` 只会得到
   乱码或几百万字符的噪声。
2. **先落盘、再切片读**：大文件用 `-o` 写文件，然后用 `rg`/`sed`/`head` 取需要的
   片段。不要整篇读进上下文，也不要重复转换同一文件。
3. **转换完必须检查缺口并告知用户**：`doc`/`ppt` 结构不全、`xls` 可能乱码、
   PDF 表格可能降级、图片一律未导出。见 [必须告知用户的四类缺口](#必须告知用户的四类缺口)。

## 决策表：怎么转

| 输入 | 命令 | 默认产物 |
|------|------|----------|
| `docx` | `multype2txt -i a.docx -o a.md` | Markdown：标题、列表、表格、超链接 |
| `pptx` | `multype2txt -i a.pptx -o a.md` | Markdown：幻灯片页、标题、层级、备注，页间 `---` |
| `xlsx` | `multype2txt -i a.xlsx -o a.csv` | CSV：全量单元格，多表以 `# sheet:` 注释行分隔 |
| `pdf` | `multype2txt -i a.pdf -o a.txt` | 纯文本：阅读顺序正文 + 表格对齐网格，页间 `\f` |
| `doc` / `ppt` / `xls` | 同上，但**必须先看对应提示** | 结构残缺或可能乱码 |

先另存为新格式通常更划算（用户侧一条命令）：

```bash
libreoffice --headless --convert-to docx --outdir out/ old.doc     # doc → docx
libreoffice --headless --convert-to pptx --outdir out/ old.ppt     # ppt → pptx
libreoffice --headless --convert-to xlsx --outdir out/ old.xls     # xls → xlsx（消除乱码）
```

## 参数

```text
multype2txt -i <输入> [-o <输出>] [-f auto|markdown|csv|text]
            [--images-dir <目录>] [--pdf-tables auto|off] [-v|-vv]
```

| 参数 | 用途 | 注意 |
|------|------|------|
| `-i, --input` | 输入文件，必填 | 扩展名大小写不敏感，工具会核对文件头 |
| `-o, --output` | 输出文件，缺省 stdout | 不要与输入同路径，避免覆盖原件 |
| `-f, --format` | `auto`（默认）/`markdown`/`csv`/`text` | **PDF 只接受 `text`**，要 markdown/csv 会直接报错；文档不支持 csv |
| `--images-dir` | 导出图片字节，占位符指向真实文件 | 每个文档用独立目录，否则同名文件互相覆盖；`doc`/`ppt` 不生效 |
| `--pdf-tables` | `auto`（默认）/`off` | 仅 PDF；`off` 关闭表格重建，输出线性阅读顺序 |
| `-v` / `-vv` | 日志额外输出到 stderr（info / debug） | 默认只写日志文件 |

## 标准流程

```bash
# 1. 落盘转换（不要直接读 stdout 进上下文）
multype2txt -i report.pdf -o /tmp/report.txt

# 2. 抽查规模与结构，决定读哪一段（这一步只花几十 token）
wc -l /tmp/report.txt
head -n 40 /tmp/report.txt
rg -n '^#' /tmp/report.txt | head        # Markdown：看标题骨架
rg -n '^# sheet:' /tmp/report.csv        # CSV：看有哪些工作表

# 3. 按需取片段
sed -n '120,200p' /tmp/report.txt
```

原则：**转换器负责全量交付，你负责略读**。不要为了「省 token」让工具裁剪内容，
也不要一次把整篇读进上下文。

## 各格式要点

### docx / pptx / xlsx：可直接用

结构是声明式还原的（标题、列表、表格、备注、工作表边界），输出可信。xlsx 的 CSV
里 `#` 开头是元信息（工作表名、行列数、合并区域），**不是数据**，解析时要跳过。

### doc / ppt：必须提示结构残缺

二进制流式格式，底层只抽得出文字与粗略标题，**列表层级、表格、粗体、图片位置
全部丢失**。实测同一内容的结构行数：`demo.doc` 1 行 vs `demo.docx` 41 行；
`demo.ppt` 26 行（无列表）vs `demo.pptx` 38 行。

转换后主动说：

> 已转换 `旧文档.doc`。注意：`.doc` 是二进制格式，本次只抽到文字与大致标题，
> 列表层级、表格、粗体与图片位置全部丢失。若结构重要，建议先另存 `.docx` 再转换。

### xls：必须检查乱码

`.xls` 的共享字符串表可能错位，把控制字节当字符读入，其后整段内容错乱。实测
`demo.xls`（65562 行）第 51~80 行混入 82134 个控制字符（含 76215 个 NUL）；`-f text`
同样中招（解析层缺陷，与输出格式无关）。

**检测**：输出里出现 `\x00`、`\x01`、成片方块/问号、`h\x00t\x00t\x00p` 这类半截串。
**处理**：不要把乱码行当数据；先转 `.xlsx` 再转换（OOXML 走 XML 解析，控制字符为 0）。

### pdf：默认重建表格，按文档类型决定是否关闭

PDF 没有结构语义，表格是**按词坐标几何重建**的（行 = 垂直投影重叠；列 = 多数行
留空且 ≥1 em 的垂直空白带；对齐方式复制原文档）。重建结果要过自检（字符守恒、
行宽上界、不得比基线少字），不过就整页回退线性文本。判据全部无量纲，没有针对
某份文档的 point 阈值。

**先判断文档属于哪一类，再决定用不用 `--pdf-tables=off`**。判断方式：看文件名与
首页标题即可（监管文件/手册/研报 vs 论文 vs 幻灯片），必要时先按默认转一次再
`head -n 60` 看版式。依据是 11 份文档与 `pdftotext` 的对拍结论：

| 文档类型 | 做法 | 原因 |
|----------|------|------|
| 中文正文 / 中文表格（监管文件、手册、研报） | **用默认** | 汉字兼容字形被归一（`⼀`→`一`、`⻚`→`页`），可搜索；表格成列对齐 |
| 英文短表（权重、数值、参数矩阵） | 用默认 | 对齐正确，正文一致 |
| 英文**长文本表**（意见反馈、定性披露，≥3 列大段文字） | `--pdf-tables=off` | 单元格换行跨列界，相邻列被整并，出现 `not abletospecifiesprovide an` 这类交错 |
| 英文论文 / 公式密集 | `--pdf-tables=off` | 公式上下标两边都还原不了，线性文本更接近原阅读顺序 |
| 幻灯片 / 海报 | 海报用默认；幻灯片 `--pdf-tables=off` | 版面文字框散布，重排后片段交错；`/Rotate` 幻灯片可能出现词粘连（`UniversityZurich`） |

**拿不准时双跑一次，只读 diff 的前几十行**（本地操作，token 成本几十）：

```bash
multype2txt -i a.pdf -o on.txt
multype2txt --pdf-tables=off -i a.pdf -o off.txt
diff off.txt on.txt | head -60

grep -oE '[A-Za-z]{20,}' on.txt | sort | uniq -c | sort -rn | head   # 粘连词
grep -c '\.\.\.\.' off.txt on.txt                                     # 目录点线数
```

判定：diff 里同一行被塞进两段无关句子、或出现非术语长粘连词、或目录点线数明显
减少 → 用 `off.txt`；否则用默认输出。

PDF 的已知边界（影响可读性，不影响字符完整性）：长文本多列表列间整并；
`/Rotate` 页把整列数值画成一段时个别数值可能落到相邻行；跨页表格不拼接；
双栏正文按两列网格还原；示意图标签框会被当多列结构重排；页眉/页脚/水印被底层
丢弃（`pdftotext` 会保留，属差异而非丢内容）。

## 读懂输出

- **PDF 分页**：页间是 `U+000C`（`\f`）。按页取片段时以它切分，不要当成普通空白。
- **CSV 元信息**：`#` 开头的行是工作表边界与合并区域说明，解析时跳过。
- **图片占位符**：`![图片1: 描述](image://1)`，无描述时只有编号；加了
  `--images-dir` 则 URL 指向真实文件。PDF 里未解开的图会留下
  `[图片N 导出失败: 原因]`——见到就告诉用户该图没导出，不要说「文档里没有图」。
- **日志**：默认只写 `/tmp/<输入文件名>.conv.err.log`，没问题时不生成；有问题时
  stderr 末尾会给一行路径提示。要看原因加 `-v`。

## 必须告知用户的四类缺口

转换完成后对照检查，命中就主动说明（不要等用户问）：

| 情况 | 一句话告知 |
|------|-----------|
| 转了 `doc` / `ppt` | 结构残缺（无列表层级/表格/粗体/图位置），建议先转 docx/pptx |
| 转了 `xls` 且出现控制字符 | 该格式底层错位，建议先转 xlsx 重转 |
| PDF 用了 `--pdf-tables=off`，或表格页有明显串列 | 已退回线性文本；关键数值建议对照原 PDF 核对 |
| 输出里有 `[图片N 导出失败: …]` | 第 N 张图未能导出，需要时请提供原图或换工具 |

## 退出码

| 码 | 含义 | 处理 |
|----|------|------|
| `0` | 成功（含 `| head` 提前关闭管道） | 继续 |
| `1` | 转换失败 | 查文件存在/格式支持/格式兼容/是否损坏，加 `-v` |
| `2` | 参数错误 | 查 `-i/-o/-f` |

## 不要做的事

- 不要 `cat`/`Read` 二进制文档本体（乱码 + 巨量 token）。
- 不要对 PDF 请求 `-f markdown` 或 `-f csv`，会直接失败；也不要对文档请求 `-f csv`。
- 不要 `-o` 指向输入文件本身（会覆盖原件）。
- 不要复用同一个 `--images-dir` 跑多个文档（同名图片互相覆盖）。
- 不要把 `doc`/`ppt` 的输出当完整结构用——它看起来像 Markdown，层级是猜的。
- 不要因为 `xls` 有乱码就逐行清洗后使用：错位是字节级的，清洗会掩盖错误。
- 不支持：`xlsb`、`csv`、`txt`、`rtf`、`odt`（返回 1）。`xlsb` 是刻意拒绝：依赖库
  存在列错位与公式值丢失。

## 排障

```bash
multype2txt --help
multype2txt -v -i broken.pdf -o out.txt     # 打开诊断日志
multype2txt --version
multype2txt -f markdown -i sheet.xlsx | head -n 20   # 确认格式映射
```
