#!/usr/bin/env python3
"""Pack the icon masters for the Figma components `NF2 Icon/{12,16,20}/<name>`.

  python3 interface/design-system/tools/icon-masters/figma_pack.py <out dir> [chunk bytes] [name ...]

Writes chunk<N>.json ([grid, name, stroke path, fill path], compact SVG path syntax) and hashes.txt.
Each chunk is pasted into one Figma `use_figma` call as `const D=<chunk>;const H=<hash>;` followed by RUNNER
below. The runner verifies the checksum before touching anything, finds each component by name on the page
`v3 · Atoms` (386:36), replaces its children with the vectors of the master, keeps the component ID, binds
strokes and fills to `color/text/secondary` (VariableID:1:8692) and sets SCALE constraints. Give names to sync
only the icons that changed. `gradient-transform` travels as a whole SVG (it has a gradient fill).
`--runner` prints the runner.
"""
import re, glob, os, json, sys

RUNNER = r'''
const s=JSON.stringify(D);let h=0;for(let i=0;i<s.length;i++)h=(h+s.charCodeAt(i)*((i%251)+1))%1000000007;
if(h!==H)return{error:'checksum',h,len:s.length};
const page=await figma.getNodeByIdAsync('386:36');await figma.setCurrentPageAsync(page);
const ink=await figma.variables.getVariableByIdAsync('VariableID:1:8692');
const map={};for(const c of page.findAllWithCriteria({types:['COMPONENT']}))if(c.name.indexOf('NF2 Icon/')===0)map[c.name]=c;
const bind=(n)=>{if('strokes' in n&&n.strokes.length)n.strokes=n.strokes.map(p=>p.type==='SOLID'?figma.variables.setBoundVariableForPaint(p,'color',ink):p);if('fills' in n&&n.fills!==figma.mixed&&n.fills.length)n.fills=n.fills.map(p=>p.type==='SOLID'?figma.variables.setBoundVariableForPaint(p,'color',ink):p);if('children' in n)for(const k of n.children)bind(k);};
const ids=[],errors=[];
for(const [g,name,sd,fd] of D){const comp=map['NF2 Icon/'+g+'/'+name];if(!comp){errors.push('missing '+g+'/'+name);continue;}
const w=g===20?'1.5':'1';
const svg=sd==='RAW'?fd:'<svg width="'+g+'" height="'+g+'" viewBox="0 0 '+g+' '+g+'" fill="none" xmlns="http://www.w3.org/2000/svg">'+(sd?'<path d="'+sd+'" stroke="#9AA3B6" stroke-width="'+w+'" stroke-linecap="round" stroke-linejoin="round"/>':'')+(fd?'<path d="'+fd+'" fill="#9AA3B6"/>':'')+'</svg>';
const tmp=figma.createNodeFromSvg(svg);for(const k of [...comp.children])k.remove();
for(const k of [...tmp.children]){const x=k.x,y=k.y;comp.appendChild(k);k.x=x;k.y=y;if('constraints' in k)k.constraints={horizontal:'SCALE',vertical:'SCALE'};bind(k);}
tmp.remove();ids.push(comp.id);}
return{count:ids.length,errors,first:ids[0],last:ids[ids.length-1]};
'''

if '--runner' in sys.argv:
    print(RUNNER.strip())
    sys.exit(0)

HERE = os.path.dirname(os.path.abspath(__file__))
E = os.path.normpath(os.path.join(HERE, '..', '..', 'exports', 'icons'))
TOK = re.compile(r'[MLCZ]|-?\d*\.?\d+')
def num(v):
    s = ('%.3f' % float(v)).rstrip('0').rstrip('.')
    if s.startswith('0.'): s = s[1:]
    if s.startswith('-0.'): s = '-' + s[2:]
    return s or '0'
def compact(d):
    toks = TOK.findall(d); out = []; i = 0; cur = None; last = None
    def join(nums):
        s = ''
        for n in nums:
            t = num(n)
            if s and not t.startswith('-') and not (t.startswith('.') and '.' in s.split(' ')[-1] and False):
                s += ' '
            s += t
        return s
    while i < len(toks):
        c = toks[i]; i += 1
        if c == 'Z': out.append('Z'); last = 'Z'; continue
        n = {'M': 2, 'L': 2, 'C': 6}[c]
        a = [float(x) for x in toks[i:i + n]]; i += n
        if c == 'L' and cur and abs(a[1] - cur[1]) < 1e-9: cmd, args = 'H', [a[0]]
        elif c == 'L' and cur and abs(a[0] - cur[0]) < 1e-9: cmd, args = 'V', [a[1]]
        else: cmd, args = c, a
        out.append(cmd + join(args))
        cur = (a[-2], a[-1])
    return ''.join(out)
items = []
for g in (12, 16, 20):
    for f in sorted(glob.glob('%s/%d/*.svg' % (E, g))):
        s = open(f).read(); name = os.path.basename(f)[:-4]
        if name == 'gradient-transform':
            items.append([g, name, 'RAW', s.replace('\n', '')]); continue
        st = re.search(r'<path d="([^"]+)" stroke-width', s); fi = re.search(r'<path d="([^"]+)" fill=', s)
        items.append([g, name, compact(st.group(1)) if st else '', compact(fi.group(1)) if fi else ''])
out = sys.argv[1]
budget = int(sys.argv[2]) if len(sys.argv) > 2 else 13000
only = set(sys.argv[3:])
if only:
    items = [it for it in items if it[1] in only]
os.makedirs(out, exist_ok=True)
total = sum(len(json.dumps(x, separators=(',', ':'))) for x in items)
print('items', len(items), 'bytes', total)
chunks, cur, size = [], [], 0
for it in items:
    b = len(json.dumps(it, separators=(',', ':'))) + 1
    if size + b > budget: chunks.append(cur); cur, size = [], 0
    cur.append(it); size += b
chunks.append(cur)
with open(os.path.join(out, 'hashes.txt'), 'w') as hf:
    for i, c in enumerate(chunks):
        s = json.dumps(c, separators=(',', ':'))
        open(os.path.join(out, 'chunk%d.json' % i), 'w').write(s)
        h = 0
        for j, ch in enumerate(s):
            h = (h + ord(ch) * ((j % 251) + 1)) % 1000000007
        hf.write('chunk%d.json %d icons %d bytes hash %d\n' % (i, len(c), len(s), h))
        print(i, len(c), len(s), h)
