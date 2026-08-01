use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug)]
pub struct Request {
    pub command: String,
    pub context: Context,
}

#[derive(Deserialize, Debug)]
pub struct Context {
    pub tab: Tab,
    pub rendered_html: String,
    pub output_path: String,
}

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)] // path/filename are part of the protocol; not all are read today
pub struct Tab {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub title: String,
}

#[derive(Serialize, Debug)]
pub struct Response {
    pub success: bool,
    pub actions: Vec<Action>,
}

#[derive(Serialize, Debug)]
#[serde(tag = "type")]
pub enum Action {
    #[serde(rename = "toast")]
    Toast {
        level: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    #[serde(rename = "cli.result")]
    CliResult {
        data: serde_json::Map<String, serde_json::Value>,
    },
}

impl Response {
    pub fn ok(actions: Vec<Action>) -> Self {
        Self { success: true, actions }
    }
    pub fn fail(actions: Vec<Action>) -> Self {
        Self { success: false, actions }
    }
}

pub fn toast_success(message: String) -> Action {
    Action::Toast { level: "success".into(), message, detail: None }
}

pub fn toast_error(message: String, detail: Option<String>) -> Action {
    Action::Toast { level: "error".into(), message, detail }
}
