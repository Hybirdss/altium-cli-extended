#![cfg(unix)]
mod common;
use common::*;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;

#[test]
fn non_unicode_paths_and_arguments_return_envelopes() {
    let path = OsString::from_vec(b"missing-ace-\xff.PrjPcb".to_vec());
    let (exit, value) = run(&[OsString::from("inspect"), path]);
    assert_eq!(exit, 3, "{value}");
    assert_eq!(value["error"]["code"], "FILE_NOT_FOUND");
    assert!(value["error"]["hint"].is_string());
    let bad = OsString::from_vec(vec![0xff]);
    let (exit, value) = run(&[OsString::from("commands"), OsString::from("--where"), bad]);
    assert_eq!(exit, 2, "{value}");
    assert_eq!(value["error"]["code"], "USAGE");
}
