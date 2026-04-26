//! Validate every embedded creature against `schema/app.schema.json`.
//! Mirrors grimoire's tests/schema.rs.

use jsonschema::Validator;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn validator() -> Validator {
    let path = project_root().join("schema").join("app.schema.json");
    let raw = fs::read_to_string(&path).expect("read schema");
    let schema: Value = serde_json::from_str(&raw).expect("parse schema");
    jsonschema::validator_for(&schema).expect("compile schema")
}

fn yaml_to_json(yaml: &str) -> Value {
    serde_yml::from_str(yaml).unwrap_or_else(|e| panic!("yaml parse: {e}"))
}

fn errors(v: &Validator, value: &Value) -> Vec<String> {
    v.iter_errors(value).map(|e| e.to_string()).collect()
}

#[test]
fn every_embedded_app_validates() {
    let v = validator();
    let dir = project_root().join("apps");
    let mut failures: Vec<String> = Vec::new();
    let mut count = 0;
    for entry in fs::read_dir(&dir).expect("read apps dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        count += 1;
        let yaml = fs::read_to_string(&path).expect("read yaml");
        let value = yaml_to_json(&yaml);
        let errs = errors(&v, &value);
        if !errs.is_empty() {
            failures.push(format!(
                "{}: {} schema violation(s)\n    - {}",
                path.file_name().unwrap().to_string_lossy(),
                errs.len(),
                errs.join("\n    - "),
            ));
        }
    }
    assert!(count > 0, "no app entries found");
    assert!(
        failures.is_empty(),
        "{} of {count} app entries failed schema validation:\n{}",
        failures.len(),
        failures.join("\n"),
    );
}

#[test]
fn rejects_missing_name() {
    let v = validator();
    let yaml = r#"
locations:
  native:
    config: ~/.config/x
"#;
    assert!(!errors(&v, &yaml_to_json(yaml)).is_empty());
}

#[test]
fn rejects_bad_name_pattern() {
    let v = validator();
    let yaml = r#"
name: BadName
locations:
  native:
    config: ~/.config/x
"#;
    assert!(!errors(&v, &yaml_to_json(yaml)).is_empty());
}

#[test]
fn rejects_unknown_top_level_field() {
    let v = validator();
    let yaml = r#"
name: x
mystery: 42
locations:
  native:
    config: ~/.config/x
"#;
    let es = errors(&v, &yaml_to_json(yaml));
    assert!(es.iter().any(|e| e.contains("mystery")), "{es:?}");
}

#[test]
fn rejects_unknown_flavor() {
    let v = validator();
    let yaml = r#"
name: x
locations:
  rubber-duck:
    config: ~/.config/x
"#;
    assert!(!errors(&v, &yaml_to_json(yaml)).is_empty());
}

#[test]
fn rejects_flatpak_location_without_id() {
    let v = validator();
    let yaml = r#"
name: x
locations:
  flatpak:
    config: ~/.var/app/x/config
"#;
    let es = errors(&v, &yaml_to_json(yaml));
    assert!(es.iter().any(|e| e.contains("flatpak_id")), "{es:?}");
}

#[test]
fn rejects_location_with_no_paths() {
    let v = validator();
    let yaml = r#"
name: x
locations:
  native: {}
"#;
    assert!(!errors(&v, &yaml_to_json(yaml)).is_empty());
}
