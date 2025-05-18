use hyperlinkr::config::settings;
use hyperlinkr::services::codegen::CodeGenerator;


#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let settings = settings::load()?;
    println!("Loaded settings: {:?}", settings);
    let codegen = CodeGenerator::new(); // Removed Arc for simplicity
    let code = codegen.next()?;
    println!("Generated code: {}", code);
    Ok(())
}