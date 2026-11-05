# Wikidot Inline Nesting

Wikidot renders crossed inline markup such as `**a//b**c//` in two stages, and both are named software. Knowing this replaces guesswork about "repair quirks" with a rule you can read.

## Stage 1: Text_Wiki pairs each construct alone

Wikidot's `Text_Wiki` runs one regular expression per construct, in a fixed order, over the whole source. `Colortext` runs first, then `Strong`, `Emphasis`, `Underline`, `Strikethrough`, `Tt`, `Superscript`, `Subscript`. Each rule replaces its markers with opaque tokens and writes flat HTML tags in place.

A rule sees only its own marker. Three consequences follow:

- Pairs of different constructs can interleave. Nothing in this stage nests them.
- A marker pairs with the first valid later marker of its own kind, first fit. A padded marker is skipped, not fatal.
- An empty pair such as `****` writes no tag and leaves nothing behind, so a later rule sees the tokens around it as adjacent. `x//**** a//b` keeps its `//` literal because `Strong` runs before `Emphasis`. `x//____ a//b` forms because `Underline` runs after it.

The regular expressions stop at a real newline. Soft line breaks are already tokens by then, so paragraphs and block edges stop pairing, and soft line breaks do not.

## Stage 2: libtidy nests the flat tags

`WikiTransformation::purifyHtml` passes the flat HTML to `tidy_parse_string`. The nesting you see on live is the `ParseInline` function and the inline stack of libtidy. The state is:

- `E`, the open elements, innermost last. A tag name can appear several times.
- `I`, the inline stack. A tag name appears at most once, so a `<span>` opened inside another `<span>` gets no entry.

A close tag `</X>` with `Y` as the innermost open element follows these rules, in order:

1. If `Y` is `X`, pop the top of `I`, whatever it is, and close `Y`.
2. If `X` and `Y` are both in `I` and `Y` is above `X`, swap the two entries, remember the lower slot, close `Y`, and retry with the next `Y`.
3. Otherwise, pop the top of `I`, close `Y`, and consume the tag. `X` stays open if it was open.

Before the next token, every entry from the remembered slot upward reopens as a new element. A paragraph end or block start closes all of `E` and leaves `I` alone, and the next content reopens what `I` holds.

Trailing whitespace in a closing element moves out to the next text. Leading whitespace moves out as each element closes, while it is that element's first child. An empty element counts as a first child, because libtidy prunes empties only after parsing, so it stops the space.

Rule 3 is why tag names decide the output at depth. In `--a##red|xb**//,,--c##--,,//**##` the `<sub>` gets a stack entry, so the literal `##` lands inside the color. In the same shape with `__` in place of `,,`, the underline gets no entry, and `##` lands outside everything.

## How FTML maps to this

- `parser/inline_pairs.rs` is stage 1. It runs over the tokens and assigns each marker a role in a chunk whose pairs interleave, or that holds an unpaired marker beside a pair of another kind. Any other chunk keeps its normal parse, and so does a chunk with text the scan cannot see through, such as a link.
- `ActiveScopes` in `inline_scope.rs` is stage 2. `tidy_close` is the close rule, and `sync_chain_to_istack` is the paragraph and block boundary.

No pass outside lowering simulates nesting. A new stream shape belongs in stage 1, not in another repair pass.

Live Wikidot remains the specification. Neither Text_Wiki nor libtidy is a dependency. Use them to generate shapes and predictions, and pin only live captures. See `misc/inline-crossing-probes/` for the captures.
