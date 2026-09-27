//! multype2txt 命令行入口。
//!
//! 用法：
//!
//! ```text
//! multype2txt -i report.docx            # 转换结果输出到标准输出
//! multype2txt -i report.pdf -o report.txt
//! ```
//!
//! 设计约定：
//!
//! - 转换结果只写入标准输出或 `-o` 指定的文件，保证管道可用；
//! - 诊断信息统一走 `log`（默认 stderr），不污染转换结果；
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
}

fn main() -> ExitCode {
    // 默认只输出 warn 及以上级别；需要更多细节时可设 RUST_LOG=info/debug。
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .filter_module("multype2txt", LevelFilter::Info)
        .init();

    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // 失败原因走日志，退出码非零，标准输出保持干净。
            log::error!("{err:#}");
            ExitCode::FAILURE
        }
    }
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
