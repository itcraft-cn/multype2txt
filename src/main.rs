//! multype2txt 命令行入口。
//!
//! 用法：
//!
//! ```text
//! multype2txt -i report.docx            # 输出 Markdown 到标准输出
//! multype2txt -i sheet.xlsx             # 输出 CSV 到标准输出
//! multype2txt -i report.pdf -o report.txt
//! multype2txt -f text -i report.docx    # 强制纯文本（丢弃结构）
//! multype2txt --images-dir imgs -i deck.pptx   # 图片字节另存到 imgs/
//! multype2txt -v -i report.pdf          # 需要排查时打开诊断日志
//! ```
//!
//! 设计约定：
//!
//! - 转换结果只写入标准输出或 `-o` 指定的文件，保证管道可用；
//! - 诊断信息固定写 stderr，且默认关闭（仅输出 error），永不进入标准输出；
//! - 是否输出日志由 `-v/-vv` 决定，刻意不读取 `RUST_LOG`，避免外部环境变量
//!   意外打开底层解析库的高频日志、弄脏数据流；
//! - 请求了输入无法提供的输出格式时直接报错退出，不静默降级；
//! - 任何失败都以非零退出码结束，便于脚本判断。

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use log::LevelFilter;

use multype2txt::output::default_format;
use multype2txt::{convert_with, ConvertOptions, ImagePolicy, OutputFormat};

/// 命令行参数。
#[derive(Debug, Parser)]
#[command(
    name = "multype2txt",
    version,
    about = "将 Office/PDF 文档转换为纯文本",
    long_about = "将 Office（DOC/DOCX/XLS/XLSX/PPT/PPTX）与 PDF 文档转换为纯文本。\n\
                  默认输出到标准输出，使用 -o 可写入指定文件。\n\n\
                  默认按输入类型选择输出格式：doc/docx/ppt/pptx 输出 Markdown，\n\
                  xls/xlsx 输出 CSV，pdf 输出纯文本，可用 -f 覆盖。"
)]
struct Cli {
    /// 输入文件（.doc/.docx/.ppt/.pptx/.xls/.xlsx/.pdf）
    #[arg(short = 'i', long = "input", value_name = "FILE")]
    input: PathBuf,

    /// 输出文件；缺省时写入标准输出
    #[arg(short = 'o', long = "output", value_name = "FILE")]
    output: Option<PathBuf>,

    /// 输出格式：auto（默认，按输入类型决定）/ markdown / csv / text
    #[arg(
        short = 'f',
        long = "format",
        value_name = "FMT",
        default_value = "auto"
    )]
    format: String,

    /// 把图片字节导出到该目录，占位符 URL 指向实际文件（仅 Markdown 输出有效）
    #[arg(long = "images-dir", value_name = "DIR")]
    images_dir: Option<PathBuf>,

    /// 诊断日志级别：-v 打开 info，-vv 打开 debug（始终写 stderr）
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    verbose: u8,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_logging(cli.verbose);

    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // 失败原因走日志（stderr），退出码非零，标准输出保持干净。
            log::error!("{err:#}");
            ExitCode::FAILURE
        }
    }
}

/// 初始化日志：永远写 stderr，默认只输出 error，`-v` 逐级放宽。
///
/// 不使用 `env_logger::from_env`，因此忽略 `RUST_LOG`。这样即使用户环境里
/// 预设了 `RUST_LOG=info/debug`，转换结果所在的数据流也不会被日志干扰。
fn init_logging(verbose: u8) {
    let level = match verbose {
        0 => LevelFilter::Error,
        1 => LevelFilter::Info,
        _ => LevelFilter::Debug,
    };

    env_logger::Builder::new()
        .target(env_logger::Target::Stderr)
        .filter_level(level)
        .init();
}

/// 执行一次转换，并按 `-o` 决定结果去向。
fn run(cli: &Cli) -> anyhow::Result<()> {
    let options = build_options(cli)?;
    let text = convert_with(&cli.input, &options)?;

    match &cli.output {
        Some(path) => {
            let mut file = std::fs::File::create(path)
                .map_err(|e| anyhow::anyhow!("无法创建输出文件 {}: {e}", path.display()))?;
            file.write_all(text.as_bytes())
                .map_err(|e| anyhow::anyhow!("写入输出文件 {} 失败: {e}", path.display()))?;
            log::info!("已写入 {} 字节到 {}", text.len(), path.display());
        }
        None => {
            // 结果直接写标准输出，禁止混入任何诊断文本。
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            let written = handle
                .write_all(text.as_bytes())
                .and_then(|()| handle.flush());
            if let Err(error) = written {
                // `multype2txt -i big.xlsx | head -10` 这类用法里，下游读够了
                // 就关管道，是我们预期内的正常结束，既不是故障也不该以
                // 非零退出码收场——报错只会污染 stderr 的日志。
                if error.kind() == std::io::ErrorKind::BrokenPipe {
                    return Ok(());
                }
                return Err(error.into());
            }
        }
    }

    Ok(())
}

/// 把命令行参数翻译成转换选项。
///
/// `--images-dir` 只在两种组合下真正生效：Markdown 输出（Office 文档的 IR
/// 携带图片与位置）与 PDF 输入（按页扫描内容流抽图，挂在页末）。因此这里
/// 先判定最终会产出的格式（显式 `-f` 优先，否则按扩展名推默认值），不匹配
/// 就直接报错退出——静默忽略会让调用方以为图片已经落盘。
fn build_options(cli: &Cli) -> anyhow::Result<ConvertOptions> {
    let mut options = ConvertOptions::new();

    let requested = OutputFormat::parse(&cli.format).map_err(|error| anyhow::anyhow!("{error}"))?;

    let extension = cli
        .input
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    let is_pdf = extension.as_deref() == Some("pdf");

    if cli.images_dir.is_some() {
        let effective = match requested {
            Some(format) => Some(format),
            None => extension.as_deref().and_then(default_format),
        };
        // PDF 的文字流没有图片落点，但仍可按页抽图，因此放开 Text 输出；
        // Office 的 Text 输出丢掉了全部结构，图片无处可挂，依旧拒绝。
        if let Some(format) = effective {
            let supported = matches!(format, OutputFormat::Markdown)
                || (is_pdf && matches!(format, OutputFormat::Text));
            if !supported {
                anyhow::bail!(
                    "--images-dir 只对 Markdown 输出与 PDF 输入有效，而当前组合是 \
                     `-f {}` + `{}`；\n  请去掉 --images-dir，或换成受支持的组合",
                    format.name(),
                    extension.as_deref().unwrap_or("未知格式")
                );
            }
        }
    }

    if let Some(format) = requested {
        options = options.with_format(format);
    }

    if let Some(dir) = &cli.images_dir {
        options = options.with_images(ImagePolicy::Export {
            dir: dir.clone(),
            // URL 前缀原样使用调用方给的路径：相对、绝对还是带 scheme，
            // 由使用场景决定，工具不替调用方猜测。
            url_prefix: dir.to_string_lossy().to_string(),
        });
    }

    Ok(options)
}
