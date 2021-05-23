use anyhow::{anyhow, Result};
use std::path::PathBuf;

/// Internal representation of the `bootimage runner` command.
pub enum RunnerCommand {
    /// A normal invocation of `bootimage runner` (i.e. no `--help` or `--version`)
    Runner(RunnerArgs),
    /// A command containing `--version`
    Version,
    /// A command containing `--help`
    Help,
}

impl RunnerCommand {
    /// Parse the given argument set into the internal representation.
    pub fn parse_args<A>(args: A) -> Result<Self>
    where
        A: Iterator<Item = String>,
    {
        let mut executable = None;
        let mut quiet = false;
        let mut runner_args = None;
        let mut grub = false;
        let mut release = false;

        let mut arg_iter = args.fuse();

        loop {
            if executable.is_some() {
                let args: Vec<_> = arg_iter.collect();
                if !args.is_empty() {
                    runner_args = Some(args);
                }
                break;
            }
            let next = match arg_iter.next() {
                Some(next) => next,
                None => break,
            };
            match next.as_str() {
                "--help" | "-h" => {
                    return Ok(RunnerCommand::Help);
                }
                "--version" => {
                    return Ok(RunnerCommand::Version);
                }
                "--quiet" => {
                    quiet = true;
                }
                "--grub" => {
                    grub = true;
                }
                "--release" => {
                    release = true;
                }
                exe => {
                    executable = Some(PathBuf::from(exe));
                }
            }
        }

        Ok(Self::Runner(RunnerArgs {
            executable: executable
                .ok_or_else(|| anyhow!("excepted path to kernel executable as first argument"))?,
            quiet,
            release,
            grub,
            runner_args,
        }))
    }
}

/// Arguments for the `bootimage runner` command
#[derive(Debug, Clone)]
pub struct RunnerArgs {
    /// Path to the executable binary
    pub executable: PathBuf,
    /// Suppress any output to stdout.
    pub quiet: bool,
    /// Build release version
    pub release: bool,
    /// Generates an iso with `grub-mkrescue`
    pub grub: bool,
    /// Additional arguments passed to the runner
    pub runner_args: Option<Vec<String>>,
}
