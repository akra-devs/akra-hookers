use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

const MAXIMUM_SCALARS: usize = 80;
const FALLBACK_NAME: &str = "Untitled Project";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectName {
    display: String,
    normalized: String,
    origin: Option<String>,
}

impl ProjectName {
    pub fn parse(value: &str) -> Result<Self, ProjectNameError> {
        let display = value.trim().to_owned();
        if display.is_empty() {
            return Err(ProjectNameError::Blank);
        }

        if let Some((index, character)) = display
            .chars()
            .enumerate()
            .find(|(_, character)| character.is_control() && !character.is_whitespace())
        {
            return Err(ProjectNameError::ControlCharacter { index, character });
        }

        let actual = display.chars().count();
        if actual > MAXIMUM_SCALARS {
            return Err(ProjectNameError::TooLong {
                maximum: MAXIMUM_SCALARS,
                actual,
            });
        }

        Ok(Self {
            normalized: normalize(&display),
            display,
            origin: None,
        })
    }

    pub fn suggest_from_path(path: &Path) -> Self {
        let suggestion = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(FALLBACK_NAME);
        Self::parse(suggestion).unwrap_or_else(|_| Self::fallback())
    }

    pub fn suggest_from_git_root_or_path(git_root: Option<&Path>, path: &Path) -> Self {
        Self::suggest_from_path(git_root.unwrap_or(path))
    }

    pub fn display(&self) -> &str {
        &self.display
    }

    pub fn normalized(&self) -> &str {
        &self.normalized
    }

    pub fn origin(&self) -> Option<&str> {
        self.origin.as_deref()
    }

    fn fallback() -> Self {
        Self {
            display: FALLBACK_NAME.to_owned(),
            normalized: normalize(FALLBACK_NAME),
            origin: None,
        }
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ProjectNameError {
    #[error("project name cannot be blank")]
    Blank,
    #[error("project name contains control character {character:?} at scalar {index}")]
    ControlCharacter { index: usize, character: char },
    #[error("project name has {actual} Unicode scalars; maximum is {maximum}")]
    TooLong { maximum: usize, actual: usize },
}

#[derive(Debug, Default)]
pub struct ProjectNames {
    by_origin: HashMap<String, ProjectName>,
    normalized: HashSet<String>,
}

impl ProjectNames {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allocate(&mut self, origin: &str, suggested: &str) -> ProjectName {
        if let Some(existing) = self.by_origin.get(origin) {
            return existing.clone();
        }

        let base = ProjectName::parse(suggested).unwrap_or_else(|_| ProjectName::fallback());
        let mut candidate = base.clone();
        let mut suffix = 2;
        while self.normalized.contains(candidate.normalized()) {
            candidate = suffixed(&base, suffix);
            suffix += 1;
        }
        candidate.origin = Some(origin.to_owned());

        self.normalized.insert(candidate.normalized.clone());
        self.by_origin.insert(origin.to_owned(), candidate.clone());
        candidate
    }
}

fn normalize(value: &str) -> String {
    let mut normalized = String::new();
    let mut pending_space = false;

    for character in value.nfkc().flat_map(char::to_lowercase) {
        if character.is_whitespace() {
            pending_space = !normalized.is_empty();
        } else {
            if pending_space {
                normalized.push(' ');
                pending_space = false;
            }
            normalized.push(character);
        }
    }
    normalized
}

fn suffixed(base: &ProjectName, suffix: usize) -> ProjectName {
    let suffix = format!(" ({suffix})");
    let keep = MAXIMUM_SCALARS.saturating_sub(suffix.chars().count());
    let stem = base.display.chars().take(keep).collect::<String>();
    let display = format!("{}{suffix}", stem.trim_end());

    ProjectName {
        normalized: normalize(&display),
        display,
        origin: None,
    }
}
