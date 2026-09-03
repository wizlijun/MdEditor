//! Harness-neutral model selection for one task invocation.
//!
//! A task template may still pin a model, but callers such as Smart Search need
//! to choose a provider-specific profile or exact model for one run without
//! rewriting the shared task on disk. This module owns that small wire contract
//! so every provider validates it identically.
use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::fmt;

/// The only portable model profiles understood by the agent-provider protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelProfile {
    Fast,
    Default,
}

impl ModelProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Default => "default",
        }
    }
}

/// A validated invocation-level model request.
///
/// The fields stay private so an instance cannot represent the forbidden state
/// where both `model_profile` and `model` are present. Providers should parse
/// their complete run-task context with [`Self::from_context`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InvocationModelRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    model_profile: Option<ModelProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
}

impl Default for InvocationModelRequest {
    fn default() -> Self {
        Self {
            model_profile: None,
            model: None,
        }
    }
}

impl InvocationModelRequest {
    /// Read the two model selectors from a complete `run-task` context.
    /// Unrelated task fields are deliberately ignored.
    pub fn from_context(context: &Value) -> Result<Self, InvocationModelRequestError> {
        let object = context
            .as_object()
            .ok_or(InvocationModelRequestError::ContextMustBeObject)?;
        parse_fields(object.get("model_profile"), object.get("model"))
    }

    pub fn model_profile(&self) -> Option<ModelProfile> {
        self.model_profile
    }

    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    pub fn is_unspecified(&self) -> bool {
        self.model_profile.is_none() && self.model.is_none()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InvocationModelWire {
    #[serde(default)]
    model_profile: Option<Value>,
    #[serde(default)]
    model: Option<Value>,
}

impl<'de> Deserialize<'de> for InvocationModelRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = InvocationModelWire::deserialize(deserializer)?;
        parse_fields(wire.model_profile.as_ref(), wire.model.as_ref()).map_err(de::Error::custom)
    }
}

fn parse_fields(
    profile: Option<&Value>,
    model: Option<&Value>,
) -> Result<InvocationModelRequest, InvocationModelRequestError> {
    let profile = optional_string(profile, "model_profile")?;
    let model = optional_string(model, "model")?;

    if profile.is_some() && model.is_some() {
        return Err(InvocationModelRequestError::MutuallyExclusive);
    }

    if let Some(profile) = profile {
        let model_profile = match profile.as_str() {
            "fast" => ModelProfile::Fast,
            "default" => ModelProfile::Default,
            _ => return Err(InvocationModelRequestError::UnknownProfile(profile)),
        };
        return Ok(InvocationModelRequest {
            model_profile: Some(model_profile),
            model: None,
        });
    }

    if let Some(model) = model {
        let model = model.trim();
        if model.is_empty() {
            return Err(InvocationModelRequestError::EmptyModel);
        }
        return Ok(InvocationModelRequest {
            model_profile: None,
            model: Some(model.to_string()),
        });
    }

    Ok(InvocationModelRequest::default())
}

fn optional_string(
    value: Option<&Value>,
    field: &'static str,
) -> Result<Option<String>, InvocationModelRequestError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(InvocationModelRequestError::MustBeString(field)),
    }
}

/// Why an invocation-level model request was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvocationModelRequestError {
    ContextMustBeObject,
    MustBeString(&'static str),
    MutuallyExclusive,
    UnknownProfile(String),
    EmptyModel,
}

impl fmt::Display for InvocationModelRequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContextMustBeObject => write!(f, "run-task context must be an object"),
            Self::MustBeString(field) => write!(f, "'{field}' must be a string"),
            Self::MutuallyExclusive => {
                write!(f, "'model_profile' and 'model' are mutually exclusive")
            }
            Self::UnknownProfile(profile) => write!(
                f,
                "unknown model profile '{profile}'; expected 'fast' or 'default'"
            ),
            Self::EmptyModel => write!(f, "'model' must not be empty"),
        }
    }
}

impl std::error::Error for InvocationModelRequestError {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_absent_selector_uses_the_provider_default_path() {
        let request = InvocationModelRequest::from_context(&json!({
            "task": "search-answer",
            "prompt": "packet"
        }))
        .unwrap();
        assert!(request.is_unspecified());
        assert_eq!(serde_json::to_value(request).unwrap(), json!({}));
    }

    #[test]
    fn parses_each_portable_profile_from_a_full_run_task_context() {
        for (raw, expected) in [
            ("fast", ModelProfile::Fast),
            ("default", ModelProfile::Default),
        ] {
            let request = InvocationModelRequest::from_context(&json!({
                "task": "search-plan",
                "model_profile": raw
            }))
            .unwrap();
            assert_eq!(request.model_profile(), Some(expected));
            assert_eq!(request.model(), None);
        }
    }

    #[test]
    fn exact_model_is_trimmed_and_round_trips() {
        let request = InvocationModelRequest::from_context(&json!({
            "task": "search-answer",
            "model": "  provider-model-1  "
        }))
        .unwrap();
        assert_eq!(request.model(), Some("provider-model-1"));
        assert_eq!(
            serde_json::from_value::<InvocationModelRequest>(
                serde_json::to_value(&request).unwrap()
            )
            .unwrap(),
            request
        );
    }

    #[test]
    fn rejects_two_selectors_instead_of_silently_picking_one() {
        let error = InvocationModelRequest::from_context(&json!({
            "model_profile": "fast",
            "model": "provider-model-1"
        }))
        .unwrap_err();
        assert_eq!(error, InvocationModelRequestError::MutuallyExclusive);
    }

    #[test]
    fn rejects_unknown_profiles_empty_models_and_wrong_types() {
        assert_eq!(
            InvocationModelRequest::from_context(&json!({ "model_profile": "cheap" })).unwrap_err(),
            InvocationModelRequestError::UnknownProfile("cheap".into())
        );
        assert_eq!(
            InvocationModelRequest::from_context(&json!({ "model": "   " })).unwrap_err(),
            InvocationModelRequestError::EmptyModel
        );
        assert_eq!(
            InvocationModelRequest::from_context(&json!({ "model": 7 })).unwrap_err(),
            InvocationModelRequestError::MustBeString("model")
        );
        assert_eq!(
            InvocationModelRequest::from_context(&json!([])).unwrap_err(),
            InvocationModelRequestError::ContextMustBeObject
        );
    }

    #[test]
    fn standalone_wire_object_rejects_unowned_fields() {
        let error = serde_json::from_value::<InvocationModelRequest>(json!({
            "model_profile": "fast",
            "task": "search-plan"
        }))
        .unwrap_err();
        assert!(error.to_string().contains("unknown field `task`"));
    }
}
