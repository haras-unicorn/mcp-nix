//! Sandboxing of programs run from built nix packages and dev shells.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use rmcp::schemars;

pub use crate::sandbox_defaults::{
  BASE_ENV, DEFAULT_BWRAP_COMMAND, DEFAULT_CUDA_SANDBOX_ARGS,
  DEFAULT_NIX_SANDBOX_ARGS, DEFAULT_SANDBOX_ARGS, NVIDIA_GPU_DEVICE,
};

/// Errors produced while running a program from a built nix package or a dev
/// shell command.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
  /// Failed to spawn the sandboxed program.
  #[error("failed to run {program}: {source}")]
  Spawn {
    /// The program being run.
    program: String,
    /// The underlying I/O error.
    #[source]
    source: std::io::Error,
  },
  /// Failed to spawn bubblewrap.
  #[error(
    "failed to spawn the sandbox: {source} (is bubblewrap installed and the sandbox setting valid?)"
  )]
  SandboxSpawn {
    /// The underlying I/O error.
    #[source]
    source: std::io::Error,
  },
  /// There was no command to run.
  #[error("there is no command to run")]
  EmptyCommand,
  /// The working directory is not a directory inside the allowed root.
  #[error("the working directory {cwd} is not a directory inside {root}")]
  WorkingDirectory {
    /// The working directory as it was given.
    cwd: String,
    /// The directory it has to be in.
    root: String,
  },
  /// The program exited with a non-zero status.
  #[error("{program} failed:\n{stderr}")]
  Failed {
    /// The program being run.
    program: String,
    /// The standard error output of the program.
    stderr: String,
  },
}

/// Parse the `sandbox` setting.
///
/// - a blank value disables the sandbox;
/// - any other value is split on whitespace and used as the bubblewrap
///   arguments. The setting defaults to [`DEFAULT_SANDBOX_ARGS`], and a value
///   replaces that default entirely.
pub fn parse_sandbox_args(value: &str) -> Option<Vec<String>> {
  match value.trim() {
    "" => None,
    value => Some(split_args(value)),
  }
}

/// Parse the `bwrap_command` setting: the command that runs bubblewrap, split
/// on whitespace, for example `nsenter --target 1 --mount bwrap`. The setting
/// defaults to [`DEFAULT_BWRAP_COMMAND`]. Only its first word is looked up on
/// the `PATH`.
pub fn parse_bwrap_command(value: &str) -> Vec<String> {
  split_args(value)
}

/// The bubblewrap arguments that give a sandbox the NVIDIA GPUs of the
/// machine, or `None` when there is no GPU, which is when its driver has not
/// made the [`NVIDIA_GPU_DEVICE`] node. They are [`DEFAULT_CUDA_SANDBOX_ARGS`]
/// and a bind for the node of each GPU, like `/dev/nvidia0`.
pub fn cuda_sandbox_args() -> Option<String> {
  cuda_sandbox_args_at(Path::new(NVIDIA_GPU_DEVICE))
}

/// How the root directory of the tools is bound into the sandbox of the
/// programs `nix_run` and `nix_develop` run. It is the `bind_root` setting,
/// as `read-only` or `read-write`.
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  serde::Serialize,
  serde::Deserialize,
  schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum BindRoot {
  /// Programs can read the root.
  ReadOnly,
  /// Programs can read and change the root.
  ReadWrite,
}

/// The bubblewrap arguments that bind the root at its own path. They go after
/// the other sandbox arguments, so that the root shows through a `--tmpfs` of
/// its parent.
pub fn bind_root_args(root: &Path, mode: BindRoot) -> Vec<String> {
  let flag = match mode {
    BindRoot::ReadOnly => "--ro-bind",
    BindRoot::ReadWrite => "--bind",
  };
  let root = root.to_string_lossy();

  [flag, root.as_ref(), root.as_ref()]
    .map(str::to_string)
    .to_vec()
}

