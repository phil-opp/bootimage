//! Parses the `package.metadata.bootimage` configuration table

use anyhow::{anyhow, Context, Result};
use std::path::Path;
use toml::Value;

/// Represents the `package.metadata.bootimage` configuration table
///
/// The bootimage crate can be configured through a `package.metadata.bootimage` table
/// in the `Cargo.toml` file of the kernel. This struct represents the parsed configuration
/// options.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Config {
    /// The cargo subcommand that is used for building the kernel for `cargo bootimage`.
    ///
    /// Defaults to `build`.
    pub build_command: Vec<String>,
    /// The run command that is invoked on `bootimage run` or `bootimage runner`
    ///
    /// The substring "{}" will be replaced with the path to the bootable disk image.
    pub run_command: Vec<String>,
    /// Additional arguments passed to the runner for not-test binaries
    ///
    /// Applies to `bootimage run` and `bootimage runner`.
    pub run_args: Option<Vec<String>>,
    /// Additional arguments passed to the runner for test binaries
    ///
    /// Applies to `bootimage runner`.
    pub test_args: Option<Vec<String>>,
    /// The timeout for running an test through `bootimage test` or `bootimage runner` in seconds
    pub test_timeout: u32,
    /// An exit code that should be considered as success for test executables (applies to
    /// `bootimage runner`)
    pub test_success_exit_code: Option<i32>,
    /// Whether the `-no-reboot` flag should be passed to test executables
    ///
    /// Defaults to `true`
    pub test_no_reboot: bool,
}

/// Reads the configuration from a `package.metadata.bootimage` in the given Cargo.toml.
pub fn read_config(manifest_path: &Path) -> Result<Config> {
    read_config_inner(manifest_path).context("Failed to read bootimage configuration")
}

fn read_config_inner(manifest_path: &Path) -> Result<Config> {
    use std::{fs::File, io::Read};
    let cargo_toml: Value = {
        let mut content = String::new();
        File::open(manifest_path)
            .context("Failed to open Cargo.toml")?
            .read_to_string(&mut content)
            .context("Failed to read Cargo.toml")?;
        content
            .parse::<Value>()
            .context("Failed to parse Cargo.toml")?
    };

    let metadata = cargo_toml
        .get("package")
        .and_then(|table| table.get("metadata"))
        .and_then(|table| table.get("bootimage"));
    let metadata = match metadata {
        None => {
            return Ok(ConfigBuilder::default().into());
        }
        Some(metadata) => metadata
            .as_table()
            .ok_or_else(|| anyhow!("Bootimage configuration invalid: {:?}", metadata))?,
    };

    let mut config = ConfigBuilder::default();

    for (key, value) in metadata {
        match (key.as_str(), value.clone()) {
            ("test-timeout", Value::Integer(timeout)) if timeout.is_negative() => {
                return Err(anyhow!("test-timeout must not be negative"))
            }
            ("test-timeout", Value::Integer(timeout)) => {
                config.test_timeout = Some(timeout as u32);
            }
            ("test-success-exit-code", Value::Integer(exit_code)) => {
                config.test_success_exit_code = Some(exit_code as i32);
            }
            ("build-command", Value::Array(array)) => {
                config.build_command = Some(parse_string_array(array, "build-command")?);
            }
            ("run-command", Value::Array(array)) => {
                config.run_command = Some(parse_string_array(array, "run-command")?);
            }
            ("run-args", Value::Array(array)) => {
                config.run_args = Some(parse_string_array(array, "run-args")?);
            }
            ("test-args", Value::Array(array)) => {
                config.test_args = Some(parse_string_array(array, "test-args")?);
            }
            ("test-no-reboot", Value::Boolean(no_reboot)) => {
                config.test_no_reboot = Some(no_reboot);
            }
            (key, value) => {
                return Err(anyhow!(
                    "unexpected `package.metadata.bootimage` \
                 key `{}` with value `{}`",
                    key,
                    value
                ))
            }
        }
    }
    Ok(config.into())
}

fn parse_string_array(array: Vec<Value>, prop_name: &str) -> Result<Vec<String>> {
    let mut parsed = Vec::new();
    for value in array {
        match value {
            Value::String(s) => parsed.push(s),
            _ => return Err(anyhow!("{} must be a list of strings", prop_name)),
        }
    }
    Ok(parsed)
}

#[derive(Default)]
struct ConfigBuilder {
    build_command: Option<Vec<String>>,
    run_command: Option<Vec<String>>,
    run_args: Option<Vec<String>>,
    test_args: Option<Vec<String>>,
    test_timeout: Option<u32>,
    test_success_exit_code: Option<i32>,
    test_no_reboot: Option<bool>,
}

impl From<ConfigBuilder> for Config {
    fn from(s: ConfigBuilder) -> Config {
        Config {
            build_command: s.build_command.unwrap_or_else(|| vec!["build".into()]),
            run_command: s.run_command.unwrap_or_else(|| {
                vec![
                    "qemu-system-x86_64".into(),
                    "-drive".into(),
                    "format=raw,file={}".into(),
                ]
            }),
            run_args: s.run_args,
            test_args: s.test_args,
            test_timeout: s.test_timeout.unwrap_or(60 * 5),
            test_success_exit_code: s.test_success_exit_code,
            test_no_reboot: s.test_no_reboot.unwrap_or(true),
        }
    }
}
