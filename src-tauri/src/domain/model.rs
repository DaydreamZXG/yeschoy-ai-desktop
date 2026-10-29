//! Station-independent model enrollment rules shared by activation and install intent.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ModelBinding {
    pub(crate) model_id: String,
    pub(crate) billing_group: String,
}

pub(crate) fn bounded_plain_text(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.chars().count() <= maximum
        && !value.chars().any(|character| {
            character.is_control()
                || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

pub(crate) fn valid_model_bindings(models: &[ModelBinding], default: &str, group: &str) -> bool {
    let mut ids = HashSet::new();
    !models.is_empty()
        && models.len() <= 200
        && models.iter().all(|m| {
            bounded_plain_text(&m.model_id, 200)
                && !m.model_id.contains(',')
                && bounded_plain_text(&m.billing_group, 128)
                && m.billing_group != "auto"
                && ids.insert(m.model_id.as_str())
        })
        && models
            .iter()
            .any(|m| m.model_id == default && m.billing_group == group)
}

// Shared adapter predicates concern only public model IDs, never credentials.
// They were hosted by the retired loopback Chat gateway.
pub(crate) fn validate_catalog(
    default: &str,
    ids: &[String],
) -> Result<(), crate::domain::error::AdapterFailure> {
    let unique: HashSet<_> = ids.iter().collect();
    if ids.is_empty()
        || ids.len() > 200
        || unique.len() != ids.len()
        || !ids.iter().any(|id| id == default)
        || ids
            .iter()
            .any(|id| id.is_empty() || id.chars().count() > 200 || id.chars().any(char::is_control))
    {
        return Err(crate::domain::error::AdapterFailure::ConfigurationFailed(
            "configuration_parse_failed",
        ));
    }
    Ok(())
}

pub(crate) fn catalog_matches(
    value: &serde_json::Value,
    field: Option<&str>,
    ids: &[String],
) -> bool {
    value.as_array().is_some_and(|rows| {
        let actual: Option<HashSet<&str>> = rows
            .iter()
            .map(|row| field.map_or(row, |key| &row[key]).as_str())
            .collect();
        rows.len() == ids.len()
            && actual.is_some_and(|actual| {
                actual.len() == ids.len() && ids.iter().all(|id| actual.contains(id.as_str()))
            })
    })
}

pub(crate) fn default_matches(
    actual: Option<&str>,
    default: &str,
    ids: &[String],
    strict: bool,
) -> bool {
    actual.is_some_and(|actual| {
        if strict {
            actual == default
        } else {
            ids.iter().any(|id| id == actual)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_unique_scope_and_default_are_required() {
        let binding = ModelBinding {
            model_id: "synthetic-model".into(),
            billing_group: "fixture-group".into(),
        };
        assert!(valid_model_bindings(
            std::slice::from_ref(&binding),
            "synthetic-model",
            "fixture-group"
        ));
        assert!(!valid_model_bindings(
            &[binding.clone(), binding.clone()],
            "synthetic-model",
            "fixture-group"
        ));
        assert!(!valid_model_bindings(
            &[binding],
            "missing-model",
            "fixture-group"
        ));
        assert!(!valid_model_bindings(
            &[],
            "synthetic-model",
            "fixture-group"
        ));
    }

    #[test]
    fn opaque_metadata_cannot_inject_control_or_token_scope_separators() {
        for model in ["a,b", "a\nb", "a\u{202e}b"] {
            assert!(!valid_model_bindings(
                &[ModelBinding {
                    model_id: model.into(),
                    billing_group: "fixture-group".into()
                }],
                model,
                "fixture-group"
            ));
        }
    }
}
