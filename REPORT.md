# Corpus fixture provenance audit (live Wikidot PagePreview)

Date: 2026-11-05. Base: `c1dc640` (main). Branch: `claude/audit-corpus-fixtures-9fa427`.

## STATUS

**Done**

- Built `scripts/audit_corpus_live.py`, the audit harness. It posts each fixture's `input.ftml` anonymously to `edit/PagePreviewModule` on `sandbox-for-codex.wikidot.com`, then parses both the live body and the fixture's `wikidot.html` as HTML5 fragments with html5lib. It masks generated ids (tabview, bibliography suffixes, `time_…`, hashes) and drops whitespace-only text that sits next to a block element, then compares the DOM token streams and the collapsed visible text. It also flags context-only differences (see below).
- Before the audit, confirmed that the corpus suite passes (`cargo test --lib test::ast::ast`). That means every `wikidot.html` is exactly what FTML outputs now, so a fixture/live mismatch is also an FTML/live mismatch.
- Audited **231 of 986** fixtures that have a `wikidot.html`. Every fixture in `color`, `code`, `list`, `span`, `misc` and `monospace` was checked, plus 23 of 69 in `strikethrough`. Results: **200 match**, **8 context-only** (PagePreview cannot show what the fixture runner renders), **23 mismatch**. Of the 23 mismatches, **6 are bug-encoded** and **17 are stale but harmless**.
- Settled one bug-encoded family with 20+ targeted live probes, and identified the rule live follows: line-start comment newline elision (below).

**Fixed (commit "Match live column-0 comment elision before list/heading/rule constructs")**

- B1 preprocessor half: removed the `exposed_line_start_construct` carve-out in `src/preproc/comments.rs`. Prose, heading, rule, separator and gallery heads after a column-0 comment now match live (`a\n[!--c--]* item` → `<p>a* item</p>`). Full `cargo test` passes.
- `heading/comment-line-start` was a second B1 victim; live gives `<h1>Head++ Sub</h1>`. Its `wikidot.html` is corrected.
- New live-confirmed fixture `misc/comment-line-start-exposed-constructs` pins the rule (live: `<p>a* item</p><p>b+ Head</p><p>c&#8212;&#8212;<br />d</p>`).

**Fixed by the follow-up lane (2026-11-05, "dsh-fixturefix")**

- B1 remainder: a list item now closes only at a marker that begins its
  physical source line (`ParseCondition::CurrentTokenAtSourceLineStart`), so
  `list/comment-line-start` pins the live one-item output. The general rule is
  pinned by the new `list/midline-marker-literal` fixture; every probe in the
  B1 table above now matches live.
- B4: duplicate bibliography labels are numbered by the distinct-label prefix,
  reproducing live's `1.`, `2.`, `2.` and duplicate `bibitem-2` id.
  `misc/bibliography-duplicate-definition` is corrected.
- B5: a run whose last content is a scored `[[span_ ...]]` is no longer wrapped
  in a paragraph, matching live. `span/fused-opener-scored-map-residual` is
  corrected.
- B6: the manufactured line break is no longer emitted after a fused list
  closer at EOF, matching live's `</ul>]`. `list/fused-close` is corrected.
- B2/B3 are untouched: the CSS highlighter port is another lane's
  (`d4l1-code`) follow-up, which had not merged when this lane finished.

**Partial**

- `strikethrough` was cut off after 23 of its 69 fixtures when the sweep was stopped.

**Remaining**

1. Audit `tabview` (57) and `table` (50), both priority families, the rest of `strikethrough` (46), and the other ~520 fixtures. Rerunning the harness skips everything already cached by source hash.
2. B2/B3 (`code/css-pseudo-class-split`, `code/css-url-token-intact`) are handed to the `d4l1-code` highlighter lane's follow-up; this audit lane did not race it.
3. For the 7 iftags-true-gate fixtures, take a saved-page capture with tags `fruit, component`. Anonymous PagePreview cannot judge them.
4. Decide whether the 17 whitespace-only divergences should be fixed or recorded as reviewed `comparison-normalization`. Project policy needs a browser review before that label applies.

## Why this audit matters

Only **140 of 986** fixtures with a `wikidot.html` have a frozen reference row in `tests/fixtures/wikidot-parity/references-*.jsonl` whose `source_sha256` matches the current `input.ftml`. For the other 846, the repo holds no live provenance for the current source. Their expectation is whatever FTML produced when the fixture was pinned. This audit found six such expectations that encode behavior live Wikidot does not show.

## Method notes

