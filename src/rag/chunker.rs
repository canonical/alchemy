use std::path::Path;

pub struct Chunker {
    pub chunk_size: usize,
    pub chunk_overlap: usize,
}

pub struct Chunk {
    pub content: String,
    pub index: usize,
}

impl Chunker {
    pub fn new(chunk_size: usize, chunk_overlap: usize) -> Self {
        Self { chunk_size, chunk_overlap }
    }

    pub fn chunk(&self, content: &str, path: &Path) -> Vec<Chunk> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        match ext {
            "md" | "markdown" => self.chunk_markdown(content),
            "rs" | "py" | "js" | "ts" | "go" | "c" | "cpp" | "java" => self.chunk_code(content),
            _ => self.chunk_text(content),
        }
    }

    fn chunk_text(&self, content: &str) -> Vec<Chunk> {
        let words: Vec<&str> = content.split_whitespace().collect();
        let mut chunks = Vec::new();
        let mut i = 0;
        let mut idx = 0;

        while i < words.len() {
            let end = (i + self.chunk_size).min(words.len());
            let chunk_content = words[i..end].join(" ");
            chunks.push(Chunk { content: chunk_content, index: idx });
            idx += 1;
            let step = if self.chunk_size > self.chunk_overlap {
                self.chunk_size - self.chunk_overlap
            } else {
                1
            };
            i += step;
        }

        if chunks.is_empty() && !content.is_empty() {
            chunks.push(Chunk { content: content.to_string(), index: 0 });
        }

        chunks
    }

    fn chunk_markdown(&self, content: &str) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let mut current = String::new();
        let mut idx = 0;

        for line in content.lines() {
            if line.starts_with('#') && !current.is_empty() {
                chunks.push(Chunk { content: current.clone(), index: idx });
                idx += 1;
                current.clear();
            }
            current.push_str(line);
            current.push('\n');

            // Check size
            if current.split_whitespace().count() >= self.chunk_size {
                chunks.push(Chunk { content: current.clone(), index: idx });
                idx += 1;
                current.clear();
            }
        }

        if !current.trim().is_empty() {
            chunks.push(Chunk { content: current, index: idx });
        }

        if chunks.is_empty() && !content.is_empty() {
            chunks.push(Chunk { content: content.to_string(), index: 0 });
        }

        chunks
    }

    fn chunk_code(&self, content: &str) -> Vec<Chunk> {
        // Simple: split on blank lines or function boundaries
        let mut chunks = Vec::new();
        let mut current = String::new();
        let mut idx = 0;

        for line in content.lines() {
            current.push_str(line);
            current.push('\n');

            if current.split_whitespace().count() >= self.chunk_size {
                chunks.push(Chunk { content: current.clone(), index: idx });
                idx += 1;
                current.clear();
            }
        }

        if !current.trim().is_empty() {
            chunks.push(Chunk { content: current, index: idx });
        }

        if chunks.is_empty() && !content.is_empty() {
            chunks.push(Chunk { content: content.to_string(), index: 0 });
        }

        chunks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_chunk_text() {
        let chunker = Chunker::new(10, 2);
        let content = "word ".repeat(25);
        let chunks = chunker.chunk(&content, &PathBuf::from("test.txt"));
        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].index, 0);
    }

    #[test]
    fn test_chunk_markdown() {
        let chunker = Chunker::new(512, 64);
        let content = "# Title\nSome content\n# Another\nMore content";
        let chunks = chunker.chunk(&content, &PathBuf::from("test.md"));
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_empty_content() {
        let chunker = Chunker::new(512, 64);
        let chunks = chunker.chunk("", &PathBuf::from("test.txt"));
        assert!(chunks.is_empty());
    }
}
