use serde_json::Value;

#[test]
fn ios_plist_registers_canvas_as_an_editable_document() {
    let plist = include_str!("../gen/apple/notemd_iOS/Info.plist");
    let canvas = plist
        .find("<string>canvas</string>")
        .expect("iOS CFBundleDocumentTypes must include the .canvas extension");
    let document_types = plist
        .find("<key>CFBundleDocumentTypes</key>")
        .expect("iOS Info.plist must declare CFBundleDocumentTypes");
    let entry_end = plist[canvas..]
        .find("</dict>")
        .map(|offset| canvas + offset)
        .expect("the iOS .canvas document type entry must be a complete dictionary");
    let canvas_entry = &plist[canvas..entry_end];

    assert!(document_types < canvas);
    assert!(canvas_entry.contains("<string>JSON Canvas</string>"));
    assert!(canvas_entry.contains("<string>Editor</string>"));
}

#[test]
fn global_shortcut_permissions_are_desktop_only() {
    let common: Value = serde_json::from_str(include_str!("../capabilities/default.json"))
        .expect("default capability must be valid JSON");
    let desktop: Value = serde_json::from_str(include_str!(
        "../capabilities/desktop-global-shortcuts.json"
    ))
    .expect("desktop shortcut capability must be valid JSON");

    let common_permissions = common["permissions"]
        .as_array()
        .expect("default permissions must be an array");
    assert!(common_permissions
        .iter()
        .all(|permission| !permission.as_str().is_some_and(|value| value.starts_with("global-shortcut:"))));

    assert_eq!(
        desktop["platforms"],
        serde_json::json!(["macOS", "windows", "linux"])
    );
    assert_eq!(
        desktop["permissions"],
        serde_json::json!([
            "global-shortcut:allow-register",
            "global-shortcut:allow-unregister",
            "global-shortcut:allow-is-registered"
        ])
    );
}

#[test]
fn canvas_clipboard_fallback_can_read_and_write_text() {
    let common: Value = serde_json::from_str(include_str!("../capabilities/default.json"))
        .expect("default capability must be valid JSON");
    let permissions = common["permissions"]
        .as_array()
        .expect("default permissions must be an array");

    for required in [
        "clipboard-manager:allow-read-text",
        "clipboard-manager:allow-write-text",
    ] {
        assert!(
            permissions.iter().any(|permission| permission == required),
            "Canvas clipboard fallback requires {required}"
        );
    }
}
