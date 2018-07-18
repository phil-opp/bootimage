use std::fs::{self, File};
use std::{io, process};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::io::Write;
use byteorder::{ByteOrder, LittleEndian};
use args::{self, Args};
use config::{self, Config};
use cargo_metadata::{self, Metadata as CargoMetadata, Package as CrateMetadata};
use failure::{Error, ResultExt};
use xmas_elf;
use tempdir::TempDir;

const BLOCK_SIZE: usize = 512;
type KernelInfoBlock = [u8; BLOCK_SIZE];

pub(crate) fn build(args: Args) -> Result<(), Error> {
    let (args, config, metadata, root_dir, out_dir) = common_setup(args)?;

    build_impl(&args, &config, &metadata, &root_dir, &out_dir, true)?;
    Ok(())
}

pub(crate) fn run(args: Args) -> Result<(), Error> {
    let (args, config, metadata, root_dir, out_dir) = common_setup(args)?;

    let output_path = build_impl(&args, &config, &metadata, &root_dir, &out_dir, true)?;
    run_impl(&args, &config, &output_path)
}

pub(crate) fn common_setup(mut args: Args) -> Result<(Args, Config, CargoMetadata, PathBuf, PathBuf), Error> {
    fn out_dir(args: &Args, metadata: &CargoMetadata) -> PathBuf {
        let target_dir = PathBuf::from(&metadata.target_directory);
        let mut out_dir = target_dir;
        if let &Some(ref target) = args.target() {
            out_dir.push(Path::new(target).file_stem().unwrap().to_str().unwrap());
        }
        if args.release() {
            out_dir.push("release");
        } else {
            out_dir.push("debug");
        }
        out_dir
    }

    let metadata = read_cargo_metadata(&args)?;
    let crate_root = PathBuf::from(&metadata.workspace_root);
    let manifest_path = args.manifest_path().as_ref().map(Clone::clone).unwrap_or({
        let mut path = crate_root.clone();
        path.push("Cargo.toml");
        path
    });
    let config = config::read_config(manifest_path)?;

    if args.target().is_none() {
        if let Some(ref target) = config.default_target {
            let mut canonicalized_target = crate_root.clone();
            canonicalized_target.push(target);
            args.set_target(canonicalized_target.to_string_lossy().into_owned());
        }
    }

    if let &Some(ref target) = args.target() {
        if !target.ends_with(".json") {
            use std::io::{self, Write};
            use std::process;

            writeln!(
                io::stderr(),
                "Please pass a path to `--target` (with `.json` extension`): `--target {}.json`",
                target
            ).unwrap();
            process::exit(1);
        }
    }

    let out_dir = out_dir(&args, &metadata);

    Ok((args, config, metadata, crate_root, out_dir))
}

pub(crate) fn build_impl(
    args: &Args,
    config: &Config,
    metadata: &CargoMetadata,
    root_dir: &Path,
    out_dir: &Path,
    verbose: bool,
) -> Result<PathBuf, Error> {
    let crate_ = metadata
        .packages
        .iter()
        .find(|p| Path::new(&p.manifest_path) == config.manifest_path)
        .expect("Could not read crate name from cargo metadata");
    let bin_name: String = args.bin_name().as_ref().unwrap_or(&crate_.name).clone();

    let kernel = build_kernel(&out_dir, &bin_name, &args, verbose)?;

    let kernel_size = kernel.metadata().context("Failed to read kernel output file")?.len();
    let kernel_info_block = create_kernel_info_block(kernel_size);

    if args.update_bootloader() {
        let mut bootloader_cargo_lock = PathBuf::from(out_dir);
        bootloader_cargo_lock.push("bootloader");
        bootloader_cargo_lock.push("Cargo.lock");

        fs::remove_file(bootloader_cargo_lock).context("Failed to remove Cargo.lock")?;
    }

    let tmp_dir = TempDir::new("bootloader").context("Failed to create a temporary directory")?;
    let bootloader = build_bootloader(tmp_dir.path(), &config).context("Failed to build bootloader")?;
    tmp_dir.close().context("Failed to close temporary directory")?;

    create_disk_image(root_dir, out_dir, &bin_name, &config, kernel, kernel_info_block, &bootloader, verbose)
}

fn run_impl(args: &Args, config: &Config, output_path: &Path) -> Result<(), Error> {
    let command = &config.run_command[0];
    let mut command = process::Command::new(command);
    for arg in &config.run_command[1..] {
        command.arg(
            arg.replace(
                "{}",
                output_path
                    .to_str()
                    .expect("output must be valid unicode"),
            ),
        );
    }
    command.args(&args.run_args);
    command.status().context(format_err!("Failed to execute run command: {:?}", command))?;
    Ok(())
}

