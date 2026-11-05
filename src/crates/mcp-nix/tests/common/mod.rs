//! Shared helpers for the integration tests.

#![allow(
  dead_code,
  reason = "each test crate uses a different part of these helpers"
)]
#![allow(clippy::print_stderr, reason = "a skipped test says why on stderr")]
#![allow(
  clippy::unwrap_used,
  clippy::expect_used,
  clippy::panic,
  reason = "helpers of tests fail by panicking, like the tests"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use mcp_nix::BuildGuard;

/// The nixpkgs revision locked in the repository's `flake.lock`, injected by
/// the `MCP_NIX_NIXPKGS_REV` environment variable set in the dev shell and the
/// package build, so that it can never drift from the lock file.
const NIXPKGS_REV: &str = env!("MCP_NIX_NIXPKGS_REV");

/// Bubblewrap arguments used by the sandboxed tests.
///
/// Note that the default sandbox of the server also disables the network
/// (`--unshare-net`); the tests only add it explicitly when exercising the
/// network.
pub const TEST_BWRAP_ARGS: &[&str] = &[
  "--die-with-parent",
  "--unshare-user",
  "--unshare-ipc",
  "--unshare-pid",
  "--ro-bind",
  "/nix",
  "/nix",
  "--ro-bind",
  "/bin/sh",
  "/bin/sh",
  "--tmpfs",
  "/tmp",
  "--proc",
  "/proc",
  "--dev",
  "/dev",
];

/// A self-contained flake written to a temporary directory.
pub struct TempFlake {
  /// The directory containing the flake.
  pub dir: PathBuf,
}

impl TempFlake {
  /// Write the given flake contents to a new temporary directory.
  pub fn new(flake_nix: &str) -> Self {
    let dir =
      std::env::temp_dir().join(format!("mcp-nix-test-{}", unique_suffix()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("flake.nix"), flake_nix).unwrap();
    Self { dir }
  }

  /// Write a file next to the flake.
  pub fn write(&self, name: &str, contents: &str) {
    fs::write(self.dir.join(name), contents).unwrap();
  }
}

impl Drop for TempFlake {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.dir);
  }
}

/// A flake whose package build echoes 25 lines to stdout so they end up in the
/// build log.
pub const ECHO_FLAKE: &str = r#"
{
  outputs = { self }: {
    packages.x86_64-linux.default = derivation {
      name = "mcp-nix-log-test";
      system = "x86_64-linux";
      builder = "/bin/sh";
      args = [ "-c"
        "i=1; while [ $i -le 25 ]; do echo \"log-line-$i\"; i=$((i + 1)); done; echo done > \"$out\""
      ];
    };
  };
}
"#;

/// A flake with a fixed-output derivation whose hash is right, offered as the
/// default package, and a package, a check and a dev shell that use it. The
/// builder only writes a local file, but nix would give it network access.
/// The dev shell needs the store `bash` as its builder, which `@bash@` stands
/// in for. The path of a fixed-output derivation comes from its name and hash
/// alone, so `@unique@` makes the name differ between tests that share a
/// store.
const FIXED_OUTPUT_FLAKE: &str = r#"
{
  outputs = { self }:
    let
      fod = derivation {
        name = "mcp-nix-fixed-output-test-@unique@";
        system = "x86_64-linux";
        builder = "/bin/sh";
        args = [ "-c" "echo mcp-nix-test > \"$out\"" ];
        outputHashMode = "flat";
        outputHash = "sha256-E3MwNTPc5Nd2XQFqub3Vj6dsMkn79RP0K1IQaXNpUX4=";
      };
      dependent = derivation {
        name = "mcp-nix-fixed-output-dependent";
        system = "x86_64-linux";
        builder = "/bin/sh";
        args = [ "-c" "echo ${fod} > \"$out\"" ];
      };
    in
    {
      packages.x86_64-linux = {
        default = fod;
        inherit dependent;
      };
      checks.x86_64-linux.default = dependent;
      devShells.x86_64-linux.default = derivation {
        name = "mcp-nix-fixed-output-shell";
        system = "x86_64-linux";
        builder = "@bash@/bin/bash";
        args = [ "-c" "echo done > \"$out\"" ];
        MCP_NIX_FOD = "${fod}";
      };
    };
}
"#;

