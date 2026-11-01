use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
}

/// A problem found in a package, located by file and YAML path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Issue {
    pub severity: Severity,
    /// File relative to the package root, e.g. `roles.yaml`.
    pub file: String,
    /// YAML path, e.g. `roles[3].requires.all[0]` (empty = whole file).
    pub path: String,
    pub message: String,
}

impl Issue {
    pub fn error(
        file: impl Into<String>,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: Severity::Error,
            file: file.into(),
            path: path.into(),
            message: message.into(),
        }
    }

    pub fn warning(
        file: impl Into<String>,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: Severity::Warning,
            ..Self::error(file, path, message)
        }
    }
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        if self.path.is_empty() {
            write!(f, "{level}: {}: {}", self.file, self.message)
        } else {
            write!(f, "{level}: {}: {}: {}", self.file, self.path, self.message)
        }
    }
}
