# bootimage

Creates a bootable disk image from a Rust OS kernel.

## Installation

```
> cargo install bootimage
```

## Usage

First you need to add a dependency on the [`bootloader`](https://github.com/rust-osdev/bootloader) crate:

```toml
# in your Cargo.toml

[dependencies]
bootloader = "0.9.8"
```

**Note**: At least bootloader version `0.5.1` is required since `bootimage 0.7.0`. For earlier bootloader versions, use `bootimage 0.6.6`.

If you want to use a custom bootloader with a different name, you can use Cargo's [rename functionality](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#renaming-dependencies-in-cargotoml).

### Building

Now you can build the kernel project and create a bootable disk image from it by running:

```
cargo bootimage --target your_custom_target.json [other_args]
```

The command will invoke `cargo build`, forwarding all passed options. Then it will build the specified bootloader together with the kernel to create a bootable disk image.

### Build with Grub
You can set the `--grub` flag to enable grub compilation mode. It uses `grub-mkrescue` to generate a bootable iso with your kernel inside linked with the bootloader crate of your choosing.
The bootloader crate must support the multiboot2 specification.

### Running

To run your kernel in QEMU, you can set a `bootimage runner` as a custom runner in a `.cargo/config` file:

```toml
[target.'cfg(target_os = "none")']
runner = "bootimage runner"
```

Then you can run your kernel through:

```
cargo xrun --target your_custom_target.json [other_args] -- [qemu args]
```

All arguments after `--` are passed to QEMU. If you want to use a custom run command, see the _Configuration_ section below.

### Testing

The `bootimage` has built-in support for running unit and integration tests of your kernel. For this, you need to use the `custom_tests_framework` feature of Rust as described [here](https://os.phil-opp.com/testing/#custom-test-frameworks).

## Configuration

Configuration is done through a through a `[package.metadata.bootimage]` table in the `Cargo.toml` of your kernel. The following options are available:

```toml
[package.metadata.bootimage]
# The cargo subcommand that will be used for building the kernel.
#
# For building using the `cargo-xbuild` crate, set this to `xbuild`.
build-command = ["build"]
# The command invoked with the created bootimage (the "{}" will be replaced
# with the path to the bootable disk image)
# Applies to `bootimage run` and `bootimage runner`
run-command = ["qemu-system-x86_64", "-drive", "format=raw,file={}"]

# Additional arguments passed to the run command for non-test executables
# Applies to `bootimage run` and `bootimage runner`
run-args = []

# Additional arguments passed to the run command for test executables
# Applies to `bootimage runner`
test-args = []

# An exit code that should be considered as success for test executables
test-success-exit-code = {integer}

# The timeout for running a test through `bootimage test` or `bootimage runner` (in seconds)
test-timeout = 300

# Whether the `-no-reboot` flag should be passed to test executables
test-no-reboot = true
```

### Inner workings
The `bootimage` command first reads the `CARGO_MANIFEST_DIR` environment variable to find out where the `Cargo.toml` of the current project is located.
Then it parses the `Cargo.toml` file to read bootimage specific configuration data out of it. It then proceeds to build the current cargo project.
Afterwards it looks if a crate named `bootloader` is defined as dependency. It reads the `Cargo.toml` of the `bootloader` crate and looks for more bootimage specific configuration data. These fields must be available:
```toml
[features]
# Since cargo doesn't support binary-only dependencies, `bootimage` manually turns on
# a feature named `binary`. This way, bootloader crates can use optional dependencies
# to improve their compile times when built as library. 
binary = []

[package.metadata.bootloader]
# A default target specification can be printed with:
# rustc +nightly -Z unstable-options --print target-spec-json --target i686-unknown-linux-gnu
target = "i686-unknown-linux-gnu.json"
# The sysroot crate that should be built using cargo's build-std feature. If this key is not
# present `cargo-xbuild` is used for building.
build-std = "core"
```
It then proceeds to build the crate with the `--features binary` flag set. The `KERNEL` environment variable is also set to the path of the elf executable of your kernel. Additionally, a `KERNEL_MANIFEST` environment variable is set to point to the `Cargo.toml` of your kernel.
The bootloader crate defines a linker script and a build.rs, the inner workings of which are described here: https://github.com/rust-osdev/bootloader#build-chain
The last step is for bootimage to create a bootable image out of it.
The default behaviour is to `objcopy -I elf64-x86-64 -O binary --binary-architecture=i386:x86-64 <bootloader_elf_path> <output_bin_path>`.
However if the `--grub` flag is set instead it creates following folder structure in the same directory of your kernel executable:
```
target/x86_64-os/debug/isofiles
└── boot
    ├── grub
    │   └── grub.cfg
    └── kernel.elf
```
It then executes `grub-mkrescue -o <output_bin_path>.iso <target_path>/isofiles` to create a bootable grub iso image.
This mode is not compatible with the [bootloader](https://github.com/rust-osdev/bootloader) crate from rust-osdev.


## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
