//! Language names and the syntect tokens iced's highlighter understands.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Plain,
    Rust,
    Markdown,
    Python,
    Toml,
    Json,
    Javascript,
    Typescript,
    Html,
    Css,
    C,
    Cpp,
    Go,
    Shell,
    Lua,
    Yaml,
}

impl Language {
    pub const ALL: &'static [Language] = &[
        Self::Plain,
        Self::Rust,
        Self::Markdown,
        Self::Python,
        Self::Toml,
        Self::Json,
        Self::Javascript,
        Self::Typescript,
        Self::Html,
        Self::Css,
        Self::C,
        Self::Cpp,
        Self::Go,
        Self::Shell,
        Self::Lua,
        Self::Yaml,
    ];

    /// Token passed to `text_editor::highlight`, matched by syntect.
    pub fn token(self) -> &'static str {
        match self {
            Self::Plain => "txt",
            Self::Rust => "rs",
            Self::Markdown => "md",
            Self::Python => "py",
            Self::Toml => "toml",
            Self::Json => "json",
            Self::Javascript => "js",
            Self::Typescript => "ts",
            Self::Html => "html",
            Self::Css => "css",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Go => "go",
            Self::Shell => "sh",
            Self::Lua => "lua",
            Self::Yaml => "yaml",
        }
    }

    pub fn from_path(path: &Path) -> Self {
        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref()
        {
            Some("rs") => Self::Rust,
            Some("md") | Some("markdown") => Self::Markdown,
            Some("py") => Self::Python,
            Some("toml") => Self::Toml,
            Some("json") => Self::Json,
            Some("js") | Some("mjs") | Some("cjs") => Self::Javascript,
            Some("ts") | Some("tsx") | Some("jsx") => Self::Typescript,
            Some("html") | Some("htm") => Self::Html,
            Some("css") => Self::Css,
            Some("c") | Some("h") => Self::C,
            Some("cpp") | Some("cc") | Some("cxx") | Some("hpp") => Self::Cpp,
            Some("go") => Self::Go,
            Some("sh") | Some("bash") | Some("zsh") => Self::Shell,
            Some("lua") => Self::Lua,
            Some("yaml") | Some("yml") => Self::Yaml,
            Some("txt") | None => Self::Plain,
            Some(_) => Self::Plain,
        }
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Plain => "Plain",
            Self::Rust => "Rust",
            Self::Markdown => "Markdown",
            Self::Python => "Python",
            Self::Toml => "TOML",
            Self::Json => "JSON",
            Self::Javascript => "JavaScript",
            Self::Typescript => "TypeScript",
            Self::Html => "HTML",
            Self::Css => "CSS",
            Self::C => "C",
            Self::Cpp => "C++",
            Self::Go => "Go",
            Self::Shell => "Shell",
            Self::Lua => "Lua",
            Self::Yaml => "YAML",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_extensions() {
        assert_eq!(Language::from_path(Path::new("src/app.rs")), Language::Rust);
        assert_eq!(
            Language::from_path(Path::new("notes.md")),
            Language::Markdown
        );
        assert_eq!(Language::from_path(Path::new("scratch")), Language::Plain);
    }
}
