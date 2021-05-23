//! Provides functions to build the kernel and the bootloader.

use crate::config::Config;
use cargo_metadata::Metadata;
use error::{BootloaderError, BuildKernelError, BuilderError, CreateBootimageError};
use std::{
    path::{Path, PathBuf},
    process,
};

/// Provides the build command for the bootloader.
mod bootloader;
/// Provides a function to create the bootable disk image.
mod disk_image;
/// Contains the errors types returned by the `Builder` methods.
pub mod error;

/// Allows building the kernel and creating a bootable disk image with it.
pub struct Builder {
    manifest_path: PathBuf,
    project_metadata: Option<Metadata>,
}

/// Arguments to create_bootimage
pub struct Bootimage<'a> {
    /// Path to kernel Cargo.toml
    pub kernel_manifest: &'a Path,
    /// Path to kernel binary
    pub bin_path: &'a Path,
    /// Output binary path
    pub output_bin_path: &'a Path,
    /// Disable logging
    pub quiet: bool,
    /// Use release build
    pub release: bool,
    /// Use grub bootloader
    pub grub: bool,
    /// Output directory
    pub iso_dir_path: &'a Path,
    /// Your project name / binary name
    pub bin_name: &'a str,
}

impl Builder {
    /// Creates a new builder for the project at the given manifest path
    ///
    /// If None is passed for `manifest_path`, it is automatically searched.
    pub fn new(manifest_path: Option<PathBuf>) -> Result<Self, BuilderError> {
        let manifest_path = match manifest_path.or_else(|| {
            std::env::var("CARGO_MANIFEST_DIR")
                .ok()
                .map(|dir| Path::new(&dir).join("Cargo.toml"))
        }) {
            Some(path) => path,
            None => {
                println!("WARNING: `CARGO_MANIFEST_DIR` env variable not set");
                locate_cargo_manifest::locate_manifest()?
            }
        };

        Ok(Builder {
            manifest_path,
            project_metadata: None,
        })
    }

    /// Returns the path to the Cargo.toml file of the project.
    pub fn manifest_path(&self) -> &Path {
        &self.manifest_path
    }

    /// Builds the kernel by executing `cargo build` with the given arguments.
    ///
    /// Returns a list of paths to all built executables. For crates with only a single binary,
    /// the returned list contains only a single element.
    ///
    /// If the quiet argument is set to true, all output to stdout is suppressed.
    pub fn build_kernel(
        &mut self,
        args: &[String],
        config: &Config,
        quiet: bool,
    ) -> Result<Vec<PathBuf>, BuildKernelError> {
        if !quiet {
            println!("Building kernel");
        }

        // try to build kernel
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
        let mut cmd = process::Command::new(&cargo);
        cmd.args(&config.build_command);
        cmd.args(args);
        if !quiet {
            cmd.stdout(process::Stdio::inherit());
            cmd.stderr(process::Stdio::inherit());
        }
        let output = cmd.output().map_err(|err| BuildKernelError::Io {
            message: "failed to execute kernel build",
            error: err,
        })?;
        if !output.status.success() {
            if config.build_command.starts_with(&["xbuild".into()]) {
                // try executing `cargo xbuild --help` to check whether cargo-xbuild is installed
                let mut help_command = process::Command::new("cargo");
                help_command.arg("xbuild").arg("--help");
                help_command.stdout(process::Stdio::null());
                help_command.stderr(process::Stdio::null());
                if let Ok(help_exit_status) = help_command.status() {
                    if !help_exit_status.success() {
                        return Err(BuildKernelError::XbuildNotFound);
                    }
                }
            }
            return Err(BuildKernelError::BuildFailed {
                stderr: output.stderr,
            });
        }

        // Retrieve binary paths
        let mut cmd = process::Command::new(cargo);
        cmd.args(&config.build_command);
        cmd.args(args);
        cmd.arg("--message-format").arg("json");
        let output = cmd.output().map_err(|err| BuildKernelError::Io {
            message: "failed to execute kernel build with json output",
            error: err,
        })?;
        if !output.status.success() {
            return Err(BuildKernelError::BuildFailed {
                stderr: output.stderr,
            });
        }
        let mut executables = Vec::new();
        for line in String::from_utf8(output.stdout)
            .map_err(BuildKernelError::BuildJsonOutputInvalidUtf8)?
            .lines()
        {
            let mut artifact =
                json::parse(line).map_err(BuildKernelError::BuildJsonOutputInvalidJson)?;
            if let Some(executable) = artifact["executable"].take_string() {
                executables.push(PathBuf::from(executable));
            }
        }

        Ok(executables)
    }

