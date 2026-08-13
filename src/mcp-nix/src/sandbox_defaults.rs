//! Defaults for the sandbox and its base environment.
//!
//! The values in this file are referenced from the documentation (via mdBook
//! anchors) so that the docs stay in sync with the code.

/// The default bubblewrap arguments, used when `MCP_NIX_SANDBOX` is unset.
///
/// Network is restricted: the package is built with `nix build` outside the
/// sandbox, and the program run inside the sandbox is not expected to need the
/// network.
// ANCHOR: default-sandbox
pub const DEFAULT_SANDBOX_ARGS: &str = "\
--die-with-parent --unshare-all \
--ro-bind-try /nix/store /nix/store \
--ro-bind /usr /usr \
--ro-bind /bin /bin --ro-bind-try /sbin /sbin \
--ro-bind-try /lib /lib --ro-bind-try /lib64 /lib64 \
--tmpfs /tmp \
--proc /proc --dev /dev";
// ANCHOR_END: default-sandbox

/// The minimal environment set inside the sandbox for programs run from built
/// packages and dev shell commands.
///
/// The sandbox clears the environment with bubblewrap's `--clearenv`; only
/// these variables are set by default. Additional variables can be provided per
/// invocation.
// ANCHOR: base-env
pub const BASE_ENV: &[(&str, &str)] = &[
  ("HOME", "/tmp"),
  ("LANG", "C.UTF-8"),
  ("PATH", "/usr/bin:/bin"),
  ("TMPDIR", "/tmp"),
];
// ANCHOR_END: base-env
