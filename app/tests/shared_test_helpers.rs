//! Source checks on `app/tests/*.rs`: the GPU lock, the iced settings and the PNG writer live once, in
//! `tests/common/mod.rs`, and every test that creates a `Simulator` holds `common::gpu()`.

use std::path::PathBuf;

/// `(file name, source)` of every test file in `app/tests/` except this one.
fn test_files() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut files: Vec<(String, String)> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs") && !p.ends_with("shared_test_helpers.rs"))
        .map(|p| {
            (p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read_to_string(&p).unwrap())
        })
        .collect();
    files.sort();
    assert!(files.len() > 20, "found the test files in {}", dir.display());
    files
}

/// Code without `//` comments, so a comment that names a pattern does not count as using it.
fn code(source: &str) -> String {
    source.lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n")
}

#[test]
fn every_simulator_is_created_under_the_shared_gpu_lock() {
    let mut missing = Vec::new();
    for (name, source) in test_files() {
        let code = code(&source);
        for (at, _) in code.match_indices("Simulator::") {
            let rest = &code[at + "Simulator::".len()..];
            if !["new(", "with_size(", "with_settings("].iter().any(|c| rest.starts_with(c)) {
                continue;
            }
            // The guard must be taken earlier in the same function.
            let start = code[..at].rfind("fn ").unwrap_or(0);
            if !code[start..at].contains("common::gpu()") {
                let line = code[..at].matches('\n').count() + 1;
                missing.push(format!("{name}:{line}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "these Simulators are created without `let _gpu = common::gpu();` earlier in the same function \
         (see tests/common/mod.rs): {missing:?}"
    );
}

#[test]
fn test_files_do_not_copy_the_shared_helpers() {
    let copies = [
        ("static GPU", "a GPU lock: use common::gpu()"),
        ("fn gpu(", "a GPU lock helper: use common::gpu()"),
        ("FONTS.iter()", "iced settings: use common::iced_settings()"),
        ("matches_image(", "a PNG writer: use common::save_png()"),
    ];
    let mut found = Vec::new();
    for (name, source) in test_files() {
        let code = code(&source);
        for (pattern, what) in copies {
            if let Some(at) = code.find(pattern) {
                found.push(format!("{name}:{} defines {what}", code[..at].matches('\n').count() + 1));
            }
        }
    }
    assert!(found.is_empty(), "test files copy helpers of tests/common/mod.rs: {found:#?}");
}
