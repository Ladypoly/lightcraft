//! Suggestions and review decisions, stored separately from ordinary keywords; XMP uses lc:aiTags.
use crate::Photo;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoTags {
    /// Model, digest and vocabulary/index version last scanned.
    pub model: String,
    pub content: String,
    pub tags: Vec<AutoTag>,
    /// Names explicitly dismissed, independent of model changes and reindexing.
    pub dismissed: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutoTag {
    pub name: String,
    /// CLIP cosine similarity or detector confidence, identified by `model`.
    pub score: f32,
    pub model: String,
    #[serde(default)]
    pub accepted: bool,
}

impl Photo {
    /// Identity shared by preview caches and the automatic keyword index.
    pub fn content_key(&self) -> String {
        match (&self.content_hash, &self.source) {
            (Some(h), _) => h.clone(),
            (None, crate::Source::Demo { scene }) => format!("demo:{scene}"),
            (None, crate::Source::File { path }) => format!("file:{path}:{}", self.file_size),
        }
    }

    /// Searchable suggestions for the current pixels; rejected, accepted and stale results stay out.
    pub fn suggested_tags(&self) -> impl Iterator<Item = &AutoTag> {
        let current = self.meta.auto_tags.as_ref().is_some_and(|a| a.content == self.content_key());
        self.meta.auto_tags.iter().filter(move |_| current).flat_map(|a| &a.tags).filter(|t| !t.accepted)
    }

    pub fn searchable_keywords(&self) -> impl Iterator<Item = &str> {
        self.meta.keywords.iter().map(String::as_str).chain(self.suggested_tags().map(|t| t.name.as_str()))
    }
}
