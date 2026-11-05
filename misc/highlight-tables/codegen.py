"""Shared helpers for the Text_Highlighter table transcribers.

Reads tables_<LANG>.json (see parse_tables.py) for the 13 Wikidot code
languages and validates/rewrites rule alternatives. The emitter transcribes
the snapshot Wikidot vendors (see README.md):
  E4: javascript gains the pre-2007 `regexp` region (removed upstream in 2007).
  E5: css loses the `mlcomment` region (added upstream in 2008).
Non-portable rule alternatives are replaced by never-matching placeholders
with identical group counts and reimplemented as custom matchers in the
Rust engine (see CUSTOM_RULES).
"""
import json
import os
import re

LANGS = ['PHP', 'HTML', 'JAVASCRIPT', 'XML', 'SQL', 'CPP', 'DIFF',
         'RUBY', 'PERL', 'PYTHON', 'JAVA', 'CSS', 'DTD']

# (lang, state, alternative-source-substring, custom_id) for alternatives the
# Rust `regex` crate cannot express. The placeholder keeps group counts stable.
CUSTOM_RULES = [
    ('PYTHON', None, r'[a-z_]\w*(?=\s*\()', 'py_funcname'),
    ('CSS', None, r'(\*))(?!', 'css_selector'),
    ('DTD', None, r'(?<=\<)!(ENTITY', 'dtd_tagname'),
    ('HTML', None, r'(?<=[\<\/?])', 'html_tagname'),
    ('XML', None, r'(?<=[\<\/?])', 'html_tagname'),
    ('PERL', None, 'q[wq]', 'perl_qqw'),
    ('PERL', None, '(q\\s*', 'perl_q'),
]

# Rules that cannot match in the deployed engine: the rule contains a
# backreference (\2/\7) that, inside the combined per-state alternation the
# generated classes use, resolves to an unrelated capture group, so the rule
# never fires. They are transcribed as never-matching placeholders, which
# reproduces the observed behavior exactly (verified against the PEAR
# reference harness).
DEAD_RULES = [
    ('PERL', r'(s|tr) '),
    ('PERL', r'(m) '),
]

# (lang, state, end-source-substring, custom_id) for non-portable endpatterns.
CUSTOM_ENDS = [
    ('CPP', '5', '(?<!' + chr(92)*2 + ')$', 'cpp_line_end'),
    ('CPP', '6', '(?<!' + chr(92)*2 + ')$', 'cpp_line_end'),
    ('PHP', '9', '$|(?=' + chr(92) + '?' + chr(92) + '>)', 'php_code_end'),
    ('DIFF', None, '(?=^[ad]' + chr(92) + 'd', 'diff_norm_end'),
    ('CSS', '0', '(?=;|' + chr(92) + '})', 'css_prop_end'),
]


def tables_dir():
    return os.environ.get('HL_TABLES_DIR', '.')


def load(lang):
    with open(os.path.join(tables_dir(), 'tables_%s.json' % lang)) as f:
        return json.load(f)


def split_alt(s):
    assert s.startswith('/') and s.endswith('/'), s[:60]
    s = s[1:-1]
    if not s:
        return []
    parts = []
    depth = 0
    cur = ''
    cc = False
    esc = False
    for ch in s:
        if cc:
            cur += ch
            if esc:
                esc = False
            elif ch == '\\':
                esc = True
            elif ch == ']' and not esc:
                cc = False
        else:
            if ch == '\\':
                esc = True
                cur += ch
                continue
            wasesc = esc
            esc = False
            if ch == '[' and not wasesc:
                cc = True
                cur += ch
            elif ch == '(' and not wasesc:
                depth += 1
                cur += ch
            elif ch == ')' and not wasesc:
                depth -= 1
                cur += ch
            elif ch == '|' and depth == 0 and not wasesc:
                parts.append(cur)
                cur = ''
            else:
                cur += ch
    parts.append(cur)
    return parts


def has_unsupported(alt):
    tmp = re.sub(r'\[(?:\\.|[^\]\\])*\]', '[]', alt)
    if re.search(r'\(\?[=!]|\(\?<[=!]|\(\?\(|\(\?P|\(\*[A-Z]+', tmp):
        return True
    if re.search(r'(?<!\\)\\[1-9]', tmp):
        return True
    if re.search(r'\(\?[a-zA-Z]*U', tmp):
        return True
    return False


def count_groups(alt):
    n = 0
    i = 0
    cc = False
    esc = False
    while i < len(alt):
        ch = alt[i]
        if cc:
            if esc:
                esc = False
            elif ch == '\\':
                esc = True
            elif ch == ']':
                cc = False
            i += 1
            continue
        if esc:
            esc = False
            i += 1
            continue
        if ch == '\\':
            esc = True
            i += 1
            continue
        if ch == '[':
            cc = True
            i += 1
            continue
        if ch == '(':
            if alt[i + 1:i + 3] == '?:' or alt[i + 1:i + 3] == '?=' \
                    or alt[i + 1:i + 3] == '?!' or alt[i + 1:i + 4] == '?<=' \
                    or alt[i + 1:i + 4] == '?<!' or alt[i + 1:i + 3] == '?P':
                pass
            elif alt[i + 1] == '?':
                # flag group (?i), (?mi) ... non-capturing
                pass
            else:
                n += 1
            i += 1
            continue
        i += 1
    return n


def alt_is_dead(lang, alt):
    for clang, sub in DEAD_RULES:
        if clang == lang and sub in alt:
            return True
    return False


def alt_needs_custom(lang, state, alt):
    for clang, cstate, sub, cid in CUSTOM_RULES:
        if clang != lang:
            continue
        if cstate is not None and str(cstate) != str(state):
            continue
        if sub in alt:
            return cid
    return None


def main():
    for lang in LANGS:
        t = load(lang)
        regs = t['_regs']
        for st, altsrc in regs.items():
            alts = split_alt(altsrc)
            counts = t['_counts'][st]
            assert len(alts) == len(counts), (lang, st, len(alts), len(counts))
            for i, a in enumerate(alts):
                if has_unsupported(a):
                    assert (
                        alt_needs_custom(lang, st, a) is not None
                        or alt_is_dead(lang, a)
                    ), (lang, st, i, a[:100])
                    continue
                # _counts excludes the alternation wrapper group
                assert count_groups(a) - 1 == counts[str(i)], \
                    (lang, st, i, count_groups(a), counts[str(i)], a[:80])
    print('split/group counts OK for all languages')


if __name__ == '__main__':
    main()
