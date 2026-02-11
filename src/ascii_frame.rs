use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AsciiFrameMetadata {
    pub source_frame_index: Option<u64>,
    pub source_timestamp_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsciiFrame {
    width: usize,
    height: usize,
    lines: Vec<String>,
    pub metadata: Option<AsciiFrameMetadata>,
}

impl AsciiFrame {
    pub fn blank(width: usize, height: usize) -> Self {
        let line = " ".repeat(width);
        Self {
            width,
            height,
            lines: vec![line; height],
            metadata: None,
        }
    }

    pub fn from_text(text: &str, width: usize, height: usize) -> Self {
        let lines = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .lines()
            .map(|line| line.to_owned())
            .collect::<Vec<_>>();
        Self::from_lines(lines, width, height)
    }

    pub fn from_lines<I>(lines: I, width: usize, height: usize) -> Self
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let normalized = normalize_lines(lines, width, height);
        Self {
            width,
            height,
            lines: normalized,
            metadata: None,
        }
    }

    pub fn with_metadata(mut self, metadata: AsciiFrameMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn to_text(&self) -> String {
        if self.lines.is_empty() {
            return String::new();
        }
        let mut value = self.lines.join("\n");
        value.push('\n');
        value
    }
}

fn normalize_lines<I>(lines: I, width: usize, height: usize) -> Vec<String>
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    let mut normalized = lines
        .into_iter()
        .take(height)
        .map(|line| normalize_line(line.as_ref(), width))
        .collect::<Vec<_>>();

    if normalized.len() < height {
        normalized.extend(std::iter::repeat(" ".repeat(width)).take(height - normalized.len()));
    }

    normalized
}

fn normalize_line(line: &str, width: usize) -> String {
    let expanded = if line.contains('\t') {
        Cow::Owned(line.replace('\t', "    "))
    } else {
        Cow::Borrowed(line)
    };

    let mut output = expanded.chars().take(width).collect::<String>();
    if output.len() < width {
        output.push_str(&" ".repeat(width - output.len()));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::AsciiFrame;

    #[test]
    fn frame_from_text_normalizes_crlf_and_dimensions() {
        let frame = AsciiFrame::from_text("A\r\nB", 3, 3);
        assert_eq!(
            frame.lines(),
            &vec!["A  ".to_owned(), "B  ".to_owned(), "   ".to_owned()]
        );
    }

    #[test]
    fn frame_to_text_uses_stable_newlines() {
        let frame = AsciiFrame::from_lines(["AB", "CD"], 2, 2);
        assert_eq!(frame.to_text(), "AB\nCD\n");
    }
}