- **Request:** `POST https://sandbox-for-codex.wikidot.com/ajax-module-connector.php` with `moduleName=edit/PagePreviewModule`, `mode=page`, `source=<input.ftml verbatim>`, and a random `wikidot_token7` cookie/field pair. Anonymous, read-only.
- **PagePreview and `[[iftags]]`:** anonymous PagePreview evaluates **every** `[[iftags]]` gate as false. That includes `[[iftags -zzz]]` and the empty `[[iftags]]`, and it still happens when you pass `page_id`/`wiki_page` of an existing tagged page (probed against page 1468865065, tagged `codex`). The corpus runner (`src/test/ast/runner.rs`) renders with tags `fruit, component`. So a fixture with a gate that is true under those tags cannot be judged from PagePreview. The harness marks these `context: iftags-true-gate` and does not count them as mismatches.
- **Page existence:** live adds `class="newpage"` to links whose target does not exist on the sandbox. That depends on the caller runtime, so it is marked `context`.
- **Mismatch classes:**
  - *Bug-encoded:* elements, attributes, classes or visible text differ, so the expectation pins observable behavior that live does not have.
  - *Stale but harmless:* the only difference is formatting whitespace at a block or line-box edge, which CSS `white-space: normal` collapses. Element and attribute structure match, and so does collapsed visible text.

## Bug-encoded fixtures

### B1. `list/comment-line-start`: line-start comment newline elision (rule confirmed)

| Source | Live | FTML / fixture |
|---|---|---|
| `[!--x--]* item\n[!--a--][!--b--]# item` | `<ul><li>item# item</li></ul>` | `<ul><li>item</li></ul><ol><li>item</li></ol>` |

Live rule, confirmed by the probes below: a comment that begins at column 0 of a line removes exactly **one** preceding line break, and the comment itself is elided. This happens whatever follows the comment. FTML gets this right for prose, but `exposed_line_start_construct` (`src/preproc/comments.rs:118`, added in c646ddd without live evidence) exempts lines that start with `* `, `# `, `+`, `----`, `~~~~`, `====` or `[[gallery`. **Every exempted construct diverges from live:**

| Probe source | Live | FTML |
|---|---|---|
| `* item\n[!--a--]# item` | `<ul><li>item# item</li></ul>` | ul + separate ol |
| `* item\n[!--a--]* item2` | `<ul><li>item* item2</li></ul>` | ul with two items |
| `# one\n[!--a--]# two` | `<ol><li>one# two</li></ol>` | ol with two items |
| `# one\n[!--a--]* two` | `<ol><li>one* two</li></ol>` | ol + ul |
| `* item\n[!--a--] # item` | `<ul><li>item # item</li></ul>` | nested ol |
| `* item\n[!--a--]+ Head` | `<ul><li>item+ Head</li></ul>` | ul + h1 |
| `a\n[!--c--]* item` | `<p>a* item</p>` | p + ul |
| `a\n[!--c--]+ Head` | `<p>a+ Head</p>` | p + h1 |
| `a\n[!--c--]----\nb` | `<p>a&#8212;&#8212;<br />b</p>` | p + hr + p |
| `a\n[!--c--]~~~~\nb` | `<p>a~~~~<br />b</p>` | clear div |
| `a\n[!--c--]====\nb` | `<p>a====<br />b</p>` | content-separator |
| `a\n[!--c--][[gallery]]\n[[/gallery]]` | `<p>a[[gallery]]<br />[[/gallery]]</p>` | gallery |
| `a\n[!--c--]\n[!--d--]* item` | `<p>a* item</p>` (each comment eats one break) | p + ul |
| `* one\n\n[!--c--]* two` | one `<ul>` with two items | two lists |
| `> q\n\n[!--c--]> r` | one blockquote, `q<br />r` | quote + literal `&gt; r` |
| `\|\| a \|\|\n\n[!--c--]\|\| b \|\|` | one table, two rows | table + literal row |

Controls that already match: `a\n[!--c--]b` → `<p>ab</p>`; `a\n  [!--c--]* item` (comment not at column 0) → `<p>a<br />* item</p>`; `a\n\n[!--c--]# item` → p + ol; `[!--c--]+ Head` at document start → h1; `x\n\n[!--c--]text` → `<p>x<br />text</p>`.

**Fix direction:** remove the exposure carve-out. A column-0 comment drops one preceding break in every case, and at document start nothing is dropped. Pin the probes above as corpus fixtures.

### B2. `code/css-pseudo-class-split`: named colour in a CSS value

`color: red;` inside `[[code type="css"]]`. Live gives `<span class="hl-var">red</span>`; FTML gives `hl-identifier`. The fixture was pinned for the pseudo-class split, and that part does match live. The colour-keyword class was pinned along with it without being checked. FTML's highlighter falls back to `hl-identifier` for every unknown word (`src/render/html/element/text.rs:249`). Live (PEAR Text_Highlighter CSS) classifies tokens differently inside a property value.

