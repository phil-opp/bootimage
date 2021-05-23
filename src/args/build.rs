use anyhow::{anyhow, Context, Result};
use std::{
    mem,
    path::{Path, PathBuf},
};

/// Internal representation of the `cargo bootimage` command.
pub enum BuildCommand {
    /// A normal invocation (i.e. no `--help` or `--version`)
    Build(BuildArgs),
    /// The `--version` command
    Version,
    /// The `--help` command
    Help,
}

impl BuildCommand {
    /// Parse the command line args into a `BuildCommand`.
    pub fn parse_args<A>(args: A) -> Result<Self>
    where
        A: Iterator<Item = String>,
    {
        let mut manifest_path: Option<PathBuf> = None;
        let mut cargo_args = Vec::new();
        let mut quiet = false;
        let mut grub = false;
        let mut release = false;
        {
            fn set<T>(arg: &mut Option<T>, value: Option<T>) -> Result<()> {
                let previous = mem::replace(arg, value);
                if previous.is_some() {
                    return Err(anyhow!("multiple arguments of same type provided"));
                }
                Ok(())
            }

            let mut arg_iter = args;
            while let Some(arg) = arg_iter.next() {
                match arg.as_ref() {
                    "--help" | "-h" => {
                        return Ok(BuildCommand::Help);
                    }
                    "--version" => {
                        return Ok(BuildCommand::Version);
                    }
                    "--quiet" => {
                        quiet = true;
                    }
                    "--release" => {
                        release = true;
                    }
                    "--grub" => {
                        grub = true;
                    }
                    "--manifest-path" => {
                        let next = arg_iter.next();
                        set(
                            &mut manifest_path,
                            next.as_ref()
                                .map(|p| Path::new(&p).canonicalize())
                                .transpose()
                                .context("--manifest-path invalid")?,
                        )?;
                        cargo_args.push(arg);
                        if let Some(next) = next {
                            cargo_args.push(next);
                        }
                    }
                    _ if arg.starts_with("--manifest-path=") => {
                        let path = Path::new(arg.trim_start_matches("--manifest-path="))
                            .canonicalize()
                            .context("--manifest-path invalid")?;
                        set(&mut manifest_path, Some(path))?;
                        cargo_args.push(arg);
                    }
                    _ => {
                        cargo_args.push(arg);
                    }
                };
            }
        }

        Ok(BuildCommand::Build(BuildArgs {
            manifest_path,
            cargo_args,
            quiet,
            release,
            grub,
        }))
    }
}

/// Arguments passed to `cargo bootimage`.
#[derive(Debug, Clone)]
pub struct BuildArgs {
    /// The manifest path (also present in `cargo_args`).
    manifest_path: Option<PathBuf>,
    /// All arguments that are passed to cargo.
    cargo_args: Vec<String>,
    /// Suppress any output to stdout.
    quiet: bool,
    /// Build release version
    release: bool,
    /// Generates an iso with `grub-mkrescue`
    grub: bool,
}

impl BuildArgs {
    /// The value of the `--manifest-path` argument, if any.
    pub fn manifest_path(&self) -> Option<&Path> {
        self.manifest_path.as_deref()
    }

    /// Arguments that should be forwarded to `cargo build`.
    pub fn cargo_args(&self) -> &[String] {
        &self.cargo_args.as_ref()
    }

    /// Whether a `--quiet` flag was passed.
    pub fn quiet(&self) -> bool {
        self.quiet
    }

    /// Whether a `--release` flag was passed.
    pub fn release(&self) -> bool {
        self.release
    }
    /// Whether a `--grub` flag was passed.
    pub fn grub(&self) -> bool {
        self.grub
    }
}
