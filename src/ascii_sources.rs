#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsciiSourceType {
    Stream,
    Pack,
    Tool,
}

impl AsciiSourceType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Stream => "stream",
            Self::Pack => "pack",
            Self::Tool => "tool",
        }
    }

    fn heading(self) -> &'static str {
        match self {
            Self::Stream => "STREAM SOURCES",
            Self::Pack => "PACK SOURCES",
            Self::Tool => "TOOL SOURCES",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AsciiSourceEntry {
    pub id: &'static str,
    pub source_type: AsciiSourceType,
    pub description: &'static str,
    pub command_example: &'static str,
}

pub const ASCII_SOURCES: &[AsciiSourceEntry] = &[
    AsciiSourceEntry {
        id: "ascii-live:forrest",
        source_type: AsciiSourceType::Stream,
        description: "ascii.live endpoint from curated ASCII Art Collection gist (Forrest run loop).",
        command_example: "vcr ascii capture --source ascii-live:forrest --out renders/ascii_live_forrest.mov --duration 8 --fps 30 --size 120x45",
    },
    AsciiSourceEntry {
        id: "ascii-live:parrot",
        source_type: AsciiSourceType::Stream,
        description: "ascii.live endpoint from curated ASCII Art Collection gist (parrot animation).",
        command_example: "vcr ascii capture --source ascii-live:parrot --out renders/ascii_live_parrot.mov --duration 8 --fps 30 --size 120x45",
    },
    AsciiSourceEntry {
        id: "ascii-live:clock",
        source_type: AsciiSourceType::Stream,
        description: "ascii.live endpoint from curated ASCII Art Collection gist (animated clock).",
        command_example: "vcr ascii capture --source ascii-live:clock --out renders/ascii_live_clock.mov --duration 8 --fps 30 --size 120x45",
    },
    AsciiSourceEntry {
        id: "ascii-live:can-you-hear-me",
        source_type: AsciiSourceType::Stream,
        description: "ascii.live endpoint from curated ASCII Art Collection gist (voice-message style animation).",
        command_example: "vcr ascii capture --source ascii-live:can-you-hear-me --out renders/ascii_live_can_you_hear_me.mov --duration 8 --fps 30 --size 120x45",
    },
    AsciiSourceEntry {
        id: "ascii-live:donut",
        source_type: AsciiSourceType::Stream,
        description: "ascii.live endpoint from curated ASCII Art Collection gist (rotating donut).",
        command_example: "vcr ascii capture --source ascii-live:donut --out renders/ascii_live_donut.mov --duration 8 --fps 30 --size 120x45",
    },
    AsciiSourceEntry {
        id: "ascii-live:earth",
        source_type: AsciiSourceType::Stream,
        description: "ascii.live endpoint currently wired in vcr ascii capture (earth render).",
        command_example: "vcr ascii capture --source ascii-live:earth --out renders/ascii_live_earth.mov --duration 8 --fps 30 --size 120x45",
    },
    AsciiSourceEntry {
        id: "pack:16colo.rs",
        source_type: AsciiSourceType::Pack,
        description: "ANSI/ASCII art archive (manual download + local conversion workflow for now).",
        command_example: "vcr ascii capture --source chafa:/path/to/16colo_export.gif --out renders/16colors_pack.mov --duration 8 --fps 30 --size 120x45",
    },
    AsciiSourceEntry {
        id: "pack:library/geist-wave",
        source_type: AsciiSourceType::Pack,
        description: "Offline built-in dev source (animated wave with letter-heavy glyphs).",
        command_example: "vcr ascii capture --source library:geist-wave --out renders/library_geist_wave.mov --duration 8 --fps 30 --size 120x45",
    },
    AsciiSourceEntry {
        id: "pack:library/geist-scan",
        source_type: AsciiSourceType::Pack,
        description: "Offline built-in dev source (scanline text animation for pixel-font look dev).",
        command_example: "vcr ascii capture --source library:geist-scan --out renders/library_geist_scan.mov --duration 8 --fps 30 --size 120x45",
    },
    AsciiSourceEntry {
        id: "pack:library/geist-blocks",
        source_type: AsciiSourceType::Pack,
        description: "Offline built-in dev source (moving block field for square/pixel emphasis).",
        command_example: "vcr ascii capture --source library:geist-blocks --out renders/library_geist_blocks.mov --duration 8 --fps 30 --size 120x45",
    },
    AsciiSourceEntry {
        id: "tool:ansilove",
        source_type: AsciiSourceType::Tool,
        description: "Render ANSI/ASCII/NFO files to raster images before ingesting with chafa.",
        command_example: "vcr ascii capture --source chafa:/tmp/ansilove_render.gif --out renders/ansilove_pipeline.mov --duration 8 --fps 30 --size 120x45",
    },
    AsciiSourceEntry {
        id: "tool:chafa",
        source_type: AsciiSourceType::Tool,
        description: "Terminal graphics-to-text converter used directly by vcr ascii capture via chafa:<path>.",
        command_example: "vcr ascii capture --source chafa:./assets/welcome_terminal_scene.gif --out renders/chafa_source.mov --duration 8 --fps 30 --size 120x45",
    },
];

const ASCII_LIVE_STREAMS: &[(&str, &str)] = &[
    ("forrest", "https://ascii.live/forrest"),
    ("parrot", "https://ascii.live/parrot"),
    ("clock", "https://ascii.live/clock"),
    ("can-you-hear-me", "https://ascii.live/can-you-hear-me"),
    ("donut", "https://ascii.live/donut"),
    ("earth", "https://ascii.live/earth"),
];

const LIBRARY_SOURCE_IDS: &[&str] = &["geist-wave", "geist-scan", "geist-blocks"];

pub fn ascii_live_stream_url(stream: &str) -> Option<&'static str> {
    let normalized = stream.trim().to_ascii_lowercase();
    ASCII_LIVE_STREAMS
        .iter()
        .find_map(|(name, url)| (*name == normalized).then_some(*url))
}

pub fn ascii_live_stream_names() -> &'static [&'static str] {
    &[
        "forrest",
        "parrot",
        "clock",
        "can-you-hear-me",
        "donut",
        "earth",
    ]
}

pub fn library_source_names() -> &'static [&'static str] {
    LIBRARY_SOURCE_IDS
}

pub fn render_ascii_sources() -> String {
    let mut output = String::new();
    output.push_str("VCR ASCII SOURCES (discoverability registry)\n");
    output.push_str("Static list only. No network fetch, no scraping.\n");
    output.push('\n');
    output.push_str("Fields: id | type | description\n");
    output.push_str("Each entry includes a vcr ascii capture command example.\n");

    for kind in [
        AsciiSourceType::Stream,
        AsciiSourceType::Pack,
        AsciiSourceType::Tool,
    ] {
        output.push('\n');
        output.push_str(kind.heading());
        output.push('\n');
        for entry in ASCII_SOURCES.iter().filter(|item| item.source_type == kind) {
            output.push_str("- id: ");
            output.push_str(entry.id);
            output.push('\n');
            output.push_str("  type: ");
            output.push_str(entry.source_type.as_str());
            output.push('\n');
            output.push_str("  description: ");
            output.push_str(entry.description);
            output.push('\n');
            output.push_str("  command: ");
            output.push_str(entry.command_example);
            output.push('\n');
        }
    }

    output
}
