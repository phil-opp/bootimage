use std::{io, path::PathBuf};
use thiserror::Error;

/// Represents an error that occurred while creating a new `Builder`.
#[derive(Debug, Error)]
pub enum BuilderError {
    /// Failed to locate cargo manifest
    #[error("Could not find Cargo.toml file starting from current folder: {0:?}")]
    LocateCargoManifest(#[from] locate_cargo_manifest::LocateManifestError),
}

/// Represents an error that occurred when building the kernel.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BuildKernelError {
    /// An unexpected I/O error occurred
    #[error("I/O error: {message}:\n{error}")]
    Io {
        /// Desciption of the failed I/O operation
        message: &'static str,
        /// The I/O error that occured
        error: io::Error,
    },

    /// Could not find the `cargo xbuild` tool. Perhaps it is not installed?
    #[error(
        "Failed to run `cargo xbuild`. Perhaps it is not installed?\n\
    Run `cargo install cargo-xbuild` to install it."
    )]
    XbuildNotFound,

    /// Running `cargo build` failed.
    #[error("Kernel build failed.\nStderr: {}", String::from_utf8_lossy(.stderr))]
    BuildFailed {
        /// The standard error output.
        stderr: Vec<u8>,
    },

    /// The output of `cargo build --message-format=json` was not valid UTF-8
    #[error("Output of kernel build with --message-format=json is not valid UTF-8:\n{0}")]
    BuildJsonOutputInvalidUtf8(std::string::FromUtf8Error),
    /// The output of `cargo build --message-format=json` was not valid JSON
    #[error("Output of kernel build with --message-format=json is not valid JSON:\n{0}")]
    BuildJsonOutputInvalidJson(json::Error),
}

/// Represents an error that occurred when creating a bootimage.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CreateBootimageError {
    /// Failed to build the bootloader.
    #[error("An error occured while trying to build the bootloader: {0}")]
    Bootloader(#[from] BootloaderError),

    /// Error while running `cargo metadata`
    #[error("Error while running `cargo metadata` for current project: {0:?}")]
    CargoMetadata(#[from] cargo_metadata::Error),

    /// Building the bootloader failed
    #[error("Bootloader build failed.\nStderr: {}", String::from_utf8_lossy(.stderr))]
    BootloaderBuildFailed {
        /// The `cargo build` output to standard error
        stderr: Vec<u8>,
    },

    /// Disk image creation failed
    #[error("An error occured while trying to create the disk image: {0}")]
    DiskImage(#[from] DiskImageError),

    /// An unexpected I/O error occurred
    #[error("I/O error: {message}:\n{error}")]
    Io {
        /// Desciption of the failed I/O operation
        message: &'static str,
        /// The I/O error that occured
        error: io::Error,
    },

    /// The output of `cargo build --message-format=json` was not valid UTF-8
    #[error("Output of bootloader build with --message-format=json is not valid UTF-8:\n{0}")]
    BuildJsonOutputInvalidUtf8(std::string::FromUtf8Error),
    /// The output of `cargo build --message-format=json` was not valid JSON
    #[error("Output of bootloader build with --message-format=json is not valid JSON:\n{0}")]
    BuildJsonOutputInvalidJson(json::Error),
}

/// There is something wrong with the bootloader dependency.
#[derive(Debug, Error)]
pub enum BootloaderError {
    /// Bootloader dependency not found
    #[error(
        "Bootloader dependency not found\n\n\
        You need to add a dependency on a crate named `bootloader` in your Cargo.toml."
    )]
    BootloaderNotFound,

    /// Bootloader dependency has not the right format
    #[error("The `bootloader` dependency has not the right format: {0}")]
    BootloaderInvalid(String),

    /// Could not find kernel package in cargo metadata
    #[error(
        "Could not find package with manifest path `{manifest_path}` in cargo metadata output"
    )]
    KernelPackageNotFound {
        /// The manifest path of the kernel package
        manifest_path: PathBuf,
    },

    /// Could not find some required information in the `cargo metadata` output
    #[error("Could not find required key `{key}` in cargo metadata output")]
    CargoMetadataIncomplete {
        /// The required key that was not found
        key: String,
    },
}

/// Creating the disk image failed.
#[derive(Debug, Error)]
pub enum DiskImageError {
    /// The `llvm-tools-preview` rustup component was not found
    #[error(
        "Could not find the `llvm-tools-preview` rustup component.\n\n\
        You can install by executing `rustup component add llvm-tools-preview`."
    )]
    LlvmToolsNotFound,

    /// There was another problem locating the `llvm-tools-preview` rustup component
    #[error("Failed to locate the `llvm-tools-preview` rustup component: {0:?}")]
    LlvmTools(llvm_tools::Error),

    /// The llvm-tools component did not contain the required `llvm-objcopy` executable
    #[error("Could not find `llvm-objcopy` in the `llvm-tools-preview` rustup component.")]
    LlvmObjcopyNotFound,

    /// The `llvm-objcopy` command failed
    #[error("Failed to run `llvm-objcopy`: {}", String::from_utf8_lossy(.stderr))]
    ObjcopyFailed {
        /// The output of `llvm-objcopy` to standard error
        stderr: Vec<u8>,
    },

    /// The `grub-mkrescue` command failed
    #[error("Failed to run `grub-mkrescue`: {}", String::from_utf8_lossy(.stderr))]
    MkResuceFailed {
        /// The output of `grub-mkrescue` to standard error
        stderr: Vec<u8>,
    },
    /// An unexpected I/O error occurred
    #[error("I/O error: {message}:\n{error}")]
    Io {
        /// Desciption of the failed I/O operation
        message: &'static str,
        /// The I/O error that occured
        error: io::Error,
    },
}

impl From<llvm_tools::Error> for DiskImageError {
    fn from(err: llvm_tools::Error) -> Self {
        match err {
            llvm_tools::Error::NotFound => DiskImageError::LlvmToolsNotFound,
            other => DiskImageError::LlvmTools(other),
        }
    }
}
