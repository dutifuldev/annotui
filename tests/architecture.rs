use std::{fs, path::Path};

#[test]
fn domain_and_source_are_terminal_and_io_independent() {
    assert_forbidden(
        &["src/domain.rs", "src/source.rs"],
        &[
            "crate::app",
            "crate::render",
            "crate::runner",
            "crate::terminal",
            "crossterm",
            "ratatui",
            "std::fs",
            "std::process",
        ],
    );
}

#[test]
fn output_does_not_depend_on_terminal_layers() {
    assert_forbidden(
        &["src/output.rs"],
        &[
            "crate::app",
            "crate::render",
            "crate::terminal",
            "crossterm",
            "ratatui",
        ],
    );
}

#[test]
fn input_hit_testing_stays_adapter_free() {
    assert_forbidden(
        &["src/input.rs"],
        &["crate::runner", "crate::terminal", "crossterm", "std::fs"],
    );
}

fn assert_forbidden(files: &[&str], forbidden: &[&str]) {
    let source = files
        .iter()
        .map(|file| {
            assert!(
                Path::new(file).exists(),
                "missing architecture input {file}"
            );
            fs::read_to_string(file).expect("read architecture input")
        })
        .collect::<Vec<_>>()
        .join("\n");
    for needle in forbidden {
        assert!(
            !source.contains(needle),
            "architecture contains forbidden dependency `{needle}`"
        );
    }
}