    /// Creates a bootimage by combining the given kernel binary with the bootloader.
    ///
    /// Places the resulting bootable disk image at the given `output_bin_path`.
    ///
    /// If the quiet argument is set to true, all output to stdout is suppressed.
    pub fn create_bootimage(&mut self, args: &Bootimage) -> Result<(), CreateBootimageError> {
        let bootloader_build_config = bootloader::BuildConfig::from_metadata(
            self.project_metadata()?,
            args.kernel_manifest,
            args.bin_path,
        )?;

        // build bootloader
        if !args.quiet {
            println!("Building bootloader");
        }
        let mut cmd = bootloader_build_config.build_command(args.release);
        if !args.quiet {
            cmd.stdout(process::Stdio::inherit());
            cmd.stderr(process::Stdio::inherit());
        }
        let output = cmd.output().map_err(|err| CreateBootimageError::Io {
            message: "failed to execute bootloader build command",
            error: err,
        })?;
        if !output.status.success() {
            return Err(CreateBootimageError::BootloaderBuildFailed {
                stderr: output.stderr,
            });
        }

        // Retrieve binary path
        let mut cmd = bootloader_build_config.build_command(args.release);
        cmd.arg("--message-format").arg("json");
        let output = cmd.output().map_err(|err| CreateBootimageError::Io {
            message: "failed to execute bootloader build command with json output",
            error: err,
        })?;
        if !output.status.success() {
            return Err(CreateBootimageError::BootloaderBuildFailed {
                stderr: output.stderr,
            });
        }
        let mut bootloader_elf_path = None;
        for line in String::from_utf8(output.stdout)
            .map_err(CreateBootimageError::BuildJsonOutputInvalidUtf8)?
            .lines()
        {
            let mut artifact =
                json::parse(line).map_err(CreateBootimageError::BuildJsonOutputInvalidJson)?;
            if let Some(executable) = artifact["executable"].take_string() {
                if bootloader_elf_path
                    .replace(PathBuf::from(executable))
                    .is_some()
                {
                    return Err(BootloaderError::BootloaderInvalid(
                        "bootloader has multiple executables".into(),
                    )
                    .into());
                }
            }
        }
        let bootloader_elf_path = bootloader_elf_path.ok_or_else(|| {
            BootloaderError::BootloaderInvalid("bootloader has no executable".into())
        })?;

        if args.grub {
            disk_image::create_iso_image(
                &bootloader_elf_path,
                args.output_bin_path,
                args.iso_dir_path,
                args.bin_name,
            )?;
        } else {
            disk_image::create_disk_image(&bootloader_elf_path, args.output_bin_path)?;
        }

        Ok(())
    }

    /// Returns the cargo metadata package that contains the given binary.
    pub fn kernel_package_for_bin(
        &mut self,
        kernel_bin_name: &str,
    ) -> Result<Option<&cargo_metadata::Package>, cargo_metadata::Error> {
        Ok(self.project_metadata()?.packages.iter().find(|p| {
            p.targets
                .iter()
                .any(|t| t.name == kernel_bin_name && t.kind.iter().any(|k| k == "bin"))
        }))
    }

    fn project_metadata(&mut self) -> Result<&Metadata, cargo_metadata::Error> {
        if let Some(ref metadata) = self.project_metadata {
            return Ok(metadata);
        }
        let metadata = cargo_metadata::MetadataCommand::new()
            .manifest_path(&self.manifest_path)
            .exec()?;
        Ok(self.project_metadata.get_or_insert(metadata))
    }
}
