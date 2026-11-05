# Wikidot code-block highlight tables

`src/render/html/element/highlight_tables.rs` is generated from the PEAR
`Text_Highlighter` code that Wikidot's `[[code]]` block uses. The rendered
shape (`hl-main`, `hl-*` span classes, tab expansion, empty lines served as a
single space) matches that package's generated classes, so its language
definitions are the specification for token classes.

## Provenance

- Upstream: PEAR Text_Highlighter at commit **`0fab9c0`** (2007-06-03, the
  initial import of the package into its current history).
  Archive: `https://codeload.github.com/pear/Text_Highlighter/tar.gz/0fab9c0`
  (sha256 `94986c7af5a2bd31501f3c47c39c4d3a2796e6115b1d2cf7e37c78e90fddad83`).
  The generated language classes are committed at `Highlighter/<LANG>.php`.
- Languages Wikidot exposes: php, html, javascript, xml, sql, cpp, diff, ruby,
  perl, python, java, css, dtd.

That snapshot — not a later release — is the one Wikidot vendors. Live
PagePreview output pins the discriminating behaviours:

- `javascript`: the pre-2007 `regexp` region is present (upstream removed it
  in 2007-06-05, commit `d13ed9f`), and the number rule is the pre-hex one
  (`0x1f` -> `hl-number "0"` + `hl-identifier "x1f"`; the hex rule landed
  2008-01-01, commit `2712a36`).
- `css`: there is no `mlcomment` region (upstream added it 2008-01-01,
  commit `b9aa447`), so `/* ... */` is not a comment on live.
- `cpp`: preprocessor lines are `hl-prepro` and numbers are the pre-2008
  simple form, matching this snapshot rather than the later releases.
- `java`: the class-name "builtin" keyword group is disabled (see below).

No generation-time deltas are needed: re-running the scripts below against
that commit reproduces the checked-in file byte-for-byte.

## Keyword groups disabled by default

Upstream's generated classes carry `_conditions`, evaluated by
`_checkDefines()` against the caller's `defines` option. FTML passes no
defines, so every action without the invert flag leaves its group disabled;
the emitter records those groups in `HlLang::disabled`. The only such
condition in this snapshot is `java.builtins` (group `builtin`), which is why
live emits `hl-identifier` for `String`, `System`, `Exception`, and the rest
of that class list.

## Regenerating

From the repository root:

```sh
curl -L -o /tmp/Text_Highlighter-0fab9c0.tar.gz \
    https://codeload.github.com/pear/Text_Highlighter/tar.gz/0fab9c0
tar xzf /tmp/Text_Highlighter-0fab9c0.tar.gz -C /tmp
upstream=/tmp/Text_Highlighter-0fab9c0/Highlighter
for lang in PHP HTML JAVASCRIPT XML SQL CPP DIFF RUBY PERL PYTHON JAVA CSS DTD; do
    python3 misc/highlight-tables/parse_tables.py "$lang" "$upstream/$lang.php" /tmp/hl-tables
done
python3 misc/highlight-tables/emit_tables.py \
    --tables-dir /tmp/hl-tables \
    --out src/render/html/element/highlight_tables.rs
```

The emitter is deterministic: the checked-in file is exactly the output of the
commands above (the regenerated copy is byte-identical, including the header).

## Engine model

Rules whose PCRE cannot be expressed by the Rust `regex` crate are replaced in
the tables by never-matching placeholders with identical capture-group counts
and are reimplemented as custom matchers in
`src/render/html/element/highlight.rs`; the table therefore encodes group
counts exactly as upstream.

Two rules in the Perl table contain backreferences that, inside the combined
per-state alternation the generated classes use, resolve to unrelated capture
groups — those rules never fire in the deployed engine. The emitter transcribes
them as never-matching placeholders too (see `DEAD_RULES` in `codegen.py`),
which is what live output shows for `s///`, `m//` and `tr///`.
