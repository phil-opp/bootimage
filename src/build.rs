use args::{self, Args};
use byteorder::{ByteOrder, LittleEndian};
use cargo_metadata::{self, Metadata as CargoMetadata};
use config::{self, Config};
use failure::{self, Error, ResultExt};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::{fmt, io, process};
use xmas_elf;

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

pub(crate) fn common_setup(
    mut args: Args,
) -> Result<(Args, Config, CargoMetadata, PathBuf, PathBuf), Error> {
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

    let maybe_package = if let Some(ref path) = config.package_filepath {
        Some(File::open(path).with_context(|e| format!("Unable to open specified package file: {}", e))?)
    } else {
        None
    };

    let maybe_package_size = if let Some(ref file) = maybe_package {
        Some(file.metadata().with_context(|e| format!("Failed to read specified package file: {}", e))?.len())
    } else {
        None
    };

    let kernel_size = kernel
        .metadata()
        .with_context(|e| format!("Failed to read kernel output file: {}", e))?
        .len();
    let kernel_info_block = create_kernel_info_block(kernel_size, maybe_package_size);

    let bootloader = build_bootloader(&metadata, &config, verbose)
        .with_context(|e| format!("Failed to build bootloader: {}", e))?;

    create_disk_image(
        root_dir,
        out_dir,
        &bin_name,
        &config,
        kernel,
        maybe_package,
        kernel_info_block,
        &bootloader,
        verbose,
    )
}

fn run_impl(args: &Args, config: &Config, output_path: &Path) -> Result<(), Error> {
    let command = &config.run_command[0];
    let mut command = process::Command::new(command);
    for arg in &config.run_command[1..] {
        command.arg(arg.replace(
            "{}",
            output_path.to_str().expect("output must be valid unicode"),
        ));
    }
    command.args(&args.run_args);
    command
        .status()
        .with_context(|e| format!("Failed to execute run `{:?}`: {}", command, e))?;
    Ok(())
}

#[derive(Debug)]
pub struct CargoMetadataError {
    error: String,
}

impl fmt::Display for CargoMetadataError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.error)
    }
}

impl failure::Fail for CargoMetadataError {}

fn read_cargo_metadata(args: &Args) -> Result<CargoMetadata, Error> {
    run_cargo_fetch();
    let metadata =
        cargo_metadata::metadata_deps(args.manifest_path().as_ref().map(PathBuf::as_path), true)
            .map_err(|e| CargoMetadataError {
                error: format!("{}", e),
            })?;
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
        .with_context(|e| format!("Failed to run `cargo xbuild`: {}", e))?;
    if !exit_status.success() {
        process::exit(1)
    }

    let mut kernel_path = out_dir.to_owned();
    kernel_path.push(bin_name);
    let kernel = File::open(kernel_path)
        .with_context(|e| format!("Failed to open kernel output file: {}", e))?;
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
                writeln!(
                    stderr,
                    "Failed to run `cargo xbuild`. Perhaps it is not installed?"
                )?;
                writeln!(stderr, "Run `cargo install cargo-xbuild` to install it.")?;
            }
        }
    }

    Ok(exit_status)
}

fn run_cargo_fetch() {
    let mut command = process::Command::new("cargo");
    command.arg("fetch");
    if !command.status().map(|s| s.success()).unwrap_or(false) {
        process::exit(1);
    }
}

fn create_kernel_info_block(kernel_size: u64, maybe_package_size: Option<u64>) -> KernelInfoBlock {
    let kernel_size = if kernel_size <= u64::from(u32::max_value()) {
        kernel_size as u32
    } else {
        panic!("Kernel can't be loaded by BIOS bootloader because is too big")
    };

    let package_size = if let Some(size) = maybe_package_size {
        if size <= u64::from(u32::max_value()) {
            size as u32
        } else {
            panic!("Package can't be loaded by BIOS bootloader because is too big")
        }
    } else {
        0
    };

    let mut kernel_info_block = [0u8; BLOCK_SIZE];
    LittleEndian::write_u32(&mut kernel_info_block[0..4], kernel_size);
    LittleEndian::write_u32(&mut kernel_info_block[8..12], package_size);

    kernel_info_block
}

