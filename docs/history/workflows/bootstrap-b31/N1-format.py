#!/usr/bin/env python3
"""Whitespace-only expansion of B3 VT; lexical tokens and comments stay intact.
Run with --baseline to independently reproduce the pre-symbol formatting evidence.
"""
import hashlib, json, re, subprocess, sys
from pathlib import Path
ROOT = Path(__file__).resolve().parents[3]
TOKEN = re.compile(r'//[^\n]*|/\*[\s\S]*?\*/|"(?:\\.|[^"\\])*"|[A-Za-z_][A-Za-z_0-9]*|[0-9][A-Za-z_0-9]*|>>=|<<=|::|->|=>|==|!=|<=|>=|&&|\|\||<<|>>|[+*%-]%|[^\s]')
def tokens(s): return TOKEN.findall(s)
def digest(s): return hashlib.sha256(s.encode()).hexdigest()
def generic_lines(s):
    # Result's type arguments are not record fields. Keep their commas inline.
    matches=list(TOKEN.finditer(s)); edits=[]; i=0
    while i<len(matches):
        if matches[i].group()!='Result': i+=1; continue
        start=i; i+=1
        if i<len(matches) and matches[i].group()=='::': i+=1
        if i>=len(matches) or matches[i].group()!='<': continue
        depth=1; i+=1
        while i<len(matches) and depth:
            t=matches[i].group()
            if t=='<': depth+=1
            if t=='>': depth-=1
            if t=='>>': depth-=2
            i+=1
        if depth==0:
            for n in range(start+1,i):
                a=matches[n-1].end(); b=matches[n].start()
                if '\n' in s[a:b]: edits.append((a,b,' '))
    for a,b,v in reversed(edits): s=s[:a]+v+s[b:]
    return s
def format_source(s):
    # Work on lexical units, keeping original intra-statement spacing. Newlines
    # occur at braces, statement terminators, and record/field separators.
    out=[]; line=''; stack=[]; depth=0; last=0
    def flush():
        nonlocal line
        if line.strip(): out.append('    '*depth+line.strip())
        line=''
    matches=list(TOKEN.finditer(s))
    for idx,m in enumerate(matches):
        t=m.group(); gap=s[last:m.start()]; last=m.end()
        if '\n' in gap: flush()
        elif gap and line: line+=' '
        if t.startswith('//') or t.startswith('/*'):
            flush(); line=t; flush(); continue
        if t == '{':
            line+=t; flush(); stack.append(t); depth+=1
        elif t == '}':
            flush(); depth-=1; assert stack.pop()=='{'; line=t
            nxt=matches[idx+1].group() if idx+1<len(matches) else ''
            if nxt not in [';',',',')',']','.','else']: flush()
        elif t in ['(','[']:
            stack.append(t); line+=t
        elif t in [')',']']:
            assert stack.pop()==('(' if t==')' else '['); line+=t
        elif t==';':
            line+=t
            if not stack or stack[-1]!='[': flush()
        elif t==',':
            line+=t
            if stack and stack[-1]=='{': flush()
            else: line+=' '
        else: line+=t
    flush(); result=generic_lines('\n'.join(out)+'\n')
    assert tokens(s)==tokens(result), 'Token sequence changed'
    return result
if __name__=='__main__':
    evidence={}
    for p in sorted((ROOT/'core-s/bootstrap').glob('*.vt')):
        rel=str(p.relative_to(ROOT))
        source=subprocess.check_output(['git','show','1c0f143:'+rel],cwd=ROOT,text=True) if '--baseline' in sys.argv else p.read_text()
        result=format_source(source)
        evidence[rel]={'before_sha256':digest(source),'formatted_sha256':digest(result),'token_sha256':digest(json.dumps(tokens(source),ensure_ascii=False)), 'token_count':len(tokens(source)), 'token_sequence_equal':tokens(source)==tokens(result),'before_lines':len(source.splitlines()),'formatted_lines':len(result.splitlines())}
        if '--baseline' not in sys.argv: p.write_text(result)
    print(json.dumps(evidence,indent=2))