/// [`cuda_sandbox_args`] for the machine whose control node is `control`, with
/// the nodes of the GPUs next to it.
fn cuda_sandbox_args_at(control: &Path) -> Option<String> {
  if !control.exists() {
    return None;
  }

  let binds = control
    .parent()
    .map(gpu_nodes)
    .unwrap_or_default()
    .into_iter()
    .map(|node| format!("--dev-bind-try {node} {node}"));

  Some(
    std::iter::once(DEFAULT_CUDA_SANDBOX_ARGS.to_string())
      .chain(binds)
      .collect::<Vec<String>>()
      .join(" "),
  )
}

/// The paths of the nodes of the GPUs in a directory of devices: `nvidia`
/// followed by a number, in order of name.
fn gpu_nodes(directory: &Path) -> Vec<String> {
  let Ok(entries) = std::fs::read_dir(directory) else {
    return Vec::new();
  };

  let mut nodes = entries
    .filter_map(Result::ok)
    .filter(|entry| {
      entry
        .file_name()
        .to_str()
        .and_then(|name| name.strip_prefix("nvidia"))
        .is_some_and(|rest| {
          !rest.is_empty() && rest.chars().all(|digit| digit.is_ascii_digit())
        })
    })
    .map(|entry| entry.path().to_string_lossy().into_owned())
    .collect::<Vec<String>>();
  nodes.sort();
  nodes
}

fn split_args(value: &str) -> Vec<String> {
  value
    .split_whitespace()
    .map(str::to_string)
    .collect::<Vec<String>>()
}

/// The executable file name of a command path, for example `cargo` for
/// `/nix/store/.../bin/cargo`.
fn executable_name(command: &str) -> &str {
  Path::new(command)
    .file_name()
    .and_then(|name| name.to_str())
    .unwrap_or(command)
}

/// Whether the given command is allowed by the allow list of the `commands`
/// setting.
///
/// An empty allow list allows any command. Otherwise the executable file name
/// of `command` must match one of the entries.
pub fn command_allowed(command: &str, allowed: &[String]) -> bool {
  allowed.is_empty()
    || allowed
      .iter()
      .any(|entry| entry.as_str() == executable_name(command))
}

/// Options for running a program from a built nix package or a dev shell
/// command.
#[derive(Debug, Default)]
pub struct RunOptions {
  /// Arguments passed to the program.
  pub args: Vec<String>,
  /// Additional environment variables set for the program.
  pub env: HashMap<String, String>,
  /// The working directory for the program.
  pub cwd: Option<String>,
  /// Bubblewrap arguments, or `None` to run the program directly.
  pub sandbox: Option<Vec<String>>,
  /// The command that runs bubblewrap, for example `nsenter --target 1 --mount
  /// bwrap` as a list of words. Empty means [`DEFAULT_BWRAP_COMMAND`].
  pub bwrap: Vec<String>,
  /// The allow list of the `commands` setting: command file names that may be
  /// executed. Empty means any command is allowed.
  pub allowed_commands: Vec<String>,
  /// Pass `--show-trace` to the nix build or `nix print-dev-env` step so that
  /// nix evaluation errors include the full stack trace.
  pub show_trace: bool,
}

/// Compute the environment set inside the sandbox for a command.
///
/// - `extra` provides an additional environment, for example a captured dev
///   shell environment; a `PATH` in it is merged with the base `PATH`;
/// - the custom environment in [`RunOptions::env`] always wins.
///
/// When `base` is true the minimal [`BASE_ENV`] is included first. When `base`
/// is false only `extra` and the custom environment are included, used to
/// overlay a dev shell environment on an unsandboxed command that otherwise
/// inherits the environment of the server.
pub fn merged_env(
  extra: &[(String, String)],
  options: &RunOptions,
  base: bool,
) -> Vec<(String, String)> {
  let mut env: Vec<(String, String)> = if base {
    BASE_ENV
      .iter()
      .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
      .collect()
  } else {
    Vec::new()
  };

  for (key, value) in extra {
    if key == "PATH" && base {
      let base_path = env
        .iter()
        .find(|(existing, _)| existing == key)
        .map(|(_, value)| value.clone())
        .unwrap_or_default();
      env.push(("PATH".to_string(), format!("{value}:{base_path}")));
    } else {
      env.push((key.clone(), value.clone()));
    }
  }

  for (key, value) in &options.env {
    env.retain(|(existing, _)| existing != key);
    env.push((key.clone(), value.clone()));
  }

  env
}