#[derive(Debug, Fail)]
#[fail(display = "Failed to execute `cargo metadata`")]
pub struct CargoMetadataError(Mutex<cargo_metadata::Error>);

fn read_cargo_metadata(args: &Args) -> Result<CargoMetadata, Error> {
    let metadata = cargo_metadata::metadata(args.manifest_path().as_ref().map(PathBuf::as_path))
        .map_err(|e| CargoMetadataError(Mutex::new(e)))?;
    Ok(metadata)
}

fn build_kernel(
    out_dir: &Path,
    bin_name: &str,
    args: &args::Args,
    verbose: bool,
) -> Result<File, Error> {
    // compile kernel
    if verbose {
        println!("Building kernel");
    }
    let exit_status = run_xbuild(&args.cargo_args)
        .context("Failed to run `cargo xbuild`")?;
    if !exit_status.success() {
        process::exit(1)
    }

    let mut kernel_path = out_dir.to_owned();
    kernel_path.push(bin_name);
    let kernel = File::open(kernel_path).context("Failed to open kernel output file")?;
    Ok(kernel)
}

fn run_xbuild(args: &[String]) -> io::Result<process::ExitStatus> {
    let mut command = process::Command::new("cargo");
    command.arg("xbuild");
    command.args(args);
    let exit_status = command.status()?;

    if !exit_status.success() {
        let mut help_command = process::Command::new("cargo");
        help_command.arg("xbuild").arg("--help");
        help_command.stdout(process::Stdio::null());
        help_command.stderr(process::Stdio::null());
        if let Ok(help_exit_status) = help_command.status() {
            if !help_exit_status.success() {
                let mut stderr = io::stderr();
                writeln!(stderr, "Failed to run `cargo xbuild`. Perhaps it is not installed?")?;
                writeln!(stderr, "Run `cargo install cargo-xbuild` to install it.")?;
            }
        }
    }

    Ok(exit_status)
}

fn create_kernel_info_block(kernel_size: u64) -> KernelInfoBlock {
    let kernel_size = if kernel_size <= u64::from(u32::max_value()) {
        kernel_size as u32
    } else {
        panic!("Kernel can't be loaded by BIOS bootloader because is too big")
    };

    let mut kernel_info_block = [0u8; BLOCK_SIZE];
    LittleEndian::write_u32(&mut kernel_info_block[0..4], kernel_size);

    kernel_info_block
}

fn download_bootloader(bootloader_dir: &Path, config: &Config) -> Result<CrateMetadata, Error> {
    use std::io::Write;

    let cargo_toml = {
        let mut dir = bootloader_dir.to_owned();
        dir.push("Cargo.toml");
        dir
    };
    let src_lib = {
        let mut dir = bootloader_dir.to_owned();
        dir.push("src");
        fs::create_dir_all(dir.as_path())
            .context("Failed to create directory for bootloader download crate")?;
        dir.push("lib.rs");
        dir
    };

    {
        let mut cargo_toml_file = File::create(&cargo_toml)
            .context("Failed to create Cargo.toml for bootloader download crate")?;
        cargo_toml_file.write_all(
            r#"
            [package]
            authors = ["author@example.com>"]
            name = "bootloader_download_helper"
            version = "0.0.0"

        "#.as_bytes(),
        ).context("Failed to write to Cargo.toml for bootloader download crate")?;
        cargo_toml_file.write_all(
            format!(
                r#"
            [dependencies.{}]
            version = "{}"
        "#,
                config.bootloader.name, config.bootloader.version
            ).as_bytes(),
        ).context("Failed to write to Cargo.toml for bootloader download crate")?;
        if let &Some(ref git) = &config.bootloader.git {
            cargo_toml_file.write_all(
                format!(
                    r#"
                    git = "{}"
            "#,
                    git
                ).as_bytes(),
            ).context("Failed to write to Cargo.toml for bootloader download crate")?;
        }
        if let &Some(ref branch) = &config.bootloader.branch {
            cargo_toml_file.write_all(
                format!(
                    r#"
                    branch = "{}"
            "#,
                    branch
                ).as_bytes(),
            ).context("Failed to write to Cargo.toml for bootloader download crate")?;
        }
        if let &Some(ref path) = &config.bootloader.path {
            cargo_toml_file.write_all(
                format!(
                    r#"
                    path = "{}"
            "#,
                    path.display()
                ).as_bytes(),
            ).context("Failed to write to Cargo.toml for bootloader download crate")?;
        }

        File::create(src_lib).context("Failed to create lib.rs for bootloader download crate")?
            .write_all(
                r#"
                #![no_std]
            "#.as_bytes(),
            ).context("Failed to write to lib.rs for bootloader download crate")?;
    }

    let mut command = process::Command::new("cargo");
    command.arg("fetch");
    command.current_dir(bootloader_dir);
    if !command.status()?.success() {
        Err(format_err!("Bootloader download failed."))?
    }

    let metadata = cargo_metadata::metadata_deps(Some(&cargo_toml), true)
        .map_err(|e| CargoMetadataError(Mutex::new(e)))?;
    let bootloader = metadata
        .packages
        .iter()
        .find(|p| p.name == config.bootloader.name)
        .ok_or(format_err!(
            "Could not find crate named “{}”",
            config.bootloader.name
        ))?;

    Ok(bootloader.clone())
}