### B3. `code/css-url-token-intact`: `url(...)` in a CSS value

`background: url(/files/deleted:component/x.png);`. Live gives `<span class="hl-code"> url(/files/deleted:component/x.png)</span>`: the leading space and the URL share one `hl-code` span. FTML gives `<span class="hl-code"> </span><span class="hl-identifier">url(…)</span>`. Same root cause as B2: words inside a value are `hl-code` in live, not `hl-identifier`.

### B4. `misc/bibliography-duplicate-definition`: duplicate-label numbering

Live numbers the duplicate-label definition with the **same** number and id as the first one: `2. Second bell entry.` then `2. Guy entry.`, both `id="bibitem-2"`. The fixture pins `3.` and `id="bibitem-3"`. Visible text differs. This needs reproducing, duplicate id included (it is Wikidot behavior).

### B5. `span/fused-opener-scored-map-residual`: paragraph wrapper

Live `<span class="c">]x</span>` (no paragraph); fixture `<p><span class="c">]x</span></p>`.

Adjudicated with live probes (captured twice): live leaves a run unwrapped when its
last content is a scored span, whatever precedes it on the line
(`a [[span_]]x[[/span]]` → `a <span>x</span>`), and wraps it again as soon as
anything follows the span (`a [[span_]]x[[/span]] b` → `<p>…</p>`,
`…[[/span]]**b**` → `<p>…</p>`). A plain `[[span]]` is always wrapped. Fixed by
tracking a sequence's trailing scored-span close through paragraph assembly and
skipping the wrapper.

### B6. `list/fused-close`: extra trailing `<br>` at EOF

Source ends with `[[/ul]]]` and no final newline. Live ends with `</ul>\n]`. The fixture ends with `</ul>]<br>`, an extra `<br>` element. Low visual impact, but it is an element-level divergence.

Adjudicated with live probes (captured twice): the residual `]` occupies the
line-end slot, so at EOF and before a blank line live emits no break, while a
single source line break after the closer still renders as `<br />`. The
manufactured break is now emitted only for the single-line-break case.

## Stale but harmless (formatting whitespace only)

All 17 have matching element/attribute structure and matching collapsed visible text. They are still raw-DOM differences produced by FTML, so they should be fixed or reviewed as `comparison-normalization`.

| Fixture | Live | Fixture |
|---|---|---|
| `code/closer-first-suffixed-wins`, `code/closer-whitespace-then-text`, `code/nested-suffixed-outer-closer` | `</div>\nx` (space after a same-line `[[/code]] x` tail dropped) | `</div>\n x` |
| `code/two-codes-suffixed-first-closer` | `</div>\nx\n<div class="code">` | `</div>\n x<div class="code">` |
| `list/explicit-crossed-pre-trailing-dissolves`, `list/explicit-scope-break-inside-paragraph`, `list/explicit-scope-break-paragraph`, `list/nested-multi-item-partial-close`, `list/nested-partial-close-in-item`, `list/zombie-pair-fired-scope-not-reopened`, `span/basic` | `…<br></span>` (no newline after the trailing `<br>` inside an inline span) | `…<br>\n</span>` |
| `list/explicit-post-table-literal`, `list/explicit-pre-kept-before-prose`, `misc/bibliography-second-block` | `</ul>\nY Z` / `</p>\n[[bibliography]]` | `</ul>Y Z` / `</p>[[bibliography]]` |
| `strikethrough/end-block-closes-scope` | `</div>\ny—` | `</div>\n y—` |
| `misc/dash-line-fusion` | `-<br></span> -<br>` | `-<br>\n</span>-<br>` |
| `misc/quote-pair-close-in-owned-value` | image `src` on `sandbox-for-codex.wdfiles.com/local--files//…` | `ast-test.wjfiles.com/local--files/test:page-…/…` |

The last row is a page/site-dependent file URL (caller runtime) plus a newline after `</p>`. It is not a fixture bug.

## Context-only (not judgeable from PagePreview)

| Fixture | Reason |
|---|---|
| `color/iftags-seam-spec`, `list/iftags-body-escape`, `list/iftags-body-escape-empty-item`, `list/iftags-body-escape-ends-run`, `list/iftags-body-escape-single-line`, `span/iftags-seam`, `span/iftags-seam-attribute-name` | Has a gate that is true under the runner's tags `fruit, component`; PagePreview evaluates all gates false. Needs a saved-page capture. |
| `color/whitespace-only-body-after-link` | Only difference is live's `class="newpage"` (page existence, caller runtime). |

## Full audit table

`status`: `match`; `mismatch` (with class); `context`. `dom`/`text` are the normalized-DOM and visible-text checks. `live sha256` is the SHA-256 of the raw PagePreview body.

