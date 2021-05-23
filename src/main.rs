/// Executable for `bootimage runner`.
use anyhow::{anyhow, Context, Result};
use bootimage::{
    args::{RunnerArgs, RunnerCommand},
    builder::Builder,
    config, help, run,
};
use std::process;
use std::{env, path::Path};

pub fn main() -> Result<()> {
    let mut raw_args = env::args();

    let executable_name = raw_args
        .next()
        .ok_or_else(|| anyhow!("no first argument (executable name)"))?;
    let file_stem = Path::new(&executable_name)
        .file_stem()
        .and_then(|s| s.to_str());
    if file_stem != Some("bootimage") {
        return Err(anyhow!(
            "Unexpected executable name: expected `bootimage`, got: `{:?}`",
            file_stem
        ));
    }
    match raw_args.next().as_deref() {
        Some("runner") => {},
        Some("--help") | Some("-h") => {
            help::print_help();
            return Ok(())
        }
        Some("--version") => {
            help::print_version();
            return Ok(())
        }
        Some(other) => return Err(anyhow!(
            "Unsupported subcommand `{:?}`. See `bootimage --help` for an overview of supported subcommands.", other
        )),
        None => return Err(anyhow!(
            "Please invoke bootimage with a subcommand. See `bootimage --help` for more information."
        )),
    }

    let exit_code = match RunnerCommand::parse_args(raw_args)? {
        RunnerCommand::Runner(args) => Some(runner(args)?),
        RunnerCommand::Version => {
            help::print_version();
            None
        }
        RunnerCommand::Help => {
            help::print_runner_help();
            None
        }
    };

    if let Some(code) = exit_code {
        process::exit(code);
    }

    Ok(())
}

pub(crate) fn runner(args: RunnerArgs) -> Result<i32> {
    let mut builder = Builder::new(None)?;
    let config = config::read_config(builder.manifest_path())?;
    let exe_parent = args
        .executable
        .parent()
        .ok_or_else(|| anyhow!("kernel executable has no parent"))?;
    let is_doctest = exe_parent
        .file_name()
        .ok_or_else(|| anyhow!("kernel executable's parent has no file name"))?
        .to_str()
        .ok_or_else(|| anyhow!("kernel executable's parent file name is not valid UTF-8"))?
        .starts_with("rustdoctest");
    let is_test = is_doctest || exe_parent.ends_with("deps");

    let iso_files = exe_parent.join("isofiles");

    let bin_name = args
        .executable
        .file_stem()
        .ok_or_else(|| anyhow!("kernel executable has no file stem"))?
        .to_str()
        .ok_or_else(|| anyhow!("kernel executable file stem is not valid UTF-8"))?;

    let output_bin_path = if args.grub {
        exe_parent.join(format!("bootimage-{}.iso", bin_name))
    } else {
        exe_parent.join(format!("bootimage-{}.bin", bin_name))
    };
    let executable_canonicalized = args.executable.canonicalize().with_context(|| {
        format!(
            "failed to canonicalize executable path `{}`",
            args.executable.display(),
        )
    })?;

    // Cargo sets a CARGO_MANIFEST_DIR environment variable for all runner
    // executables. This variable contains the path to the Cargo.toml of the
    // crate that the executable belongs to (i.e. not the project root
    // manifest for workspace projects)
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")
        .context("Failed to read CARGO_MANIFEST_DIR environment variable")?;
    let kernel_manifest_path = Path::new(&manifest_dir).join("Cargo.toml");

    let bootimage = bootimage::builder::Bootimage {
        kernel_manifest: &kernel_manifest_path,
        bin_path: &executable_canonicalized,
        output_bin_path: &output_bin_path,
        quiet: args.quiet,
        release: args.release,
        grub: args.grub,
        iso_dir_path: &iso_files,
        bin_name: &bin_name,
    };

    builder.create_bootimage(&bootimage)?;

    let exit_code = run::run(config, args, &output_bin_path, is_test)?;

    Ok(exit_code)
}
