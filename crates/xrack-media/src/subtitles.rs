use std::{fs, path::Path};

pub(crate) struct Cue {
    pub(crate) start: f64,
    pub(crate) end: f64,
    pub(crate) text: String,
}

fn time(value: &str) -> Option<f64> {
    let mut parts = value.split(':').rev();
    let seconds: f64 = parts.next()?.replace(',', ".").parse().ok()?;
    let minutes: u32 = parts.next()?.parse().ok()?;
    let hours: u32 = parts.next().unwrap_or("0").parse().ok()?;
    (parts.next().is_none() && (0.0..60.0).contains(&seconds) && minutes < 60)
        .then_some(f64::from(hours) * 3600.0 + f64::from(minutes) * 60.0 + seconds)
}

pub(crate) fn load(path: &Path) -> Result<Vec<Cue>, String> {
    use std::io::Read;
    const LIMIT: u64 = 4 * 1024 * 1024;
    let mut text = String::new();
    fs::File::open(path)
        .and_then(|file| file.take(LIMIT + 1).read_to_string(&mut text))
        .map_err(|error| format!("read subtitles {}: {error}", path.display()))?;
    if text.len() as u64 > LIMIT {
        return Err("subtitle file exceeds 4 MiB".into());
    }
    let mut cues = Vec::new();
    for block in text.replace("\r\n", "\n").split("\n\n") {
        let mut lines = block.lines();
        let Some((start, end)) = lines.find_map(|line| line.split_once("-->")) else {
            continue;
        };
        let Some(start) = time(start.trim()) else {
            continue;
        };
        let Some(end) = end.split_whitespace().next().and_then(time) else {
            continue;
        };
        let text = lines.collect::<Vec<_>>().join("\n");
        if start < end && !text.is_empty() {
            cues.push(Cue { start, end, text });
        }
    }
    cues.sort_by(|left, right| left.start.total_cmp(&right.start));
    Ok(cues)
}

pub(crate) fn at(cues: &[Cue], position: f64) -> String {
    cues[..cues.partition_point(|cue| cue.start <= position)]
        .iter()
        .filter(|cue| cue.end > position)
        .map(|cue| cue.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_srt_and_webvtt_at_seeked_positions() {
        use std::{fs, path::PathBuf};
        let base = std::env::temp_dir().join(format!("xrack-subtitles-{}", std::process::id()));
        for (ext, input) in [
            ("srt", "1\r\n00:00:01,000 --> 00:00:02,000\r\nfirst\r\n"),
            (
                "vtt",
                "WEBVTT\n\n00:01.000 --> 00:02.000 align:center\nfirst\n",
            ),
        ] {
            let path: PathBuf = base.with_extension(ext);
            fs::write(&path, input).unwrap();
            let cues = super::load(&path).unwrap();
            assert_eq!(super::at(&cues, 1.2), "first");
            assert_eq!(super::at(&cues, 2.0), "");
            fs::remove_file(path).unwrap();
        }
    }
}
