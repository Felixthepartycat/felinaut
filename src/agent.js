// Page side of the agent control server. Felifox injects it only with FELIFOX_AGENT=1.
// It gives no native access: each function only reads or changes this document.
(() => {
  const SELECTOR =
    "a[href],button,input:not([type=hidden]),select,textarea,summary,[onclick]," +
    "[contenteditable]:not([contenteditable=false]),[tabindex]:not([tabindex='-1'])," +
    "[role=button],[role=link],[role=checkbox],[role=radio],[role=tab],[role=menuitem]," +
    "[role=option],[role=switch],[role=combobox],[role=textbox],[role=searchbox]";
  const BUTTON_TYPES = ["button", "submit", "reset", "image"];
  // The index in this list is the number in a reference such as @e12.
  let refs = [];

  // ponytail: main document and open shadow roots only. Add frames if agents need them.
  const collect = (root, out) => {
    for (const el of root.querySelectorAll("*")) {
      if (el.matches(SELECTOR)) out.push(el);
      if (el.shadowRoot) collect(el.shadowRoot, out);
    }
    return out;
  };
  const visible = (el) => {
    const rect = el.getBoundingClientRect();
    return (
      rect.width > 0 &&
      rect.height > 0 &&
      !el.disabled &&
      (el.checkVisibility?.({ visibilityProperty: true }) ?? true)
    );
  };
  const text = (value) =>
    String(value ?? "")
      .replace(/\s+/g, " ")
      .trim()
      .slice(0, 80);
  const isField = (el) =>
    ["input", "select", "textarea"].includes(el.localName) ||
    el.isContentEditable;
  const role = (el) => {
    const explicit = el.getAttribute("role");
    if (explicit) return explicit;
    const tag = el.localName;
    if (tag === "a") return "link";
    if (tag === "summary") return "button";
    if (tag === "select") return "combobox";
    if (tag === "input") {
      if (el.type === "checkbox" || el.type === "radio") return el.type;
      return BUTTON_TYPES.includes(el.type) ? "button" : "textbox";
    }
    return tag === "textarea" || el.isContentEditable ? "textbox" : tag;
  };
  const name = (el) =>
    text(el.getAttribute("aria-label")) ||
    text(el.labels?.[0]?.innerText) ||
    text(el.placeholder) ||
    text(el.alt) ||
    // The text of a field is its value, not its name.
    (isField(el) ? "" : text(el.innerText)) ||
    (BUTTON_TYPES.includes(el.type) ? text(el.value) : "") ||
    text(el.title) ||
    text(el.querySelector("img[alt]")?.alt) ||
    text(el.getAttribute("name"));
  const value = (el) => {
    if (el.localName === "input") {
      if (el.type === "checkbox" || el.type === "radio") return el.checked;
      // A password must not go into the context of an agent.
      if (el.type === "password" || BUTTON_TYPES.includes(el.type)) return;
      return text(el.value);
    }
    if (el.localName === "select") return text(el.selectedOptions[0]?.text);
    if (el.localName === "textarea") return text(el.value);
    if (el.isContentEditable) return text(el.innerText);
  };

  const find = (ref) => {
    const el = refs[Number(String(ref).replace(/^@?e/, ""))];
    if (!el || !el.isConnected)
      throw new Error(`Unknown or stale reference ${ref}. Take a new snapshot.`);
    return el;
  };
  /// Scrolls the element into view. Gives its centre in viewport coordinates.
  const point = (el) => {
    el.scrollIntoView({ block: "center", inline: "center", behavior: "instant" });
    const rect = el.getBoundingClientRect();
    return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
  };

  const api = {
    snapshot() {
      refs = collect(document, []).filter(visible);
      return {
        url: location.href,
        title: document.title,
        elements: refs.map((el, index) => ({
          ref: `@e${index}`,
          role: role(el),
          name: name(el),
          value: value(el),
        })),
      };
    },
    click(ref) {
      const el = find(ref);
      const { x, y } = point(el);
      const init = {
        bubbles: true,
        cancelable: true,
        composed: true,
        view: window,
        clientX: x,
        clientY: y,
      };
      el.dispatchEvent(new PointerEvent("pointerdown", init));
      el.dispatchEvent(new MouseEvent("mousedown", init));
      el.focus();
      el.dispatchEvent(new PointerEvent("pointerup", init));
      el.dispatchEvent(new MouseEvent("mouseup", init));
      el.click();
    },
    /// Replaces the value of a field. For a select, `value` is an option value or label.
    type(ref, value) {
      const el = find(ref);
      point(el);
      el.focus();
      if (el.isContentEditable) {
        getSelection().selectAllChildren(el);
        document.execCommand("insertText", false, value);
        return;
      }
      if (el.localName === "select") {
        const option = [...el.options].find(
          (option) => option.value === value || option.text.trim() === value,
        );
        if (!option) throw new Error(`${ref} has no option "${value}".`);
        el.value = option.value;
      } else if (el.localName === "input" || el.localName === "textarea") {
        // React replaces the `value` property of the element. The prototype setter
        // changes the real value, thus React sees the change in the input event.
        const prototype =
          el.localName === "input"
            ? HTMLInputElement.prototype
            : HTMLTextAreaElement.prototype;
        Object.getOwnPropertyDescriptor(prototype, "value").set.call(el, value);
      } else {
        throw new Error(`${ref} is not a field.`);
      }
      el.dispatchEvent(
        new InputEvent("input", {
          bubbles: true,
          composed: true,
          inputType: "insertText",
          data: value,
        }),
      );
      el.dispatchEvent(new Event("change", { bubbles: true }));
    },
    press(key) {
      const el = document.activeElement || document.body;
      const init = { key, bubbles: true, cancelable: true, composed: true };
      const proceed = el.dispatchEvent(new KeyboardEvent("keydown", init));
      el.dispatchEvent(new KeyboardEvent("keyup", init));
      // An untrusted key event has no default action. Enter in a form field is the
      // one that agents use most. Other keys need the native mode.
      if (proceed && key === "Enter" && el.localName === "input" && el.form)
        el.form.requestSubmit();
    },
    /// For a native click: where the element is.
    point: (ref) => point(find(ref)),
    /// For native text: focus the field and select its contents, so that the text replaces them.
    focus(ref) {
      const el = find(ref);
      point(el);
      el.focus();
      if (el.isContentEditable) getSelection().selectAllChildren(el);
      else el.select?.();
    },
    eval: (thunk) => thunk(),
  };

  // The native side gets one JSON string for each call, also when the call throws.
  window.__felifox = (method, ...args) => {
    try {
      return JSON.stringify({ ok: true, value: api[method](...args) });
    } catch (error) {
      return JSON.stringify({ ok: false, error: String(error?.message ?? error) });
    }
  };
})();
