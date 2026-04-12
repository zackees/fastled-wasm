use clap::Parser;

#[derive(Parser)]
#[command(name = "fastled-viewer", version, about = "FastLED WASM native viewer")]
struct Cli {
    /// Directory containing compiled frontend assets (fastled.js, fastled.wasm, index.html)
    #[arg(long)]
    frontend_dir: Option<String>,

    /// Window title
    #[arg(long, default_value = "FastLED Viewer")]
    title: String,

    /// Window width
    #[arg(long, default_value = "800")]
    width: u32,

    /// Window height
    #[arg(long, default_value = "600")]
    height: u32,
}

fn main() {
    let cli = Cli::parse();

    tauri::Builder::default()
        .setup(move |app| {
            let mut builder = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            );
            builder = builder
                .title(&cli.title)
                .inner_size(cli.width as f64, cli.height as f64);
            builder.build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}
