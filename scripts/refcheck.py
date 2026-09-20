#!/usr/bin/env python3
"""Check def/*.lfy for dangling [[references]], unresolved use paths and duplicate declarations.

A [[Name]] resolves when it is declared in the file, imported by a non-aliased use,
a parameter or loop variable in scope, or -- per def/model/main.lfy -- the single
`rule` entity with that identifier anywhere in the tree.
"""
import re, sys, pathlib, collections

ROOT = pathlib.Path(__file__).resolve().parent.parent
import json
MANIFEST = json.loads((ROOT / 'elfie.json').read_text())
PACKAGES = {name: ROOT / entry['root'] for name, entry in MANIFEST.get('dependencies', {}).items()}
FILES = sorted(p for d in [ROOT / 'def', *PACKAGES.values()] for p in d.rglob('*.lfy'))

DECL  = re.compile(r'^(?:ace\s+)?(?:d|trait|type|enum|fn|function|external|const|let)\s+(\w+)', re.M)
IS    = re.compile(r'^(?:d|external)\s+\w+\s+(?:is|extends)\s+([^:{\n]+)', re.M)
TEXT  = re.compile(r'^trait\s+(\w+)[^\n]*?\bextends\s+([^:{\n]+)', re.M)
USE   = re.compile(r'^use\s+"([^"]+)"(?:\s+as\s+(\w+))?\s*;', re.M)
REF   = re.compile(r'(?<!\\)\[\[(&?[A-Za-z_][^\]\n]*)\]\]')  # a reference starts with a name, so [[1, 2], x] in a test is not one
PARM  = re.compile(r'(?:fn|function|trait)\s+\w*\(([^)]*)\)|\(([^)]*)\)\s*=>')
LOOP  = re.compile(r'for\s*\(\s*(?:const|let)\s+(\w+)')

decls, uses, texts, applied = {}, {}, {}, {}
for f in FILES:
    t = f.read_text(); texts[f] = t
    decls[f] = set(DECL.findall(t))
    uses[f] = USE.findall(t)
    applied[f] = [set(re.findall(r'\w+', m)) for m in IS.findall(t)]

# traits that are rules: `rule` plus anything extending one, transitively
rulish = {'rule'}
for _ in range(6):
    for f in FILES:
        for name, ext in TEXT.findall(texts[f]):
            if rulish & set(re.findall(r'\w+', ext)):
                rulish.add(name)

rule_entities = collections.Counter()
for f in FILES:
    for name, ext in zip(DECL.findall(texts[f]), []): pass
    for m in re.finditer(r'^(?:d|external)\s+(\w+)\s+(?:is|extends)\s+([^:{\n]+)', texts[f], re.M):
        if rulish & set(re.findall(r'\w+', m.group(2))):
            rule_entities[m.group(1)] += 1

def resolve(f, p):
    if p.startswith('.'):
        base = (f.parent / p).resolve()
    else:
        head, _, rest = p.partition('/')
        if head not in PACKAGES: return None
        base = (PACKAGES[head] / rest).resolve() if rest else PACKAGES[head].resolve()
    for cand in (base.with_suffix('.lfy'), base / 'main.lfy'):
        if cand.exists(): return cand
    return None

bad_use, dangling, dupes, ambiguous, unused = [], [], [], [], []
for name, n in rule_entities.items():
    if n > 1: ambiguous.append(name)

for f in FILES:
    rel = f.relative_to(ROOT)
    counts = collections.Counter(DECL.findall(texts[f]))
    dupes += [(rel, n) for n, c in counts.items() if c > 1]
    visible, aliases = set(decls[f]) | {'global'}, set()
    for p, alias in uses[f]:
        tgt = resolve(f, p)
        if tgt is None: bad_use.append((rel, p)); continue
        body = USE.sub('', texts[f])
        if not DECL.search(texts[f]): continue   # a file that declares nothing only re-exports
        # an import is also used when it feeds @entities, or when it only applies components
        feeds = '@entities' in body or '.apply(' in texts[tgt]
        if alias:
            aliases.add(alias)
            if not feeds and not re.search(rf'\b{re.escape(alias)}\b', body): unused.append((rel, p))
        else:
            visible |= decls[tgt]
            if not feeds and not any(re.search(rf'\b{re.escape(n)}\b', body) for n in decls[tgt]):
                unused.append((rel, p))
    for a, b in PARM.findall(texts[f]):
        for part in (a or b).split(','):
            m = re.match(r'[\s(]*(?:\.\.\.)?(\w+)', part)
            if m: visible.add(m.group(1))
    visible |= set(LOOP.findall(texts[f]))
    for ref in REF.findall(texts[f]):
        head = re.split(r'[.@$]', ref.lstrip('&'))[0].strip()
        if not head or head in aliases or head in visible: continue
        if rule_entities.get(head) == 1: continue   # global rule fallback
        dangling.append((rel, ref))

fail = 0
for label, rows in (('unresolved use', bad_use), ('duplicate declaration', dupes),
                    ('ambiguous rule identifier', [(x, '') for x in ambiguous]),
                    ('dangling reference', dangling), ('unused use', unused)):
    print(f'== {label}: {len(rows)}')
    for a, b in rows: print(f'   {a}: {b}' if b else f'   {a}')
    fail += len(rows)
sys.exit(1 if fail else 0)
