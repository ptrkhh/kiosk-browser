// Bundled document-start keyboard. It deliberately uses only DOM construction and
// direct style/property writes: deployed pages cannot block it with a page CSP and
// it has no external asset, font, inline handler, or data URI dependency.
(function () {
  var active = null;
  var panel = null;
  var shifted = false;
  var symbols = false;

  function isTextEntry(el) {
    if (!el || el.nodeType !== 1) return false;
    if (el.tagName === 'TEXTAREA' || el.isContentEditable) return true;
    if (el.tagName !== 'INPUT') return false;
    var type = (el.getAttribute('type') || 'text').toLowerCase();
    return ['text', 'search', 'url', 'tel', 'email', 'password', 'number'].indexOf(type) !== -1;
  }

  function dispatchInput(el, data) {
    try {
      el.dispatchEvent(new InputEvent('input', {
        bubbles: true,
        inputType: data ? 'insertText' : 'deleteContentBackward',
        data: data || null
      }));
    } catch (_) {
      el.dispatchEvent(new Event('input', { bubbles: true }));
    }
  }

  function insertText(el, text) {
    if (el.isContentEditable) {
      document.execCommand('insertText', false, text);
      dispatchInput(el, text);
      return;
    }
    var value = String(el.value || '');
    var start = typeof el.selectionStart === 'number' ? el.selectionStart : value.length;
    var end = typeof el.selectionEnd === 'number' ? el.selectionEnd : start;
    el.value = value.slice(0, start) + text + value.slice(end);
    var caret = start + text.length;
    try { el.setSelectionRange(caret, caret); } catch (_) {}
    dispatchInput(el, text);
  }

  function backspace(el) {
    if (el.isContentEditable) {
      document.execCommand('delete', false, null);
      dispatchInput(el, null);
      return;
    }
    var value = String(el.value || '');
    var start = typeof el.selectionStart === 'number' ? el.selectionStart : value.length;
    var end = typeof el.selectionEnd === 'number' ? el.selectionEnd : start;
    if (start === end && start > 0) start -= 1;
    if (start === end) return;
    el.value = value.slice(0, start) + value.slice(end);
    try { el.setSelectionRange(start, start); } catch (_) {}
    dispatchInput(el, null);
  }

  function hide() {
    if (!panel) return;
    if (active && !active.isContentEditable && 'dispatchEvent' in active) {
      try { active.dispatchEvent(new Event('change', { bubbles: true })); } catch (_) {}
    }
    panel.remove();
    panel = null;
    active = null;
    shifted = false;
    symbols = false;
  }

  function keyLabel(key) {
    if (key === 'backspace') return '⌫';
    if (key === 'shift') return shifted ? '⇧' : 'shift';
    if (key === 'symbols') return symbols ? 'abc' : '123';
    if (key === 'space') return 'space';
    return shifted && !symbols ? key.toUpperCase() : key;
  }

  function makeKey(key) {
    var button = document.createElement('button');
    button.type = 'button';
    button.textContent = keyLabel(key);
    button.setAttribute('aria-label', key);
    button.style.cssText = 'min-width:38px;min-height:34px;margin:2px;padding:3px 7px;border:1px solid #777;border-radius:4px;background:#222;color:#fff;font:14px sans-serif;user-select:none;-webkit-user-select:none;touch-action:manipulation;';
    button.addEventListener('pointerdown', function (event) {
      event.preventDefault();
      if (!active) return;
      if (key === 'shift') {
        shifted = !shifted;
      } else if (key === 'symbols') {
        symbols = !symbols;
      } else if (key === 'backspace') {
        backspace(active);
      } else if (key === 'space') {
        insertText(active, ' ');
      } else {
        var value = shifted && !symbols ? key.toUpperCase() : key;
        insertText(active, value);
        if (shifted) shifted = false;
      }
      rebuild();
    }, true);
    return button;
  }

  function rebuild() {
    if (!panel) return;
    while (panel.firstChild) panel.removeChild(panel.firstChild);
    var rows = symbols
      ? ['1234567890', '-_=+[]{};:,.!?/@#$%&*()'.replace(/ /g, '')]
      : ['qwertyuiop', 'asdfghjkl', 'zxcvbnm'];
    rows.forEach(function (row) {
      var line = document.createElement('div');
      line.style.cssText = 'display:flex;justify-content:center;flex-wrap:wrap;';
      for (var i = 0; i < row.length; i += 1) line.appendChild(makeKey(row[i]));
      panel.appendChild(line);
    });
    var controls = document.createElement('div');
    controls.style.cssText = 'display:flex;justify-content:center;flex-wrap:wrap;';
    controls.appendChild(makeKey('shift'));
    controls.appendChild(makeKey('space'));
    controls.appendChild(makeKey('backspace'));
    controls.appendChild(makeKey('symbols'));
    panel.appendChild(controls);
  }

  function show(el) {
    if (!isTextEntry(el)) return;
    active = el;
    if (!panel) {
      panel = document.createElement('div');
      panel.id = 'kiosk-osk';
      panel.setAttribute('role', 'group');
      panel.setAttribute('aria-label', 'On-screen keyboard');
      panel.style.cssText = 'position:fixed;z-index:2147483647;left:0;right:0;bottom:0;padding:4px;background:rgba(0,0,0,.92);color:#fff;user-select:none;-webkit-user-select:none;';
      document.body.appendChild(panel);
    }
    rebuild();
  }

  document.addEventListener('focusin', function (event) { show(event.target); }, true);
  document.addEventListener('focusout', function () { hide(); }, true);
}());
