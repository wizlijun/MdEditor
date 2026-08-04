//! OKF v0.2 §7 actor 支持:本机的人类身份。
//!
//! `verified: [{ by: human:<id>, at }]` 需要一个稳定的人类 id。vault 是 git 仓库,
//! 提交你判断的那个身份就是最自然的来源:优先 `git config user.email` 的本地部分,
//! 其次 `user.name`,再次系统用户名。前端只拿到已经算好的 id(纯函数在此可测)。

use std::path::Path;
use std::process::Command;

/// 从 git 身份与系统用户名推出人类 id。与前端 `src/lib/okf/actor.ts` 的
/// `humanActorId` 同规则(CJK 原样保留,不做音译)。
pub fn human_id_from(name: &str, email: &str, os_user: &str) -> String {
    let local = email.split('@').next().unwrap_or("").trim();
    if !local.is_empty() {
        return local.to_string();
    }
    let name = slug(name);
    if !name.is_empty() {
        return name;
    }
    let os = slug(os_user);
    if !os.is_empty() {
        return os;
    }
    "local".to_string()
}

fn slug(v: &str) -> String {
    v.split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
}

/// `git config --get <key>`,在 `dir` 下执行(未设仓库级时 git 自动回退到全局)。
fn git_config(dir: &Path, key: &str) -> String {
    Command::new("git")
        .args(["config", "--get", key])
        .current_dir(dir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// 本机人类身份 id。`vault_path` 为空或不存在时只用系统用户名。
#[tauri::command]
pub fn notemd_okf_human_id(vault_path: Option<String>) -> String {
    let os_user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
    let dir = vault_path.as_deref().map(Path::new).filter(|p| p.is_dir());
    match dir {
        Some(d) => human_id_from(&git_config(d, "user.name"), &git_config(d, "user.email"), &os_user),
        None => human_id_from("", "", &os_user),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_the_git_email_local_part() {
        assert_eq!(human_id_from("Bruce Li", "bruce@runningbruce.com", "brucel"), "bruce");
    }

    #[test]
    fn falls_back_to_the_git_name_then_the_os_user() {
        assert_eq!(human_id_from("Bruce Li", "", "brucel"), "bruce-li");
        assert_eq!(human_id_from("", "", "brucel"), "brucel");
    }

    #[test]
    fn never_yields_an_empty_id() {
        assert_eq!(human_id_from("", "", ""), "local");
        assert_eq!(human_id_from("   ", "  ", "  "), "local");
    }

    #[test]
    fn keeps_cjk_names_verbatim() {
        assert_eq!(human_id_from("李雷", "", ""), "李雷");
    }

    #[test]
    fn matches_the_frontend_rule_for_a_dotted_email() {
        assert_eq!(human_id_from("X", "first.last@corp.com", "x"), "first.last");
    }
}
