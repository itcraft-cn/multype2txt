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
//! - 诊断日志默认只写入临时目录的 `<输入文件名>.conv.err.log`（如
//!   `/tmp/demo.xlsx.conv.err.log`），既不进 stdout 也不在终端刷屏；
//!   `-v` 起才额外同步输出到 stderr，`-v` 的日志本身仍不经 stdout；
//! - 默认模式下（无 `-v`）本次转换存在警告或错误时，全部输出结束后
//!   stderr 追加一行提示，指向上述日志文件；没有任何警告或错误则不
//!   留下日志文件。`-v` 时日志已实时显示在 console，不再重复提示；
//! - 同一输入文件重复转换时旧日志先被删除，日志只反映最近一次；
//! - 是否输出更多日志由 `-v/-vv` 决定，刻意不读取 `RUST_LOG`，避免外部
//!   环境变量意外打开底层解析库的高频日志、弄脏数据流；
//! - 请求了输入无法提供的输出格式时直接报错退出，不静默降级；
//! - 任何失败都以非零退出码结束，便于脚本判断。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use log::LevelFilter;
use log4rs::append::console::{ConsoleAppender, Target};
use log4rs::append::file::FileAppender;
use log4rs::config::{Appender, Config, Root};
use log4rs::encode::pattern::PatternEncoder;

use multype2txt::output::default_format;
use multype2txt::{convert_with, ConvertOptions, ImagePolicy, OutputFormat};

/// stderr 控制台日志格式：时间 级别 target 消息。
///
/// 与 env_logger 的默认观感保持接近，便于按既有习惯检索；控制台不着色，
/// 因为 `-v` 的输出常被重定向到文件或 CI 日志，转义序列只会成为噪声。
const PATTERN_CONSOLE: &str = "[{d} {l} {t}] {m}{n}";
/// 日志文件格式：在控制台格式基础上附行号，便于回溯到源码位置。
const PATTERN_FILE: &str = "[{d} {l} {t} L{L}] {m}{n}";

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

    /// 诊断日志：默认只写日志文件；-v 额外输出到 stderr 并放宽到 info，-vv 到 debug
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    verbose: u8,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // 日志文件按输入文件命名（demo.xlsx → /tmp/demo.xlsx.conv.err.log）。
    // 先删旧文件：同一输入重复转换时日志覆盖而非续写，只反映最近一次。
    let log_path = log_file_path(&cli.input);
    let _ = std::fs::remove_file(&log_path);

    init_logging(cli.verbose, &log_path);

    let result = run(&cli);

    // 提示行必须排在全部输出之后，因此先落日志、再报告、最后给提示。
    // （FileAppender 每条日志写完即 flush，此处读到的文件状态已是最终态。）
    match &result {
        Ok(()) => {}
        Err(err) => {
            // 失败原因是命令行的正式输出（与非零退出码配套），直接写 stderr。
            // 不走日志系统：默认模式日志只进文件，走日志会让调用方在终端
            // 看不到失败原因。
            eprintln!("error: {err:#}");
        }
    }
    report_log(&log_path, cli.verbose);

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

/// 构造日志文件路径：`<临时目录>/<输入文件名>.conv.err.log`。
///
/// Linux 下临时目录即 `/tmp`，例如输入 `demo.xlsx` 得到
/// `/tmp/demo.xlsx.conv.err.log`。用 `env::temp_dir()` 而非硬编码 `/tmp`，
/// 在没有该目录的平台上退化到系统临时目录而不是直接失败。
///
/// # 参数
///
/// - `input`: 输入文件路径，只取其文件名部分，避免目录层级撑爆临时目录。
///
/// # 返回
///
/// 日志文件的完整路径。
fn log_file_path(input: &Path) -> PathBuf {
    let name = input
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| String::from("multype2txt"));
    std::env::temp_dir().join(format!("{name}.conv.err.log"))
}

/// 收尾检查日志文件：默认模式下有内容则提示路径，空文件一律删除。
///
/// 「没有警告或错误就不留下日志」是本工具对使用者的承诺：默认模式（无
/// `-v`）级别为 `Warn`，文件存在即说明本次转换确有问题；反之
/// FileAppender 预创建的空文件必须清掉，否则「文件存在」本身就变成假信号。
///
/// `-v` 时不打提示行：日志已经在 console（stderr）实时显示，不存在
/// 「被藏起来需要指路」的情况，而文件里可能只是 info 记录，再打一句
/// 「存在问题」反而误导；文件本身仍留档，便于事后回看。
///
/// # 参数
///
/// - `log_path`: [`log_file_path`] 给出的日志文件路径。
/// - `verbose`: `-v` 的个数，大于 0 时跳过提示行。
fn report_log(log_path: &Path, verbose: u8) {
    let has_content = std::fs::metadata(log_path)
        .map(|meta| meta.len() > 0)
        .unwrap_or(false);
    if has_content {
        if verbose == 0 {
            eprintln!(
                "warn: 本次转化存在些许问题，请查看日志 {}",
                log_path.display()
            );
        }
    } else {
        let _ = std::fs::remove_file(log_path);
    }
}

/// 初始化日志：默认只写文件，`-v` 起追加 stderr 控制台，级别逐级放宽。
///
/// 用 log4rs 而非 env_logger，是为了同时控制「写到哪」与「写不写控制台」：
///
/// - **无 `-v`**：仅 file appender。终端除转换结果与收尾提示外不出现任何
///   日志，数据流与诊断彻底分离；
/// - **`-v` / `-vv`**：file + console 双路。console 显式指向 stderr，
///   永不进入 stdout；日志同时在文件留档，方便事后回看；
/// - **级别**：默认 `Warn`（只在真有问题时才产生日志，默认不打 info），
///   `-v` 放宽到 `Info`，`-vv` 到 `Debug`；
/// - 与旧实现一致，**不读取 `RUST_LOG`**，避免外部环境变量意外打开
///   底层解析库的高频日志。
///
/// 初始化失败时在 stderr 说明一句并返回——日志缺失不影响转换正确性，
/// 流程继续，但绝不静默吞掉。正常返回时 log4rs 已接管全局 logger，
/// 每条日志写完即 flush，无需调用方再收尾。
///
/// # 参数
///
/// - `verbose`: `-v` 的个数（0/1/2+）。
/// - `log_path`: 日志文件路径；调用方须已删除旧文件，保证覆盖语义。
fn init_logging(verbose: u8, log_path: &Path) {
    let level = match verbose {
        0 => LevelFilter::Warn,
        1 => LevelFilter::Info,
        _ => LevelFilter::Debug,
    };

    let file_appender = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new(PATTERN_FILE)))
        .build(log_path);
    let file_appender = match file_appender {
        Ok(appender) => appender,
        Err(error) => {
            eprintln!("error: 无法创建日志文件 {}: {error}", log_path.display());
            return;
        }
    };

    let mut builder =
        Config::builder().appender(Appender::builder().build("file", Box::new(file_appender)));
    let mut root = Root::builder().appender("file");

    if verbose > 0 {
        // 控制台显式指向 stderr：stdout 永远只有转换结果。
        let console = ConsoleAppender::builder()
            .target(Target::Stderr)
            .encoder(Box::new(PatternEncoder::new(PATTERN_CONSOLE)))
            .build();
        builder = builder.appender(Appender::builder().build("console", Box::new(console)));
        root = root.appender("console");
    }

    let config = match builder.build(root.build(level)) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("error: 构建日志配置失败: {error}");
            return;
        }
    };
    if let Err(error) = log4rs::init_config(config) {
        eprintln!("error: 初始化日志失败: {error}");
    }
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
