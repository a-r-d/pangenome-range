use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pangenome-range"))
}

#[test]
fn reports_the_cargo_package_version() {
    let output = cli().arg("--version").output().expect("run CLI");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 stdout"),
        format!("pangenome-range {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn encode_help_does_not_require_positional_arguments() {
    let output = cli().args(["encode", "--help"]).output().expect("run CLI");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert!(stdout.contains("pangenome-range encode <input.gbz> <output.pngr>"));
    assert!(output.stderr.is_empty());
}

#[test]
fn verify_help_documents_explicit_reference_haplotype() {
    let output = cli()
        .args(["verify", "archive.pngr", "--help"])
        .output()
        .expect("run CLI");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert!(stdout.contains("--reference-haplotype N"));
    assert!(output.stderr.is_empty());
}
