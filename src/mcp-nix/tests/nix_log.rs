mod common;

use common::{ECHO_FLAKE, TempFlake, build_flake_with_log};
use mcp_nix::{LogOptions, fetch_log};

#[test]
fn fetch_log_returns_the_first_page_of_the_build_log() {
  let flake = TempFlake::new(ECHO_FLAKE);
  let Some(store_path) = build_flake_with_log(&flake) else {
    eprintln!("skipping: build log not available in this environment");
    return;
  };

  let log = fetch_log(
    &store_path,
    &LogOptions {
      offset: 0,
      limit: 10,
      from_end: false,
    },
  )
  .unwrap();

  assert!(
    log.contains("log-line-1"),
    "expected the first page to start at the beginning of the log, got:\n{log}"
  );
  assert!(
    log.contains("[nix_log] lines 0..9"),
    "expected the window footer, got:\n{log}"
  );
  assert!(
    log.contains("next offset: 10"),
    "expected the next offset in the footer, got:\n{log}"
  );
}

#[test]
fn fetch_log_returns_a_page_from_an_offset() {
  let flake = TempFlake::new(ECHO_FLAKE);
  let Some(store_path) = build_flake_with_log(&flake) else {
    eprintln!("skipping: build log not available in this environment");
    return;
  };

  let log = fetch_log(
    &store_path,
    &LogOptions {
      offset: 10,
      limit: 10,
      from_end: false,
    },
  )
  .unwrap();

  assert!(
    log.starts_with("log-line-11\n"),
    "expected the page to start at the requested line, got:\n{log}"
  );
  assert!(
    !log.starts_with("log-line-1\n"),
    "expected the page not to repeat earlier lines, got:\n{log}"
  );
}

#[test]
fn fetch_log_from_end_returns_the_end_of_the_log() {
  let flake = TempFlake::new(ECHO_FLAKE);
  let Some(store_path) = build_flake_with_log(&flake) else {
    eprintln!("skipping: build log not available in this environment");
    return;
  };

  let log = fetch_log(
    &store_path,
    &LogOptions {
      offset: 0,
      limit: 5,
      from_end: true,
    },
  )
  .unwrap();

  assert!(
    log.contains("log-line-25"),
    "expected the last page to end at the end of the log, got:\n{log}"
  );
  assert!(
    log.contains("next offset: 5"),
    "expected the next offset to keep walking backwards, got:\n{log}"
  );
}

#[test]
fn fetch_log_from_end_with_offset_walks_backwards() {
  let flake = TempFlake::new(ECHO_FLAKE);
  let Some(store_path) = build_flake_with_log(&flake) else {
    eprintln!("skipping: build log not available in this environment");
    return;
  };

  let log = fetch_log(
    &store_path,
    &LogOptions {
      offset: 5,
      limit: 5,
      from_end: true,
    },
  )
  .unwrap();

  assert!(
    log.starts_with("log-line-16\n"),
    "expected the page before the end, got:\n{log}"
  );
  assert!(
    log.contains("[nix_log] lines 15..19 of 25"),
    "expected the window footer, got:\n{log}"
  );
  assert!(
    log.contains("next offset: 10"),
    "expected the next offset in the footer, got:\n{log}"
  );
}

#[test]
fn fetch_log_reports_missing_build_logs() {
  let error = fetch_log(
    "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-does-not-exist",
    &LogOptions::default(),
  )
  .unwrap_err();

  assert!(
    error.to_string().contains("nix log failed"),
    "unexpected error: {error}"
  );
}
