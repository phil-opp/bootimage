use super::error::DiskImageError;
use std::fs::OpenOptions;
use std::io::ErrorKind::AlreadyExists;
use std::io::Write;
use std::{path::Path, process::Command};

pub fn create_disk_image(
    bootloader_elf_path: &Path,
    output_bin_path: &Path,
) -> Result<(), DiskImageError> {
    let llvm_tools = llvm_tools::LlvmTools::new()?;
    let objcopy = llvm_tools
        .tool(&llvm_tools::exe("llvm-objcopy"))
        .ok_or(DiskImageError::LlvmObjcopyNotFound)?;

    // convert bootloader to binary
    let mut cmd = Command::new(objcopy);
    cmd.arg("-I").arg("elf64-x86-64");
    cmd.arg("-O").arg("binary");
    cmd.arg("--binary-architecture=i386:x86-64");
    cmd.arg(bootloader_elf_path);
    cmd.arg(output_bin_path);
    let output = cmd.output().map_err(|err| DiskImageError::Io {
        message: "failed to execute llvm-objcopy command",
        error: err,
    })?;
    if !output.status.success() {
        return Err(DiskImageError::ObjcopyFailed {
            stderr: output.stderr,
        });
    }

    pad_to_nearest_block_size(output_bin_path)?;
    Ok(())
}

pub fn create_iso_image(
    bootloader_elf_path: &Path,
    output_bin_path: &Path,
    isodir: &Path,
    bin_name: &str,
) -> Result<(), DiskImageError> {
    match std::fs::create_dir(isodir) {
        Ok(_) => Ok(()),
        Err(e) => {
            if e.kind() == AlreadyExists {
                Ok(())
            } else {
                Err(DiskImageError::Io {
                    message: "failed to create isodir",
                    error: e,
                })
            }
        }
    }?;

    let grub_dir = isodir.join("boot/grub");
    match std::fs::create_dir_all(&grub_dir) {
        Ok(_) => Ok(()),
        Err(e) => {
            if e.kind() == AlreadyExists {
                Ok(())
            } else {
                Err(DiskImageError::Io {
                    message: "failed to create boot/grub",
                    error: e,
                })
            }
        }
    }?;

    let mut grubcfg = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&grub_dir.join("grub.cfg"))
        .map_err(|err| DiskImageError::Io {
            message: "failed to open grub.cfg",
            error: err,
        })?;

    grubcfg
        .write(
            format!(
                r#"
        set timeout=0
        set default=0

        menuentry "{}" {{
            multiboot2 /boot/kernel.elf
            boot
        }}
        "#,
                bin_name
            )
            .as_bytes(),
        )
        .map_err(|err| DiskImageError::Io {
            message: "failed to write grub.cfg",
            error: err,
        })?;

    std::fs::copy(bootloader_elf_path, isodir.join("boot/kernel.elf")).map_err(|err| {
        DiskImageError::Io {
            message: "failed to create kernel.elf",
            error: err,
        }
    })?;

    let mut cmd = Command::new("grub-mkrescue");
    cmd.arg("-o").arg(output_bin_path);
    cmd.arg(isodir);

    let output = cmd.output().map_err(|err| DiskImageError::Io {
        message: "failed to execute grub-mkrescue command",
        error: err,
    })?;
    if !output.status.success() {
        return Err(DiskImageError::MkResuceFailed {
            stderr: output.stderr,
        });
    }

    Ok(())
}

fn pad_to_nearest_block_size(output_bin_path: &Path) -> Result<(), DiskImageError> {
    const BLOCK_SIZE: u64 = 512;
    let file = OpenOptions::new()
        .write(true)
        .open(&output_bin_path)
        .map_err(|err| DiskImageError::Io {
            message: "failed to open boot image",
            error: err,
        })?;
    let file_size = file
        .metadata()
        .map_err(|err| DiskImageError::Io {
            message: "failed to get size of boot image",
            error: err,
        })?
        .len();
    let remainder = file_size % BLOCK_SIZE;
    let padding = if remainder > 0 {
        BLOCK_SIZE - remainder
    } else {
        0
    };
    file.set_len(file_size + padding)
        .map_err(|err| DiskImageError::Io {
            message: "failed to pad boot image to a multiple of the block size",
            error: err,
        })
}
