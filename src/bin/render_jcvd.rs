use fontdue::{Font, FontSettings};
use image::{Rgba, RgbaImage};
use serde_json;
use std::fs;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let json_path = "jcvd_frames.json";
    let font_path = "assets/fonts/geist_pixel/GeistPixel-Line.ttf";
    let out_dir = "jcvd_frames";

    let frames_json = fs::read_to_string(json_path)?;
    let frames: Vec<String> = serde_json::from_str(&frames_json)?;

    let font_data = fs::read(font_path)?;
    let font =
        Font::from_bytes(font_data, FontSettings::default()).map_err(|e| anyhow::anyhow!(e))?;

    if !Path::new(out_dir).exists() {
        fs::create_dir_all(out_dir)?;
    }

    let cell_w = 10;
    let cell_h = 20;
    let cols = 100;
    let rows = 54;
    let img_w = cols * cell_w;
    let img_h = rows * cell_h;

    // Font size
    let font_size = 18.0;

    // 8 seconds at 30fps = 240 frames
    let target_frame_count = 240;

    for f_idx in 0..target_frame_count {
        let source_f_idx = f_idx % frames.len();
        let frame = &frames[source_f_idx];
        let mut img = RgbaImage::new(img_w, img_h);

        let lines: Vec<&str> = frame.lines().collect();
        for (row_idx, line) in lines.iter().enumerate() {
            if row_idx >= rows as usize {
                break;
            }
            for (col_idx, char) in line.chars().enumerate() {
                if col_idx >= cols as usize {
                    break;
                }
                if char.is_whitespace() {
                    continue;
                }

                let (metrics, bitmap) = font.rasterize(char, font_size);

                // Draw white character
                let x_start = (col_idx as u32 * cell_w) as i32 + metrics.xmin;
                let y_start =
                    (row_idx as u32 * cell_h) as i32 + (cell_h as i32 - metrics.height as i32) / 2;

                for y in 0..metrics.height {
                    for x in 0..metrics.width {
                        let alpha = bitmap[y * metrics.width + x];
                        if alpha > 0 {
                            let px = x_start + x as i32;
                            let py = y_start + y as i32;
                            if px >= 0 && px < img_w as i32 && py >= 0 && py < img_h as i32 {
                                img.put_pixel(px as u32, py as u32, Rgba([255, 255, 255, alpha]));
                            }
                        }
                    }
                }
            }
        }

        let out_path = format!("{}/frame_{:04}.png", out_dir, f_idx);
        img.save(out_path)?;
        if f_idx % 20 == 0 {
            println!("Rendered {}/{} frames...", f_idx, target_frame_count);
        }
    }

    println!(
        "Finished rendering {} frames (looped from {} source frames).",
        target_frame_count,
        frames.len()
    );
    Ok(())
}