/// The working directory of a command as a canonical path, or an error when it
/// is not a directory inside `root`.
///
/// A relative `cwd` is taken from `root`. Links are followed before the check,
/// so neither `..` nor a link can lead out of `root`, and the canonical path
/// is the one to use from then on. The sandboxes show the working directory
/// to the command, so a caller that could pick any directory could read
/// anything the user can.
pub(crate) fn confine_working_directory(
  cwd: &str,
  root: &Path,
) -> Result<String, SandboxError> {
  let outside = || SandboxError::WorkingDirectory {
    cwd: cwd.to_string(),
    root: root.display().to_string(),
  };
  let root = root.canonicalize().map_err(|_| outside())?;
  let resolved = root.join(cwd).canonicalize().map_err(|_| outside())?;

  if resolved.is_dir() && resolved.starts_with(&root) {
    Ok(resolved.to_string_lossy().into_owned())
  } else {
    Err(outside())
  }
}

/// Resolve an executable to an absolute path using the current process's
/// `PATH`.
///
/// The program runs with a cleared environment, so executables must be located
/// before their own `PATH` is cleared.
pub fn resolve_on_path(program: &str) -> Result<String, std::io::Error> {
  let path = std::env::var_os("PATH").ok_or_else(|| {
    std::io::Error::new(std::io::ErrorKind::NotFound, "PATH is not set")
  })?;

  for dir in std::env::split_paths(&path) {
    let candidate = dir.join(program);
    if is_executable(&candidate) {
      return Ok(candidate.to_string_lossy().into_owned());
    }
  }

  Err(std::io::Error::new(
    std::io::ErrorKind::NotFound,
    format!("{program} not found on PATH"),
  ))
}

fn is_executable(path: &std::path::Path) -> bool {
  use std::os::unix::fs::PermissionsExt;
  path.metadata().is_ok_and(|metadata| {
    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
  })
}

/// The command that runs bubblewrap, with its first word resolved to an
/// absolute path. An empty command means [`DEFAULT_BWRAP_COMMAND`].
pub(crate) fn resolve_bwrap_command(
  command: &[String],
) -> Result<Vec<String>, SandboxError> {
  let default = [DEFAULT_BWRAP_COMMAND.to_string()];
  let command = if command.is_empty() {
    default.as_slice()
  } else {
    command
  };
  let Some((executable, arguments)) = command.split_first() else {
    return Err(SandboxError::EmptyCommand);
  };

  let executable = resolve_on_path(executable)
    .map_err(|source| SandboxError::SandboxSpawn { source })?;

  let mut resolved = vec![executable];
  resolved.extend(arguments.iter().cloned());
  Ok(resolved)
}

/// Build the argument vector for running `command` with `args`, wrapped in the
/// configured sandbox.
///
/// When [`RunOptions::sandbox`] is `Some`, the [`RunOptions::bwrap`] command is
/// resolved to an absolute path and prepended together with `--clearenv`, the
/// sandbox arguments,
/// `--setenv` pairs for [`merged_env`], an optional `--chdir`, the command and
/// its arguments. `--clearenv` comes first because bubblewrap applies
/// environment arguments in order, so a `--setenv` in the sandbox arguments would
/// otherwise be wiped by it. When it is `None`, the command and its arguments
/// are returned directly.
pub fn sandboxed_argv(
  command: &str,
  args: &[String],
  options: &RunOptions,
  extra_env: &[(String, String)],
) -> Result<Vec<String>, SandboxError> {
  match &options.sandbox {
    Some(bwrap_args) => {
      let mut argv = resolve_bwrap_command(&options.bwrap)?;
      argv.push("--clearenv".to_string());
      argv.extend(bwrap_args.iter().cloned());
      for (key, value) in merged_env(extra_env, options, true) {
        argv.push("--setenv".to_string());
        argv.push(key);
        argv.push(value);
      }
      if let Some(cwd) = &options.cwd {
        argv.push("--chdir".to_string());
        argv.push(cwd.clone());
      }
      argv.push(command.to_string());
      argv.extend(args.iter().cloned());
      Ok(argv)
    }
    None => {
      let mut argv = Vec::new();
      argv.push(command.to_string());
      argv.extend(args.iter().cloned());
      Ok(argv)
    }
  }
}

