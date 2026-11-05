import re, json, sys

BS = chr(92)  # backslash

def find_array_end(src, start):
    # start: index just after 'array ('
    depth, k = 1, start
    instr, esc = False, False
    n = len(src)
    while depth and k < n:
        ch = src[k]
        if instr:
            if esc:
                esc = False
            elif ch == BS:
                esc = True
            elif ch == "'":
                instr = False
        else:
            if ch == "'":
                instr = True
            elif ch == '(':
                depth += 1
            elif ch == ')':
                depth -= 1
        k += 1
    assert depth == 0, 'unbalanced'
    return k - 1

def parse_php_array(src, var):
    i = src.find('$this->' + var)
    assert i >= 0, var
    j = src.find('array (', i)
    assert j >= 0
    k = j + len('array (')
    end = find_array_end(src, k)
    return parse_array_body(src[k:end])

def split_top(s):
    parts, depth, cur, instr, esc = [], 0, '', False, False
    for ch in s:
        if instr:
            cur += ch
            if esc:
                esc = False
            elif ch == BS:
                esc = True
            elif ch == "'":
                instr = False
        else:
            if ch == "'":
                instr = True
                cur += ch
            elif ch == '(':
                depth += 1
                cur += ch
            elif ch == ')':
                depth -= 1
                cur += ch
            elif ch == ',' and depth == 0:
                parts.append(cur)
                cur = ''
            else:
                cur += ch
    if cur.strip():
        parts.append(cur)
    return parts

def parse_value(t):
    t = t.strip()
    if t.startswith('array'):
        j = t.find('(')
        end = find_array_end(t, j + 1)
        return parse_array_body(t[j + 1:end])
    if len(t) >= 2 and t.startswith("'") and t.endswith("'"):
        inner = t[1:-1]
        out, ii = [], 0
        while ii < len(inner):
            if inner[ii] == BS and ii + 1 < len(inner) and inner[ii + 1] in (BS, "'"):
                out.append(inner[ii + 1])
                ii += 2
            else:
                out.append(inner[ii])
                ii += 1
        return ''.join(out)
    if t == 'true':
        return True
    if t == 'false':
        return False
    if t == 'NULL':
        return None
    if re.fullmatch(r'-?\d+', t):
        return int(t)
    return t

def parse_array_body(body):
    items = [p for p in split_top(body) if p.strip()]
    if items and all('=>' in p for p in items):
        d = {}
        for p in items:
            k, v = p.split('=>', 1)
            k = parse_value(k)
            d[k] = parse_value(v)
        return d
    return [parse_value(p) for p in items]

if __name__ == '__main__':
    import argparse
    import os
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('lang')
    parser.add_argument('php_file')
    parser.add_argument('out_dir')
    args = parser.parse_args()

    src = open(args.php_file, encoding='utf-8', errors='replace').read()
    tables = {}
    for var in ['_regs', '_counts', '_states', '_delim', '_inner', '_end',
                '_keywords', '_parts', '_subst', '_kwmap', '_conditions',
                '_defClass']:
        if var == '_defClass':
            m = re.search(r"\$this->_defClass = '(.*?)';", src)
            tables[var] = m.group(1) if m else None
        else:
            tables[var] = parse_php_array(src, var)
    os.makedirs(args.out_dir, exist_ok=True)
    out = os.path.join(args.out_dir, 'tables_%s.json' % args.lang)
    json.dump(tables, open(out, 'w'))
    regs = tables['_regs']
    print(args.lang, 'states:', sorted(regs.keys(), key=str), '->', out)
