use std::ffi::OsString;
use std::process::{Command, ExitStatus};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "xtask", about = "Workspace task runner")]
struct Cli {
    #[command(subcommand)]
    command: Task,
}

#[derive(Debug, Subcommand)]
enum Task {
    Fmt(FmtArgs),
    Clippy(ClippyArgs),
    Lint(LintArgs),
    Test(TestArgs),
    Build(BuildArgs),
    Release(ReleaseArgs),
    RunCli(RunArgs),
    RunDesktop(RunArgs),
    Coverage(CoverageArgs),
}

#[derive(Debug, Args)]
struct FmtArgs {
    #[arg(long)]
    check: bool,
}

#[derive(Debug, Args)]
struct ClippyArgs {
    #[arg(long)]
    locked: bool,
}

#[derive(Debug, Args)]
struct LintArgs {
    #[arg(long)]
    check: bool,
    #[arg(long)]
    locked: bool,
}

#[derive(Debug, Args)]
struct TestArgs {
    #[arg(long)]
    locked: bool,
    #[arg(long)]
    lenient_desktop: bool,
}

#[derive(Debug, Args)]
struct BuildArgs {
    #[arg(long)]
    release: bool,
    #[arg(long)]
    locked: bool,
}

#[derive(Debug, Args)]
struct ReleaseArgs {
    #[arg(long)]
    locked: bool,
}

#[derive(Debug, Args)]
struct RunArgs {
    #[arg(long)]
    release: bool,
    #[arg(last = true, allow_hyphen_values = true)]
    args: Vec<OsString>,
}

#[derive(Debug, Args)]
struct CoverageArgs {
    #[arg(long)]
    locked: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("xtask failed: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Task::Fmt(args) => fmt(args),
        Task::Clippy(args) => clippy(args),
        Task::Lint(args) => lint(args),
        Task::Test(args) => test(args),
        Task::Build(args) => build(args),
        Task::Release(args) => release(args),
        Task::RunCli(args) => run_package("rca-cli", args),
        Task::RunDesktop(args) => run_package("rca-desktop", args),
        Task::Coverage(args) => coverage(args),
    }
}

fn fmt(args: FmtArgs) -> Result<()> {
    let mut fmt_args = vec!["fmt", "--all"];
    if args.check {
        fmt_args.push("--");
        fmt_args.push("--check");
    }

    run_cargo(fmt_args, false)
}

fn clippy(args: ClippyArgs) -> Result<()> {
    run_cargo(
        [
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--exclude",
            "xtask",
            "--",
            "-D",
            "warnings",
        ],
        args.locked,
    )
}

fn lint(args: LintArgs) -> Result<()> {
    fmt(FmtArgs { check: args.check })?;
    clippy(ClippyArgs {
        locked: args.locked,
    })
}

fn test(args: TestArgs) -> Result<()> {
    if args.lenient_desktop {
        run_cargo(["test", "-p", "rca-core"], args.locked)?;
        let desktop_status = cargo_status(["test", "-p", "rca-desktop"], args.locked)?;
        if !desktop_status.success() {
            eprintln!("xtask: ignoring rca-desktop test failure because --lenient-desktop was set");
        }
        return Ok(());
    }

    run_cargo(
        [
            "test",
            "--workspace",
            "--all-features",
            "--exclude",
            "xtask",
        ],
        args.locked,
    )
}

fn build(args: BuildArgs) -> Result<()> {
    let mut cargo_args = vec![OsString::from("build")];
    if args.release {
        cargo_args.push(OsString::from("--release"));
    }
    cargo_args.push(OsString::from("--workspace"));
    cargo_args.push(OsString::from("--exclude"));
    cargo_args.push(OsString::from("xtask"));
    run_cargo_os(cargo_args, args.locked)
}

fn release(args: ReleaseArgs) -> Result<()> {
    build(BuildArgs {
        release: true,
        locked: args.locked,
    })
}

fn run_package(package: &str, args: RunArgs) -> Result<()> {
    let mut cargo_args = vec![
        OsString::from("run"),
        OsString::from("-p"),
        OsString::from(package),
    ];
    if args.release {
        cargo_args.push(OsString::from("--release"));
    }
    if !args.args.is_empty() {
        cargo_args.push(OsString::from("--"));
        cargo_args.extend(args.args);
    }

    run_cargo_os(cargo_args, false)
}

fn coverage(args: CoverageArgs) -> Result<()> {
    let version_status = cargo_status(["llvm-cov", "--version"], false)
        .context("failed to check whether cargo-llvm-cov is installed")?;
    if !version_status.success() {
        bail!("cargo-llvm-cov is not available; install it with `cargo install cargo-llvm-cov`");
    }

    run_cargo(
        [
            "llvm-cov",
            "--workspace",
            "--all-features",
            "--exclude",
            "xtask",
        ],
        args.locked,
    )
}

fn run_cargo<I, S>(args: I, locked: bool) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    run_cargo_os(args.into_iter().map(Into::into).collect::<Vec<_>>(), locked)
}

fn run_cargo_os(args: Vec<OsString>, locked: bool) -> Result<()> {
    let status = cargo_status_os(args.clone(), locked)?;
    ensure_success("cargo", &args, status)
}

fn cargo_status<I, S>(args: I, locked: bool) -> Result<ExitStatus>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    cargo_status_os(args.into_iter().map(Into::into).collect::<Vec<_>>(), locked)
}

fn cargo_status_os(mut args: Vec<OsString>, locked: bool) -> Result<ExitStatus> {
    if locked {
        let insertion_index = args
            .iter()
            .position(|arg| arg == "--")
            .unwrap_or(args.len());
        args.insert(insertion_index, OsString::from("--locked"));
    }

    eprintln!("+ cargo {}", shell_join(&args));
    Command::new("cargo")
        .args(&args)
        .status()
        .with_context(|| format!("failed to execute cargo {}", shell_join(&args)))
}

fn ensure_success(program: &str, args: &[OsString], status: ExitStatus) -> Result<()> {
    if status.success() {
        return Ok(());
    }

    match status.code() {
        Some(code) => bail!(
            "{program} {} exited with status code {code}",
            shell_join(args)
        ),
        None => bail!("{program} {} terminated by signal", shell_join(args)),
    }
}

fn shell_join(args: &[OsString]) -> String {
    args.iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}
