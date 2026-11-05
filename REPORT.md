# Tabview: `display:none` on panels after the first (Wikidot layout)

## Evidence (live, anonymous)
2026-11-05, `POST https://sandbox-for-codex.wikidot.com/ajax-module-connector.php`, `moduleName=edit/PagePreviewModule`
(anonymous `wikidot_token7` cookie/param only, no credentials). Source:

    [[tabview]] [[tab One]] first [[/tab]] [[tab Two]] second [[/tab]] [[tab Three]] third [[/tab]] [[/tabview]]

(one tag per line). Live panel bytes:

    <div id="wiki-tab-0-0">...  <div id="wiki-tab-0-1" style="display:none">...  <div id="wiki-tab-0-2" style="display:none">...

The first panel has no style; every later panel has ` style="display:none"` directly after the `id`.

## Fix
`src/render/html/element/tabs.rs` (`render_wikidot_tabview`): panels with `tab_index > 0` now emit `style="display:none"`.
The Wikijump layout (`wj-tabs`, `hidden`) is unchanged. The unit test `wikidot_tabview_uses_yui_dom_and_separates_labels`
previously asserted `!contains("display:")`; it now asserts the exact second-panel bytes.

## Fixtures
Each input below was re-posted to live PagePreview and the second panel came back as `<div id="wiki-tab-0-1" style="display:none">`.
Only `wikidot.html` was changed (hand-patched; `FTML_UPDATE_TESTS` was used only for the new fixture, scoped with `FTML_ONLY_TESTS`):

- `test/tabview/blockquote`
- `test/tabview/blockquote-nested`
- `test/tabview/blockquote-text-around`
- `test/tabview/iftags-seam-abutting`
- `test/tabview/title-footnote-numbering`
- `test/tabview/title-typography-raw`

New: `test/tabview/multi-tab-display-none` (3 tabs; `input.ftml`, `tree.json`, `wikidot.html`, `output.html`).
`output.html` files of existing fixtures are untouched (Wikijump layout).

## Validation
`cargo fmt --check` ok; `cargo test` fully green (1160 lib + 582 integration). `RUSTFLAGS='-D warnings' cargo clippy --tests --no-deps`
fails with ~35 errors that are identical on the untouched base tree and none are in `tabs.rs`.
