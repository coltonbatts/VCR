#[cfg(feature = "workflow")]
mod implementation {
    use std::fs;
    use std::path::{Path, PathBuf};

    use anyhow::{anyhow, bail, Context, Result};
    use clap::Parser;
    use image::RgbaImage;
    use url::Url;
    use vcr::animation_engine::{
        AnimationImportOptions, AnimationLayer, AnimationManager, AsciiCellMetrics, BoutiqueFilter,
        FitOptions, PlaybackOptions, DEFAULT_ANIMATIONS_ROOT,
    };
    use vcr::ascii_atlas::GeistPixelAtlas;
    use vcr::encoding::FfmpegPipe;
    use vcr::renderer::Renderer;
    use vcr::schema::{
        AsciiFontVariant, Duration as ManifestDuration, EncodingConfig, Environment,
        ProResProfile, Resolution,
    };
    use vcr::timeline::RenderSceneData;

    #[derive(Debug, Parser)]
    #[command(
        name = "ascii-link-overlay",
        about = "Import an ascii.co.uk animated-art URL and render white-on-alpha overlay video"
    )]
    struct Cli {
        #[arg(long)]
        url: String,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        width: Option<u32>,
        #[arg(long)]
        height: Option<u32>,
        #[arg(long, default_value_t = 24)]
        fps: u32,
        #[arg(long, default_value_t = 0)]
        frames: u32,
        #[arg(long, default_value_t = 16)]
        cell_width: u32,
        #[arg(long, default_value_t = 31)]
        // Use 31 or 32 for ~1:2 ratio. 16x31 is a common Geist sweet spot.
        cell_height: u32,
        #[arg(long, default_value_t = 0)]
        padding: u32,
        #[arg(long, default_value_t = false, action = clap::ArgAction::Set)]
        white_matte: bool,
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        trim_leading_blank: bool,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        checker_preview: bool,
        #[arg(long, default_value = "geist-pixel-line")]
        font: String,
    }

    pub fn main() -> Result<()> {
        let cli = Cli::parse();
        if let Some(w) = cli.width {
            if w == 0 {
                bail!("--width must be > 0");
            }
        }
        if let Some(h) = cli.height {
            if h == 0 {
                bail!("--height must be > 0");
            }
        }
        if cli.fps == 0 {
            bail!("--fps must be > 0");
        }

        let font_variant = parse_font_variant(&cli.font)
            .ok_or_else(|| anyhow!("invalid font variant: {}", cli.font))?;
        let atlas = GeistPixelAtlas::new(font_variant);

        let page = fetch_text(&cli.url)?;
        let parsed = parse_ascii_co_uk_page(&page).context("failed to parse ascii.co.uk page")?;
        if parsed.frames.is_empty() {
            bail!("no animation frames found at URL: {}", cli.url);
        }
        let normalized_frames = parsed
            .frames
            .iter()
            .map(|frame| normalize_frame_text(frame, &atlas))
            .collect::<Vec<_>>();
        let leading_blank_frames = count_leading_blank_frames(&normalized_frames);
        if leading_blank_frames >= normalized_frames.len() {
            bail!(
                "all parsed frames are blank after normalization for URL: {}",
                cli.url
            );
        }

        let animation_id = cli.id.unwrap_or_else(|| {
            derive_animation_id(&cli.url).unwrap_or_else(|| "imported_animation".to_owned())
        });
        let asset_dir = Path::new(DEFAULT_ANIMATIONS_ROOT).join(&animation_id);
        write_imported_animation(
            &asset_dir,
            &animation_id,
            &cli.url,
            &parsed.title,
            parsed.source_link.as_deref(),
            &normalized_frames,
        )?;
        println!(
            "Imported {} frames -> {} (leading blank frames: {})",
            normalized_frames.len(),
            asset_dir.display(),
            leading_blank_frames
        );

        let mut manager = AnimationManager::new();
        manager.load_from_assets_root(
            DEFAULT_ANIMATIONS_ROOT,
            &animation_id,
            AnimationImportOptions {
                source_fps: parsed.source_fps,
                strip_ansi_escape_codes: true,
            },
        )?;

        let source_cycle_frames = if cli.trim_leading_blank {
            normalized_frames
                .len()
                .saturating_sub(leading_blank_frames)
                .max(1)
        } else {
            normalized_frames.len()
        };
        let output_frames = if cli.frames > 0 {
            cli.frames
        } else {
            let cycle = ((source_cycle_frames as f64 * cli.fps as f64) / parsed.source_fps as f64)
                .ceil() as u32;
            cycle.max(1)
        };

        let mut max_cols = 0;
        let mut max_rows = 0;
        for frame in &normalized_frames {
            let lines: Vec<&str> = frame.lines().collect();
            max_rows = max_rows.max(lines.len() as u32);
            for line in lines {
                max_cols = max_cols.max(line.chars().count() as u32);
            }
        }

        let mut target_width = cli.width.unwrap_or(max_cols * cli.cell_width);
        let mut target_height = cli.height.unwrap_or(max_rows * cli.cell_height);

        // Ensure even dimensions for ffmpeg yuv420p compatibility
        if target_width % 2 != 0 {
            target_width += 1;
        }
        if target_height % 2 != 0 {
            target_height += 1;
        }

        println!(
            "Grid size: {}x{} | Native Resolution: {}x{}",
            max_cols, max_rows, target_width, target_height
        );

        let environment = Environment {
            resolution: Resolution {
                width: target_width,
                height: target_height,
            },
            fps: cli.fps,
            duration: ManifestDuration::Frames {
                frames: output_frames,
            },
            color_space: Default::default(),
            encoding: EncodingConfig {
                prores_profile: ProResProfile::Prores4444,
                ..EncodingConfig::default()
            },
        };

        let mut renderer = Renderer::new_software(&environment, &[], RenderSceneData::default())?;
        let mut layer = AnimationLayer {
            clip_name: animation_id.clone(),
            font_variant,
            ..AnimationLayer::new(&animation_id)
        };
        layer.playback = PlaybackOptions::default();
        if cli.trim_leading_blank && leading_blank_frames > 0 {
            layer.playback.start_offset_frames = u32::try_from(leading_blank_frames)
                .context("leading blank frame count exceeds u32")?;
        }
        layer.cell = AsciiCellMetrics {
            width: cli.cell_width,
            height: cli.cell_height,
            pixel_aspect_ratio: 1.0,
        };
        layer.colors.foreground = [255, 255, 255, 255];
        layer.colors.background = [255, 255, 255, 0];
        layer.fit = FitOptions {
            padding_px: cli.padding,
            anchor_x: 0.5,
            anchor_y: 0.5,
        };
        layer.filter = BoutiqueFilter {
            seed: 0,
            drop_frame_probability: 0.0,
            brightness_jitter: 0.0,
            horizontal_shift_px: 0,
        };

        let output_path = cli
            .output
            .unwrap_or_else(|| PathBuf::from(format!("renders/{}_white_alpha.mov", animation_id)));
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let still_path = output_path.with_extension("png");

        let ffmpeg = FfmpegPipe::spawn(&environment, &output_path)?;
        let mut first_frame_written = false;
        for frame_index in 0..output_frames {
            let mut rgba =
                renderer.render_frame_rgba_with_animation_layer(frame_index, &manager, &layer)?;
            if cli.white_matte {
                apply_white_matte_to_transparent(&mut rgba);
            }
            if !first_frame_written {
                write_png(&still_path, target_width, target_height, &rgba)?;
                first_frame_written = true;
            }
            ffmpeg.write_frame(rgba)?;
        }
        ffmpeg.finish()?;

        println!("Wrote {}", output_path.display());
        println!("Wrote {}", still_path.display());

        if cli.checker_preview {
            let checker_path = output_path.with_file_name(format!(
                "{}_checker.mp4",
                output_path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("overlay")
            ));
            write_checker_preview(
                &output_path,
                &checker_path,
                target_width,
                target_height,
                cli.fps,
            )?;
            println!("Wrote {}", checker_path.display());
        }

        Ok(())
    }

    fn fetch_text(url: &str) -> Result<String> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to build tokio runtime")?;
        runtime.block_on(async move {
            let response = reqwest::get(url)
                .await
                .with_context(|| format!("failed to fetch {url}"))?;
            let status = response.status();
            if !status.is_success() {
                bail!("request failed for {url}: HTTP {status}");
            }
            response
                .text()
                .await
                .with_context(|| format!("failed to read response body from {url}"))
        })
    }

    #[derive(Debug)]
    struct ParsedAsciiPage {
        title: String,
        source_fps: u32,
        source_link: Option<String>,
        frames: Vec<String>,
    }

    fn parse_ascii_co_uk_page(html: &str) -> Result<ParsedAsciiPage> {
        let frames = parse_js_frames(html)?;
        let title = extract_h1_text(html).unwrap_or_else(|| "Imported ASCII Animation".to_owned());
        let source_link = extract_source_link(html);
        let source_fps = extract_delay_ms(html)
            .map(|delay| ((1000.0 / delay as f64).round() as u32).max(1))
            .unwrap_or(24);

        Ok(ParsedAsciiPage {
            title,
            source_fps,
            source_link,
            frames,
        })
    }

    fn parse_js_frames(html: &str) -> Result<Vec<String>> {
        let bytes = html.as_bytes();
        let mut cursor = 0_usize;
        let mut indexed = Vec::<(usize, String)>::new();

        while let Some(rel) = html[cursor..].find("n[") {
            let start = cursor + rel;
            let mut i = start + 2;
            let mut index_text = String::new();
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                index_text.push(bytes[i] as char);
                i += 1;
            }
            if index_text.is_empty() || i >= bytes.len() || bytes[i] != b']' {
                cursor = start + 2;
                continue;
            }
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= bytes.len() || bytes[i] != b'=' {
                cursor = i;
                continue;
            }
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= bytes.len() || bytes[i] != b'\'' {
                cursor = i;
                continue;
            }
            i += 1;
            let mut escaped = false;
            let mut raw = String::new();
            while i < bytes.len() {
                let ch = bytes[i] as char;
                if escaped {
                    raw.push('\\');
                    raw.push(ch);
                    escaped = false;
                    i += 1;
                    continue;
                }
                if ch == '\\' {
                    escaped = true;
                    i += 1;
                    continue;
                }
                if ch == '\'' {
                    break;
                }
                raw.push(ch);
                i += 1;
            }
            if i >= bytes.len() {
                bail!("unterminated JS frame string while parsing");
            }

            let index = index_text
                .parse::<usize>()
                .with_context(|| format!("invalid frame index '{index_text}'"))?;
            let decoded = decode_js_escaped_string(&raw);
            indexed.push((index, decoded));
            cursor = i + 1;
        }

        indexed.sort_by_key(|(index, _)| *index);
        Ok(indexed.into_iter().map(|(_, frame)| frame).collect())
    }

    fn decode_js_escaped_string(input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let mut chars = input.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch != '\\' {
                out.push(ch);
                continue;
            }
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('\'') => out.push('\''),
                Some('"') => out.push('"'),
                Some('u') => {
                    let mut code = String::new();
                    for _ in 0..4 {
                        if let Some(hex) = chars.next() {
                            code.push(hex);
                        } else {
                            break;
                        }
                    }
                    if let Ok(value) = u32::from_str_radix(&code, 16) {
                        if let Some(decoded) = char::from_u32(value) {
                            out.push(decoded);
                        }
                    }
                }
                Some(other) => out.push(other),
                None => break,
            }
        }
        out
    }

    fn extract_h1_text(html: &str) -> Option<String> {
        let start = html.find("<h1>")?;
        let end = html[start + 4..].find("</h1>")?;
        let text = &html[start + 4..start + 4 + end];
        let compact = text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_owned();
        if compact.is_empty() {
            None
        } else {
            Some(compact)
        }
    }

    fn extract_delay_ms(html: &str) -> Option<u32> {
        let key = "setTimeout(r,";
        let start = html.find(key)? + key.len();
        let tail = &html[start..];
        let digits = tail
            .chars()
            .skip_while(|ch| ch.is_whitespace())
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>();
        digits.parse::<u32>().ok().filter(|value| *value > 0)
    }

    fn extract_source_link(html: &str) -> Option<String> {
        let source_pos = html.find("Source:<br />")?;
        let segment = &html[source_pos..html.len().min(source_pos + 1200)];
        let href_key = "href=\"";
        let href_start = segment.find(href_key)? + href_key.len();
        let rest = &segment[href_start..];
        let href_end = rest.find('"')?;
        Some(rest[..href_end].to_owned())
    }

    fn derive_animation_id(url: &str) -> Option<String> {
        let parsed = Url::parse(url).ok()?;
        let slug = parsed
            .path_segments()?
            .next_back()?
            .trim_end_matches(".html");
        if slug.is_empty() {
            return None;
        }
        Some(format!("ascii_co_uk_{}", sanitize_id(slug)))
    }

    fn sanitize_id(raw: &str) -> String {
        raw.chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() {
                    ch.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect::<String>()
            .split('_')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("_")
    }

    fn write_imported_animation(
        asset_dir: &Path,
        animation_id: &str,
        page_url: &str,
        title: &str,
        source_link: Option<&str>,
        normalized_frames: &[String],
    ) -> Result<()> {
        fs::create_dir_all(asset_dir)
            .with_context(|| format!("failed to create {}", asset_dir.display()))?;

        for (idx, frame) in normalized_frames.iter().enumerate() {
            let path = asset_dir.join(format!("{:04}.txt", idx + 1));
            fs::write(&path, frame)
                .with_context(|| format!("failed to write {}", path.display()))?;
        }

        let metadata = serde_json::json!({
            "title": title,
            "artist": "ascii.co.uk contributor (unknown)",
            "artist_url": source_link,
            "source_url": page_url,
            "license": "Unknown (source attribution required)",
            "tags": ["ascii.co.uk", "animated-art", animation_id],
            "credit": format!("{title} from ascii.co.uk (source: {page_url})")
        });
        let metadata_path = asset_dir.join("metadata.json");
        fs::write(&metadata_path, serde_json::to_string_pretty(&metadata)?)
            .with_context(|| format!("failed to write {}", metadata_path.display()))?;

        Ok(())
    }

    fn normalize_frame_text(frame: &str, atlas: &GeistPixelAtlas) -> String {
        let normalized = frame.replace("\r\n", "\n").replace('\r', "\n");
        let lines = normalized
            .lines()
            .map(|line| {
                line.chars()
                    .map(|ch| {
                        if ch.is_ascii() && (ch == ' ' || ch.is_ascii_graphic()) {
                            ch
                        } else if ch.is_whitespace() {
                            ' '
                        } else {
                            map_unicode_art_char(ch, atlas)
                        }
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }

    fn frame_has_visible_ascii(frame: &str) -> bool {
        frame.chars().any(|ch| ch.is_ascii_graphic())
    }

    fn count_leading_blank_frames(frames: &[String]) -> usize {
        frames
            .iter()
            .take_while(|frame| !frame_has_visible_ascii(frame))
            .count()
    }

    fn map_unicode_art_char(ch: char, atlas: &GeistPixelAtlas) -> char {
        match ch {
            // Density mapping for block characters
            '█' | '▉' | '▇' | '▆' | '▅' | '■' | '◼' | '⬛' | '●' => {
                atlas.closest_character_by_density(1.0) as char
            }
            '▓' => atlas.closest_character_by_density(0.75) as char,
            '▒' | '▄' | '▀' | '▌' | '▐' | '◆' | '◾' | '◉' => {
                atlas.closest_character_by_density(0.5) as char
            }
            '░' | '•' | '·' => atlas.closest_character_by_density(0.25) as char,

            // Line drawing - keep them as '-' or '+' but we could do more here later
            '─' | '━' | '│' | '┃' | '┄' | '┅' | '┆' | '┇' | '┈' | '┉' | '┊' | '┋' | '╴' | '╵'
            | '╶' | '╷' => '-',
            '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' | '╔' | '╗' | '╚' | '╝' | '╠'
            | '╣' | '╦' | '╩' | '╬' => '+',
            _ => atlas.closest_character_by_density(1.0) as char,
        }
    }

    fn parse_font_variant(s: &str) -> Option<AsciiFontVariant> {
        match s.to_ascii_lowercase().replace('_', "-").as_str() {
            "geist-pixel-line" | "line" | "geist-pixel-regular" | "regular" => {
                Some(AsciiFontVariant::GeistPixelLine)
            }
            "geist-pixel-square" | "square" | "geist-pixel-medium" | "medium" => {
                Some(AsciiFontVariant::GeistPixelSquare)
            }
            "geist-pixel-grid" | "grid" | "geist-pixel-bold" | "bold" => {
                Some(AsciiFontVariant::GeistPixelGrid)
            }
            "geist-pixel-circle" | "circle" | "geist-pixel-light" | "light" => {
                Some(AsciiFontVariant::GeistPixelCircle)
            }
            "geist-pixel-triangle" | "triangle" | "geist-pixel-mono" | "mono" => {
                Some(AsciiFontVariant::GeistPixelTriangle)
            }
            _ => None,
        }
    }

    fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
        let image = RgbaImage::from_raw(width, height, rgba.to_vec())
            .ok_or_else(|| anyhow!("failed to build RGBA image"))?;
        image
            .save(path)
            .with_context(|| format!("failed to write {}", path.display()))
    }

    fn apply_white_matte_to_transparent(rgba: &mut [u8]) {
        for pixel in rgba.chunks_exact_mut(4) {
            if pixel[3] == 0 {
                pixel[0] = 255;
                pixel[1] = 255;
                pixel[2] = 255;
            }
        }
    }

    fn write_checker_preview(
        alpha_mov: &Path,
        output_mp4: &Path,
        width: u32,
        height: u32,
        fps: u32,
    ) -> Result<()> {
        let status = std::process::Command::new("ffmpeg")
            .arg("-y")
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg(format!("color=c=#1a1a1a:s={}x{}:r={fps}", width, height))
            .arg("-i")
            .arg(alpha_mov)
            .arg("-filter_complex")
            .arg("[0][1]overlay=shortest=1")
            .arg("-c:v")
            .arg("libx264")
            .arg("-preset")
            .arg("slow")
            .arg("-crf")
            .arg("12")
            .arg("-pix_fmt")
            .arg("yuv420p")
            .arg(output_mp4)
            .status()
            .context("failed to spawn ffmpeg for checker preview")?;
        if !status.success() {
            bail!("failed to render checker preview with ffmpeg");
        }
        Ok(())
    }
}

#[cfg(feature = "workflow")]
fn main() -> anyhow::Result<()> {
    implementation::main()
}

#[cfg(not(feature = "workflow"))]
fn main() {
    eprintln!("ascii-link-overlay requires the 'workflow' feature.");
    std::process::exit(1);
}
