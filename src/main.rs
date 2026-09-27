//! multype2txt 命令行入口。
//!
//! 用法：
//!
//! ```text
//! multype2txt -i report.docx            # 转换结果输出到标准输出
//! multype2txt -i report.pdf -o report.txt
//! multype2txt -v -i report.pdf          # 需要排查时打开诊断日志
//! ```
//!
//! 设计约定：
//!
//! - 转换结果只写入标准输出或 `-o` 指定的文件，保证管道可用；
//! - 诊断信息固定写 stderr，且默认关闭（仅输出 error），永不进入标准输出；
//! - 是否输出日志由 `-v/-vv` 决定，刻意不读取 `RUST_LOG`，避免外部环境变量
//!   意外打开底层解析库的高频日志、弄脏数据流；
//! - 任何失败都以非零退出码结束，便于脚本判断。

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use log::LevelFilter;

use multype2txt::convert_file;

/// 命令行参数。
#[derive(Debug, Parser)]
#[command(
    name = "multype2txt",
    version,
    about = "将 Office/PDF 文档转换为纯文本",
    long_about = "将 Office（DOC/DOCX/XLS/XLSX/PPT/PPTX）与 PDF 文档转换为纯文本。\n\
                  默认输出到标准输出，使用 -o 可写入指定文件。"
)]
struct Cli {
    /// 输入文件（.doc/.docx/.ppt/.pptx/.xls/.xlsx/.pdf）
    #[arg(short = 'i', long = "input", value_name = "FILE")]
    input: PathBuf,

    /// 输出文件；缺省时写入标准输出
    #[arg(short = 'o', long = "output", value_name = "FILE")]
    output: Option<PathBuf>,

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
    let text = convert_file(&cli.input)?;

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
            handle.write_all(text.as_bytes())?;
            handle.flush()?;
        }
    }

    Ok(())
}
