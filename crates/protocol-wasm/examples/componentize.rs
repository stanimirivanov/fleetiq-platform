//! Wrap a Rust-produced core module using its embedded WIT metadata.
use std::{env, fs};
use wit_component::ComponentEncoder;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: componentize INPUT.wasm OUTPUT.wasm".into());
    }
    let module = fs::read(&args[0])?;
    let component = ComponentEncoder::default()
        .module(&module)?
        .validate(true)
        .encode()?;
    fs::write(&args[1], component)?;
    Ok(())
}
