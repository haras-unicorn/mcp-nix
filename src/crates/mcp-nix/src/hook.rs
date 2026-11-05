//! The decision of the nix `pre-build-hook` that `mcp-nix-hook` runs.
//!
//! The builder of a fixed-output derivation has network access, and nix
//! compares the hash only after the builder ran. So a caller that writes a
//! flake with such a derivation could run a script with network access before
//! anyone vetted it, and a right hash does not help. Nix runs the hook before
//! each build it does itself, with the derivation file as the argument, and
//! does not build when the hook fails. Outputs that are in the store or come
//! from a binary cache are not built, so the hook never sees them.
//!
//! The hook is told about a derivation by the output of `nix derivation show`.
//! [`verdict`] reads that output. It only allows a derivation it understood
//! and found harmless, so anything unexpected is refused.
//!
//! The other end of the hook is the server, which makes its `nix` commands use
//! the hook: [`PreBuildHook`].

use std::path::Path;

use serde_json::{Map, Value};

use crate::nix::NixError;

/// Added to an error of nix that names the hook, because the reason the hook
/// gave goes to the log of the nix daemon and not to nix.
const REFUSAL_NOTICE: &str = "The pre-build hook refused to build a \
   derivation that is fixed-output, impure or leaves the sandbox. Ask the \
   human to build it first. The reason is in the log of the nix daemon.";

/// The attributes of a derivation that ask nix for more than a plain build.
/// A derivation that sets one to anything but an empty value is refused. Nix
/// honors `__noChroot` only when the administrator set `sandbox = relaxed`,
/// which is not a choice made for the caller of this server.
const PRIVILEGED_ATTRIBUTES: &[&str] = &["__noChroot"];

/// The system features a derivation may not require. With `recursive-nix` the
/// builder can start builds of its own from inside the build, which the hook
/// has not been shown to cover.
const PRIVILEGED_FEATURES: &[&str] = &["recursive-nix"];

/// The attribute that lists the system features a derivation requires.
const FEATURES_ATTRIBUTE: &str = "requiredSystemFeatures";

/// The environment variable that holds the attributes of a derivation with
/// structured attributes as a JSON document, in the older output of
/// `nix derivation show`.
const STRUCTURED_ENV: &str = "__json";