/// Run the given argument vector, optionally wrapped in a bubblewrap sandbox,
/// and return its standard output.
///
/// When sandboxed the environment is controlled by the `--clearenv`/`--setenv`
/// arguments in `argv`. When unsandboxed the command inherits the environment
/// of the server, with `overlay` applied on top.
pub fn run_command_argv(
  argv: &[String],
  options: &RunOptions,
  overlay: &[(String, String)],
  program: &str,
) -> Result<String, SandboxError> {
  let sandboxed = options.sandbox.is_some();

  let Some((executable, arguments)) = argv.split_first() else {
    return Err(SandboxError::EmptyCommand);
  };

  let mut process = Command::new(executable);
  if let Some(cwd) = &options.cwd {
    process.current_dir(cwd);
  }
  if !sandboxed {
    for (key, value) in overlay {
      process.env(key, value);
    }
  }
  process.args(arguments);

  let output = process.output().map_err(|source| {
    if sandboxed {
      SandboxError::SandboxSpawn { source }
    } else {
      SandboxError::Spawn {
        program: program.to_string(),
        source,
      }
    }
  })?;

  if !output.status.success() {
    return Err(SandboxError::Failed {
      program: program.to_string(),
      stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    });
  }

  Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Run an arbitrary command, optionally wrapped in a bubblewrap sandbox, and
/// return its standard output.
///
/// When sandboxed the command runs with a clean environment: only [`BASE_ENV`]
/// plus the variables in [`RunOptions::env`]. When unsandboxed it inherits the
/// environment of the server, with the variables in [`RunOptions::env`] applied
/// on top.
pub fn run_sandboxed(
  command: &str,
  args: &[String],
  options: &RunOptions,
) -> Result<String, SandboxError> {
  let argv = sandboxed_argv(command, args, options, &[])?;
  let overlay: Vec<(String, String)> = options
    .env
    .iter()
    .map(|(key, value)| (key.clone(), value.clone()))
    .collect();
  run_command_argv(&argv, options, &overlay, command)
}

/// Run a program from a built nix package, optionally wrapped in a bubblewrap
/// sandbox, and return its standard output.
///
/// The program runs with a clean environment when sandboxed: only [`BASE_ENV`]
/// plus the variables in [`RunOptions::env`]. When unsandboxed it inherits the
/// environment of the server, with the variables in [`RunOptions::env`] applied
/// on top.
pub fn run_program(
  store_path: &str,
  program: &str,
  options: &RunOptions,
) -> Result<String, SandboxError> {
  let bin = format!("{store_path}/bin/{program}");
  run_sandboxed(&bin, &options.args, options)
}

/// The bubblewrap sandbox the `nix` commands of the tools run in.
///
/// The pre-build hook only sees builds. What nix fetches while it evaluates,
/// like a flake input or `builtins.fetchurl`, is done by the `nix` command
/// itself, so that command runs without network. Evaluation then only has what
/// is in the nix store, and whatever is missing has to be fetched by a human
/// first, like a fixed-output derivation.
///
/// The sandbox keeps the socket of the nix daemon, which does the building and
/// the downloading of cached outputs, with the hook. A sandbox that has more
/// than that, in particular a way to run `nix` again with a socket to the
/// daemon and no hook, undoes the hook as well.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NixSandbox {
  bwrap_command: Vec<String>,
  args: Vec<String>,
}

impl NixSandbox {
  /// A sandbox run by the given bubblewrap command with the given
  /// arguments. The working directory of a command is added to the arguments,
  /// read-only.
  pub(crate) fn new(bwrap_command: Vec<String>, args: Vec<String>) -> Self {
    Self {
      bwrap_command,
      args,
    }
  }

