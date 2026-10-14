use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub struct FileId(pub u32);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize)]
pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(file: FileId, start: usize, end: usize) -> Self {
        Self {
            file,
            start: start as u32,
            end: end as u32,
        }
    }

    pub fn join(self, other: Self) -> Self {
        debug_assert_eq!(self.file, other.file);
        Self {
            file: self.file,
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SourceFile {
    pub id: FileId,
    pub path: PathBuf,
    pub text: String,
}

#[derive(Default, Debug)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn add(&mut self, path: impl Into<PathBuf>, text: String) -> FileId {
        let id = FileId(self.files.len() as u32);
        self.files.push(SourceFile {
            id,
            path: path.into(),
            text,
        });
        id
    }

    pub fn get(&self, id: FileId) -> &SourceFile {
        &self.files[id.0 as usize]
    }
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    pub fn line_column(&self, span: Span) -> (usize, usize) {
        let text = &self.get(span.file).text;
        let mut offset = (span.start as usize).min(text.len());
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        let prefix = &text[..offset];
        let mut line = 1;
        let mut column = 1;
        let mut chars = prefix.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    line += 1;
                    column = 1;
                }
                '\n' => {
                    line += 1;
                    column = 1;
                }
                _ => column += 1,
            }
        }
        (line, column)
    }
}