/// Why a derivation is not allowed to be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
  /// The output of `nix derivation show` was not what the hook knows how to
  /// read.
  #[error("the output of nix derivation show was not understood")]
  NotUnderstood,
  /// An output of the derivation has a fixed hash, so the builder has network
  /// access.
  #[error("it is a fixed-output derivation")]
  FixedOutput,
  /// The derivation is impure, so the builder has network access.
  #[error("it is an impure derivation")]
  Impure,
  /// The derivation asks for something that [`PRIVILEGED_ATTRIBUTES`] lists.
  #[error("it sets {0}")]
  Privileged(&'static str),
  /// The derivation requires a system feature that [`PRIVILEGED_FEATURES`]
  /// lists.
  #[error("it requires the system feature {0}")]
  PrivilegedFeature(&'static str),
}

/// Decide from the output of `nix derivation show` whether the derivations in
/// it may be built.
///
/// Both the output with a `derivations` key and the older one, which maps
/// derivation files straight to derivations, are read.
pub fn verdict(derivation_show: &[u8]) -> Result<(), Refusal> {
  let document: Value = serde_json::from_slice(derivation_show)
    .map_err(|_| Refusal::NotUnderstood)?;
  let derivations = document
    .get("derivations")
    .unwrap_or(&document)
    .as_object()
    .filter(|derivations| !derivations.is_empty())
    .ok_or(Refusal::NotUnderstood)?;

  derivations.values().try_for_each(derivation_verdict)
}

fn derivation_verdict(derivation: &Value) -> Result<(), Refusal> {
  let outputs = derivation
    .get("outputs")
    .and_then(Value::as_object)
    .filter(|outputs| !outputs.is_empty())
    .ok_or(Refusal::NotUnderstood)?;

  for output in outputs.values() {
    let output = output.as_object().ok_or(Refusal::NotUnderstood)?;
    if output.contains_key("hash") {
      return Err(Refusal::FixedOutput);
    }
    if output.get("impure").and_then(Value::as_bool) == Some(true) {
      return Err(Refusal::Impure);
    }
  }

  let env = derivation
    .get("env")
    .and_then(Value::as_object)
    .ok_or(Refusal::NotUnderstood)?;
  let structured = structured_attributes(derivation, env)?;
  let sources = || std::iter::once(env).chain(structured.as_ref());

  for attribute in PRIVILEGED_ATTRIBUTES {
    if sources().any(|source| source.get(*attribute).is_some_and(is_set)) {
      return Err(Refusal::Privileged(attribute));
    }
  }
  for feature in PRIVILEGED_FEATURES {
    let requires = |source: &Map<String, Value>| {
      source
        .get(FEATURES_ATTRIBUTE)
        .is_some_and(|features| lists(features, feature))
    };
    if sources().any(requires) {
      return Err(Refusal::PrivilegedFeature(feature));
    }
  }

  Ok(())
}

/// The attributes of a derivation with structured attributes. Nix keeps them
/// out of the environment, in `structuredAttrs` or, in the older output, as a
/// JSON document in `__json`. A derivation without them has none.
fn structured_attributes(
  derivation: &Value,
  env: &Map<String, Value>,
) -> Result<Option<Map<String, Value>>, Refusal> {
  if let Some(structured) = derivation.get("structuredAttrs") {
    return structured
      .as_object()
      .cloned()
      .map(Some)
      .ok_or(Refusal::NotUnderstood);
  }

  let Some(json) = env.get(STRUCTURED_ENV) else {
    return Ok(None);
  };
  json
    .as_str()
    .and_then(|json| serde_json::from_str::<Value>(json).ok())
    .and_then(|document| document.as_object().cloned())
    .map(Some)
    .ok_or(Refusal::NotUnderstood)
}

/// Whether an attribute asks for something: an environment variable that is
/// not empty, or a structured attribute that is true or not empty.
fn is_set(value: &Value) -> bool {
  match value {
    Value::Null => false,
    Value::Bool(set) => *set,
    Value::String(text) => !text.is_empty(),
    _ => true,
  }
}

/// Whether the value of `requiredSystemFeatures` lists the feature: a string
/// of words in the environment, or a list in the structured attributes.
fn lists(features: &Value, feature: &str) -> bool {
  match features {
    Value::String(words) => {
      words.split_whitespace().any(|word| word == feature)
    }
    Value::Array(items) => {
      items.iter().any(|item| item.as_str() == Some(feature))
    }
    _ => false,
  }
}

/// The program that nix runs before each build it does itself, and skips the
/// build when the program fails. Nix applies it to every build, including the
/// ones evaluation needs, so every command is covered and nothing has to be
/// read from what nix prints.
///
/// Three things have to hold for the hook to be the last word:
///
/// - the build happens on this machine, because a remote builder never runs
///   the hook, so [`PreBuildHook::nix_args`] turns remote builders off;
/// - nix actually uses the setting. A daemon only takes it from a trusted user
///   and ignores it, with a warning, from anyone else, so
///   [`PreBuildHook::verify`] refuses to go on for an untrusted user;
/// - the hook can be run by the daemon, so it has to be an absolute path.
///
/// The hook only sees builds. What nix fetches while it evaluates is done by
/// the `nix` command itself, which is why that command runs in a sandbox, see
/// [`crate::sandbox`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreBuildHook {
  path: String,
}

impl PreBuildHook {
  /// The hook at the given path, which `mcp-nix-hook` is.
  pub(crate) fn new(path: impl Into<String>) -> Self {
    Self { path: path.into() }
  }

  /// The arguments that make `nix` use the hook and no remote builders.
  pub(crate) fn nix_args(&self) -> Vec<String> {
    [
      "--option",
      "pre-build-hook",
      self.path.as_str(),
      "--builders",
      "",
    ]
    .map(str::to_string)
    .to_vec()
  }

  /// Check that the hook is an executable file at an absolute path.
  pub(crate) fn verify_program(&self) -> Result<(), NixError> {
    if is_executable_file(Path::new(&self.path)) {
      return Ok(());
    }
    Err(NixError::Guard {
      detail: format!(
        "the pre-build hook {} is not an executable file at an absolute path",
        self.path
      ),
    })
  }

  /// Check that the output of `nix store info --json`, run the way the
  /// commands of the server are, says the user is trusted by the daemon,
  /// which otherwise ignores the hook.
  pub(crate) fn verify_trusted(
    &self,
    store_info: &[u8],
  ) -> Result<(), NixError> {
    match trusted(store_info) {
      Some(true) => Ok(()),
      _ => Err(NixError::Guard {
        detail: "nix does not say that this user is trusted by the nix \
                 daemon, which then ignores the pre-build hook. Add the user \
                 to trusted-users or allow fixed-output derivations"
          .to_string(),
      }),
    }
  }