  /// The program and arguments that run `nix` with the given arguments in the
  /// sandbox. The working directory comes after the sandbox arguments, so
  /// that it shows through a `--tmpfs` of its parent.
  pub(crate) fn wrap(
    &self,
    nix: &str,
    nix_args: &[String],
    cwd: Option<&str>,
  ) -> Result<(String, Vec<String>), SandboxError> {
    let bwrap = resolve_bwrap_command(&self.bwrap_command)?;
    let Some((program, bwrap_args)) = bwrap.split_first() else {
      return Err(SandboxError::EmptyCommand);
    };
    let spawn = |source| SandboxError::Spawn {
      program: nix.to_string(),
      source,
    };
    let cwd = working_directory(cwd).map_err(spawn)?;
    let nix = absolute(nix).map_err(spawn)?;

    let mut arguments = bwrap_args.to_vec();
    arguments.extend(self.args.iter().cloned());
    arguments.extend(
      [
        "--ro-bind",
        cwd.as_str(),
        cwd.as_str(),
        "--chdir",
        cwd.as_str(),
      ]
      .map(str::to_string),
    );
    arguments.push(nix);
    arguments.extend(nix_args.iter().cloned());

    Ok((program.clone(), arguments))
  }
}

fn working_directory(cwd: Option<&str>) -> Result<String, std::io::Error> {
  match cwd {
    Some(cwd) => Ok(cwd.to_string()),
    None => {
      std::env::current_dir().map(|dir| dir.to_string_lossy().into_owned())
    }
  }
}

