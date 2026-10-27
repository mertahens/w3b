//! The command line interface, driven as a subprocess.
mod common;

use common::{Response, TestServer};
use std::process::Command;

fn w3b() -> Command {
    Command::new(env!("CARGO_BIN_EXE_w3b"))
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(args: &[&str]) -> Out {
    let out = w3b().args(args).output().expect("run w3b");
    Out {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

#[test]
fn version_and_help() {
    let out = run(&["--version"]);
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("w3b"), "{}", out.stdout);
    let out = run(&["--help"]);
    assert_eq!(out.code, 0);
    for flag in ["--dump", "--config", "--init-config", "--width", "--print-config"] {
        assert!(out.stdout.contains(flag), "{flag} missing from help:\n{}", out.stdout);
    }
}

#[test]
fn dump_prints_the_page_and_exits_zero() {
    let server = TestServer::start(vec![(
        "/page",
        Response::html(200, "<title>Doc</title><h1>Heading</h1><p>Body text.</p>"),
    )]);
    let out = run(&["--dump", &server.url("/page")]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.contains("Heading"), "{:?}", out.stdout);
    assert!(out.stdout.contains("Body text."), "{:?}", out.stdout);
    assert!(out.stderr.contains("200 OK"), "{}", out.stderr);
    assert!(out.stderr.contains("Doc"), "{}", out.stderr);
}

#[test]
fn dump_honours_the_width() {
    let server =
        TestServer::start(vec![("/wide", Response::html(200, &format!("<p>{}</p>", "word ".repeat(60))))]);
    let narrow = run(&["--dump", "--width", "30", &server.url("/wide")]);
    let wide = run(&["--dump", "--width", "100", &server.url("/wide")]);
    let longest = |s: &str| s.lines().map(str::chars).map(|c| c.count()).max().unwrap_or(0);
    assert!(longest(&narrow.stdout) <= 30, "narrow output: {:?}", narrow.stdout.lines().next());
    assert!(longest(&wide.stdout) > 30, "wide output should use the extra room");
}

#[test]
fn http_errors_change_the_exit_code() {
    let server = TestServer::start(vec![
        ("/gone", Response::html(404, "<h1>nope</h1>")),
        ("/boom", Response::html(500, "<h1>oops</h1>")),
    ]);
    let out = run(&["--dump", &server.url("/gone")]);
    assert_eq!(out.code, 1, "4xx exits 1");
    assert!(out.stderr.contains("404 NOT FOUND"), "{}", out.stderr);
    assert!(out.stdout.contains("nope"), "the body is still printed");

    let out = run(&["--dump", &server.url("/boom")]);
    assert_eq!(out.code, 1, "5xx exits 1");
    assert!(out.stderr.contains("500 INTERNAL SERVER ERROR"), "{}", out.stderr);
}

#[test]
fn unreachable_server_exits_two() {
    // Nothing listens on this port.
    let out = run(&["--dump", "http://127.0.0.1:1/"]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("ERROR"), "{}", out.stderr);
}

#[test]
fn without_a_terminal_it_says_so_and_points_at_dump() {
    // Test harnesses have pipes, not ttys.
    let dead = "http://127.0.0.1:1/";
    let out = run(&[dead]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("not a terminal"), "{}", out.stderr);
    assert!(out.stderr.contains("--dump"), "the hint must mention --dump: {}", out.stderr);
    assert!(!out.stderr.contains("panicked"), "{}", out.stderr);
}

#[test]
fn print_config_emits_the_shipped_defaults() {
    let out = run(&["--print-config"]);
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("[style.status]"), "{}", out.stdout);
    assert!(out.stdout.contains("home_url"), "{}", out.stdout);
    assert_eq!(out.stdout, w3b::config::DEFAULT_CONFIG_TOML);
}

#[test]
fn init_config_writes_a_file_that_loads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("w3b.toml");
    let out = run(&["--init-config", "--config-out", path.to_str().unwrap()]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(path.exists(), "config file written");

    // The written file must load without complaint, and the colors must be ours.
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("bg = \"green\""), "{text}");
    let out = run(&["-c", path.to_str().unwrap(), "--print-config"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
}

#[test]
fn a_broken_config_is_reported_not_panicked() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("bad.toml");
    std::fs::write(&path, "[style.link]\nfg = \"notacolor\"\n").unwrap();
    let out = run(&["-c", path.to_str().unwrap(), "--dump", "http://127.0.0.1:1/"]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("notacolor"), "{}", out.stderr);
    assert!(!out.stderr.contains("panicked"), "{}", out.stderr);
}

#[test]
fn an_unknown_config_key_is_reported_with_its_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("bad.toml");
    std::fs::write(&path, "[behavior]\ntimout_secs = 3\n").unwrap();
    let out = run(&["-c", path.to_str().unwrap(), "--dump", "http://127.0.0.1:1/"]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("timout_secs"), "{}", out.stderr);
}

#[test]
fn the_home_url_from_the_config_is_used_when_no_target_is_given() {
    let server =
        TestServer::start(vec![("/home", Response::html(200, "<title>Home</title><p>default page</p>"))]);
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    std::fs::write(&path, format!("[behavior]\nhome_url = \"{}\"\n", server.url("/home"))).unwrap();
    let out = run(&["-c", path.to_str().unwrap(), "--dump"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.contains("default page"), "{:?}", out.stdout);
    assert_eq!(server.request_paths(), vec!["/home".to_string()]);
}

#[test]
fn a_plain_text_url_is_dumped_as_text() {
    let server = TestServer::start(vec![("/t", Response::bytes(200, "text/plain", b"one\n  two\n"))]);
    let out = run(&["--dump", &server.url("/t")]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.contains("  two"), "{:?}", out.stdout);
}