  /// The error output of a failed `nix` command, with the reason for a
  /// refusal added when nix names the hook.
  pub(crate) fn explain(&self, stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr).into_owned();
    if stderr.contains(self.path.as_str()) {
      format!("{}\n{REFUSAL_NOTICE}", stderr.trim_end())
    } else {
      stderr
    }
  }
}

/// Whether the output of `nix store info --json` says the user is trusted,
/// which older versions of nix write as the number 1 or 0.
fn trusted(store_info: &[u8]) -> Option<bool> {
  match serde_json::from_slice::<Value>(store_info)
    .ok()?
    .get("trusted")?
  {
    Value::Bool(trusted) => Some(*trusted),
    Value::Number(trusted) => Some(trusted.as_i64()? == 1),
    _ => None,
  }
}

fn is_executable_file(path: &Path) -> bool {
  use std::os::unix::fs::PermissionsExt;
  path.is_absolute()
    && path.metadata().is_ok_and(|metadata| {
      metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    })
}

#[cfg(test)]
mod tests {
  use super::*;

  fn shown(output: &str, env: &str) -> Vec<u8> {
    format!(
      r#"{{"derivations":{{"abc-t.drv":{{"outputs":{{"out":{output}}},"env":{env}}}}},"version":4}}"#
    )
    .into_bytes()
  }

  #[test]
  fn a_plain_derivation_is_allowed() {
    let document = shown(r#"{"path":"abc-t"}"#, r#"{"name":"t"}"#);

    assert_eq!(verdict(&document), Ok(()));
  }

  #[test]
  fn a_content_addressed_derivation_without_a_hash_is_allowed() {
    let document = shown(r#"{"hashAlgo":"sha256","method":"nar"}"#, "{}");

    assert_eq!(verdict(&document), Ok(()));
  }

  #[test]
  fn a_fixed_output_derivation_is_refused() {
    let document = shown(
      r#"{"hash":"abc","hashAlgo":"sha256","method":"flat","path":"abc-t"}"#,
      "{}",
    );

    assert_eq!(verdict(&document), Err(Refusal::FixedOutput));
  }

  #[test]
  fn an_impure_derivation_is_refused() {
    let document = shown(r#"{"hashAlgo":"sha256","impure":true}"#, "{}");

    assert_eq!(verdict(&document), Err(Refusal::Impure));
  }

  #[test]
  fn leaving_the_sandbox_is_refused() {
    let document = shown(r#"{"path":"abc-t"}"#, r#"{"__noChroot":"1"}"#);

    assert_eq!(verdict(&document), Err(Refusal::Privileged("__noChroot")));
  }

  #[test]
  fn staying_in_the_sandbox_explicitly_is_allowed() {
    let document = shown(r#"{"path":"abc-t"}"#, r#"{"__noChroot":""}"#);

    assert_eq!(verdict(&document), Ok(()));
  }

  #[test]
  fn leaving_the_sandbox_with_structured_attributes_is_refused() {
    let document = br#"{"derivations":{"a-t.drv":{
      "outputs":{"out":{"path":"a-t"}},
      "env":{},
      "structuredAttrs":{"__noChroot":true}
    }}}"#;

    assert_eq!(verdict(document), Err(Refusal::Privileged("__noChroot")));
  }

  #[test]
  fn staying_in_the_sandbox_with_structured_attributes_is_allowed() {
    let document = br#"{"derivations":{"a-t.drv":{
      "outputs":{"out":{"path":"a-t"}},
      "env":{},
      "structuredAttrs":{"__noChroot":false,"name":"t"}
    }}}"#;