| fixture | status | class / context | dom | text | live sha256 (prefix) |
|---|---|---|---|---|---|
| `code/anchor-copy-inside` | match |  | ok | ok | `ef7b8c4de106` |
| `code/basic` | match |  | ok | ok | `e489af28b02a` |
| `code/closer-comment-suffix-literal` | match |  | ok | ok | `b7b92391ee79` |
| `code/closer-directly-suffixed-literal` | match |  | ok | ok | `26bd6b48b63b` |
| `code/closer-first-suffixed-wins` | mismatch | stale-but-harmless (whitespace) | diff | ok | `c7e0b88ae74f` |
| `code/closer-whitespace-then-text` | mismatch | stale-but-harmless (whitespace) | diff | ok | `05965146d1f1` |
| `code/comment-name` | match |  | ok | ok | `c48e42343749` |
| `code/css-pseudo-class-split` | mismatch | bug-encoded (B2) | diff | ok | `e898c2e89ed3` |
| `code/css-url-token-intact` | mismatch | bug-encoded (B3) | diff | ok | `802d8eccfd62` |
| `code/fail` | match |  | ok | ok | `e89ddf755d2c` |
| `code/fused-head-close` | match |  | ok | ok | `04c1718480d1` |
| `code/iftags-seam-head` | match |  | ok | ok | `ab38e7617aa6` |
| `code/iftags-seam-name` | match |  | ok | ok | `c48e42343749` |
| `code/iftags-seam-prefix-name` | match |  | ok | ok | `79c4072f45a3` |
| `code/invalid-closer-then-nested` | match |  | ok | ok | `6411b25e03de` |
| `code/mid-line-opener-literal` | match |  | ok | ok | `2ff30eee0dbc` |
| `code/nested-suffixed-outer-closer` | mismatch | stale-but-harmless (whitespace) | diff | ok | `ed2f9bcdb13e` |
| `code/revert` | match |  | ok | ok | `aba9708a46aa` |
| `code/two-codes-suffixed-first-closer` | mismatch | stale-but-harmless (whitespace) | diff | ok | `eb8813db8438` |
| `color/basic` | match |  | ok | ok | `d4608283ad63` |
| `color/collapsible-end-barrier` | match |  | ok | ok | `412d641fc17d` |
| `color/comment-spec` | match |  | ok | ok | `3f630bcf6df6` |
| `color/crossed-span-closer-swaps` | match |  | ok | ok | `b6dc0018d628` |
| `color/crossed-strikethrough-empty-segment` | match |  | ok | ok | `3c71408f5b6b` |
| `color/div-end-barrier` | match |  | ok | ok | `c7f785839e27` |
| `color/fail` | match |  | ok | ok | `d0510b5fe429` |
| `color/footnote-end-barrier` | match |  | ok | ok | `381d3615081d` |
| `color/iftags-seam-spec` | context | iftags-true-gate | diff | ok | `a2cf54bb0061` |
| `color/malformed-nested` | match |  | ok | ok | `d2a163416d40` |
| `color/malformed-nested-footnote-bug` | match |  | ok | ok | `133b31441a94` |
| `color/padded-close-still-closes` | match |  | ok | ok | `1549e42f5b0e` |
| `color/trailing-newline-body` | match |  | ok | ok | `161143426964` |
| `color/whitespace-only-body` | match |  | ok | ok | `e8bb85331b4c` |
| `color/whitespace-only-body-after-link` | context | page-existence-newpage-class | diff | ok | `27b55b1c1386` |
| `color/whitespace-only-body-after-span` | match |  | ok | ok | `9e808ffab002` |
| `color/whitespace-only-body-before-block` | match |  | ok | ok | `a847c2c9d682` |
| `color/whitespace-only-body-before-span-close` | match |  | ok | ok | `aa349f910959` |
| `color/whitespace-only-body-consecutive` | match |  | ok | ok | `06c582ce8b08` |
| `color/whitespace-only-body-glued` | match |  | ok | ok | `06c582ce8b08` |
| `color/whitespace-only-body-span-mid-run` | match |  | ok | ok | `5c513cd5db20` |
| `color/whitespace-only-body-span-open-hoist` | match |  | ok | ok | `13ad8d48f707` |
| `color/whitespace-only-body-span-trailing-run` | match |  | ok | ok | `aa349f910959` |
| `color/whitespace-only-body-vanishing-paragraph` | match |  | ok | ok | `af2dc9f55635` |
| `list/after-spaced-line` | match |  | ok | ok | `5176c04a490c` |
| `list/block` | match |  | ok | ok | `2183a77d264f` |
| `list/block-fail` | match |  | ok | ok | `f932285b7185` |
| `list/block-score` | match |  | ok | ok | `fcb0360eb8cb` |
| `list/block-score-nested` | match |  | ok | ok | `84751b0ae19e` |
| `list/block-spaced-end` | match |  | ok | ok | `dccad0d200d7` |
| `list/comment-line-start` | mismatch | bug-encoded (B1) | diff | diff | `2d715798de2d` |
| `list/definition-scope-cross` | match |  | ok | ok | `4fa985222836` |
| `list/explicit-crossed-pre-trailing-dissolves` | mismatch | stale-but-harmless (whitespace) | diff | ok | `483657d5e9e8` |
| `list/explicit-empty-post-prose-absorbed` | match |  | ok | ok | `8f56fd0409bc` |
| `list/explicit-post-blank-stays-bare` | match |  | ok | ok | `753e70938b80` |
| `list/explicit-post-table-literal` | mismatch | stale-but-harmless (whitespace) | diff | ok | `7ac859f0e340` |
| `list/explicit-pre-dissolves-before-div` | match |  | ok | ok | `8fc8b228d1b2` |
| `list/explicit-pre-kept-before-prose` | mismatch | stale-but-harmless (whitespace) | diff | ok | `65f708cada99` |
| `list/explicit-pre-trailing-dissolves` | match |  | ok | ok | `083d1171bd0a` |
| `list/explicit-scope-break-inside-paragraph` | mismatch | stale-but-harmless (whitespace) | diff | ok | `10b846067d25` |
| `list/explicit-scope-break-paragraph` | mismatch | stale-but-harmless (whitespace) | diff | ok | `488b5e5b8783` |
| `list/fused-close` | mismatch | bug-encoded (B6) | diff | ok | `cef44d17ad7b` |
| `list/fused-head-close` | match |  | ok | ok | `bc8c215bf45d` |
| `list/fused-opener-ol-residual` | match |  | ok | ok | `df6aa8c74b87` |
| `list/fused-opener-ul-residual` | match |  | ok | ok | `d987da0d03d7` |
| `list/iftags-body-escape` | context | iftags-true-gate | diff | diff | `75a11da44c80` |
| `list/iftags-body-escape-empty-item` | context | iftags-true-gate | diff | diff | `75a11da44c80` |
| `list/iftags-body-escape-ends-run` | context | iftags-true-gate | diff | diff | `1bae9c606d49` |
| `list/iftags-body-escape-single-line` | context | iftags-true-gate | diff | diff | `75a11da44c80` |
| `list/iftags-seam-name` | match |  | ok | ok | `393eeba0a234` |
| `list/item-blank-after-block-break` | match |  | ok | ok | `ba10db654eaa` |
| `list/item-blank-merge` | match |  | ok | ok | `84fa52a6658d` |
| `list/item-circular-close-extent` | match |  | ok | ok | `f7e78e03d0d8` |
| `list/item-close-before-nested-list` | match |  | ok | ok | `af91ad2bd26b` |
| `list/item-close-extent-last-close` | match |  | ok | ok | `d8beb37d90c0` |
| `list/item-close-inside-discarded-gate` | match |  | ok | ok | `a010da8754d1` |
| `list/item-close-literal-in-span` | match |  | ok | ok | `721416b3e83c` |
| `list/item-head-inside-item-body` | match |  | ok | ok | `2b45c7177c58` |
| `list/item-middle-paragraphs` | match |  | ok | ok | `6de520d26240` |
| `list/item-newline-after-nested-list` | match |  | ok | ok | `229b457f2564` |
| `list/native` | match |  | ok | ok | `24490df54bb8` |
| `list/native-literal` | match |  | ok | ok | `310847481215` |
| `list/native-skipped-depth-empty-parent` | match |  | ok | ok | `be320f96578a` |
| `list/native-wikidot-structure` | match |  | ok | ok | `ad31706347bd` |
| `list/nested-item-close-pairing-inner` | match |  | ok | ok | `981f1e592016` |
| `list/nested-line-item-partial-close` | match |  | ok | ok | `81f4dd4ae7cb` |
| `list/nested-list-items-both-closed` | match |  | ok | ok | `229b457f2564` |
| `list/nested-multi-item-partial-close` | mismatch | stale-but-harmless (whitespace) | diff | ok | `6c881923fdf2` |
| `list/nested-partial-close-in-item` | mismatch | stale-but-harmless (whitespace) | diff | ok | `2433ad7e4867` |
| `list/nested-unclosed-item-close-dropped` | match |  | ok | ok | `f6a4f4bb1af8` |
| `list/scope-cross-item` | match |  | ok | ok | `97431c3fa54a` |
| `list/scope-cross-item-dash` | match |  | ok | ok | `f7b73269674d` |
| `list/scope-cross-item-nested` | match |  | ok | ok | `bc7fa3d5ba54` |
| `list/scope-opens-before-line-list` | match |  | ok | ok | `edd81538b5b9` |
| `list/ul-space-only-argument-region` | match |  | ok | ok | `c2b4c61c6eef` |
| `list/unclosed-item-in-closed-list` | match |  | ok | ok | `e6ae1070ff05` |
| `list/whitespace-only-item-vanishes` | match |  | ok | ok | `e4774acd00a5` |
| `list/zombie-pair-fired-scope-not-reopened` | mismatch | stale-but-harmless (whitespace) | diff | ok | `27e52c392057` |
| `list/zombie-para-pair-fired-scope-not-reopened` | match |  | ok | ok | `07b67546863a` |
| `misc/bibcite-iftags-seam` | match |  | ok | ok | `5cac15e40e9e` |
| `misc/bibliography` | match |  | ok | ok | `a88856ccb232` |
| `misc/bibliography-dashes` | match |  | ok | ok | `7c8c46f6f2e3` |
| `misc/bibliography-definition-separators` | match |  | ok | ok | `5d85e2b05752` |
| `misc/bibliography-duplicate-definition` | mismatch | bug-encoded (B4) | diff | diff | `8b234414f335` |
| `misc/bibliography-empty` | match |  | ok | ok | `85e1074c0df6` |
| `misc/bibliography-inline-scopes` | match |  | ok | ok | `aaa0fe46fbd5` |
| `misc/bibliography-invalid-line-residual` | match |  | ok | ok | `120a5652ef3d` |
| `misc/bibliography-label-charset` | match |  | ok | ok | `72d3b3ef17f3` |
| `misc/bibliography-label-only-crossing` | match |  | ok | ok | `85d5f2292886` |
| `misc/bibliography-missing-cite` | match |  | ok | ok | `b98191708936` |
| `misc/bibliography-next-line-value` | match |  | ok | ok | `3ef3ad98f68d` |
| `misc/bibliography-second-block` | mismatch | stale-but-harmless (whitespace) | diff | ok | `a25fb08b856c` |
| `misc/bibliography-tab-gap` | match |  | ok | ok | `991c83833b74` |
| `misc/bibliography-tab-prefix` | match |  | ok | ok | `9fd1d0d2d551` |
| `misc/bibliography-tabview-residue` | match |  | ok | ok | `2ad871158f0a` |
| `misc/bibliography-unclosed-keyed-items` | match |  | ok | ok | `2ff4e00ea296` |
| `misc/bibliography-unclosed-residue-vanish` | match |  | ok | ok | `eb7e219d6ec0` |
| `misc/bibliography-unclosed-trailing-paragraph` | match |  | ok | ok | `ea68b94d8584` |
| `misc/center` | match |  | ok | ok | `8eab0e2dd09b` |
| `misc/center-after-spaced-line` | match |  | ok | ok | `eb24cf9a25a0` |
| `misc/center-leading-space` | match |  | ok | ok | `c8cb31082414` |
| `misc/char` | match |  | ok | ok | `05a079fb9b5f` |
| `misc/clear-float` | match |  | ok | ok | `df4e01d6644e` |
| `misc/comment` | match |  | ok | ok | `5c03c4e50b9b` |
| `misc/comment-nospace-glue` | match |  | ok | ok | `5964e67762cf` |
| `misc/comment-space-collapse` | match |  | ok | ok | `2a065ea8097d` |
| `misc/comment-space-merge-bold` | match |  | ok | ok | `20fc86f3824a` |
| `misc/dash-edges` | match |  | ok | ok | `044b6d6e8108` |
| `misc/dash-line-fusion` | mismatch | stale-but-harmless (whitespace) | diff | ok | `a71ee2e6d4ad` |
| `misc/em-dash` | match |  | ok | ok | `edc5eb57bedc` |
| `misc/email` | match |  | ok | ok | `fe69ca6b9e3c` |
| `misc/guillemet` | match |  | ok | ok | `abbe4ed12aca` |
| `misc/hr` | match |  | ok | ok | `8c0d6c14305e` |
| `misc/hr-after-spaced-line` | match |  | ok | ok | `849c5cc48b23` |
| `misc/hr-leading-whitespace` | match |  | ok | ok | `1ce02946bbbf` |
| `misc/number-dollar-no-nbsp` | match |  | ok | ok | `f982ef19844a` |
| `misc/quote-pair-close-in-owned-value` | mismatch | stale-but-harmless (whitespace) | diff | ok | `d7e575222757` |
| `misc/spaces` | match |  | ok | ok | `75a11da44c80` |
| `misc/symbols` | match |  | ok | ok | `55fc463599f5` |
| `misc/typography-comment-split-runs` | match |  | ok | ok | `10739d93448c` |
| `monospace/basic` | match |  | ok | ok | `281d2fcb87f8` |
| `monospace/collapsible-end-barrier` | match |  | ok | ok | `842edec6c714` |
| `monospace/crossed-bold-italics-close-inside` | match |  | ok | ok | `721cf844ebb4` |
| `monospace/crossed-bold-italics-close-outside` | match |  | ok | ok | `4b03976256e6` |
| `monospace/div-end-barrier` | match |  | ok | ok | `ce6107de3fe1` |
| `monospace/div-inside-forms` | match |  | ok | ok | `5e6864f9d7e9` |
| `monospace/fail` | match |  | ok | ok | `e69966606a6b` |
| `monospace/footnote-end-barrier` | match |  | ok | ok | `f4ffcb46b34e` |
| `monospace/hidden-gates-pad-close-strips` | match |  | ok | ok | `4e25f90e52f7` |
| `monospace/opener-space-forms` | match |  | ok | ok | `ca41330ddf25` |
| `monospace/padded-close-still-closes` | match |  | ok | ok | `5c7f7dcb1711` |
| `monospace/span-end-forms` | match |  | ok | ok | `fbaa5c725266` |
| `monospace/url-shed-close` | match |  | ok | ok | `5b4b4f0b57bb` |
| `monospace/url-shed-split` | match |  | ok | ok | `78a456dae2e7` |
| `span/basic` | mismatch | stale-but-harmless (whitespace) | diff | ok | `b7190d37bc1c` |
| `span/code-literal-inside` | match |  | ok | ok | `dacb9bc2544b` |
| `span/comment-argument-gap` | match |  | ok | ok | `f20f0807e945` |
| `span/comment-attribute-name` | match |  | ok | ok | `8da3b4c222c0` |
| `span/comment-attribute-value` | match |  | ok | ok | `8ddda002e29c` |
| `span/comment-attribute-value-empty` | match |  | ok | ok | `b689600a696e` |
| `span/comment-name` | match |  | ok | ok | `946ec24cb467` |
| `span/comment-only-argument-region` | match |  | ok | ok | `a48fbdabfad7` |
| `span/comment-prefix-name` | match |  | ok | ok | `a2fda310ece5` |
| `span/crossed-bold-empty-segments` | match |  | ok | ok | `ab2590a1789b` |
| `span/crossed-close-after-scope-ended-closes-current` | match |  | ok | ok | `3664f557385e` |
| `span/crossed-closers-close-innermost` | match |  | ok | ok | `b633fcd108ed` |
| `span/crossed-color-closer-swaps` | match |  | ok | ok | `95db6291dded` |
| `span/crossed-delimiter-native` | match |  | ok | ok | `91fa2fa6b51d` |
| `span/crossed-div-and-span` | match |  | ok | ok | `eb3d85a8b9ec` |
| `span/crossed-div-trailing-space` | match |  | ok | ok | `5ca1167acfdf` |
| `span/crossed-paragraph-break-bold` | match |  | ok | ok | `8dc152c7376d` |
| `span/crossed-strike-closes-first` | match |  | ok | ok | `8bc328f50033` |
| `span/crossed-with-blockquote` | match |  | ok | ok | `a69b917f7655` |
| `span/crossed-with-div` | match |  | ok | ok | `b05174a60dc2` |
| `span/crossed-with-div-attributes` | match |  | ok | ok | `189e58164801` |
| `span/crosses-paragraph-break` | match |  | ok | ok | `6d06fcd24a28` |
| `span/div-nested-literal` | match |  | ok | ok | `3e61f78da37e` |
| `span/empty-id-preserved` | match |  | ok | ok | `800d3c04180d` |
| `span/fused-open` | match |  | ok | ok | `d23ee49fea28` |
| `span/fused-opener-map-residual` | match |  | ok | ok | `d23ee49fea28` |
| `span/fused-opener-name-residual` | match |  | ok | ok | `8ee06435fa2d` |
| `span/fused-opener-scored-map-residual` | mismatch | bug-encoded (B5) | diff | ok | `e6aa2e903698` |
| `span/fused-scored-residual-space` | match |  | ok | ok | `54a1adfb645d` |
| `span/iftags-seam` | context | iftags-true-gate | diff | ok | `b1342d8328eb` |
| `span/iftags-seam-attribute-name` | context | iftags-true-gate | diff | ok | `d93beaf52854` |
| `span/iftags-seam-fused-close` | match |  | ok | ok | `5f97a3dbd8cc` |
| `span/iftags-seam-fused-open` | match |  | ok | ok | `d23ee49fea28` |
| `span/iftags-seam-name` | match |  | ok | ok | `946ec24cb467` |
| `span/iftags-seam-prefix-name` | match |  | ok | ok | `a2fda310ece5` |
| `span/inner-opener-loses-to-first` | match |  | ok | ok | `d1ac4080195d` |
| `span/leading-space` | match |  | ok | ok | `e1e485c0c6c4` |
| `span/leading-space-list-item-edge` | match |  | ok | ok | `1c5e08f8a78e` |
| `span/leading-space-paragraph-edge` | match |  | ok | ok | `c1a68f91dc7b` |
| `span/leading-space-separator-fates` | match |  | ok | ok | `c8c1c73dee54` |
| `span/monospace-reopens-after-crossed-close` | match |  | ok | ok | `fbaa5c725266` |
| `span/nested-closers-match-openers` | match |  | ok | ok | `b633fcd108ed` |
| `span/nested-edge-spaces` | match |  | ok | ok | `1e038cf08f28` |
| `span/newline-argument-region` | match |  | ok | ok | `cf2f4915af48` |
| `span/paragraph-break` | match |  | ok | ok | `11e689928f25` |
| `span/scored-two-space-argument-region` | match |  | ok | ok | `8d3f654f3661` |
| `span/single-quoted-attribute` | match |  | ok | ok | `cf4b477cec4f` |
| `span/space-only-argument-region` | match |  | ok | ok | `a48fbdabfad7` |
| `span/span-head-split-comment-keeps-spacing` | match |  | ok | ok | `946ec24cb467` |
| `span/stray-closer-in-bold` | match |  | ok | ok | `0991e856f1fc` |
| `span/stray-closer-keeps-spaces` | match |  | ok | ok | `ffdd15911448` |
| `span/tab-argument-region` | match |  | ok | ok | `576a97f8a64c` |
| `span/two-space-argument-region` | match |  | ok | ok | `576a97f8a64c` |
| `span/unpaired-size-closer-stays-literal` | match |  | ok | ok | `d7de107a5cff` |
| `strikethrough/basic` | match |  | ok | ok | `e310a7db1e01` |
| `strikethrough/color-body-dash-pairs-across` | match |  | ok | ok | `1f15960775b9` |
| `strikethrough/color-close-kept-from-nested-strike` | match |  | ok | ok | `18ab911f35ec` |
| `strikethrough/color-deep-nested-strike-no-outer` | match |  | ok | ok | `86589e21bfe9` |
| `strikethrough/color-empty-span-dropped-in-strike` | match |  | ok | ok | `878ef1d65297` |
| `strikethrough/color-leading-space-pairs-with-trailing` | match |  | ok | ok | `204474432fca` |
| `strikethrough/color-nested-strike-no-outer` | match |  | ok | ok | `c004c4112ed5` |
| `strikethrough/crossed-after-pair` | match |  | ok | ok | `106a58e4fdd1` |
| `strikethrough/crossed-bold-close-inside` | match |  | ok | ok | `2dc54f828a0c` |
| `strikethrough/crossed-bold-empty-segment` | match |  | ok | ok | `f6861b01cabf` |
| `strikethrough/crossed-bold-empty-segments` | match |  | ok | ok | `1a87385537c1` |
| `strikethrough/crossed-bold-open-inside` | match |  | ok | ok | `c4b9ec4118fc` |
| `strikethrough/crossed-bold-shed-run` | match |  | ok | ok | `54c21e2c6e17` |
| `strikethrough/crossed-color-close-inside` | match |  | ok | ok | `5faf8c916257` |
| `strikethrough/crossed-italics-close-inside` | match |  | ok | ok | `250a5469843d` |
| `strikethrough/crossed-monospace-close-inside` | match |  | ok | ok | `ee5ed59d475e` |
| `strikethrough/crossed-nested-italics` | match |  | ok | ok | `f3301e2e1ad6` |
| `strikethrough/crossed-nested-monospace` | match |  | ok | ok | `191b1a376543` |
| `strikethrough/crossed-subscript-close-inside` | match |  | ok | ok | `561dd6c4a8b5` |
| `strikethrough/crossed-superscript-close-inside` | match |  | ok | ok | `b181b6f368ee` |
| `strikethrough/crossed-underline-close-inside` | match |  | ok | ok | `f8cc9fed8461` |
| `strikethrough/deep-nesting-trailing-em-inside` | match |  | ok | ok | `df9797d8f17d` |
| `strikethrough/end-block-closes-scope` | mismatch | stale-but-harmless (whitespace) | diff | ok | `384af40530ed` |
| `strikethrough/fail` | match |  | ok | ok | `9ac78bbe0c42` |

Captured 2026-11-05 between 07:55 and 08:40 UTC.
