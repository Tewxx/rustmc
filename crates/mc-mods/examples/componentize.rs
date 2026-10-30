//! Convert a `wit-bindgen` wasm32-unknown-unknown core module into a component without WASI.
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let input = args.next().ok_or_else(|| anyhow::anyhow!("expected input.wasm output.wasm"))?;
    let output = args.next().ok_or_else(|| anyhow::anyhow!("expected input.wasm output.wasm"))?;
    if args.next().is_some() { anyhow::bail!("expected input.wasm output.wasm"); }
    let core = std::fs::read(input)?;
    let component = wit_component::ComponentEncoder::default().module(&core)?.validate(true).encode()?;
    std::fs::write(output, component)?;
    Ok(())
}