fn build_bootloader(bootloader_dir: &Path, config: &Config) -> Result<Box<[u8]>, Error> {
    use std::io::Read;

    let bootloader_metadata = download_bootloader(bootloader_dir, config)?;
    let bootloader_dir = Path::new(&bootloader_metadata.manifest_path)
        .parent()
        .unwrap();

    let mut bootloader_target_path = PathBuf::from(bootloader_dir);
    bootloader_target_path.push(&config.bootloader.target);

    let bootloader_elf_path = if !config.bootloader.precompiled {
        let args = &[
            String::from("--manifest-path"),
            bootloader_metadata.manifest_path.clone(),
            String::from("--target"),
            bootloader_target_path.display().to_string(),
            String::from("--release"),
        ];

        println!("Building bootloader");
        let exit_status = run_xbuild(args).context("Failed to run `cargo xbuild`")?;
        if !exit_status.success() {
            process::exit(1)
        }

        let mut bootloader_elf_path = bootloader_dir.to_path_buf();
        bootloader_elf_path.push("target");
        bootloader_elf_path.push(config.bootloader.target.file_stem().unwrap());
        bootloader_elf_path.push("release");
        bootloader_elf_path.push("bootloader");
        bootloader_elf_path
    } else {
        let mut bootloader_elf_path = bootloader_dir.to_path_buf();
        bootloader_elf_path.push("bootloader");
        bootloader_elf_path
    };

    let mut bootloader_elf_bytes = Vec::new();
    let mut bootloader = File::open(&bootloader_elf_path).context("Could not open bootloader")?;
    bootloader.read_to_end(&mut bootloader_elf_bytes).context("Could not read bootloader")?;

    // copy bootloader section of ELF file to bootloader_path
    let elf_file = xmas_elf::ElfFile::new(&bootloader_elf_bytes).unwrap();
    xmas_elf::header::sanity_check(&elf_file).unwrap();
    let bootloader_section = elf_file
        .find_section_by_name(".bootloader")
        .expect("bootloader must have a .bootloader section");

    Ok(Vec::from(bootloader_section.raw_data(&elf_file)).into_boxed_slice())
}

fn create_disk_image(
    root_dir: &Path,
    out_dir: &Path,
    bin_name: &str,
    config: &Config,
    mut kernel: File,
    kernel_info_block: KernelInfoBlock,
    bootloader_data: &[u8],
    verbose: bool,
) -> Result<PathBuf, Error> {
    use std::io::{Read, Write};

    let mut output_path = PathBuf::from(out_dir);
    let file_name = format!("bootimage-{}.bin", bin_name);
    output_path.push(file_name);

    if let Some(ref output) = config.output {
        output_path = output.clone();
    }

    if verbose {
        println!("Creating disk image at {}",
            output_path.strip_prefix(root_dir).unwrap_or(output_path.as_path()).display());
    }
    let mut output = File::create(&output_path).context("Could not create output bootimage file")?;
    output.write_all(&bootloader_data).context("Could not write output bootimage file")?;
    output.write_all(&kernel_info_block).context("Could not write output bootimage file")?;

    // write out kernel elf file
    let kernel_size = kernel.metadata()?.len();
    let mut buffer = [0u8; 1024];
    loop {
        let (n, interrupted) = match kernel.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => (n, false),
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => (0, true),
            Err(e) => Err(e)?,
        };
        if !interrupted {
            output.write_all(&buffer[..n])?
        }
    }

    let padding_size = ((512 - (kernel_size % 512)) % 512) as usize;
    let padding = [0u8; 512];
    output.write_all(&padding[..padding_size]).context("Could not write output bootimage file")?;

    if let Some(min_size) = config.minimum_image_size {
        // we already wrote to output successfully,
        // both metadata and set_len should succeed.
        if output.metadata()?.len() < min_size {
            output.set_len(min_size)?;
        }
    }

    Ok(output_path)
}