    assert_eq!(verdict(document), Ok(()));
  }

  #[test]
  fn leaving_the_sandbox_in_the_older_structured_attributes_is_refused() {
    let document = br#"{"/nix/store/a-t.drv":{
      "outputs":{"out":{"path":"a"}},
      "env":{"__json":"{\"__noChroot\":true}"}
    }}"#;

    assert_eq!(verdict(document), Err(Refusal::Privileged("__noChroot")));
  }

  #[test]
  fn structured_attributes_that_are_not_understood_are_refused() {
    let documents: &[&[u8]] = &[
      br#"{"derivations":{"a.drv":{"outputs":{"out":{}},"env":{},"structuredAttrs":"x"}}}"#,
      br#"{"derivations":{"a.drv":{"outputs":{"out":{}},"env":{"__json":"nope"}}}}"#,
    ];

    for document in documents {
      assert_eq!(verdict(document), Err(Refusal::NotUnderstood));
    }
  }

  #[test]
  fn requiring_recursive_nix_is_refused() {
    let in_env = shown(
      r#"{"path":"abc-t"}"#,
      r#"{"requiredSystemFeatures":"kvm recursive-nix"}"#,
    );
    let structured = br#"{"derivations":{"a-t.drv":{
      "outputs":{"out":{"path":"a-t"}},
      "env":{},
      "structuredAttrs":{"requiredSystemFeatures":["recursive-nix"]}
    }}}"#;

    for document in [in_env.as_slice(), structured.as_slice()] {
      assert_eq!(
        verdict(document),
        Err(Refusal::PrivilegedFeature("recursive-nix"))
      );
    }
  }

  #[test]
  fn requiring_other_system_features_is_allowed() {
    let document = shown(
      r#"{"path":"abc-t"}"#,
      r#"{"requiredSystemFeatures":"kvm big-parallel"}"#,
    );

    assert_eq!(verdict(&document), Ok(()));
  }

  #[test]
  fn one_refused_derivation_refuses_all() {
    let document = br#"{"derivations":{
      "a-t.drv":{"outputs":{"out":{"path":"a-t"}},"env":{}},
      "b-t.drv":{"outputs":{"out":{"hash":"x"}},"env":{}}
    }}"#;

    assert_eq!(verdict(document), Err(Refusal::FixedOutput));
  }

  #[test]
  fn the_older_output_without_a_derivations_key_is_read() {
    let allowed =
      br#"{"/nix/store/a-t.drv":{"outputs":{"out":{"path":"a"}},"env":{}}}"#;
    let refused =
      br#"{"/nix/store/a-t.drv":{"outputs":{"out":{"hash":"x"}},"env":{}}}"#;

    assert_eq!(verdict(allowed), Ok(()));
    assert_eq!(verdict(refused), Err(Refusal::FixedOutput));
  }

  #[test]
  fn anything_not_understood_is_refused() {
    let documents: &[&[u8]] = &[
      b"",
      b"not json",
      b"[]",
      b"{}",
      br#"{"derivations":{}}"#,
      br#"{"derivations":{"a.drv":{}}}"#,
      br#"{"derivations":{"a.drv":{"outputs":{},"env":{}}}}"#,
      br#"{"derivations":{"a.drv":{"outputs":{"out":"x"},"env":{}}}}"#,
      br#"{"derivations":{"a.drv":{"outputs":{"out":{}}}}}"#,
    ];

    for document in documents {
      assert_eq!(
        verdict(document),
        Err(Refusal::NotUnderstood),
        "allowed: {}",
        String::from_utf8_lossy(document)
      );
    }
  }

  #[test]
  fn nix_is_given_the_hook_and_no_remote_builders() {
    assert_eq!(
      PreBuildHook::new("/bin/hook").nix_args(),
      ["--option", "pre-build-hook", "/bin/hook", "--builders", ""]
    );
  }

  #[test]
  fn a_refusal_is_explained_when_nix_names_the_hook() {
    let hook = PreBuildHook::new("/bin/hook");

    let explained = hook.explain(b"error: program '/bin/hook' failed\n");

    assert!(explained.starts_with("error: program '/bin/hook' failed\n"));
    assert!(explained.ends_with(REFUSAL_NOTICE));
  }

  #[test]
  fn other_errors_are_left_alone() {
    let hook = PreBuildHook::new("/bin/hook");

    assert_eq!(hook.explain(b"error: oops"), "error: oops");
  }

  #[test]
  fn trust_is_read_from_the_store_info() {
    assert_eq!(trusted(br#"{"trusted":true,"url":"daemon"}"#), Some(true));
    assert_eq!(trusted(br#"{"trusted":false}"#), Some(false));
    assert_eq!(trusted(br#"{"trusted":1}"#), Some(true));
    assert_eq!(trusted(br#"{"trusted":0}"#), Some(false));
    assert_eq!(trusted(br#"{"trusted":"yes"}"#), None);
    assert_eq!(trusted(br#"{"url":"daemon"}"#), None);
    assert_eq!(trusted(b"not json"), None);
  }

  #[test]
  fn a_relative_or_missing_hook_does_not_verify() {
    for path in ["mcp-nix-hook", "/mcp-nix/no/such/hook"] {
      let result = PreBuildHook::new(path).verify_program();

      assert!(matches!(result, Err(NixError::Guard { .. })), "{path}");
    }
  }
}
