//! Defaults for the sandbox and its base environment.
//!
//! The values in this file are referenced from the documentation (via mdBook
//! anchors) so that the docs stay in sync with the code.

/// The default bubblewrap arguments, the default of the `bwrap_args` setting.
///
/// Network is restricted: the package is built with `nix build` outside the
/// sandbox, and the program run inside the sandbox is not expected to need the
/// network.
// ANCHOR: default-sandbox
pub const DEFAULT_SANDBOX_ARGS: &str = "\
--die-with-parent --new-session --unshare-all \
--ro-bind-try /nix/store /nix/store \
--ro-bind-try /usr /usr \
--ro-bind-try /bin /bin --ro-bind-try /sbin /sbin \
--ro-bind-try /lib /lib --ro-bind-try /lib64 /lib64 \
--tmpfs /tmp \
--proc /proc --dev /dev";
// ANCHOR_END: default-sandbox

/// The default bubblewrap arguments for the `nix` commands of the tools, the
/// default of the `nix_bwrap_args` setting.
///
/// The network is off, so evaluation can only use what is in the nix store,
/// and the daemon does the building and the downloading of cached outputs. Its
/// socket is the one way out of the sandbox. The working directory of the
/// command is added read-only by the server.
///
/// `NIX_REMOTE` is set because the sandbox makes `/nix/var/nix` for the socket,
/// and `nix` finds that directory writable, so by default it opens a store of
/// its own there, with nothing in it, and never talks to the daemon.
///
/// `nix log` does not ask the daemon for a build log, it reads the file from
/// `/nix/var/log/nix` itself, so that is bound read-only too, or `nix_log`
/// finds no log at all. The logs there are readable by every user anyway.
///
/// `TMPDIR` is set to the `/tmp` of the sandbox, because the one of the server
/// is not in the sandbox when it is not `/tmp`, and `nix` fails to make its
/// temporary files there.
///
/// On NixOS `/etc/nix/nix.conf` and `registry.json` are links into
/// `/etc/static/nix`, so that is bound too, or the settings and the registry
/// of the system would not be visible.
// ANCHOR: default-nix-sandbox
pub const DEFAULT_NIX_SANDBOX_ARGS: &str = "\
--die-with-parent --new-session --unshare-all \
--ro-bind-try /nix/store /nix/store \
--ro-bind-try /nix/var/nix/daemon-socket /nix/var/nix/daemon-socket \
--ro-bind-try /nix/var/log/nix /nix/var/log/nix \
--ro-bind-try /etc/nix /etc/nix \
--ro-bind-try /etc/static/nix /etc/static/nix \
--ro-bind-try /etc/passwd /etc/passwd --ro-bind-try /etc/group /etc/group \
--ro-bind-try /etc/nsswitch.conf /etc/nsswitch.conf \
--tmpfs /tmp --setenv HOME /tmp --setenv TMPDIR /tmp \
--setenv NIX_REMOTE daemon \
--proc /proc --dev /dev";
// ANCHOR_END: default-nix-sandbox

/// The bubblewrap arguments that give the sandbox of the programs an NVIDIA
/// GPU, which the server adds after `bwrap_args` when a GPU is present (see
/// [`NVIDIA_GPU_DEVICE`]) and the `add_cuda_bwrap_args` setting is on. The
/// `nix` sandbox does not get them, since nothing there uses a GPU. The server
/// also binds a node for each GPU it finds, `/dev/nvidia0` and so on, which
/// these arguments cannot list.
///
/// The device nodes are bound at their own paths, after the `--dev /dev` of the
/// defaults, which would hide them otherwise. `/run/opengl-driver` is where
/// NixOS links the userspace driver, and where programs from the nix store look
/// for `libcuda`. The binds are `-try` ones, so a node the driver has not made
/// yet is skipped and not an error.
// ANCHOR: default-cuda-sandbox
pub const DEFAULT_CUDA_SANDBOX_ARGS: &str = "\
--dev-bind-try /dev/nvidiactl /dev/nvidiactl \
--dev-bind-try /dev/nvidia-uvm /dev/nvidia-uvm \
--dev-bind-try /dev/nvidia-uvm-tools /dev/nvidia-uvm-tools \
--ro-bind-try /run/opengl-driver /run/opengl-driver";
// ANCHOR_END: default-cuda-sandbox

/// The device node that tells an NVIDIA GPU is present: the control node the
/// driver makes when it is loaded. It is a const because it is the name the
/// driver gives it, and [`DEFAULT_CUDA_SANDBOX_ARGS`] binds the same one.
pub const NVIDIA_GPU_DEVICE: &str = "/dev/nvidiactl";

/// The default command that runs bubblewrap, the default of the
/// `bwrap_command` setting. Only its first word is looked up on the `PATH`.
pub const DEFAULT_BWRAP_COMMAND: &str = "bwrap";

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
