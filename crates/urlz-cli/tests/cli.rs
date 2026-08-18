use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::*;

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("urlz_cli_{}_{}", std::process::id(), name))
}

#[test]
fn encode_then_decode_roundtrips() {
    let url = "https://example.com/some/long/path?query=1&x=2";
    let payload = Command::cargo_bin("urlz")
        .unwrap()
        .args(["encode", url])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let payload = String::from_utf8(payload).unwrap();
    let payload = payload.trim();
    assert!(
        payload
            .bytes()
            .all(|b| urlz::alphabet::BASE85_ALPHABET.contains(&b)),
        "stdout contains chars outside BASE85_ALPHABET: {payload:?}"
    );

    Command::cargo_bin("urlz")
        .unwrap()
        .args(["decode", payload])
        .assert()
        .success()
        .stdout(predicates::str::contains(url));
}

#[test]
fn encode_invalid_url_fails() {
    Command::cargo_bin("urlz")
        .unwrap()
        .args(["encode", "not a url"])
        .assert()
        .failure()
        .stderr(predicates::str::is_empty().not());
}

#[test]
fn cli_dict_build_roundtrip() {
    let dir = temp_path("dict_roundtrip");
    let _ = fs::remove_dir_all(&dir);

    Command::cargo_bin("urlz")
        .unwrap()
        .args(["dict", "build", "--out"])
        .arg(&dir)
        .assert()
        .success();

    let codebook = dir.join("codebook.bin");
    let bytes = fs::read(&codebook).unwrap();
    assert_eq!(bytes.len(), 256);
    let cb = urlz::huffman::deserialize_codebook(&bytes).unwrap();
    assert_eq!(urlz::huffman::serialize_codebook(&cb), bytes);
    let _ = fs::remove_dir_all(&dir);
}
