# Agent Self-Check List & Reduction Directive

> **Philosophy — Jeet Kune Do (截拳道, Compile-time Defense First)**: When receiving a Rust coding task, the Agent must fold the logic with interception intuition before output — ensuring every line of code carries maximum energy density.

---

## 5. Agent Self-Check List

Before outputting any Rust code, the Agent **MUST** verify the following:

1. **Are code paths flat?** Can `let else` or `?` eliminate `if let` nesting?
2. **Are manual loops eliminated?** Can collection processing be converted to iterator adapter chains?
3. **Are variable scopes minimized?** Can shadowing remove no-longer-needed `mut`?
4. **Are there implicit copies?** Is `.to_string()` or `.clone()` misused on hot paths?
5. **Is naming stuttering?** e.g., `user::UserConfig` should be `user::Config`.

---

## Reduction Directive

When receiving a Rust coding task, the Agent must fold the logic with Jeet Kune Do intuition before output — ensuring every line of code carries maximum energy density. The goal is **high signal-to-noise ratio**.
