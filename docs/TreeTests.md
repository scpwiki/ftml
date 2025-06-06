[<< Return to the README](../README.md)

## Abstract Syntax Tree Tests

An important part of verifying that ftml performs as expected is having an expansive battery of unit tests. This is essentially an organized set of files which feeds a particular wikitext as input into the parser and renderer(s), and then checks the outputs against the expected results.

### File Structure

Within `/test` in this repository, there are directories which correspond to "test groups", a set of tests which are related in some way. Within each test group is a particular test case.

Consider this structure:

```
test/
├── diff
│   ├── alias
│   ├── basic
│   └── newlines
└── underline
    ├── basic
    ├── empty
    └── fail
```

These directories define the following six test cases:
* `diff/alias`
* `diff/basic`
* `diff/newlines`
* `underline/basic`
* `underline/empty`
* `underline/fail`

Within each unit test directory, we have a series of files with standard names:
* `input.ftml` (required) &mdash; The input wikitext for this test case.
* `tree.json` &mdash; The syntax tree produced from parsing this wikitext.
* `errors.json` (required if `tree.json` exists and parsing yields errors) &mdash; If absent, assumed to be equivalent to `[]`. If present, then all errors within are compared to those produced by the parser.
* `wikidot.html` &mdash; The rendered output (HTML renderer, Wikidot layout).
* `output.html` &mdash; The rendered output (HTML renderer, Wikijump layout).
* `output.txt` &mdash; The rendered output (text renderer).

### Execution

You can run the AST test suite and see the output by using `cargo`:

```
$ cargo test -- ast --nocapture
```

This will run all syntax tree tests currently in the repository.