fn build_bootloader(metadata: &CargoMetadata, config: &Config, verbose: bool) -> Result<Box<[u8]>, Error> {
    use std::io::Read;

    let bootloader_metadata = metadata.packages.iter().find(|p| {
        if let Some(name) = config.bootloader.name.as_ref() {
            p.name == name.as_str()
        } else {
            p.name == "bootloader" || p.name == "bootloader_precompiled"
        }
    });
    let bootloader_metadata =
        match bootloader_metadata {
            Some(package_metadata) => package_metadata.clone(),
            None => Err(format_err!("Bootloader dependency not found\n\n\
            You need to add a dependency on the `bootloader` or `bootloader_precompiled` crates \
            in your Cargo.toml.\n\nIn case you just updated bootimage from an earlier version, \
            check out the migration guide at https://github.com/rust-osdev/bootimage/pull/16. \
            Alternatively, you can downgrade to bootimage 0.4 again by executing \
            `cargo install bootimage --version {} --force`.", r#""^0.4""#
        ))?,
        };
    let bootloader_dir = Path::new(&bootloader_metadata.manifest_path)
        .parent()
        .unwrap();

    let mut bootloader_target_path = PathBuf::from(bootloader_dir);
    bootloader_target_path.push(&config.bootloader.target);

    let bootloader_elf_path = if bootloader_metadata.name == "bootloader_precompiled" {
        let mut bootloader_elf_path = bootloader_dir.to_path_buf();
        bootloader_elf_path.push("bootloader");
        bootloader_elf_path
    } else {
        let mut args = vec![
            String::from("--manifest-path"),
            bootloader_metadata.manifest_path.clone(),
            String::from("--target"),
            bootloader_target_path.display().to_string(),
            String::from("--release"),
            String::from("--features"),
            config
                .bootloader
                .features
                .iter()
                .fold(String::new(), |i, j| i + " " + j),
        ];

        if !config.bootloader.default_features {
            args.push(String::from("--no-default-features"));
        }

        if verbose {
            args.push(String::from("--verbose"));
        }

        println!("Building bootloader v{}", bootloader_metadata.version);
        let exit_status =
            run_xbuild(&args).with_context(|e| format!("Failed to run `cargo xbuild`: {}", e))?;
        if !exit_status.success() {
            process::exit(1)
        }

        let mut bootloader_elf_path = bootloader_dir.to_path_buf();
        bootloader_elf_path.push("target");
        bootloader_elf_path.push(config.bootloader.target.file_stem().unwrap());
        bootloader_elf_path.push("release");
        bootloader_elf_path.push("bootloader");
        bootloader_elf_path
    };

    let mut bootloader_elf_bytes = Vec::new();
    let mut bootloader = File::open(&bootloader_elf_path)
        .with_context(|e| format!("Could not open bootloader: {}", e))?;
    bootloader
        .read_to_end(&mut bootloader_elf_bytes)
        .with_context(|e| format!("Could not read bootloader: {}", e))?;

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
    mut maybe_package: Option<File>,
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
        println!(
            "Creating disk image at {}",
            output_path
                .strip_prefix(root_dir)
                .unwrap_or(output_path.as_path())
                .display()
        );
    }
    let mut output = File::create(&output_path)
        .with_context(|e| format!("Could not create output bootimage file: {}", e))?;
    output
        .write_all(&bootloader_data)
        .with_context(|e| format!("Could not write output bootimage file: {}", e))?;
    output
        .write_all(&kernel_info_block)
        .with_context(|e| format!("Could not write output bootimage file: {}", e))?;

    fn write_file_to_file(output: &mut File, datafile: &mut File) -> Result<usize, Error> {
        let data_size = datafile.metadata()?.len();
        let mut buffer = [0u8; 1024];
        let mut acc = 0;
        loop {
            let (n, interrupted) = match datafile.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => (n, false),
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => (0, true),
                Err(e) => Err(e)?,
            };
            if !interrupted {
                acc += n;
                output.write_all(&buffer[..n])?
            }
        }

        assert!(data_size == acc as u64);

        Ok(acc)
    }

    fn pad_file(output: &mut File, written_size: usize, padding: &[u8]) -> Result<(), Error> {
        let padding_size = (padding.len() - (written_size % padding.len())) % padding.len();
        output.write_all(&padding[..padding_size]).with_context(|e| format!("Could not write to output file: {}", e))?;
        Ok(())
    }

    // write out kernel elf file

    let kernel_size = write_file_to_file(&mut output, &mut kernel)?;

    pad_file(&mut output, kernel_size, &[0; 512])?;

    if let Some(ref mut package) = maybe_package {
        println!("Writing specified package to output");
        let package_size = write_file_to_file(&mut output, package)?;
        pad_file(&mut output, package_size, &[0; 512])?;
    }

    if let Some(min_size) = config.minimum_image_size {
        // we already wrote to output successfully,
        // both metadata and set_len should succeed.
        if output.metadata()?.len() < min_size {
            output.set_len(min_size)?;
        }
    }

    Ok(output_path)
}
