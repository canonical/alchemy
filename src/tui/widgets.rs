/// Braille spinner frames
#[allow(dead_code)]
pub const SPINNER_FRAMES: &[char] = &['⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];

#[allow(dead_code)]
pub fn spinner_frame(tick: usize) -> char {
    SPINNER_FRAMES[tick % SPINNER_FRAMES.len()]
}
