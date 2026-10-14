use crate::source::Span;

#[derive(Clone, Debug, serde::Serialize)]
pub struct Note {
    pub message: String,
    pub span: Option<Span>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Diagnostic {
    pub code: &'static str,
    pub message: String,
    pub span: Span,
    pub notes: Vec<Note>,
}

impl Diagnostic {
    pub fn new(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            span,
            notes: Vec::new(),
        }
    }

    pub fn note(mut self, span: Option<Span>, message: impl Into<String>) -> Self {
        self.notes.push(Note {
            message: message.into(),
            span,
        });
        self
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Diagnostic {}
pub type Result<T> = std::result::Result<T, Diagnostic>;
