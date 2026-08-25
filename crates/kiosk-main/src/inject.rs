//! Document-start injection assembly (P1-D2b Task 6, spec §7 M1/M8).
//!
//! [`build_injection`] is pure and host-tested: it only assembles a `String` of JS,
//! it never touches WebView2. The one caller is `main.rs`'s `.setup()`, which passes
//! the result to `WebviewWindowBuilder::initialization_script` — Tauri/WebView2 runs
//! `initialization_script` content BEFORE any page script on every navigation
//! (`Document Created`, i.e. "document-start"), so this is the right place for
//! controls that must never race page JS (the `user-select`/drag/print overrides) or
//! that must survive every navigation without re-injection (the cursor-autohide
//! timer).
//!
//! `initialization_script` may be called only ONCE per webview (a second call
//! clobbers the first — see `nav_policy::csp_policy`'s doc comment) and is set at
//! BUILD time from the just-booted config. **A later config change to
//! `display.cursor_autohide_seconds` or `input.allow_text_selection` does NOT take
//! effect until the next process restart** (the nightly reload, spec §7) — there is
//! no live-reinjection path, by design (re-registering `initialization_script` after
//! the fact is not an operation WebView2 exposes).

/// Assembles the document-start control script.
///
/// * `cursor_autohide_seconds == 0` ⇒ the autohide feature is off; the timer block is
///   omitted entirely (not just parameterized to a no-op), so there is nothing for a
///   page to observe or disable.
/// * `select_text == true` ⇒ the operator has opted IN to normal text selection; the
///   `user-select: none` rule (and its `input`/`textarea` carve-out, which would be
///   meaningless without the blanket rule) is omitted. Drag/drop and print blocking
///   apply either way — those are not "text selection" controls.
pub fn build_injection(
    cursor_autohide_seconds: u64,
    select_text: bool,
    on_screen_keyboard: bool,
) -> String {
    let mut script = String::from("(function(){\n");

    if !select_text {
        script.push_str(
            "var style=document.createElement('style');\
             style.textContent='*{user-select:none;-webkit-user-select:none}\
             input,textarea{user-select:text;-webkit-user-select:text}';\
             document.documentElement.appendChild(style);\n",
        );
    }

    script.push_str(
        "document.addEventListener('dragstart',function(e){e.preventDefault()},true);\n\
         document.addEventListener('drop',function(e){e.preventDefault()},true);\n",
    );

    script.push_str(
        "Object.defineProperty(window,\"print\",{value:()=>{},writable:false,configurable:false});\n",
    );

    if cursor_autohide_seconds > 0 {
        let ms = cursor_autohide_seconds * 1000;
        script.push_str(&format!(
            "(function(){{var t;function hide(){{document.documentElement.style.cursor='none'}}\
             function show(){{document.documentElement.style.cursor='';clearTimeout(t);t=setTimeout(hide,{ms})}}\
             document.addEventListener('mousemove',show,true);show();}})();\n"
        ));
    }

    if on_screen_keyboard {
        script.push_str("try{");
        script.push_str(include_str!("keyboard.js"));
        script.push_str("}catch(e){}\n");
    }

    script.push_str("})();\n");
    script
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_call_locks_selection_blocks_drag_and_print() {
        let s = build_injection(5, false, false);
        assert!(s.contains("user-select:none"));
        assert!(s.contains("input,textarea{user-select:text"));
        assert!(s.contains("'dragstart'"));
        assert!(s.contains("e.preventDefault()"));
        assert!(s.contains("'drop'"));
        assert!(s.contains(
            "Object.defineProperty(window,\"print\",{value:()=>{},writable:false,configurable:false});"
        ));
        assert!(s.contains("5000"));
    }

    #[test]
    fn zero_seconds_omits_the_autohide_timer() {
        let s = build_injection(0, false, false);
        assert!(!s.contains("setTimeout"));
        assert!(!s.contains("mousemove"));
    }

    #[test]
    fn select_text_true_omits_the_user_select_none_rule() {
        let s = build_injection(5, true, false);
        assert!(!s.contains("user-select:none"));
        // Drag/drop + print still apply regardless of text-selection choice.
        assert!(s.contains("'dragstart'"));
        assert!(s.contains("Object.defineProperty(window,\"print\""));
    }

    #[test]
    fn the_keyboard_block_is_present_only_when_enabled() {
        let with = build_injection(5, false, true);
        let without = build_injection(5, false, false);
        assert!(with.contains("focusin"));
        assert!(!without.contains("focusin"));
    }

    #[test]
    fn the_disabled_arm_is_byte_identical_to_the_two_argument_era() {
        let s = build_injection(5, false, false);
        assert!(s.ends_with("})();\n"));
        assert!(!s.contains("kiosk-osk"));
    }

    #[test]
    fn the_keyboard_sets_its_own_user_select_none_either_way() {
        let s = build_injection(5, true, true);
        assert!(s.contains("user-select"));
    }
}
