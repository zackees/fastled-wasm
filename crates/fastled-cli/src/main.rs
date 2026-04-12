use clap::Parser;

#[derive(Parser)]
#[command(name = "fastled", version, about = "FastLED WASM compilation CLI")]
struct Cli {
    /// Print version and exit
    #[arg(long)]
    version_info: bool,
}

fn main() {
    let _cli = Cli::parse();
    println!("fastled-cli {}", env!("CARGO_PKG_VERSION"));
}
