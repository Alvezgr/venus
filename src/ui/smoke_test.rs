//! End-to-end UI test: drives the real frame loop headlessly with
//! egui_kittest (clicks, keys, typing) against a local HTTP server.
//! Copy a block from here to reproduce a UI bug before fixing it.

use super::app::{Dialog, VenusApp};
use eframe::egui::{self, accesskit, Key, Modifiers};
use egui_kittest::{
    kittest::{NodeT, Queryable},
    Harness,
};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

/// Replies 200 JSON whose body is the raw request it received.
fn echo_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = [0; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let body = serde_json::json!({ "echo": request, "token":"response-token", "property":{"id":42}, "items":[{"id":7}] }).to_string();
            // `/status/404` replies 404, so failing statuses can be exercised
            let code = request
                .split_whitespace()
                .nth(1)
                .and_then(|p| p.strip_prefix("/status/"))
                .and_then(|c| c.split('?').next())
                .unwrap_or("200");
            let status = format!("{code} Test");
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nSet-Cookie: s=1; Path=/\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    format!("http://{addr}")
}

fn wait_for_response(harness: &mut Harness<'_, VenusApp>) {
    for _ in 0..200 {
        harness.step();
        if harness.state().in_flight.is_none() {
            harness.run_steps(2);
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("request did not finish");
}

#[test]
fn open_send_edit_and_save() {
    let dir = tempfile::TempDir::new().unwrap();
    // SAFETY: the only test in the crate that touches env vars / storage on disk
    unsafe { std::env::set_var("VENUS_HOME", dir.path().join("home")) };
    let ws = dir.path().join("ws");
    std::fs::create_dir_all(ws.join("users")).unwrap();
    let server = echo_server();
    std::fs::write(
        ws.join(".env.dev"),
        format!("BASE={server}\nTOKEN=secret-token\n"),
    )
    .unwrap();
    // a request two folders deep that the test never expands: the sidebar
    // filter has to reach it
    std::fs::create_dir_all(ws.join("billing/invoices")).unwrap();
    std::fs::write(
        ws.join("billing/invoices/get-invoice.json"),
        r#"{"method":"GET","url":"{{BASE}}/invoices/42"}"#,
    )
    .unwrap();
    let file = ws.join("users/list-users.json");
    std::fs::write(
        &file,
        r#"{"method":"GET","url":"{{BASE}}/users?q=caf%C3%A9","headers":{"Authorization":"Bearer {{TOKEN}}"},"insomnia_id":"req_example"}"#,
    )
    .unwrap();

    let ctx = egui::Context::default();
    let mut app = VenusApp::new(&ctx);
    app.open_workspace(ws.clone(), None);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_state(|ctx, app: &mut VenusApp| app.frame(ctx), app);
    harness.run_steps(2);
    assert_eq!(harness.state().env_name(), Some(".env.dev"));

    // The environment menu's pencil opens the shared key/value editor.
    harness.get_by_role_and_label(accesskit::Role::Button, ".env.dev").click();
    harness.run_steps(2);
    harness.get_by_role_and_label(accesskit::Role::Button, "Edit .env.dev").click();
    harness.run_steps(2);
    harness.get_by_label("Bulk edit").click();
    harness.run_steps(2);
    harness.get_by_role(accesskit::Role::MultilineTextInput).click();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_role(accesskit::Role::MultilineTextInput).type_text(
        &format!("BASE={server}\nTOKEN=secret-token\nEDITOR=from-menu"),
    );
    harness.run_steps(2);
    harness.get_by_role_and_label(accesskit::Role::Button, "Save environment").click();
    harness.run_steps(2);
    assert_eq!(harness.state().env_vars["TOKEN"], "secret-token");
    assert_eq!(harness.state().env_vars["EDITOR"], "from-menu");
    harness.get_by_role_and_label(accesskit::Role::Button, "Send");

    harness
        .get_by_role_and_label(accesskit::Role::Button, "Sync")
        .click();
    harness.run_steps(2);
    harness.get_by_role_and_label(accesskit::Role::Button, "Open synced workspace…");
    assert!(harness
        .get_by_role_and_label(accesskit::Role::Button, "Pull from master")
        .accesskit_node()
        .is_disabled());
    assert!(harness
        .get_by_role_and_label(accesskit::Role::Button, "Push to master")
        .accesskit_node()
        .is_disabled());
    harness.key_press(Key::Escape);
    harness.run_steps(2);

    // Every control Venus draws by hand has to name itself in the
    // accessibility tree, or it reaches a screen reader — and the keyboard
    // Tab order — as an anonymous box. The text *inside* a row is not
    // enough: it is the row itself that takes focus and the click.
    for (role, name) in [
        (accesskit::Role::Button, "users"),   // a sidebar tree row
        (accesskit::Role::ComboBox, "GET"),   // the method picker
        (accesskit::Role::Link, "Shortcuts"), // a status-bar link
        (accesskit::Role::Button, "Send"),
    ] {
        harness.get_by_role_and_label(role, name);
    }

    // Panel widths must not drift. A widget that ends up wider than the space
    // it was handed feeds straight back into the panel width, and the panel
    // grows a little every frame — and springs back after you resize it.
    let panel_width = |harness: &Harness<'_, VenusApp>, name: &str| {
        egui::containers::panel::PanelState::load(&harness.ctx, egui::Id::new(name))
            .map(|state| state.rect.width())
            .expect("panel is laid out")
    };
    harness.state_mut().show_history = true;
    harness.run_steps(4);
    let settled = [
        panel_width(&harness, "sidebar"),
        panel_width(&harness, "response_panel"),
    ];
    harness.run_steps(8);
    assert_eq!(
        [
            panel_width(&harness, "sidebar"),
            panel_width(&harness, "response_panel")
        ],
        settled,
        "a panel width drifted while nothing was interacting with it"
    );
    harness.state_mut().show_history = false;
    harness.run_steps(2);

    // expand the folder, open the request
    harness
        .get_by_role_and_label(accesskit::Role::Button, "users")
        .click();
    harness.run_steps(2);
    harness
        .get_by_role_and_label(accesskit::Role::Button, "list-users")
        .click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().current_file.as_deref(),
        Some(file.as_path())
    );
    assert_eq!(harness.state().query_params[0].value, "café");

    // ⌘Enter sends; env vars are substituted into URL and headers
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    wait_for_response(&mut harness);
    let response = harness.state().response.clone().expect("response shown");
    assert_eq!(response.status, 200);
    assert!(
        response.body.contains("GET /users?q=caf%C3%A9"),
        "{}",
        response.body
    );
    assert!(
        response.body.contains("authorization: Bearer secret-token"),
        "{}",
        response.body
    );
    assert_eq!(response.cookies, vec!["s=1; Path=/"]);
    assert_eq!(harness.state().history.len(), 1);
    // saved requests don't go to Recent
    assert!(harness.state().recent.is_empty());

    // a new env file that sorts first must not shift the selection, and
    // edits to the selected file take effect (file watcher -> refresh)
    std::fs::write(ws.join(".env.a-first"), "TOKEN=wrong\n").unwrap();
    let dev = std::fs::read_to_string(ws.join(".env.dev")).unwrap();
    std::fs::write(ws.join(".env.dev"), dev.replace("secret-token", "rotated")).unwrap();
    for _ in 0..300 {
        harness.step();
        if harness.state().env_files.len() == 2
            && harness.state().env_vars.get("TOKEN").map(String::as_str) == Some("rotated")
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(harness.state().env_name(), Some(".env.dev"));
    assert_eq!(harness.state().env_vars["TOKEN"], "rotated");

    // typing "?" in the URL bar must not open the shortcuts dialog
    harness.key_press_modifiers(Modifiers::COMMAND, Key::L); // focus URL bar
    harness.run_steps(2);
    harness.event(egui::Event::Key {
        key: Key::End,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    harness.event(egui::Event::Text("&x?".into()));
    harness.run_steps(2);
    assert!(harness.state().dialog.is_none());
    assert!(harness.state().url.ends_with("&x?"));

    // ⌘S writes the file (autosave may already have, after 5s of frame time)
    harness.key_press_modifiers(Modifiers::COMMAND, Key::S);
    harness.run_steps(2);
    assert!(!harness.state().has_unsaved_changes());
    let saved =
        crate::model::RequestFile::from_json(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(saved.insomnia_id.as_deref(), Some("req_example"));
    assert!(std::fs::read_to_string(&file).unwrap().contains("&x?"));

    // the filter reaches requests inside folders that were never expanded
    harness.state_mut().search = "invoice".into();
    harness.run_steps(3);
    assert!(
        harness
            .query_by_role_and_label(accesskit::Role::Button, "get-invoice")
            .is_some(),
        "the sidebar filter must search the whole workspace, not just open folders"
    );
    harness.state_mut().search.clear();
    harness.run_steps(2);

    // history records the request that was sent, even if the form changes
    // while it is in flight
    let sent_url = harness.state().url.clone();
    harness.state_mut().send_request();
    harness.state_mut().url = "https://edited-after-send.example.com".into();
    wait_for_response(&mut harness);
    let recorded = harness.state().history.last().expect("history entry");
    assert_eq!(
        recorded.url, sent_url,
        "history must pair the response with the request that produced it"
    );
    harness.state_mut().url = sent_url;
    harness.run_steps(2);

    // ⌘N starts an unsaved request; "?" outside a text field opens help
    harness.key_press_modifiers(Modifiers::COMMAND, Key::N);
    harness.run_steps(2);
    assert!(harness.state().current_file.is_none());
    harness.state_mut().dialog = None;
    harness.remove_cursor();
    harness.event(egui::Event::Key {
        key: Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(1);
    harness.key_press_modifiers(Modifiers::SHIFT, Key::Questionmark);
    harness.run_steps(2);
    assert!(matches!(harness.state().dialog, Some(Dialog::Shortcuts)));

    // The response pane puts the status at the top, so a toast repeating it
    // is noise on every send — but only while the pane is actually visible.
    harness.state_mut().url = format!("{server}/status/404");
    harness.state_mut().send_request();
    wait_for_response(&mut harness);
    assert_eq!(
        harness.state().response.as_ref().map(|r| r.status),
        Some(404),
        "the failing status has to reach the pane, which is now the only signal"
    );
    assert!(
        harness.state().toast.is_none(),
        "the status badge is on screen; a toast repeating it is noise"
    );

    // ...with history covering the pane, the toast is the only signal left
    harness.state_mut().show_history = true;
    harness.state_mut().send_request();
    wait_for_response(&mut harness);
    assert!(
        harness.state().toast.as_ref().is_some_and(|t| t.is_error),
        "a failing status behind the history panel must still be announced, and not as a success"
    );
    harness.state_mut().show_history = false;
    harness.run_steps(2);

    // a response that lands after the user opened another request belongs to
    // neither of them on screen, so it goes to history only
    harness.state_mut().open_file(&file);
    harness.run_steps(2);
    harness.state_mut().send_request();
    harness.run_steps(1);
    harness
        .state_mut()
        .open_file(&ws.join("billing/invoices/get-invoice.json"));
    wait_for_response(&mut harness);
    assert!(
        harness.state().response.is_none(),
        "a response was attributed to a request that did not make it"
    );
    assert!(
        harness
            .state()
            .history
            .last()
            .is_some_and(|h| h.url.contains("users")),
        "the sent request was lost from history"
    );

    // Pre/post variable rules execute from the sent request, without writing environment files.
    harness.state_mut().dialog = None;
    harness.state_mut().open_file(&file);
    harness.run_steps(2);
    harness
        .get_by_role_and_label(accesskit::Role::Button, "Variables")
        .click();
    harness.run_steps(2);
    harness.get_by_label("Before sending: literal values or {{existing_variables}}");
    harness.get_by_label("After a successful response: JSON field paths");
    harness.state_mut().pre_request_variables = "PAGE=3\nTOKEN=pre-token".into();
    harness.state_mut().post_request_variables =
        "TOKEN=token\nPROPERTY=property.id\nFIRST=items.0.id".into();
    harness.state_mut().url = format!("{server}/capture?page={{{{PAGE}}}}");
    harness.state_mut().headers_text = "Authorization: Bearer {{TOKEN}}".into();
    harness.state_mut().send_request();
    harness.state_mut().post_request_variables = "TOKEN=missing".into();
    let first_env = harness
        .state()
        .env_files
        .iter()
        .position(|file| file == ".env.a-first")
        .unwrap();
    let dev_env = harness
        .state()
        .env_files
        .iter()
        .position(|file| file == ".env.dev")
        .unwrap();
    harness.state_mut().select_env(Some(first_env));
    wait_for_response(&mut harness);
    assert_eq!(harness.state().env_vars["TOKEN"], "wrong");
    harness.state_mut().select_env(Some(dev_env));
    let body = &harness.state().response.as_ref().unwrap().body;
    assert!(body.contains("page=3") && body.contains("Bearer pre-token"));
    assert_eq!(harness.state().env_vars["TOKEN"], "response-token");
    assert_eq!(harness.state().env_vars["PROPERTY"], "42");
    assert_eq!(harness.state().env_vars["FIRST"], "7");
    assert!(!std::fs::read_to_string(ws.join(".env.dev"))
        .unwrap()
        .contains("response-token"));

    harness
        .state_mut()
        .open_file(&ws.join("billing/invoices/get-invoice.json"));
    assert!(harness.state().post_request_variables.is_empty());
    assert_eq!(harness.state().env_vars["TOKEN"], "response-token");
    harness.state_mut().headers_text = "Authorization: Bearer {{TOKEN}}".into();
    harness.state_mut().send_request();
    wait_for_response(&mut harness);
    assert!(harness
        .state()
        .response
        .as_ref()
        .unwrap()
        .body
        .contains("Bearer response-token"));

    // Missing paths do not partially apply; failed HTTP responses do not capture tokens.
    harness.state_mut().pre_request_variables = "FIRST=keep\nTOKEN=keep".into();
    harness.state_mut().post_request_variables = "FIRST=items.0.id\nTOKEN=missing".into();
    harness.state_mut().send_request();
    wait_for_response(&mut harness);
    assert_eq!(harness.state().env_vars["FIRST"], "keep");
    assert_eq!(harness.state().env_vars["TOKEN"], "keep");
    assert!(harness.state().toast.as_ref().unwrap().is_error);
    harness.state_mut().post_request_variables = "TOKEN=token".into();
    harness.state_mut().url = format!("{server}/status/404");
    harness.state_mut().send_request();
    wait_for_response(&mut harness);
    assert_eq!(harness.state().env_vars["TOKEN"], "keep");

    let history_count = harness.state().history.len();
    harness.state_mut().pre_request_variables = "INVALID".into();
    harness.state_mut().send_request();
    assert!(harness.state().in_flight.is_none());
    assert_eq!(harness.state().history.len(), history_count);
    harness.state_mut().pre_request_variables = "TOKEN=should-not-apply".into();
    harness.state_mut().post_request_variables = "TOKEN=items[]".into();
    harness.state_mut().send_request();
    assert_eq!(harness.state().env_vars["TOKEN"], "keep");

    harness.state_mut().select_env(Some(first_env));
    assert_eq!(harness.state().env_vars["TOKEN"], "wrong");
    harness.state_mut().select_env(Some(dev_env));
    assert_eq!(harness.state().env_vars["TOKEN"], "keep");
    harness
        .state_mut()
        .clear_session_variables(Some(".env.dev".into()));
    assert_eq!(harness.state().env_vars["TOKEN"], "rotated");

    // Discard late captures when the user changes workspaces.
    harness.state_mut().pre_request_variables.clear();
    harness.state_mut().post_request_variables = "TOKEN=token".into();
    harness.state_mut().url = format!("{server}/capture");
    harness.state_mut().send_request();
    let other = dir.path().join("other-workspace");
    std::fs::create_dir(&other).unwrap();
    harness.state_mut().open_workspace(other, None);
    harness
        .state_mut()
        .open_workspace(ws.clone(), Some(".env.dev"));
    wait_for_response(&mut harness);
    assert_eq!(harness.state().env_vars["TOKEN"], "rotated");
    harness.state_mut().open_file(&file);
    harness.state_mut().pre_request_variables = "PAGE=3".into();
    harness.state_mut().post_request_variables = "TOKEN=token".into();

    // quitting and relaunching reopens the request that was open, instead of
    // orphaning it into an "Untitled" unsaved request
    harness.run_steps(2);
    eframe::App::on_exit(harness.state_mut(), None);
    let restarted = VenusApp::new(&egui::Context::default());
    assert_eq!(restarted.current_file.as_deref(), Some(file.as_path()));
    assert!(!restarted.has_unsaved_changes());
    assert_eq!(restarted.pre_request_variables, "PAGE=3");
    assert_eq!(restarted.post_request_variables, "TOKEN=token");
    assert_eq!(restarted.env_vars["TOKEN"], "rotated");
}
