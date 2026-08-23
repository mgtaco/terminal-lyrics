//! Diagnostic: convert a TTML file to enhanced LRC and show what was parsed.
use terminal_lyrics::lrc;
use terminal_lyrics::lyrics::ttml;

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("usage: ttml_probe <file.ttml>");
    let xml = std::fs::read_to_string(path)?;
    let a2 = ttml::to_enhanced_lrc(&xml)?;
    for line in a2.lines().take(4) {
        println!("{line}");
    }
    let parsed = lrc::parse(&a2);
    println!("\nlines: {}", parsed.lines.len());
    println!("word timings: {}", parsed.has_word_timings());
    for line in parsed.lines.iter().take(3) {
        println!("  [{:.3}..{:.3}] {:?}", line.start, line.end, line.text);
        for w in &line.words {
            println!(
                "      {:.3}..{:.3} {:?}",
                w.start,
                w.end,
                line.text.chars().skip(w.range.start).take(w.range.end - w.range.start).collect::<String>()
            );
        }
    }
    Ok(())
}
