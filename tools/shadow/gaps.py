#!/usr/bin/env python3
"""gaps: stubbed Minecraft/Forge/LWJGL methods that mods reference but Rust does not implement yet.

Usage: python -I tools/shadow/gaps.py REGISTERED.txt SCAN.json... [--json OUT.json]

REGISTERED.txt is `cargo run -p mc-forge --example registered`. Classes matched by keep-real.txt run vanilla Java and are
not gaps. A method counts as implemented when its exact key, `Class.*`, or (for SRG `func_` names, which are unique per
virtual method) any `*.func_x` / `Other.func_x` key is registered. Prints classes ranked by how many mods hit them.
"""
import collections, json, os, re, sys

STUB_ROOTS = ('net/minecraft/', 'net/minecraftforge/', 'org/lwjgl/')


def main():
    args = sys.argv[1:]
    out = None
    if '--json' in args:
        i = args.index('--json'); out = args[i + 1]; del args[i:i + 2]
    registered = set(l.strip() for l in open(args[0], encoding='utf-8') if l.strip())
    by_method = collections.defaultdict(set)
    for k in registered:
        cls, _, m = k.rpartition('.')
        by_method[m].add(cls)
    here = os.path.dirname(os.path.abspath(__file__))
    keep = [re.compile(l.strip()) for l in open(os.path.join(here, 'keep-real.txt'), encoding='utf-8')
            if l.strip() and not l.startswith('#')]
    keep.append(re.compile('fml/repackage/'))

    def real(owner):
        return any(k.search(owner + '.class') for k in keep)

    def done(owner, name):
        if (owner + '.' + name) in registered or (owner + '.*') in registered: return True
        return name.startswith('func_') and bool(by_method.get(name))

    gaps = collections.defaultdict(lambda: [set(), 0, None])  # (owner, name, desc) -> mods, sites, kind
    for path in args[1:]:
        scan = json.load(open(path, encoding='utf-8'))
        mod = os.path.basename(path)[:-5]
        for group in ('minecraft', 'forge', 'forge-fml', 'lwjgl'):
            for e in scan['external_refs'].get(group, []):
                if e['kind'] != 'method': continue
                owner = e['owner'].replace('.', '/')
                if not owner.startswith(STUB_ROOTS) or real(owner) or e['name'] == '<clinit>': continue
                if done(owner, e['name']): continue
                g = gaps[(owner, e['name'], e['desc'])]
                g[0].add(mod); g[1] += e.get('call_sites') or e.get('classes') or 0
    classes = collections.defaultdict(list)
    for (owner, name, desc), (mods, sites, _) in gaps.items():
        classes[owner].append((name, desc, len(mods), sites))
    ranked = sorted(classes.items(), key=lambda kv: (-max(m[2] for m in kv[1]), -len(kv[1])))
    print('%d missing methods in %d classes' % (len(gaps), len(classes)))
    for owner, methods in ranked[:80]:
        print('%3d mods %4d methods  %s' % (max(m[2] for m in methods), len(methods), owner))
    if out:
        json.dump({o: sorted(([n, d, k, s] for n, d, k, s in ms), key=lambda r: (-r[2], -r[3])) for o, ms in ranked},
                  open(out, 'w', encoding='utf-8'), indent=0)


if __name__ == '__main__':
    main()
