//! Enforce the owner-approved exception for generated canonical ABI glue only.
use std::{fs, path::Path, process::Command};
use syn::{
    Attribute, ItemMod,
    visit::{self, Visit},
};
const OWNER: &str = "examples/custom-protocol/src/component.rs";
const GENERATED: &str = "examples/custom-protocol/src/component/bindings.rs";
const GLUE: &str = r#"wit_bindgen::generate!({path:"../../crates/protocol-wasm/wit",world:"identity-protocol-package"});use super::Package;export!(Package);"#;
fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}
struct Sensor<'a> {
    path: &'a str,
    allowed: bool,
    count: usize,
    violations: Vec<String>,
}
impl<'ast> Visit<'ast> for Sensor<'_> {
    fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
        if item.mac.path.is_ident("macro_rules")
            && item.mac.tokens.to_string().contains("unsafe_code")
        {
            self.violations.push(format!(
                "{}: macro definitions cannot introduce unsafe lint overrides",
                self.path
            ));
        }
        visit::visit_item_macro(self, item);
    }

    fn visit_item_mod(&mut self, item: &'ast ItemMod) {
        let previous = self.allowed;
        self.allowed = self.path == OWNER
            && item.ident == "bindings"
            && item.content.is_none()
            && matches!(item.vis, syn::Visibility::Inherited)
            && item.attrs.len() == 1;
        visit::visit_item_mod(self, item);
        self.allowed = previous;
    }
    fn visit_attribute(&mut self, attr: &'ast Attribute) {
        if let syn::Meta::List(meta) = &attr.meta
            && (attr.path().is_ident("allow")
                || attr.path().is_ident("expect")
                || attr.path().is_ident("cfg_attr"))
            && meta.tokens.to_string().contains("unsafe_code")
        {
            if self.allowed
                && attr.path().is_ident("allow")
                && compact(&meta.tokens.to_string())
                    == "unsafe_code,clippy::same_length_and_capacity"
            {
                self.count += 1;
            } else {
                self.violations.push(format!("{}: unsafe allowance outside approved generated ABI module; see docs/architecture/dependency-rules.md",self.path));
            }
        }
        visit::visit_attribute(self, attr);
    }
}
fn inspect(path: &str, source: &str) -> Result<usize, String> {
    let file = syn::parse_file(source)
        .map_err(|_| format!("{path}: cannot parse Rust for unsafe-boundary policy"))?;
    let mut sensor = Sensor {
        path,
        allowed: false,
        count: 0,
        violations: vec![],
    };
    sensor.visit_file(&file);
    if sensor.violations.is_empty() {
        Ok(sensor.count)
    } else {
        Err(sensor.violations.join("\n"))
    }
}
pub(super) fn check(root: &Path) -> Result<(), Vec<String>> {
    let output = Command::new("git")
        .current_dir(root)
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            "*.rs",
        ])
        .output()
        .map_err(|_| vec!["unsafe-boundary Git inventory unavailable".into()])?;
    if !output.status.success() {
        return Err(vec!["unsafe-boundary Git inventory failed".into()]);
    }
    let paths = String::from_utf8(output.stdout)
        .map_err(|_| vec!["unsafe-boundary inventory must be UTF-8".into()])?;
    let manifest = fs::read_to_string(root.join("Cargo.toml"))
        .ok()
        .and_then(|s| s.parse::<toml::Table>().ok());
    if manifest
        .as_ref()
        .and_then(|t| t.get("workspace"))
        .and_then(|t| t.get("lints"))
        .and_then(|t| t.get("rust"))
        .and_then(|t| t.get("unsafe_code"))
        .and_then(toml::Value::as_str)
        != Some("deny")
    {
        return Err(vec![
            "workspace unsafe_code must remain deny; generated ABI allowance is scoped".into(),
        ]);
    }
    let mut count = 0;
    let mut violations = vec![];
    for path in paths.split('\0').filter(|p| !p.is_empty()) {
        let result = fs::read_to_string(root.join(path))
            .map_err(|_| format!("{path}: unsafe-boundary source unavailable"))
            .and_then(|s| inspect(path, &s));
        match result {
            Ok(n) => count += n,
            Err(e) => violations.push(e),
        }
    }
    if count != 1 {
        violations.push("expected exactly one reviewed generated-ABI unsafe allowance".into());
    }
    if !fs::read_to_string(root.join(GENERATED)).is_ok_and(|s| compact(&s) == compact(GLUE)) {
        violations.push(format!(
            "{GENERATED}: only the reviewed generator and export invocations are permitted"
        ));
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_the_exact_out_of_line_generated_module_is_allowed() {
        assert_eq!(
            inspect(
                OWNER,
                "#[allow(unsafe_code, clippy::same_length_and_capacity)] mod bindings;"
            )
            .unwrap(),
            1
        );
        for source in [
            "#![allow(unsafe_code)]",
            "#[allow(unsafe_code)] mod bindings;",
            "#[allow(unsafe_code, clippy::all)] mod bindings;",
            "#[allow(unsafe_code, clippy::same_length_and_capacity, unused)] mod bindings;",
            "macro_rules! bypass { () => { #[allow(unsafe_code)] fn f() {} } }",
            "#[expect(unsafe_code)] fn f() {}",
            "#[cfg_attr(any(),allow(unsafe_code))] fn f() {}",
            "#[allow(unsafe_code)] mod other;",
            "#[allow(unsafe_code)] mod bindings { unsafe fn f() {} }",
        ] {
            assert!(inspect(OWNER, source).is_err());
        }
        assert!(
            inspect(
                "crates/domain/src/lib.rs",
                "#[allow(unsafe_code)] mod bindings;"
            )
            .is_err()
        );
        assert!(
            inspect(
                OWNER,
                "// #[allow(unsafe_code)]\nfn f() { let _ = \"unsafe_code\"; }"
            )
            .is_ok()
        );
    }
    #[test]
    fn generated_boundary_rejects_handwritten_code() {
        assert_eq!(compact(GLUE), compact(&format!("  {GLUE}\n")));
        assert_ne!(
            compact(GLUE),
            compact(&format!("{GLUE} unsafe fn bypass() {{}}"))
        );
    }
}
