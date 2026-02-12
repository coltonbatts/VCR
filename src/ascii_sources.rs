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
        id: "ascii-live:parrot",
        source_type: AsciiSourceType::Stream,
        description: "ascii.live endpoint (classic party parrot).",
        command_example: "vcr ascii capture --source ascii-live:parrot --out renders/parrot.mov",
    },
    AsciiSourceEntry {
        id: "ascii-live:nyan",
        source_type: AsciiSourceType::Stream,
        description: "ascii.live endpoint (nyan cat).",
        command_example: "vcr ascii capture --source ascii-live:nyan --out renders/nyan.mov",
    },
    AsciiSourceEntry {
        id: "ascii-live:forrest",
        source_type: AsciiSourceType::Stream,
        description: "ascii.live endpoint (Forrest Gump running).",
        command_example: "vcr ascii capture --source ascii-live:forrest --out renders/forrest.mov",
    },
    AsciiSourceEntry {
        id: "ascii-live:donut",
        source_type: AsciiSourceType::Stream,
        description: "ascii.live endpoint (rotating donut).",
        command_example: "vcr ascii capture --source ascii-live:donut --out renders/donut.mov",
    },
    AsciiSourceEntry {
        id: "ascii-live:clock",
        source_type: AsciiSourceType::Stream,
        description: "ascii.live endpoint (real-time clock).",
        command_example: "vcr ascii capture --source ascii-live:clock --out renders/clock.mov",
    },
    AsciiSourceEntry {
        id: "ascii-live:earth",
        source_type: AsciiSourceType::Stream,
        description: "ascii.live endpoint (rotating earth).",
        command_example: "vcr ascii capture --source ascii-live:earth --out renders/earth.mov",
    },
    AsciiSourceEntry {
        id: "ascii-live:maxwell",
        source_type: AsciiSourceType::Stream,
        description: "ascii.live endpoint (Maxwell the cat).",
        command_example: "vcr ascii capture --source ascii-live:maxwell --out renders/maxwell.mov",
    },
    AsciiSourceEntry {
        id: "ascii-live:rick",
        source_type: AsciiSourceType::Stream,
        description: "ascii.live endpoint (Rickroll).",
        command_example: "vcr ascii capture --source ascii-live:rick --out renders/rick.mov",
    },
    AsciiSourceEntry {
        id: "pack:16colo.rs",
        source_type: AsciiSourceType::Pack,
        description: "ANSI/ASCII art archive (manual download + local conversion workflow).",
        command_example: "vcr ascii capture --source chafa:/path/to/archive.gif --out renders/16colors.mov",
    },
    AsciiSourceEntry {
        id: "pack:library/geist-wave",
        source_type: AsciiSourceType::Pack,
        description: "Offline built-in: animated wave with letter-heavy glyphs.",
        command_example: "vcr ascii capture --source library:geist-wave --out renders/wave.mov",
    },
    AsciiSourceEntry {
        id: "tool:asciiville",
        source_type: AsciiSourceType::Tool,
        description: "Comprehensive ASCII/ANSI art suite and gallery browser.",
        command_example: "asciiville -V Art # Use to browse, then capture local files",
    },
    AsciiSourceEntry {
        id: "tool:chafa",
        source_type: AsciiSourceType::Tool,
        description: "Native VCR converter. Supports local files and remote URLs.",
        command_example: "vcr ascii capture --source chafa:https://example.com/video.mp4 --out renders/remote.mov",
    },
];

const ASCII_LIVE_STREAMS: &[(&str, &str)] = &[
    ("bnr", "https://ascii.live/bnr"),
    ("knot", "https://ascii.live/knot"),
    ("earth", "https://ascii.live/earth"),
    ("batman", "https://ascii.live/batman"),
    ("coin", "https://ascii.live/coin"),
    ("donut", "https://ascii.live/donut"),
    ("hes", "https://ascii.live/hes"),
    ("spidyswing", "https://ascii.live/spidyswing"),
    ("maxwell", "https://ascii.live/maxwell"),
    ("kitty", "https://ascii.live/kitty"),
    ("batman-running", "https://ascii.live/batman-running"),
    ("dvd", "https://ascii.live/dvd"),
    ("forrest", "https://ascii.live/forrest"),
    ("nyan", "https://ascii.live/nyan"),
    ("torus-knot", "https://ascii.live/torus-knot"),
    ("purdue", "https://ascii.live/purdue"),
    ("bomb", "https://ascii.live/bomb"),
    ("india", "https://ascii.live/india"),
    ("can-you-hear-me", "https://ascii.live/can-you-hear-me"),
    ("clock", "https://ascii.live/clock"),
    ("parrot", "https://ascii.live/parrot"),
    ("playstation", "https://ascii.live/playstation"),
    ("rick", "https://ascii.live/rick"),
    ("as", "https://ascii.live/as"),
];

const LIBRARY_SOURCE_IDS: &[&str] = &["geist-wave", "geist-scan", "geist-blocks"];

pub fn ascii_live_stream_url(stream: &str) -> Option<&'static str> {
    let normalized = stream.trim().to_ascii_lowercase();
    ASCII_LIVE_STREAMS
        .iter()
        .find_map(|(name, url)| (*name == normalized).then_some(*url))
}

pub fn ascii_live_stream_names() -> Vec<&'static str> {
    ASCII_LIVE_STREAMS.iter().map(|(name, _)| *name).collect()
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
