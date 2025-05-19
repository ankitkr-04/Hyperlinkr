use hyperlinkr::config::cache::CacheConfig;
use hyperlinkr::config::settings;
use hyperlinkr::services::codegen::generator::CodeGenerator;


#[tokio::main]
async fn main() -> anyhow::Result<()> {

    let _config = CacheConfig {
        l1_capacity: 2_000_000, // 2 million
        l2_capacity: 10_000_000, // 10 million
        bloom_bits: 1_000_000_000, // 100 MB
        bloom_expected: 100_000_000, // 100 million items
        redis_pool_size: 32, // 32 connections
    };

    let settings = settings::load()?;
    println!("Loaded settings: {:?}", settings);
    let codegen = CodeGenerator::new(); // Removed Arc for simplicity
    let code = codegen.next()?;
    println!("Generated code: {}", code);
    Ok(())
    
}