/// A [`FIXED_OUTPUT_FLAKE`] written to a temporary directory.
pub fn fixed_output_flake() -> TempFlake {
  let bash = store_bin_dir("bash").unwrap_or_default();
  TempFlake::new(
    &FIXED_OUTPUT_FLAKE
      .replace("@bash@", &bash)
      .replace("@unique@", &unique_suffix()),
  )
}

/// A suffix that differs between calls, across tests and test runs.
///
/// The clock alone is not enough, because tests run in parallel and can read
/// the same time, so a counter tells them apart.
pub fn unique_suffix() -> String {
  static COUNTER: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

  let nanos = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap_or(std::time::Duration::ZERO)
    .as_nanos();
  let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

  format!("{}-{nanos}-{count}", process::id())
}

/// A flake whose package is named by the contents of a fixed-output
/// derivation, so that evaluating it means building that derivation first.
pub const IMPORTED_FIXED_OUTPUT_FLAKE: &str = r#"
{
  outputs = { self }:
    let
      fod = derivation {
        name = "mcp-nix-imported-fixed-output";
        system = "x86_64-linux";
        builder = "/bin/sh";
        args = [ "-c" "echo '\"imported\"' > \"$out\"" ];
        outputHashMode = "flat";
        outputHash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
      };
    in
    {
      packages.x86_64-linux.default = derivation {
        name = import fod;
        system = "x86_64-linux";
        builder = "/bin/sh";
        args = [ "-c" "echo done > \"$out\"" ];
      };
    };
}
"#;

/// A mock HTTP server on the loopback interface that counts the requests it
/// gets. The tools must never give the caller the network, so a test serves a
/// file from here and expects that nothing asks for it.
pub struct MockServer {
  server: wiremock::MockServer,
  runtime: tokio::runtime::Runtime,
}

impl MockServer {
  /// Start the server. It answers "not found" until [`MockServer::serve`] is
  /// called.
  pub fn start() -> Self {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(wiremock::MockServer::start());
    Self { server, runtime }
  }

  /// The host and port of the server, as they are in a URL.
  pub fn address(&self) -> String {
    self.server.address().to_string()
  }

  /// Answer every request with the given body.
  pub fn serve(&self, body: &str) {
    let response = wiremock::ResponseTemplate::new(200).set_body_string(body);
    self.runtime.block_on(
      wiremock::Mock::given(wiremock::matchers::any())
        .respond_with(response)
        .mount(&self.server),
    );
  }

  /// How many requests the server got so far.
  pub fn requests(&self) -> usize {
    self
      .runtime
      .block_on(self.server.received_requests())
      .map_or(0, |requests| requests.len())
  }
}

