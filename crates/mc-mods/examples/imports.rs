fn main() -> anyhow::Result<()> {
    let path = std::env::args_os().nth(1).ok_or_else(|| anyhow::anyhow!("expected component path"))?;
    let data = std::fs::read(path)?;
    let mut depth = 0;
    for payload in wasmparser::Parser::new(0).parse_all(&data) {
        match payload? {
            wasmparser::Payload::Version { .. } => depth += 1,
            wasmparser::Payload::End(_) => depth -= 1,
            wasmparser::Payload::ComponentImportSection(section) if depth == 1 => {
                for item in section { println!("{}", item?.name.0); }
            }
            _ => {}
        }
    }
    Ok(())
}