/// The program as an absolute path with its links followed, because the
/// sandbox has neither the `PATH` of the server nor its links outside of the
/// nix store.
fn absolute(program: &str) -> Result<String, std::io::Error> {
  let path = if program.contains('/') {
    program.to_string()
  } else {
    resolve_on_path(program)?
  };
  Ok(
    std::fs::canonicalize(&path)
      .map_or(path, |canonical| canonical.to_string_lossy().into_owned()),
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn sandbox_args_defaults_restrict_network() {
    let args = parse_sandbox_args(DEFAULT_SANDBOX_ARGS).unwrap_or_default();
    assert!(args.contains(&"--unshare-all".to_string()));
  }

  #[test]
  fn the_default_sandboxes_start_a_new_session() {
    for defaults in [DEFAULT_SANDBOX_ARGS, DEFAULT_NIX_SANDBOX_ARGS] {
      let args = parse_sandbox_args(defaults).unwrap_or_default();

      assert!(args.contains(&"--new-session".to_string()), "{defaults}");
    }
  }

  #[test]
  fn the_cuda_arguments_come_from_a_known_gpu_node_and_bind_it() {
    let args =
      parse_sandbox_args(DEFAULT_CUDA_SANDBOX_ARGS).unwrap_or_default();

    assert!(args.contains(&NVIDIA_GPU_DEVICE.to_string()));
    assert!(args.contains(&"/run/opengl-driver".to_string()));
  }

  #[test]
  fn the_cuda_arguments_only_skip_what_is_missing() {
    let args =
      parse_sandbox_args(DEFAULT_CUDA_SANDBOX_ARGS).unwrap_or_default();

    for flag in args.iter().filter(|arg| arg.starts_with("--")) {
      assert!(flag.ends_with("-try"), "{flag}");
    }
  }

  /// A directory of devices with the given empty files in it, removed on
  /// drop.
  struct Devices {
    directory: std::path::PathBuf,
  }

  impl Devices {
    fn new(name: &str, nodes: &[&str]) -> Self {
      let directory = std::env::temp_dir()
        .join(format!("mcp-nix-devices-{}-{name}", std::process::id()));
      std::fs::create_dir_all(&directory).unwrap();
      for node in nodes {
        std::fs::write(directory.join(node), "").unwrap();
      }
      Self { directory }
    }

    fn control(&self) -> std::path::PathBuf {
      self.directory.join("nvidiactl")
    }
  }

  impl Drop for Devices {
    fn drop(&mut self) {
      let _ = std::fs::remove_dir_all(&self.directory);
    }
  }

  #[test]
  fn there_are_no_cuda_arguments_without_the_control_node() {
    let devices = Devices::new("none", &["nvidia0"]);

    assert_eq!(cuda_sandbox_args_at(&devices.control()), None);
  }

  #[test]
  fn the_cuda_arguments_bind_the_node_of_every_gpu() {
    let devices = Devices::new(
      "gpus",
      &[
        "nvidiactl",
        "nvidia1",
        "nvidia0",
        "nvidia-uvm",
        "nvidia-foo",
        "null",
      ],
    );
    let directory = devices.directory.display();

    let args = cuda_sandbox_args_at(&devices.control()).unwrap();

    assert_eq!(
      args,
      format!(
        "{DEFAULT_CUDA_SANDBOX_ARGS} \
         --dev-bind-try {directory}/nvidia0 {directory}/nvidia0 \
         --dev-bind-try {directory}/nvidia1 {directory}/nvidia1"
      )
    );
  }

  #[test]
  fn the_cuda_arguments_without_gpu_nodes_are_the_defaults() {
    let devices = Devices::new("one", &["nvidiactl"]);

    let args = cuda_sandbox_args_at(&devices.control()).unwrap();

    assert_eq!(args, DEFAULT_CUDA_SANDBOX_ARGS);
  }

  #[test]
  fn the_root_is_bound_at_its_own_path() {
    let root = Path::new("/work");

    assert_eq!(
      bind_root_args(root, BindRoot::ReadOnly),
      ["--ro-bind", "/work", "/work"]
    );
    assert_eq!(
      bind_root_args(root, BindRoot::ReadWrite),
      ["--bind", "/work", "/work"]
    );
  }

  #[test]
  fn sandbox_args_disabled_when_blank() {
    assert_eq!(parse_sandbox_args("   "), None);
  }

  #[test]
  fn sandbox_args_splits_on_whitespace() {
    assert_eq!(
      parse_sandbox_args("--die-with-parent --ro-bind /nix/store /nix/store"),
      Some(vec![
        "--die-with-parent".to_string(),
        "--ro-bind".to_string(),
        "/nix/store".to_string(),
        "/nix/store".to_string(),
      ])
    );
  }

  #[test]
  fn clearenv_precedes_user_sandbox_args() {
    let options = RunOptions {
      sandbox: Some(split_args("--setenv FOO bar")),
      ..RunOptions::default()
    };
    // bwrap may not be on PATH in every environment running unit tests.
    let Ok(argv) = sandboxed_argv("true", &[], &options, &[]) else {
      return;
    };
    let position = |flag: &str| argv.iter().position(|arg| arg == flag);
    let clearenv = position("--clearenv").unwrap();
    let user_setenv = position("--setenv").unwrap();
    assert!(clearenv < user_setenv);
    assert_eq!(
      argv.get(user_setenv.saturating_add(1)),
      Some(&"FOO".to_string())
    );
  }

  #[test]
  fn bwrap_command_is_split_on_whitespace() {
    assert_eq!(
      parse_bwrap_command("nsenter --target 1 bwrap"),
      vec!["nsenter", "--target", "1", "bwrap"]
    );
    assert!(parse_bwrap_command("  ").is_empty());
  }

  #[test]
  fn bwrap_command_prefixes_the_sandbox_and_only_resolves_its_first_word() {
    let options = RunOptions {
      sandbox: Some(split_args("--proc /proc")),
      bwrap: split_args("env FOO=bar bwrap"),
      ..RunOptions::default()
    };

    let argv = sandboxed_argv("true", &[], &options, &[]).unwrap();

    assert!(argv[0].ends_with("/env"), "unexpected argv: {argv:?}");
    assert_eq!(argv[1..4], ["FOO=bar", "bwrap", "--clearenv"]);
  }

  #[test]
  fn empty_bwrap_command_means_the_default() {
    // bwrap may not be on PATH in every environment running unit tests.
    let Ok(argv) = resolve_bwrap_command(&[]) else {
      return;
    };

    assert!(argv[0].ends_with("/bwrap"), "unexpected argv: {argv:?}");
  }

  #[test]
  fn unknown_bwrap_command_is_a_sandbox_spawn_error() {
    let result =
      resolve_bwrap_command(&["mcp-nix-no-such-executable".to_string()]);

    assert!(matches!(result, Err(SandboxError::SandboxSpawn { .. })));
  }

  #[test]
  fn run_command_argv_rejects_an_empty_command() {
    let result = run_command_argv(&[], &RunOptions::default(), &[], "nothing");

    assert!(matches!(result, Err(SandboxError::EmptyCommand)));
  }

  #[test]
  fn command_allowed_matches_executable_file_name() {
    assert!(command_allowed("hello", &[]));
    assert!(command_allowed("hello", &["hello".to_string()]));
    assert!(command_allowed(
      "/nix/store/abc/bin/cargo",
      &["cargo".to_string()]
    ));
    assert!(!command_allowed("hello", &["hello2".to_string()]));
    assert!(!command_allowed(
      "/nix/store/abc/bin/cargo",
      &["make".to_string()]
    ));
  }

  /// A directory with an `inside` directory in it, and a link in it to a
  /// directory next to it, removed on drop.
  struct Tree {
    base: std::path::PathBuf,
  }

  impl Tree {
    fn new(name: &str) -> Self {
      let base = std::env::temp_dir()
        .join(format!("mcp-nix-confine-{}-{name}", std::process::id()));
      std::fs::create_dir_all(base.join("root/inside")).unwrap();
      std::fs::create_dir_all(base.join("outside")).unwrap();
      std::os::unix::fs::symlink(base.join("outside"), base.join("root/link"))
        .unwrap();
      Self { base }
    }

    fn root(&self) -> std::path::PathBuf {
      self.base.join("root")
    }
  }

  impl Drop for Tree {
    fn drop(&mut self) {
      let _ = std::fs::remove_dir_all(&self.base);
    }
  }

  #[test]
  fn a_working_directory_inside_the_root_is_resolved() {
    let tree = Tree::new("inside");
    let inside = tree.root().canonicalize().unwrap().join("inside");
    let expected = inside.to_string_lossy().into_owned();

    for cwd in ["inside", "./inside/..//inside", expected.as_str()] {
      assert_eq!(
        confine_working_directory(cwd, &tree.root()).unwrap(),
        expected,
        "{cwd}"
      );
    }
  }

  #[test]
  fn the_root_is_a_working_directory() {
    let tree = Tree::new("root");
    let expected = tree.root().canonicalize().unwrap();

    assert_eq!(
      confine_working_directory(".", &tree.root()).unwrap(),
      expected.to_string_lossy()
    );
  }

  #[test]
  fn a_working_directory_outside_the_root_is_refused() {
    let tree = Tree::new("outside");
    let outside = tree.base.join("outside").to_string_lossy().into_owned();

    for cwd in ["..", "../outside", "/", "/tmp", outside.as_str()] {
      let result = confine_working_directory(cwd, &tree.root());

      assert!(
        matches!(result, Err(SandboxError::WorkingDirectory { .. })),
        "{cwd}: {result:?}"
      );
    }
  }

  #[test]
  fn a_link_out_of_the_root_is_refused() {
    let tree = Tree::new("link");

    for cwd in ["link", "link/.", "./link/../outside"] {
      let result = confine_working_directory(cwd, &tree.root());

      assert!(
        matches!(result, Err(SandboxError::WorkingDirectory { .. })),
        "{cwd}: {result:?}"
      );
    }
  }

  #[test]
  fn a_missing_working_directory_is_refused() {
    let tree = Tree::new("missing");

    let result = confine_working_directory("nowhere", &tree.root());

    assert!(matches!(result, Err(SandboxError::WorkingDirectory { .. })));
  }

  #[test]
  fn the_nix_sandbox_wraps_nix_and_adds_the_working_directory_last() {
    let sandbox =
      NixSandbox::new(split_args("sh"), split_args("--unshare-all --tmpfs"));

    let (_, arguments) = sandbox
      .wrap("/bin/nix", &["build".to_string()], Some("/work"))
      .unwrap();

    assert_eq!(
      arguments,
      [
        "--unshare-all",
        "--tmpfs",
        "--ro-bind",
        "/work",
        "/work",
        "--chdir",
        "/work",
        "/bin/nix",
        "build"
      ]
    );
  }

  #[test]
  fn the_nix_sandbox_with_an_unknown_bubblewrap_command_is_an_error() {
    let sandbox =
      NixSandbox::new(split_args("mcp-nix-no-such-bwrap"), Vec::new());

    let result = sandbox.wrap("/bin/nix", &[], Some("/work"));

    assert!(matches!(result, Err(SandboxError::SandboxSpawn { .. })));
  }
}
