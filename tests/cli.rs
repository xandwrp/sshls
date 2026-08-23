use std::path::Path;
use std::process::{Command, Output};

fn sshls(directory: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sshls"))
        .env("SSHLS_DIR", directory)
        .env("SSHLS_USER_CONFIG", directory.join("config"))
        .args(arguments)
        .output()
        .unwrap()
}

fn success(directory: &Path, arguments: &[&str]) -> String {
    let output = sshls(directory, arguments);
    assert!(
        output.status.success(),
        "sshls {arguments:?} failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn complete_inventory_lifecycle() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path();

    assert_eq!(success(directory, &[]), "No destinations.\n");
    success(
        directory,
        &[
            "add",
            "prod-web",
            "2001:db8::10",
            "--user",
            "deploy",
            "--port",
            "2222",
            "--tag",
            "prod",
            "--tag",
            "web",
        ],
    );

    let listing = success(directory, &[]);
    assert!(listing.contains("deploy@[2001:db8::10]:2222"));
    assert!(listing.contains("prod,web"));

    let shown = success(directory, &["show", "PROD-WEB"]);
    assert!(shown.contains("HostName:  2001:db8::10"));
    assert!(shown.contains("User:      deploy"));
    assert!(success(directory, &["find", "WEB"]).contains("prod-web"));

    success(directory, &["edit", "prod-web", "--clear-port"]);
    success(directory, &["tag", "add", "prod-web", "critical"]);
    success(directory, &["tag", "rm", "prod-web", "web"]);
    success(directory, &["clone", "prod-web", "stage-web"]);
    success(directory, &["rename", "stage-web", "staging-web"]);

    let inventory = std::fs::read_to_string(directory.join("sshls.toml")).unwrap();
    assert!(inventory.contains("name = \"prod-web\""));
    assert!(inventory.contains("name = \"staging-web\""));
    let parsed: toml::Value = toml::from_str(&inventory).unwrap();
    assert_eq!(
        parsed["hosts"][0]["tags"],
        toml::Value::Array(vec!["critical".into(), "prod".into()])
    );

    let generated = std::fs::read_to_string(directory.join("sshls.conf")).unwrap();
    assert_eq!(generated.matches("Host ").count(), 2);
    assert!(!generated.contains("Port "));
    assert!(!generated.contains("critical"));

    success(directory, &["rm", "staging-web"]);
    assert!(!success(directory, &[]).contains("staging-web"));
}

#[test]
fn failed_mutation_leaves_files_unchanged() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path();
    success(directory, &["add", "prod", "example.com"]);
    let before_inventory = std::fs::read(directory.join("sshls.toml")).unwrap();
    let before_config = std::fs::read(directory.join("sshls.conf")).unwrap();

    let output = sshls(directory, &["edit", "prod", "--host", "bad host"]);
    assert!(!output.status.success());
    assert_eq!(
        std::fs::read(directory.join("sshls.toml")).unwrap(),
        before_inventory
    );
    assert_eq!(
        std::fs::read(directory.join("sshls.conf")).unwrap(),
        before_config
    );
}

#[test]
fn config_sync_is_repeatable() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path();
    success(directory, &["config", "sync"]);
    let first = std::fs::read(directory.join("sshls.conf")).unwrap();
    success(directory, &["config", "sync"]);
    let second = std::fs::read(directory.join("sshls.conf")).unwrap();
    assert_eq!(first, second);
}

#[test]
#[cfg(unix)]
fn config_check_verifies_real_openssh_resolution() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path();
    success(directory, &["add", "durable-test", "example.invalid"]);
    let sync_output = success(directory, &["config", "sync"]);
    let directive = sync_output.lines().last().unwrap();
    assert!(directive.starts_with("Include "));
    std::fs::write(directory.join("config"), format!("{directive}\n")).unwrap();

    let check = success(directory, &["config", "check"]);
    assert!(check.contains("generated config is current"));
    assert!(check.contains("SSH resolves durable-test as managed"));
}