/// Assert that this environment can build with a real nix, which needs the
/// `nix-command` feature and a store to build in. A test that needs it fails
/// when it cannot, and does not skip, so a broken environment cannot hide.
///
/// The build runs once per test binary and the tests that come after it wait
/// for it, because without a daemon tests that open a fresh store at the same
/// time fail with a busy database.
pub fn assert_nix_builds() {
  static BUILD: std::sync::OnceLock<Result<(), String>> =
    std::sync::OnceLock::new();

  let result = BUILD.get_or_init(|| {
    let flake = TempFlake::new(ECHO_FLAKE);
    let output = process::Command::new("nix")
      .args([
        "build",
        "--no-link",
        &format!("{}#default", flake.dir.display()),
      ])
      .output()
      .expect("failed to spawn nix");

    if output.status.success() {
      Ok(())
    } else {
      Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
  });

  if let Err(error) = result {
    panic!("nix cannot build here: {error}");
  }
}

/// The guard the tests run `nix` with: the hook binary built next to the
/// tests.
pub fn guard() -> BuildGuard {
  BuildGuard::pre_build_hook(env!("CARGO_BIN_EXE_mcp-nix-hook"))
}

/// The guard of [`guard`], with `nix` run in its sandbox, or `None` when
/// bubblewrap cannot be used here.
///
/// The sandbox is the default one, with what a test environment needs on top
/// of it. There is usually no daemon, so `nix` works on a store of its own,
/// which the default `NIX_REMOTE` would keep it from, and runs the hook
/// itself, and the flakes of the tests are in the temporary directory.
pub fn sandboxed_guard() -> Option<BuildGuard> {
  if !bwrap_usable() {
    return None;
  }

  let temporary = std::env::temp_dir().display().to_string();
  let hook = Path::new(env!("CARGO_BIN_EXE_mcp-nix-hook"));
  let hook_dir = hook.parent().map(|dir| dir.display().to_string());
  let home = std::env::var("HOME").unwrap_or_default();

  let mut args = mcp_nix::parse_sandbox_args(mcp_nix::DEFAULT_NIX_SANDBOX_ARGS)
    .unwrap_or_default();
  let extra = [
    ("--ro-bind-try", "/bin".to_string()),
    ("--ro-bind-try", "/usr".to_string()),
    ("--ro-bind-try", hook_dir.unwrap_or_default()),
    ("--bind-try", "/nix/var/nix".to_string()),
    ("--bind-try", format!("{home}/.local/share/nix")),
    ("--bind", temporary),
  ];
  for (flag, path) in extra {
    args.extend([flag.to_string(), path.clone(), path]);
  }
  args.extend(["--setenv", "NIX_REMOTE", "auto"].map(str::to_string));

  Some(guard().sandboxed(
    mcp_nix::parse_bwrap_command(mcp_nix::DEFAULT_BWRAP_COMMAND),
    args,
  ))
}

/// A flake whose package is a derivation with the given extra attributes,
/// for example `__noChroot = true;`. The builder only writes a local file.
pub fn hook_flake(attributes: &str) -> String {
  format!(
    r#"
{{
  outputs = {{ self }}: {{
    packages.x86_64-linux.default = derivation {{
      name = "mcp-nix-hook-test";
      system = "x86_64-linux";
      builder = "/bin/sh";
      args = [ "-c" "echo done > \"$out\"" ];
      {attributes}
    }};
  }};
}}
"#
  )
}

/// A flake whose package asks to be built outside the sandbox.
pub const NO_CHROOT_FLAKE: &str = r#"
{
  outputs = { self }: {
    packages.x86_64-linux.default = derivation {
      name = "mcp-nix-no-chroot-test";
      system = "x86_64-linux";
      builder = "/bin/sh";
      args = [ "-c" "echo done > \"$out\"" ];
      __noChroot = true;
    };
  };
}
"#;

/// Assert that an error message says the pre-build hook refused a build.
pub fn assert_build_refused(message: &str) {
  assert!(
    message.contains("pre-build hook refused"),
    "expected the refusal of the pre-build hook, got: {message}"
  );
}

/// Assert that an error message says a build outside the sandbox was refused.
///
/// With the sandbox on, nix refuses `__noChroot` itself before it gets to the
/// hook, and with the sandbox relaxed or off the hook does. Which one happens
/// depends on where the tests run, and both keep the build from running.
pub fn assert_leaving_sandbox_refused(message: &str) {
  assert!(
    message.contains("pre-build hook refused")
      || message.contains("'__noChroot' set"),
    "expected a refusal of a build outside the sandbox, got: {message}"
  );
}

/// Build the default package of the given flake and return its store path.
///
/// This is plain `nix build`, so it builds fixed-output derivations too.
pub fn build_flake_package(flake: &TempFlake) -> String {
  let output = process::Command::new("nix")
    .args([
      "build",
      "--no-link",
      "--print-out-paths",
      &format!("{}#default", flake.dir.display()),
    ])
    .output()
    .expect("failed to spawn nix build");
  assert!(
    output.status.success(),
    "nix build failed: {}",
    String::from_utf8_lossy(&output.stderr)
  );
  String::from_utf8_lossy(&output.stdout)
    .split_whitespace()
    .next()
    .expect("nix build produced no output")
    .to_string()
}

/// Whether the build log of a freshly built store path can be retrieved with
/// `nix log`, or `None` when it is not available.
///
/// A build failure is a hard error; only the log lookup is treated as an
/// environment precondition, so tests can skip when the build log is not
/// retained (for example on CI).
pub fn build_flake_with_log(flake: &TempFlake) -> Option<String> {
  assert_nix_builds();
  let store_path = build_flake_package(flake);
  let output = process::Command::new("nix")
    .args(["log", &store_path])
    .output()
    .expect("failed to spawn nix log");
  if !output.status.success() {
    return None;
  }
  if String::from_utf8_lossy(&output.stdout).trim().is_empty() {
    return None;
  }
  Some(store_path)
}

/// Return the nix store directory containing the given program, or `None`
/// when the program does not resolve into the store.
pub fn store_bin_dir(program: &str) -> Option<String> {
  let output = process::Command::new("sh")
    .args(["-c", &format!("command -v {program}")])
    .output()
    .ok()?;
  if !output.status.success() {
    return None;
  }

  let path = String::from_utf8(output.stdout).ok()?;
  let canonical = fs::canonicalize(path.trim()).ok()?;
  let canonical = canonical.to_string_lossy().into_owned();
  if !canonical.starts_with("/nix/store/") {
    return None;
  }

  Path::new(&canonical)
    .parent()?
    .parent()
    .map(|dir| dir.to_string_lossy().into_owned())
}

/// The nix store directory of the `nix` binary, when it resolves into the
/// store.
pub fn nix_store_dir() -> Option<String> {
  store_bin_dir("nix")
}

/// Whether bubblewrap is installed and the test sandbox can be created.
pub fn bwrap_usable() -> bool {
  let version = process::Command::new("bwrap").arg("--version").output();
  let Ok(version) = version else {
    eprintln!("skipping bwrap test: bwrap not found on PATH");
    return false;
  };
  if !version.status.success() {
    eprintln!("skipping bwrap test: bwrap is not functional");
    return false;
  }

  match process::Command::new("bwrap")
    .args(TEST_BWRAP_ARGS)
    .args(["/bin/sh", "-c", "exit 0"])
    .output()
  {
    Ok(output) if output.status.success() => true,
    Ok(output) => {
      eprintln!(
        "skipping bwrap test: sandbox unusable: {}",
        String::from_utf8_lossy(&output.stderr).trim()
      );
      false
    }
    Err(error) => {
      eprintln!("skipping bwrap test: {error}");
      false
    }
  }
}

/// Escape a string for embedding in a nix double-quoted string.
fn nix_escape(value: &str) -> String {
  value
    .replace('\\', "\\\\")
    .replace('"', "\\\"")
    .replace('$', "\\$")
}

/// A self-contained flake exposing a package and a dev shell, both built as
/// raw derivations. The package is built with `/bin/sh`; the dev shell is
/// built with the store `bash` so that `nix print-dev-env` accepts it.
pub fn flake_nix(build_command: &str) -> String {
  let bash = store_bin_dir("bash").unwrap_or_default();
  let build = nix_escape(build_command);
  format!(
    r#"
{{
  outputs =
    {{ self }}:
    {{
      packages.x86_64-linux.default = derivation {{
        name = "mcp-nix-test";
        system = "x86_64-linux";
        builder = "/bin/sh";
        args = [ "-c" "{build}" ];
      }};
      packages.aarch64-linux.default = derivation {{
        name = "mcp-nix-test";
        system = "aarch64-linux";
        builder = "/bin/sh";
        args = [ "-c" "{build}" ];
      }};
      devShells.x86_64-linux.default = derivation {{
        name = "mcp-nix-develop-test";
        system = "x86_64-linux";
        outputs = [ "out" ];
        builder = "{bash}/bin/bash";
        args = [ "-c" "{build}" ];
        shellHook = "export MCP_NIX_SHELL_HOOK=ran";
        MCP_NIX_DEVELOP_TEST = "from-dev-shell";
      }};
      devShells.aarch64-linux.default = derivation {{
        name = "mcp-nix-develop-test";
        system = "aarch64-linux";
        outputs = [ "out" ];
        builder = "{bash}/bin/bash";
        args = [ "-c" "{build}" ];
        shellHook = "export MCP_NIX_SHELL_HOOK=ran";
        MCP_NIX_DEVELOP_TEST = "from-dev-shell";
      }};
    }};
}}
"#
  )
}

/// A flake whose default package is built with nixpkgs tools (`hello`) and
/// whose dev shell is a nixpkgs `mkShell` with `hello` available. The dev
/// shell sets the same environment markers as [`flake_nix`].
pub fn nixpkgs_flake() -> String {
  format!(
    r#"
{{
  inputs = {{
    nixpkgs.url = "github:nixos/nixpkgs/{NIXPKGS_REV}";
  }};
  outputs =
    {{ self, nixpkgs }}:
    {{
      packages.x86_64-linux.default = nixpkgs.legacyPackages.x86_64-linux.hello;
      packages.aarch64-linux.default = nixpkgs.legacyPackages.aarch64-linux.hello;
      devShells.x86_64-linux.default = nixpkgs.legacyPackages.x86_64-linux.mkShell {{
        packages = [ nixpkgs.legacyPackages.x86_64-linux.hello ];
        shellHook = "export MCP_NIX_SHELL_HOOK=ran";
        MCP_NIX_DEVELOP_TEST = "from-dev-shell";
      }};
      devShells.aarch64-linux.default = nixpkgs.legacyPackages.aarch64-linux.mkShell {{
        packages = [ nixpkgs.legacyPackages.aarch64-linux.hello ];
        shellHook = "export MCP_NIX_SHELL_HOOK=ran";
        MCP_NIX_DEVELOP_TEST = "from-dev-shell";
      }};
    }};
}}
"#
  )
}

/// Build the dev shell environment of the given flake ahead of time, so that
/// `nix print-dev-env` does not have to build it at test time.
///
/// The raw-derivation dev shell is built with a store `bash` whose dynamic
/// dependencies are not visible inside the default nix build sandbox; the
/// build sandbox paths are therefore augmented to expose the store `bash`.
/// Returns whether the environment could be built; tests skip when it cannot.
pub fn prime_dev_shell(flake_dir: &Path, sandbox_paths: Option<&str>) -> bool {
  let mut command = process::Command::new("nix");
  command.args(["print-dev-env", &flake_dir.display().to_string(), "--json"]);
  if let Some(paths) = sandbox_paths {
    command.args(["--option", "sandbox-paths", paths]);
  }
  command.stdout(process::Stdio::null());
  command.stderr(process::Stdio::null());

  match command.status() {
    Ok(status) if status.success() => true,
    Ok(_) => {
      eprintln!(
        "skipping flake test: could not build the dev shell environment"
      );
      false
    }
    Err(error) => {
      eprintln!("skipping flake test: failed to run nix: {error}");
      false
    }
  }
}

/// A primed flake exposing the raw-derivation package and dev shell, or `None`
/// when the dev shell environment could not be built.
fn raw_dev_shell_flake() -> Option<TempFlake> {
  let flake = TempFlake::new(&flake_nix("echo mcp-nix-test > \"$out\""));
  let bash = store_bin_dir("bash")?;
  let sandbox_paths = format!("bash={bash}/bin/bash");
  if prime_dev_shell(&flake.dir, Some(&sandbox_paths)) {
    Some(flake)
  } else {
    None
  }
}

/// A primed flake exposing a nixpkgs package and dev shell, or `None` when the
/// dev shell environment could not be built.
fn nixpkgs_dev_shell_flake() -> Option<TempFlake> {
  let flake = TempFlake::new(&nixpkgs_flake());
  if prime_dev_shell(&flake.dir, None) {
    Some(flake)
  } else {
    None
  }
}

/// A primed flake fixture exposing a package (`#default`) and a dev shell.
pub struct Fixture {
  /// The flake.
  pub flake: TempFlake,
  /// A program provided by the built package, when it has one.
  pub package_program: Option<&'static str>,
}

/// All flake fixtures the tools are tested against: a raw-derivation flake and
/// a flake built with nixpkgs tools. Flakes whose dev shell environment could
/// not be built are skipped.
pub fn fixtures() -> Vec<Fixture> {
  assert_nix_builds();
  let mut fixtures = Vec::new();
  if let Some(flake) = raw_dev_shell_flake() {
    fixtures.push(Fixture {
      flake,
      package_program: None,
    });
  }
  if let Some(flake) = nixpkgs_dev_shell_flake() {
    fixtures.push(Fixture {
      flake,
      package_program: Some("hello"),
    });
  }
  fixtures
}

/// Run the given test against every fixture in [`fixtures`].
pub fn for_each_flake(mut test: impl FnMut(&Fixture)) {
  for fixture in fixtures() {
    test(&fixture);
  }
}
