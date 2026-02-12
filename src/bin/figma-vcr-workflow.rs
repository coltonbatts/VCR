use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::Utc;
use clap::Parser;
use reqwest::Client;
use vcr::workflow::assets::download_product_card_assets;
use vcr::workflow::figma_client::FigmaClient;
use vcr::workflow::frame_client::maybe_upload_render_to_frame_io;
use vcr::workflow::manifest_generator::ManifestGenerator;
use vcr::workflow::vcr_renderer::{check_manifest, render_manifest};

const FIGMA_VCR_HEADER: &str = r#"
 ███████╗██╗ ██████╗ ███╗   ███╗ █████╗     ██╗    ██╗ ██████╗██████╗ 
 ██╔════╝██║██╔════╝ ████╗ ████║██╔══██╗    ██║    ██║██╔════╝██╔══██╗
 █████╗  ██║██║  ███╗██╔████╔██║███████║    ██║    ██║██║     ██████╔╝
 ██╔══╝  ██║██║   ██║██║╚██╔╝██║██╔══██║    ╚██╗  ██╔╝██║     ██╔══██╗
 ██║     ██║╚██████╔╝██║ ╚═╝ ██║██║  ██║     ╚████╔╝ ╚██████╗██║  ██║
 ╚═╝     ╚═╝ ╚═════╝ ╚═╝     ╚═╝╚═╝  ╚═╝      ╚═══╝   ╚═════╝╚═╝  ╚═╝
"#;

#[derive(Debug, Parser)]
#[command(name = "figma-vcr-workflow")]
#[command(about = "Figma -> VCR motion graphics workflow for product card MVP")]
struct Cli {
    #[arg(long = "figma-file")]
    figma_file: String,
    #[arg(long = "description")]
    description: String,
    #[arg(long = "frame-project")]
    frame_project: Option<String>,
    #[arg(long = "output-folder", default_value = "./exports")]
    output_folder: PathBuf,
    #[arg(long = "skip-render", default_value_t = false)]
    skip_render: bool,
    #[arg(long = "render-timeout-seconds", default_value_t = 30)]
    render_timeout_seconds: u64,
    #[arg(long = "verbose", default_value_t = false)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    println!("{}", FIGMA_VCR_HEADER);
    println!("Figma -> VCR workflow initialization\n");
    let started = Instant::now();

    let figma_token = env::var("FIGMA_TOKEN")
        .context("FIGMA_TOKEN is required to fetch data from the Figma API")?;
    let anthropic_api_key = env::var("ANTHROPIC_API_KEY").ok();
    let anthropic_model = env::var("ANTHROPIC_MODEL").ok();
    let frame_token = env::var("FRAME_IO_TOKEN").ok();

    fs::create_dir_all(&cli.output_folder).with_context(|| {
        format!(
            "failed to create output folder {}",
            cli.output_folder.display()
        )
    })?;
    let run_folder = cli
        .output_folder
        .join(format!("figma-vcr-{}", Utc::now().format("%Y%m%d-%H%M%S")));
    fs::create_dir_all(&run_folder)
        .with_context(|| format!("failed to create run folder {}", run_folder.display()))?;

    let http = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(25))
        .build()
        .context("failed to create HTTP client")?;

    println!("1/5 Extracting product card data from Figma...");
    let figma_client = FigmaClient::new(http.clone(), figma_token, cli.verbose);
    let card_data = figma_client
        .extract_product_card_data(&cli.figma_file, &cli.description)
        .await
        .context("Figma extraction failed")?;
    let extracted_json_path = run_folder.join("product_card_data.json");
    fs::write(
        &extracted_json_path,
        serde_json::to_string_pretty(&card_data)?,
    )
    .with_context(|| format!("failed to write {}", extracted_json_path.display()))?;

    println!("2/5 Downloading Figma-exported node assets...");
    let local_assets = download_product_card_assets(&http, &card_data, &run_folder, cli.verbose)
        .await
        .context("failed downloading Figma assets")?;

    println!("3/5 Generating VCR manifest...");
    let generator = ManifestGenerator::new(http, anthropic_api_key, anthropic_model, cli.verbose);
    let manifest_output = generator
        .generate_manifest(&card_data, &local_assets, &cli.description)
        .await
        .context("manifest generation failed")?;
    let manifest_path = run_folder.join("product_card.vcr");
    fs::write(&manifest_path, manifest_output.yaml)
        .with_context(|| format!("failed writing manifest {}", manifest_path.display()))?;

    println!("4/5 Validating manifest with VCR...");
    check_manifest(&manifest_path, Duration::from_secs(20))
        .context("VCR manifest validation failed")?;

    let mov_path = run_folder.join("product_card.mov");
    let mut render_note = "Skipped render (--skip-render enabled).".to_owned();
    let mut render_stdout = String::new();
    let mut render_stderr = String::new();

    if !cli.skip_render {
        println!("5/5 Rendering with VCR...");
        let render_result = render_manifest(
            &manifest_path,
            &mov_path,
            Duration::from_secs(cli.render_timeout_seconds),
        )
        .context("VCR render failed")?;
        render_note = format!(
            "Render completed in {} ms at {}",
            render_result.elapsed_ms,
            render_result.output_path.display()
        );
        render_stdout = render_result.stdout;
        render_stderr = render_result.stderr;
    } else {
        println!("5/5 Render skipped by user.");
    }

    let frame_result = if cli.skip_render {
        vcr::workflow::types::FrameUploadResult {
            uploaded: false,
            link: None,
            note: "Frame.io upload skipped because no render output was produced.".to_owned(),
        }
    } else {
        maybe_upload_render_to_frame_io(
            cli.frame_project.as_deref(),
            frame_token.as_deref(),
            &mov_path,
        )
        .await
        .context("Frame.io upload step failed")?
    };

    println!("\nWorkflow complete.");
    println!("Run folder: {}", run_folder.display());
    println!("Extracted data JSON: {}", extracted_json_path.display());
    println!("Manifest: {}", manifest_path.display());
    if !cli.skip_render {
        println!("Render output: {}", mov_path.display());
    }
    println!("Manifest generation: {}", manifest_output.note);
    println!("Render: {render_note}");
    println!("Frame.io: {}", frame_result.note);
    if let Some(link) = frame_result.link {
        println!("Frame.io link: {link}");
    }
    if !render_stdout.trim().is_empty() {
        println!("\nVCR stdout:\n{render_stdout}");
    }
    if !render_stderr.trim().is_empty() {
        println!("\nVCR stderr:\n{render_stderr}");
    }
    println!(
        "\nTotal elapsed: {} ms",
        Instant::now().duration_since(started).as_millis()
    );

    Ok(())
}
