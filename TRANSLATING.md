# Translating OxyPad

OxyPad's interface text lives in one file per language in
[`crates/app/locales/`](crates/app/locales):

| File | Language |
| ---- | -------- |
| `en.yml` | English, the reference every translation follows |
| `pt-BR.yml` | Portuguese (Brazil) |

## Improving a translation

Edit the language's file and send a pull request. Each line is a key, which
stays as it is, and the text, which you translate:

```yaml
dialog:
  save_changes: Save changes to “%{name}”?
```

```yaml
dialog:
  save_changes: Salvar as alterações em “%{name}”?
```

- Keep placeholders such as `%{name}` or `%{count}` exactly as they are; OxyPad
  replaces them with a file name, a number and so on. Move them wherever the
  sentence needs them.
- Quote the text (`"…"`) when it starts with `%` or contains `: `.
- Keep the `…` on menu items that open a window or ask for more input.
- Proper names stay in English: OxyPad, Tail (the log mode), file formats, keys.

## Adding a language

1. Copy `en.yml` to a file named after the language's code: `es.yml`,
   `fr.yml`, `pt-PT.yml`, `zh-CN.yml`…
2. Translate every text. Don't add, remove or rename keys; a test checks that
   each file has exactly the keys of `en.yml`.
3. Add the language to `LANGUAGES` in [`crates/app/src/i18n.rs`](crates/app/src/i18n.rs),
   with its name written in that language (`("es", "Español")`), and to
   `CFBundleLocalizations` in [`scripts/bundle-macos.sh`](scripts/bundle-macos.sh).
4. Run `cargo test -p oxypad`, then `cargo run -p oxypad` and pick the language
   in Settings › General › Language (it switches right away).

OxyPad uses the system's preferred language when it has a translation for
it (any region: Portuguese from Portugal gets `pt-BR` until there is a
`pt-PT`), and English otherwise. Choosing a language in Settings overrides that.

The `gpui_component` section at the end of each file covers texts drawn by
the [gpui-kit](https://github.com/longbridge/gpui-kit) components OxyPad is
built with, such as Cut, Copy and Paste in the editor's context menu. OxyPad
looks them up in its own files first, so they don't depend on gpui-kit having
the language; their keys are gpui-kit's (`crates/component/locales/ui.yml`
in its repository) and must stay as they are.
